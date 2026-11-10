// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The record of the files Sway Things generated.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item, Table};

use super::HEADER;
use crate::atomic;

const VERSION: i64 = 1;

/// How a file on disk relates to what Sway Things generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ownership {
    /// There is no such file, so it can be created.
    Absent,
    /// The file already holds the content that would be written.
    Current,
    /// The file is as Sway Things wrote it last, so it can be replaced.
    Ours,
    /// Sway Things generated the file, but it was changed since: its hash differs from the
    /// recorded one, or it isn't recorded here but starts with the header (it came from another
    /// machine with the dotfiles, or the manifest was lost). Ask before replacing it.
    Edited,
    /// The file isn't recorded and doesn't start with the header: someone else's. Never write
    /// it.
    Foreign,
}

/// `manifest.toml`: every generated file with the SHA-256 hash of its content, to notice files
/// that were changed outside Sway Things.
///
/// ```toml
/// version = 1
///
/// [files."/home/me/.config/sway/sway-things/10-input.conf"]
/// sha256 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    path: PathBuf,
    /// The hash of each file, by its absolute path.
    files: BTreeMap<String, String>,
    dirty: bool,
}

impl Manifest {
    /// Loads the manifest at `path`. One that is missing, can't be read or isn't a manifest of
    /// this version is empty: the generated files are then recognized by their header.
    pub fn load(path: &Path) -> Self {
        let files = fs::read_to_string(path)
            .ok()
            .and_then(|text| text.parse::<DocumentMut>().ok())
            .filter(|document| document.get("version").and_then(Item::as_integer) == Some(VERSION))
            .and_then(|document| {
                let files = document.get("files")?.as_table_like()?;
                Some(
                    files
                        .iter()
                        .filter_map(|(file, entry)| {
                            let hash = entry.as_table_like()?.get("sha256")?.as_str()?;
                            Some((file.to_owned(), hash.to_owned()))
                        })
                        .collect(),
                )
            })
            .unwrap_or_default();
        Self {
            path: path.to_owned(),
            files,
            dirty: false,
        }
    }

    /// The path of the manifest.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How the file at `file` relates to what Sway Things generated, given the `content` that
    /// would be written now.
    ///
    /// # Errors
    ///
    /// The I/O error if the file exists but can't be read.
    pub fn ownership(&self, file: &Path, content: &str) -> io::Result<Ownership> {
        let on_disk = match fs::read(file) {
            Ok(on_disk) => on_disk,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Ownership::Absent),
            Err(error) => return Err(error),
        };
        let recorded = file.to_str().and_then(|file| self.files.get(file));
        Ok(if on_disk == content.as_bytes() {
            Ownership::Current
        } else if recorded.is_some_and(|recorded| *recorded == sha256(&on_disk)) {
            Ownership::Ours
        } else if recorded.is_some() || on_disk.starts_with(HEADER.as_bytes()) {
            Ownership::Edited
        } else {
            Ownership::Foreign
        })
    }

    /// Whether `file` is recorded.
    pub fn contains(&self, file: &Path) -> bool {
        file.to_str()
            .is_some_and(|file| self.files.contains_key(file))
    }

    /// Records that `file` now holds `content`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` if the path isn't UTF-8, which a TOML key has to be.
    pub fn record(&mut self, file: &Path, content: &str) -> io::Result<()> {
        let key = file
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path is not UTF-8"))?;
        let hash = sha256(content.as_bytes());
        if self.files.get(key) != Some(&hash) {
            self.files.insert(key.to_owned(), hash);
            self.dirty = true;
        }
        Ok(())
    }

    /// Removes the record of `file`, which no longer exists.
    pub fn forget(&mut self, file: &Path) {
        if file
            .to_str()
            .is_some_and(|file| self.files.remove(file).is_some())
        {
            self.dirty = true;
        }
    }

    /// Writes the manifest if it changed since it was loaded, replacing the file atomically.
    ///
    /// # Errors
    ///
    /// The I/O error if it can't be written.
    pub fn save(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let mut document = DocumentMut::new();
        document.insert("version", toml_edit::value(VERSION));
        let mut files = Table::new();
        // Only the tables of the files get a header.
        files.set_implicit(true);
        for (file, hash) in &self.files {
            let mut entry = Table::new();
            entry.insert("sha256", toml_edit::value(hash.as_str()));
            files.insert(file, Item::Table(entry));
        }
        document.insert("files", Item::Table(files));
        atomic::write(&self.path, &document.to_string())?;
        self.dirty = false;
        Ok(())
    }
}

