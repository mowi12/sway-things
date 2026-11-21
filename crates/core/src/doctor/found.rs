// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The compositor the checkup looks at: the running one, or the installed `sway`.

use std::fmt;

use crate::generate::{SwayValidator, WriteError};
use crate::i18n::Text;
use crate::include::MainConfig;
use crate::ipc::{Client, Compositor, IpcError};
use crate::msg;

/// The installed `sway`, as `sway --version` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    /// This compositor.
    Version(Compositor),
    /// `sway` runs, but doesn't print a version of sway or SwayFX that Sway Things can read.
    Unknown,
    /// `sway` can't be run, with the error.
    Missing(Text),
}

impl Installed {
    /// Runs `sway --version` through `validator`.
    pub async fn of(validator: &SwayValidator) -> Self {
        match validator.compositor().await {
            Ok(Some(compositor)) => Self::Version(compositor),
            Ok(None) => Self::Unknown,
            Err(error) => Self::Missing(Text::data(error.to_string())),
        }
    }
}

/// The compositor the checkup is for: the one in this session, or the installed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// A supported sway or SwayFX runs in this session.
    Session(Compositor),
    /// No session runs.
    NoSession {
        /// Why, e.g. `SWAYSOCK isn't set`.
        reason: Text,
        /// The installed compositor.
        installed: Installed,
    },
    /// A session runs, but Sway Things can't use it: the compositor is too old, or it doesn't
    /// answer like sway does.
    Unusable {
        /// What went wrong.
        error: Text,
        /// The installed compositor.
        installed: Installed,
    },
}

impl Found {
    /// Looks for the session in `$SWAYSOCK`, and without a usable one asks the installed `sway`
    /// through `validator`. Also returns the main config: the one the running sway loaded, or
    /// the one sway would find.
    pub async fn detect(validator: &SwayValidator) -> (Self, MainConfig) {
        let error = match Client::connect().await {
            Ok(mut client) => {
                let main = match client.version().await {
                    Ok(version) if !version.loaded_config_file_name.is_empty() => {
                        MainConfig::Path(version.loaded_config_file_name.into())
                    }
                    _ => MainConfig::Default,
                };
                return (Self::Session(client.compositor()), main);
            }
            Err(error) => error,
        };
        let installed = Installed::of(validator).await;
        let found = match error {
            IpcError::NoSession => Self::NoSession {
                reason: msg!("SWAYSOCK isn't set").into(),
                installed,
            },
            IpcError::NotRunning { socket, .. } => Self::NoSession {
                reason: Text::new(msg!("nothing answers on {socket}"))
                    .arg("socket", socket.display()),
                installed,
            },
            error => Self::Unusable {
                error: error.text(),
                installed,
            },
        };
        (found, MainConfig::Default)
    }

    /// The compositor to generate for: the running one, else the installed one. `None` if
    /// neither is known.
    pub fn compositor(&self) -> Option<Compositor> {
        match self {
            Self::Session(compositor) => Some(*compositor),
            Self::NoSession { installed, .. } | Self::Unusable { installed, .. } => match installed
            {
                Installed::Version(compositor) => Some(*compositor),
                Installed::Unknown | Installed::Missing(_) => None,
            },
        }
    }

    /// Why `sway` can't be run, when no session runs. In a session, it is tried anyway.
    pub(super) fn sway_missing(&self) -> Option<&Text> {
        match self {
            Self::NoSession {
                installed: Installed::Missing(error),
                ..
            } => Some(error),
            _ => None,
        }
    }
}

/// Why [`Checkup::fix`] couldn't write the generated files.
#[derive(Debug)]
pub enum FixError {
    /// Neither a session nor `sway --version` says which compositor to generate for.
    NoCompositor,
    /// A file can't be written, or sway can't be run to validate it.
    Write(WriteError),
}

impl FixError {
    /// Why nothing was written, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::NoCompositor => msg!(
                "it isn't known which compositor to generate for: no sway session runs, and \
                 `sway --version` names none"
            )
            .into(),
            Self::Write(error) => error.text(),
        }
    }
}

impl fmt::Display for FixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for FixError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoCompositor => None,
            Self::Write(error) => Some(error),
        }
    }
}
