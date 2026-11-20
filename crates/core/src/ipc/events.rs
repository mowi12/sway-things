// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Events from sway.

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::types::{Event, EventType, WorkspaceChange};
use super::wire::{self, Connection, WireError};
use super::{Client, IpcError, Outcome};

impl Client {
    /// Subscribes to `events` on a connection of its own, so requests on this client aren't
    /// mixed with events.
    ///
    /// For [`EventType::Tick`], sway sends a first tick event right away (with `first` set), which
    /// confirms the subscription.
    ///
    /// # Errors
    ///
    /// [`IpcError::NotRunning`] if sway is gone, [`IpcError::Protocol`] if it refuses the
    /// subscription, and the errors of a request.
    pub async fn subscribe(&self, events: &[EventType]) -> Result<Events, IpcError> {
        let mut connection = match Connection::open(&self.socket).await {
            Ok(connection) => connection,
            Err(source) => {
                return Err(IpcError::NotRunning {
                    socket: self.socket.clone(),
                    source,
                });
            }
        };
        let payload = serde_json::to_vec(events).map_err(|source| IpcError::InvalidReply {
            request: "SUBSCRIBE",
            source,
        })?;
        let outcome: Outcome =
            super::query(&mut connection, wire::SUBSCRIBE, &payload, "SUBSCRIBE").await?;
        if !outcome.success {
            return Err(IpcError::Protocol("sway refused the subscription"));
        }
        Ok(Events {
            connection: Some(connection),
        })
    }
}

/// The events of a subscription, in the order sway sends them.
#[derive(Debug)]
pub struct Events {
    /// `None` once the connection ended.
    connection: Option<Connection>,
}

impl Events {
    /// Waits for the next event. `None` once sway closed the connection, which it does when it
    /// exits; a [`Event::Shutdown`] comes before that if it was subscribed to.
    ///
    /// # Errors
    ///
    /// [`IpcError::InvalidReply`] for an event that can't be read; the events after it still
    /// come. After [`IpcError::Io`] or [`IpcError::Protocol`] there are no more events.
    pub async fn next(&mut self) -> Option<Result<Event, IpcError>> {
        loop {
            let connection = self.connection.as_mut()?;
            let (kind, payload) = match connection.receive().await {
                Ok(message) => message,
                Err(error) => {
                    self.connection = None;
                    return match error {
                        WireError::Io(error)
                            if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                        {
                            None
                        }
                        error => Some(Err(error.into())),
                    };
                }
            };
            if kind & wire::EVENT == 0 {
                self.connection = None;
                return Some(Err(IpcError::Protocol(
                    "a reply arrived instead of an event",
                )));
            }
            // An event type this client doesn't know is skipped.
            if let Some(event) = decode(kind & !wire::EVENT, &payload) {
                return Some(event.map_err(|source| IpcError::InvalidReply {
                    request: "an event",
                    source,
                }));
            }
        }
    }
}

/// The reloads of sway's config, whoever asked for them: sway's `workspace` events with the
/// change `reload`.
#[derive(Debug)]
pub struct Reloads {
    events: Events,
}

impl Reloads {
    /// Subscribes to the `workspace` events of the session of `client`, on a connection of its
    /// own.
    ///
    /// # Errors
    ///
    /// The errors of [`Client::subscribe`].
    pub async fn start(client: &Client) -> Result<Self, IpcError> {
        let events = client.subscribe(&[EventType::Workspace]).await?;
        Ok(Self { events })
    }

    /// Waits until sway reloaded its config. `None` once sway closed the connection.
    ///
    /// # Errors
    ///
    /// Those of [`Events::next`].
    pub async fn next(&mut self) -> Option<Result<(), IpcError>> {
        loop {
            match self.events.next().await? {
                Ok(Event::Workspace(event)) if event.change == WorkspaceChange::Reload => {
                    return Some(Ok(()));
                }
                Ok(_) => {}
                Err(error) => return Some(Err(error)),
            }
        }
    }
}

/// Reads the event with the number `kind` (`man 7 sway-ipc`, "EVENTS").
fn decode(kind: u32, payload: &[u8]) -> Option<serde_json::Result<Event>> {
    fn read<'a, T: Deserialize<'a>>(payload: &'a [u8]) -> serde_json::Result<T> {
        serde_json::from_slice(payload)
    }
    fn boxed<T: DeserializeOwned>(payload: &[u8]) -> serde_json::Result<Box<T>> {
        read(payload).map(Box::new)
    }
    Some(match kind {
        0x00 => boxed(payload).map(Event::Workspace),
        0x01 => read(payload).map(Event::Output),
        0x02 => read(payload).map(Event::Mode),
        0x03 => boxed(payload).map(Event::Window),
        0x04 => boxed(payload).map(Event::BarConfigUpdate),
        0x05 => read(payload).map(Event::Binding),
        0x06 => read(payload).map(Event::Shutdown),
        0x07 => read(payload).map(Event::Tick),
        0x14 => read(payload).map(Event::BarStateUpdate),
        0x15 => boxed(payload).map(Event::Input),
        _ => return None,
    })
}
