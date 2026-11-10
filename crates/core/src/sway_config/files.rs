// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the parser reads: files, directories and environment variables.

use std::io;
use std::path::{Path, PathBuf};

/// The file system and environment the parser reads from. The parser only reads through this,
/// so it can be tested and fuzzed on files that exist only in memory.
pub trait Files {
    /// The content of the file at `path`.
    ///
    /// # Errors
    ///
    /// The I/O error if the file can't be read.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// The names of the entries of `directory`, in any order.
    ///
    /// # Errors
    ///
    /// The I/O error if it isn't a directory or can't be read.
    fn list(&self, directory: &Path) -> io::Result<Vec<String>>;

    /// Whether something exists at `path`.
    fn exists(&self, path: &Path) -> bool;

    /// The absolute path of `path` with every symbolic link resolved, like `realpath`.
    ///
    /// # Errors
    ///
    /// The I/O error if the path doesn't exist.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;

    /// The value of the environment variable `name`.
    fn env(&self, name: &str) -> Option<String>;
}

/// The real file system and the environment of this process.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Disk;

impl Files for Disk {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn list(&self, directory: &Path) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            // A name that isn't UTF-8 can't be matched or shown; sway configs don't have them.
            if let Ok(name) = entry?.file_name().into_string() {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

/// Files and environment variables in memory, for tests and fuzzing. Paths are absolute, there
/// are no symbolic links, and a directory exists if a file is in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryFiles {
    files: std::collections::BTreeMap<PathBuf, Vec<u8>>,
    env: std::collections::BTreeMap<String, String>,
}

impl MemoryFiles {
    /// No files and no variables.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the file `path` (absolute) with `content`.
    #[must_use]
    pub fn file(mut self, path: &str, content: impl Into<Vec<u8>>) -> Self {
        self.files
            .insert(normalize(Path::new(path)), content.into());
        self
    }

    /// Sets the environment variable `name`.
    #[must_use]
    pub fn var(mut self, name: &str, value: &str) -> Self {
        self.env.insert(name.to_owned(), value.to_owned());
        self
    }

    fn is_directory(&self, path: &Path) -> bool {
        self.files
            .keys()
            .any(|file| file != path && file.starts_with(path))
    }
}

/// Resolves `.` and `..` in an absolute path without looking at a file system.
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::Normal(name) => normalized.push(name),
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    normalized
}

impl Files for MemoryFiles {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.files
            .get(&normalize(path))
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn list(&self, directory: &Path) -> io::Result<Vec<String>> {
        let directory = normalize(directory);
        if !self.is_directory(&directory) {
            return Err(io::ErrorKind::NotFound.into());
        }
        let names: std::collections::BTreeSet<String> = self
            .files
            .keys()
            .filter_map(|file| file.strip_prefix(&directory).ok())
            .filter_map(|rest| rest.components().next())
            .filter_map(|name| name.as_os_str().to_str())
            .map(ToOwned::to_owned)
            .collect();
        Ok(names.into_iter().collect())
    }

    fn exists(&self, path: &Path) -> bool {
        let path = normalize(path);
        self.files.contains_key(&path) || self.is_directory(&path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        if path.is_absolute() && self.exists(path) {
            Ok(normalize(path))
        } else {
            Err(io::ErrorKind::NotFound.into())
        }
    }

    fn env(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }
}
