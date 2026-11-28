// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where Sway Things keeps its files: the two settings files ([`Layer`]) and their history, the
//! links to the pictures of the wallpaper collections ([`Paths`]), and the generated sway files
//! with their manifest ([`Locations`]). Below `state` and `include`, which both use it.

use std::ffi::OsString;
use std::fmt;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::i18n::Text;
use crate::msg;
use crate::registry::{ConfigFile, Scope, Setting};

/// One of the two settings files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// `settings.toml`: shared between machines, e.g. in a dotfiles repository.
    Shared,
    /// `settings.d/<hostname>.toml`: this machine only. Wins over the shared file key by key.
    Host,
}

impl Layer {
    /// Both files, the shared one first.
    pub(crate) const ALL: [Self; 2] = [Self::Shared, Self::Host];

    /// The file a new value of `setting` goes to unless the user chooses otherwise: values bound
    /// to this machine's hardware go to the host overlay.
    pub fn default_for(setting: &Setting, device: Option<&str>) -> Self {
        if setting.host_by_default() {
            return Self::Host;
        }
        match (setting.scope(), device) {
            (Scope::Global | Scope::PerCollection, _)
            | (Scope::PerDevice(_) | Scope::PerOutput, None) => Self::Shared,
            (Scope::PerHost, _) | (Scope::PerDevice(_) | Scope::PerOutput, Some(_)) => Self::Host,
        }
    }
}

/// The paths of the two settings files and of their history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    shared: PathBuf,
    host: PathBuf,
    history: PathBuf,
    ui: PathBuf,
    displays: PathBuf,
    wallpaper: PathBuf,
    /// The home folder, which a `bg` line names the links by (`~/`); `None` in tests.
    home: Option<PathBuf>,
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
            ui: state_dir.join("ui.toml"),
            displays: config_dir.join("displays.toml"),
            wallpaper: state_dir.join("wallpaper"),
            home: None,
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
        let mut paths = Self::new(
            &config_home.join("sway-things"),
            &state_home.join("sway-things"),
            hostname,
        )?;
        paths.home = home().map(PathBuf::from).filter(|home| home.is_absolute());
        Ok(paths)
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
    pub fn relative(&self, layer: Layer) -> PathBuf {
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

    /// The state of the app's window that isn't a setting (`ui.toml` next to the history):
    /// which sections are collapsed. See [`UiState`](crate::ui_state::UiState).
    pub fn ui(&self) -> &Path {
        &self.ui
    }

    /// The setups of the displays remembered per combination (`displays.toml` next to the
    /// settings). See [`Profiles`](crate::outputs::profiles::Profiles).
    pub fn displays(&self) -> &Path {
        &self.displays
    }

    /// The link to the current picture of the wallpaper collection of `output`, or of every
    /// output: `all` or `output-<identifier>`, the identifier [escaped](escape), in
    /// `$XDG_STATE_HOME/sway-things/wallpaper`. The generated config names the link, so it stays
    /// the same while the pictures change.
    pub fn wallpaper_link(&self, output: Option<&str>) -> PathBuf {
        self.wallpaper.join(match output {
            None => "all".to_owned(),
            Some(identifier) => format!("output-{}", escape(identifier)),
        })
    }

    /// The link of [`Paths::wallpaper_link`] as a `bg` line names it: with `~/` for the home
    /// folder, so the line doesn't change with the user name. `None` while the link points at no
    /// picture: a `bg` line can't name it then.
    pub fn wallpaper_picture(&self, output: Option<&str>) -> Option<PathBuf> {
        let link = self.wallpaper_link(output);
        if !link.is_file() {
            return None;
        }
        Some(
            match self
                .home
                .as_deref()
                .and_then(|home| link.strip_prefix(home).ok())
            {
                Some(rest) => Path::new("~").join(rest),
                None => link,
            },
        )
    }
}

/// `identifier` as part of a file or unit name, the way `systemd-escape` writes it: ASCII
/// letters, digits, `:`, `_` and `.` (but not a leading `.`) stay, `/` becomes `-`, and every other
/// byte is `\xNN`. An output's identifier is an external string (make, model and serial from the
/// screen), so it can hold anything a file or unit name can't.
pub fn escape(identifier: &str) -> String {
    let mut escaped = String::with_capacity(identifier.len());
    for (index, byte) in identifier.bytes().enumerate() {
        match byte {
            b'/' => escaped.push('-'),
            b'.' if index > 0 => escaped.push('.'),
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b':' | b'_' => escaped.push(char::from(byte)),
            // Writing to a `String` can't fail.
            _ => drop(write!(escaped, "\\x{byte:02x}")),
        }
    }
    escaped
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

impl PathsError {
    /// What is missing, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::NoConfigDir => {
                msg!("neither XDG_CONFIG_HOME and XDG_STATE_HOME nor HOME are set").into()
            }
            Self::Hostname => msg!("the host name can't be used as a file name").into(),
        }
    }
}

impl fmt::Display for PathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for PathsError {}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Paths, PathsError, xdg_dir};
    use crate::paths::Layer;

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

/// Where the generated sway config files and their manifest are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locations {
    directory: PathBuf,
    manifest: PathBuf,
}

impl Locations {
    /// The generated files in `directory` (normally `$XDG_CONFIG_HOME/sway/sway-things`), with
    /// the manifest `manifest.toml` in `state_dir` (normally `$XDG_STATE_HOME/sway-things`).
    pub fn new(directory: &Path, state_dir: &Path) -> Self {
        Self {
            directory: directory.to_owned(),
            manifest: state_dir.join("manifest.toml"),
        }
    }

    /// The locations of this user: `$XDG_CONFIG_HOME/sway/sway-things` and
    /// `$XDG_STATE_HOME/sway-things/manifest.toml`, with the usual fallbacks in `$HOME`.
    ///
    /// # Errors
    ///
    /// [`PathsError::NoConfigDir`] if neither the variables nor `HOME` give the directories.
    pub fn from_env() -> Result<Self, PathsError> {
        let home = || std::env::var_os("HOME");
        let config_home = xdg_dir(std::env::var_os("XDG_CONFIG_HOME"), home(), ".config")
            .ok_or(PathsError::NoConfigDir)?;
        let state_home = xdg_dir(std::env::var_os("XDG_STATE_HOME"), home(), ".local/state")
            .ok_or(PathsError::NoConfigDir)?;
        Ok(Self::new(
            &config_home.join("sway").join("sway-things"),
            &state_home.join("sway-things"),
        ))
    }

    /// The directory of the generated files, which the user's sway config includes.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The path of `file`.
    pub fn path(&self, file: ConfigFile) -> PathBuf {
        self.directory.join(file.file_name())
    }

    /// The path of the manifest.
    pub fn manifest(&self) -> &Path {
        &self.manifest
    }
}
