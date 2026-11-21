// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The notification daemon on the session bus: who owns `org.freedesktop.Notifications`, and
//! what Do Not Disturb, a reload and a test notification mean for it.
//!
//! Every call goes to the unique name of the daemon that owns `org.freedesktop.Notifications`,
//! never to a well-known name: a call to a name nobody owns makes the bus start whichever daemon
//! has a D-Bus service file for it, and mako and swaync both have one. So asking never starts a
//! daemon, and a second daemon's interface can't answer for the one that shows notifications.
//!
//! - mako: `fr.emersion.Mako` at `/fr/emersion/Mako`, `ListModes`/`SetModes` (what
//!   `makoctl mode -a/-r` call), `Reload`, `ListNotifications`, `InvokeAction`.
//! - swaync: `org.erikreider.swaync.cc` at `/org/erikreider/swaync/cc`, `GetDnd`/`SetDnd` (what
//!   `swaync-client -D/-dn/-df` call), `ReloadConfig` and `ReloadCss` (`-R`, `-rs`),
//!   `SetVisibility`, and the signal `SubscribeV2` on each change.
//! - dunst: `org.dunstproject.cmd0` at `/org/freedesktop/Notifications`, the property `paused`
//!   (what `dunstctl set-paused`/`is-paused` use) and `ConfigReload`.

use std::collections::HashMap;

use futures_lite::{Stream, StreamExt};
use zbus::names::{BusName, OwnedUniqueName};
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, MessageStream};

use super::Daemon;
use crate::i18n::Text;
use crate::msg;

/// The well-known name of the notification server.
pub const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const MAKO_PATH: &str = "/fr/emersion/Mako";
const MAKO: &str = "fr.emersion.Mako";
const SWAYNC_PATH: &str = "/org/erikreider/swaync/cc";
const SWAYNC: &str = "org.erikreider.swaync.cc";
const DUNST: &str = "org.dunstproject.cmd0";
/// mako's mode for Do Not Disturb, which our `[mode=do-not-disturb]` section hides.
pub const MAKO_DND_MODE: &str = "do-not-disturb";

/// What the daemon says about itself (`GetServerInformation`), and who it is on the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// Which daemon this is.
    pub daemon: Daemon,
    /// Its name as it gives it (`mako`, `SwayNotificationCenter`, `dunst`, …).
    pub name: String,
    /// Its version as it gives it; mako always says `0.0.0`.
    pub version: String,
    /// Its unique name on the bus.
    pub owner: OwnedUniqueName,
    /// Its process, if the bus says.
    pub pid: Option<u32>,
}

/// Why the daemon couldn't be asked or changed.
#[derive(Debug)]
pub enum Error {
    /// The bus or the daemon refused.
    Bus(zbus::Error),
    /// The daemon has no Do Not Disturb Sway Things knows (not mako, swaync or dunst).
    Unsupported(String),
}

impl Error {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Bus(error) => Text::new(msg!("the notification daemon didn't answer: {error}"))
                .arg("error", error),
            Self::Unsupported(name) => Text::new(msg!(
                "{daemon} has no Do Not Disturb that Sway Things can switch"
            ))
            .arg("daemon", name),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for Error {}

impl From<zbus::Error> for Error {
    fn from(error: zbus::Error) -> Self {
        Self::Bus(error)
    }
}

impl From<zbus::fdo::Error> for Error {
    fn from(error: zbus::fdo::Error) -> Self {
        Self::Bus(error.into())
    }
}

/// A handle on the session bus for the notification daemon.
#[derive(Debug, Clone)]
pub struct Notifications {
    connection: Connection,
}

impl Notifications {
    /// On the session bus.
    ///
    /// # Errors
    ///
    /// If there is no session bus.
    pub async fn session() -> Result<Self, Error> {
        Ok(Self::new(Connection::session().await?))
    }

