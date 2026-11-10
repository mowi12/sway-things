// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings state: what the user changed, in two TOML files.
//!
//! `settings.toml` holds the values shared between machines, `settings.d/<hostname>.toml` the
//! overlay of this machine, which wins key by key. Both are sparse: a key that isn't stored isn't
//! managed by Sway Things. Values for one input device are stored below `device."<identifier>"`,
//! at the same key as the value for all devices of the type:
//!
//! ```toml
//! version = 1
//!
//! [input.touchpad]
//! tap = true
//!
//! [device."1267:12377:ELAN1300:00_04F3:3059_Touchpad".input.touchpad]
//! tap = false
//! ```
//!
//! The files may be edited by hand. Loading reports what doesn't fit the registry as
//! [`Problem`]s and loads the rest; saving keeps comments, order and unknown keys.
//! [`State::reload`] picks up changes made outside, and every save first keeps the files it
//! replaces as a [`Snapshot`] that [`State::restore`] brings back.
//!
//! ```
//! use sway_things_core::registry::{Registry, Value};
//! use sway_things_core::state::{Layer, Paths, State};
//!
//! # let dir = tempfile::tempdir()?;
//! # let (config_dir, state_dir) = (&dir.path().join("config"), &dir.path().join("state"));
//! let paths = Paths::new(config_dir, state_dir, "laptop")?;
//! let mut state = State::load(Registry::builtin(), paths);
//! state.set("windows.gaps.inner", None, Value::Integer(8), None)?;
//! state.save()?;
//!
//! assert_eq!(state.get("windows.gaps.inner", None), Some(&Value::Integer(8)));
//! assert_eq!(state.layer("windows.gaps.inner", None), Some(Layer::Shared));
//! assert_eq!(
//!     std::fs::read_to_string(config_dir.join("settings.toml"))?,
//!     "version = 1\n\n[windows.gaps]\ninner = 8\n"
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod file;
mod history;
mod paths;
#[cfg(test)]
mod tests;
mod write;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::command::input::InputSelector;
use crate::registry::{Registry, Scope, Setting, Value, ValueError, Values};
use file::{FileError, SettingsFile};

pub use history::Snapshot;
pub use paths::{Paths, PathsError};

/// One of the two settings files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// `settings.toml`: shared between machines, e.g. in a dotfiles repository.
    Shared,
    /// `settings.d/<hostname>.toml`: this machine only. Wins over the shared file key by key.
    Host,
}

impl Layer {
    const ALL: [Self; 2] = [Self::Shared, Self::Host];

    /// The file a new value of `setting` goes to unless the user chooses otherwise: values bound
    /// to this machine's hardware go to the host overlay.
    pub fn default_for(setting: &Setting, device: Option<&str>) -> Self {
        match (setting.scope(), device) {
            (Scope::Global, _) | (Scope::PerDevice(_), None) => Self::Shared,
            (Scope::PerHost | Scope::PerOutput, _) | (Scope::PerDevice(_), Some(_)) => Self::Host,
        }
    }
}

/// A stored value.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    /// The setting.
    pub setting: &'static Setting,
    /// The input device the value is for, or `None` for the whole session or input type.
    pub device: Option<&'a str>,
    /// The value in effect.
    pub value: &'a Value,
    /// The file the value comes from.
    pub layer: Layer,
}

/// Something in a settings file that Sway Things can't use. Nothing is dropped: the line stays in
/// the file, and the rest of the file still loads.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    /// The file.
    pub layer: Layer,
    /// The path of the file.
    pub path: PathBuf,
    /// What is wrong.
    pub kind: ProblemKind,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.kind)
    }
}

