// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the files for the notification daemons are, whether mako's config includes ours, and
//! writing ours. A file of ours that doesn't start with our header (`$comment` in swaync's
//! `config.json`) is someone else's and never written; the user's mako config is only created,
//! with the include line alone, when there is none.

use std::io;
use std::path::{Path, PathBuf};

use super::HEADER;
use crate::atomic;
use crate::i18n::Text;
use crate::msg;

/// The files of the notification daemons for this user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Files {
    /// `$HOME`, for writing paths with `~/`.
    home: Option<PathBuf>,
    /// The mako config mako reads: the one the running mako was given with `-c`, else
    /// `~/.mako/config` if there is one, else `$XDG_CONFIG_HOME/mako/config`
    /// (`get_default_config_path` in mako's `config.c`).
    pub mako_config: PathBuf,
    /// Whether `mako_config` is the one the running mako was given with `-c`: it is never
    /// created then.
    pub mako_given: bool,
    /// Ours for mako.
    pub mako_ours: PathBuf,
    /// Our `config.json` for swaync.
    pub swaync_config: PathBuf,
    /// Our `style.css` for swaync.
    pub swaync_style: PathBuf,
    /// The user's own swaync config, which an import reads.
    pub swaync_user_config: PathBuf,
}

/// Whether the user's mako config includes ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inclusion {
    /// It does, among its global options.
    Included,
    /// It does, but after a `[section]`, where mako refuses `include=` and so the whole config.
    InSection,
    /// It names ours in a way mako refuses, and with it the whole config: with blanks around
    /// `=`, or a path that starts with neither `/` nor `~/` (mako expands no variables).
    Malformed,
    /// There is a config without the line.
    Missing,
    /// There is no config: it is created with the line.
    NoConfig,
}

/// Why a file for a notification daemon wasn't written.
#[derive(Debug)]
pub enum FileError {
    /// The file is there, but Sway Things didn't write it.
    Foreign(PathBuf),
    /// It couldn't be written.
    Io(PathBuf, io::Error),
}

impl FileError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Foreign(path) => Text::new(msg!(
                "{file} wasn't written by Sway Things, so it is left alone"
            ))
            .arg("file", path.display()),
            Self::Io(path, error) => Text::new(msg!("{file} can't be written: {error}"))
                .arg("file", path.display())
                .arg("error", error),
        }
    }
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for FileError {}

