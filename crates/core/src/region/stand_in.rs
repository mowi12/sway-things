// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A stand-in for systemd-timedated, systemd-localed and geoclue, for tests: a private
//! `dbus-daemon` and, on it, a service that owns `org.freedesktop.timedate1` and answers as
//! timedated does, one that owns `org.freedesktop.locale1` and answers as localed does, and one
//! that owns `org.freedesktop.GeoClue2` and gives a location, as far as Sway Things uses them.
//! timedated and localed can refuse every change as polkit does when the user doesn't give the
//! password, geoclue can refuse Sway Things as when its config doesn't allow it. Built with the `test-support` feature; it
//! needs `dbus-daemon`.

use std::sync::{Arc, Mutex};

use zbus::Connection;
use zbus::object_server::SignalEmitter;

use super::localed::Localed;
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
    /// Whether geoclue gives Sway Things the location.
    pub geo_allowed: bool,
    /// Whether geoclue holds `Start` for an agent, as it does for an app its config doesn't name
    /// when there is no agent: until this is false again.
    pub geo_waits: bool,
    /// Where geoclue says the computer is: degrees north and east.
    pub place: (f64, f64),
    /// The app id and accuracy level the client was given.
    pub geo_client: (String, u32),
    /// localed's `Locale`: the system's variables.
    pub locale: Vec<String>,
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
            geo_allowed: true,
            geo_waits: false,
            // Tokyo.
            place: (35.68, 139.69),
            geo_client: (String::new(), 0),
            locale: vec!["LANG=en_US.UTF-8".to_owned()],
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

/// localed's bus name and object.
const LOCALE_SERVICE: &str = "org.freedesktop.locale1";
const LOCALE_PATH: &str = "/org/freedesktop/locale1";

struct Localed1 {
    shared: Shared,
}

#[zbus::interface(name = "org.freedesktop.locale1")]
impl Localed1 {
    /// Sets the locale as localed does: each entry a known category with a value, the categories
    /// left out unset.
    async fn set_locale(
        &self,
        locale: Vec<String>,
        interactive: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let _ = interactive;
        if lock(&self.shared).deny {
            return Err(zbus::fdo::Error::AccessDenied("Access denied".to_owned()));
        }
        for entry in &locale {
            let known = entry.split_once('=').is_some_and(|(name, value)| {
                !value.is_empty() && (name == "LANG" || name.starts_with("LC_"))
            });
            if !known {
                return Err(zbus::fdo::Error::InvalidArgs(format!(
                    "Locale assignment {entry} not valid, refusing."
                )));
            }
        }
        lock(&self.shared).locale = locale;
        let _ = self.locale_changed(&emitter).await;
        Ok(())
    }

    #[zbus(property)]
    fn locale(&self) -> Vec<String> {
        lock(&self.shared).locale.clone()
    }
}

/// geoclue's names and objects in the stand-in.
const GEO_SERVICE: &str = "org.freedesktop.GeoClue2";
const GEO_MANAGER: &str = "/org/freedesktop/GeoClue2/Manager";
const GEO_CLIENT: &str = "/org/freedesktop/GeoClue2/Client/1";
const GEO_LOCATION: &str = "/org/freedesktop/GeoClue2/Client/1/Location/1";

struct GeoManager;

#[zbus::interface(name = "org.freedesktop.GeoClue2.Manager")]
impl GeoManager {
    #[allow(clippy::unused_self, reason = "the method of geoclue's interface")]
    fn get_client(&self) -> zbus::zvariant::OwnedObjectPath {
        zbus::zvariant::OwnedObjectPath::try_from(GEO_CLIENT)
            .unwrap_or_else(|_| zbus::zvariant::OwnedObjectPath::default())
    }
}

struct GeoClient {
    shared: Shared,
}

#[zbus::interface(name = "org.freedesktop.GeoClue2.Client")]
impl GeoClient {
    /// Starts locating: refused unless geoclue allows the client, else the location follows.
    async fn start(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let (allowed, waits, id) = {
            let record = lock(&self.shared);
            (
                record.geo_allowed,
                record.geo_waits,
                record.geo_client.0.clone(),
            )
        };
        if waits {
            // Until a test lets go: calls to the stand-in wait behind this one.
            while lock(&self.shared).geo_waits {
                async_io::Timer::after(std::time::Duration::from_millis(50)).await;
            }
        }
        if !allowed {
            return Err(zbus::fdo::Error::AccessDenied(format!(
                "'{id}' disallowed, no agent for UID 1000"
            )));
        }
        let none = zbus::zvariant::ObjectPath::from_static_str_unchecked("/");
        let location = zbus::zvariant::ObjectPath::from_static_str_unchecked(GEO_LOCATION);
        let _ = Self::location_updated(&emitter, none, location).await;
        Ok(())
    }

    #[allow(clippy::unused_self, reason = "the method of geoclue's interface")]
    fn stop(&self) {}

    /// geoclue found a new location: the old one's object and the new one's.
    #[zbus(signal)]
    async fn location_updated(
        emitter: &SignalEmitter<'_>,
        old: zbus::zvariant::ObjectPath<'_>,
        new: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<()>;

    #[zbus(property)]
    fn desktop_id(&self) -> String {
        lock(&self.shared).geo_client.0.clone()
    }

    #[zbus(property)]
    fn set_desktop_id(&mut self, id: String) {
        lock(&self.shared).geo_client.0 = id;
    }

    #[zbus(property)]
    fn requested_accuracy_level(&self) -> u32 {
        lock(&self.shared).geo_client.1
    }

    #[zbus(property)]
    fn set_requested_accuracy_level(&mut self, level: u32) {
        lock(&self.shared).geo_client.1 = level;
    }
}

struct GeoLocation {
    shared: Shared,
}

#[zbus::interface(name = "org.freedesktop.GeoClue2.Location")]
impl GeoLocation {
    #[zbus(property)]
    fn latitude(&self) -> f64 {
        lock(&self.shared).place.0
    }

    #[zbus(property)]
    fn longitude(&self) -> f64 {
        lock(&self.shared).place.1
    }
}

/// A failure to start the stand-in.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// A private bus with the stand-ins on it. Stops them when dropped.
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
    /// Starts a bus and the stand-ins on it, with [`Record::default`].
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
            .serve_at(GEO_MANAGER, GeoManager)?
            .serve_at(
                GEO_CLIENT,
                GeoClient {
                    shared: shared.clone(),
                },
            )?
            .serve_at(
                GEO_LOCATION,
                GeoLocation {
                    shared: shared.clone(),
                },
            )?
            .serve_at(
                LOCALE_PATH,
                Localed1 {
                    shared: shared.clone(),
                },
            )?
            .name(SERVICE)?
            .name(LOCALE_SERVICE)?
            .name(GEO_SERVICE)?
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

    /// A connection to localed's stand-in, like [`Localed::connect`] makes to the system bus.
    ///
    /// # Errors
    ///
    /// If the bus refuses the connection.
    pub async fn localed(&self) -> Result<Localed, Error> {
        Ok(Localed::connect(&self.bus()).await?)
    }

    /// What the stand-in holds now.
    pub fn record(&self) -> Record {
        lock(&self.shared).clone()
    }

    /// Changes what the stand-in holds with `change`, without telling anyone.
    pub fn change(&self, change: impl FnOnce(&mut Record)) {
        change(&mut lock(&self.shared));
    }

    /// Stops the stand-ins, as on a system without timedated and localed: nobody owns their
    /// names any more.
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
