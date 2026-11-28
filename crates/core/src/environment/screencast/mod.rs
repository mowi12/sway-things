// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How xdg-desktop-portal-wlr asks which display to share: Sway Things' own
//! `~/.config/xdg-desktop-portal-wlr/sway`, from `environment.screencast_chooser` and
//! `environment.screencast_output`.
//!
//! xdpw reads the first of `<desktop>` (for each desktop of `XDG_CURRENT_DESKTOP`, as written)
//! and `config` in `$XDG_CONFIG_HOME/xdg-desktop-portal-wlr`, then the same in
//! `/etc/xdg/xdg-desktop-portal-wlr` (xdg-desktop-portal-wlr(5)). Ours is only written where the
//! user has none of these files in their directory: it would hide theirs, or theirs would hide
//! it. xdpw runs `chooser_cmd` through `sh -c`, so only fixed commands are written; the output's
//! name is checked to be a plain word.

use std::path::{Path, PathBuf};

use crate::apps::Dirs;
use crate::capture::tools::Program;
use crate::manifest::HEADER;
use crate::manifest::Wanted;
use crate::registry::Value;
pub use crate::registry::environment::is_output_name;
use crate::registry::environment::{SCREENCAST_CHOOSER, SCREENCAST_OUTPUT};
use crate::state::State;

/// The name of Sway Things' file, read for the desktop `sway`.
pub const FILE_NAME: &str = "sway";

/// The directory of xdpw's configuration below a configuration directory.
const DIRECTORY: &str = "xdg-desktop-portal-wlr";

/// The system's configuration directory xdpw reads after the user's.
const SYSTEM: &str = "/etc/xdg";

/// How xdpw asks for the display to share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chooser {
    /// No file: xdpw picks the first chooser it finds.
    Default,
    /// Click a display, with slurp.
    Slurp,
    /// Pick from a menu, with the first launcher installed ([`MENUS`]).
    Menu,
    /// Always the same display, `environment.screencast_output`.
    Output,
}

impl Chooser {
    /// The chooser stored as `id`; the default for anything else.
    pub fn from_id(id: &str) -> Self {
        match id {
            "slurp" => Self::Slurp,
            "menu" => Self::Menu,
            "output" => Self::Output,
            _ => Self::Default,
        }
    }

    /// The chooser stored in `state`.
    pub fn of(state: &State) -> Self {
        match state.get(SCREENCAST_CHOOSER, None) {
            Some(Value::Choice(id)) => Self::from_id(id),
            _ => Self::Default,
        }
    }
}

/// slurp, which lets the user click a display.
pub const SLURP: Program = crate::capture::tools::SLURP;

/// A launcher that shows a menu read from its standard input, with its fixed command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Menu {
    /// The program.
    pub program: Program,
    /// The command xdpw runs, fixed.
    pub command: &'static str,
}

/// The launchers for the menu, in the order they are preferred.
pub const MENUS: [Menu; 5] = [
    Menu {
        program: Program {
            name: "fuzzel",
            package: "fuzzel",
        },
        command: "fuzzel --dmenu",
    },
    Menu {
        program: Program {
            name: "wofi",
            package: "wofi",
        },
        command: "wofi --dmenu",
    },
    Menu {
        program: Program {
            name: "rofi",
            package: "rofi",
        },
        command: "rofi -dmenu",
    },
    Menu {
        program: Program {
            name: "bemenu",
            package: "bemenu",
        },
        command: "bemenu",
    },
    Menu {
        program: Program {
            name: "wmenu",
            package: "wmenu",
        },
        command: "wmenu",
    },
];

/// The first launcher of [`MENUS`] installed for `dirs`.
pub fn menu(dirs: &Dirs) -> Option<Menu> {
    MENUS
        .into_iter()
        .find(|menu| dirs.find_program(menu.program.name).is_some())
}

/// Whether slurp is installed for `dirs`.
pub fn has_slurp(dirs: &Dirs) -> bool {
    dirs.find_program(SLURP.name).is_some()
}

/// The names of the files xdpw reads in a directory, in its order, for `dirs`' desktops.
fn names(dirs: &Dirs) -> Vec<String> {
    let mut names: Vec<String> = dirs.desktops.clone();
    names.push("config".to_owned());
    names
}

