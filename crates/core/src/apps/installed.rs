// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The installed apps: the desktop entries in the `applications` directories of the XDG data
//! directories, each under its desktop file ID.
//!
//! The ID is the path below `applications/` with `/` turned into `-`
//! (`applications/kde/konsole.desktop` is `kde-konsole.desktop`). Where several directories
//! have an entry of one ID, the first directory wins: `$XDG_DATA_HOME` before `$XDG_DATA_DIRS`,
//! those in their order. An entry with `Hidden=true` hides the ID altogether, as if it weren't
//! installed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::Dirs;
use super::entry::{DesktopEntry, is_desktop_id};

/// How deep below `applications/` entries are looked for.
const MAX_DEPTH: usize = 4;
/// How many files of one directory tree are looked at, at most.
const MAX_FILES: usize = 20_000;

/// An installed app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Its desktop file ID, like `org.mozilla.firefox.desktop`.
    pub id: String,
    /// Its desktop entry.
    pub path: PathBuf,
    /// Its name, localized.
    pub name: String,
    /// Its icon: a theme icon name or an absolute path.
    pub icon: Option<String>,
    /// The MIME types it opens (`MimeType`).
    pub mime_types: Vec<String>,
    /// Its categories (`Categories`).
    pub categories: Vec<String>,
    /// Whether it runs in a terminal (`Terminal=true`).
    pub terminal: bool,
    /// Whether menus leave it out (`NoDisplay=true`). It can still open files.
    pub no_display: bool,
    /// The desktops it is only shown in (`OnlyShowIn`).
    pub only_show_in: Vec<String>,
    /// The desktops it isn't shown in (`NotShowIn`).
    pub not_show_in: Vec<String>,
    /// Whether the program of `TryExec` is missing, so the app counts as not installed.
    pub try_exec_missing: bool,
    /// The entry itself.
    pub entry: DesktopEntry,
}

impl App {
    /// Reads the app of ID `id` from the desktop entry at `path`. `None` for an entry that isn't
    /// an application (`Type=Link`, `Type=Directory`) or has no name.
    pub fn read(id: &str, path: &Path, dirs: &Dirs) -> Option<Self> {
        let entry = DesktopEntry::read(path)?;
        Self::of(id, path, entry, dirs)
    }

    /// The app of ID `id` with the desktop entry `entry` from `path`.
    pub fn of(id: &str, path: &Path, entry: DesktopEntry, dirs: &Dirs) -> Option<Self> {
        if entry.raw("Type").is_some_and(|kind| kind != "Application") {
            return None;
        }
        let name = entry
            .localized("Name", &dirs.languages)
            .filter(|name| !name.trim().is_empty())?;
        let try_exec_missing = entry
            .string("TryExec")
            .filter(|program| !program.is_empty())
            .is_some_and(|program| dirs.find_program(&program).is_none());
        Some(Self {
            id: id.to_owned(),
            path: path.to_owned(),
            name: single_line(&name),
            icon: entry.string("Icon").filter(|icon| !icon.is_empty()),
            mime_types: entry.list("MimeType"),
            categories: entry.list("Categories"),
            terminal: entry.boolean("Terminal"),
            no_display: entry.boolean("NoDisplay"),
            only_show_in: entry.list("OnlyShowIn"),
            not_show_in: entry.list("NotShowIn"),
            try_exec_missing,
            entry,
        })
    }

    /// Whether it opens the MIME type `mime` (compared without regard to case).
    pub fn opens(&self, mime: &str) -> bool {
        self.mime_types
            .iter()
            .any(|known| known.eq_ignore_ascii_case(mime))
    }

    /// Whether menus of the current desktop show it: not `NoDisplay`, and `OnlyShowIn` and
    /// `NotShowIn` allow one of the desktops of `XDG_CURRENT_DESKTOP`.
    pub fn shown_in(&self, desktops: &[String]) -> bool {
        !self.no_display && shows_in(&self.only_show_in, &self.not_show_in, desktops)
    }

    /// Whether it is a terminal emulator (`Categories=…;TerminalEmulator;`).
    pub fn is_terminal_emulator(&self) -> bool {
        self.categories
            .iter()
            .any(|category| category == "TerminalEmulator")
    }
}

/// Whether an entry with `only` (`OnlyShowIn`) and `not` (`NotShowIn`) shows in a session whose
/// `XDG_CURRENT_DESKTOP` names `desktops`: the first desktop named in either list decides, in the
/// order of `desktops`; with none named, it shows unless `OnlyShowIn` is set. This is how
/// `systemd-xdg-autostart-condition` decides.
pub fn shows_in(only: &[String], not: &[String], desktops: &[String]) -> bool {
    for desktop in desktops {
        if only.iter().any(|name| name == desktop) {
            return true;
        }
        if not.iter().any(|name| name == desktop) {
            return false;
        }
    }
    only.is_empty()
}