/// What is wrong in a settings file.
#[derive(Debug, Clone, PartialEq)]
pub enum ProblemKind {
    /// The file can't be read. Nothing in it is loaded, and it isn't written.
    Unreadable {
        /// The error of the operating system.
        message: String,
    },
    /// The file isn't valid TOML. Nothing in it is loaded, and it isn't written.
    Syntax {
        /// The parser's message, with line and column.
        message: String,
    },
    /// The file's `version` isn't one this build knows. Nothing in it is loaded, and it isn't
    /// written.
    UnsupportedVersion {
        /// The version as written in the file.
        found: String,
    },
    /// No setting has this key.
    UnknownKey {
        /// The key.
        key: String,
        /// The device table the key is in, if any.
        device: Option<String>,
    },
    /// The value doesn't fit the setting.
    InvalidValue {
        /// The key.
        key: &'static str,
        /// The device table the key is in, if any.
        device: Option<String>,
        /// What is wrong with the value.
        error: ValueError,
    },
    /// An entry of the `device` table isn't a table named after a device identifier.
    InvalidDevice {
        /// The name of the entry.
        device: String,
    },
    /// A device table holds a setting that can't be set per device.
    NotPerDevice {
        /// The key.
        key: &'static str,
        /// The device.
        device: String,
    },
}

impl fmt::Display for ProblemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let place = |f: &mut fmt::Formatter<'_>, key: &str, device: &Option<String>| match device {
            Some(device) => write!(f, "{key} (device {device})"),
            None => f.write_str(key),
        };
        match self {
            Self::Unreadable { message } => write!(f, "can't be read: {message}"),
            Self::Syntax { message } => write!(f, "not valid TOML: {}", message.trim_end()),
            Self::UnsupportedVersion { found } => write!(
                f,
                "version {found} is not supported (this Sway Things reads version {})",
                file::VERSION
            ),
            Self::UnknownKey { key, device } => {
                f.write_str("unknown setting ")?;
                place(f, key, device)
            }
            Self::InvalidValue { key, device, error } => {
                place(f, key, device)?;
                write!(f, ": {error}")
            }
            Self::InvalidDevice { device } => {
                write!(
                    f,
                    "device.{device:?} is not a table for a device identifier"
                )
            }
            Self::NotPerDevice { key, device } => {
                write!(
                    f,
                    "{key} can't be set for a single device (device {device})"
                )
            }
        }
    }
}

/// The settings the user changed, from the shared file and the host overlay.
#[derive(Debug, Clone)]
pub struct State {
    registry: &'static Registry,
    paths: Paths,
    shared: SettingsFile,
    host: SettingsFile,
    shared_disk: Disk,
    host_disk: Disk,
}

/// What was on disk when a file was last loaded or saved.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Disk {
    Missing,
    Text(String),
    Unreadable(String),
}

impl Disk {
    fn read(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::Text(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Unreadable(error.to_string()),
        }
    }

    fn parse(&self, registry: &'static Registry) -> SettingsFile {
        match self {
            Self::Missing => SettingsFile::empty(),
            Self::Text(text) => SettingsFile::parse(text, registry),
            Self::Unreadable(message) => SettingsFile::unusable(ProblemKind::Unreadable {
                message: message.clone(),
            }),
        }
    }
}

/// What a [`State::reload`] or a [`State::restore`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// The files that were replaced in memory. Their [problems](State::problems) may have
    /// changed.
    pub layers: Vec<Layer>,
    /// The settings whose value in effect changed, was added or was removed, ordered by device
    /// (those without one first) and key.
    pub keys: Vec<ChangedKey>,
}

impl Changes {
    /// Whether no file was replaced.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

/// A setting whose stored value changed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChangedKey {
    /// The input device the value is for, or `None` for the whole session or input type.
    pub device: Option<String>,
    /// The key of the setting.
    pub key: &'static str,
}

impl State {
    /// Loads both files. A file that doesn't exist is empty; whatever can't be used is in
    /// [`State::problems`].
    pub fn load(registry: &'static Registry, paths: Paths) -> Self {
        let shared_disk = Disk::read(paths.path(Layer::Shared));
        let host_disk = Disk::read(paths.path(Layer::Host));
        Self {
            registry,
            shared: shared_disk.parse(registry),
            host: host_disk.parse(registry),
            shared_disk,
            host_disk,
            paths,
        }
    }

