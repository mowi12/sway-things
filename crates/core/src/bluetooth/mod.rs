// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bluetooth through BlueZ on the system D-Bus: the adapters, their devices, what can be done with
//! them, the pairing agent, and the radio's kill switch (rfkill).
//!
//! [`Bluez`] is the backend; the types here are plain data, free of D-Bus. Device names come from
//! anyone in radio range: they are data, shown as plain text and never put into commands.
//!
//! Nothing here is stored by Sway Things: the adapter's power, the pairings and the aliases are
//! BlueZ's own state, which it keeps across reboots.

mod agent;
mod bluez;
pub mod rfkill;
#[cfg(any(test, feature = "test-support"))]
pub mod stand_in;

use crate::dbus::clean;
use std::fmt;

use crate::i18n::Text;
use crate::msg;

pub use agent::{AgentEvent, Answer, Prompt, Request, parse_passkey, passkey_text, valid_pin};
pub use bluez::Bluez;

/// A Bluetooth adapter: the radio of the machine (a built-in chip or a USB dongle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    /// The D-Bus object path (`/org/bluez/hci0`), which the actions take.
    pub path: String,
    /// The adapter's alias, else its system name, else its address.
    pub name: String,
    /// The Bluetooth address (`00:11:22:33:44:55`).
    pub address: String,
    /// Whether the radio is on.
    pub powered: bool,
    /// Whether it is looking for devices.
    pub discovering: bool,
    /// Whether other devices can find it.
    pub discoverable: bool,
    /// How long it stays visible after [`discoverable`](Self::discoverable) is switched on, in
    /// seconds; 0 means until it is switched off.
    pub discoverable_timeout: u32,
}

impl Adapter {
    /// The kernel's name of the adapter (`hci0`), the last part of its path.
    pub fn id(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// A device an adapter knows: paired, or seen while discovering.
#[allow(
    clippy::struct_excessive_bools,
    reason = "the flags are BlueZ's own device properties, independent of each other"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// The D-Bus object path (`/org/bluez/hci0/dev_00_11_22_33_44_55`), which the actions take.
    pub path: String,
    /// The path of the adapter it belongs to.
    pub adapter: String,
    /// The Bluetooth address.
    pub address: String,
    /// The alias the user gave it, else the name it announces, else its address, without
    /// control characters. Untrusted: show it as plain text only.
    pub name: String,
    /// Whether it has a name (if not, [`name`](Self::name) is the address).
    pub named: bool,
    /// A freedesktop icon name for its kind (`audio-headset`, `input-mouse`), as BlueZ reports it.
    pub icon: Option<String>,
    /// Whether it is paired.
    pub paired: bool,
    /// Whether it is connected.
    pub connected: bool,
    /// Whether it may connect without asking.
    pub trusted: bool,
    /// Its battery charge in percent, if it reports one.
    pub battery: Option<u8>,
}

/// What BlueZ reports right now: the adapters and their devices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// The adapters, by path (so `hci0` first). Empty without an adapter.
    pub adapters: Vec<Adapter>,
    /// The devices of all adapters: connected ones first, then the paired ones, then those with a
    /// name, each by name.
    pub devices: Vec<Device>,
}

impl Snapshot {
    /// The adapter at `path`.
    pub fn adapter(&self, path: &str) -> Option<&Adapter> {
        self.adapters.iter().find(|adapter| adapter.path == path)
    }

    /// The devices of the adapter at `path`, in the order of [`devices`](Self::devices).
    pub fn devices_of<'a>(&'a self, adapter: &'a str) -> impl Iterator<Item = &'a Device> {
        self.devices
            .iter()
            .filter(move |device| device.adapter == adapter)
    }

    /// The device at `path`.
    pub fn device(&self, path: &str) -> Option<&Device> {
        self.devices.iter().find(|device| device.path == path)
    }
}

/// Why Bluetooth can't be read or an action failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No system bus, or BlueZ isn't running on it. The text says which.
    Unavailable(Text),
    /// The radio is blocked by rfkill, so it can't be switched on.
    Blocked,
    /// A name that BlueZ wouldn't take: see [`check_alias`].
    InvalidName(NameError),
    /// BlueZ refused or failed: the D-Bus error and its message (from BlueZ, so untrusted text, as
    /// data).
    Failed(Text),
}