/// The first line of `text`, without control characters, so a name can't break a label.
pub fn single_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Every installed app, by ID, the first directory winning; hidden IDs and those whose
/// `TryExec` program is missing are left out. Sorted by name, then ID.
///
/// # Blocking
///
/// Reads the application directories of the XDG data directories.
pub fn installed(dirs: &Dirs) -> Vec<App> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut apps = Vec::new();
    for data_dir in dirs.data_search() {
        let base = data_dir.join("applications");
        for (id, path) in entries_below(&base) {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(entry) = DesktopEntry::read(&path) else {
                continue;
            };
            if entry.boolean("Hidden") {
                continue;
            }
            if let Some(app) = App::of(&id, &path, entry, dirs) {
                if !app.try_exec_missing {
                    apps.push(app);
                }
            }
        }
    }
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    apps
}

/// Why no installed app answers to an ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// The ID isn't a desktop file ID, or no data directory has an entry of it.
    NotFound,
    /// The entry that counts has `Hidden=true`: it is deleted.
    Hidden,
    /// The program of `TryExec` isn't installed.
    TryExec(String),
}

/// The installed app of ID `id`, if any: the entry of the first directory that has the ID, unless
/// it is hidden or its `TryExec` program is missing.
///
/// # Blocking
///
/// Reads the application directories of the XDG data directories.
pub fn find(dirs: &Dirs, id: &str) -> Option<App> {
    lookup(dirs, id).ok()
}

/// The installed app of ID `id`, or why there is none. An ID has to be a desktop file ID
/// ([`is_desktop_id`]): no `/`, nothing that leads out of the directories.
///
/// # Blocking
///
/// Reads the application directories of the XDG data directories.
///
/// # Errors
///
/// What makes the ID not an installed app.
pub fn lookup(dirs: &Dirs, id: &str) -> Result<App, Missing> {
    if !is_desktop_id(id) {
        return Err(Missing::NotFound);
    }
    for data_dir in dirs.data_search() {
        let Some(path) = locate(&data_dir.join("applications"), id, 0) else {
            continue;
        };
        let entry = DesktopEntry::read(&path).ok_or(Missing::NotFound)?;
        if entry.boolean("Hidden") {
            return Err(Missing::Hidden);
        }
        let app = App::of(id, &path, entry, dirs).ok_or(Missing::NotFound)?;
        if app.try_exec_missing {
            return Err(Missing::TryExec(
                app.entry.string("TryExec").unwrap_or_default(),
            ));
        }
        return Ok(app);
    }
    Err(Missing::NotFound)
}

/// The file below `dir` whose ID is `rest`, as [`entries_below`] names them: `rest` itself, or
/// a subdirectory `name` (not hidden) followed by `-` and the ID of a file in it, at most
/// [`MAX_DEPTH`] directories deep. A file in `dir` comes before one in a subdirectory.
///
/// # Blocking
///
/// Reads the application directories of the XDG data directories.
fn locate(dir: &Path, rest: &str, depth: usize) -> Option<PathBuf> {
    let direct = dir.join(rest);
    if std::fs::metadata(&direct).is_ok_and(|meta| meta.is_file()) {
        return Some(direct);
    }
    if depth >= MAX_DEPTH {
        return None;
    }
    let mut subdirs: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|item| item.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.') && rest.starts_with(&format!("{name}-")))
        .collect();
    subdirs.sort();
    subdirs.into_iter().find_map(|name| {
        let sub = dir.join(&name);
        if !std::fs::metadata(&sub).is_ok_and(|meta| meta.is_dir()) {
            return None;
        }
        locate(&sub, &rest[name.len() + 1..], depth + 1)
    })
}

/// The desktop entries below `base`, each with its ID, in a stable order.
///
/// # Blocking
///
/// Reads the application directories of the XDG data directories.
fn entries_below(base: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut stack = vec![(base.to_path_buf(), String::new(), 0usize)];
    let mut looked = 0usize;
    while let Some((dir, prefix, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<(String, PathBuf, bool)> = read
            .filter_map(Result::ok)
            .filter_map(|item| {
                let name = item.file_name().into_string().ok()?;
                let is_dir = item.file_type().ok()?.is_dir()
                    || std::fs::metadata(item.path()).is_ok_and(|meta| meta.is_dir());
                Some((name, item.path(), is_dir))
            })
            .collect();
        names.sort();
        for (name, path, is_dir) in names {
            looked += 1;
            if looked > MAX_FILES {
                return found;
            }
            if is_dir {
                if depth < MAX_DEPTH && !name.starts_with('.') {
                    stack.push((path, format!("{prefix}{name}-"), depth + 1));
                }
            } else if name.ends_with(".desktop") {
                found.push((format!("{prefix}{name}"), path));
            }
        }
    }
    found
}
