// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files of other programs Sway Things writes, such as the default apps list, the autostart
//! overrides or the environment.d file: each starts with the header of the generated files and is
//! recorded in the manifest with its hash, like the sway files. A file is created where none is,
//! replaced while it is as Sway Things wrote it, and deleted once it is no longer needed, only
//! then. A file someone else wrote, or one of Sway Things changed since, is left alone and
//! reported.

use std::path::{Path, PathBuf};

use super::{Manifest, Ownership};
use crate::i18n::Text;
use crate::msg;

/// A file Sway Things wants, or wants gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// Where it is.
    pub path: PathBuf,
    /// Its text; `None` for a file of Sway Things that is no longer needed.
    pub text: Option<String>,
}

/// Why a file wasn't written or deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Someone else's file is there; Sway Things never writes it.
    Foreign(PathBuf),
    /// Sway Things wrote it, but it was changed since; it is left as it is.
    Edited(PathBuf),
    /// It couldn't be read, written or deleted.
    Io(PathBuf, String),
}

impl Problem {
    /// The file.
    pub fn path(&self) -> &Path {
        match self {
            Self::Foreign(path) | Self::Edited(path) | Self::Io(path, _) => path,
        }
    }

    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        let file = self.path().display().to_string();
        match self {
            Self::Foreign(_) => Text::new(msg!(
                "{file} is your own file: Sway Things doesn't change it, so its choice there \
                 doesn't take effect"
            ))
            .arg("file", file),
            Self::Edited(_) => Text::new(msg!(
                "{file} was changed outside Sway Things and is left as it is; delete it to have \
                 Sway Things write it again"
            ))
            .arg("file", file),
            Self::Io(_, error) => Text::new(msg!("{file} can't be written: {error}"))
                .arg("file", file)
                .arg("error", error.clone()),
        }
    }
}

/// Brings the files on disk to `wanted`, with `manifest` saying which are Sway Things' as it
/// wrote them, and records what it wrote there (the caller saves it). Returns what couldn't be
/// done.
pub fn keep(wanted: &[Wanted], manifest: &mut Manifest) -> Vec<Problem> {
    let mut problems = Vec::new();
    for file in wanted {
        let path = &file.path;
        let io = |error: std::io::Error| Problem::Io(path.clone(), error.to_string());
        // A symbolic link is followed where it leads to a file, like the sway files; one that
        // leads nowhere is someone else's and never replaced.
        let link = std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
        if link && std::fs::metadata(path).is_err() {
            if file.text.is_some() {
                problems.push(Problem::Foreign(path.clone()));
            }
            continue;
        }
        let ownership = match manifest.ownership(path, file.text.as_deref().unwrap_or_default()) {
            Ok(ownership) => ownership,
            Err(error) => {
                problems.push(io(error));
                continue;
            }
        };
        match (&file.text, ownership) {
            // Created only if still nothing is there: a file that appears meanwhile stays.
            (Some(text), Ownership::Absent) => match crate::atomic::create_new(path, text) {
                Ok(()) => {
                    if let Err(error) = manifest.record(path, text) {
                        problems.push(io(error));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    problems.push(Problem::Foreign(path.clone()));
                }
                Err(error) => problems.push(io(error)),
            },
            (Some(text), Ownership::Ours) => {
                if let Err(error) = crate::atomic::write(path, text) {
                    problems.push(io(error));
                } else if let Err(error) = manifest.record(path, text) {
                    problems.push(io(error));
                }
            }
            (Some(text), Ownership::Current) => {
                if let Err(error) = manifest.record(path, text) {
                    problems.push(io(error));
                }
            }
            (Some(_), Ownership::Edited) => problems.push(Problem::Edited(path.clone())),
            (Some(_), Ownership::Foreign) => problems.push(Problem::Foreign(path.clone())),
            (None, Ownership::Absent) => manifest.forget(path),
            // A link of the user's to a file of ours stays; only a file of ours goes.
            (None, Ownership::Ours) if link => {}
            (None, Ownership::Ours) => match std::fs::remove_file(path) {
                Ok(()) => manifest.forget(path),
                Err(error) => problems.push(io(error)),
            },
            // An empty file of ours would be `Current`; one that isn't recorded with its hash is
            // left alone. Only a changed file of ours is worth telling.
            (None, Ownership::Edited) if manifest.contains(path) => {
                problems.push(Problem::Edited(path.clone()));
            }
            (None, _) => {}
        }
    }
    problems
}
