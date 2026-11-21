// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Apps & Autostart page: the terminal and launcher the main keys open, the default apps,
//! and everything that starts with the session.
//!
//! - [`entry`]: desktop entries, read as the Desktop Entry Specification says, their command
//!   line split into arguments and never passed through a shell.
//! - [`installed`]: the installed apps under their desktop file IDs.
//! - [`launchers`]: the terminals and launchers, and the bindings of `$mod+Return` and `$mod+d`
//!   Sway Things writes for them through the bindings of the Keyboard Shortcuts page.
//! - [`mime`]: the default apps: what the MIME apps lists say now and which one wins, and our
//!   own `sway-mimeapps.list`.
//! - [`autostart`]: the `exec` lines of Sway Things in `70-autostart.conf`, the user's own `exec`
//!   lines, the XDG autostart entries with what runs them, and our `Hidden=true` overrides.
//! - [`files`]: the files of other programs Sway Things writes for the page, each only if it is
//!   its own.
//! - [`launch`]: starting an app by its desktop file ID, for `sway-things hook launch`.

pub mod autostart;
pub mod entry;
pub mod files;
pub mod installed;
pub mod launch;
pub mod launchers;
pub mod mime;
#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub use installed::App;

/// The XDG directories and the session's desktop, as the specifications read them from the
/// environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// `$XDG_CONFIG_HOME`, or `~/.config`.
    pub config_home: PathBuf,
    /// `$XDG_CONFIG_DIRS`, or `/etc/xdg`.
    pub config_dirs: Vec<PathBuf>,
    /// `$XDG_DATA_HOME`, or `~/.local/share`.
    pub data_home: PathBuf,
    /// `$XDG_DATA_DIRS`, or `/usr/local/share` and `/usr/share`.
    pub data_dirs: Vec<PathBuf>,
    /// The desktops of `XDG_CURRENT_DESKTOP`, in order, as written.
    pub desktops: Vec<String>,
    /// `PATH`, to find programs (`TryExec`).
    pub path: Option<OsString>,
    /// The languages to localize names for, most wanted first (`de_DE.UTF-8`).
    pub languages: Vec<String>,
}

impl Dirs {
    /// The directories of the environment of this process. `None` without a home directory
    /// (`HOME` unset or relative) where an XDG variable doesn't name one.
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("PATH");
        let mut dirs = Self::with(|name| std::env::var(name).ok())?;
        dirs.path = path;
        Some(dirs)
    }

    /// The directories from the variables `var` gives. `PATH` is read too.
    pub fn with(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let absolute = |name: &str| {
            var(name)
                .filter(|value| value.starts_with('/'))
                .map(PathBuf::from)
        };
        let home = absolute("HOME");
        let list = |name: &str, default: &[&str]| {
            let value = var(name).filter(|value| !value.is_empty());
            let dirs: Vec<PathBuf> = match value {
                Some(value) => value
                    .split(':')
                    .filter(|dir| dir.starts_with('/'))
                    .map(PathBuf::from)
                    .collect(),
                None => default.iter().map(PathBuf::from).collect(),
            };
            dirs
        };
        let config_home = absolute("XDG_CONFIG_HOME")
            .or_else(|| home.as_ref().map(|home| home.join(".config")))?;
        let data_home = absolute("XDG_DATA_HOME")
            .or_else(|| home.as_ref().map(|home| home.join(".local/share")))?;
        Some(Self {
            config_home,
            config_dirs: list("XDG_CONFIG_DIRS", &["/etc/xdg"]),
            data_home,
            data_dirs: list("XDG_DATA_DIRS", &["/usr/local/share", "/usr/share"]),
            desktops: var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .split(':')
                .filter(|desktop| !desktop.is_empty())
                .map(str::to_owned)
                .collect(),
            path: var("PATH").map(OsString::from),
            languages: crate::i18n::languages(&var),
        })
    }

    /// The data directories in the order they are searched: `data_home`, then `data_dirs`.
    pub fn data_search(&self) -> Vec<PathBuf> {
        let mut all = vec![self.data_home.clone()];
        for dir in &self.data_dirs {
            if !all.contains(dir) {
                all.push(dir.clone());
            }
        }
        all
    }

    /// The config directories in the order they are searched: `config_home`, then
    /// `config_dirs`.
    pub fn config_search(&self) -> Vec<PathBuf> {
        let mut all = vec![self.config_home.clone()];
        for dir in &self.config_dirs {
            if !all.contains(dir) {
                all.push(dir.clone());
            }
        }
        all
    }

    /// Whether `XDG_CURRENT_DESKTOP` names sway (without regard to case), so the MIME apps
    /// lists read `sway-mimeapps.list`.
    pub fn is_sway(&self) -> bool {
        self.desktops
            .iter()
            .any(|desktop| desktop.eq_ignore_ascii_case("sway"))
    }

    /// The program `name`: an absolute path if it is an executable file, else the executable of
    /// that name in `PATH`.
    pub fn find_program(&self, name: &str) -> Option<PathBuf> {
        if name.starts_with('/') {
            let path = Path::new(name);
            return is_executable(path).then(|| path.to_owned());
        }
        if name.contains('/') || name.is_empty() {
            return None;
        }
        crate::delegate::find_program(name, self.path.as_deref())
    }
}

/// Whether `path` is an executable file.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}
