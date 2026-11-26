// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The brightness of external monitors over DDC/CI, through `ddcutil`.
//!
//! DDC/CI lets a program set a monitor's brightness (VCP feature `0x10`) over the video cable's
//! I²C bus, `/dev/i2c-<n>`. That needs read and write access to the bus (the `i2c` group, or a
//! udev rule) and the `i2c-dev` module; [`access`] tells which is missing. Each call takes from
//! 50 to 500 ms, so the page finds the monitors in the background ([`detect`]) and sends a new
//! value only once the slider stands still. Monitors are addressed by their bus, which stays the
//! same while they are connected. A built-in panel (eDP, LVDS, DSI) is left out: its backlight is
//! a [`super::Device`].
//!
//! Every function runs `ddcutil` with an argument list and waits for it: run them off the main
//! loop.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::delegate::find_program;
use crate::i18n::Text;
use crate::msg;

/// The program, and the package that has it.
pub const PROGRAM: &str = "ddcutil";

/// The VCP feature of the brightness.
const BRIGHTNESS: &str = "10";

/// An external monitor that answers over DDC/CI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    /// The number of its I²C bus, `/dev/i2c-<bus>`.
    pub bus: u32,
    /// Its model as its EDID names it (`DELL U2720Q`), else its maker; cleaned of control
    /// characters, shown only as plain text.
    pub name: String,
    /// The connector the graphics driver names (`DP-1`), if ddcutil tells it.
    pub connector: Option<String>,
}

/// The brightness of a monitor: now and its highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    /// The value now.
    pub current: u32,
    /// The highest value, usually 100.
    pub max: u32,
}

impl Level {
    /// The brightness in percent.
    pub fn percent(self) -> u32 {
        super::percent_of(self.current, self.max)
    }
}

/// Why the monitors can't be reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdcError {
    /// ddcutil isn't installed.
    Missing,
    /// ddcutil couldn't be run or failed; its last line says why.
    Failed(String),
    /// The monitor's answer wasn't a brightness.
    Unreadable(String),
}

impl DdcError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Missing => Text::new(msg!("ddcutil isn't installed")),
            Self::Failed(error) => Text::new(msg!("ddcutil failed: {error}")).arg("error", error),
            Self::Unreadable(answer) => {
                Text::new(msg!("The monitor's answer isn't a brightness: {answer}"))
                    .arg("answer", answer)
            }
        }
    }
}

impl std::fmt::Display for DdcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for DdcError {}

/// Whether the I²C buses can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// At least one bus can be read and written.
    Granted,
    /// There are buses, but none can be read and written: the user isn't in the group that owns
    /// them (`i2c` on most distributions) and no udev rule gives access.
    Denied,
    /// There are no `/dev/i2c-*`: the `i2c-dev` module isn't loaded.
    NoBuses,
}

/// Where ddcutil is installed, if it is.
pub fn find() -> Option<PathBuf> {
    find_program(PROGRAM, std::env::var_os("PATH").as_deref())
}

/// Whether the I²C buses can be used: opens each `/dev/i2c-*` for reading and writing, which
/// sends nothing to the monitor.
///
/// # Blocking
///
/// Reads `/dev` for the I²C devices.
pub fn access() -> Access {
    access_in(Path::new("/dev"))
}

/// [`access`] for the devices in `dev`.
///
/// # Blocking
///
/// Reads `/dev` for the I²C devices.
pub fn access_in(dev: &Path) -> Access {
    let Ok(entries) = std::fs::read_dir(dev) else {
        return Access::NoBuses;
    };
    let buses: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("i2c-"))
        .map(|entry| entry.path())
        .collect();
    if buses.is_empty() {
        return Access::NoBuses;
    }
    let usable = buses.iter().any(|bus| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(bus)
            .is_ok()
    });
    if usable {
        Access::Granted
    } else {
        Access::Denied
    }
}

/// Runs ddcutil with `args` and returns what it printed.
///
/// # Blocking
///
/// Waits for ddcutil, which talks to the monitor and can take seconds.
fn run(args: &[OsString]) -> Result<String, DdcError> {
    let program = find().ok_or(DdcError::Missing)?;
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| DdcError::Failed(error.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let said = last_line(&stderr)
        .or_else(|| last_line(&stdout))
        .unwrap_or_else(|| output.status.to_string());
    Err(DdcError::Failed(said))
}

/// The last line of `text` that isn't blank, trimmed.
fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(clean)
}

