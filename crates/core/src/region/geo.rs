// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The automatic time zone: where the computer is, from geoclue (`org.freedesktop.GeoClue2` on
//! the system bus) at city level, and the zone of the nearest city tzdata lists.
//!
//! geoclue locates the computer through a location service on the network (beaconDB by default,
//! or what the distribution configured), which sees the computer's IP address and, as geoclue is
//! set up, the Wi-Fi networks nearby. Sway Things only asks geoclue, and only while
//! `region.automatic_timezone` is on, which it is not by default. geoclue answers apps its
//! config allows, or ones an agent approves; sway has no agent, so the user allows Sway Things
//! once in a drop-in of geoclue's config ([`DROP_IN`]), which is root's file.
//!
//! ```
//! use sway_things_core::region::geo;
//!
//! let tab = "DE\t+5230+01322\tEurope/Berlin\nFR\t+4852+00220\tEurope/Paris\n";
//! assert_eq!(geo::nearest_in(tab, 52.4, 13.0).as_deref(), Some("Europe/Berlin"));
//! assert_eq!(geo::nearest_in(tab, 45.76, 4.84).as_deref(), Some("Europe/Paris"));
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use futures_lite::{StreamExt, future};
use zbus::message::Type;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream};

use super::{Bus, clean};
use crate::dbus;
use crate::i18n::Text;
use crate::msg;

/// geoclue's bus name.
const SERVICE: &str = "org.freedesktop.GeoClue2";
/// Its manager object and interface.
const MANAGER_PATH: &str = "/org/freedesktop/GeoClue2/Manager";
const MANAGER: &str = "org.freedesktop.GeoClue2.Manager";
/// The interface of a client and of a location.
const CLIENT: &str = "org.freedesktop.GeoClue2.Client";
const LOCATION: &str = "org.freedesktop.GeoClue2.Location";
/// The standard interface for properties.
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// geoclue's accuracy level for a city (`GCLUE_ACCURACY_LEVEL_CITY`): enough for a time zone.
const CITY: u32 = 4;
/// How long to wait for a location.
const TIMEOUT: Duration = Duration::from_secs(30);
/// How long `Start` may take. geoclue answers it at once for an app its config allows; for any
/// other app it holds the call until an agent approves, and sway has none, so geoclue waits
/// until it quits for being idle.
const START_TIMEOUT: Duration = Duration::from_secs(5);
/// The error the bus gives a call whose service quit before answering.
const NO_REPLY: &str = "org.freedesktop.DBus.Error.NoReply";

/// The app id Sway Things gives geoclue, which its config names to allow it.
pub const DESKTOP_ID: &str = "io.github.mowi12.SwayThings";

/// Where the drop-in that allows Sway Things goes, and what it says: created by the user, since
/// geoclue's config is root's.
pub const DROP_IN: &str = "/etc/geoclue/conf.d/90-sway-things.conf";

/// The text of [`DROP_IN`].
pub fn drop_in_text() -> String {
    format!("[{DESKTOP_ID}]\nallowed=true\nsystem=false\nusers=\n")
}

/// The command that creates [`DROP_IN`], to copy into a terminal; only ever shown.
pub fn drop_in_command() -> String {
    format!(
        "printf '{}' | sudo tee {DROP_IN}",
        drop_in_text().replace('\n', "\\n")
    )
}

/// Where the computer is: degrees north and east.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    /// Degrees north (negative for south).
    pub latitude: f64,
    /// Degrees east (negative for west).
    pub longitude: f64,
}

/// Why the location isn't known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// geoclue isn't installed or there is no system bus.
    Unavailable,
    /// geoclue doesn't let Sway Things know: its config doesn't allow it, and no agent asked (or
    /// geoclue waits for one that never comes).
    NotAllowed,
    /// geoclue found no location in time, or failed: its message (untrusted, as data), or what
    /// Sway Things makes of it.
    Failed(Text),
}