/// The path of Sway Things' file for `dirs`.
pub fn path(dirs: &Dirs) -> PathBuf {
    dirs.config_home.join(DIRECTORY).join(FILE_NAME)
}

/// Whether the file at `path` is one Sway Things wrote: it starts with the header of the
/// generated files.
fn is_ours(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.starts_with(HEADER))
}

/// The user's own file in `~/.config/xdg-desktop-portal-wlr` that would hide Sway Things' file or
/// be hidden by it: a `sway` Sway Things didn't write, the file of another desktop of
/// `XDG_CURRENT_DESKTOP`, or `config`.
pub fn users_config(dirs: &Dirs) -> Option<PathBuf> {
    let user_dir = dirs.config_home.join(DIRECTORY);
    std::iter::once(FILE_NAME.to_owned())
        .chain(names(dirs))
        .map(|name| user_dir.join(name))
        .find(|path| path.is_file() && !is_ours(path))
}

/// The file xdpw reads now, for `dirs`, with `system` as `/etc/xdg`.
pub fn deciding_in(dirs: &Dirs, system: &Path) -> Option<PathBuf> {
    [dirs.config_home.as_path(), system]
        .iter()
        .find_map(|base| {
            names(dirs)
                .iter()
                .map(|name| base.join(DIRECTORY).join(name))
                .find(|path| path.is_file())
        })
}

/// The file xdpw reads now, for `dirs`.
pub fn deciding(dirs: &Dirs) -> Option<PathBuf> {
    deciding_in(dirs, Path::new(SYSTEM))
}

/// The system's file Sway Things' file replaces for xdpw, with `system` as `/etc/xdg`: the one
/// it would read without ours.
pub fn shadowed_in(dirs: &Dirs, system: &Path) -> Option<PathBuf> {
    names(dirs)
        .iter()
        .map(|name| system.join(DIRECTORY).join(name))
        .find(|path| path.is_file())
}

/// The system's file Sway Things' file replaces for xdpw.
pub fn shadowed(dirs: &Dirs) -> Option<PathBuf> {
    shadowed_in(dirs, Path::new(SYSTEM))
}

/// The `[screencast]` lines for `chooser`, with `output` for [`Chooser::Output`] and `menu` the
/// launcher installed. `None` where no file is needed or the tool isn't installed: xdpw then
/// picks a chooser itself.
pub fn lines(chooser: Chooser, output: &str, slurp: bool, menu: Option<Menu>) -> Option<String> {
    let body = match chooser {
        Chooser::Slurp if slurp => "chooser_type=simple\nchooser_cmd=slurp -f %o -or\n".to_owned(),
        Chooser::Menu => format!("chooser_type=dmenu\nchooser_cmd={}\n", menu?.command),
        Chooser::Output if is_output_name(output) => {
            format!("chooser_type=none\noutput_name={output}\n")
        }
        Chooser::Default | Chooser::Slurp | Chooser::Output => return None,
    };
    Some(format!("[screencast]\n{body}"))
}

/// The text of Sway Things' file the settings in `state` want, for `dirs`.
pub fn text(state: &State, dirs: &Dirs) -> Option<String> {
    let output = match state.get(SCREENCAST_OUTPUT, None) {
        Some(Value::Text(output)) => output.as_str(),
        _ => "",
    };
    let lines = lines(Chooser::of(state), output, has_slurp(dirs), menu(dirs))?;
    Some(format!("{HEADER}{lines}"))
}

/// The file the settings in `state` want: Sway Things' own, where the user has none there.
pub fn wanted(state: &State, dirs: &Dirs) -> Wanted {
    Wanted {
        path: path(dirs),
        text: text(state, dirs).filter(|_| users_config(dirs).is_none()),
    }
}

/// Restarts xdg-desktop-portal-wlr if it runs, so it reads its configuration again.
///
/// # Errors
///
/// What `systemctl` reported.
pub async fn restart() -> Result<(), String> {
    crate::systemd::run(&["try-restart", super::portals::Component::Wlr.unit()]).await
}

#[cfg(test)]
mod tests;