/// `text` without control characters.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The external monitors that answer over DDC/CI (`ddcutil detect --brief`).
///
/// # Blocking
///
/// Waits for ddcutil, which talks to the monitor and can take seconds.
///
/// # Errors
///
/// [`DdcError::Missing`] without ddcutil, [`DdcError::Failed`] if it fails.
pub fn detect() -> Result<Vec<Monitor>, DdcError> {
    let output = run(&["detect".into(), "--brief".into()])?;
    Ok(parse_detect(&output))
}

/// The monitors in what `ddcutil detect --brief` printed: each `Display <n>` block with its
/// `I2C bus`, `DRM connector` and `Monitor` (`MFG:MODEL:SERIAL`) lines. Blocks of `Invalid
/// display` or `Phantom display` and built-in panels are left out.
pub fn parse_detect(output: &str) -> Vec<Monitor> {
    let mut monitors = Vec::new();
    let mut current: Option<(Option<u32>, Option<String>, Option<String>)> = None;
    let mut finish = |block: Option<(Option<u32>, Option<String>, Option<String>)>| {
        if let Some((Some(bus), connector, name)) = block {
            let internal = connector.as_deref().is_some_and(|connector| {
                ["eDP", "LVDS", "DSI"]
                    .iter()
                    .any(|kind| connector.starts_with(kind))
            });
            if !internal {
                monitors.push(Monitor {
                    bus,
                    name: name.unwrap_or_else(|| format!("/dev/i2c-{bus}")),
                    connector,
                });
            }
        }
    };
    for line in output.lines() {
        if !line.starts_with(char::is_whitespace) {
            finish(current.take());
            if line.trim_start().starts_with("Display ") {
                current = Some((None, None, None));
            }
            continue;
        }
        let Some(block) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().replace('_', " ").as_str() {
            "i2c bus" => {
                block.0 = value
                    .strip_prefix("/dev/i2c-")
                    .and_then(|bus| bus.parse().ok());
            }
            "drm connector" => {
                // `card1-DP-1`: the connector after the card.
                let connector = value
                    .split_once('-')
                    .filter(|(card, _)| card.starts_with("card"))
                    .map_or(value, |(_, connector)| connector);
                block.1 = Some(clean(connector)).filter(|connector| !connector.is_empty());
            }
            "monitor" => {
                let mut parts = value.split(':');
                let maker = parts.next().map(clean).unwrap_or_default();
                let model = parts.next().map(clean).unwrap_or_default();
                block.2 = [model, maker].into_iter().find(|name| !name.is_empty());
            }
            _ => {}
        }
    }
    finish(current.take());
    monitors
}

/// The brightness of the monitor on `bus` (`ddcutil getvcp 10 --bus <n> --brief`).
///
/// # Blocking
///
/// Waits for ddcutil, which talks to the monitor and can take seconds.
///
/// # Errors
///
/// [`DdcError::Missing`], [`DdcError::Failed`], [`DdcError::Unreadable`] for an answer that
/// isn't a brightness.
pub fn get(bus: u32) -> Result<Level, DdcError> {
    let output = run(&[
        "getvcp".into(),
        BRIGHTNESS.into(),
        "--bus".into(),
        bus.to_string().into(),
        "--brief".into(),
    ])?;
    parse_level(&output).ok_or_else(|| DdcError::Unreadable(clean(&output)))
}

/// The brightness in what `ddcutil getvcp 10 --brief` printed: `VCP 10 C <current> <max>`.
pub fn parse_level(output: &str) -> Option<Level> {
    output.lines().find_map(|line| {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["VCP", feature, "C", current, max] if feature.eq_ignore_ascii_case(BRIGHTNESS) => {
                let level = Level {
                    current: current.parse().ok()?,
                    max: max.parse().ok()?,
                };
                (level.max > 0).then_some(level)
            }
            _ => None,
        }
    })
}

/// Sets the brightness of the monitor on `bus` to `value` (`ddcutil setvcp 10 <value> --bus
/// <n>`).
///
/// # Blocking
///
/// Waits for ddcutil, which talks to the monitor and can take seconds.
///
/// # Errors
///
/// [`DdcError::Missing`], [`DdcError::Failed`].
pub fn set(bus: u32, value: u32) -> Result<(), DdcError> {
    run(&[
        "setvcp".into(),
        BRIGHTNESS.into(),
        value.to_string().into(),
        "--bus".into(),
        bus.to_string().into(),
    ])
    .map(|_| ())
}