impl Error {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Unavailable => {
                msg!("The location isn't available: geoclue isn't installed or doesn't run").into()
            }
            Self::NotAllowed => Text::new(msg!(
                "geoclue doesn't let Sway Things know the location. Allow it once with the file \
                 {file}, which the command below creates (it asks for your password)."
            ))
            .arg("file", DROP_IN),
            Self::Failed(reason) => {
                Text::new(msg!("The location isn't known: {reason}")).text("reason", reason.clone())
            }
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for Error {}

/// Asks geoclue on `bus` where the computer is, at city level, waiting up to 30 seconds.
///
/// # Errors
///
/// [`Error::Unavailable`] without geoclue, [`Error::NotAllowed`] if it refuses Sway Things or
/// waits for an agent to allow it, [`Error::Failed`] if it finds nothing in time.
pub async fn locate(bus: &Bus) -> Result<Place, Error> {
    let connection = match bus {
        Bus::System => Connection::system().await.map_err(|_| Error::Unavailable)?,
        Bus::Address(address) => zbus::connection::Builder::address(address.as_str())
            .map_err(|_| Error::Unavailable)?
            .build()
            .await
            .map_err(|_| Error::Unavailable)?,
        Bus::Off => return Err(Error::Unavailable),
    };
    let reply = connection
        .call_method(Some(SERVICE), MANAGER_PATH, Some(MANAGER), "GetClient", &())
        .await
        .map_err(failure)?;
    let client: OwnedObjectPath = reply.body().deserialize().map_err(failure)?;
    let set = |name: &'static str, value: Value<'static>| {
        let connection = connection.clone();
        let client = client.clone();
        async move {
            connection
                .call_method(
                    Some(SERVICE),
                    client.as_str(),
                    Some(PROPERTIES),
                    "Set",
                    &(CLIENT, name, value),
                )
                .await
                .map(drop)
                .map_err(failure)
        }
    };
    set("DesktopId", Value::from(DESKTOP_ID)).await?;
    set("RequestedAccuracyLevel", Value::from(CITY)).await?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(CLIENT)
        .and_then(|rule| rule.member("LocationUpdated"))
        .and_then(|rule| rule.path(client.as_str()))
        .map_err(failure)?
        .build();
    let mut updates = MessageStream::for_match_rule(rule, &connection, None)
        .await
        .map_err(failure)?;
    let start = async {
        connection
            .call_method(Some(SERVICE), client.as_str(), Some(CLIENT), "Start", &())
            .await
            .map(drop)
            .map_err(|error| match error {
                // geoclue quit while it held `Start` for an agent.
                zbus::Error::MethodError(name, _, _) if name.as_str() == NO_REPLY => {
                    Error::NotAllowed
                }
                other => failure(other),
            })
    };
    let waiting = async {
        async_io::Timer::after(START_TIMEOUT).await;
        Err(Error::NotAllowed)
    };
    future::or(start, waiting).await?;
    let wait = async {
        while let Some(message) = updates.next().await {
            let Ok(message) = message else { continue };
            if let Ok((_, new)) = message
                .body()
                .deserialize::<(OwnedObjectPath, OwnedObjectPath)>()
            {
                return place(&connection, &new).await;
            }
        }
        Err(Error::Failed(msg!("geoclue went away").into()))
    };
    let timeout = async {
        async_io::Timer::after(TIMEOUT).await;
        Err(Error::Failed(
            msg!("geoclue found no location within 30 seconds").into(),
        ))
    };
    let found = future::or(wait, timeout).await;
    let _ = connection
        .call_method(Some(SERVICE), client.as_str(), Some(CLIENT), "Stop", &())
        .await;
    found
}

/// The latitude and longitude of geoclue's location object at `path`.
async fn place(connection: &Connection, path: &OwnedObjectPath) -> Result<Place, Error> {
    let reply = connection
        .call_method(
            Some(SERVICE),
            path.as_str(),
            Some(PROPERTIES),
            "GetAll",
            &(LOCATION,),
        )
        .await
        .map_err(failure)?;
    let properties: HashMap<String, OwnedValue> = reply.body().deserialize().map_err(failure)?;
    let degrees = |name: &str| {
        properties
            .get(name)
            .and_then(|value| f64::try_from(value).ok())
            .filter(|value| value.is_finite())
    };
    match (degrees("Latitude"), degrees("Longitude")) {
        (Some(latitude), Some(longitude)) => Ok(Place {
            latitude,
            longitude,
        }),
        _ => Err(Error::Failed(msg!("geoclue gave no coordinates").into())),
    }
}

/// The error of a failed call.
fn failure(error: zbus::Error) -> Error {
    match error {
        zbus::Error::MethodError(name, message, _) => {
            let name = name.as_str();
            if dbus::is_gone(name) {
                Error::Unavailable
            } else if name == "org.freedesktop.DBus.Error.AccessDenied" {
                Error::NotAllowed
            } else {
                let message = clean(&message.unwrap_or_default());
                Error::Failed(Text::data(if message.is_empty() {
                    dbus::short_name(name).to_owned()
                } else {
                    message
                }))
            }
        }
        other => Error::Failed(Text::data(other.to_string())),
    }
}

/// What the automatic time zone did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detected {
    /// The zone found is the one the system has: nothing changed.
    Same(String),
    /// The system's zone was set to the one found.
    Set(String),
}

