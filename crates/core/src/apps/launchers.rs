// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The terminal and the launcher the main keys open, and the bindings Sway Things writes for
//! them.
//!
//! `set $term foot` and `set $menu wmenu-run` in the user's config only reach the lines that use
//! them, and sway replaces a variable when it reads a line, so the user's own `bindsym
//! $mod+Return exec $term` can't be changed from outside. Sway Things therefore binds the keys
//! itself (`$mod+Return` and `$mod+d` unless chosen otherwise) in `50-bindings.conf`, with the
//! other bindings of the Keyboard Shortcuts page ([`crate::bindings::ours::file_lines`]): with
//! `--no-warn`, since choosing a terminal here means taking its keys, and with the clash detection
//! and the live apply of that page. Nothing is bound while no terminal or launcher is chosen.

use crate::i18n::Msg;
use crate::msg;
use crate::registry::Value;
pub use crate::registry::apps::{
    KEYS, LAUNCHER, LAUNCHER_KEYS, TERMINAL, TERMINAL_KEYS, check_command, check_keys, line,
};
use crate::state::State;
use crate::sway_config::bindings::{Line, LineError};

use super::Dirs;
use super::installed::App;

/// A binding of the page: the setting of its command and the setting of its keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// The key of the command's setting.
    pub key: &'static str,
    /// The key of the keys' setting.
    pub keys_key: &'static str,
    /// The keys by default.
    pub default_keys: &'static str,
    /// What it opens, for the rows that name it.
    pub title: Msg,
}

/// The terminal's and the launcher's binding, in the order of the file.
pub const BINDINGS: [Binding; 2] = [
    Binding {
        key: TERMINAL,
        keys_key: TERMINAL_KEYS,
        default_keys: "$mod+Return",
        title: msg!("Terminal"),
    },
    Binding {
        key: LAUNCHER,
        keys_key: LAUNCHER_KEYS,
        default_keys: "$mod+d",
        title: msg!("Launcher"),
    },
];

impl Binding {
    /// The binding whose command has the key `key`.
    pub fn of(key: &str) -> Option<Self> {
        BINDINGS.into_iter().find(|binding| binding.key == key)
    }

    /// The command stored, if any.
    pub fn command(self, state: &State) -> Option<String> {
        match state.get(self.key, None) {
            Some(Value::Text(text)) if !text.trim().is_empty() => Some(text.trim().to_owned()),
            _ => None,
        }
    }

    /// The keys stored, else the default ones, with `$mod` as written.
    pub fn keys(self, state: &State) -> String {
        match state.get(self.keys_key, None) {
            Some(Value::Text(text)) if !text.trim().is_empty() => text.trim().to_owned(),
            _ => self.default_keys.to_owned(),
        }
    }

    /// The binding line for `state`, with `$mod` as written in its keys; `None` while no command
    /// is chosen.
    ///
    /// # Errors
    ///
    /// The [`LineError`] of a command or keys that can't be written.
    pub fn line(self, state: &State) -> Option<Result<Line, LineError>> {
        let command = self.command(state)?;
        Some(line(&self.keys(state), &command))
    }
}

/// A terminal or launcher Sway Things knows: its program, the command line that opens it, and
/// its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    /// The program that must be in `PATH`.
    pub program: &'static str,
    /// The command line a binding runs.
    pub command: &'static str,
    /// What it is called.
    pub name: &'static str,
}

const fn known(program: &'static str, command: &'static str, name: &'static str) -> Known {
    Known {
        program,
        command,
        name,
    }
}

/// The terminals Sway Things knows, the ones of sway's default config first.
pub const TERMINALS: &[Known] = &[
    known("foot", "foot", "foot"),
    known("alacritty", "alacritty", "Alacritty"),
    known("kitty", "kitty", "kitty"),
    known("wezterm", "wezterm", "WezTerm"),
    known("ghostty", "ghostty", "Ghostty"),
    known("kgx", "kgx", "Console (GNOME)"),
    known("gnome-terminal", "gnome-terminal", "GNOME Terminal"),
    known("konsole", "konsole", "Konsole"),
    known("xfce4-terminal", "xfce4-terminal", "Xfce Terminal"),
    known("tilix", "tilix", "Tilix"),
    known("terminator", "terminator", "Terminator"),
    known("st", "st", "st"),
    known("urxvt", "urxvt", "rxvt-unicode"),
    known("xterm", "xterm", "XTerm"),
];

/// The launchers Sway Things knows, each with the command line that shows the apps.
pub const LAUNCHERS: &[Known] = &[
    known("fuzzel", "fuzzel", "Fuzzel"),
    known("wofi", "wofi --show drun", "Wofi"),
    known("rofi", "rofi -show drun", "Rofi"),
    known("tofi-drun", "tofi-drun --drun-launch=true", "tofi"),
    known("bemenu-run", "bemenu-run", "bemenu"),
    known("wmenu-run", "wmenu-run", "wmenu"),
    known("anyrun", "anyrun", "Anyrun"),
    known("walker", "walker", "Walker"),
    known("sirula", "sirula", "Sirula"),
    known("nwg-drawer", "nwg-drawer", "nwg-drawer"),
    known("dmenu_run", "dmenu_run", "dmenu"),
    // Shows its window through its server (`vicinae server`), which runs on its own.
    known("vicinae", "vicinae toggle", "Vicinae"),
];

/// The installed terminals as `(command, name)`: the known ones in `PATH`, then other desktop
/// entries of the category `TerminalEmulator` whose program is installed, by the name or the
/// absolute path of their `Exec`.
pub fn terminals(dirs: &Dirs, apps: &[App]) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = TERMINALS
        .iter()
        .filter(|known| dirs.find_program(known.program).is_some())
        .map(|known| (known.command.to_owned(), known.name.to_owned()))
        .collect();
    for app in apps.iter().filter(|app| app.is_terminal_emulator()) {
        let Some(program) = plain_program(app, dirs) else {
            continue;
        };
        let name = program.rsplit('/').next().unwrap_or(&program);
        if found.iter().any(|(command, _)| *command == program)
            || TERMINALS.iter().any(|known| known.program == name)
        {
            continue;
        }
        found.push((program, app.name.clone()));
    }
    found
}

/// The installed launchers as `(command, name)`: the known ones in `PATH`.
pub fn launchers(dirs: &Dirs) -> Vec<(String, String)> {
    LAUNCHERS
        .iter()
        .filter(|known| dirs.find_program(known.program).is_some())
        .map(|known| (known.command.to_owned(), known.name.to_owned()))
        .collect()
}

/// The program of `app`'s command line: an absolute path as it is, a name if it is in `PATH`;
/// either only of characters that mean nothing to a shell.
fn plain_program(app: &App, dirs: &Dirs) -> Option<String> {
    let args = app.entry.command(&app.path, &dirs.languages).ok()?;
    let program = args.first()?;
    let plain_part = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+')
            })
    };
    let plain = match program.strip_prefix('/') {
        // Directories may be hidden (`~/.local/bin`), but never `.` or `..`.
        Some(path) => path
            .split('/')
            .all(|part| plain_part(part) && part != "." && part != ".."),
        None => plain_part(program) && !program.starts_with(['-', '.']),
    };
    (plain && dirs.find_program(program).is_some()).then(|| program.clone())
}