/// The SHA-256 hash of `content` as `sha256sum` prints it.
fn sha256(content: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(content) {
        // Writing to a `String` can't fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::{Manifest, Ownership, sha256};
    use crate::generate::HEADER;

    #[test]
    fn hashes_are_what_sha256sum_prints() {
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256(b"test"),
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
    }

    #[test]
    fn a_file_is_ours_until_someone_changes_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("10-input.conf");
        let mut manifest = Manifest::load(&dir.path().join("state/manifest.toml"));
        let old = format!("{HEADER}input type:touchpad tap enabled\n");
        let new = format!("{HEADER}input type:touchpad tap disabled\n");
        let ownership =
            |manifest: &Manifest, content: &str| manifest.ownership(&file, content).unwrap();

        assert_eq!(ownership(&manifest, &old), Ownership::Absent);
        fs::write(&file, &old).unwrap();
        manifest.record(&file, &old).unwrap();
        assert!(manifest.contains(&file));
        assert_eq!(ownership(&manifest, &old), Ownership::Current);
        assert_eq!(ownership(&manifest, &new), Ownership::Ours);

        // Changed outside, even if the header is gone.
        fs::write(&file, "input type:touchpad tap enabled\n").unwrap();
        assert_eq!(ownership(&manifest, &new), Ownership::Edited);
        // Unless it now holds what would be written anyway.
        fs::write(&file, &new).unwrap();
        assert_eq!(ownership(&manifest, &new), Ownership::Current);

        manifest.forget(&file);
        assert!(!manifest.contains(&file));
    }

    #[test]
    fn an_unrecorded_file_is_ours_only_with_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = Manifest::load(&dir.path().join("manifest.toml"));
        let new = format!("{HEADER}gaps inner 8\n");

        // From another machine's dotfiles: generated, but not recorded here.
        let synced = dir.path().join("30-appearance.conf");
        fs::write(&synced, format!("{HEADER}gaps inner 4\n")).unwrap();
        assert_eq!(
            manifest.ownership(&synced, &new).unwrap(),
            Ownership::Edited
        );
        fs::write(&synced, &new).unwrap();
        assert_eq!(
            manifest.ownership(&synced, &new).unwrap(),
            Ownership::Current
        );

        // Someone else's file, also if the header is only somewhere in it.
        let foreign = dir.path().join("40-behavior.conf");
        for content in ["focus_follows_mouse no\n".to_owned(), format!("\n{HEADER}")] {
            fs::write(&foreign, content).unwrap();
            assert_eq!(
                manifest.ownership(&foreign, &new).unwrap(),
                Ownership::Foreign
            );
        }
        // A directory in the way can't be read as a file.
        assert!(manifest.ownership(dir.path(), &new).is_err());
    }

    #[test]
    fn the_manifest_survives_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/manifest.toml");
        let mut manifest = Manifest::load(&path);
        // Nothing recorded, nothing written.
        manifest.save().unwrap();
        assert!(!path.exists());

        manifest
            .record(
                Path::new("/home/me/.config/sway/sway-things/10-input.conf"),
                "test",
            )
            .unwrap();
        manifest
            .record(
                Path::new("/home/me/.config/sway/sway-things/90-effects.conf"),
                "",
            )
            .unwrap();
        manifest.save().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            r#"version = 1

[files."/home/me/.config/sway/sway-things/10-input.conf"]
sha256 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"

[files."/home/me/.config/sway/sway-things/90-effects.conf"]
sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
"#
        );
        assert_eq!(Manifest::load(&path), manifest);
        assert_eq!(manifest.path(), path);

        // Recording the same content again changes nothing.
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        manifest
            .record(
                Path::new("/home/me/.config/sway/sway-things/90-effects.conf"),
                "",
            )
            .unwrap();
        manifest.save().unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }

    #[test]
    fn a_broken_manifest_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.toml");
        for text in [
            "[files",
            "version = 2\n[files.\"/a\"]\nsha256 = \"00\"\n",
            "[files.\"/a\"]\nsha256 = \"00\"\n",
            "version = 1\nfiles = 3\n",
        ] {
            fs::write(&path, text).unwrap();
            assert!(!Manifest::load(&path).contains(Path::new("/a")), "{text}");
        }
        fs::write(
            &path,
            "version = 1\n[files.\"/a\"]\nsha256 = \"00\"\n[files.\"/b\"]\n",
        )
        .unwrap();
        let manifest = Manifest::load(&path);
        assert!(manifest.contains(Path::new("/a")));
        assert!(!manifest.contains(Path::new("/b")));
    }
}
