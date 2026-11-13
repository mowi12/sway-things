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

    /// The content of the file at `path`, read no further than `limit + 1` bytes: a file longer
    /// than `limit` comes back longer than `limit` without being read to its end. The default
    /// reads the whole file.
    ///
    /// # Errors
    ///
    /// The I/O error if the file can't be read. On the real file system also if it isn't a
    /// regular file: a device, FIFO or socket is never read, and opening it doesn't block.
    fn read_bounded(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        let _ = limit;
        self.read(path)
    }

    /// The names of the entries of `directory`, in any order.
    ///
    /// # Errors
    ///
    /// The I/O error if it isn't a directory or can't be read.
    fn list(&self, directory: &Path) -> io::Result<Vec<String>>;

    /// Whether something exists at `path`.
    fn exists(&self, path: &Path) -> bool;

    /// Whether `path`, with symbolic links followed, is a regular file: not a directory, device,
    /// FIFO or socket. It isn't opened.
    ///
    /// # Errors
    ///
    /// The I/O error if nothing is there or it can't be looked at.
    fn is_file(&self, path: &Path) -> io::Result<bool>;

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

    fn read_bounded(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        use rustix::fs::{Mode, OFlags};
        use std::io::Read;
        // The path may lead somewhere else by now than when it was looked at. Opened without
        // blocking, a FIFO doesn't wait for a writer, and what was opened is checked before
        // anything is read. O_NONBLOCK doesn't change how a regular file reads.
        let flags = OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC;
        let file = std::fs::File::from(rustix::fs::open(path, flags, Mode::empty())?);
        if !file.metadata()?.is_file() {
            return Err(not_a_file());
        }
        let mut content = Vec::new();
        file.take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
            .read_to_end(&mut content)?;
        Ok(content)
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

    fn is_file(&self, path: &Path) -> io::Result<bool> {
        Ok(std::fs::metadata(path)?.is_file())
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

/// The error for reading something that isn't a regular file.
fn not_a_file() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a regular file")
}

/// Files and environment variables in memory, for tests and fuzzing. Paths are absolute, there
/// are no symbolic links, and a directory exists if a file is in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryFiles {
    /// The content of each file; none for one that isn't a regular file.
    files: std::collections::BTreeMap<PathBuf, Option<Vec<u8>>>,
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
            .insert(normalize(Path::new(path)), Some(content.into()));
        self
    }

    /// Adds something at `path` (absolute) that isn't a regular file, like a device, a FIFO or a
    /// socket. Reading it fails; on the real file system, it might never end.
    #[must_use]
    pub fn special(mut self, path: &str) -> Self {
        self.files.insert(normalize(Path::new(path)), None);
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
        match self.files.get(&normalize(path)) {
            Some(Some(content)) => Ok(content.clone()),
            Some(None) => Err(not_a_file()),
            None => Err(io::ErrorKind::NotFound.into()),
        }
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

    fn is_file(&self, path: &Path) -> io::Result<bool> {
        let path = normalize(path);
        match self.files.get(&path) {
            Some(content) => Ok(content.is_some()),
            None if self.is_directory(&path) => Ok(false),
            None => Err(io::ErrorKind::NotFound.into()),
        }
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
