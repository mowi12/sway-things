// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The battery of a Bluetooth device and whether it charges.
//!
//! BlueZ's `org.bluez.Battery1` has only a percentage. Whether a device charges comes from UPower
//! (`org.freedesktop.UPower.Device` on the system bus) or, without UPower, from the kernel's
//! power supply of the device (`/sys/class/power_supply/*`, scope `Device`). Both are tied to a
//! BlueZ device by its Bluetooth address:
//!
//! - UPower's own BlueZ devices have the device's object path as `NativePath`
//!   (`/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF`) and its address as `Serial`. They know no more than
//!   BlueZ: their `State` is unknown.
//! - A device whose kernel driver reports its battery (HID++ mice and keyboards, game
//!   controllers) has a power supply whose `serial_number` is the address (in lower case): the
//!   kernel takes it from the HID device's `uniq`, which is the address for Bluetooth. UPower's
//!   device for it has the supply's name as `NativePath` and the same `Serial`.
//!
//! A driver that puts something else into `serial_number` (a USB receiver's serial, an empty
//! one) can't be tied to a device: its device then shows BlueZ's percentage without a charging
//! state. Nothing here guesses from names or models.

use std::fs;
use std::path::Path;
use std::time::Duration;

use async_io::Timer;
use futures_lite::future;
use zbus::Connection;
use zbus::zvariant::OwnedObjectPath;

use crate::dbus::{Properties, clean, number, text};

/// The bus name of UPower.
pub(super) const UPOWER: &str = "org.freedesktop.UPower";
/// UPower's main object, which lists the devices and reports new and removed ones.
pub(super) const UPOWER_PATH: &str = "/org/freedesktop/UPower";
/// Where UPower's devices are.
pub(super) const UPOWER_DEVICES: &str = "/org/freedesktop/UPower/devices";
/// The interface of a UPower device.
pub(super) const UPOWER_DEVICE: &str = "org.freedesktop.UPower.Device";
/// The properties of a UPower device that the battery shows; a change of only others (the time
/// of the last update, the energy rate) isn't reported.
pub(super) const UPOWER_SHOWN: &[&str] = &[
    "State",
    "Percentage",
    "BatteryLevel",
    "IsPresent",
    "PowerSupply",
    "NativePath",
    "Serial",
];
/// UPower's device that sums up the computer's own batteries; not among its devices, but its
/// changes come with theirs.
pub(super) const UPOWER_DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
/// How long UPower may take to answer before the snapshot does without it.
const UPOWER_TIMEOUT: Duration = Duration::from_secs(2);
/// Where the kernel lists its power supplies.
pub const POWER_SUPPLY: &str = "/sys/class/power_supply";

/// The battery of a device: its level and whether it charges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    /// How full it is.
    pub level: Level,
    /// Whether it charges; [`Charge::Unknown`] if nothing says.
    pub charge: Charge,
}

/// How full a battery is. Devices measure it themselves, often from the battery's voltage, so
/// it is an estimate that can jump while charging and right after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// A percentage, 0 to 100.
    Percent(u8),
    /// A coarse level, from a device that reports no percentage.
    Coarse(Coarse),
}

/// The coarse level of a battery that reports no percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coarse {
    /// Nearly empty.
    Critical,
    /// Low.
    Low,
    /// Neither low nor high.
    Normal,
    /// High.
    High,
    /// Full.
    Full,
}

/// Whether a battery charges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charge {
    /// Nothing says: BlueZ alone, or a device that doesn't report it.
    Unknown,
    /// It charges.
    Charging,
    /// It runs on its battery.
    Discharging,
    /// It is fully charged (and still plugged in).
    Full,
}

/// Where the charging state of the devices comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChargeSource {
    /// Nowhere: neither UPower nor the kernel's power supplies (BlueZ's percentage only).
    #[default]
    None,
    /// UPower, which reports changes.
    UPower,
    /// The kernel's power supplies, which report no changes: read them again now and then. Set
    /// without UPower while a paired device is connected or a device's battery is among the
    /// supplies, so a battery that shows up after its device connects is found too.
    Kernel,
}