    /// Loads the files that changed on disk since they were last loaded or saved, e.g. after a
    /// hand edit or a `git pull` of the dotfiles, and returns what changed. A file this state
    /// wrote itself hasn't changed, so a reload after [`State::save`] does nothing.
    ///
    /// The file on disk wins: changes to a reloaded file that weren't saved yet are dropped, and
    /// their keys are among the changed ones.
    pub fn reload(&mut self) -> Changes {
        let before = self.in_effect();
        let mut layers = Vec::new();
        for layer in Layer::ALL {
            let disk = Disk::read(self.paths.path(layer));
            if disk != *self.disk(layer) {
                *self.file_mut(layer) = disk.parse(self.registry);
                *self.disk_mut(layer) = disk;
                layers.push(layer);
            }
        }
        if layers.is_empty() {
            return Changes::default();
        }
        Changes {
            layers,
            keys: self.changed_since(&before),
        }
    }

    /// The settings whose value in effect differs from the one in `before`.
    fn changed_since(&self, before: &BTreeMap<ChangedKey, Value>) -> Vec<ChangedKey> {
        let after = self.in_effect();
        before
            .iter()
            .filter(|(key, value)| after.get(*key) != Some(*value))
            .chain(after.iter().filter(|(key, _)| !before.contains_key(*key)))
            .map(|(key, _)| key.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The values in effect, by device and key.
    fn in_effect(&self) -> BTreeMap<ChangedKey, Value> {
        self.entries()
            .into_iter()
            .map(|entry| {
                let key = ChangedKey {
                    device: entry.device.map(ToOwned::to_owned),
                    key: entry.setting.key(),
                };
                (key, entry.value.clone())
            })
            .collect()
    }

    fn disk(&self, layer: Layer) -> &Disk {
        match layer {
            Layer::Shared => &self.shared_disk,
            Layer::Host => &self.host_disk,
        }
    }

    fn disk_mut(&mut self, layer: Layer) -> &mut Disk {
        match layer {
            Layer::Shared => &mut self.shared_disk,
            Layer::Host => &mut self.host_disk,
        }
    }

    /// The paths of the two files.
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    fn file(&self, layer: Layer) -> &SettingsFile {
        match layer {
            Layer::Shared => &self.shared,
            Layer::Host => &self.host,
        }
    }

    fn file_mut(&mut self, layer: Layer) -> &mut SettingsFile {
        match layer {
            Layer::Shared => &mut self.shared,
            Layer::Host => &mut self.host,
        }
    }

    /// What couldn't be used in the two files, the shared file first.
    pub fn problems(&self) -> Vec<Problem> {
        Layer::ALL
            .into_iter()
            .flat_map(|layer| {
                self.file(layer).problems().iter().map(move |kind| Problem {
                    layer,
                    path: self.paths.path(layer).to_owned(),
                    kind: kind.clone(),
                })
            })
            .collect()
    }

    /// The stored value of `key`: the one in the host overlay, else the one in the shared file.
    /// With a `device`, only the value stored for that device; the value for its type is
    /// `get(key, None)`.
    pub fn get(&self, key: &str, device: Option<&str>) -> Option<&Value> {
        self.host
            .get(key, device)
            .or_else(|| self.shared.get(key, device))
    }

    /// The file the value of [`State::get`] comes from.
    pub fn layer(&self, key: &str, device: Option<&str>) -> Option<Layer> {
        if self.host.get(key, device).is_some() {
            Some(Layer::Host)
        } else if self.shared.get(key, device).is_some() {
            Some(Layer::Shared)
        } else {
            None
        }
    }

    /// Every stored value in effect, ordered by device (those without one first) and key.
    pub fn entries(&self) -> Vec<Entry<'_>> {
        let mut entries = Vec::new();
        self.collect(None, &mut entries);
        let mut devices: Vec<&str> = Layer::ALL
            .into_iter()
            .flat_map(|layer| self.file(layer).devices().keys().map(String::as_str))
            .collect();
        devices.sort_unstable();
        devices.dedup();
        for device in devices {
            self.collect(Some(device), &mut entries);
        }
        entries
    }

    /// Adds the values in effect for `device`, ordered by key.
    fn collect<'a>(&'a self, device: Option<&'a str>, entries: &mut Vec<Entry<'a>>) {
        let values = |layer| {
            let file = self.file(layer);
            match device {
                Some(device) => file.devices().get(device),
                None => Some(file.values()),
            }
            .into_iter()
            .flatten()
        };
        let mut merged: Vec<_> = values(Layer::Host)
            .map(|(key, value)| (*key, value, Layer::Host))
            .chain(
                values(Layer::Shared)
                    .filter(|(key, _)| self.host.get(key, device).is_none())
                    .map(|(key, value)| (*key, value, Layer::Shared)),
            )
            .collect();
        merged.sort_by_key(|(key, ..)| *key);
        entries.extend(merged.into_iter().filter_map(|(key, value, layer)| {
            Some(Entry {
                setting: self.registry.get(key)?,
                device,
                value,
                layer,
            })
        }));
    }

