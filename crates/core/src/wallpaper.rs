// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wallpapers set outside sway's `output … bg`: an `exec` line of the user's config that starts
//! a wallpaper tool or a script of their own.
//!
//! sway draws the background of `output … bg` with swaybg. A wallpaper tool started from the
//! config (swww, wpaperd, a second swaybg, …) draws its own layer, and either can end up on top.
//! The Wallpaper page shows such a line as "managed externally", with file and line, and leaves
//! it alone: Sway Things never edits the user's config.
//!
//! ```
//! use std::path::Path;
//! use sway_things_core::sway_config::{MemoryFiles, parse};
//! use sway_things_core::wallpaper::{self, Starter};
//!
//! let files = MemoryFiles::new().file(
//!     "/home/me/.config/sway/config",
//!     "exec swww-daemon\nexec_always ~/.local/bin/random-wallpaper.sh\nexec foot\n",
//! );
//! let config = parse(Path::new("/home/me/.config/sway/config"), &files);
//! let found = wallpaper::starters(&config);
//! assert_eq!(found.len(), 2);
//! assert_eq!(found[0].starter, Starter::Tool("swww-daemon"));
//! assert_eq!(found[1].directive.line, 2);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod collection;
mod cycle;

pub use collection::{
    CHANGE, COLLECTION, Change, Collection, DAY, EXTENSIONS, INTERVAL, ORDER, Order, PICTURES,
    expand, home, is_collection_name, is_picture, pictures_in, tilde,
};
pub use cycle::{
    CycleError, Moment, Shown, choose, current, has_timers, is_due, login, next, point, prepare,
    program, reschedule, schedule, show, shown, shown_on, start_command, unit, unschedule,
};

use crate::sway_config::{Directive, SwayConfig};

/// Programs that set the wallpaper on wlroots compositors.
const TOOLS: &[&str] = &[
    "swaybg",
    "swww",
    "swww-daemon",
    "awww",
    "awww-daemon",
    "wpaperd",
    "hyprpaper",
    "mpvpaper",
    "oguri",
    "wbg",
    "waypaper",
    "azote",
    "variety",
    "setwallpaper",
    "multibg-sway",
    "multibg-wayland",
    "wallutils",
];

/// What sets the wallpaper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Starter {
    /// A known wallpaper tool, by its program name.
    Tool(&'static str),
    /// A program whose name contains "wallpaper", as written in the line: likely a script of the
    /// user's.
    Script(String),
}

/// An `exec` line of the user's config that sets the wallpaper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct External {
    /// The line: file, line number and text.
    pub directive: Directive,
    /// What it starts.
    pub starter: Starter,
}

/// The `exec` and `exec_always` lines of `config` that start a wallpaper tool or a wallpaper
/// script, in the order sway runs them. Words in a shell command (`exec sh -c '…'`) count too.
pub fn starters(config: &SwayConfig) -> Vec<External> {
    config
        .directives
        .iter()
        .filter(|directive| {
            directive.block.is_empty()
                && (directive.name.eq_ignore_ascii_case("exec")
                    || directive.name.eq_ignore_ascii_case("exec_always"))
        })
        .filter_map(|directive| {
            // sway keeps a quoted argument as one: `sh -c 'pkill swww; swww-daemon'`.
            let starter = directive
                .args
                .iter()
                .flat_map(|arg| {
                    arg.split(|character: char| {
                        character.is_whitespace()
                            || matches!(character, ';' | '&' | '|' | '(' | ')')
                    })
                })
                .take_while(|word| !is_own(word))
                .find_map(starter)?;
            Some(External {
                directive: directive.clone(),
                starter,
            })
        })
        .collect()
}

/// Whether `word` runs Sway Things, which the generated config does to change the pictures of
/// a collection: the rest of the line is its arguments, not a wallpaper program.
fn is_own(word: &str) -> bool {
    word.rsplit('/').next() == Some("sway-things")
}

/// What `word` of an `exec` line starts, if it is a wallpaper program.
fn starter(word: &str) -> Option<Starter> {
    let word = word.trim_matches(|character| matches!(character, '"' | '\''));
    let program = word.rsplit('/').next().unwrap_or(word);
    if let Some(tool) = TOOLS.iter().find(|tool| **tool == program) {
        return Some(Starter::Tool(tool));
    }
    program
        .to_ascii_lowercase()
        .contains("wallpaper")
        .then(|| Starter::Script(word.to_owned()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Starter, starters};
    use crate::sway_config::{MemoryFiles, parse};

    fn found(text: &str) -> Vec<(usize, Starter)> {
        let files = MemoryFiles::new().file("/c/config", text);
        let config = parse(Path::new("/c/config"), &files);
        starters(&config)
            .into_iter()
            .map(|external| (external.directive.line, external.starter))
            .collect()
    }

    #[test]
    fn wallpaper_tools_and_scripts_are_found() {
        assert_eq!(
            found(
                "exec swaybg -i ~/a.png\n\
                 exec_always --no-startup-id /usr/bin/wpaperd -d\n\
                 exec sh -c 'pkill swww-daemon; swww-daemon'\n\
                 exec ~/bin/Wallpaper-Rotate.sh\n\
                 EXEC waypaper --restore\n"
            ),
            [
                (1, Starter::Tool("swaybg")),
                (2, Starter::Tool("wpaperd")),
                (3, Starter::Tool("swww-daemon")),
                (4, Starter::Script("~/bin/Wallpaper-Rotate.sh".to_owned())),
                (5, Starter::Tool("waypaper")),
            ]
        );
    }

    #[test]
    fn other_lines_are_not_wallpapers() {
        assert_eq!(
            found(
                "exec foot\n\
                 bindsym $mod+w exec swaybg -c '#000000'\n\
                 output * bg ~/a.png fill\n\
                 mode resize {\n  bindsym w exec wpaperd\n}\n\
                 exec swaync\n\
                 exec sway-things hook wallpaper\n"
            ),
            []
        );
    }
}
