// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The GTK themes and icon themes installed for the user, for the Apps Look & Feel page.
//!
//! A GTK theme is a directory with `gtk-3.0/gtk.css` or `gtk-4.0/gtk.css` in one of the `themes`
//! directories GTK searches ([`theme_dirs`]); a directory with only `gtk-keys.css` (Emacs key
//! bindings) is no look. GTK 3 builds in Adwaita and `HighContrast`, so those are always offered.
//! An icon theme is a directory in one of the `icons` directories ([`icon_dirs`]) whose
//! `index.theme` lists icon directories, which leaves out cursor-only themes; hicolor is the
//! fallback every theme inherits, not a look of its own. The name is the directory's, which is
//! what GSettings takes; a theme found in more than one directory counts where it is found first.
//!
//! Only reading: `index.theme` files are bounded in size and parsed as data.
//!
//! ```
//! use sway_things_core::look;
//! use sway_things_core::sway_config::MemoryFiles;
//!
//! let files = MemoryFiles::new()
//!     .var("HOME", "/home/me")
//!     .file("/usr/share/themes/Arc/gtk-3.0/gtk.css", "")
//!     .file("/usr/share/themes/Emacs/gtk-3.0/gtk-keys.css", "")
//!     .file(
//!         "/usr/share/icons/Papirus/index.theme",
//!         "[Icon Theme]\nName=Papirus\nDirectories=16x16/apps\n",
//!     )
//!     .file("/usr/share/icons/Bibata/index.theme", "[Icon Theme]\nName=Bibata\n");
//! let themes: Vec<String> = look::gtk_themes(&files).into_iter().map(|t| t.name).collect();
//! assert_eq!(themes, ["Adwaita", "Arc", "HighContrast"]);
//! let icons: Vec<String> = look::icon_themes(&files).into_iter().map(|t| t.name).collect();
//! assert_eq!(icons, ["Papirus"]);
//! ```

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use crate::sway_config::Files;

/// The themes GTK 3 builds in, found without a directory.
pub const BUILTIN_GTK_THEMES: [&str; 2] = ["Adwaita", "HighContrast"];

/// The icon theme every theme falls back to; not offered.
const FALLBACK_ICON_THEME: &str = "hicolor";

/// The largest `index.theme` read.
const MAX_INDEX_SIZE: usize = 256 * 1024;

/// An installed GTK theme or icon theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// The name of its directory, which GSettings takes.
    pub name: String,
    /// The name it gives itself in `index.theme`, or its directory name; with the directory name
    /// in parentheses if another theme has the same.
    pub title: String,
    /// The theme's directory, or `None` for a theme built into GTK.
    pub dir: Option<PathBuf>,
}

/// The data directories, in order: `$XDG_DATA_HOME` (or `~/.local/share`), then
/// `$XDG_DATA_DIRS` (or `/usr/local/share:/usr/share`). Relative entries are ignored, as the XDG
/// Base Directory spec asks.
fn data_dirs(files: &impl Files) -> Vec<PathBuf> {
    let home = files.env("HOME").filter(|home| !home.is_empty());
    let mut dirs = Vec::new();
    match files
        .env("XDG_DATA_HOME")
        .filter(|dir| dir.starts_with('/'))
    {
        Some(data_home) => dirs.push(PathBuf::from(data_home)),
        None => dirs.extend(home.map(|home| Path::new(&home).join(".local/share"))),
    }
    let data_dirs = files
        .env("XDG_DATA_DIRS")
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|dir| dir.starts_with('/'))
            .map(PathBuf::from),
    );
    dirs
}

/// `dirs` with `extra` (the old dot directory in the home) after the first one, each once.
fn with_home_dir(files: &impl Files, dirs: Vec<PathBuf>, extra: &str) -> Vec<PathBuf> {
    let mut all = Vec::with_capacity(dirs.len() + 1);
    let mut dirs = dirs.into_iter();
    all.extend(dirs.next());
    if let Some(home) = files.env("HOME").filter(|home| !home.is_empty()) {
        all.push(Path::new(&home).join(extra));
    }
    all.extend(dirs);
    let mut unique = Vec::with_capacity(all.len());
    for dir in all {
        if !unique.contains(&dir) {
            unique.push(dir);
        }
    }
    unique
}

/// The directories GTK searches for themes, in order: the `themes` directory of
/// `$XDG_DATA_HOME`, `~/.themes`, then those of `$XDG_DATA_DIRS`.
pub fn theme_dirs(files: &impl Files) -> Vec<PathBuf> {
    let dirs = data_dirs(files)
        .into_iter()
        .map(|dir| dir.join("themes"))
        .collect();
    with_home_dir(files, dirs, ".themes")
}

