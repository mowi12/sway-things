// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A stand-in for the notification daemons, for tests: a private `dbus-daemon` and, on it, a
//! daemon that owns `org.freedesktop.Notifications` and speaks like mako (`fr.emersion.Mako`),
//! swaync (`org.erikreider.swaync.cc`) or dunst (`org.dunstproject.cmd0`), as far as Sway Things
//! uses them. Built with the `test-support` feature; it needs `dbus-daemon`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use tempfile::TempDir;
use zbus::Connection;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

use super::Daemon;

/// What the stand-in daemon was asked, for the tests to check.
#[derive(Debug, Default)]
pub struct Record {
    /// The notifications sent to it: app name, summary, body.
    pub notified: Vec<(String, String, String)>,
    /// How often it was asked to reload.
    pub reloads: u32,
    /// Whether swaync's panel was closed.
    pub panel_closed: bool,
    /// mako's actions invoked: id and action.
    pub invoked: Vec<(u32, String)>,
}

type Shared = Arc<Mutex<Record>>;

fn record(shared: &Shared) -> std::sync::MutexGuard<'_, Record> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Server {
    name: &'static str,
    shared: Shared,
}

#[allow(
    clippy::unused_self,
    reason = "the methods of the interfaces, as the daemons have them"
)]
#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_server_information(&self) -> (String, String, String, String) {
        (
            self.name.into(),
            "stand-in".into(),
            "1.0".into(),
            "1.2".into(),
        )
    }

    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".into()]
    }

    #[allow(clippy::too_many_arguments, reason = "the method of the specification")]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let _ = (replaces_id, app_icon, actions, hints, expire_timeout);
        let mut record = record(&self.shared);
        record.notified.push((app_name, summary, body));
        u32::try_from(record.notified.len()).unwrap_or(u32::MAX)
    }
}

struct Mako {
    modes: Vec<String>,
    shared: Shared,
}

#[allow(
    clippy::unused_self,
    reason = "the methods of the interfaces, as the daemons have them"
)]
#[zbus::interface(name = "fr.emersion.Mako")]
impl Mako {
    fn list_modes(&self) -> Vec<String> {
        self.modes.clone()
    }

    async fn set_modes(
        &mut self,
        modes: Vec<String>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        self.modes = modes;
        let _ = self.modes_changed(&emitter).await;
    }

    #[zbus(property)]
    fn modes(&self) -> Vec<String> {
        self.modes.clone()
    }

    fn reload(&self) {
        record(&self.shared).reloads += 1;
    }

    fn list_notifications(&self) -> Vec<HashMap<String, OwnedValue>> {
        let entry = |id: u32, app: &str, entry: &str| {
            HashMap::from([
                ("id".to_owned(), OwnedValue::from(id)),
                (
                    "app-name".to_owned(),
                    OwnedValue::from(zbus::zvariant::Str::from(app.to_owned())),
                ),
                (
                    "desktop-entry".to_owned(),
                    OwnedValue::from(zbus::zvariant::Str::from(entry.to_owned())),
                ),
            ])
        };
        vec![entry(7, "Signal", "signal-desktop")]
    }

    fn invoke_action(&self, id: u32, action: String) {
        record(&self.shared).invoked.push((id, action));
    }

    fn dismiss_notifications(&self, options: HashMap<String, OwnedValue>) {
        drop(options);
    }
}

struct Swaync {
    dnd: bool,
    shared: Shared,
}

#[allow(
    clippy::unused_self,
    reason = "the methods of the interfaces, as the daemons have them"
)]
#[zbus::interface(name = "org.erikreider.swaync.cc")]
impl Swaync {
    fn get_dnd(&self) -> bool {
        self.dnd
    }

    async fn set_dnd(&mut self, state: bool, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        self.dnd = state;
        let _ = Self::subscribe_v2(&emitter, 0, state, false, false).await;
    }

    fn reload_config(&self) {
        record(&self.shared).reloads += 1;
    }

    fn reload_css(&self) -> bool {
        true
    }

    fn set_visibility(&self, visibility: bool) {
        if !visibility {
            record(&self.shared).panel_closed = true;
        }
    }

    /// What swaync sends on every change.
    #[zbus(signal)]
    async fn subscribe_v2(
        emitter: &SignalEmitter<'_>,
        count: u32,
        dnd: bool,
        cc_open: bool,
        inhibited: bool,
    ) -> zbus::Result<()>;
}

