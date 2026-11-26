// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The brightness of the screens and the keyboard, live: nothing of it is stored.
//!
//! - **Backlights** (`/sys/class/backlight`) and the **keyboard light** (an LED of
//!   `/sys/class/leds` whose name ends in `::kbd_backlight`) are read from sysfs and set through
//!   logind's `Session.SetBrightness` on the system bus ([`Logind`]), which lets the user of the
//!   session set them without root. A backlight never goes below 1 % ([`Device::raw_for`]), so
//!   the panel can't go black.
//! - **External monitors** speak DDC/CI over their cable: [`ddc`] runs `ddcutil`.
//!
//! The kernel or `systemd-backlight` brings a backlight back at boot, and a monitor keeps its own
//! value, so there is nothing to store.

pub mod ddc;
#[cfg(any(test, feature = "test-support"))]
pub mod stand_in;

use std::path::{Path, PathBuf};

use crate::i18n::Text;
use crate::msg;

/// Where sysfs has the devices.
const SYSFS: &str = "/sys/class";

/// The lowest share of a backlight, in percent: at 0 many panels go fully dark.
pub const MIN_BACKLIGHT_PERCENT: u32 = 1;

/// The step of `sway-things brightness up` and `down`, in percent: as the brightness keys.
pub const STEP_PERCENT: u32 = 5;

/// What kind of device a brightness is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// A backlight the firmware (ACPI) drives; preferred, as in systemd and GNOME.
    Firmware,
    /// A backlight of a platform driver (`thinkpad_screen`, `apple_backlight`).
    Platform,
    /// A backlight the graphics driver drives directly (`intel_backlight`, `amdgpu_bl0`).
    Raw,
    /// The keyboard's backlight, an LED.
    Keyboard,
}

impl Kind {
    /// The sysfs class and logind subsystem the kind is in.
    pub const fn subsystem(self) -> &'static str {
        match self {
            Self::Firmware | Self::Platform | Self::Raw => "backlight",
            Self::Keyboard => "leds",
        }
    }

    /// Whether it lights a screen.
    pub const fn is_screen(self) -> bool {
        !matches!(self, Self::Keyboard)
    }
}

/// A backlight or the keyboard light, as sysfs has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Its name in sysfs, which logind takes: `intel_backlight`, `tpacpi::kbd_backlight`.
    pub name: String,
    /// What it is.
    pub kind: Kind,
    /// Its brightness now, from 0 to `max`.
    pub brightness: u32,
    /// Its highest brightness.
    pub max: u32,
}

impl Device {
    /// The brightness in percent, rounded.
    pub fn percent(&self) -> u32 {
        percent_of(self.brightness, self.max)
    }

    /// The lowest raw value it may be set to: 1 % of a backlight (at least 1), 0 for the keyboard.
    pub fn min_raw(&self) -> u32 {
        if self.kind.is_screen() {
            raw_of(MIN_BACKLIGHT_PERCENT, self.max).max(1).min(self.max)
        } else {
            0
        }
    }

    /// The raw value for `percent`, never below [`Device::min_raw`] nor above the maximum.
    pub fn raw_for(&self, percent: u32) -> u32 {
        raw_of(percent.min(100), self.max).clamp(self.min_raw(), self.max)
    }

    /// The raw value one step of [`STEP_PERCENT`] up (if `up`) or down from the brightness now,
    /// within the limits of [`Device::raw_for`]; at least one raw step where the range is small.
    pub fn stepped(&self, up: bool) -> u32 {
        let step = raw_of(STEP_PERCENT, self.max).max(1);
        let raw = if up {
            self.brightness.saturating_add(step)
        } else {
            self.brightness.saturating_sub(step)
        };
        raw.clamp(self.min_raw(), self.max)
    }
}

/// `raw` of `max` in percent, rounded.
pub fn percent_of(raw: u32, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let percent = (u64::from(raw) * 100 + u64::from(max) / 2) / u64::from(max);
    u32::try_from(percent.min(100)).unwrap_or(100)
}

/// `percent` of `max`, rounded.
pub fn raw_of(percent: u32, max: u32) -> u32 {
    let raw = (u64::from(percent) * u64::from(max) + 50) / 100;
    u32::try_from(raw).unwrap_or(max)
}

/// The backlights and keyboard lights of this computer, the screens first (firmware before
/// platform before raw, as systemd and GNOME prefer them), then by name.
///
/// # Blocking
///
/// Reads `/sys/class/backlight` and `/sys/class/leds`.
pub fn devices() -> Vec<Device> {
    devices_in(Path::new(SYSFS))
}

