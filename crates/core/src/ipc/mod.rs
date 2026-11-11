// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The connection to the running sway or SwayFX over its IPC socket (`man 7 sway-ipc`).
//!
//! A [`Client`] runs the commands built with the [command builders](crate::command) and reads the
//! state of the session. It is async and needs no particular executor: the app awaits it on the
//! glib main loop, the command line with `async_io::block_on`.
//!
//! ```no_run
//! use sway_things_core::command::behavior;
//! use sway_things_core::ipc::Client;
//! use sway_things_core::ipc::types::{Event, EventType};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut sway = Client::connect().await?;
//! println!("connected to {}", sway.compositor());
//! sway.run(&behavior::tiling_drag(true)).await?;
//! for input in sway.inputs().await? {
//!     println!("{}", input.identifier);
//! }
//!
//! // Hotplugged devices arrive as events.
//! let mut events = sway.subscribe(&[EventType::Input]).await?;
//! while let Some(event) = events.next().await {
//!     if let Event::Input(event) = event? {
//!         println!("{:?}: {}", event.change, event.input.identifier);
//!     }
//! }
//! # Ok(())
//! # }
//! ```

mod compositor;
mod events;
#[cfg(test)]
pub(crate) mod tests;
mod wire;

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::command::{CommandError, SwayCmd, Target};
use compositor::VersionReply;
use wire::{Connection, WireError};

pub use compositor::{Compositor, Unmet};
pub use events::Events;
/// The types of sway's replies, from the `swayipc-types` crate.
pub use swayipc_types as types;

/// The reply to `GET_VERSION`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// sway or SwayFX, with the version.
    pub compositor: Compositor,
    /// The version as the compositor prints it, e.g. with the commit of a development build.
    pub human_readable: String,
    /// The path of the config file that is loaded.
    pub loaded_config_file_name: String,
}

/// A connection to the running compositor.
///
/// After an [`IpcError::Io`] or [`IpcError::Protocol`] the connection is out of step with sway
/// and has to be replaced by a new one.
#[derive(Debug)]
pub struct Client {
    connection: Connection,
    socket: PathBuf,
    compositor: Compositor,
}

impl Client {
    /// Connects to the compositor of this session, through the socket in `$SWAYSOCK`.
    ///
    /// # Errors
    ///
    /// [`IpcError::NoSession`] if `SWAYSOCK` isn't set, so this isn't a sway session; otherwise
    /// like [`Client::connect_to`].
    pub async fn connect() -> Result<Self, IpcError> {
        Self::connect_to(socket_from(std::env::var_os("SWAYSOCK"))?).await
    }

