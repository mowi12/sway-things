// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Notifications through the desktop's notification server (`org.freedesktop.Notifications` on
//! the session bus: mako, swaync, dunst, …), with the picture and buttons.
//!
//! A notification with buttons needs someone to listen for the button pressed: the command line
//! that showed it waits for that, or for the notification to close, at most a few minutes, and
//! then ends. A server without actions (`GetCapabilities`) gets the notification without
//! buttons, and nobody waits.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use futures_lite::StreamExt as _;
use zbus::message::Type;
use zbus::zvariant::Value;
use zbus::{Connection, MatchRule, MessageStream};

/// The notification server's bus name, path and interface.
const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// The name notifications come from.
const APP_NAME: &str = "Sway Things";
/// The desktop entry of the app, so a server can show its icon.
const DESKTOP_ENTRY: &str = "io.github.mowi12.SwayThings";

/// A notification to show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notification {
    /// The title.
    pub summary: String,
    /// The text below it.
    pub body: String,
    /// A picture to show with it: the screenshot.
    pub image: Option<std::path::PathBuf>,
    /// The buttons: an id and a label each.
    pub actions: Vec<(String, String)>,
    /// How long it shows, in milliseconds; 0 until it is closed, -1 as long as the server likes.
    pub timeout: i32,
    /// The notification it takes the place of, by its id; 0 for a new one.
    pub replaces: u32,
}

/// `text` for the body of a notification, which servers may read as markup: `&`, `<` and `>`
/// escaped, so a path or a program's message shows as it is.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            c => escaped.push(c),
        }
    }
    escaped
}

/// A connection to the notification server.
#[derive(Debug, Clone)]
pub struct Notifier {
    connection: Connection,
}

/// Why a notification couldn't be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyError(pub String);

impl std::fmt::Display for NotifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotifyError {}

fn error(error: impl std::fmt::Display) -> NotifyError {
    NotifyError(error.to_string())
}

impl Notifier {
    /// Connects to the session bus.
    ///
    /// # Errors
    ///
    /// A [`NotifyError`] without a session bus.
    pub async fn connect() -> Result<Self, NotifyError> {
        Ok(Self {
            connection: Connection::session().await.map_err(error)?,
        })
    }

    /// Whether the server shows buttons (`actions` among its capabilities).
    pub async fn has_actions(&self) -> bool {
        let reply = self
            .connection
            .call_method(Some(SERVICE), PATH, Some(SERVICE), "GetCapabilities", &())
            .await;
        reply
            .ok()
            .and_then(|reply| reply.body().deserialize::<Vec<String>>().ok())
            .is_some_and(|capabilities| capabilities.iter().any(|c| c == "actions"))
    }

    /// Shows `notification` and returns its id.
    ///
    /// # Errors
    ///
    /// A [`NotifyError`] if no server shows it.
    pub async fn show(&self, notification: &Notification) -> Result<u32, NotifyError> {
        let actions: Vec<&str> = notification
            .actions
            .iter()
            .flat_map(|(id, label)| [id.as_str(), label.as_str()])
            .collect();
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        hints.insert("desktop-entry", Value::from(DESKTOP_ENTRY));
        let image = notification
            .image
            .as_deref()
            .and_then(Path::to_str)
            .map(str::to_owned);
        if let Some(image) = &image {
            hints.insert("image-path", Value::from(image.as_str()));
        }
        let reply = self
            .connection
            .call_method(
                Some(SERVICE),
                PATH,
                Some(SERVICE),
                "Notify",
                &(
                    APP_NAME,
                    notification.replaces,
                    "",
                    notification.summary.as_str(),
                    escape(&notification.body).as_str(),
                    actions,
                    hints,
                    notification.timeout,
                ),
            )
            .await
            .map_err(error)?;
        reply.body().deserialize::<u32>().map_err(error)
    }

    /// Closes the notification `id`.
    ///
    /// # Errors
    ///
    /// A [`NotifyError`] if the server can't be asked.
    pub async fn close(&self, id: u32) -> Result<(), NotifyError> {
        self.connection
            .call_method(
                Some(SERVICE),
                PATH,
                Some(SERVICE),
                "CloseNotification",
                &(id,),
            )
            .await
            .map(|_| ())
            .map_err(error)
    }

    /// Shows `notification` and waits until a button is pressed (its id comes back), it is
    /// closed or `limit` passed (`None`). Without buttons, or with a server that shows none, it
    /// doesn't wait.
    ///
    /// # Errors
    ///
    /// A [`NotifyError`] if no server shows it.
    pub async fn show_and_wait(
        &self,
        notification: &Notification,
        limit: Duration,
    ) -> Result<Option<String>, NotifyError> {
        if notification.actions.is_empty() || !self.has_actions().await {
            let plain = Notification {
                actions: Vec::new(),
                ..notification.clone()
            };
            self.show(&plain).await?;
            return Ok(None);
        }
        // Listening before it shows, so a quick press can't be missed.
        let rule = |member: &'static str| {
            MatchRule::builder()
                .msg_type(Type::Signal)
                .interface(SERVICE)
                .and_then(|rule| rule.member(member))
                .map(zbus::match_rule::Builder::build)
        };
        let mut invoked = MessageStream::for_match_rule(
            rule("ActionInvoked").map_err(error)?,
            &self.connection,
            None,
        )
        .await
        .map_err(error)?;
        let mut closed = MessageStream::for_match_rule(
            rule("NotificationClosed").map_err(error)?,
            &self.connection,
            None,
        )
        .await
        .map_err(error)?;
        let id = self.show(notification).await?;
        let wait = async {
            loop {
                let next = futures_lite::future::or(
                    async { invoked.next().await.map(|message| (true, message)) },
                    async { closed.next().await.map(|message| (false, message)) },
                )
                .await;
                let Some((action, Ok(message))) = next else {
                    return None;
                };
                if action {
                    if let Ok((of, key)) = message.body().deserialize::<(u32, String)>() {
                        if of == id {
                            return Some(key);
                        }
                    }
                } else if message
                    .body()
                    .deserialize::<(u32, u32)>()
                    .is_ok_and(|(of, _)| of == id)
                {
                    return None;
                }
            }
        };
        let timeout = async {
            async_io::Timer::after(limit).await;
            None
        };
        Ok(futures_lite::future::or(wait, timeout).await)
    }
}
