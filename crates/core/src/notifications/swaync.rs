// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The handover for swaync: our own complete `config.json` and `style.css` in
//! `$XDG_CONFIG_HOME/sway-things/swaync/`, which swaync reads when started with `-c` and `-s`.
//!
//! - **config.json** starts from swaync's own default config (the installed one in
//!   `$XDG_CONFIG_DIRS/swaync/config.json`, else the one swaync 0.12.6 ships), then the options
//!   an import passed through (`notifications.swaync.other`), then the stored settings. With
//!   nothing stored it says what swaync's default says. Its first member, `$comment`, marks it
//!   as ours; swaync ignores members it doesn't know.
//! - **style.css** goes on top of swaync's packaged style sheet, which swaync always loads first:
//!   with nothing stored it is only a comment, so swaync looks as it does by itself. The
//!   frosted preset and the colors and corners of the page come after it.
//! - [`import`] reads a swaync config (the user's) as a starting point: the options that have
//!   a setting become stored values, the others are passed through as they are.

use std::fmt::Write as _;

use serde_json::{Map, Value as Json, json};

use super::{HEADER, Stored};
use crate::command::{Hook, QuickSwitch};
use crate::registry::notifications::{self as keys, swaync};
use crate::registry::{Registry, Value};

/// swaync's default config, as swaync 0.12.6 ships it.
const DEFAULT_CONFIG: &str = include_str!("swaync-config.json");

/// The frosted preset.
pub const FROSTED: &str = include_str!("frosted.css");

/// The id of the script that runs our hook when a notification is clicked.
pub const CLICK_SCRIPT: &str = "sway-things-click";

/// The options of swaync's config that are settings here, and their keys.
const OPTIONS: &[(&str, &str)] = &[
    ("notification-window-width", swaync::WIDTH),
    ("timeout", swaync::TIMEOUT),
    ("timeout-low", swaync::TIMEOUT_LOW),
    ("timeout-critical", swaync::TIMEOUT_CRITICAL),
    ("notification-grouping", swaync::GROUPING),
    ("control-center-width", swaync::PANEL_WIDTH),
    ("control-center-height", swaync::PANEL_HEIGHT),
    ("widgets", swaync::WIDGETS),
    ("notification-window-preferred-output", keys::OUTPUT),
];

/// swaync's default config: the installed one, else the one swaync 0.12.6 ships.
pub fn default_config() -> Map<String, Json> {
    let dirs = std::env::var("XDG_CONFIG_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/etc/xdg".to_owned());
    dirs.split(':')
        .filter(|dir| dir.starts_with('/'))
        .map(|dir| std::path::Path::new(dir).join("swaync/config.json"))
        .find_map(|path| read_object(&std::fs::read_to_string(path).ok()?))
        .or_else(|| read_object(DEFAULT_CONFIG))
        .unwrap_or_default()
}

/// `text` as a JSON object, if it is one.
fn read_object(text: &str) -> Option<Map<String, Json>> {
    match serde_json::from_str(text).ok()? {
        Json::Object(object) => Some(object),
        _ => None,
    }
}

/// The position as swaync takes it: `positionX` and `positionY`.
fn position(id: &str) -> (&'static str, &'static str) {
    let y = if id.starts_with("bottom") {
        "bottom"
    } else if id.starts_with("top") {
        "top"
    } else {
        "center"
    };
    let x = if id.ends_with("right") {
        "right"
    } else if id.ends_with("left") {
        "left"
    } else {
        "center"
    };
    (x, y)
}

/// A button of the `buttons-grid` widget: our hook, so no shell code goes into the config.
fn button(preset: &str) -> Option<Json> {
    if preset == "settings" {
        return Some(json!({ "label": "Settings", "command": "sway-things open notifications" }));
    }
    let switch = QuickSwitch::from_name(preset)?;
    let label = match switch {
        QuickSwitch::Dnd => "Do Not Disturb",
        QuickSwitch::Wifi => "Wi-Fi",
        QuickSwitch::Bluetooth => "Bluetooth",
        QuickSwitch::Sound => "Sound",
        QuickSwitch::Microphone => "Microphone",
    };
    Some(json!({
        "label": label,
        "type": "toggle",
        "command": switch.toggle_line(),
        "update-command": switch.state_line(),
    }))
}

