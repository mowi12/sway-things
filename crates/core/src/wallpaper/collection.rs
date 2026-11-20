// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wallpaper collections: named sets of pictures and folders whose pictures take turns on an
//! output. Their settings are stored per collection, by its name (`[collection."Nature"]`).

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::registry::{Registry, Scope, Value};
use crate::state::State;

/// The collection an output shows, by name; empty for none (per output).
pub const COLLECTION: &str = "wallpaper.collection";
/// The pictures and folders of a collection, with `~/` for the home folder.
pub const PICTURES: &str = "collection.pictures";
/// When the next picture is shown: `login`, `daily` or `interval`.
pub const CHANGE: &str = "collection.change";
/// The minutes between two pictures, for `interval`.
pub const INTERVAL: &str = "collection.interval";
/// Which picture comes next: `shuffle` or `in_order`.
pub const ORDER: &str = "collection.order";

/// The file name extensions of pictures, which swaybg reads through gdk-pixbuf.
pub const EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "avif", "jxl", "bmp", "tif", "tiff",
];

/// One day.
pub const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Whether `name` can name a collection: 1 to 64 characters, no control characters and no
/// spaces at either end.
pub fn is_collection_name(name: &str) -> bool {
    (1..=64).contains(&name.chars().count())
        && name.trim() == name
        && !name.chars().any(char::is_control)
}

/// Whether `path` has the extension of a picture.
pub fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
}

/// The pictures in `folder` (not in its subfolders), by name.
///
/// # Errors
///
/// The error of the operating system if the folder can't be read.
pub fn pictures_in(folder: &Path) -> io::Result<Vec<PathBuf>> {
    let mut pictures: Vec<PathBuf> = std::fs::read_dir(folder)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_picture(path) && path.is_file())
        .collect();
    pictures.sort();
    Ok(pictures)
}

/// The home folder, from `HOME`.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
}

/// `stored` with a leading `~/` replaced by `home`.
pub fn expand(stored: &str, home: Option<&Path>) -> PathBuf {
    match (stored.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(stored),
    }
}

/// `path` as it is stored: with `~/` for `home`.
pub fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// When the next picture of a collection is shown. A collection changes at login, too, when its
/// time has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// At every login.
    Login,
    /// Once a day: 24 hours after the last change.
    Daily,
    /// After this time.
    Every(Duration),
}

impl Change {
    /// The time between two pictures; `None` if only logins change the picture.
    pub fn period(self) -> Option<Duration> {
        match self {
            Self::Login => None,
            Self::Daily => Some(DAY),
            Self::Every(period) => Some(period),
        }
    }
}

/// Which picture comes next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Any other picture.
    Shuffle,
    /// The one after the current one, by the order of the pictures and folders.
    InOrder,
}

/// A collection with its settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    /// The name.
    pub name: String,
    /// The pictures and folders, as stored (`~/` for the home folder).
    pub entries: Vec<String>,
    /// When the picture changes.
    pub change: Change,
    /// Which picture comes next.
    pub order: Order,
}

impl Collection {
    /// The names of the collections in `state`, sorted.
    pub fn names(state: &State) -> Vec<String> {
        let mut names: Vec<String> = state
            .entries()
            .into_iter()
            .filter(|entry| entry.setting.scope() == Scope::PerCollection)
            .filter_map(|entry| entry.device.map(ToOwned::to_owned))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// The collection `name` of `state`; settings that aren't stored have their default.
    pub fn load(state: &State, name: &str) -> Self {
        let value = |key: &str| {
            state.get(key, Some(name)).cloned().or_else(|| {
                Registry::builtin()
                    .get(key)
                    .map(crate::registry::Setting::default_value)
            })
        };
        let entries = match value(PICTURES) {
            Some(Value::List(entries)) => entries,
            _ => Vec::new(),
        };
        let minutes = match value(INTERVAL) {
            Some(Value::Integer(minutes)) => u64::try_from(minutes).unwrap_or(1).max(1),
            _ => 30,
        };
        let change = match value(CHANGE) {
            Some(Value::Choice(change)) if change == "login" => Change::Login,
            Some(Value::Choice(change)) if change == "interval" => {
                Change::Every(Duration::from_secs(minutes * 60))
            }
            _ => Change::Daily,
        };
        let order = match value(ORDER) {
            Some(Value::Choice(order)) if order == "in_order" => Order::InOrder,
            _ => Order::Shuffle,
        };
        Self {
            name: name.to_owned(),
            entries,
            change,
            order,
        }
    }

    /// The pictures of the collection: each picture, and the pictures in each folder, in order
    /// and without repeats. Entries that are gone or can't be read are left out.
    pub fn pictures(&self, home: Option<&Path>) -> Vec<PathBuf> {
        let mut pictures = Vec::new();
        for entry in &self.entries {
            let path = expand(entry, home);
            if path.is_dir() {
                pictures.extend(pictures_in(&path).unwrap_or_default());
            } else if path.is_file() && is_picture(&path) {
                pictures.push(path);
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        pictures.retain(|picture| seen.insert(picture.clone()));
        pictures
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Collection, Order, expand, is_collection_name, pictures_in, tilde};

    #[test]
    fn collection_names_are_short_lines() {
        for name in ["Nature", "Holidays 2025", "Ä", &"x".repeat(64)] {
            assert!(is_collection_name(name), "{name}");
        }
        for name in ["", " Nature", "Nature ", "a\nb", "tab\t", &"x".repeat(65)] {
            assert!(!is_collection_name(name), "{name:?}");
        }
    }

    #[test]
    fn paths_keep_the_home_folder_as_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(
            expand("~/Pictures/a.png", Some(home)),
            home.join("Pictures/a.png")
        );
        assert_eq!(expand("/srv/a.png", Some(home)), Path::new("/srv/a.png"));
        assert_eq!(tilde(&home.join("Pictures"), Some(home)), "~/Pictures");
        assert_eq!(tilde(home, Some(home)), "/home/me");
        assert_eq!(tilde(Path::new("/srv/a.png"), Some(home)), "/srv/a.png");
    }

    #[test]
    fn pictures_and_folders_make_one_list_without_repeats() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Nature");
        std::fs::create_dir(&folder).unwrap();
        for name in ["b.JPG", "a.png", "notes.txt", "c.webp"] {
            std::fs::write(folder.join(name), b"").unwrap();
        }
        std::fs::create_dir(folder.join("d.png")).unwrap();
        let single = dir.path().join("single.png");
        std::fs::write(&single, b"").unwrap();
        let names: Vec<_> = pictures_in(&folder)
            .unwrap()
            .iter()
            .map(|path| path.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["a.png", "b.JPG", "c.webp"]);
        let collection = Collection {
            name: "Nature".to_owned(),
            entries: vec![
                single.display().to_string(),
                folder.display().to_string(),
                folder.join("a.png").display().to_string(),
                dir.path().join("gone.png").display().to_string(),
            ],
            change: super::Change::Login,
            order: Order::InOrder,
        };
        let pictures = collection.pictures(None);
        assert_eq!(
            pictures,
            [
                single,
                folder.join("a.png"),
                folder.join("b.JPG"),
                folder.join("c.webp")
            ]
        );
        assert!(pictures_in(Path::new("/nonexistent/folder")).is_err());
    }
}