/// [`devices`] below `root` (the place of `/sys/class`).
///
/// # Blocking
///
/// Reads `/sys/class/backlight` and `/sys/class/leds`.
pub fn devices_in(root: &Path) -> Vec<Device> {
    let mut found = Vec::new();
    for (class, keyboard) in [("backlight", false), ("leds", true)] {
        let Ok(entries) = std::fs::read_dir(root.join(class)) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
                continue;
            };
            if keyboard && !name.ends_with("::kbd_backlight") {
                continue;
            }
            if let Some(device) = read_device(&entry.path(), name, keyboard) {
                found.push(device);
            }
        }
    }
    found.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
    found
}

/// The device whose sysfs directory is `dir`, named `name`; `None` if its files can't be read
/// or it has no range.
fn read_device(dir: &Path, name: String, keyboard: bool) -> Option<Device> {
    let number = |file: &str| -> Option<u32> {
        std::fs::read_to_string(dir.join(file))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    let max = number("max_brightness").filter(|max| *max > 0)?;
    let brightness = number("brightness")?.min(max);
    let kind = if keyboard {
        Kind::Keyboard
    } else {
        match std::fs::read_to_string(dir.join("type")).ok()?.trim() {
            "firmware" => Kind::Firmware,
            "platform" => Kind::Platform,
            _ => Kind::Raw,
        }
    };
    Some(Device {
        name,
        kind,
        brightness,
        max,
    })
}

/// The device of `name`, read again; `None` if it is gone.
///
/// # Blocking
///
/// Reads `/sys/class/backlight` and `/sys/class/leds`.
pub fn device(name: &str) -> Option<Device> {
    devices().into_iter().find(|device| device.name == name)
}

/// The backlight of the screen: the first of [`devices`] that lights one.
///
/// # Blocking
///
/// Reads `/sys/class/backlight` and `/sys/class/leds`.
pub fn screen() -> Option<Device> {
    devices().into_iter().find(|device| device.kind.is_screen())
}

/// Why a brightness couldn't be set.
#[derive(Debug)]
pub enum BrightnessError {
    /// There is no system bus, or logind isn't on it.
    NoLogind(String),
    /// logind refused it: no session of the user's, or a device of another seat.
    Refused(String),
    /// No backlight or keyboard light of that name.
    NoDevice(String),
}

impl BrightnessError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::NoLogind(error) => Text::new(msg!(
                "The brightness is set through logind, which can't be reached: {error}"
            ))
            .arg("error", error),
            Self::Refused(error) => {
                Text::new(msg!("logind didn't change the brightness: {error}")).arg("error", error)
            }
            Self::NoDevice(name) => {
                Text::new(msg!("There is no backlight named {name}")).arg("name", name)
            }
        }
    }
}

impl std::fmt::Display for BrightnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for BrightnessError {}

/// The user's logind session on the system bus, which sets the brightness.
#[derive(Debug, Clone)]
pub struct Logind {
    connection: zbus::Connection,
    path: PathBuf,
}

/// The object of the caller's session: the session the process is in, else the user's display
/// session (logind resolves `auto` so).
const SESSION: &str = "/org/freedesktop/login1/session/auto";

impl Logind {
    /// Connects to the system bus.
    ///
    /// # Errors
    ///
    /// [`BrightnessError::NoLogind`] without a system bus.
    pub async fn connect() -> Result<Self, BrightnessError> {
        let connection = zbus::Connection::system()
            .await
            .map_err(|error| BrightnessError::NoLogind(error.to_string()))?;
        Ok(Self::with(connection, SESSION))
    }

    /// The session at `path` on `connection`, for the tests' stand-in.
    pub fn with(connection: zbus::Connection, path: &str) -> Self {
        Self {
            connection,
            path: PathBuf::from(path),
        }
    }

    /// Sets `device` to the raw value `raw`, within its limits ([`Device::min_raw`]).
    ///
    /// # Errors
    ///
    /// [`BrightnessError::NoLogind`] if logind isn't there, [`BrightnessError::Refused`] if it
    /// refuses.
    pub async fn set(&self, device: &Device, raw: u32) -> Result<(), BrightnessError> {
        let raw = raw.clamp(device.min_raw(), device.max);
        let path = self.path.to_string_lossy();
        self.connection
            .call_method(
                Some("org.freedesktop.login1"),
                path.as_ref(),
                Some("org.freedesktop.login1.Session"),
                "SetBrightness",
                &(device.kind.subsystem(), device.name.as_str(), raw),
            )
            .await
            .map(|_| ())
            .map_err(|error| match &error {
                zbus::Error::MethodError(name, _, _) if crate::dbus::is_gone(name.as_str()) => {
                    BrightnessError::NoLogind(error.to_string())
                }
                _ => BrightnessError::Refused(error.to_string()),
            })
    }
}

#[cfg(test)]
mod tests;
