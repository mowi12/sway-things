// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Atomic file writes.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Replaces the contents of `path` with `text`: writes a temporary file next to it and renames
/// that over it, so a crash leaves the old or the new contents, never a mix. Missing parent
/// directories are created.
///
/// If `path` is a symbolic link, the file it points to is replaced and the link stays, so a
/// file that lives in a dotfiles repository keeps living there.
pub(crate) fn write(path: &Path, text: &str) -> io::Result<()> {
    stage(path, text, "tmp")?.commit()
}

/// Creates the file `path` with `text`, only if nothing is there: no file, and no symbolic
/// link, not even a dangling one. The text is staged next to it ([`stage`]) and then linked into
/// place ([`Staged::commit_new`]), which fails if `path` appeared meanwhile, so nothing is ever
/// replaced. Missing parent directories are created.
///
/// # Errors
///
/// `AlreadyExists` if something is at `path`, and any I/O error.
pub(crate) fn create_new(path: &Path, text: &str) -> io::Result<()> {
    if fs::symlink_metadata(path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "something is there already",
        ));
    }
    stage(path, text, "new")?.commit_new()
}

/// `text` written to a hidden temporary file next to the file it is to replace, and synced, so
/// it can be checked before it replaces that file ([`Staged::commit`]). Dropped without a commit,
/// it is removed.
#[derive(Debug)]
pub(crate) struct Staged {
    temporary: PathBuf,
    target: PathBuf,
    committed: bool,
}

/// Stages `text` for `path` ([`Staged`]), as `.<name>.<pid>.<suffix>` in the directory of the
/// file `path` is or points to; missing directories are created. A file of that name that a
/// crashed process with the same id left behind is removed first, so it never blocks a write.
pub(crate) fn stage(path: &Path, text: &str, suffix: &str) -> io::Result<Staged> {
    // Only an existing path can be a link; `canonicalize` follows it to the real file.
    let target = match fs::canonicalize(path) {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_owned(),
        Err(error) => return Err(error),
    };
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "not a file path");
    let directory = target.parent().ok_or_else(invalid)?;
    let name = target.file_name().ok_or_else(invalid)?;
    fs::create_dir_all(directory)?;

    let mut temporary_name = OsString::from(".");
    temporary_name.push(name);
    temporary_name.push(format!(".{}.{suffix}", std::process::id()));
    let temporary = directory.join(temporary_name);
    // No live process has this id, so a file of this name is a leftover. Removing a link
    // removes the link, not what it points to.
    match fs::remove_file(&temporary) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }

    let staged = Staged {
        temporary,
        target,
        committed: false,
    };
    // `create_new` never follows a link or reuses a file someone else put there meanwhile.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged.temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    Ok(staged)
}

impl Staged {
    /// The temporary file, to check before the commit.
    pub(crate) fn path(&self) -> &Path {
        &self.temporary
    }

    /// Renames the temporary file over the file it replaces, so exactly the bytes that were
    /// checked are in place.
    pub(crate) fn commit(mut self) -> io::Result<()> {
        fs::rename(&self.temporary, &self.target)?;
        self.committed = true;
        // Make the rename durable. Not every file system can sync a directory, and the data is
        // already in place.
        if let Some(directory) = self.target.parent() {
            if let Ok(directory) = File::open(directory) {
                let _ = directory.sync_all();
            }
        }
        Ok(())
    }

    /// Links the temporary file into place only if nothing is at the file it is for: `link`
    /// fails with `AlreadyExists` where `rename` would replace. The temporary name goes either
    /// way.
    pub(crate) fn commit_new(self) -> io::Result<()> {
        fs::hard_link(&self.temporary, &self.target)?;
        if let Some(directory) = self.target.parent() {
            if let Ok(directory) = File::open(directory) {
                let _ = directory.sync_all();
            }
        }
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.committed {
            // Best effort: what counts is the error or the result that led here.
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::{create_new, stage, write};

    /// The names in `dir`, sorted.
    fn names(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_new_file_is_written_with_its_directories_and_no_temporary_file_stays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/file.conf");
        write(&path, "one\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\n");
        write(&path, "two\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two\n");
        assert_eq!(names(&dir.path().join("a/b")), ["file.conf"]);
    }

    #[test]
    fn a_symbolic_link_stays_and_its_target_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("dotfiles")).unwrap();
        let real = dir.path().join("dotfiles/settings.toml");
        fs::write(&real, "old\n").unwrap();
        let link = dir.path().join("settings.toml");
        symlink(&real, &link).unwrap();

        write(&link, "new\n").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "new\n");
        assert_eq!(names(&dir.path().join("dotfiles")), ["settings.toml"]);
    }

    #[test]
    fn a_leftover_of_a_crashed_process_with_the_same_id_doesnt_block_the_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.conf");
        let leftover = dir
            .path()
            .join(format!(".file.conf.{}.tmp", std::process::id()));
        fs::write(&leftover, "half written").unwrap();

        write(&path, "whole\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "whole\n");
        assert_eq!(names(dir.path()), ["file.conf"]);
    }

    #[test]
    fn a_leftover_link_is_removed_and_what_it_points_to_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        fs::write(&elsewhere, "keep\n").unwrap();
        let leftover = dir
            .path()
            .join(format!(".file.conf.{}.tmp", std::process::id()));
        symlink(&elsewhere, &leftover).unwrap();

        write(&dir.path().join("file.conf"), "new\n").unwrap();
        assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "keep\n");
        assert_eq!(names(dir.path()), ["elsewhere", "file.conf"]);
    }

    #[test]
    fn a_staged_file_replaces_the_old_one_only_when_committed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.conf");
        fs::write(&path, "old\n").unwrap();

        // Checked and dropped: the old file stays and the staged one is gone.
        let staged = stage(&path, "rejected\n", "staged").unwrap();
        assert_eq!(fs::read_to_string(staged.path()).unwrap(), "rejected\n");
        drop(staged);
        assert_eq!(fs::read_to_string(&path).unwrap(), "old\n");
        assert_eq!(names(dir.path()), ["file.conf"]);

        // Committed: exactly the staged bytes are in place.
        let staged = stage(&path, "accepted\n", "staged").unwrap();
        staged.commit().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "accepted\n");
        assert_eq!(names(dir.path()), ["file.conf"]);
    }

    #[test]
    fn a_failed_rename_leaves_the_target_and_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        // A directory with something in it can't be replaced by a file.
        let path = dir.path().join("taken");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("inside"), "").unwrap();

        assert!(write(&path, "text").is_err());
        assert!(path.is_dir());
        assert_eq!(names(dir.path()), ["taken"]);
    }

    #[test]
    fn create_new_never_replaces_a_file_or_a_dangling_link() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/new.desktop");
        create_new(&path, "one\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\n");
        assert_eq!(names(&dir.path().join("a")), ["new.desktop"]);

        let error = create_new(&path, "two\n").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\n");

        let dangling = dir.path().join("dangling.desktop");
        symlink(dir.path().join("nowhere"), &dangling).unwrap();
        let error = create_new(&dangling, "three\n").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(!dir.path().join("nowhere").exists());
        assert_eq!(names(dir.path()), ["a", "dangling.desktop"]);
    }

    #[test]
    fn a_path_without_a_file_name_is_refused() {
        let error = write(std::path::Path::new("/"), "text").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
