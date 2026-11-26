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
//! They are kept like every file of another program ([`crate::generate::other`]): only files of
//! Sway Things are written or deleted.

use super::Dirs;
use super::autostart::{self, Runner};
use super::installed::{self, App};
use super::mime::{self, Kind};
pub use crate::generate::other::{Problem, Wanted, keep};
use crate::state::State;

/// The files the settings in `state` want, for `dirs`.
///
/// # Blocking
///
/// Reads the user's autostart directory.
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
