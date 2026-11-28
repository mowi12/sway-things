// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The seam between the features and the parts that apply, generate and check every setting:
//! each feature that adds something beyond the lines of the registry is a [`Participant`], and
//! [`PARTICIPANTS`] lists them. `generate`, `apply` and `doctor` reach a feature through this list
//! only, so a new page adds a participant here instead of an arm in each of them.
//!
//! A participant
//! - adds [lines](Participant::lines) to the generated files: pure, from the stored settings;
//! - names the [files of other programs](Participant::other_files) it keeps (the environment.d
//!   file, the portals' configuration, …), and what happens [when one
//!   changed](Participant::when_written), such as a service reading it again;
//! - says what has to [follow a change](Participant::follow_ups) of its settings in the running
//!   session, such as a program started or ended. These are data ([`FollowUp`]), which the front
//!   end runs off its main loop ([`FollowUp::run`]).
//!
//! The order of [`PARTICIPANTS`] is the order of their lines in each file.

mod files;
mod follow_up;
mod line;
mod lines;
mod session;
#[cfg(test)]
mod tests;

use crate::apps::Dirs;
use crate::manifest::Wanted;
use crate::registry::{ConfigFile, Registry, Setting};
use crate::state::State;
use crate::sway_config::SwayConfig;

pub use follow_up::{Done, FollowUp, merged};
pub use line::{Line, Source};

/// What a participant makes its lines from.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The registry of the settings.
    pub registry: &'a Registry,
    /// The stored settings.
    pub state: &'a State,
}

/// A change that is in effect now: what follows it is worked out from this.
#[derive(Debug, Clone, Copy)]
pub struct Change<'a> {
    /// The keys of the settings that changed.
    pub keys: &'a [&'a str],
    /// The settings now.
    pub state: &'a State,
    /// The user's sway config, if it was read: a program it starts itself isn't started again.
    pub config: Option<&'a SwayConfig>,
}

impl Change<'_> {
    /// Whether one of `keys` changed.
    pub fn touches(&self, keys: &[&str]) -> bool {
        self.keys.iter().any(|key| keys.contains(key))
    }
}

/// A feature that takes part in generating, keeping files and following changes. Every method
/// has a default that does nothing.
pub trait Participant: Sync {
    /// The lines it adds to the generated files, each with its file, in the order they go there.
    fn lines(&self, _ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        Vec::new()
    }

    /// The files of other programs it keeps for the settings in `state`, in the directories
    /// `dirs`. It may read a directory of the user's to see what is there, so the applier calls
    /// it off the main loop.
    fn other_files(&self, _state: &State, _dirs: &Dirs) -> Vec<Wanted> {
        Vec::new()
    }

    /// What has to happen when one of its [`other_files`](Participant::other_files) changed.
    fn when_written(&self) -> Option<FollowUp> {
        None
    }

    /// What has to follow `change` in the running session.
    fn follow_ups(&self, _change: &Change<'_>) -> Vec<FollowUp> {
        Vec::new()
    }

    /// Whether a change of `key` is applied as a change of the bindings: as the difference of
    /// the whole table to what the user's config binds ([`crate::bindings::live`]).
    fn binds(&self, _key: &str) -> bool {
        false
    }

    /// Whether its lines name something that has to exist first ([`Participant::prepare`]) after
    /// a live change of `setting`.
    fn prepares_for(&self, _setting: &Setting) -> bool {
        false
    }

    /// Makes what its lines name exist, before they are written or sent: the links to the
    /// wallpaper collections' pictures. Best effort; it may read the user's folders, so the
    /// applier runs it off the executor.
    fn prepare(&self, _state: &State) {}
}

/// Every participant, in the order of their lines in each generated file.
pub static PARTICIPANTS: &[&dyn Participant] = &[
    &lines::Bindings,
    &lines::Autostart,
    &lines::Bar,
    &session::Gsettings,
    &session::Clipboard,
    &session::NightLight,
    &session::Workspaces,
    &session::Timezone,
    &session::NotificationDaemon,
    &lines::Wallpaper,
    &files::AppsFiles,
    &files::Environment,
    &files::Portals,
    &files::Screencast,
];

/// The lines of every participant, in order.
pub fn lines(ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
    PARTICIPANTS
        .iter()
        .flat_map(|participant| participant.lines(ctx))
        .collect()
}

/// Whether a change of `key` is applied as a change of the bindings ([`Participant::binds`]).
pub fn binds(key: &str) -> bool {
    PARTICIPANTS
        .iter()
        .any(|participant| participant.binds(key))
}

/// Whether a participant has to [prepare](Participant::prepare) after a live change of
/// `setting`.
pub fn prepares_for(setting: &Setting) -> bool {
    PARTICIPANTS
        .iter()
        .any(|participant| participant.prepares_for(setting))
}

/// [`Participant::prepare`] of every participant. It reads the folders of the wallpaper
/// collections (through a method the architecture test can't follow), so call it off the main
/// loop and the executor.
pub fn prepare(state: &State) {
    for participant in PARTICIPANTS {
        participant.prepare(state);
    }
}

/// What has to follow `change`, of every participant, each once.
pub fn follow_ups(change: &Change<'_>) -> Vec<FollowUp> {
    merged(
        PARTICIPANTS
            .iter()
            .flat_map(|participant| participant.follow_ups(change))
            .collect(),
    )
}
