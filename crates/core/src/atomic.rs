// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Atomic file writes.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

/// Replaces the contents of `path` with `text`: writes a temporary file next to it and renames
/// that over it, so a crash leaves the old or the new contents, never a mix. Missing parent
/// directories are created.
///
/// If `path` is a symbolic link, the file it points to is replaced and the link stays, so a
/// file that lives in a dotfiles repository keeps living there.
pub(crate) fn write(path: &Path, text: &str) -> io::Result<()> {
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

    let mut temporary_name = std::ffi::OsString::from(".");
    temporary_name.push(name);
    temporary_name.push(format!(".{}.tmp", std::process::id()));
    let temporary = directory.join(temporary_name);

    let written = (|| {
        // `create_new` never follows a link or reuses a file someone else put there.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, &target)
    })();
    if written.is_err() {
        // Best effort: the error to report is the one from writing.
        let _ = fs::remove_file(&temporary);
        return written;
    }
    // Make the rename durable. Not every file system can sync a directory, and the data is
    // already in place.
    if let Ok(directory) = File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}
