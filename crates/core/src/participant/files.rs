// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The participants that keep files of other programs ([`crate::manifest::keep`]): the default
//! apps and autostart overrides, the environment.d file, the portals' configuration and that of
//! xdg-desktop-portal-wlr.

use super::{FollowUp, Participant};
use crate::apps::Dirs;
use crate::manifest::Wanted;
use crate::state::State;

/// The files of Apps & Autostart: the default apps list, the switched-off autostart entries and
/// the systemd unit of the runner ([`crate::apps::files::wanted`]).
pub(super) struct AppsFiles;

impl Participant for AppsFiles {
    fn other_files(&self, state: &State, dirs: &Dirs) -> Vec<Wanted> {
        crate::apps::files::wanted(state, dirs)
    }
}

/// Sway Things' environment.d file; the user manager reads it again once it changed, for the
/// services it starts from now on (it also doesn't keep the old variables when it outlives a
/// quick logout). Without systemd the file waits for the next login all the same.
pub(super) struct Environment;

impl Participant for Environment {
    fn other_files(&self, state: &State, dirs: &Dirs) -> Vec<Wanted> {
        vec![crate::environment::wanted(state, dirs)]
    }

    fn when_written(&self) -> Option<FollowUp> {
        Some(FollowUp::ReloadUserManager)
    }
}

/// The portals' configuration; they read it only when they start, so the ones that run start
/// again once it changed.
pub(super) struct Portals;

impl Participant for Portals {
    fn other_files(&self, state: &State, dirs: &Dirs) -> Vec<Wanted> {
        vec![crate::environment::portals::wanted(state, dirs)]
    }

    fn when_written(&self) -> Option<FollowUp> {
        Some(FollowUp::RestartPortals)
    }
}

/// The configuration of xdg-desktop-portal-wlr's screen sharing; it starts again once it
/// changed.
pub(super) struct Screencast;

impl Participant for Screencast {
    fn other_files(&self, state: &State, dirs: &Dirs) -> Vec<Wanted> {
        vec![crate::environment::screencast::wanted(state, dirs)]
    }

    fn when_written(&self) -> Option<FollowUp> {
        Some(FollowUp::RestartScreencast)
    }
}