/// Why the automatic time zone changed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectError {
    /// The location isn't known.
    Location(Error),
    /// tzdata lists no city to take the zone from.
    NoZone,
    /// timedated didn't take the zone found (polkit refused it, say): the error and the zone,
    /// for the command that sets it in a terminal.
    Clock(super::Error, String),
}

impl DetectError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Location(error) => error.text(),
            Self::NoZone => msg!("tzdata lists no time zone for the location").into(),
            Self::Clock(error, _) => error.text(),
        }
    }

    /// The command that does in a terminal what failed, to copy: the drop-in that allows Sway
    /// Things, or setting the zone found.
    pub fn command(&self) -> Option<String> {
        match self {
            Self::Location(Error::NotAllowed) => Some(drop_in_command()),
            Self::Clock(super::Error::Denied(_), zone) => {
                Some(super::Change::TimeZone(zone.clone()).command())
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for DetectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for DetectError {}

/// Finds the zone where the computer is ([`locate`] on `bus`, [`nearest`]) and sets it with
/// `timedate` if the system has another one, which may make polkit ask for the password.
///
/// # Errors
///
/// See [`DetectError`].
pub async fn detect(bus: &Bus, timedate: &super::Timedate) -> Result<Detected, DetectError> {
    let place = locate(bus).await.map_err(DetectError::Location)?;
    let zone = nearest(place).ok_or(DetectError::NoZone)?;
    let current = timedate
        .clock()
        .await
        .map_err(|error| DetectError::Clock(error, zone.clone()))?
        .timezone;
    if current == zone {
        return Ok(Detected::Same(zone));
    }
    timedate
        .set_timezone(&zone)
        .await
        .map_err(|error| DetectError::Clock(error, zone.clone()))?;
    Ok(Detected::Set(zone))
}

/// tzdata's table of zones with the coordinates of their main city: `zone.tab` (a zone per
/// country), else `zone1970.tab`, in `$TZDIR` or `/usr/share/zoneinfo`.
pub fn zone_table() -> Option<String> {
    let dir = std::env::var_os("TZDIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| PathBuf::from("/usr/share/zoneinfo"), PathBuf::from);
    ["zone.tab", "zone1970.tab"]
        .iter()
        .find_map(|name| std::fs::read_to_string(dir.join(name)).ok())
}

/// The zone of the city in `table` (tzdata's `zone.tab` format: country codes, coordinates as
/// ISO 6709, zone) nearest to `latitude` and `longitude`. Near a border the nearest city can be
/// across it, as with GNOME's own lookup; `zone.tab` lists a city for most countries, which keeps
/// that rare.
pub fn nearest_in(table: &str, latitude: f64, longitude: f64) -> Option<String> {
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let _countries = fields.next()?;
            let (lat, lon) = coordinates(fields.next()?)?;
            let zone = fields.next()?.trim();
            (!zone.is_empty()).then(|| (distance(latitude, longitude, lat, lon), zone))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, zone)| zone.to_owned())
}

/// The zone nearest to `place`, from the system's tzdata.
pub fn nearest(place: Place) -> Option<String> {
    nearest_in(&zone_table()?, place.latitude, place.longitude)
}

/// ISO 6709 coordinates as tzdata writes them (`+5230+01322`, `-332552+1510818`) in degrees.
fn coordinates(text: &str) -> Option<(f64, f64)> {
    let split = text
        .char_indices()
        .skip(1)
        .find(|(_, c)| *c == '+' || *c == '-')
        .map(|(index, _)| index)?;
    let (lat, lon) = text.split_at(split);
    Some((degrees(lat, 2)?, degrees(lon, 3)?))
}

/// `±DDMM[SS]` (with `width` digits of degrees) in degrees.
fn degrees(text: &str, width: usize) -> Option<f64> {
    let sign = match text.chars().next()? {
        '+' => 1.0,
        '-' => -1.0,
        _ => return None,
    };
    let digits = &text[1..];
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let part = |from: usize, to: usize| digits.get(from..to)?.parse::<f64>().ok();
    let (whole, minutes) = (part(0, width)?, part(width, width + 2)?);
    let seconds = match digits.len() - width {
        2 => 0.0,
        4 => part(width + 2, width + 4)?,
        _ => return None,
    };
    Some(sign * (whole + minutes / 60.0 + seconds / 3600.0))
}

/// The angle between two places on the globe, in radians: what the nearest city is ranked by.
fn distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (lat1, lon1, lat2, lon2) = (
        lat1.to_radians(),
        lon1.to_radians(),
        lat2.to_radians(),
        lon2.to_radians(),
    );
    let a = ((lat2 - lat1) / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
    2.0 * a.sqrt().clamp(0.0, 1.0).asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_read_as_iso_6709() {
        let (lat, lon) = coordinates("+5230+01322").unwrap();
        assert!((lat - 52.5).abs() < 1e-9 && (lon - (13.0 + 22.0 / 60.0)).abs() < 1e-9);
        let (lat, lon) = coordinates("-332552+1510818").unwrap();
        assert!((lat + (33.0 + 25.0 / 60.0 + 52.0 / 3600.0)).abs() < 1e-9);
        assert!((lon - (151.0 + 8.0 / 60.0 + 18.0 / 3600.0)).abs() < 1e-9);
        assert_eq!(coordinates("+52"), None);
        assert_eq!(coordinates("5230+01322"), None);
        assert_eq!(coordinates("+52x0+01322"), None);
    }

    #[test]
    fn the_nearest_city_gives_the_zone() {
        let table = "# comment\n\
            US\t+404251-0740023\tAmerica/New_York\tEastern (most areas)\n\
            JP\t+353916+1394441\tAsia/Tokyo\n\
            AU\t-3352+15113\tAustralia/Sydney\tNew South Wales (most areas)\n\
            broken line\n";
        assert_eq!(
            nearest_in(table, 40.0, -75.0).as_deref(),
            Some("America/New_York")
        );
        assert_eq!(
            nearest_in(table, 34.7, 135.5).as_deref(),
            Some("Asia/Tokyo")
        );
        assert_eq!(
            nearest_in(table, -37.8, 145.0).as_deref(),
            Some("Australia/Sydney")
        );
        // Across the date line.
        assert_eq!(
            nearest_in(
                "FJ\t-1808+17825\tPacific/Fiji\nUS\t+211825-1575130\tPacific/Honolulu\n",
                -17.0,
                -179.5
            )
            .as_deref(),
            Some("Pacific/Fiji")
        );
        assert_eq!(nearest_in("", 0.0, 0.0), None);
    }

    #[test]
    fn the_drop_in_allows_sway_things() {
        assert_eq!(
            drop_in_text(),
            "[io.github.mowi12.SwayThings]\nallowed=true\nsystem=false\nusers=\n"
        );
        assert_eq!(
            drop_in_command(),
            "printf '[io.github.mowi12.SwayThings]\\nallowed=true\\nsystem=false\\nusers=\\n' | \
             sudo tee /etc/geoclue/conf.d/90-sway-things.conf"
        );
    }
}