impl Error {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Unavailable(reason) => Text::new(msg!("Bluetooth isn't available: {reason}"))
                .text("reason", reason.clone()),
            Self::Blocked => msg!("Bluetooth is blocked").into(),
            Self::InvalidName(error) => error.text(),
            Self::Failed(reason) => reason.clone(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for Error {}

/// Why a name can't be a device's alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// Longer than the [`MAX_NAME_BYTES`] bytes Bluetooth allows.
    TooLong,
    /// Has a control character (a newline, a tab, an escape).
    ControlCharacter,
}

impl NameError {
    /// What is wrong with the name, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::TooLong => {
                Text::new(msg!("The name is longer than {max} bytes")).arg("max", MAX_NAME_BYTES)
            }
            Self::ControlCharacter => msg!("The name has a control character").into(),
        }
    }
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

/// The longest name Bluetooth has room for, in bytes of UTF-8 (the length of a device name in the
/// Core Specification).
pub const MAX_NAME_BYTES: usize = 248;

/// `name` as an alias for a device: trimmed, at most [`MAX_NAME_BYTES`] bytes, no control
/// characters. An empty alias gives the device its own name back.
///
/// # Errors
///
/// The [`NameError`] that rules it out.
pub fn check_alias(name: &str) -> Result<String, NameError> {
    let name = name.trim();
    if name.chars().any(char::is_control) {
        Err(NameError::ControlCharacter)
    } else if name.len() > MAX_NAME_BYTES {
        Err(NameError::TooLong)
    } else {
        Ok(name.to_owned())
    }
}

/// Sorts `devices`: connected first, then paired, then those with a name, each by name.
fn sort(devices: &mut [Device]) {
    devices.sort_by_cached_key(|device| {
        (
            !device.connected,
            !device.paired,
            !device.named,
            device.name.to_lowercase(),
            device.address.clone(),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, connected: bool, paired: bool, named: bool) -> Device {
        Device {
            path: format!("/org/bluez/hci0/dev_{name}"),
            adapter: "/org/bluez/hci0".to_owned(),
            address: name.to_owned(),
            name: name.to_owned(),
            named,
            icon: None,
            paired,
            connected,
            trusted: false,
            battery: None,
        }
    }

    #[test]
    fn devices_sort_connected_then_paired_then_named_then_by_name() {
        let mut devices = vec![
            device("zeta", false, false, true),
            device("00:11", false, false, false),
            device("Beta", false, true, true),
            device("alpha", false, true, true),
            device("Gamma", true, true, true),
        ];
        sort(&mut devices);
        let names: Vec<&str> = devices.iter().map(|device| device.name.as_str()).collect();
        assert_eq!(names, ["Gamma", "alpha", "Beta", "zeta", "00:11"]);
    }

    #[test]
    fn control_characters_are_cleaned() {
        assert_eq!(clean(" Head\nphones\u{1b}[31m "), "Head phones [31m");
        assert_eq!(clean("\n\t"), "");
    }

    #[test]
    fn aliases_are_checked() {
        assert_eq!(
            check_alias("  Kitchen speaker "),
            Ok("Kitchen speaker".to_owned())
        );
        assert_eq!(check_alias(""), Ok(String::new()));
        assert_eq!(check_alias("a\nb"), Err(NameError::ControlCharacter));
        assert_eq!(check_alias(&"ä".repeat(124)), Ok("ä".repeat(124)));
        assert_eq!(check_alias(&"ä".repeat(125)), Err(NameError::TooLong));
    }

    #[test]
    fn the_snapshot_finds_adapters_and_their_devices() {
        let mut other = device("other", false, true, true);
        other.adapter = "/org/bluez/hci1".to_owned();
        let snapshot = Snapshot {
            adapters: Vec::new(),
            devices: vec![device("mine", false, true, true), other],
        };
        let mine: Vec<&str> = snapshot
            .devices_of("/org/bluez/hci0")
            .map(|device| device.name.as_str())
            .collect();
        assert_eq!(mine, ["mine"]);
        assert!(snapshot.device("/org/bluez/hci0/dev_other").is_some());
    }

    #[test]
    fn an_adapter_id_is_the_last_part_of_its_path() {
        let adapter = Adapter {
            path: "/org/bluez/hci1".to_owned(),
            name: String::new(),
            address: String::new(),
            powered: false,
            discovering: false,
            discoverable: false,
            discoverable_timeout: 0,
        };
        assert_eq!(adapter.id(), "hci1");
    }
}