    /// The values to render settings from with [`Registry::render`]: for the whole session and
    /// the input types, or for one `device`. A device falls back to the value of its type, so the
    /// settings of one directive render complete.
    pub fn values<'a>(&'a self, device: Option<&'a str>) -> impl Values + 'a {
        View {
            state: self,
            device,
        }
    }

    fn setting(&self, key: &str, device: Option<&str>) -> Result<&'static Setting, StateError> {
        let setting = self
            .registry
            .get(key)
            .ok_or_else(|| StateError::UnknownKey {
                key: key.to_owned(),
            })?;
        if let Some(device) = device {
            if !matches!(setting.scope(), Scope::PerDevice(_)) {
                return Err(StateError::NotPerDevice { key: setting.key() });
            }
            if InputSelector::device(device).is_err() {
                return Err(StateError::InvalidDevice {
                    device: device.to_owned(),
                });
            }
        }
        Ok(setting)
    }

    /// Stores `value` for `key`, for the whole session or input type, or for one `device`.
    ///
    /// `layer` is the file to store it in. `None` keeps the value where it is, and stores a new
    /// one in the [default file](Layer::default_for) of the setting. A value stored in the shared
    /// file replaces one in the host overlay, which would hide it otherwise.
    ///
    /// Nothing is written to disk until [`State::save`].
    ///
    /// # Errors
    ///
    /// [`StateError::UnknownKey`], [`StateError::InvalidValue`], [`StateError::NotPerDevice`] or
    /// [`StateError::InvalidDevice`] if the arguments don't fit the registry;
    /// [`StateError::Unusable`] if the file couldn't be loaded (it is never changed, and nothing
    /// in it is in effect), and [`StateError::NotATable`] if something else is in the file where
    /// a table of the key must be.
    pub fn set(
        &mut self,
        key: &str,
        device: Option<&str>,
        value: Value,
        layer: Option<Layer>,
    ) -> Result<(), StateError> {
        let setting = self.setting(key, device)?;
        let key = setting.key();
        setting
            .check(&value)
            .map_err(|error| StateError::InvalidValue { key, error })?;
        let layer = layer
            .or_else(|| self.layer(key, device))
            .unwrap_or_else(|| Layer::default_for(setting, device));
        self.file_mut(layer)
            .set(key, device, value)
            .map_err(|error| match error {
                FileError::Unusable => self.unusable(layer),
                FileError::NotATable => StateError::NotATable {
                    key,
                    path: self.paths.path(layer).to_owned(),
                },
            })?;
        if layer == Layer::Shared {
            self.host.remove(key, device);
        }
        Ok(())
    }

    fn unusable(&self, layer: Layer) -> StateError {
        StateError::Unusable {
            path: self.paths.path(layer).to_owned(),
        }
    }

    /// Moves the stored value of `key` to the file `layer` ("this machine only" on or off).
    /// Returns whether there was a value to move.
    ///
    /// # Errors
    ///
    /// Like [`State::set`].
    pub fn move_to(
        &mut self,
        key: &str,
        device: Option<&str>,
        layer: Layer,
    ) -> Result<bool, StateError> {
        let setting = self.setting(key, device)?;
        let key = setting.key();
        let Some(value) = self.get(key, device).cloned() else {
            return Ok(false);
        };
        let other = match layer {
            Layer::Shared => Layer::Host,
            Layer::Host => Layer::Shared,
        };
        self.set(key, device, value, Some(layer))?;
        self.file_mut(other).remove(key, device);
        Ok(true)
    }

    /// Removes `key` from both files, so Sway Things no longer manages the setting. Returns
    /// whether it was stored. This also removes a value that was reported as invalid.
    ///
    /// # Errors
    ///
    /// [`StateError::UnknownKey`], [`StateError::NotPerDevice`] or [`StateError::InvalidDevice`]
    /// if the arguments don't fit the registry.
    pub fn reset(&mut self, key: &str, device: Option<&str>) -> Result<bool, StateError> {
        let key = self.setting(key, device)?.key();
        let shared = self.shared.remove(key, device);
        let host = self.host.remove(key, device);
        Ok(shared || host)
    }

    /// Whether there are changes that [`State::save`] would write.
    pub fn is_dirty(&self) -> bool {
        self.shared.is_dirty() || self.host.is_dirty()
    }

    /// Writes the changed files. Each file is replaced atomically, so a crash leaves the old or
    /// the new one. A file that is a symbolic link (into a dotfiles repository) stays one; its
    /// target is replaced.
    ///
    /// Before anything is replaced, both files as they are on disk go to the history as one
    /// [`Snapshot`], unless the newest snapshot already holds the same contents. The history
    /// keeps the newest 50 snapshots.
    ///
    /// # Errors
    ///
    /// [`StateError::Io`] if the snapshot or a file can't be written. Without the snapshot
    /// nothing is replaced; files written before a failing one stay written. The state stays
    /// dirty.
    pub fn save(&mut self) -> Result<(), StateError> {
        if !self.is_dirty() {
            return Ok(());
        }
        let on_disk = Layer::ALL.map(|layer| {
            let content = match Disk::read(self.paths.path(layer)) {
                Disk::Text(text) => Some(text),
                Disk::Missing | Disk::Unreadable(_) => None,
            };
            (self.paths.relative(layer), content)
        });
        history::take(
            self.paths.history(),
            &on_disk,
            SystemTime::now(),
            history::LIMIT,
        )
        .map_err(|source| self.history_error(source))?;
        for layer in Layer::ALL {
            if !self.file(layer).is_dirty() {
                continue;
            }
            let Some(text) = self.file(layer).text() else {
                continue;
            };
            let path = self.paths.path(layer);
            write::atomically(path, &text).map_err(|source| StateError::Io {
                path: path.to_owned(),
                source,
            })?;
            self.file_mut(layer).mark_saved();
            *self.disk_mut(layer) = Disk::Text(text);
        }
        Ok(())
    }

    fn history_error(&self, source: io::Error) -> StateError {
        StateError::Io {
            path: self.paths.history().to_owned(),
            source,
        }
    }

    /// The snapshots in the history, newest first.
    ///
    /// # Errors
    ///
    /// [`StateError::Io`] if the history can't be read.
    pub fn snapshots(&self) -> Result<Vec<Snapshot>, StateError> {
        history::list(self.paths.history()).map_err(|source| self.history_error(source))
    }

    /// Replaces both files in memory with their contents in the snapshot `id`, comments and
    /// unknown keys included, and returns what changed. A file that didn't exist when the
    /// snapshot was taken becomes empty.
    ///
    /// Like any other change, the restore is written by [`State::save`], which first takes a
    /// snapshot of the files it replaces. So a restore can be undone by restoring that one.
    ///
    /// # Errors
    ///
    /// [`StateError::UnknownSnapshot`] if there is no snapshot `id`,
    /// [`StateError::UnusableSnapshot`] if it holds a file that can't be loaded (it was taken
    /// of a file with a syntax error or another version), [`StateError::Io`] if it can't be
    /// read. The state is unchanged then.
    pub fn restore(&mut self, id: &str) -> Result<Changes, StateError> {
        let relative = Layer::ALL.map(|layer| self.paths.relative(layer));
        let paths = relative.each_ref().map(PathBuf::as_path);
        let contents = history::read(self.paths.history(), id, &paths).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                StateError::UnknownSnapshot { id: id.to_owned() }
            } else {
                self.history_error(source)
            }
        })?;
        let mut files = Vec::new();
        for (layer, content) in Layer::ALL.into_iter().zip(contents) {
            let mut file = SettingsFile::parse(content.as_deref().unwrap_or(""), self.registry);
            if !file.is_usable() {
                return Err(StateError::UnusableSnapshot { id: id.to_owned() });
            }
            let same_as_disk = match self.disk(layer) {
                Disk::Missing => content.is_none(),
                Disk::Text(text) => content.as_ref() == Some(text),
                Disk::Unreadable(_) => false,
            };
            if !same_as_disk {
                file.mark_dirty();
            }
            files.push((layer, file));
        }
        let before = self.in_effect();
        for (layer, file) in files {
            *self.file_mut(layer) = file;
        }
        Ok(Changes {
            layers: Layer::ALL.to_vec(),
            keys: self.changed_since(&before),
        })
    }
}