/// The text of our `config.json` for the values in `stored`, on top of `base` (swaync's
/// default config, [`default_config`]).
pub fn config(stored: &Stored<'_>, mut base: Map<String, Json>) -> String {
    let mut config = Map::new();
    config.insert("$comment".to_owned(), Json::from(HEADER));
    base.remove("$comment");
    config.append(&mut base);
    if let Some(Ok(Json::Object(other))) = stored
        .text(swaync::OTHER)
        .filter(|text| !text.trim().is_empty())
        .map(serde_json::from_str::<Json>)
    {
        for (key, value) in other {
            if key != "$comment" {
                config.insert(key, value);
            }
        }
    }
    if let Some(id) = stored.text(keys::POSITION) {
        let (x, y) = position(id);
        config.insert("positionX".to_owned(), Json::from(x));
        config.insert("positionY".to_owned(), Json::from(y));
    }
    for (option, key) in OPTIONS {
        let value = match stored.get(key) {
            Some(Value::Integer(value)) => Json::from(*value),
            Some(Value::Bool(value)) => Json::from(*value),
            // An empty display is the focused one, which swaync says without the option.
            Some(Value::Text(value)) if value.is_empty() => {
                config.remove(*option);
                continue;
            }
            Some(Value::Text(value)) => Json::from(value.as_str()),
            Some(Value::List(items)) => Json::from(items.clone()),
            _ => continue,
        };
        config.insert((*option).to_owned(), value);
    }
    if let Some(buttons) = stored.list(swaync::BUTTONS) {
        let widgets = config
            .entry("widget-config")
            .or_insert_with(|| Json::Object(Map::new()));
        if let Json::Object(widgets) = widgets {
            let actions: Vec<Json> = buttons.iter().filter_map(|name| button(name)).collect();
            let per_row = actions.len().clamp(1, 4);
            widgets.insert(
                "buttons-grid".to_owned(),
                json!({ "buttons-per-row": per_row, "actions": actions }),
            );
        }
    }
    if stored.bool(keys::CLICK_FOCUS) == Some(true) {
        let scripts = config
            .entry("scripts")
            .or_insert_with(|| Json::Object(Map::new()));
        if let Json::Object(scripts) = scripts {
            scripts.insert(
                CLICK_SCRIPT.to_owned(),
                json!({
                    "exec": Hook::NotificationClick.command_line(&[]),
                    "app-name": ".*",
                    "run-on": "action",
                }),
            );
        }
    }
    let mut text = serde_json::to_string_pretty(&Json::Object(config)).unwrap_or_default();
    text.push('\n');
    text
}

/// The GTK swaync is built with, which decides what its style sheet can say: GTK 4 from
/// swaync 0.12 on, GTK 3 before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Gtk {
    /// swaync 0.11 and older: named colors (`@define-color`) only. GTK 3 rejects the whole
    /// style sheet over a CSS variable.
    Three,
    /// swaync 0.12 and newer: CSS variables, and the frosted preset.
    #[default]
    Four,
}

impl Gtk {
    /// The GTK of the swaync with the version `version` (`0.12.6`); GTK 4 for one that can't be
    /// read.
    pub fn of_version(version: &str) -> Self {
        let version = version.trim().trim_start_matches(['v', 'V']);
        let mut parts = version.split(['.', '-']).map(str::parse::<u32>);
        match (parts.next(), parts.next()) {
            (Some(Ok(0)), Some(Ok(minor))) if minor < 12 => Self::Three,
            _ => Self::Four,
        }
    }

    /// The GTK of the installed swaync, from `swaync --version` (`swaync 0.12.6`), asked once;
    /// GTK 4 if it can't be told.
    ///
    /// # Blocking
    ///
    /// Waits for `swaync --version`.
    pub fn installed() -> Self {
        static INSTALLED: std::sync::OnceLock<Gtk> = std::sync::OnceLock::new();
        *INSTALLED.get_or_init(Self::ask)
    }

    /// # Blocking
    ///
    /// Waits for `swaync --version`.
    fn ask() -> Self {
        std::process::Command::new("swaync")
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|text| text.split_whitespace().nth(1).map(Self::of_version))
            .unwrap_or_default()
    }
}

