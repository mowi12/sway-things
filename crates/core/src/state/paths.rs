// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the settings files are.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use super::Layer;

/// The paths of the two settings files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    shared: PathBuf,
    host: PathBuf,
}

impl Paths {
    /// The files in `config_dir` (normally `$XDG_CONFIG_HOME/sway-things`) for the machine called
    /// `hostname`: `settings.toml` and `settings.d/<hostname>.toml`.
    ///
    /// # Errors
    ///
    /// [`PathsError::Hostname`] if `hostname` can't be a file name.
    pub fn new(config_dir: &Path, hostname: &str) -> Result<Self, PathsError> {
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
        })
    }

    /// The files of this machine and user: in `$XDG_CONFIG_HOME/sway-things`, or
    /// `$HOME/.config/sway-things` if that variable is unset, empty or relative; the host name is
    /// the kernel's (`uname -n`).
    ///
    /// # Errors
    ///
    /// [`PathsError::NoConfigDir`] without a usable `XDG_CONFIG_HOME` and `HOME`,
    /// [`PathsError::Hostname`] if the host name can't be a file name.
    pub fn from_env() -> Result<Self, PathsError> {
        let config_home = config_home(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("HOME"),
        )
        .ok_or(PathsError::NoConfigDir)?;
        let uname = rustix::system::uname();
        let hostname = uname
            .nodename()
            .to_str()
            .map_err(|_| PathsError::Hostname)?;
        Self::new(&config_home.join("sway-things"), hostname)
    }

    /// The file of `layer`.
    pub fn path(&self, layer: Layer) -> &Path {
        match layer {
            Layer::Shared => &self.shared,
            Layer::Host => &self.host,
        }
    }
}

/// `$XDG_CONFIG_HOME` if it is an absolute path (the XDG base directory specification says to
/// ignore relative ones), else `$HOME/.config`.
fn config_home(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let absolute = |dir: OsString| Some(PathBuf::from(dir)).filter(|dir| dir.is_absolute());
    xdg_config_home
        .and_then(absolute)
        .or_else(|| home.and_then(absolute).map(|home| home.join(".config")))
}

/// Why the settings files can't be located.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathsError {
    /// Neither `XDG_CONFIG_HOME` nor `HOME` is an absolute path.
    NoConfigDir,
    /// The host name is empty, starts with a dot, or contains a slash or a control character, so
    /// it can't name the overlay file.
    Hostname,
}

impl fmt::Display for PathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoConfigDir => f.write_str("neither XDG_CONFIG_HOME nor HOME is set"),
            Self::Hostname => f.write_str("the host name can't be used as a file name"),
        }
    }
}

impl std::error::Error for PathsError {}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Paths, PathsError, config_home};
    use crate::state::Layer;

    #[test]
    fn files_are_named_after_the_host() {
        let paths = Paths::new(Path::new("/home/me/.config/sway-things"), "laptop").unwrap();
        assert_eq!(
            paths.path(Layer::Shared),
            Path::new("/home/me/.config/sway-things/settings.toml")
        );
        assert_eq!(
            paths.path(Layer::Host),
            Path::new("/home/me/.config/sway-things/settings.d/laptop.toml")
        );
    }

    #[test]
    fn host_names_that_leave_the_directory_are_rejected() {
        for hostname in ["", ".", "..", ".hidden", "a/b", "../x", "a\nb"] {
            assert_eq!(
                Paths::new(Path::new("/c"), hostname),
                Err(PathsError::Hostname),
                "{hostname:?}"
            );
        }
    }

    #[test]
    fn xdg_config_home_wins_if_absolute() {
        let dir = |xdg: Option<&str>, home: Option<&str>| {
            config_home(xdg.map(Into::into), home.map(Into::into))
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
        }
    }
}
