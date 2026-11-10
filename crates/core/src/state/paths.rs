// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the settings files and their history are.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use super::Layer;

/// The paths of the two settings files and of their history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    shared: PathBuf,
    host: PathBuf,
    history: PathBuf,
}

impl Paths {
    /// The files in `config_dir` (normally `$XDG_CONFIG_HOME/sway-things`) for the machine called
    /// `hostname`: `settings.toml` and `settings.d/<hostname>.toml`. Their history is the
    /// directory `history` in `state_dir` (normally `$XDG_STATE_HOME/sway-things`).
    ///
    /// # Errors
    ///
    /// [`PathsError::Hostname`] if `hostname` can't be a file name.
    pub fn new(config_dir: &Path, state_dir: &Path, hostname: &str) -> Result<Self, PathsError> {
        if hostname.is_empty()
            || hostname.starts_with('.')
            || hostname
                .chars()
                .any(|char| char == '/' || char.is_control())
        {
            return Err(PathsError::Hostname);
        }
        Ok(Self {
            shared: config_dir.join("settings.toml"),
            host: config_dir
                .join("settings.d")
                .join(format!("{hostname}.toml")),
            history: state_dir.join("history"),
        })
    }

    /// The files of this machine and user: the settings in `$XDG_CONFIG_HOME/sway-things`, their
    /// history in `$XDG_STATE_HOME/sway-things`. A variable that is unset, empty or relative
    /// falls back to `$HOME/.config` or `$HOME/.local/state`. The host name is the kernel's
    /// (`uname -n`).
    ///
    /// # Errors
    ///
    /// [`PathsError::NoConfigDir`] if neither the variables nor `HOME` give the directories,
    /// [`PathsError::Hostname`] if the host name can't be a file name.
    pub fn from_env() -> Result<Self, PathsError> {
        let home = || std::env::var_os("HOME");
        let config_home = xdg_dir(std::env::var_os("XDG_CONFIG_HOME"), home(), ".config")
            .ok_or(PathsError::NoConfigDir)?;
        let state_home = xdg_dir(std::env::var_os("XDG_STATE_HOME"), home(), ".local/state")
            .ok_or(PathsError::NoConfigDir)?;
        let uname = rustix::system::uname();
        let hostname = uname
            .nodename()
            .to_str()
            .map_err(|_| PathsError::Hostname)?;
        Self::new(
            &config_home.join("sway-things"),
            &state_home.join("sway-things"),
            hostname,
        )
    }

    /// The file of `layer`.
    pub fn path(&self, layer: Layer) -> &Path {
        match layer {
            Layer::Shared => &self.shared,
            Layer::Host => &self.host,
        }
    }

    /// The path of the file of `layer` relative to the config directory, which is also its path
    /// in a snapshot.
    pub(super) fn relative(&self, layer: Layer) -> PathBuf {
        let name = self.path(layer).file_name().unwrap_or_default();
        match layer {
            Layer::Shared => PathBuf::from(name),
            Layer::Host => Path::new("settings.d").join(name),
        }
    }

    /// The directory with the snapshots of the settings files.
    pub fn history(&self) -> &Path {
        &self.history
    }
}

/// The XDG base directory in `variable` if it is an absolute path (the specification says to
/// ignore relative ones), else `fallback` in `$HOME`.
pub(crate) fn xdg_dir(
    variable: Option<OsString>,
    home: Option<OsString>,
    fallback: &str,
) -> Option<PathBuf> {
    let absolute = |dir: OsString| Some(PathBuf::from(dir)).filter(|dir| dir.is_absolute());
    variable
        .and_then(absolute)
        .or_else(|| home.and_then(absolute).map(|home| home.join(fallback)))
}

/// Why the settings files can't be located.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathsError {
    /// Neither `XDG_CONFIG_HOME` and `XDG_STATE_HOME` nor `HOME` are absolute paths.
    NoConfigDir,
    /// The host name is empty, starts with a dot, or contains a slash or a control character, so
    /// it can't name the overlay file.
    Hostname,
}

impl fmt::Display for PathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoConfigDir => {
                f.write_str("neither XDG_CONFIG_HOME and XDG_STATE_HOME nor HOME are set")
            }
            Self::Hostname => f.write_str("the host name can't be used as a file name"),
        }
    }
}

impl std::error::Error for PathsError {}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Paths, PathsError, xdg_dir};
    use crate::state::Layer;

    #[test]
    fn files_are_named_after_the_host() {
        let paths = Paths::new(
            Path::new("/home/me/.config/sway-things"),
            Path::new("/home/me/.local/state/sway-things"),
            "laptop",
        )
        .unwrap();
        assert_eq!(
            paths.path(Layer::Shared),
            Path::new("/home/me/.config/sway-things/settings.toml")
        );
        assert_eq!(
            paths.path(Layer::Host),
            Path::new("/home/me/.config/sway-things/settings.d/laptop.toml")
        );
        assert_eq!(
            paths.history(),
            Path::new("/home/me/.local/state/sway-things/history")
        );
    }

    #[test]
    fn host_names_that_leave_the_directory_are_rejected() {
        for hostname in ["", ".", "..", ".hidden", "a/b", "../x", "a\nb"] {
            assert_eq!(
                Paths::new(Path::new("/c"), Path::new("/s"), hostname),
                Err(PathsError::Hostname),
                "{hostname:?}"
            );
        }
    }

    #[test]
    fn an_xdg_variable_wins_if_absolute() {
        let dir = |xdg: Option<&str>, home: Option<&str>| {
            xdg_dir(xdg.map(Into::into), home.map(Into::into), ".config")
        };
        assert_eq!(
            dir(Some("/xdg"), Some("/home/me")),
            Some(PathBuf::from("/xdg"))
        );
        for xdg in [None, Some(""), Some("relative")] {
            assert_eq!(
                dir(xdg, Some("/home/me")),
                Some(PathBuf::from("/home/me/.config"))
            );
        }
        assert_eq!(
            xdg_dir(None, Some("/home/me".into()), ".local/state"),
            Some(PathBuf::from("/home/me/.local/state"))
        );
        assert_eq!(dir(None, None), None);
        assert_eq!(dir(None, Some("")), None);
    }

    #[test]
    fn this_machine_has_paths() {
        // Only if the environment of the test run has a home.
        if let Ok(paths) = Paths::from_env() {
            assert!(
                paths
                    .path(Layer::Shared)
                    .ends_with("sway-things/settings.toml")
            );
            assert_eq!(
                paths
                    .path(Layer::Host)
                    .extension()
                    .and_then(|ext| ext.to_str()),
                Some("toml")
            );
            assert!(paths.history().ends_with("sway-things/history"));
        }
    }
}