/// The text of our `style.css` for the values in `stored`, for a swaync with `gtk`.
pub fn style(stored: &Stored<'_>, gtk: Gtk) -> String {
    let mut text = format!(
        "/* {HEADER}\n   swaync loads its own style sheet first; this goes on top of it. */\n"
    );
    if stored.text(swaync::STYLE) == Some("frosted") && gtk == Gtk::Four {
        // The preset without its license header, which is about the preset's source.
        let rules = FROSTED.split_once("*/").map_or(FROSTED, |(_, rules)| rules);
        text.push_str(rules);
    }
    let mut root = Vec::new();
    if let Some(colors) = stored.colors() {
        // swaync 0.11 (GTK 3) names its colors; 0.12 (GTK 4) uses the variables below.
        let _ = write!(
            text,
            "\n@define-color noti-bg {};\n@define-color text-color {};\n\
             @define-color noti-border-color {};\n@define-color bg-selected {};\n",
            css_color(colors.background),
            css_color(colors.text),
            css_color(colors.border),
            css_color(colors.border)
        );
        let [red, green, blue] = colors.background.channels();
        root.push(format!("--noti-bg: {red}, {green}, {blue};"));
        root.push(format!(
            "--noti-bg-alpha: {:.2};",
            f64::from(colors.background.alpha().unwrap_or(0xff)) / 255.0
        ));
        root.push(format!("--text-color: {};", css_color(colors.text)));
        root.push(format!(
            "--noti-border-color: {};",
            css_color(colors.border)
        ));
        root.push(format!("--bg-selected: {};", css_color(colors.border)));
    }
    if let Some(radius) = stored.integer(swaync::CORNER_RADIUS) {
        root.push(format!("--border-radius: {radius}px;"));
    }
    if !root.is_empty() && gtk == Gtk::Four {
        text.push_str("\n:root {\n");
        for line in root {
            text.push_str("  ");
            text.push_str(&line);
            text.push('\n');
        }
        text.push_str("}\n");
    }
    text
}

/// A color in CSS: `rgba(r, g, b, a)`.
fn css_color(color: crate::command::Color) -> String {
    let [red, green, blue] = color.channels();
    format!(
        "rgba({red}, {green}, {blue}, {:.2})",
        f64::from(color.alpha().unwrap_or(0xff)) / 255.0
    )
}

/// What [`import`] found in a swaync config.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Imported {
    /// The settings to store, with their values.
    pub values: Vec<(&'static str, Value)>,
    /// The options without a setting that differ from swaync's default, as a JSON object for
    /// `notifications.swaync.other`; empty if there are none.
    pub other: String,
}

/// Reads `text`, a swaync `config.json`, as a starting point: the options that have a setting
/// and differ from its default become values to store, and the other options that differ from
/// `base` (swaync's default) are passed through. Nothing is written.
///
/// # Errors
///
/// If `text` isn't a JSON object.
pub fn import(text: &str, base: &Map<String, Json>) -> Result<Imported, serde_json::Error> {
    let config: Map<String, Json> = serde_json::from_str(text)?;
    let registry = Registry::builtin();
    let mut imported = Imported::default();
    let mut other = Map::new();
    let mut store = |key: &'static str, value: Value| {
        let fits = registry.get(key).is_some_and(|setting| {
            setting.check(&value).is_ok() && setting.default_value() != value
        });
        if fits {
            imported.values.push((key, value));
        }
        fits
    };
    let x = config.get("positionX").and_then(Json::as_str);
    let y = config.get("positionY").and_then(Json::as_str);
    let positioned = match (x, y) {
        (Some(x), Some(y)) => {
            let id = match (y, x) {
                ("center", "center") => "center".to_owned(),
                (y, x) => format!("{y}-{x}"),
            };
            registry
                .get(keys::POSITION)
                .is_some_and(|setting| setting.check(&Value::Choice(id.clone())).is_ok())
                .then(|| {
                    store(keys::POSITION, Value::Choice(id));
                })
                .is_some()
        }
        _ => false,
    };
    for (option, value) in &config {
        if option == "$schema" || option == "$comment" {
            continue;
        }
        if positioned && (option == "positionX" || option == "positionY") {
            continue;
        }
        if let Some((_, key)) = OPTIONS.iter().find(|(name, _)| name == option) {
            let as_value = match value {
                Json::Bool(on) => Some(Value::Bool(*on)),
                Json::Number(number) => number.as_i64().map(Value::Integer),
                Json::String(text) => Some(Value::Text(text.clone())),
                Json::Array(items) => items
                    .iter()
                    .map(|item| item.as_str().map(ToOwned::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .map(Value::List),
                _ => None,
            };
            let taken = as_value.is_some_and(|value| {
                registry
                    .get(key)
                    .is_some_and(|setting| setting.check(&value).is_ok())
                    && (store(key, value.clone())
                        || registry
                            .get(key)
                            .is_some_and(|s| s.default_value() == value))
            });
            if taken {
                continue;
            }
        }
        if base.get(option) != Some(value) {
            other.insert(option.clone(), value.clone());
        }
    }
    if !other.is_empty() {
        imported.other = serde_json::to_string_pretty(&Json::Object(other))?;
    }
    Ok(imported)
}
