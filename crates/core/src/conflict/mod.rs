// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The lines of the user's sway config that set the same thing as Sway Things.
//!
//! sway runs its config from top to bottom, and the last line that sets something wins. A line of
//! the user's before the generated file of a setting is dead while Sway Things stores a value
//! for it; a line after it wins over Sway Things. [`Report::check`] finds these lines for one
//! setting, with the value each one sets, read as sway reads it.
//!
//! - **Top-level directives** (`focus_follows_mouse`, `gaps inner`) match by name and first
//!   argument.
//! - **`input` options** merge per device: for one device, the last line wins among those for
//!   `*`, for its type and for its identifier, in config order (sway's `store_input_config`
//!   merges each onto the others). For a whole input type, a line for one device is reported
//!   with that device; it doesn't decide the type's value.
//! - **`seat` options** count for `seat *` and the default seat, `seat0`, which are those Sway
//!   Things changes with `seat *`.
//! - **`output` settings** merge per output the same way: the last line wins among those for
//!   `*`, for the output's name and for its identifier (make, model and serial). A line with
//!   several settings (`output DP-4 mode 2560x1440 pos 0 0`) counts for each of them.
//! - **One directive, several settings** (the five colors of `client.focused`): a line sets all
//!   of them, so every setting of the group reports the same lines.
//!
//! The user's value is what their config alone sets: pages show it while Sway Things stores
//! nothing ("from ~/.config/sway/config:120").
//!
//! ```
//! use std::path::Path;
//! use sway_things_core::conflict::{Known, Order, Report};
//! use sway_things_core::generate::Locations;
//! use sway_things_core::include::IncludeStatus;
//! use sway_things_core::registry::{Registry, Value};
//! use sway_things_core::sway_config::MemoryFiles;
//!
//! let locations = Locations::new(
//!     Path::new("/home/me/.config/sway/sway-things"),
//!     Path::new("/home/me/.local/state/sway-things"),
//! );
//! let files = MemoryFiles::new().var("HOME", "/home/me").file(
//!     "/home/me/.config/sway/config",
//!     "gaps inner 4\ninclude ~/.config/sway/sway-things/*.conf\ngaps inner 12\n",
//! );
//! let status =
//!     IncludeStatus::detect(Some(Path::new("/home/me/.config/sway/config")), &files, &locations);
//! let gaps = Registry::builtin().get("windows.gaps.inner").ok_or("no such setting")?;
//!
//! let report = Report::check(&status, gaps, None, Known::default()).ok_or("not a sway setting")?;
//! // Line 1 is dead while Sway Things stores a value, line 3 wins over it.
//! assert_eq!(report.overridden().map(|line| line.line()).collect::<Vec<_>>(), [1]);
//! let winner = report.overriding().ok_or("no line after the include")?;
//! assert_eq!((winner.line(), winner.order), (3, Order::After));
//! assert_eq!(winner.value, Ok(Value::Integer(12)));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#[cfg(test)]
mod tests;

use std::path::Path;

use crate::devices::{Device, Devices};
use crate::include::IncludeStatus;
use crate::outputs::Outputs;
use crate::registry::{Block, ReadError, Registry, Scope, Setting, Value};
use crate::sway_config::Directive;

/// Where a user's line runs, relative to the generated file of the setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Before the generated file: Sway Things wins where it stores a value.
    Before,
    /// After the generated file: the user's line wins.
    After,
    /// The config doesn't include the generated file, so nothing Sway Things generates is in
    /// effect.
    NotIncluded,
}

/// The input devices and outputs of the session, as far as they are known. Lines of the user's
/// config that name them are matched with them: a line for a device of another type is left out,
/// and a line that names an output by its name counts for the output stored by its identifier.
#[derive(Debug, Clone, Copy, Default)]
pub struct Known<'a> {
    /// The input devices.
    pub devices: Option<&'a Devices>,
    /// The outputs.
    pub outputs: Option<&'a Outputs>,
}

/// Which devices or outputs a user's `input` or `output` line covers, of those the report is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// All of them: a top-level line, `input *`, the setting's `type:`, `output *`, or the
    /// identifier or name of the device or output the report is for.
    All,
    /// Only this device or output, in a report for a whole input type or for every output.
    Device(String),
}

/// A line of the user's config that sets the setting.
#[derive(Debug, Clone, PartialEq)]
pub struct UserLine {
    /// The directive: file, line, text and arguments.
    pub directive: Directive,
    /// Where it runs, relative to the generated file.
    pub order: Order,
    /// Which devices it covers.
    pub reach: Reach,
    /// The value it sets. A line sway rejects ([`ReadError::Rejected`]) sets nothing.
    pub value: Result<Value, ReadError>,
}

impl UserLine {
    /// The file the line is in.
    pub fn file(&self) -> &Path {
        &self.directive.file
    }

    /// The number of its first line, counted from 1.
    pub fn line(&self) -> usize {
        self.directive.line
    }

    /// The line as written.
    pub fn text(&self) -> &str {
        &self.directive.text
    }

    /// Whether sway takes the line, so it changes the setting.
    pub fn is_effective(&self) -> bool {
        !matches!(self.value, Err(ReadError::Rejected(_)))
    }

    /// Whether it sets the setting for everything the report is for.
    fn decides(&self) -> bool {
        self.reach == Reach::All && self.is_effective()
    }
}

/// The lines of the user's config that set one setting, for the whole session or input type, or
/// for one device.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    key: &'static str,
    device: Option<String>,
    lines: Vec<UserLine>,
}

