// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! systemd-timedated on the system bus (`org.freedesktop.timedate1`): the time zone, automatic
//! time and the clock, the zones it knows, and a stream of its changes.

use std::collections::HashMap;

use futures_lite::{Stream, StreamExt};
use zbus::message::Type;
use zbus::proxy::MethodFlags;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, MessageStream};

use super::{Error, clean};
use crate::dbus::{self, flag, text};
use crate::i18n::Text;
use crate::msg;

/// timedated's bus name and interface.
const SERVICE: &str = "org.freedesktop.timedate1";
/// Its object.
const PATH: &str = "/org/freedesktop/timedate1";
/// The standard interface for properties.
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// Where timedated is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Bus {
    /// The system bus, where it runs.
    #[default]
    System,
    /// Another bus, with a stand-in for it (tests).
    Address(String),
    /// None: the page shows that timedated isn't there.
    Off,
}

/// What timedated reports, read at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clock {
    /// The time zone, by its IANA name (`Europe/Berlin`); empty if timedated has none.
    pub timezone: String,
    /// Whether automatic time (NTP) is on.
    pub ntp: bool,
    /// Whether a service for automatic time is installed (systemd-timesyncd, chrony): without
    /// one, automatic time can't be turned on.
    pub can_ntp: bool,
    /// Whether the clock is synchronized.
    pub synchronized: bool,
}

/// A connection to timedated. Cheap to clone; every clone shares the connection.
#[derive(Debug, Clone)]
pub struct Timedate {
    connection: Connection,
}

impl Timedate {
    /// Connects to `bus`. Succeeds while timedated isn't running too: the bus starts it on the
    /// first call.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] without the bus.
    pub async fn connect(bus: &Bus) -> Result<Self, Error> {
        Ok(Self {
            connection: connect(bus, gone).await?,
        })
    }

    /// The time zone, automatic time and whether the clock is synchronized.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] without timedated, [`Error::Failed`] if it doesn't answer.
    pub async fn clock(&self) -> Result<Clock, Error> {
        let reply = self
            .connection
            .call_method(Some(SERVICE), PATH, Some(PROPERTIES), "GetAll", &(SERVICE,))
            .await
            .map_err(failure)?;
        let properties: HashMap<String, OwnedValue> =
            reply.body().deserialize().map_err(failure)?;
        Ok(Clock {
            timezone: text(&properties, "Timezone").unwrap_or_default(),
            ntp: flag(&properties, "NTP"),
            can_ntp: flag(&properties, "CanNTP"),
            synchronized: flag(&properties, "NTPSynchronized"),
        })
    }

    /// The time zones timedated takes, as tzdata lists them, sorted.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] without timedated, [`Error::Failed`] if it doesn't answer.
    pub async fn zones(&self) -> Result<Vec<String>, Error> {
        let reply = self
            .connection
            .call_method(Some(SERVICE), PATH, Some(SERVICE), "ListTimezones", &())
            .await
            .map_err(failure)?;
        let mut zones: Vec<String> = reply
            .body()
            .deserialize::<Vec<String>>()
            .map_err(failure)?
            .into_iter()
            .map(|zone| clean(&zone))
            .filter(|zone| !zone.is_empty())
            .collect();
        zones.sort();
        zones.dedup();
        Ok(zones)
    }

    /// Sets the time zone to `zone`, one of [`Timedate::zones`]. polkit may ask the user for a
    /// password first, which the call waits for.
    ///
    /// # Errors
    ///
    /// [`Error::Denied`] if polkit says no, [`Error::Unavailable`] without timedated,
    /// [`Error::Failed`] if timedated refuses (a zone it doesn't know).
    pub async fn set_timezone(&self, zone: &str) -> Result<(), Error> {
        self.call("SetTimezone", &(zone, true)).await
    }

    /// Turns automatic time on or off.
    ///
    /// # Errors
    ///
    /// As [`Timedate::set_timezone`]; also [`Error::Failed`] without an NTP service.
    pub async fn set_ntp(&self, on: bool) -> Result<(), Error> {
        self.call("SetNTP", &(on, true)).await
    }

    /// Sets the clock to `micros`, microseconds since the epoch (UTC). timedated refuses while
    /// automatic time is on.
    ///
    /// # Errors
    ///
    /// As [`Timedate::set_timezone`].
    pub async fn set_time(&self, micros: i64) -> Result<(), Error> {
        self.call("SetTime", &(micros, false, true)).await
    }