/// The directories searched for icon themes, in order: the `icons` directory of
/// `$XDG_DATA_HOME`, `~/.icons`, then those of `$XDG_DATA_DIRS`.
pub fn icon_dirs(files: &impl Files) -> Vec<PathBuf> {
    let dirs = data_dirs(files)
        .into_iter()
        .map(|dir| dir.join("icons"))
        .collect();
    with_home_dir(files, dirs, ".icons")
}

/// The installed GTK themes with those GTK builds in, sorted by title (ignoring case).
pub fn gtk_themes(files: &impl Files) -> Vec<Theme> {
    let mut themes = find(files, &theme_dirs(files), |files, dir| {
        ["gtk-3.0", "gtk-4.0"]
            .iter()
            .any(|version| files.exists(&dir.join(version).join("gtk.css")))
            .then(|| index_value(files, dir, "Desktop Entry", "Name"))
    });
    for name in BUILTIN_GTK_THEMES {
        if !themes.iter().any(|theme| theme.name == name) {
            themes.push(Theme {
                name: name.to_owned(),
                title: name.to_owned(),
                dir: None,
            });
        }
    }
    sort(&mut themes);
    themes
}

/// The installed icon themes, sorted by title (ignoring case).
pub fn icon_themes(files: &impl Files) -> Vec<Theme> {
    let mut themes = find(files, &icon_dirs(files), |files, dir| {
        let directories = index_value(files, dir, "Icon Theme", "Directories")?;
        if directories.split(',').all(|entry| entry.trim().is_empty()) {
            return None;
        }
        Some(index_value(files, dir, "Icon Theme", "Name"))
    });
    themes.retain(|theme| theme.name != FALLBACK_ICON_THEME);
    sort(&mut themes);
    themes
}

/// The themes in `dirs` that `accept` takes, with the title it reads (`Some(None)` for none).
fn find<F: Files>(
    files: &F,
    dirs: &[PathBuf],
    accept: impl Fn(&F, &Path) -> Option<Option<String>>,
) -> Vec<Theme> {
    let mut themes: Vec<Theme> = Vec::new();
    for dir in dirs {
        let Ok(mut names) = files.list(dir) else {
            continue;
        };
        names.sort();
        for name in names {
            if name.starts_with('.')
                || name.chars().any(char::is_control)
                || themes.iter().any(|theme| theme.name == name)
            {
                continue;
            }
            let theme_dir = dir.join(&name);
            let Some(title) = accept(files, &theme_dir) else {
                continue;
            };
            themes.push(Theme {
                title: title.unwrap_or_else(|| name.clone()),
                name,
                dir: Some(theme_dir),
            });
        }
    }
    themes
}

/// Sorts `themes` by title, and adds the directory name to a title more than one theme has, so
/// each can be told apart (and is found by its title).
fn sort(themes: &mut [Theme]) {
    let shared: Vec<String> = themes
        .iter()
        .filter(|theme| {
            themes
                .iter()
                .filter(|other| other.title == theme.title)
                .nth(1)
                .is_some()
        })
        .map(|theme| theme.title.clone())
        .collect();
    for theme in themes.iter_mut() {
        if shared.contains(&theme.title) {
            theme.title = format!("{} ({})", theme.title, theme.name);
        }
    }
    themes.sort_by_key(|theme| (theme.title.to_lowercase(), theme.name.clone()));
}

/// The value of `key` in `[group]` of the theme's `index.theme`, without a locale suffix.
/// GTK themes keep their name under `[Desktop Entry]` or `[X-GNOME-Metatheme]`; both are read for
/// the group `Desktop Entry`.
fn index_value(files: &impl Files, dir: &Path, group: &str, key: &str) -> Option<String> {
    let bytes = files
        .read_bounded(&dir.join("index.theme"), MAX_INDEX_SIZE)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let groups: &[&str] = if group == "Desktop Entry" {
        &["[Desktop Entry]", "[X-GNOME-Metatheme]"]
    } else {
        &[]
    };
    let header = format!("[{group}]");
    let mut in_group = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_group = line == header || groups.contains(&line);
        } else if in_group {
            if let Some(value) = line
                .strip_prefix(key)
                .and_then(|rest| rest.trim_start().strip_prefix('='))
            {
                let value = value.trim();
                if !value.is_empty() && !value.chars().any(char::is_control) {
                    return Some(value.to_owned());
                }
            }
        }
    }
    None
}