/// A battery UPower or the kernel reports, before it is tied to a Bluetooth device.
#[derive(Debug, Clone, PartialEq)]
pub struct Supply {
    /// UPower's `NativePath` (a BlueZ object path or the kernel's name of the supply), or the
    /// kernel's name of the supply.
    pub native_path: String,
    /// UPower's `Serial` or the supply's `serial_number`: the address, for the batteries of
    /// Bluetooth devices.
    pub serial: String,
    /// Its percentage, if it reports one.
    pub percent: Option<f64>,
    /// Its coarse level, if it reports only that.
    pub coarse: Option<Coarse>,
    /// Whether it charges.
    pub charge: Charge,
    /// Whether the battery is there.
    pub present: bool,
    /// Whether it powers the computer (a laptop's battery, a mains adapter): never a device's.
    pub powers_system: bool,
}

/// Whether two Bluetooth addresses are the same: six pairs of hex digits with colons, in any
/// case. Anything else, an empty serial included, matches nothing.
fn same_address(a: &str, b: &str) -> bool {
    is_address(a) && is_address(b) && a.eq_ignore_ascii_case(b)
}

/// Whether `text` is a Bluetooth address (`AA:BB:CC:DD:EE:FF`, any case).
fn is_address(text: &str) -> bool {
    let parts: Vec<&str> = text.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The supply among `supplies` that is the battery of the BlueZ device at `path` with
/// `address`: one whose native path is the device's object path or whose serial is its address.
/// Of several, the one that knows whether it charges, then one of the kernel (by serial), which
/// knows more than UPower's copy of BlueZ's battery. Supplies that aren't there or power the
/// computer never match.
pub fn find<'a>(path: &str, address: &str, supplies: &'a [Supply]) -> Option<&'a Supply> {
    supplies
        .iter()
        .filter(|supply| supply.present && !supply.powers_system)
        .filter(|supply| supply.native_path == path || same_address(&supply.serial, address))
        .min_by_key(|supply| {
            (
                supply.charge == Charge::Unknown,
                !same_address(&supply.serial, address) || supply.native_path == path,
                supply.native_path.clone(),
            )
        })
}

/// The battery of a device with BlueZ's percentage `bluez` and the matching `supply`: the
/// supply's level and charging state, else BlueZ's percentage with an unknown state. `None`
/// without a level from either.
pub fn battery(bluez: Option<u8>, supply: Option<&Supply>) -> Option<Battery> {
    let charge = supply.map_or(Charge::Unknown, |supply| supply.charge);
    let level = supply
        .and_then(|supply| {
            supply.coarse.map(Level::Coarse).or_else(|| {
                let percent = supply.percent.and_then(percent)?;
                // UPower's 0 % without a state is a battery that reports no level yet.
                (percent > 0 || charge != Charge::Unknown).then_some(Level::Percent(percent))
            })
        })
        .or_else(|| bluez.map(Level::Percent))?;
    Some(Battery { level, charge })
}

/// `value` as a whole percentage; `None` outside 0 to 100.
fn percent(value: f64) -> Option<u8> {
    if !(0.0..=100.0).contains(&value) {
        return None;
    }
    (0u8..=100).min_by(|a, b| {
        (f64::from(*a) - value)
            .abs()
            .total_cmp(&(f64::from(*b) - value).abs())
    })
}

/// What UPower reports: the batteries, and the object paths of those that power the computer,
/// whose changes don't concern Bluetooth devices.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct UPowerDevices {
    pub supplies: Vec<Supply>,
    pub system_paths: Vec<String>,
}

/// What UPower reports, or `None` while it isn't running or doesn't answer within
/// [`UPOWER_TIMEOUT`].
pub(super) async fn upower(connection: &Connection) -> Option<UPowerDevices> {
    future::or(upower_devices(connection), async {
        Timer::after(UPOWER_TIMEOUT).await;
        None
    })
    .await
}

/// UPower's version (`1.91.5`), from its `DaemonVersion` property; `None` while it isn't
/// running or doesn't answer within [`UPOWER_TIMEOUT`]. Asks only UPower's owner, so the question
/// never starts it.
pub(super) async fn upower_version(connection: &Connection) -> Option<String> {
    let version = async {
        let owner = upower_owner(connection).await?;
        let reply = connection
            .call_method(
                Some(owner.as_str()),
                UPOWER_PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(UPOWER, "DaemonVersion"),
            )
            .await
            .ok()?;
        let value: zbus::zvariant::OwnedValue = reply.body().deserialize().ok()?;
        let version = clean(&String::try_from(value).ok()?);
        (!version.is_empty()).then_some(version)
    };
    future::or(version, async {
        Timer::after(UPOWER_TIMEOUT).await;
        None
    })
    .await
}

