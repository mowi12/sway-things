// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Screenshots, the clipboard history and screen recording: the Screenshots & Clipboard page.
//!
//! Sway has none of these itself. Its keys call the command line of Sway Things
//! (`bindsym Print exec "sway-things screenshot region"`), which runs the tools that do the work
//! (grim, slurp, wl-copy, cliphist, a launcher, wf-recorder or wl-screenrec, swappy or satty)
//! with argument lists, never through a shell, and ends again:
//!
//! - [`shortcuts`]: the keys, which go to `50-bindings.conf` with those of the Keyboard
//!   Shortcuts page ([`crate::bindings::ours::file_lines`]), so that page lists them and finds
//!   their clashes.
//! - [`name`]: the folder and the file names, from a pattern with date and time.
//! - [`shot`]: what a screenshot captures and where it goes.
//! - [`record`]: a recording that the same key starts and stops.
//! - [`clipboard`]: the history (`wl-paste --watch cliphist store`, started detached for the
//!   session like `wl-mirror`), picking an entry and clearing it.
//! - [`notify`]: the notification afterwards, with _Open_ and _Edit_.
//! - [`tools`]: which programs each part needs, and the launchers and editors that are installed.
//!
//! The watcher and the recorder are found again in `/proc` with [`crate::session::process`].

pub mod clipboard;
pub mod name;
pub mod notify;
pub mod record;
pub mod shortcuts;
pub mod shot;
pub mod tools;

pub use crate::registry::capture::{
    AFTERWARDS, DEFAULT_FILE_NAME, DEFAULT_RECORDING_FILE_NAME, DESTINATION, FILE_NAME, FOLDER,
    HISTORY, HISTORY_KEY, KEY_ALL, KEY_DISPLAY, KEY_REGION, KEY_WINDOW, MAX_ENTRIES, RECORDING,
    RECORDING_AREA, RECORDING_AUDIO, RECORDING_FILE_NAME, RECORDING_KEY, SCREENSHOTS,
};

use std::path::PathBuf;

use crate::i18n::Text;
use crate::registry::{Registry, Value};
use crate::state::State;

/// The value of `key` in effect in `state`: the stored one, else the default.
pub fn value(state: &State, key: &str) -> Option<Value> {
    state.get(key, None).cloned().or_else(|| {
        Registry::builtin()
            .get(key)
            .map(crate::registry::Setting::default_value)
    })
}

/// The text of `key` in effect in `state`; empty for a setting of another type.
pub fn text(state: &State, key: &str) -> String {
    match value(state, key) {
        Some(Value::Text(text) | Value::Choice(text)) => text,
        _ => String::new(),
    }
}

/// Whether the switch `key` is on in `state`.
pub fn is_on(state: &State, key: &str) -> bool {
    value(state, key) == Some(Value::Bool(true))
}

/// Where a screenshot goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// Copied to the clipboard and saved to the folder.
    Both,
    /// Only copied to the clipboard.
    Clipboard,
    /// Only saved to the folder.
    Folder,
}

impl Destination {
    /// The destination of the choice `id`; both for an unknown one.
    pub fn of(id: &str) -> Self {
        match id {
            "clipboard" => Self::Clipboard,
            "folder" => Self::Folder,
            _ => Self::Both,
        }
    }

    /// Whether the picture is saved to the folder.
    pub fn saves(self) -> bool {
        self != Self::Clipboard
    }

    /// Whether the picture is copied to the clipboard.
    pub fn copies(self) -> bool {
        self != Self::Folder
    }
}

/// What happens after a screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Afterwards {
    /// A notification with the picture, _Open_ and _Edit_.
    Notify,
    /// The editor opens right away.
    Edit,
    /// Nothing.
    Nothing,
}

impl Afterwards {
    /// What the choice `id` means; a notification for an unknown one.
    pub fn of(id: &str) -> Self {
        match id {
            "edit" => Self::Edit,
            "nothing" => Self::Nothing,
            _ => Self::Notify,
        }
    }
}

/// The settings a screenshot, a recording or the clipboard history work with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Where a screenshot goes.
    pub destination: Destination,
    /// The folder of screenshots and recordings.
    pub folder: PathBuf,
    /// The file name pattern of screenshots.
    pub file_name: String,
    /// What happens after a screenshot.
    pub afterwards: Afterwards,
    /// Whether a recording captures a region (else the focused display).
    pub record_region: bool,
    /// The file name pattern of recordings.
    pub recording_file_name: String,
    /// Whether a recording has sound.
    pub audio: bool,
    /// How many entries the clipboard history keeps.
    pub max_entries: u32,
    /// The keys that start and stop a recording, as the user reads them, for the notification
    /// that says how to stop it; empty without a key.
    pub recording_keys: String,
}

