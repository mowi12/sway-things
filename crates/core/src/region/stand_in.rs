// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A stand-in for systemd-timedated, for tests: a private `dbus-daemon` and, on it, a service that
//! owns `org.freedesktop.timedate1` and answers as timedated does, as far as Sway Things uses it.
//! It can refuse every change as polkit does when the user doesn't give the password. Built with
//! the `test-support` feature; it needs `dbus-daemon`.

use std::sync::{Arc, Mutex};

use zbus::Connection;
use zbus::object_server::SignalEmitter;

use super::{Bus, Timedate};

/// timedated's bus name and object.
const SERVICE: &str = "org.freedesktop.timedate1";
const PATH: &str = "/org/freedesktop/timedate1";

/// What the stand-in holds and was asked, for the tests to set and check.
#[allow(
    clippy::struct_excessive_bools,
    reason = "timedated's properties, each a flag of its own"
)]
#[derive(Debug, Clone)]
pub struct Record {
    /// The time zone.
    pub timezone: String,
    /// Whether automatic time is on.
    pub ntp: bool,
    /// Whether an NTP service is installed.
    pub can_ntp: bool,
    /// Whether the clock is synchronized.
    pub synchronized: bool,
    /// The zones `ListTimezones` gives.
    pub zones: Vec<String>,
    /// Whether polkit refuses every change.
    pub deny: bool,
    /// The times `SetTime` was called with, in microseconds.
    pub times: Vec<i64>,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            timezone: "Europe/Berlin".to_owned(),
            ntp: true,
            can_ntp: true,
            synchronized: true,
            zones: ["America/New_York", "Asia/Tokyo", "Europe/Berlin", "UTC"]
                .map(str::to_owned)
                .to_vec(),
            deny: false,
            times: Vec::new(),
        }
    }
}

type Shared = Arc<Mutex<Record>>;

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Record> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Timedated {
    shared: Shared,
}

impl Timedated {
    fn refuse(&self) -> zbus::fdo::Result<()> {
        if lock(&self.shared).deny {
            Err(zbus::fdo::Error::AccessDenied("Access denied".to_owned()))
        } else {
            Ok(())
        }
    }
}

#[zbus::interface(name = "org.freedesktop.timedate1")]
impl Timedated {
    fn list_timezones(&self) -> Vec<String> {
        lock(&self.shared).zones.clone()
    }

    async fn set_timezone(
        &self,
        zone: String,
        interactive: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let _ = interactive;
        self.refuse()?;
        if !lock(&self.shared).zones.contains(&zone) {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "Invalid or not installed time zone '{zone}'"
            )));
        }
        lock(&self.shared).timezone = zone;
        let _ = self.timezone_changed(&emitter).await;
        Ok(())
    }

    #[zbus(name = "SetNTP")]
    async fn set_ntp(
        &self,
        on: bool,
        interactive: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let _ = interactive;
        self.refuse()?;
        if !lock(&self.shared).can_ntp {
            return Err(zbus::fdo::Error::Failed("NTP not supported".to_owned()));
        }
        lock(&self.shared).ntp = on;
        let _ = self.n_t_p_changed(&emitter).await;
        Ok(())
    }

    fn set_time(&self, micros: i64, relative: bool, interactive: bool) -> zbus::fdo::Result<()> {
        let _ = (relative, interactive);
        self.refuse()?;
        let mut record = lock(&self.shared);
        if record.ntp {
            return Err(zbus::fdo::Error::Failed(
                "Automatic time synchronization is enabled".to_owned(),
            ));
        }
        record.times.push(micros);
        Ok(())
    }

    #[zbus(property)]
    fn timezone(&self) -> String {
        lock(&self.shared).timezone.clone()
    }

    #[zbus(property, name = "NTP")]
    fn ntp(&self) -> bool {
        lock(&self.shared).ntp
    }

    #[zbus(property, name = "CanNTP")]
    fn can_ntp(&self) -> bool {
        lock(&self.shared).can_ntp
    }

    #[zbus(property, name = "NTPSynchronized")]
    fn ntp_synchronized(&self) -> bool {
        lock(&self.shared).synchronized
    }
}

/// A failure to start the stand-in.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// A private bus with timedated's stand-in on it. Stops both when dropped.
pub struct StandIn {
    bus: crate::dbus::TestBus,
    server: Option<Connection>,
    shared: Shared,
}

impl std::fmt::Debug for StandIn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StandIn")
            .field("address", &self.bus.address)
            .finish_non_exhaustive()
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = self.bus.daemon.kill();
        let _ = self.bus.daemon.wait();
    }
}

impl StandIn {
    /// Starts a bus and timedated's stand-in on it, with [`Record::default`].
    ///
    /// # Errors
    ///
    /// If `dbus-daemon` can't be started or the name can't be taken.
    pub async fn start() -> Result<Self, Error> {
        let bus = crate::dbus::TestBus::start().await?;
        let shared = Shared::default();
        let server = zbus::connection::Builder::address(bus.address.as_str())?
            .serve_at(
                PATH,
                Timedated {
                    shared: shared.clone(),
                },
            )?
            .name(SERVICE)?
            .build()
            .await?;
        Ok(Self {
            bus,
            server: Some(server),
            shared,
        })
    }

    /// The address of the bus.
    pub fn address(&self) -> &str {
        &self.bus.address
    }

    /// Where the code under test finds the stand-in.
    pub fn bus(&self) -> Bus {
        Bus::Address(self.bus.address.clone())
    }

    /// A connection to the stand-in, like [`Timedate::connect`] makes to the system bus.
    ///
    /// # Errors
    ///
    /// If the bus refuses the connection.
    pub async fn timedate(&self) -> Result<Timedate, Error> {
        Ok(Timedate::connect(&self.bus()).await?)
    }

    /// What the stand-in holds now.
    pub fn record(&self) -> Record {
        lock(&self.shared).clone()
    }

    /// Changes what the stand-in holds with `change`, without telling anyone.
    pub fn change(&self, change: impl FnOnce(&mut Record)) {
        change(&mut lock(&self.shared));
    }

    /// Stops timedated's stand-in, as on a system without it: nobody owns its name any more.
    pub fn stop(&mut self) {
        self.server = None;
    }

    /// Sets the time zone as `timedatectl` would elsewhere, and reports the change.
    ///
    /// # Errors
    ///
    /// If the change can't be signaled, or the stand-in was stopped.
    pub async fn set_timezone_elsewhere(&self, zone: &str) -> Result<(), Error> {
        zone.clone_into(&mut lock(&self.shared).timezone);
        let server = self.server.as_ref().ok_or("the stand-in was stopped")?;
        let interface = server
            .object_server()
            .interface::<_, Timedated>(PATH)
            .await?;
        interface
            .get()
            .await
            .timezone_changed(interface.signal_emitter())
            .await?;
        Ok(())
    }
}