/// Who owns UPower's bus name; `None` while it isn't running.
async fn upower_owner(connection: &Connection) -> Option<String> {
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &(UPOWER,),
        )
        .await
        .ok()?;
    reply.body().deserialize().ok()
}

/// What UPower reports. Asks who owns UPower's bus name first and sends the questions to that
/// owner, so they never start UPower.
async fn upower_devices(connection: &Connection) -> Option<UPowerDevices> {
    let owner = upower_owner(connection).await?;
    let reply = connection
        .call_method(
            Some(owner.as_str()),
            UPOWER_PATH,
            Some(UPOWER),
            "EnumerateDevices",
            &(),
        )
        .await
        .ok()?;
    let paths: Vec<OwnedObjectPath> = reply.body().deserialize().ok()?;
    let mut devices = UPowerDevices::default();
    for path in paths {
        // A device that went in between is left out.
        let Ok(reply) = connection
            .call_method(
                Some(owner.as_str()),
                path.as_str(),
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(UPOWER_DEVICE,),
            )
            .await
        else {
            continue;
        };
        if let Ok(properties) = reply.body().deserialize::<Properties>() {
            let supply = upower_supply(&properties);
            if supply.powers_system {
                devices.system_paths.push(path.to_string());
            }
            devices.supplies.push(supply);
        }
    }
    Some(devices)
}

/// The supply of a UPower device with these `properties`.
fn upower_supply(properties: &Properties) -> Supply {
    let flag = |name: &str| {
        properties
            .get(name)
            .and_then(|value| bool::try_from(value).ok())
    };
    // UPower's `UpDeviceState`.
    let charge = match number(properties, "State") {
        1 => Charge::Charging,
        2 | 3 | 6 => Charge::Discharging,
        4 => Charge::Full,
        // 0 unknown, 5 pending charge (plugged in, not charging).
        _ => Charge::Unknown,
    };
    // UPower's `UpDeviceLevel`: "none" (1) and unknown mean the percentage counts.
    let coarse = match number(properties, "BatteryLevel") {
        3 => Some(Coarse::Low),
        4 => Some(Coarse::Critical),
        6 => Some(Coarse::Normal),
        7 => Some(Coarse::High),
        8 => Some(Coarse::Full),
        _ => None,
    };
    Supply {
        native_path: text(properties, "NativePath").unwrap_or_default(),
        serial: text(properties, "Serial").unwrap_or_default(),
        percent: properties
            .get("Percentage")
            .and_then(|value| f64::try_from(value).ok()),
        coarse,
        charge,
        present: flag("IsPresent").unwrap_or(true),
        powers_system: flag("PowerSupply").unwrap_or(false),
    }
}

/// [`kernel`] on a thread of the blocking pool, off the caller's main loop.
pub(super) async fn kernel_unblocked(dir: std::path::PathBuf) -> Vec<Supply> {
    blocking::unblock(move || kernel(&dir)).await
}

/// The batteries of devices among the kernel's power supplies in `dir`
/// ([`POWER_SUPPLY`]): those of scope `Device` and type `Battery`; of the others only these two
/// files are read. Reads only; a supply that can't be read is left out.
///
/// # Blocking
///
/// Reads the files of every power supply in `dir`.
pub fn kernel(dir: &Path) -> Vec<Supply> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut supplies: Vec<Supply> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            kernel_supply(&entry.path(), name)
        })
        .collect();
    supplies.sort_by(|a, b| a.native_path.cmp(&b.native_path));
    supplies
}

