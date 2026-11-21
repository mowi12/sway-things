// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cursor themes installed for the user, with the sizes they ship and a preview image.
//!
//! A cursor theme is a directory with a `cursors/` subdirectory in one of the directories
//! libXcursor and wlroots search ([`search_path`]). Its name is the name of the directory, which is
//! what `seat * xcursor_theme` and gsettings take; its title is the `Name=` of its `index.theme`.
//! A theme found in more than one directory counts where it is found first, as for libXcursor.
//!
//! Only reading: the files are bounded in size and parsed as data ([`xcursor`]).
//!
//! ```
//! use sway_things_core::cursor;
//! use sway_things_core::sway_config::MemoryFiles;
//!
//! let files = MemoryFiles::new()
//!     .var("HOME", "/home/me")
//!     .file("/usr/share/icons/Adwaita/index.theme", "[Icon Theme]\nName=Adwaita\n")
//!     .file("/usr/share/icons/Adwaita/cursors/left_ptr", "not an Xcursor file")
//!     .file("/usr/share/icons/hicolor/index.theme", "[Icon Theme]\nName=Hicolor\n");
//! let themes = cursor::themes(&files);
//! // hicolor has no cursors; the arrow of Adwaita can't be read, so it has no sizes.
//! assert_eq!(themes.len(), 1);
//! assert_eq!(themes[0].name, "Adwaita");
//! assert!(themes[0].sizes.is_empty());
//! ```

#[cfg(test)]
mod tests;
pub mod xcursor;

use std::path::{Path, PathBuf};

use crate::sway_config::Files;

pub use crate::registry::cursor::{DEFAULT_SIZE, DEFAULT_THEME};

/// The largest cursor file read; real ones are well below 4 MiB.
pub const MAX_FILE_SIZE: usize = 16 * 1024 * 1024;

/// The largest `index.theme` read.
const MAX_INDEX_SIZE: usize = 64 * 1024;

/// The cursor files a preview is taken from, in order: the arrow.
const PREVIEW_CURSORS: [&str; 3] = ["left_ptr", "default", "arrow"];

/// An installed cursor theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorTheme {
    /// The name of its directory, which sway and gsettings take.
    pub name: String,
    /// The name it gives itself in `index.theme`, or its directory name.
    pub title: String,
    /// The theme's directory.
    pub dir: PathBuf,
    /// The sizes its arrow cursor ships, smallest first; empty if that file can't be read.
    pub sizes: Vec<u32>,
}

impl CursorTheme {
    /// The arrow cursor of the theme, in the shipped size closest to `size`.
    ///
    /// # Errors
    ///
    /// An [`xcursor::XcursorError`] if no arrow cursor can be read.
    pub fn preview(
        &self,
        files: &impl Files,
        size: u32,
    ) -> Result<xcursor::Image, xcursor::XcursorError> {
        let bytes = arrow(files, &self.dir).ok_or(xcursor::XcursorError::NoImage)?;
        xcursor::image(&bytes, size)
    }
}

/// The directories searched for cursor themes, in order: `$XCURSOR_PATH` if set; otherwise the
/// `icons` directories of `$XDG_DATA_HOME` and `$XDG_DATA_DIRS` with `~/.icons` after the first
/// one, then `/usr/share/pixmaps`.
pub fn search_path(files: &impl Files) -> Vec<PathBuf> {
    if let Some(path) = files.env("XCURSOR_PATH").filter(|path| !path.is_empty()) {
        return path
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(|dir| expand_home(files, dir))
            .collect();
    }
    let home = files.env("HOME").filter(|home| !home.is_empty());
    let mut dirs = Vec::new();
    match files
        .env("XDG_DATA_HOME")
        .filter(|dir| dir.starts_with('/'))
    {
        Some(data_home) => dirs.push(Path::new(&data_home).join("icons")),
        None => dirs.extend(
            home.as_ref()
                .map(|home| Path::new(home).join(".local/share/icons")),
        ),
    }
    dirs.extend(home.as_ref().map(|home| Path::new(home).join(".icons")));
    let data_dirs = files
        .env("XDG_DATA_DIRS")
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|dir| dir.starts_with('/'))
            .map(|dir| Path::new(dir).join("icons")),
    );
    dirs.push(PathBuf::from("/usr/share/pixmaps"));
    let mut unique = Vec::with_capacity(dirs.len());
    for dir in dirs {
        if !unique.contains(&dir) {
            unique.push(dir);
        }
    }
    unique
}

fn expand_home(files: &impl Files, dir: &str) -> PathBuf {
    match (dir.strip_prefix("~/"), files.env("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(dir),
    }
}

/// The installed cursor themes, sorted by title (ignoring case). Each name is listed once, from
/// the first directory of the [`search_path`] that has it.
pub fn themes(files: &impl Files) -> Vec<CursorTheme> {
    let mut themes: Vec<CursorTheme> = Vec::new();
    for dir in search_path(files) {
        let Ok(mut names) = files.list(&dir) else {
            continue;
        };
        names.sort();
        for name in names {
            if themes.iter().any(|theme| theme.name == name) {
                continue;
            }
            let theme_dir = dir.join(&name);
            if !files.exists(&theme_dir.join("cursors")) {
                continue;
            }
            let sizes = arrow(files, &theme_dir)
                .and_then(|bytes| xcursor::sizes(&bytes).ok())
                .unwrap_or_default();
            let title = index_name(files, &theme_dir).unwrap_or_else(|| name.clone());
            themes.push(CursorTheme {
                name,
                title,
                dir: theme_dir,
                sizes,
            });
        }
    }
    themes.sort_by_key(|theme| (theme.title.to_lowercase(), theme.name.clone()));
    themes
}

/// The bytes of the theme's arrow cursor, if one can be read.
fn arrow(files: &impl Files, dir: &Path) -> Option<Vec<u8>> {
    PREVIEW_CURSORS.iter().find_map(|name| {
        files
            .read_bounded(&dir.join("cursors").join(name), MAX_FILE_SIZE)
            .ok()
    })
}

/// The `Name=` in the `[Icon Theme]` group of the theme's `index.theme`.
fn index_name(files: &impl Files, dir: &Path) -> Option<String> {
    let bytes = files
        .read_bounded(&dir.join("index.theme"), MAX_INDEX_SIZE)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut in_group = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_group = line == "[Icon Theme]";
        } else if in_group {
            if let Some(name) = line
                .strip_prefix("Name")
                .and_then(|rest| rest.trim_start().strip_prefix('='))
            {
                let name = name.trim();
                if !name.is_empty() && !name.chars().any(char::is_control) {
                    return Some(name.to_owned());
                }
            }
        }
    }
    None
}