    /// Calls `method` with `args`, allowing polkit to ask for a password.
    async fn call<B>(&self, method: &str, args: &B) -> Result<(), Error>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        let proxy = zbus::Proxy::new(&self.connection, SERVICE, PATH, SERVICE)
            .await
            .map_err(failure)?;
        let reply: Result<(), zbus::Error> = proxy
            .call_with_flags::<_, _, ()>(method, MethodFlags::AllowInteractiveAuth.into(), args)
            .await
            .map(|_| ());
        reply.map_err(failure)
    }

    /// A stream that yields whenever timedated's properties change: a change made here or
    /// elsewhere (`timedatectl`). timedated quits when idle and the bus starts it again, which
    /// changes nothing, so its coming and going isn't reported.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the bus doesn't take the match rule.
    pub async fn changes(&self) -> Result<impl Stream<Item = ()> + Unpin + use<>, Error> {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(PROPERTIES)
            .and_then(|rule| rule.member("PropertiesChanged"))
            .and_then(|rule| rule.path(PATH))
            .map_err(failure)?
            .build();
        let stream = MessageStream::for_match_rule(rule, &self.connection, None)
            .await
            .map_err(failure)?;
        Ok(stream.filter_map(|message| message.ok().map(drop)))
    }
}

/// A connection to `bus`; `gone` says which service is missing with [`Bus::Off`].
pub(super) async fn connect(bus: &Bus, gone: fn() -> Text) -> Result<Connection, Error> {
    match bus {
        Bus::System => Connection::system().await.map_err(|error| {
            Error::Unavailable(Text::new(msg!("no system bus ({error})")).arg("error", error))
        }),
        Bus::Address(address) => zbus::connection::Builder::address(address.as_str())
            .map_err(|error| unavailable(address, &error))?
            .build()
            .await
            .map_err(|error| unavailable(address, &error)),
        Bus::Off => Err(Error::Unavailable(gone())),
    }
}

/// The text for a system without timedated.
fn gone() -> Text {
    msg!("systemd-timedated isn't available").into()
}

fn unavailable(address: &str, error: &zbus::Error) -> Error {
    Error::Unavailable(
        Text::new(msg!("no bus at {address} ({error})"))
            .arg("address", address)
            .arg("error", error),
    )
}

/// The error of a failed call.
fn failure(error: zbus::Error) -> Error {
    call_failure(error, gone, |other| {
        Text::new(msg!("systemd-timedated failed: {error}")).arg("error", other)
    })
}

/// The error of a failed call to a service of systemd that polkit guards: `gone` for a service
/// that isn't there, `failed` for an error that isn't the service's own.
pub(super) fn call_failure(
    error: zbus::Error,
    gone: fn() -> Text,
    failed: fn(zbus::Error) -> Text,
) -> Error {
    match error {
        zbus::Error::MethodError(name, message, _) => {
            let name = name.as_str();
            let message = clean(&message.unwrap_or_default());
            if dbus::is_gone(name) || name == "org.freedesktop.DBus.Error.UnknownMethod" {
                Error::Unavailable(gone())
            } else if name == "org.freedesktop.DBus.Error.AccessDenied"
                || name == "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired"
                || name.starts_with("org.freedesktop.PolicyKit1.Error")
            {
                Error::Denied(if message.is_empty() {
                    dbus::short_name(name).to_owned()
                } else {
                    message
                })
            } else if message.is_empty() {
                Error::Failed(Text::data(dbus::short_name(name)))
            } else {
                Error::Failed(Text::data(message))
            }
        }
        other => Error::Failed(failed(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method_error(name: &'static str, message: &str) -> zbus::Error {
        let name = zbus::names::OwnedErrorName::try_from(name).unwrap();
        let reply = zbus::message::Message::method_call("/", "Ping")
            .unwrap()
            .build(&())
            .unwrap();
        zbus::Error::MethodError(name, Some(message.to_owned()), reply)
    }

    #[test]
    fn errors_say_what_polkit_and_the_bus_said() {
        assert!(matches!(
            failure(method_error(
                "org.freedesktop.DBus.Error.ServiceUnknown",
                ""
            )),
            Error::Unavailable(_)
        ));
        assert_eq!(
            failure(method_error(
                "org.freedesktop.DBus.Error.AccessDenied",
                "Access denied\n"
            )),
            Error::Denied("Access denied".to_owned())
        );
        assert_eq!(
            failure(method_error(
                "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired",
                ""
            )),
            Error::Denied("InteractiveAuthorizationRequired".to_owned())
        );
        assert_eq!(
            failure(method_error(
                "org.freedesktop.DBus.Error.InvalidArgs",
                "Invalid or not installed time zone 'Mars/Olympus'"
            )),
            Error::Failed(Text::data(
                "Invalid or not installed time zone 'Mars/Olympus'"
            ))
        );
    }
}