    /// On `connection`.
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// The connection.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    async fn proxy(
        &self,
        server: &Server,
        path: &'static str,
        interface: &'static str,
    ) -> Result<zbus::Proxy<'static>, Error> {
        Ok(zbus::proxy::Builder::new(&self.connection)
            .destination(BusName::from(server.owner.clone()))?
            .path(path)?
            .interface(interface)?
            .cache_properties(CacheProperties::No)
            .build()
            .await?)
    }

    /// The daemon that shows notifications now, or `None` if nobody owns
    /// `org.freedesktop.Notifications`. Starts nothing.
    ///
    /// # Errors
    ///
    /// If the bus or the daemon doesn't answer.
    pub async fn server(&self) -> Result<Option<Server>, Error> {
        let dbus = zbus::fdo::DBusProxy::new(&self.connection).await?;
        let name = BusName::from_static_str(NAME).map_err(zbus::Error::from)?;
        let owner = match dbus.get_name_owner(name.clone()).await {
            Ok(owner) => owner,
            Err(zbus::fdo::Error::NameHasNoOwner(_)) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let pid = dbus
            .get_connection_unix_process_id(BusName::from(owner.clone()))
            .await
            .ok();
        let mut server = Server {
            daemon: Daemon::Other,
            name: String::new(),
            version: String::new(),
            owner,
            pid,
        };
        let proxy = self.proxy(&server, PATH, NAME).await?;
        let (name, _vendor, version, _spec): (String, String, String, String) =
            proxy.call("GetServerInformation", &()).await?;
        server.daemon = Daemon::from_server_name(&name);
        server.name = clean(&name);
        server.version = clean(&version);
        Ok(Some(server))
    }

    /// Whether Do Not Disturb is on.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] for a daemon other than mako, swaync and dunst, else if it doesn't
    /// answer.
    pub async fn dnd(&self, server: &Server) -> Result<bool, Error> {
        match server.daemon {
            Daemon::Mako => Ok(self
                .mako_modes(server)
                .await?
                .iter()
                .any(|mode| mode == MAKO_DND_MODE)),
            Daemon::Swaync => Ok(self
                .proxy(server, SWAYNC_PATH, SWAYNC)
                .await?
                .call("GetDnd", &())
                .await?),
            Daemon::Dunst => Ok(self
                .proxy(server, PATH, DUNST)
                .await?
                .get_property("paused")
                .await?),
            Daemon::Other => Err(Error::Unsupported(server.name.clone())),
        }
    }

    /// Switches Do Not Disturb on or off and returns what the daemon reports afterwards.
    ///
    /// # Errors
    ///
    /// Like [`Notifications::dnd`].
    pub async fn set_dnd(&self, server: &Server, on: bool) -> Result<bool, Error> {
        match server.daemon {
            Daemon::Mako => {
                let mut modes = self.mako_modes(server).await?;
                // `makoctl mode -a` adds a mode a second time; one is enough.
                modes.retain(|mode| mode != MAKO_DND_MODE);
                if on {
                    modes.push(MAKO_DND_MODE.to_owned());
                }
                let proxy = self.proxy(server, MAKO_PATH, MAKO).await?;
                proxy.call::<_, _, ()>("SetModes", &(modes,)).await?;
            }
            Daemon::Swaync => {
                let proxy = self.proxy(server, SWAYNC_PATH, SWAYNC).await?;
                proxy.call::<_, _, ()>("SetDnd", &(on,)).await?;
            }
            Daemon::Dunst => {
                let proxy = self.proxy(server, PATH, DUNST).await?;
                proxy.set_property("paused", on).await?;
            }
            Daemon::Other => return Err(Error::Unsupported(server.name.clone())),
        }
        self.dnd(server).await
    }

    async fn mako_modes(&self, server: &Server) -> Result<Vec<String>, Error> {
        Ok(self
            .proxy(server, MAKO_PATH, MAKO)
            .await?
            .call("ListModes", &())
            .await?)
    }

    /// Has the daemon read its config again: mako's `Reload`, swaync's `ReloadConfig` and
    /// `ReloadCss`, dunst's `ConfigReload`. Nothing for another daemon.
    ///
    /// # Errors
    ///
    /// If the daemon refuses, e.g. mako over a config it can't read.
    pub async fn reload(&self, server: &Server) -> Result<(), Error> {
        match server.daemon {
            Daemon::Mako => {
                let proxy = self.proxy(server, MAKO_PATH, MAKO).await?;
                proxy.call::<_, _, ()>("Reload", &()).await?;
            }
            Daemon::Swaync => {
                let proxy = self.proxy(server, SWAYNC_PATH, SWAYNC).await?;
                proxy.call::<_, _, ()>("ReloadConfig", &()).await?;
                proxy.call::<_, _, bool>("ReloadCss", &()).await?;
            }
            Daemon::Dunst => {
                let proxy = self.proxy(server, PATH, DUNST).await?;
                proxy
                    .call::<_, _, ()>("ConfigReload", &(Vec::<String>::new(),))
                    .await?;
            }
            Daemon::Other => {}
        }
        Ok(())
    }

    /// Sends a notification through the daemon that runs, as Sway Things, and returns its id.
    ///
    /// # Errors
    ///
    /// If the daemon refuses it.
    pub async fn notify(&self, server: &Server, summary: &str, body: &str) -> Result<u32, Error> {
        let proxy = self.proxy(server, PATH, NAME).await?;
        let hints: HashMap<&str, zbus::zvariant::Value<'_>> =
            HashMap::from([("desktop-entry", "io.github.mowi12.SwayThings".into())]);
        Ok(proxy
            .call(
                "Notify",
                &(
                    "Sway Things",
                    0_u32,
                    "io.github.mowi12.SwayThings",
                    summary,
                    body,
                    Vec::<&str>::new(),
                    hints,
                    -1_i32,
                ),
            )
            .await?)
    }

    /// Closes swaync's panel, if it is open. Nothing for another daemon.
    ///
    /// # Errors
    ///
    /// If swaync doesn't answer.
    pub async fn close_panel(&self, server: &Server) -> Result<(), Error> {
        if server.daemon == Daemon::Swaync {
            let proxy = self.proxy(server, SWAYNC_PATH, SWAYNC).await?;
            proxy.call::<_, _, ()>("SetVisibility", &(false,)).await?;
        }
        Ok(())
    }

    /// mako's notification `id`: its app name and desktop entry, as the app sent them.
    ///
    /// # Errors
    ///
    /// If mako doesn't answer.
    pub async fn mako_notification(
        &self,
        server: &Server,
        id: u32,
    ) -> Result<Option<(String, String)>, Error> {
        let proxy = self.proxy(server, MAKO_PATH, MAKO).await?;
        let list: Vec<HashMap<String, OwnedValue>> = proxy.call("ListNotifications", &()).await?;
        let text = |entry: &HashMap<String, OwnedValue>, key: &str| {
            entry
                .get(key)
                .and_then(|value| String::try_from(value.clone()).ok())
                .unwrap_or_default()
        };
        Ok(list
            .iter()
            .find(|entry| {
                entry
                    .get("id")
                    .and_then(|value| u32::try_from(value.clone()).ok())
                    == Some(id)
            })
            .map(|entry| (text(entry, "app-name"), text(entry, "desktop-entry"))))
    }

    /// What clicking mako's notification `id` would have done: invokes its default action, so
    /// the app opens what it is about, and dismisses it.
    ///
    /// # Errors
    ///
    /// If mako doesn't answer.
    pub async fn mako_click(&self, server: &Server, id: u32) -> Result<(), Error> {
        let proxy = self.proxy(server, MAKO_PATH, MAKO).await?;
        proxy
            .call::<_, _, ()>("InvokeAction", &(id, "default"))
            .await?;
        let dismiss: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::from([("id", id.into())]);
        // mako 1.10 and newer; 1.9 has `DismissNotification(ub)`.
        if proxy
            .call::<_, _, ()>("DismissNotifications", &(dismiss,))
            .await
            .is_err()
        {
            proxy
                .call::<_, _, ()>("DismissNotification", &(id, true))
                .await?;
        }
        Ok(())
    }

    /// A stream that ticks when the daemon may have changed: another one took
    /// `org.freedesktop.Notifications`, mako's modes or dunst's `paused` changed, or swaync
    /// reported a change. Read the state again on each tick.
    ///
    /// # Errors
    ///
    /// If the bus refuses the match rules.
    pub async fn changes(&self) -> Result<impl Stream<Item = ()> + Unpin + use<>, Error> {
        let owner = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg(0, NAME)?
            .build();
        let properties = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus.Properties")?
            .member("PropertiesChanged")?
            .build();
        let swaync = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(SWAYNC)?
            .member("SubscribeV2")?
            .build();
        let mut streams = Vec::new();
        for rule in [owner, properties, swaync] {
            streams.push(MessageStream::for_match_rule(rule, &self.connection, Some(8)).await?);
        }
        let mut merged: std::pin::Pin<Box<dyn Stream<Item = ()>>> =
            Box::pin(futures_lite::stream::empty());
        for stream in streams {
            merged = Box::pin(merged.or(stream.filter_map(|message| {
                let message = message.ok()?;
                let header = message.header();
                // Only the properties of mako and dunst matter, not those of every service.
                if header
                    .member()
                    .is_some_and(|member| member == "PropertiesChanged")
                {
                    let (changed, _, _): (String, HashMap<String, OwnedValue>, Vec<String>) =
                        message.body().deserialize().ok()?;
                    if changed != MAKO && changed != DUNST {
                        return None;
                    }
                }
                Some(())
            })));
        }
        Ok(merged)
    }
}

/// `text` without control characters: what a daemon reports is shown as it is otherwise.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}