/// The supply in `dir` named `name`, if it is the battery of a device.
fn kernel_supply(dir: &Path, name: String) -> Option<Supply> {
    let read = |file: &str| {
        fs::read_to_string(dir.join(file))
            .ok()
            .map(|text| clean(&text))
    };
    if read("scope")? != "Device" || read("type").as_deref() != Some("Battery") {
        return None;
    }
    let charge = match read("status").as_deref() {
        Some("Charging") => Charge::Charging,
        Some("Discharging") => Charge::Discharging,
        Some("Full") => Charge::Full,
        // "Not charging" (plugged in), "Unknown".
        _ => Charge::Unknown,
    };
    let coarse = match read("capacity_level").as_deref() {
        Some("Critical") => Some(Coarse::Critical),
        Some("Low") => Some(Coarse::Low),
        Some("Normal") => Some(Coarse::Normal),
        Some("High") => Some(Coarse::High),
        Some("Full") => Some(Coarse::Full),
        _ => None,
    };
    let percent = read("capacity").and_then(|text| text.parse::<u8>().ok());
    Some(Supply {
        native_path: name,
        serial: read("serial_number").unwrap_or_default(),
        percent: percent.map(f64::from),
        // A percentage is finer: the level counts only without one.
        coarse: if percent.is_some() { None } else { coarse },
        charge,
        present: read("present").is_none_or(|present| present != "0"),
        powers_system: false,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use zbus::zvariant::{OwnedValue, Value};

    use super::*;

    const PATH: &str = "/org/bluez/hci0/dev_DF_54_EE_3C_AC_81";
    const ADDRESS: &str = "DF:54:EE:3C:AC:81";

    fn supply(native_path: &str, serial: &str, charge: Charge) -> Supply {
        Supply {
            native_path: native_path.to_owned(),
            serial: serial.to_owned(),
            percent: Some(55.0),
            coarse: None,
            charge,
            present: true,
            powers_system: false,
        }
    }

    #[test]
    fn a_kernel_supply_matches_by_its_serial_in_any_case() {
        let supplies = [supply(
            "hidpp_battery_7",
            "df:54:ee:3c:ac:81",
            Charge::Charging,
        )];
        assert_eq!(
            find(PATH, ADDRESS, &supplies).map(|s| s.native_path.as_str()),
            Some("hidpp_battery_7")
        );
        assert!(find("/org/bluez/hci0/dev_11", "11:22:33:44:55:66", &supplies).is_none());
    }

    #[test]
    fn upowers_bluez_device_matches_by_its_path() {
        let supplies = [supply(PATH, "", Charge::Unknown)];
        assert!(find(PATH, ADDRESS, &supplies).is_some());
        assert!(find("/org/bluez/hci1/dev_DF_54_EE_3C_AC_81", "", &supplies).is_none());
    }

    #[test]
    fn the_supply_that_knows_whether_it_charges_wins() {
        let supplies = [
            supply(PATH, ADDRESS, Charge::Unknown),
            supply("hidpp_battery_7", "df:54:ee:3c:ac:81", Charge::Discharging),
        ];
        assert_eq!(
            find(PATH, ADDRESS, &supplies).map(|s| s.native_path.as_str()),
            Some("hidpp_battery_7")
        );
        // Neither knows: the kernel's, by serial.
        let supplies = [
            supply(PATH, ADDRESS, Charge::Unknown),
            supply("hidpp_battery_7", "df:54:ee:3c:ac:81", Charge::Unknown),
        ];
        assert_eq!(
            find(PATH, ADDRESS, &supplies).map(|s| s.native_path.as_str()),
            Some("hidpp_battery_7")
        );
    }

    #[test]
    fn what_isnt_an_address_matches_nothing() {
        for serial in ["", "1234567890", "df:54:ee:3c:ac", "df:54:ee:3c:ac:8g"] {
            let supplies = [supply("hidpp_battery_1", serial, Charge::Charging)];
            assert!(find(PATH, serial, &supplies).is_none(), "{serial:?}");
        }
    }

    #[test]
    fn the_computers_batteries_and_absent_ones_match_nothing() {
        let mut system = supply("BAT0", ADDRESS, Charge::Charging);
        system.powers_system = true;
        let mut absent = supply("hidpp_battery_7", ADDRESS, Charge::Charging);
        absent.present = false;
        assert!(find(PATH, ADDRESS, &[system, absent]).is_none());
    }

    #[test]
    fn the_battery_takes_the_supplys_level_and_state() {
        let mut charging = supply("hidpp_battery_7", ADDRESS, Charge::Charging);
        charging.percent = Some(60.4);
        assert_eq!(
            battery(Some(55), Some(&charging)),
            Some(Battery {
                level: Level::Percent(60),
                charge: Charge::Charging
            })
        );
        charging.coarse = Some(Coarse::High);
        assert_eq!(
            battery(Some(55), Some(&charging)).map(|b| b.level),
            Some(Level::Coarse(Coarse::High))
        );
        // Without a level of its own, BlueZ's percentage with the supply's state.
        let mut full = supply(PATH, ADDRESS, Charge::Full);
        full.percent = None;
        assert_eq!(
            battery(Some(100), Some(&full)),
            Some(Battery {
                level: Level::Percent(100),
                charge: Charge::Full
            })
        );
        full.percent = Some(250.0);
        assert_eq!(battery(None, Some(&full)), None);
    }

    #[test]
    fn upowers_zero_without_a_state_is_no_level() {
        let mut empty = supply(PATH, ADDRESS, Charge::Unknown);
        empty.percent = Some(0.0);
        assert_eq!(
            battery(Some(70), Some(&empty)).map(|b| b.level),
            Some(Level::Percent(70))
        );
        assert_eq!(battery(None, Some(&empty)), None);
        // A battery that says it discharges at 0 % is empty.
        empty.charge = Charge::Discharging;
        assert_eq!(
            battery(Some(70), Some(&empty)).map(|b| b.level),
            Some(Level::Percent(0))
        );
    }

    #[test]
    fn bluez_alone_claims_no_state() {
        assert_eq!(
            battery(Some(42), None),
            Some(Battery {
                level: Level::Percent(42),
                charge: Charge::Unknown
            })
        );
        assert_eq!(battery(None, None), None);
    }

    #[test]
    fn upower_properties_are_read() {
        let value = |value: Value<'static>| OwnedValue::try_from(value).unwrap();
        let properties = HashMap::from([
            (
                "NativePath".to_owned(),
                value(Value::from("hidpp_battery_7")),
            ),
            (
                "Serial".to_owned(),
                value(Value::from("df:54:ee:3c:ac:81\n")),
            ),
            ("State".to_owned(), value(Value::from(1u32))),
            ("Percentage".to_owned(), value(Value::from(55.0f64))),
            ("BatteryLevel".to_owned(), value(Value::from(1u32))),
            ("IsPresent".to_owned(), value(Value::from(true))),
            ("PowerSupply".to_owned(), value(Value::from(false))),
        ]);
        let supply = upower_supply(&properties);
        assert_eq!(supply.serial, "df:54:ee:3c:ac:81");
        assert_eq!(supply.charge, Charge::Charging);
        assert_eq!(supply.coarse, None);
        assert_eq!(supply.percent, Some(55.0));
        for (state, charge) in [
            (0u32, Charge::Unknown),
            (2, Charge::Discharging),
            (4, Charge::Full),
            (5, Charge::Unknown),
        ] {
            let mut properties = properties.clone();
            properties.insert("State".to_owned(), value(Value::from(state)));
            assert_eq!(upower_supply(&properties).charge, charge, "state {state}");
        }
        let mut coarse = properties.clone();
        coarse.insert("BatteryLevel".to_owned(), value(Value::from(7u32)));
        assert_eq!(upower_supply(&coarse).coarse, Some(Coarse::High));
    }

    #[test]
    fn the_kernels_device_batteries_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, files: &[(&str, &str)]| {
            let supply = dir.path().join(name);
            fs::create_dir(&supply).unwrap();
            for (file, text) in files {
                fs::write(supply.join(file), format!("{text}\n")).unwrap();
            }
        };
        write(
            "hidpp_battery_7",
            &[
                ("scope", "Device"),
                ("type", "Battery"),
                ("status", "Charging"),
                ("capacity", "55"),
                ("serial_number", "df:54:ee:3c:ac:81"),
                ("online", "1"),
            ],
        );
        write(
            "hidpp_battery_8",
            &[
                ("scope", "Device"),
                ("type", "Battery"),
                ("status", "Not charging"),
                ("capacity_level", "Low"),
                ("serial_number", "11:22:33:44:55:66"),
            ],
        );
        write("BAT0", &[("type", "Battery"), ("status", "Full")]);
        write("AC", &[("scope", "System"), ("type", "Mains")]);
        let supplies = kernel(dir.path());
        assert_eq!(supplies.len(), 2);
        assert_eq!(supplies[0].native_path, "hidpp_battery_7");
        assert_eq!(supplies[0].charge, Charge::Charging);
        assert_eq!(supplies[0].percent, Some(55.0));
        assert_eq!(supplies[1].charge, Charge::Unknown);
        assert_eq!(supplies[1].coarse, Some(Coarse::Low));
        assert_eq!(
            find(PATH, ADDRESS, &supplies).map(|s| s.native_path.as_str()),
            Some("hidpp_battery_7")
        );
        assert_eq!(kernel(&dir.path().join("missing")), Vec::new());
    }
}
