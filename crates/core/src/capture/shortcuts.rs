// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The keys of the Screenshots & Clipboard page: each a setting with the keys as sway writes them
//! (`Print`, `$mod+Shift+v`; empty for none), bound while the switch of its part is on.
//!
//! They become lines of `50-bindings.conf` after the media keys
//! ([`crate::bindings::ours::file_lines`]), with `--no-warn` like those, since switching a part
//! on means taking its keys; the page and the Keyboard Shortcuts page show what they take the
//! place of. Each runs the command line of Sway Things once per press (`--no-repeat`):
//! `exec "sway-things screenshot region"`.

use super::{
    HISTORY, HISTORY_KEY, KEY_ALL, KEY_DISPLAY, KEY_REGION, KEY_WINDOW, RECORDING, RECORDING_KEY,
    SCREENSHOTS,
};
use crate::bindings::ours::{self, FileLine};
use crate::bindings::{Action, Flags, Line, Origin, keys};
use crate::registry::ValueError;
use crate::state::State;

/// A key of the page: the setting with its keys, the switch that binds it, and the command line
/// of Sway Things it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    /// The key of the setting with the keys.
    pub key: &'static str,
    /// The key of the switch that binds it.
    pub switch: &'static str,
    /// The command line it runs.
    pub run: &'static str,
}

/// The keys of the page, in the order of its rows.
pub const SHORTCUTS: [Shortcut; 6] = [
    Shortcut {
        key: KEY_REGION,
        switch: SCREENSHOTS,
        run: "sway-things screenshot region",
    },
    Shortcut {
        key: KEY_WINDOW,
        switch: SCREENSHOTS,
        run: "sway-things screenshot window",
    },
    Shortcut {
        key: KEY_DISPLAY,
        switch: SCREENSHOTS,
        run: "sway-things screenshot display",
    },
    Shortcut {
        key: KEY_ALL,
        switch: SCREENSHOTS,
        run: "sway-things screenshot all",
    },
    Shortcut {
        key: HISTORY_KEY,
        switch: HISTORY,
        run: "sway-things clipboard pick",
    },
    Shortcut {
        key: RECORDING_KEY,
        switch: RECORDING,
        run: "sway-things record",
    },
];

/// The keys of the settings that change `50-bindings.conf`: the switches and the keys.
pub const KEYS: [&str; 9] = [
    SCREENSHOTS,
    KEY_REGION,
    KEY_WINDOW,
    KEY_DISPLAY,
    KEY_ALL,
    HISTORY,
    HISTORY_KEY,
    RECORDING,
    RECORDING_KEY,
];

impl Shortcut {
    /// The shortcut whose keys are the setting `key`.
    pub fn of(key: &str) -> Option<Self> {
        SHORTCUTS.into_iter().find(|shortcut| shortcut.key == key)
    }

    /// Its binding for the keys `text` with `$mod` as `modifier`; `None` for no keys or keys
    /// that can't be bound.
    pub fn line(self, text: &str, modifier: keys::Modifiers) -> Option<Line> {
        if text.is_empty() {
            return None;
        }
        let resolved = keys::with_mod(text, modifier);
        let combo = keys::parse(&resolved, false, false).ok()?;
        let command = Action::Run(self.run.to_owned()).command().ok()?;
        Some(Line {
            unbind: false,
            code: false,
            // Held down, a key would run its command again and again: screenshots in a row, a
            // recording started and stopped.
            flags: Flags {
                no_repeat: true,
                ..Flags::default()
            },
            input: None,
            keys: resolved,
            combo,
            command: Some(command.as_str().to_owned()),
        })
    }
}

/// The check of a setting with keys: empty, or keys that need no quotes (`$mod` for the main
/// modifier) and leave typing alone.
///
/// # Errors
///
/// [`ValueError::NotAKey`] for keys that can't be bound, [`ValueError::TypingKey`] for a key
/// that types without Super, Ctrl or Alt.
pub fn check_keys(text: &str) -> Result<(), ValueError> {
    if text.is_empty() {
        return Ok(());
    }
    if !keys::is_plain(text) {
        return Err(ValueError::NotAKey);
    }
    let combo = keys::parse(&keys::with_mod(text, keys::Modifiers::SUPER), false, false)
        .map_err(|_| ValueError::NotAKey)?;
    if combo.keys.is_empty() || combo.device == keys::Device::Mouse {
        return Err(ValueError::NotAKey);
    }
    if !keys::leaves_typing(&combo) {
        return Err(ValueError::TypingKey);
    }
    Ok(())
}

/// The bindings of the page's keys whose switch is on in `state`, as lines of
/// `50-bindings.conf`.
pub fn file_lines(state: &State) -> Vec<FileLine> {
    let modifier = ours::modifier(state);
    SHORTCUTS
        .into_iter()
        .filter(|shortcut| super::is_on(state, shortcut.switch))
        .filter_map(|shortcut| {
            let line = shortcut.line(&super::text(state, shortcut.key), modifier)?;
            Some(FileLine {
                origin: Origin::Feature(shortcut.key),
                line: Ok(line),
                no_warn: true,
            })
        })
        .collect()
}

/// The keys of the setting `key` in `state` as people read them (`Super + Shift + V`); empty
/// without keys.
pub fn label(state: &State, key: &str) -> String {
    let text = super::text(state, key);
    let Some(line) =
        Shortcut::of(key).and_then(|shortcut| shortcut.line(&text, ours::modifier(state)))
    else {
        return String::new();
    };
    line.labels(keys::Modifiers::NONE, None)
        .iter()
        .map(crate::i18n::Text::translated)
        .collect::<Vec<_>>()
        .join(" + ")
}