impl Files {
    /// The files for this user, from `$HOME` and `$XDG_CONFIG_HOME`. `None` without either.
    pub fn from_env() -> Option<Self> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| home.as_ref().map(|home| home.join(".config")))?;
        Some(Self::new(home, &config_home))
    }

    /// The files below `config_home`, with `home` for `~/.mako/config` and paths with `~/`.
    pub fn new(home: Option<PathBuf>, config_home: &Path) -> Self {
        let dot_mako = home.as_ref().map(|home| home.join(".mako/config"));
        let mako_config = dot_mako
            .filter(|path| path.is_file())
            .unwrap_or_else(|| config_home.join("mako/config"));
        let ours = config_home.join("sway-things/swaync");
        Self {
            home,
            mako_config,
            mako_given: false,
            mako_ours: config_home.join("mako/sway-things"),
            swaync_config: ours.join("config.json"),
            swaync_style: ours.join("style.css"),
            swaync_user_config: config_home.join("swaync/config.json"),
        }
    }

    /// These files with `config` as mako's config: the one the running mako was given with
    /// `-c` ([`crate::notifications::starts::config_arg`]).
    #[must_use]
    pub fn with_mako_config(mut self, config: PathBuf) -> Self {
        self.mako_config = config;
        self.mako_given = true;
        self
    }

    /// A system-wide mako config in `$XDG_CONFIG_DIRS/mako/config` (`/etc/xdg` by default), if
    /// there is one. mako 1.11 doesn't read it, but a setup that has one keeps its config there,
    /// so none is created in the user's place.
    pub fn mako_system_config() -> Option<PathBuf> {
        let dirs = std::env::var("XDG_CONFIG_DIRS")
            .ok()
            .filter(|dirs| !dirs.is_empty())
            .unwrap_or_else(|| "/etc/xdg".to_owned());
        dirs.split(':')
            .filter(|dir| dir.starts_with('/'))
            .map(|dir| Path::new(dir).join("mako/config"))
            .find(|path| path.is_file())
    }

    /// Where our file for mako lands when its directory is a symbolic link (into a dotfiles
    /// repository, say): the resolved path, or `None` if it is where it says.
    pub fn mako_ours_target(&self) -> Option<PathBuf> {
        let dir = self.mako_ours.parent()?;
        let resolved = std::fs::canonicalize(dir).ok()?;
        let name = self.mako_ours.file_name()?;
        (resolved != dir).then(|| resolved.join(name))
    }

    /// `path` with `~/` for the home directory where it is in there.
    pub fn shown(&self, path: &Path) -> String {
        self.home
            .as_deref()
            .and_then(|home| path.strip_prefix(home).ok())
            .map_or_else(
                || path.display().to_string(),
                |relative| format!("~/{}", relative.display()),
            )
    }

    /// The line the user's mako config needs: `include=` of ours.
    pub fn mako_include_line(&self) -> String {
        format!("include={}", self.shown(&self.mako_ours))
    }

    /// Whether the user's mako config includes ours.
    pub fn mako_inclusion(&self) -> Inclusion {
        match std::fs::read_to_string(&self.mako_config) {
            Ok(text) => self.inclusion_in(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Inclusion::NoConfig,
            Err(_) => Inclusion::Missing,
        }
    }

    /// Whether `text`, a mako config, includes ours: `include=` with our path, absolute or with
    /// `~/` as mako expands it.
    pub fn inclusion_in(&self, text: &str) -> Inclusion {
        let mut in_section = false;
        let mut malformed = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') && line.ends_with(']') {
                in_section = true;
                continue;
            }
            let Some(path) = line.strip_prefix("include=") else {
                // `include = …` or a path mako can't expand, of a file named like ours.
                let names_ours = self
                    .mako_ours
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| line.contains(name));
                malformed |= line.starts_with("include") && names_ours;
                continue;
            };
            if !(path.starts_with('/') || path.starts_with("~/")) {
                malformed |= path.contains("sway-things");
                continue;
            }
            let expanded = match (path.strip_prefix("~/"), &self.home) {
                (Some(rest), Some(home)) => home.join(rest),
                _ => PathBuf::from(path),
            };
            if expanded == self.mako_ours {
                return if in_section {
                    Inclusion::InSection
                } else {
                    Inclusion::Included
                };
            }
        }
        if malformed {
            Inclusion::Malformed
        } else {
            Inclusion::Missing
        }
    }

    /// Whether a mako config with only the include line may be created: there is none, the
    /// running mako wasn't given one with `-c`, and there is no system-wide one.
    pub fn may_create_mako_config(&self) -> bool {
        !self.mako_given
            && self.mako_inclusion() == Inclusion::NoConfig
            && Self::mako_system_config().is_none()
    }

    /// Writes `text` to ours for mako, unless it holds that already, and with `create` (mako is
    /// the daemon chosen on the page) creates the user's mako config with the include line if
    /// [`Files::may_create_mako_config`]. Says whether anything was written.
    ///
    /// # Errors
    ///
    /// [`FileError::Foreign`] if ours is someone else's, else the error of writing.
    pub fn write_mako(&self, text: &str, create: bool) -> Result<bool, FileError> {
        let mut written = write_ours(&self.mako_ours, text, |old| {
            old.starts_with(&format!("# {HEADER}"))
        })?;
        if create && self.may_create_mako_config() {
            let config = format!(
                "# Created by Sway Things, as there was no mako config. It is yours: add your own \
                 options\n# above the include line, and sections below it.\n{}\n",
                self.mako_include_line()
            );
            written |= create_new(&self.mako_config, &config)?;
        }
        Ok(written)
    }

    /// Writes our `config.json` and `style.css` for swaync, unless they hold that already. Says
    /// whether anything was written.
    ///
    /// # Errors
    ///
    /// [`FileError::Foreign`] if one is someone else's, else the error of writing.
    pub fn write_swaync(&self, config: &str, style: &str) -> Result<bool, FileError> {
        let config_written = write_ours(&self.swaync_config, config, |old| {
            old.contains(&format!("\"$comment\": \"{HEADER}\""))
        })?;
        let style_written = write_ours(&self.swaync_style, style, |old| {
            old.starts_with(&format!("/* {HEADER}"))
        })?;
        Ok(config_written || style_written)
    }

    /// Whether ours for mako is someone else's.
    pub fn mako_is_foreign(&self) -> bool {
        is_foreign(&self.mako_ours, |old| {
            old.starts_with(&format!("# {HEADER}"))
        })
    }
}

/// Whether `path` is there, not empty, and not ours by `is_ours`.
fn is_foreign(path: &Path, is_ours: impl Fn(&str) -> bool) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| !text.trim().is_empty() && !is_ours(&text))
}

/// Writes `text` to `path` unless it holds it already; an empty file is ours to fill, one with
/// something in it only if `is_ours` says so.
fn write_ours(path: &Path, text: &str, is_ours: impl Fn(&str) -> bool) -> Result<bool, FileError> {
    match std::fs::read_to_string(path) {
        Ok(old) if old == text => return Ok(false),
        Ok(old) if !old.trim().is_empty() && !is_ours(&old) => {
            return Err(FileError::Foreign(path.to_owned()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(FileError::Io(path.to_owned(), error)),
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| FileError::Io(dir.to_owned(), error))?;
    }
    atomic::write(path, text).map_err(|error| FileError::Io(path.to_owned(), error))?;
    Ok(true)
}

/// Creates `path` with `text` only if it still isn't there: a file the user made meanwhile stays.
fn create_new(path: &Path, text: &str) -> Result<bool, FileError> {
    use std::io::Write as _;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| FileError::Io(dir.to_owned(), error))?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(text.as_bytes())
                .map_err(|error| FileError::Io(path.to_owned(), error))?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(FileError::Io(path.to_owned(), error)),
    }
}
