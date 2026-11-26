// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! GTK's warnings that a widget was measured narrower or lower than its minimum (`Trying to
//! measure GtkStackSwitcher … for width of 369, but it needs at least 400`): a layout gave it
//! less room than it needs, so it is cut off or overlaps what is next to it. GTK only prints
//! them, so the checks collect them here and each group of checks fails on the ones it caused.

use std::sync::Mutex;

use gtk::glib::{self, LogField, LogLevel};

use super::Outcome;

/// The warnings since the last [`take`].
static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// What such a warning starts with.
const PREFIX: &str = "Trying to measure ";

/// Collects the warnings from now on, and still prints every message as `GLib` would. Call it
/// once, before GTK starts.
pub(crate) fn watch() {
    glib::log_set_writer_func(|level, fields| {
        if matches!(level, LogLevel::Warning | LogLevel::Critical) {
            let message = fields
                .iter()
                .find(|field| field.key() == "MESSAGE")
                .and_then(LogField::value_str);
            if let Some(message) = message.filter(|message| message.starts_with(PREFIX)) {
                if let Ok(mut warnings) = WARNINGS.lock() {
                    warnings.push(message.to_owned());
                }
            }
        }
        glib::log_writer_default(level, fields)
    });
}

/// The warnings since the last call, each once, without the widget's address.
fn take() -> Vec<String> {
    let Ok(mut warnings) = WARNINGS.lock() else {
        return Vec::new();
    };
    let mut seen: Vec<String> = Vec::new();
    for warning in warnings.drain(..) {
        let warning = without_address(&warning);
        if !seen.contains(&warning) {
            seen.push(warning);
        }
    }
    seen
}

/// `warning` without the pointer GTK names the widget by (`GtkWindow 0x5636…`), which differs
/// from run to run.
fn without_address(warning: &str) -> String {
    warning
        .split(' ')
        .filter(|word| !word.starts_with("0x"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `results` of the checks of `group`, and one more that fails if they measured a widget below
/// its minimum.
pub(crate) fn checked(group: &str, mut results: Vec<(String, Outcome)>) -> Vec<(String, Outcome)> {
    let warnings = take();
    let outcome = if warnings.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "  a layout gave a widget less room than its minimum:\n  {}",
            warnings.join("\n  ")
        ))
    };
    results.push((
        format!("{group}: every widget gets at least its minimum size"),
        outcome,
    ));
    results
}
