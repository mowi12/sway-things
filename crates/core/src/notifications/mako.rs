// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Our file for mako, `$XDG_CONFIG_HOME/mako/sway-things`, in mako's own format (`mako(5)`).
//!
//! mako reads an included file as if its lines stood at the `include=` line, starting in the
//! global section: our options before the first `[criteria]` are global and win over the
//! user's global options above the line; our sections come after the user's sections above it
//! and before those below it, and where several sections match, the last one wins. `include=`
//! is only taken among the global options, so the user puts the line before their first
//! section. The file always has `[mode=do-not-disturb] invisible=1`, which Do Not Disturb
//! switches on with the mode.

use std::fmt::Write as _;

use super::{HEADER, Stored};
use crate::command::Hook;
use crate::registry::notifications::{self as keys, mako};

/// The section that hides every notification while Do Not Disturb is on.
pub const DND_SECTION: &str = "[mode=do-not-disturb]";

/// What mako runs on a left click while clicking shows the app: our hook, with the id mako sets
/// as `$id` in the shell it runs the command in. It invokes the default action itself.
pub fn click_binding() -> String {
    format!(
        "on-button-left=exec {}",
        Hook::NotificationClick.command_line(&["--mako", "\"$id\""])
    )
}

/// The text of our file for the values in `stored`.
pub fn generate(stored: &Stored<'_>) -> String {
    let mut text =
        format!("# {HEADER}\n# mako reads it through the include= line of its config.\n");
    let mut line = |line: String| {
        text.push_str(&line);
        text.push('\n');
    };
    if let Some(position) = stored.text(keys::POSITION) {
        line(format!("anchor={position}"));
    }
    if let Some(output) = stored.text(keys::OUTPUT) {
        line(format!("output={output}"));
    }
    for (key, option) in [
        (mako::WIDTH, "width"),
        (mako::HEIGHT, "height"),
        (mako::MAX_VISIBLE, "max-visible"),
        (mako::BORDER_SIZE, "border-size"),
        (mako::BORDER_RADIUS, "border-radius"),
    ] {
        if let Some(value) = stored.integer(key) {
            line(format!("{option}={value}"));
        }
    }
    if let Some(seconds) = stored.integer(mako::TIMEOUT) {
        line(format!("default-timeout={}", seconds * 1000));
    }
    // Without grouping is mako's own default, so only grouping is written.
    if stored.bool(mako::GROUP) == Some(true) {
        line("group-by=app-name".to_owned());
    }
    if let Some(icons) = stored.bool(mako::ICONS) {
        line(format!("icons={}", u8::from(icons)));
    }
    if let Some(font) = stored.text(mako::FONT) {
        line(format!("font={font}"));
    }
    if let Some(colors) = stored.colors() {
        line(format!(
            "background-color={}",
            with_alpha(colors.background)
        ));
        line(format!("text-color={}", with_alpha(colors.text)));
        line(format!("border-color={}", with_alpha(colors.border)));
        line(format!("progress-color=over {}", with_alpha(colors.border)));
    }
    if stored.bool(keys::CLICK_FOCUS) == Some(true) {
        line(click_binding());
    }
    if let Some(seconds) = stored.integer(mako::CRITICAL_TIMEOUT) {
        line(String::new());
        line("[urgency=critical]".to_owned());
        line(format!("default-timeout={}", seconds * 1000));
    }
    let _ = write!(text, "\n{DND_SECTION}\ninvisible=1\n");
    text
}

/// A color as mako writes it: `#RRGGBBAA`.
fn with_alpha(color: crate::command::Color) -> String {
    let [red, green, blue] = color.channels();
    format!(
        "#{red:02x}{green:02x}{blue:02x}{:02x}",
        color.alpha().unwrap_or(0xff)
    )
}

/// Whether `text`, a mako config, hides notifications in the mode `do-not-disturb` itself.
pub fn has_dnd_section(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        line.starts_with('[')
            && line.ends_with(']')
            && line[1..line.len() - 1]
                .split_whitespace()
                .any(|field| field == "mode=do-not-disturb")
    })
}