impl Settings {
    /// The settings in effect in `state`.
    pub fn of(state: &State) -> Self {
        let recording_keys = if is_on(state, RECORDING) {
            shortcuts::label(state, RECORDING_KEY)
        } else {
            String::new()
        };
        Self::read(|key| value(state, key), recording_keys)
    }

    /// The settings of the user's settings files; the defaults where they can't be read.
    pub fn load() -> Self {
        match crate::state::Paths::from_env() {
            Ok(paths) => Self::of(&State::load(Registry::builtin(), paths)),
            Err(_) => Self::read(
                |key| {
                    Registry::builtin()
                        .get(key)
                        .map(crate::registry::Setting::default_value)
                },
                String::new(),
            ),
        }
    }

    fn read(value: impl Fn(&str) -> Option<Value>, recording_keys: String) -> Self {
        let text = |key: &str| match value(key) {
            Some(Value::Text(text) | Value::Choice(text)) => text,
            _ => String::new(),
        };
        let max_entries = match value(MAX_ENTRIES) {
            Some(Value::Integer(n)) => u32::try_from(n).unwrap_or(750),
            _ => 750,
        };
        Self {
            destination: Destination::of(&text(DESTINATION)),
            folder: name::folder(&text(FOLDER), &name::Env::process()),
            file_name: text(FILE_NAME),
            afterwards: Afterwards::of(&text(AFTERWARDS)),
            record_region: text(RECORDING_AREA) != "display",
            recording_file_name: text(RECORDING_FILE_NAME),
            audio: value(RECORDING_AUDIO) == Some(Value::Bool(true)),
            max_entries,
            recording_keys,
        }
    }
}

/// Opens `path` in the default app for its type, as GIO finds it.
///
/// # Errors
///
/// What GIO says if no app opens it.
pub fn open_file(path: &std::path::Path) -> Result<(), String> {
    use gio::prelude::FileExt as _;
    let uri = gio::File::for_path(path).uri();
    gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
        .map_err(|error| error.to_string())
}

/// Why a screenshot, a recording or the clipboard history didn't work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// The user canceled the selection: not an error to show.
    Canceled,
    /// A program that is needed isn't installed: its name, and the package it comes with.
    Missing {
        /// The program.
        program: &'static str,
        /// The package that has it.
        package: &'static str,
    },
    /// A program failed: its name and what it said.
    Failed {
        /// The program.
        program: String,
        /// What it said on its standard error, or how it ended.
        message: String,
    },
    /// There is no window with the focus to capture.
    NoWindow,
    /// There is no display with the focus to capture.
    NoDisplay,
    /// The file name pattern can't be used.
    FileName,
    /// The file or its folder can't be written: the path and the error.
    Write {
        /// The file or folder.
        path: PathBuf,
        /// The error.
        error: String,
    },
    /// sway couldn't be asked.
    Sway(Text),
    /// The clipboard history is empty.
    EmptyHistory,
    /// The entry picked from the history has no data.
    EmptyEntry,
    /// The local time can't be read, which the file name needs.
    Time,
    /// Another press of the key is starting or stopping a recording.
    Busy,
}

impl CaptureError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        use crate::msg;
        match self {
            Self::Canceled => msg!("Canceled").into(),
            Self::Missing { program, package } => Text::new(msg!(
                "{program} isn't installed. Install the package {package} with your package \
                 manager."
            ))
            .arg("program", program)
            .arg("package", package),
            Self::Failed { program, message } if message.is_empty() => {
                Text::new(msg!("{program} failed")).arg("program", program)
            }
            Self::Failed { program, message } => Text::new(msg!("{program} failed: {message}"))
                .arg("program", program)
                .arg("message", message),
            Self::NoWindow => msg!("No window has the focus").into(),
            Self::NoDisplay => msg!("No display has the focus").into(),
            Self::FileName => {
                msg!("The file name pattern can't be used. Change it on the Screenshots & Clipboard page.")
                    .into()
            }
            Self::Write { path, error } => Text::new(msg!("Can't write {path}: {error}"))
                .arg("path", path.display())
                .arg("error", error),
            Self::Sway(text) => text.clone(),
            Self::EmptyHistory => msg!("The clipboard history is empty").into(),
            Self::EmptyEntry => msg!("The entry has no data, so nothing was copied").into(),
            Self::Time => msg!("The local time can't be read for the file name").into(),
            Self::Busy => msg!("A recording is being started or stopped already").into(),
        }
    }
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for CaptureError {}

#[cfg(test)]
mod tests;
