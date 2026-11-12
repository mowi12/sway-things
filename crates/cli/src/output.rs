// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What `get` prints: a line per setting for people, JSON for scripts.

use std::io::Write;
use std::path::PathBuf;

use serde_json::{Value as Json, json};
use sway_things_core::conflict::Report;
use sway_things_core::generate::InvalidLine;
use sway_things_core::include::IncludeStatus;
use sway_things_core::ipc::Compositor;
use sway_things_core::registry::{Setting, Value};
use sway_things_core::state::{Layer, Paths, State};
use sway_things_core::view::{Locked, SettingView, Source};

/// A setting as `get` shows it: what a row of the app shows, and the line of the user's config
/// that wins over the stored value, if one does.
pub(crate) struct Shown {
    key: &'static str,
    device: Option<String>,
    view: SettingView,
    overriding: Option<(PathBuf, usize)>,
}

impl Shown {
    pub(crate) fn of(
        setting: &'static Setting,
        device: Option<&str>,
        state: &State,
        status: &IncludeStatus,
        compositor: Compositor,
    ) -> Self {
        let view = SettingView::of(setting, device, state, status, compositor);
        let overriding = if matches!(view.source, Source::Stored { .. }) {
            Report::check(status, setting, device, None).and_then(|report| {
                report
                    .overriding()
                    .map(|line| (line.file().to_owned(), line.line()))
            })
        } else {
            None
        };
        Self {
            key: setting.key(),
            device: device.map(str::to_owned),
            view,
            overriding,
        }
    }

    /// `key = value  (where it comes from)`.
    pub(crate) fn text(&self, paths: &Paths) -> String {
        format!(
            "{} = {}  ({})",
            name(self.key, self.device.as_deref()),
            self.view.value,
            self.notes(paths).join("; ")
        )
    }

    /// The same without the device, for a list grouped by device.
    fn text_in_group(&self, paths: &Paths) -> String {
        format!(
            "{} = {}  ({})",
            self.key,
            self.view.value,
            self.notes(paths).join("; ")
        )
    }

    fn notes(&self, paths: &Paths) -> Vec<String> {
        let mut notes = vec![match &self.view.source {
            Source::Stored { layer, for_type } => {
                let file = paths.relative(*layer).display().to_string();
                if *for_type {
                    format!("{file}, for the input type")
                } else {
                    file
                }
            }
            Source::Config { file, line } => format!("your config: {}:{line}", file.display()),
            Source::Default => "default".to_owned(),
        }];
        if let Some((file, line)) = &self.overriding {
            notes.push(format!("your config wins: {}:{line}", file.display()));
        }
        if let Some(Locked::Unsupported(unmet)) = &self.view.locked {
            notes.push(unmet.to_string());
        }
        notes
    }

    pub(crate) fn json(&self, paths: &Paths) -> Json {
        let source = match &self.view.source {
            Source::Stored { layer, for_type } => json!({
                "kind": match layer {
                    Layer::Shared => "shared",
                    Layer::Host => "host",
                },
                "file": paths.path(*layer).display().to_string(),
                "for_type": for_type,
            }),
            Source::Config { file, line } => json!({
                "kind": "config",
                "file": file.display().to_string(),
                "line": line,
            }),
            Source::Default => json!({ "kind": "default" }),
        };
        let overridden_by = self
            .overriding
            .as_ref()
            .map(|(file, line)| json!({ "file": file.display().to_string(), "line": line }));
        let unsupported = match &self.view.locked {
            Some(Locked::Unsupported(unmet)) => Some(unmet.to_string()),
            _ => None,
        };
        json!({
            "key": self.key,
            "device": self.device,
            "value": value(&self.view.value),
            "source": source,
            "line": self.view.line,
            "overridden_by": overridden_by,
            "unsupported": unsupported,
        })
    }
}

/// Every setting of `all`, those of the whole session first, then those of each device under it.
pub(crate) fn list(all: &[Shown], paths: &Paths, out: &mut dyn Write) {
    let mut device = None;
    for shown in all {
        if shown.device != device {
            device.clone_from(&shown.device);
            if let Some(device) = &device {
                let _ = writeln!(out, "\ndevice {device}");
            }
        }
        if shown.device.is_some() {
            let _ = writeln!(out, "  {}", shown.text_in_group(paths));
        } else {
            let _ = writeln!(out, "{}", shown.text(paths));
        }
    }
}

/// The setting `key` as the user names it: with `--device`, if it is for one.
pub(crate) fn name(key: &str, device: Option<&str>) -> String {
    match device {
        Some(device) => format!("{key} --device {device}"),
        None => key.to_owned(),
    }
}

/// A line of a generated file that sway rejected, with the settings it comes from.
pub(crate) fn invalid_line(line: &InvalidLine) -> String {
    let place = match line.line {
        0 => String::new(),
        number => format!("line {number}: "),
    };
    let source = line
        .source
        .as_ref()
        .map(|source| {
            let keys = source.keys.join(", ");
            format!(" (from {})", name(&keys, source.device.as_deref()))
        })
        .unwrap_or_default();
    format!("{place}{}{source}", line.message)
}

/// A value as JSON: a boolean, a number, a list of strings, or a string for the others.
fn value(value: &Value) -> Json {
    match value {
        Value::Bool(value) => json!(value),
        Value::Integer(value) => json!(value),
        Value::Decimal(value) => json!(value),
        Value::List(items) => json!(items),
        Value::Choice(_) | Value::Color(_) | Value::Text(_) => json!(value.to_string()),
    }
}
