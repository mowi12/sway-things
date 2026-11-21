// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The folder of screenshots and recordings and the names of their files.
//!
//! The folder is `$XDG_PICTURES_DIR/Screenshots` unless the setting names another one
//! (`~/` stands for the home folder); it is made when a file goes there. A file name comes from
//! a pattern with `strftime` conversions (`Screenshot from %Y-%m-%d %H-%M-%S.png`), filled in
//! with the local time by `GLib` (`g_date_time_format`). Where a file of that name exists (two
//! screenshots in one second), the name gets a number: `… (2).png`. A new file is created only
//! if no file of its name exists, so nothing is ever overwritten.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use gio::glib;

use crate::registry::ValueError;

/// What the folder of screenshots depends on: the home folder and the pictures folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    /// `$HOME`.
    pub home: Option<PathBuf>,
    /// `XDG_PICTURES_DIR` from `user-dirs.dirs`, if it is set.
    pub pictures: Option<PathBuf>,
}

impl Env {
    /// The home folder and pictures folder of this process, as `GLib` reads them.
    pub fn process() -> Self {
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from);
        Self {
            home,
            pictures: glib::user_special_dir(glib::UserDirectory::Pictures),
        }
    }
}

/// The folder of screenshots and recordings for the setting's `text`: empty for the default,
/// `$XDG_PICTURES_DIR/Screenshots` (or `~/Pictures/Screenshots` without one); `~/…` below the
/// home folder.
pub fn folder(text: &str, env: &Env) -> PathBuf {
    let home = env.home.clone().unwrap_or_else(|| PathBuf::from("/"));
    if text.is_empty() {
        let pictures = env
            .pictures
            .clone()
            .filter(|pictures| pictures.is_absolute() && *pictures != home)
            .unwrap_or_else(|| home.join("Pictures"));
        return pictures.join("Screenshots");
    }
    if text == "~" {
        return home;
    }
    match text.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(text),
    }
}

/// The check of the folder setting: empty, or a path that starts with `/` or `~/`, without
/// control characters.
///
/// # Errors
///
/// [`ValueError::NotAFolder`].
pub fn check_folder(text: &str) -> Result<(), ValueError> {
    let rooted = text.is_empty() || text.starts_with('/') || text == "~" || text.starts_with("~/");
    if rooted && !text.chars().any(char::is_control) {
        Ok(())
    } else {
        Err(ValueError::NotAFolder)
    }
}

/// The file name `pattern` makes at `time`, or `None` if it can't make one: empty, a slash or a
/// control character, an unknown conversion, or a name of only dots.
pub fn file_name(pattern: &str, time: &glib::DateTime) -> Option<String> {
    if pattern.is_empty() || pattern.contains('/') || pattern.chars().any(char::is_control) {
        return None;
    }
    let name = time.format(pattern).ok()?.to_string();
    let usable = !name.is_empty()
        && !name.contains('/')
        && !name.chars().any(char::is_control)
        && !name.chars().all(|c| c == '.');
    usable.then_some(name)
}

/// The check of a file name pattern setting: it must make a file name now.
///
/// # Errors
///
/// [`ValueError::NotAFileName`].
pub fn check_file_name(pattern: &str) -> Result<(), ValueError> {
    let now = glib::DateTime::now_local().map_err(|_| ValueError::NotAFileName)?;
    file_name(pattern, &now)
        .map(|_| ())
        .ok_or(ValueError::NotAFileName)
}

/// `name` with one of `extensions` (in lowercase, without the dot): as it is if it ends with
/// one of them (in any case), else with the first appended. Returns the name and its extension.
pub fn with_extension(name: &str, extensions: &[&'static str]) -> (String, &'static str) {
    let ext = Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    if let Some(known) = extensions
        .iter()
        .find(|known| ext.as_deref() == Some(**known))
    {
        (name.to_owned(), known)
    } else {
        let first = extensions.first().copied().unwrap_or("png");
        (format!("{name}.{first}"), first)
    }
}

/// The `n`-th name for `name` (counted from 1): `name` itself, then `stem (n).ext`.
pub fn numbered(name: &str, n: u32) -> String {
    if n <= 1 {
        return name.to_owned();
    }
    let path = Path::new(name);
    match (
        path.file_stem().and_then(|stem| stem.to_str()),
        path.extension().and_then(|ext| ext.to_str()),
    ) {
        (Some(stem), Some(ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
        // A name that is only an extension (`.png`) keeps it at its end.
        _ if name.starts_with('.') && !name[1..].contains('.') => format!("({n}){name}"),
        _ => format!("{name} ({n})"),
    }
}

/// The most numbered names tried before giving up.
const MAX_TRIES: u32 = 1000;

/// The first path in `folder` for `name` that no file has yet ([`numbered`]); `None` after
/// [`MAX_TRIES`] names. Only for showing the next name: a file is made with [`write_new`].
pub fn free_path(folder: &Path, name: &str) -> Option<PathBuf> {
    (1..=MAX_TRIES)
        .map(|n| folder.join(numbered(name, n)))
        .find(|path| fs::symlink_metadata(path).is_err())
}

/// Makes `folder` (and the folders above it) if it doesn't exist.
///
/// # Errors
///
/// The error of making it.
pub fn make_folder(folder: &Path) -> io::Result<()> {
    fs::create_dir_all(folder)
}

/// Writes `bytes` to a new file in `folder` named after `name`, numbered if a file of that name
/// exists ([`numbered`]); makes the folder if needed. A file that exists is never written.
///
/// # Errors
///
/// The error of making the folder or writing the file.
pub fn write_new(folder: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    make_folder(folder)?;
    for n in 1..=MAX_TRIES {
        let path = folder.join(numbered(name, n));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                let written = file.write_all(bytes).and_then(|()| file.sync_all());
                if let Err(error) = written {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(error);
                }
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

/// The name of the next file `pattern` makes in `folder` now, for the page to show; `None` if
/// the pattern can't make one.
pub fn next_name(pattern: &str, folder: &Path, extensions: &[&'static str]) -> Option<String> {
    let now = glib::DateTime::now_local().ok()?;
    let (name, _) = with_extension(&file_name(pattern, &now)?, extensions);
    let path = free_path(folder, &name)?;
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
}
