// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What has to happen once a change is in effect, besides the generated files: a program of the
//! session follows the settings, or a service reads its files again. A participant returns them
//! as data ([`FollowUp`]); whoever applies the change runs them ([`FollowUp::run`]), off the
//! executor, and reports what they say ([`Done`]).

use crate::capture::clipboard;
use crate::night_light;

/// Something to do after a change.
#[derive(Debug, Clone, PartialEq)]
pub enum FollowUp {
    /// The clipboard history's watcher follows its switch and size
    /// ([`clipboard::follow`]).
    SyncClipboard {
        /// Whether the history is on.
        on: bool,
        /// How many entries it keeps.
        max: u32,
    },
    /// The night light's tool follows its settings ([`night_light::sync_with`]).
    SyncNightLight {
        /// The settings.
        settings: night_light::Settings,
        /// Whether the user's config starts a night light itself.
        external: bool,
    },
    /// The systemd user manager reads environment.d again.
    ReloadUserManager,
    /// The XDG desktop portals that run start again with their configuration (and
    /// xdg-desktop-portal-wlr with them).
    RestartPortals,
    /// xdg-desktop-portal-wlr starts again with its configuration.
    RestartScreencast,
}

/// What a [`FollowUp`] did.
#[derive(Debug)]
pub enum Done {
    /// The clipboard history's watcher.
    Clipboard(Result<clipboard::Synced, clipboard::SyncError>),
    /// The night light's tool; `external` as the follow-up said.
    NightLight {
        /// What the sync did.
        result: Result<night_light::Synced, night_light::SyncError>,
        /// Whether the user's config starts a night light itself.
        external: bool,
    },
    /// A service; the error `systemctl` reported.
    Service(Result<(), String>),
}

impl FollowUp {
    /// Does it: a program is followed on a thread of the blocking pool (it may wait for a lock),
    /// a service through `systemctl`.
    pub async fn run(self) -> Done {
        match self {
            Self::SyncClipboard { on, max } => Done::Clipboard(
                blocking::unblock(move || clipboard::follow(move || (on, max))).await,
            ),
            Self::SyncNightLight { settings, external } => Done::NightLight {
                result: blocking::unblock(move || night_light::sync_with(|| settings, external))
                    .await,
                external,
            },
            Self::ReloadUserManager => {
                Done::Service(crate::environment::reload_user_manager().await)
            }
            Self::RestartPortals => Done::Service(crate::environment::portals::restart().await),
            Self::RestartScreencast => {
                Done::Service(crate::environment::screencast::restart().await)
            }
        }
    }
}

/// `follow_ups` without the ones another one does already: restarting the portals restarts
/// xdg-desktop-portal-wlr too.
pub fn merged(follow_ups: Vec<FollowUp>) -> Vec<FollowUp> {
    let portals = follow_ups.contains(&FollowUp::RestartPortals);
    let mut kept: Vec<FollowUp> = Vec::with_capacity(follow_ups.len());
    for follow_up in follow_ups {
        let covered = portals && follow_up == FollowUp::RestartScreencast;
        if !covered && !kept.contains(&follow_up) {
            kept.push(follow_up);
        }
    }
    kept
}