    /// Connects to the compositor that listens on `socket` and reads which one it is.
    ///
    /// # Errors
    ///
    /// [`IpcError::NotRunning`] if nothing listens on the socket, [`IpcError::Unsupported`] if
    /// the compositor is older than Sway Things supports, and the errors of a request.
    pub async fn connect_to(socket: impl Into<PathBuf>) -> Result<Self, IpcError> {
        let socket = socket.into();
        let mut connection = match Connection::open(&socket).await {
            Ok(connection) => connection,
            Err(source) => {
                return Err(match source.kind() {
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                        IpcError::NotRunning { socket, source }
                    }
                    _ => IpcError::Io(source),
                });
            }
        };
        let compositor = version(&mut connection).await?.compositor;
        compositor
            .check_supported()
            .map_err(|unmet| IpcError::Unsupported { compositor, unmet })?;
        Ok(Self {
            connection,
            socket,
            compositor,
        })
    }

    /// Which compositor this is, as detected when connecting.
    pub fn compositor(&self) -> Compositor {
        self.compositor
    }

    /// The path of the IPC socket.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Runs `command`.
    ///
    /// # Errors
    ///
    /// [`IpcError::Render`] if the command can't be sent over IPC,
    /// [`IpcError::CommandUnsupported`] if it needs a newer sway or SwayFX than the running one
    /// (it isn't sent then: sway doesn't reject every value it doesn't know),
    /// [`IpcError::CommandFailed`] with sway's message if sway rejects it, and the errors of a
    /// request.
    pub async fn run(&mut self, command: &SwayCmd) -> Result<(), IpcError> {
        let rendered = command.render(Target::Ipc).map_err(IpcError::Render)?;
        if let Err(unmet) = self.compositor.supports(command) {
            return Err(IpcError::CommandUnsupported {
                command: rendered,
                unmet,
            });
        }
        // One command per message, so the results belong to this command alone.
        let outcomes: Vec<Outcome> = self
            .query(wire::RUN_COMMAND, rendered.as_bytes(), "RUN_COMMAND")
            .await?;
        if outcomes.is_empty() {
            return Err(IpcError::Protocol("sway sent no result for a command"));
        }
        match outcomes.into_iter().find(|outcome| !outcome.success) {
            None => Ok(()),
            Some(failed) => Err(IpcError::CommandFailed {
                command: rendered,
                message: failed.error.unwrap_or_default(),
                parse_error: failed.parse_error,
            }),
        }
    }

    /// Runs `commands` in order, also after one failed, and returns the result of each.
    pub async fn run_all(&mut self, commands: &[SwayCmd]) -> Vec<Result<(), IpcError>> {
        let mut results = Vec::with_capacity(commands.len());
        for command in commands {
            results.push(self.run(command).await);
        }
        results
    }

    /// `GET_VERSION`: the compositor and its loaded config file.
    ///
    /// # Errors
    ///
    /// This and the other requests fail with [`IpcError::Io`] if the connection breaks,
    /// [`IpcError::Protocol`] if sway's answer isn't an IPC reply to the request, and
    /// [`IpcError::InvalidReply`] if the reply can't be read.
    pub async fn version(&mut self) -> Result<Version, IpcError> {
        version(&mut self.connection).await
    }

    /// `GET_INPUTS`: the input devices with their libinput settings.
    ///
    /// # Errors
    ///
    /// See [`Client::version`].
    pub async fn inputs(&mut self) -> Result<Vec<types::Input>, IpcError> {
        self.query(wire::GET_INPUTS, b"", "GET_INPUTS").await
    }

    /// `GET_OUTPUTS`: the outputs with their modes.
    ///
    /// # Errors
    ///
    /// See [`Client::version`].
    pub async fn outputs(&mut self) -> Result<Vec<types::Output>, IpcError> {
        self.query(wire::GET_OUTPUTS, b"", "GET_OUTPUTS").await
    }

    /// `GET_SEATS`: the seats with their devices.
    ///
    /// # Errors
    ///
    /// See [`Client::version`].
    pub async fn seats(&mut self) -> Result<Vec<types::Seat>, IpcError> {
        self.query(wire::GET_SEATS, b"", "GET_SEATS").await
    }

    /// `GET_BINDING_MODES`: the names of the binding modes.
    ///
    /// # Errors
    ///
    /// See [`Client::version`].
    pub async fn binding_modes(&mut self) -> Result<Vec<String>, IpcError> {
        self.query(wire::GET_BINDING_MODES, b"", "GET_BINDING_MODES")
            .await
    }

    /// `GET_BINDING_STATE`: the name of the active binding mode.
    ///
    /// # Errors
    ///
    /// See [`Client::version`].
    pub async fn binding_state(&mut self) -> Result<String, IpcError> {
        #[derive(Deserialize)]
        struct BindingState {
            name: String,
        }
        let state: BindingState = self
            .query(wire::GET_BINDING_STATE, b"", "GET_BINDING_STATE")
            .await?;
        Ok(state.name)
    }

    /// `GET_TREE`: the tree of outputs, workspaces and windows.
    ///
    /// # Errors
    ///
    /// See [`Client::version`].
    pub async fn tree(&mut self) -> Result<types::Node, IpcError> {
        self.query(wire::GET_TREE, b"", "GET_TREE").await
    }

    /// `GET_CONFIG`: the text of the main config file as sway loaded it, without the files it
    /// includes.
    ///
    /// # Errors
    ///
    /// See [`Client::version`].
    pub async fn config(&mut self) -> Result<String, IpcError> {
        #[derive(Deserialize)]
        struct Config {
            config: String,
        }
        let config: Config = self.query(wire::GET_CONFIG, b"", "GET_CONFIG").await?;
        Ok(config.config)
    }

    /// `SEND_TICK`: sends a tick event with `payload` to every client that subscribed to them.
    ///
    /// # Errors
    ///
    /// See [`Client::version`]; [`IpcError::Protocol`] if sway refuses.
    pub async fn send_tick(&mut self, payload: &str) -> Result<(), IpcError> {
        let outcome: Outcome = self
            .query(wire::SEND_TICK, payload.as_bytes(), "SEND_TICK")
            .await?;
        if outcome.success {
            Ok(())
        } else {
            Err(IpcError::Protocol("sway refused to send a tick"))
        }
    }

    async fn query<T: DeserializeOwned>(
        &mut self,
        kind: u32,
        payload: &[u8],
        request: &'static str,
    ) -> Result<T, IpcError> {
        query(&mut self.connection, kind, payload, request).await
    }
}

