// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the D-Bus backends share (BlueZ, NetworkManager): reading properties, cleaning the names
//! services report, and recognizing a service that isn't running. Each backend maps D-Bus errors
//! to its own error type; the stand-ins of the tests share the private bus they run on.

use std::collections::HashMap;

use zbus::zvariant::OwnedValue;

/// The properties of one interface of an object, by name.
pub(crate) type Properties = HashMap<String, OwnedValue>;

/// `text` with control characters replaced by spaces and trimmed: names come from devices and
/// networks, and are only ever shown as plain text.
pub(crate) fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The text property `name`, cleaned; `None` if missing or empty.
pub(crate) fn text(properties: &Properties, name: &str) -> Option<String> {
    let value = properties.get(name)?;
    let text = clean(<&str>::try_from(value).ok()?);
    (!text.is_empty()).then_some(text)
}

/// The number property `name`; 0 if missing.
pub(crate) fn number(properties: &Properties, name: &str) -> u32 {
    properties
        .get(name)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

/// The boolean property `name`; false if missing.
pub(crate) fn flag(properties: &Properties, name: &str) -> bool {
    properties
        .get(name)
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(false)
}

/// Whether the D-Bus error `name` says the service isn't there: not running, or not installed.
pub(crate) fn is_gone(name: &str) -> bool {
    matches!(
        name,
        "org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner"
    )
}

/// The last part of the D-Bus error `name`: `PermissionDenied` for
/// `org.freedesktop.NetworkManager.PermissionDenied`.
pub(crate) fn short_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// A private session bus for the stand-ins of the tests: `dbus-daemon` with its socket in a
/// directory of its own, and a connection to it.
#[cfg(any(test, feature = "test-support"))]
pub(crate) struct TestBus {
    /// The directory of the socket; removed when dropped.
    pub(crate) dir: tempfile::TempDir,
    /// The address of the bus.
    pub(crate) address: String,
    /// The daemon, to stop when the stand-in goes.
    pub(crate) daemon: std::process::Child,
    /// A connection to the bus.
    pub(crate) connection: zbus::Connection,
}

#[cfg(any(test, feature = "test-support"))]
impl TestBus {
    /// Starts `dbus-daemon` on a socket in a new temporary directory and connects to it.
    pub(crate) async fn start() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

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
        let connection = zbus::connection::Builder::address(address.as_str())?
            .build()
            .await?;
        Ok(Self {
            dir,
            address,
            daemon,
            connection,
        })
    }
}
