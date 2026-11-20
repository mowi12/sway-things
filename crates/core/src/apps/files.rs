// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files of other programs that Sway Things writes for the Apps & Autostart page, each of
//! them its own:
//!
//! - `$XDG_CONFIG_HOME/sway-mimeapps.list`: the default apps;
//! - `$XDG_CONFIG_HOME/sway-xdg-terminals.list`: the terminal, for `xdg-terminal-exec`;
//! - `$XDG_CONFIG_HOME/autostart/<ID>.desktop` with `Hidden=true`: a system autostart entry
//!   switched off;
//! - `$XDG_CONFIG_HOME/systemd/user/sway-things-autostart.target`: what starts systemd's
//!   `xdg-desktop-autostart.target`.
//!
//! Each starts with the header of the generated files and is recorded in the manifest with its
//! hash, like the sway files. A file is created where none is, replaced while it is as Sway
//! Things wrote it, and deleted once it is no longer needed, only then. A file someone else
//! wrote, or one of Sway Things changed since, is left alone and reported.

use std::path::{Path, PathBuf};

use super::Dirs;
use super::autostart::{self, Runner};
use super::installed::{self, App};
use super::mime::{self, Kind};
use crate::generate::{Manifest, Ownership};
use crate::i18n::Text;
use crate::msg;
use crate::state::State;

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

/// The files the settings in `state` want, for `dirs`.
pub fn wanted(state: &State, dirs: &Dirs) -> Vec<Wanted> {
    // Only the apps chosen are looked up, not every installed one: this runs at each save.
    let apps: Vec<App> = Kind::ALL
        .iter()
        .filter_map(|kind| kind.stored(state))
        .filter_map(|id| installed::find(dirs, &id))
        .collect();
    let mut files = vec![
        Wanted {
            path: mime::ours(dirs),
            text: mime::ours_text(state, &apps),
        },
        Wanted {
            path: mime::terminals_list(dirs),
            text: mime::terminals_text(state),
        },
        Wanted {
            path: autostart::systemd_unit_path(dirs),
            text: (Runner::stored(state) == Some(Runner::Systemd))
                .then(autostart::systemd_unit_text),
        },
    ];
    let hidden = autostart::hidden(state);
    let user = autostart::user_dir(dirs);
    for id in &hidden {
        files.push(Wanted {
            path: user.join(id),
            text: Some(autostart::override_text(id)),
        });
    }
    // Overrides of Sway Things for entries no longer switched off go.
    if let Ok(read) = std::fs::read_dir(&user) {
        let mut names: Vec<String> = read
            .filter_map(Result::ok)
            .filter_map(|item| item.file_name().into_string().ok())
            .filter(|name| name.ends_with(".desktop") && !hidden.contains(name))
            .collect();
        names.sort();
        for name in names {
            let path = user.join(&name);
            let ours = std::fs::read(&path)
                .is_ok_and(|text| autostart::is_ours(&String::from_utf8_lossy(&text)));
            if ours {
                files.push(Wanted { path, text: None });
            }
        }
    }
    files
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