struct View<'a> {
    state: &'a State,
    device: Option<&'a str>,
}

impl Values for View<'_> {
    fn get(&self, key: &str) -> Option<&Value> {
        self.device
            .and_then(|device| self.state.get(key, Some(device)))
            .or_else(|| self.state.get(key, None))
    }
}

/// Why the state can't be changed or saved.
#[derive(Debug)]
pub enum StateError {
    /// No setting has this key.
    UnknownKey {
        /// The key.
        key: String,
    },
    /// The value doesn't fit the setting.
    InvalidValue {
        /// The key.
        key: &'static str,
        /// What is wrong with the value.
        error: ValueError,
    },
    /// The setting can't be set for a single device.
    NotPerDevice {
        /// The key.
        key: &'static str,
    },
    /// The string isn't a device identifier.
    InvalidDevice {
        /// The string.
        device: String,
    },
    /// The file couldn't be loaded (see [`State::problems`]), so it isn't changed.
    Unusable {
        /// The file.
        path: PathBuf,
    },
    /// The file has a value where a table of the key's path must be, e.g. `input = 1`.
    NotATable {
        /// The key.
        key: &'static str,
        /// The file.
        path: PathBuf,
    },
    /// The history has no snapshot with this id.
    UnknownSnapshot {
        /// The id.
        id: String,
    },
    /// The snapshot holds a file that can't be loaded.
    UnusableSnapshot {
        /// The id.
        id: String,
    },
    /// A settings file can't be written, or the history can't be read or written.
    Io {
        /// The file.
        path: PathBuf,
        /// The error of the operating system.
        source: io::Error,
    },
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let show = |path: &Path| path.display().to_string();
        match self {
            Self::UnknownKey { key } => write!(f, "unknown setting {key}"),
            Self::InvalidValue { key, error } => write!(f, "{key}: {error}"),
            Self::NotPerDevice { key } => write!(f, "{key} can't be set for a single device"),
            Self::InvalidDevice { device } => write!(f, "{device:?} is not a device identifier"),
            Self::Unusable { path } => write!(
                f,
                "{} couldn't be loaded, so it is left unchanged",
                show(path)
            ),
            Self::NotATable { key, path } => write!(
                f,
                "{}: a value is in the way of the table for {key}",
                show(path)
            ),
            Self::UnknownSnapshot { id } => write!(f, "there is no snapshot {id}"),
            Self::UnusableSnapshot { id } => {
                write!(f, "snapshot {id} holds a file that can't be loaded")
            }
            Self::Io { path, source } => write!(f, "{}: {source}", show(path)),
        }
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidValue { error, .. } => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