impl Report {
    /// Finds the lines of the user's config, as `status` read it, that set `setting`: for the
    /// whole session, the setting's input type or every output, or for the input `device` or the
    /// output with this identifier. With the `known` devices, lines for devices of another type
    /// are left out; with the known outputs, lines that name an output by its name count for it.
    /// `None` for a preference of Sway Things itself.
    ///
    /// The search is linear in the length of the config, so run it again after each change: for
    /// one setting after it changed, for all of them ([`check_all`]) after the config or the
    /// settings files changed on disk.
    pub fn check(
        status: &IncludeStatus,
        setting: &'static Setting,
        device: Option<&str>,
        known: Known<'_>,
    ) -> Option<Self> {
        let mapping = setting.sway()?;
        let position = status.position(mapping.file());
        let block = mapping.directive().block;
        let mut lines = Vec::new();
        for (index, directive) in status.config().directives.iter().enumerate() {
            let order = match position {
                Some(position) if index < position => Order::Before,
                Some(_) => Order::After,
                None => Order::NotIncluded,
            };
            let Some(value) = setting.read(directive) else {
                continue;
            };
            let reach = match block {
                Block::Top => Some(Reach::All),
                Block::Input => reach(directive, setting.scope(), device, known.devices),
                Block::Seat => seat_reach(directive),
                Block::Output => reach_output(directive, device, known.outputs),
            };
            let Some(reach) = reach else { continue };
            lines.push(UserLine {
                directive: directive.clone(),
                order,
                reach,
                value,
            });
        }
        Some(Self {
            key: setting.key(),
            device: device.map(str::to_owned),
            lines,
        })
    }

    /// The key of the setting.
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// The device the report is for, or `None` for the whole session or input type.
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// Every line that sets the setting, in the order sway runs them, also those sway rejects.
    pub fn lines(&self) -> &[UserLine] {
        &self.lines
    }

    /// The line whose value the user's config alone sets: the last one sway takes that covers
    /// everything the report is for. Its value is in effect while Sway Things stores nothing.
    pub fn user_value(&self) -> Option<&UserLine> {
        self.lines.iter().rev().find(|line| line.decides())
    }

    /// The lines before the generated file that sway takes: dead while Sway Things stores a
    /// value. ("Overridden: your config line N is no longer in effect.")
    pub fn overridden(&self) -> impl Iterator<Item = &UserLine> {
        self.lines
            .iter()
            .filter(|line| line.order == Order::Before && line.is_effective())
    }

    /// The last line after the generated file that sway takes and that covers everything the
    /// report is for: it wins over Sway Things. ("Your config line N overrides this setting.")
    pub fn overriding(&self) -> Option<&UserLine> {
        self.lines
            .iter()
            .rev()
            .find(|line| line.order == Order::After && line.decides())
    }

    /// In a report for a whole input type or every output, the lines for single devices or
    /// outputs that sway takes. Each
    /// decides the value of its device where it comes after the others.
    pub fn for_devices(&self) -> impl Iterator<Item = &UserLine> {
        self.lines
            .iter()
            .filter(|line| matches!(line.reach, Reach::Device(_)) && line.is_effective())
    }
}

/// Which of the devices the report is for an `input` line covers, or `None` if none of them.
fn reach(
    directive: &Directive,
    scope: Scope,
    device: Option<&str>,
    devices: Option<&Devices>,
) -> Option<Reach> {
    let Scope::PerDevice(kind) = scope else {
        return None;
    };
    // sway compares identifiers and types with `strcmp`.
    let selector = directive.block.get(1)?;
    if selector == "*" {
        return Some(Reach::All);
    }
    if let Some(name) = selector.strip_prefix("type:") {
        return (name == kind.name()).then_some(Reach::All);
    }
    if let Some(device) = device {
        return (selector == device).then_some(Reach::All);
    }
    let other_type = devices
        .and_then(|devices| devices.get(selector))
        .and_then(Device::kind)
        .is_some_and(|known| known != kind);
    (!other_type).then(|| Reach::Device(selector.clone()))
}

/// Whether a `seat` line covers the seats Sway Things sets: `seat *` and the default seat do;
/// another seat isn't what the setting is about. sway compares seat names with `strcmp`.
fn seat_reach(directive: &Directive) -> Option<Reach> {
    let name = directive.block.get(1)?;
    (name == "*" || name == "seat0").then_some(Reach::All)
}

/// Which of the outputs the report is for an `output` line covers, or `None` if none of them.
/// sway compares names and identifiers exactly.
fn reach_output(
    directive: &Directive,
    output: Option<&str>,
    outputs: Option<&Outputs>,
) -> Option<Reach> {
    let selector = directive.block.get(1)?;
    if selector == "*" {
        return Some(Reach::All);
    }
    let Some(output) = output else {
        return Some(Reach::Device(selector.clone()));
    };
    let same = selector == output
        || outputs
            .and_then(|outputs| outputs.find(selector))
            .is_some_and(|named| named.identifier() == output);
    same.then_some(Reach::All)
}

/// Reports for every sway setting of `registry`: for the whole session, input type or every
/// output, and for each known device of a per-device setting's type and each known output.
pub fn check_all(
    status: &IncludeStatus,
    registry: &'static Registry,
    known: Known<'_>,
) -> Vec<Report> {
    let mut reports = Vec::new();
    for setting in registry.iter() {
        reports.extend(Report::check(status, setting, None, known));
        match (setting.scope(), known.devices, known.outputs) {
            (Scope::PerDevice(kind), Some(devices), _) => {
                for device in devices.of_type(kind) {
                    reports.extend(Report::check(
                        status,
                        setting,
                        Some(device.identifier()),
                        known,
                    ));
                }
            }
            (Scope::PerOutput, _, Some(outputs)) => {
                for output in outputs.iter() {
                    reports.extend(Report::check(
                        status,
                        setting,
                        Some(output.identifier()),
                        known,
                    ));
                }
            }
            _ => {}
        }
    }
    reports
}
