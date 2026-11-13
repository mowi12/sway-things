// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The user's files with every generated file in place, whether it was written yet or not.

use std::io;
use std::path::{Component, Path, PathBuf};

use crate::registry::ConfigFile;
use crate::sway_config::Files;

/// What each generated file holds in the overlay: one directive, which marks the place where
/// sway reads the file.
const MARKER: &[u8] = b"sway-things\n";

/// A view of `inner` in which every file Sway Things can generate exists and holds one marker
/// directive, so that parsing the user's config shows whether and where sway would read each of
/// them. A generated file that wasn't written yet, or holds only its header, is found this way
/// too.
pub(super) struct Overlay<'a> {
    inner: &'a dyn Files,
    /// The directory of the generated files, with symbolic links resolved as far as it exists.
    directory: PathBuf,
    /// A file whose content is replaced, by its resolved path.
    replaced: Option<(PathBuf, Vec<u8>)>,
}

impl<'a> Overlay<'a> {
    pub(super) fn new(inner: &'a dyn Files, directory: &Path) -> Self {
        Self {
            inner,
            directory: resolve(inner, directory),
            replaced: None,
        }
    }

    /// Reads `content` at `path` instead of what is there, also if nothing is.
    pub(super) fn replacing(mut self, path: &Path, content: Vec<u8>) -> Self {
        self.replaced = Some((resolve(self.inner, path), content));
        self
    }

    /// The path the parser reports for `file`.
    pub(super) fn path(&self, file: ConfigFile) -> PathBuf {
        self.directory.join(file.file_name())
    }

    /// The generated file at `path`, if it is one.
    fn generated(&self, path: &Path) -> Option<ConfigFile> {
        let name = path.file_name()?.to_str()?;
        let file = ConfigFile::ALL
            .into_iter()
            .find(|file| file.file_name() == name)?;
        (resolve(self.inner, path.parent()?) == self.directory).then_some(file)
    }

    fn replacement(&self, path: &Path) -> Option<&(PathBuf, Vec<u8>)> {
        self.replaced
            .as_ref()
            .filter(|(replaced, _)| *replaced == resolve(self.inner, path))
    }
}

/// `path` with symbolic links resolved as far as it exists; the rest is appended as written.
pub(super) fn resolve(files: &dyn Files, path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        let Ok(mut real) = files.canonicalize(ancestor) else {
            continue;
        };
        // What doesn't exist can't be a link, so `..` just drops the name before it.
        for component in path
            .strip_prefix(ancestor)
            .iter()
            .flat_map(|rest| rest.components())
        {
            match component {
                Component::Normal(name) => real.push(name),
                Component::ParentDir => {
                    real.pop();
                }
                Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            }
        }
        return real;
    }
    path.to_owned()
}

impl Files for Overlay<'_> {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        if let Some((_, content)) = self.replacement(path) {
            Ok(content.clone())
        } else if self.generated(path).is_some() {
            Ok(MARKER.to_vec())
        } else {
            self.inner.read(path)
        }
    }

    fn read_bounded(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        if self.replacement(path).is_some() || self.generated(path).is_some() {
            self.read(path)
        } else {
            self.inner.read_bounded(path, limit)
        }
    }

    fn list(&self, directory: &Path) -> io::Result<Vec<String>> {
        let listed = self.inner.list(directory);
        let real = resolve(self.inner, directory);
        let added: Vec<String> = if real == self.directory {
            ConfigFile::ALL
                .iter()
                .map(|file| file.file_name().to_owned())
                .collect()
        } else {
            // A directory above the generated files holds the next one on the way to them.
            self.directory
                .strip_prefix(&real)
                .ok()
                .and_then(|rest| rest.components().next())
                .and_then(|name| name.as_os_str().to_str())
                .map(ToOwned::to_owned)
                .into_iter()
                .collect()
        };
        if added.is_empty() {
            return listed;
        }
        let mut names = listed.unwrap_or_default();
        for name in added {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
            || self.generated(path).is_some()
            || self.replacement(path).is_some()
            || (path.is_absolute() && self.directory.starts_with(resolve(self.inner, path)))
    }

    fn is_file(&self, path: &Path) -> io::Result<bool> {
        if self.replacement(path).is_some() || self.generated(path).is_some() {
            Ok(true)
        } else {
            self.inner.is_file(path)
        }
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        // Not through a generated file that is a symbolic link: it is ours wherever it leads.
        if let Some(file) = self.generated(path) {
            Ok(self.path(file))
        } else if let Some((replaced, _)) = self.replacement(path) {
            Ok(replaced.clone())
        } else {
            self.inner.canonicalize(path)
        }
    }

    fn env(&self, name: &str) -> Option<String> {
        self.inner.env(name)
    }
}
