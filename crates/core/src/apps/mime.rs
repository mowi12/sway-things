// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The default apps: which app opens a web link, a folder, a PDF.
//!
//! The XDG MIME Applications spec reads `mimeapps.list` files in a fixed order, and before each
//! `mimeapps.list` the desktop-specific `<desktop>-mimeapps.list` of every desktop in
//! `XDG_CURRENT_DESKTOP` (lowercase). In the `[Default Applications]` group of the first file
//! that names an installed app for a type, that app wins. Sway Things writes its choices to its
//! own `$XDG_CONFIG_HOME/sway-mimeapps.list`, which comes before the user's `mimeapps.list` in
//! the same directory when `XDG_CURRENT_DESKTOP` names sway. The user's `mimeapps.list` is never
//! touched.
//!
//! The terminal is not a MIME type for most programs: `GLib` (2.76 and newer) and Sway Things' own
//! editor start apps with `Terminal=true` through `xdg-terminal-exec`, which reads
//! `sway-xdg-terminals.list` the same way. The terminal row writes that list too, and
//! `x-scheme-handler/terminal` for the programs that ask for that type.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::Dirs;
use super::installed::App;
use crate::generate::HEADER;
use crate::registry::Value;
pub use crate::registry::apps::{Kind, check_id};
use crate::state::State;

/// The name of Sway Things' list of default apps in `$XDG_CONFIG_HOME`.
pub const OURS: &str = "sway-mimeapps.list";
/// The name of Sway Things' list of terminals for `xdg-terminal-exec` in `$XDG_CONFIG_HOME`.
pub const TERMINALS: &str = "sway-xdg-terminals.list";
/// The group of the default apps in a MIME apps list.
const DEFAULTS: &str = "Default Applications";
/// The group of added associations in a MIME apps list.
const ADDED: &str = "Added Associations";
/// The MIME type of the default terminal.
pub const TERMINAL_TYPE: &str = "x-scheme-handler/terminal";

impl Kind {
    /// The MIME types it is the default for, the one that decides what the row shows first.
    pub const fn types(self) -> &'static [&'static str] {
        match self {
            Self::Browser => &[
                "x-scheme-handler/https",
                "x-scheme-handler/http",
                "text/html",
                "application/xhtml+xml",
            ],
            Self::FileManager => &["inode/directory"],
            Self::Mail => &["x-scheme-handler/mailto"],
            Self::ImageViewer => &[
                "image/png",
                "image/jpeg",
                "image/gif",
                "image/webp",
                "image/avif",
                "image/bmp",
                "image/tiff",
                "image/svg+xml",
            ],
            Self::PdfViewer => &["application/pdf"],
            Self::VideoPlayer => &[
                "video/mp4",
                "video/x-matroska",
                "video/webm",
                "video/mpeg",
                "video/quicktime",
                "video/x-msvideo",
                "video/ogg",
            ],
            Self::MusicPlayer => &[
                "audio/mpeg",
                "audio/flac",
                "audio/ogg",
                "audio/x-vorbis+ogg",
                "audio/opus",
                "audio/mp4",
                "audio/aac",
                "audio/x-wav",
                "audio/wav",
            ],
            Self::TextEditor => &["text/plain"],
            Self::Terminal => &[TERMINAL_TYPE],
        }
    }

    /// The type the row shows the default of.
    pub fn main_type(self) -> &'static str {
        self.types().first().copied().unwrap_or_default()
    }

    /// Whether `app` can be offered: it opens the main type (for the terminal, also any
    /// terminal emulator). Apps with `NoDisplay` count: many handlers are hidden from menus.
    pub fn offers(self, app: &App) -> bool {
        match self {
            Self::Terminal => app.is_terminal_emulator() || app.opens(TERMINAL_TYPE),
            Self::Browser => {
                app.opens("x-scheme-handler/https") || app.opens("x-scheme-handler/http")
            }
            _ => app.opens(self.main_type()),
        }
    }

    /// The apps that can be offered, from `apps`.
    pub fn candidates(self, apps: &[App]) -> Vec<&App> {
        apps.iter().filter(|app| self.offers(app)).collect()
    }

    /// The desktop file ID stored, if any.
    pub fn stored(self, state: &State) -> Option<String> {
        match state.get(self.key(), None) {
            Some(Value::Text(id)) if !id.is_empty() => Some(id.clone()),
            _ => None,
        }
    }
}

/// One MIME apps list: the IDs of each type in its groups.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct List {
    /// `[Default Applications]`: the IDs for each type, in order.
    pub defaults: BTreeMap<String, Vec<String>>,
    /// `[Added Associations]`: the IDs for each type, in order.
    pub added: BTreeMap<String, Vec<String>>,
}

impl List {
    /// Reads the text of a MIME apps list. Types compare in lowercase.
    pub fn parse(text: &str) -> Self {
        let mut list = Self::default();
        let mut group = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line
                .strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
            {
                name.clone_into(&mut group);
                continue;
            }
            let Some((mime, ids)) = line.split_once('=') else {
                continue;
            };
            let ids: Vec<String> = ids
                .split(';')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect();
            let target = match group.as_str() {
                DEFAULTS => &mut list.defaults,
                ADDED => &mut list.added,
                _ => continue,
            };
            target.insert(mime.trim().to_ascii_lowercase(), ids);
        }
        list
    }

    /// Reads the list at `path`; `None` if there is none or it can't be read.
    pub fn read(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > super::entry::MAX_SIZE {
            return None;
        }
        Some(Self::parse(&std::fs::read_to_string(path).ok()?))
    }
}

