// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The wire format of sway's IPC (`man 7 sway-ipc`): every message is the magic string
//! `i3-ipc`, the payload length and the message type as 32-bit integers in the byte order of the
//! machine, and the payload.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use async_io::{Async, Timer};
use futures_lite::{AsyncReadExt, AsyncWriteExt};

const MAGIC: &[u8; 6] = b"i3-ipc";
const HEADER_LENGTH: usize = MAGIC.len() + 8;

/// A reply longer than this is a broken message, not a big tree: nothing is allocated for it.
const MAX_PAYLOAD: u32 = 64 * 1024 * 1024;

/// How often and how long apart a connection is tried while sway's queue is full: 2 seconds.
const CONNECT_ATTEMPTS: u32 = 200;
const CONNECT_PAUSE: Duration = Duration::from_millis(10);

/// The message types, which a reply repeats.
pub(super) const RUN_COMMAND: u32 = 0;
pub(super) const SUBSCRIBE: u32 = 2;
pub(super) const GET_OUTPUTS: u32 = 3;
pub(super) const GET_TREE: u32 = 4;
pub(super) const GET_VERSION: u32 = 7;
pub(super) const GET_BINDING_MODES: u32 = 8;
pub(super) const GET_CONFIG: u32 = 9;
pub(super) const SEND_TICK: u32 = 10;
pub(super) const GET_BINDING_STATE: u32 = 12;
pub(super) const GET_INPUTS: u32 = 100;
pub(super) const GET_SEATS: u32 = 101;

/// Set in the type of a message that is an event, not a reply.
pub(super) const EVENT: u32 = 1 << 31;

/// A connection to the IPC socket.
#[derive(Debug)]
pub(super) struct Connection {
    stream: Async<UnixStream>,
}

impl Connection {
    /// Connects to `socket`.
    ///
    /// sway queues only a few connections it hasn't accepted yet. While that queue is full, a
    /// non-blocking connect ends with a socket that isn't connected, so the connection is checked
    /// and tried again for a short while.
    pub(super) async fn open(socket: &Path) -> io::Result<Self> {
        for _ in 0..CONNECT_ATTEMPTS {
            let attempt = async {
                let stream = Async::<UnixStream>::connect(socket).await?;
                stream.get_ref().peer_addr()?;
                io::Result::Ok(stream)
            };
            match attempt.await {
                Ok(stream) => return Ok(Self { stream }),
                Err(error) if error.kind() == io::ErrorKind::NotConnected => {
                    Timer::after(CONNECT_PAUSE).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "sway doesn't accept the connection",
        ))
    }

    pub(super) async fn send(&mut self, kind: u32, payload: &[u8]) -> Result<(), WireError> {
        let length = u32::try_from(payload.len()).map_err(|_| WireError::TooLong)?;
        let mut message = Vec::with_capacity(HEADER_LENGTH + payload.len());
        message.extend_from_slice(MAGIC);
        message.extend_from_slice(&length.to_ne_bytes());
        message.extend_from_slice(&kind.to_ne_bytes());
        message.extend_from_slice(payload);
        self.stream.write_all(&message).await?;
        Ok(())
    }

    /// The next message: its type and payload.
    pub(super) async fn receive(&mut self) -> Result<(u32, Vec<u8>), WireError> {
        let mut header = [0; HEADER_LENGTH];
        self.stream.read_exact(&mut header).await?;
        let (magic, numbers) = header.split_at(MAGIC.len());
        if magic != MAGIC {
            return Err(WireError::NotIpc);
        }
        let (length, kind) = numbers.split_at(4);
        let number = |bytes: &[u8]| bytes.try_into().map(u32::from_ne_bytes);
        let (Ok(length), Ok(kind)) = (number(length), number(kind)) else {
            return Err(WireError::NotIpc);
        };
        if length > MAX_PAYLOAD {
            return Err(WireError::TooLong);
        }
        let mut payload = vec![0; length as usize];
        self.stream.read_exact(&mut payload).await?;
        Ok((kind, payload))
    }

    /// Sends a request and returns the payload of its reply.
    pub(super) async fn request(
        &mut self,
        kind: u32,
        payload: &[u8],
    ) -> Result<Vec<u8>, WireError> {
        self.send(kind, payload).await?;
        let (reply, payload) = self.receive().await?;
        if reply == kind {
            Ok(payload)
        } else {
            Err(WireError::UnexpectedReply)
        }
    }
}

/// Why a message couldn't be exchanged. The connection is out of step afterwards.
#[derive(Debug)]
pub(super) enum WireError {
    Io(io::Error),
    /// A message doesn't start with the magic string.
    NotIpc,
    /// A payload is longer than the format or this client allows.
    TooLong,
    /// The reply has another type than the request.
    UnexpectedReply,
}

impl From<io::Error> for WireError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