async fn query<T: DeserializeOwned>(
    connection: &mut Connection,
    kind: u32,
    payload: &[u8],
    request: &'static str,
) -> Result<T, IpcError> {
    let reply = connection.request(kind, payload).await?;
    serde_json::from_slice(&reply).map_err(|source| IpcError::InvalidReply { request, source })
}

async fn version(connection: &mut Connection) -> Result<Version, IpcError> {
    let reply: VersionReply = query(connection, wire::GET_VERSION, b"", "GET_VERSION").await?;
    let compositor = Compositor::from_reply(&reply)
        .map_err(|_| IpcError::Protocol("sway sent a version that isn't one"))?;
    Ok(Version {
        compositor,
        human_readable: reply.human_readable,
        loaded_config_file_name: reply.loaded_config_file_name,
    })
}

/// The socket path from the value of `$SWAYSOCK`.
fn socket_from(swaysock: Option<OsString>) -> Result<PathBuf, IpcError> {
    swaysock
        .filter(|socket| !socket.is_empty())
        .map(PathBuf::from)
        .ok_or(IpcError::NoSession)
}

/// The result of a command, or of a request that only succeeds or fails.
#[derive(Deserialize)]
struct Outcome {
    success: bool,
    #[serde(default)]
    parse_error: bool,
    error: Option<String>,
}

/// Why sway couldn't be reached or a request failed.
#[derive(Debug)]
pub enum IpcError {
    /// `SWAYSOCK` isn't set: this isn't a sway session.
    NoSession,
    /// Nothing listens on the socket: sway isn't running, or `SWAYSOCK` is left over from a
    /// session that ended.
    NotRunning {
        /// The socket.
        socket: PathBuf,
        /// The error of the operating system.
        source: io::Error,
    },
    /// The compositor is older than Sway Things supports.
    Unsupported {
        /// What is running.
        compositor: Compositor,
        /// What Sway Things needs.
        unmet: Unmet,
    },
    /// The connection broke.
    Io(io::Error),
    /// The other end doesn't speak sway's IPC as expected.
    Protocol(&'static str),
    /// A reply can't be read.
    InvalidReply {
        /// The request, e.g. `GET_INPUTS`.
        request: &'static str,
        /// What the parser found.
        source: serde_json::Error,
    },
    /// The command can't be sent over IPC.
    Render(CommandError),
    /// The command needs a newer sway or SwayFX than the running one. It wasn't sent.
    CommandUnsupported {
        /// The command as it would have been sent.
        command: String,
        /// What it needs.
        unmet: Unmet,
    },
    /// sway rejected the command.
    CommandFailed {
        /// The command as sent.
        command: String,
        /// sway's error message.
        message: String,
        /// Whether sway couldn't parse the command, rather than failing to run it.
        parse_error: bool,
    },
}

impl From<WireError> for IpcError {
    fn from(error: WireError) -> Self {
        match error {
            WireError::Io(error) => Self::Io(error),
            WireError::NotIpc => Self::Protocol("the answer isn't an IPC message"),
            WireError::TooLong => Self::Protocol("a message is too long"),
            WireError::UnexpectedReply => Self::Protocol("the reply doesn't match the request"),
        }
    }
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSession => f.write_str("SWAYSOCK is not set: not in a sway session"),
            Self::NotRunning { socket, .. } => {
                write!(f, "sway is not running (no answer on {})", socket.display())
            }
            Self::Unsupported { compositor, unmet } => {
                write!(f, "{compositor} is not supported: Sway Things {unmet}")
            }
            Self::Io(error) => write!(f, "the connection to sway failed: {error}"),
            Self::Protocol(problem) => write!(f, "unexpected answer from sway: {problem}"),
            Self::InvalidReply { request, source } => {
                write!(f, "can't read sway's reply to {request}: {source}")
            }
            Self::Render(error) => error.fmt(f),
            Self::CommandUnsupported { command, unmet } => write!(f, "`{command}` {unmet}"),
            Self::CommandFailed {
                command, message, ..
            } => write!(f, "sway rejected `{command}`: {message}"),
        }
    }
}

impl std::error::Error for IpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotRunning { source, .. } | Self::Io(source) => Some(source),
            Self::InvalidReply { source, .. } => Some(source),
            Self::Render(error) => Some(error),
            Self::Unsupported { unmet, .. } | Self::CommandUnsupported { unmet, .. } => Some(unmet),
            Self::NoSession | Self::Protocol(_) | Self::CommandFailed { .. } => None,
        }
    }
}