/// The MIME apps lists in the order the spec reads them, for the desktops of `dirs`: in each of
/// the config directories, then in the `applications` directory of each data directory, the
/// desktop-specific lists (in the order of `XDG_CURRENT_DESKTOP`) and then `mimeapps.list`.
pub fn list_paths(dirs: &Dirs) -> Vec<PathBuf> {
    let desktops: Vec<String> = dirs
        .desktops
        .iter()
        .map(|desktop| desktop.to_lowercase())
        .collect();
    let mut paths = Vec::new();
    let data: Vec<PathBuf> = dirs
        .data_search()
        .into_iter()
        .map(|dir| dir.join("applications"))
        .collect();
    for dir in dirs.config_search().into_iter().chain(data) {
        for desktop in &desktops {
            paths.push(dir.join(format!("{desktop}-mimeapps.list")));
        }
        paths.push(dir.join("mimeapps.list"));
    }
    paths
}

/// Where a default app comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The `[Default Applications]` of this list.
    List(PathBuf),
    /// Nothing names one: the first installed app that says it opens the type (from the `[Added
    /// Associations]` of a list, else its `MimeType`), which is what most apps take.
    Fallback,
}

/// The app that opens a type, and where that comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultApp {
    /// Its desktop file ID.
    pub id: String,
    /// Where it comes from.
    pub source: Source,
}

/// The default app for `mime` by the lists of `dirs`, the list at `skip` left out (to tell what
/// would open the type without Sway Things' choice). `installed` says whether an ID is an
/// installed app; the spec skips the IDs that aren't.
pub fn default_for(
    dirs: &Dirs,
    mime: &str,
    apps: &[App],
    skip: Option<&Path>,
) -> Option<DefaultApp> {
    let mime = mime.to_ascii_lowercase();
    let installed = |id: &str| apps.iter().any(|app| app.id == id);
    let lists: Vec<(PathBuf, List)> = list_paths(dirs)
        .into_iter()
        .filter(|path| Some(path.as_path()) != skip)
        .filter_map(|path| List::read(&path).map(|list| (path, list)))
        .collect();
    for (path, list) in &lists {
        if let Some(id) = list
            .defaults
            .get(&mime)
            .and_then(|ids| ids.iter().find(|id| installed(id)))
        {
            return Some(DefaultApp {
                id: id.clone(),
                source: Source::List(path.clone()),
            });
        }
    }
    let added = lists.iter().find_map(|(_, list)| {
        list.added
            .get(&mime)
            .and_then(|ids| ids.iter().find(|id| installed(id)))
    });
    let declared = || apps.iter().find(|app| app.opens(&mime)).map(|app| &app.id);
    added.or_else(declared).map(|id| DefaultApp {
        id: id.clone(),
        source: Source::Fallback,
    })
}

/// The path of Sway Things' list of default apps.
pub fn ours(dirs: &Dirs) -> PathBuf {
    dirs.config_home.join(OURS)
}

/// The path of Sway Things' list of terminals for `xdg-terminal-exec`.
pub fn terminals_list(dirs: &Dirs) -> PathBuf {
    dirs.config_home.join(TERMINALS)
}

/// The text of Sway Things' list of default apps for the choices stored in `state`, each for the
/// types of its kind that the app opens (all of them for an app that isn't installed); `None`
/// while nothing is chosen.
pub fn ours_text(state: &State, apps: &[App]) -> Option<String> {
    let mut lines = Vec::new();
    for kind in Kind::ALL {
        let Some(id) = kind.stored(state) else {
            continue;
        };
        let app = apps.iter().find(|app| app.id == id);
        for mime in kind.types() {
            let opens = app.is_none_or(|app| app.opens(mime));
            let terminal = kind == Kind::Terminal && *mime == TERMINAL_TYPE;
            if opens || terminal {
                lines.push(format!("{mime}={id};"));
            }
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(format!("{HEADER}\n[{DEFAULTS}]\n{}\n", lines.join("\n")))
}

/// The text of Sway Things' list of terminals for the terminal stored in `state`; `None` while
/// none is chosen.
pub fn terminals_text(state: &State) -> Option<String> {
    let id = Kind::Terminal.stored(state)?;
    Some(format!("{HEADER}{id}\n"))
}

/// Who opens the main type of `kind` now, and whether Sway Things' choice is the one: what a row
/// of the page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// What opens it now, with all lists read.
    pub now: Option<DefaultApp>,
    /// What would open it without Sway Things' list.
    pub without_ours: Option<DefaultApp>,
    /// Whether Sway Things' choice is in effect: its list is read and comes first.
    pub ours_wins: bool,
}

/// What a row of `kind` shows, for `dirs` and the installed `apps`.
pub fn shown(dirs: &Dirs, kind: Kind, apps: &[App]) -> Shown {
    let ours = ours(dirs);
    let mime = kind.main_type();
    let now = default_for(dirs, mime, apps, None);
    let without_ours = default_for(dirs, mime, apps, Some(&ours));
    let ours_wins = now
        .as_ref()
        .is_some_and(|now| now.source == Source::List(ours.clone()));
    Shown {
        now,
        without_ours,
        ours_wins,
    }
}