struct Dunst {
    paused: bool,
    shared: Shared,
}

#[allow(
    clippy::unused_self,
    reason = "the methods of the interfaces, as the daemons have them"
)]
#[zbus::interface(name = "org.dunstproject.cmd0")]
impl Dunst {
    #[zbus(property, name = "paused")]
    fn paused(&self) -> bool {
        self.paused
    }

    #[zbus(property, name = "paused")]
    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    fn config_reload(&self, configs: Vec<String>) {
        drop(configs);
        record(&self.shared).reloads += 1;
    }
}

/// The private bus with a stand-in daemon on it.
#[derive(Debug)]
pub struct StandIn {
    address: String,
    daemon: Child,
    server: Option<Connection>,
    shared: Shared,
    _dir: TempDir,
}

/// A failure to start the stand-in.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

impl StandIn {
    /// Starts a private bus without a notification daemon.
    ///
    /// # Errors
    ///
    /// If `dbus-daemon` can't be started.
    pub fn start() -> Result<Self, Error> {
        let dir = tempfile::tempdir()?;
        let socket = dir.path().join("bus");
        let mut daemon = Command::new("dbus-daemon")
            .arg("--session")
            .arg("--nofork")
            .arg("--nopidfile")
            .arg(format!("--address=unix:path={}", socket.display()))
            .arg("--print-address")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("can't start dbus-daemon: {error}"))?;
        let mut address = String::new();
        if let Some(stdout) = daemon.stdout.take() {
            BufReader::new(stdout).read_line(&mut address)?;
        }
        let address = address.trim().to_owned();
        if address.is_empty() {
            let _ = daemon.kill();
            return Err("dbus-daemon printed no address".into());
        }
        Ok(Self {
            address,
            daemon,
            server: None,
            shared: Shared::default(),
            _dir: dir,
        })
    }

    /// The bus address.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// A new connection to the bus, for the code under test.
    ///
    /// # Errors
    ///
    /// If the bus can't be reached.
    pub async fn connect(&self) -> Result<Connection, Error> {
        Ok(zbus::connection::Builder::address(self.address.as_str())?
            .build()
            .await?)
    }

    /// Runs a stand-in for `daemon` (anything but [`Daemon::Other`] answers like that daemon;
    /// [`Daemon::Other`] only has the notification interface) with Do Not Disturb as `dnd`.
    /// One that runs already goes away first.
    ///
    /// # Errors
    ///
    /// If it can't take the name.
    pub async fn run(&mut self, daemon: Daemon, dnd: bool) -> Result<(), Error> {
        self.stop().await;
        let name = match daemon {
            Daemon::Mako => "mako",
            Daemon::Swaync => "SwayNotificationCenter",
            Daemon::Dunst => "dunst",
            Daemon::Other => "fnott",
        };
        let shared = self.shared.clone();
        let mut builder = zbus::connection::Builder::address(self.address.as_str())?.serve_at(
            "/org/freedesktop/Notifications",
            Server {
                name,
                shared: shared.clone(),
            },
        )?;
        builder = match daemon {
            Daemon::Mako => builder.serve_at(
                "/fr/emersion/Mako",
                Mako {
                    modes: if dnd {
                        vec!["default".into(), "do-not-disturb".into()]
                    } else {
                        vec!["default".into()]
                    },
                    shared,
                },
            )?,
            Daemon::Swaync => {
                builder.serve_at("/org/erikreider/swaync/cc", Swaync { dnd, shared })?
            }
            Daemon::Dunst => builder.serve_at(
                "/org/freedesktop/Notifications",
                Dunst {
                    paused: dnd,
                    shared,
                },
            )?,
            Daemon::Other => builder,
        };
        let connection = builder.name(super::bus::NAME)?.build().await?;
        self.server = Some(connection);
        Ok(())
    }

    /// Ends the stand-in daemon: it gives up the name and its connection closes.
    pub async fn stop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.release_name(super::bus::NAME).await;
            let _ = server.close().await;
        }
    }

    /// What the daemon was asked so far.
    pub fn record(&self) -> std::sync::MutexGuard<'_, Record> {
        record(&self.shared)
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.server = None;
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}
