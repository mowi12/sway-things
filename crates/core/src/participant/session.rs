// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The participants that run something in the session: the hooks of `80-hooks.conf`, and the
//! programs that follow their settings at once.

use super::{Change, Context, FollowUp, Line, Participant, Source};
use crate::command::{self, Hook};
use crate::registry::{ConfigFile, Setting, Value};

/// The line that runs `sway-things hook <hook>` in `80-hooks.conf`, for the settings `keys`.
fn hook_line(hook: Hook, keys: Vec<&'static str>) -> (ConfigFile, Line) {
    (
        ConfigFile::Hooks,
        Line::rendered(&command::hook(hook), Source::of(keys)),
    )
}

/// `sway-things hook gsettings` while a setting with a GSettings key is stored, so its value
/// reaches GSettings at login and after each reload, also when the settings file came from
/// another machine.
pub(super) struct Gsettings;

impl Participant for Gsettings {
    fn lines(&self, ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        let keys: Vec<&'static str> = ctx
            .registry
            .iter()
            .filter(|setting| setting.gsettings().is_some())
            .map(Setting::key)
            .filter(|key| ctx.state.get(key, None).is_some())
            .collect();
        if keys.is_empty() {
            return Vec::new();
        }
        vec![hook_line(Hook::Gsettings, keys)]
    }
}

/// The clipboard history: `sway-things hook clipboard` while it is on, which starts its watcher
/// once per session; after a change of its switch or size, the watcher follows.
pub(super) struct Clipboard;

impl Participant for Clipboard {
    fn lines(&self, ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        if !crate::capture::is_on(ctx.state, crate::capture::HISTORY) {
            return Vec::new();
        }
        vec![hook_line(Hook::Clipboard, vec![crate::capture::HISTORY])]
    }

    fn follow_ups(&self, change: &Change<'_>) -> Vec<FollowUp> {
        if !change.touches(&crate::capture::clipboard::KEYS) {
            return Vec::new();
        }
        vec![FollowUp::SyncClipboard {
            on: crate::capture::is_on(change.state, crate::capture::HISTORY),
            max: crate::capture::Settings::of(change.state).max_entries,
        }]
    }
}

/// The night light: `sway-things hook night-light` while it is on, which starts its tool; after
/// a change of its settings, the tool follows, unless the user's config starts a night light.
pub(super) struct NightLight;

impl Participant for NightLight {
    fn lines(&self, ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        if !crate::night_light::Settings::of(ctx.state).on {
            return Vec::new();
        }
        vec![hook_line(
            Hook::NightLight,
            vec![crate::night_light::ENABLED],
        )]
    }

    fn follow_ups(&self, change: &Change<'_>) -> Vec<FollowUp> {
        if !change.touches(&crate::night_light::KEYS) {
            return Vec::new();
        }
        let external = change
            .config
            .is_some_and(|config| !crate::night_light::own_lines(config).is_empty());
        vec![FollowUp::SyncNightLight {
            settings: crate::night_light::Settings::of(change.state),
            external,
        }]
    }
}

/// `sway-things hook workspaces` while the workspaces are placed, since sway forgets their
/// displays at a reload.
pub(super) struct Workspaces;

impl Participant for Workspaces {
    fn lines(&self, ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        if !crate::outputs::workspaces::Rule::of(ctx.state).places() {
            return Vec::new();
        }
        vec![hook_line(
            Hook::Workspaces,
            vec![crate::registry::workspaces::PLACEMENT],
        )]
    }
}

/// `sway-things hook timezone` once at login while the automatic time zone is on.
pub(super) struct Timezone;

impl Participant for Timezone {
    fn lines(&self, ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        let key = crate::registry::region::AUTOMATIC_TIMEZONE;
        if ctx.state.get(key, None) != Some(&Value::Bool(true)) {
            return Vec::new();
        }
        vec![(
            ConfigFile::Hooks,
            Line::rendered(
                &command::hook_at_start(Hook::Timezone),
                Source::of(vec![key]),
            ),
        )]
    }
}

/// The line that starts the notification daemon chosen on the Notifications page
/// (`notifications.start`) through `sway-things hook notification-daemon`.
pub(super) struct NotificationDaemon;

impl Participant for NotificationDaemon {
    fn lines(&self, ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        use crate::notifications::Daemon;
        use crate::registry::notifications::START;
        let Some(Value::Choice(program)) = ctx.state.get(START, None) else {
            return Vec::new();
        };
        if Daemon::from_program(program).is_none() {
            return Vec::new();
        }
        vec![hook_line(Hook::NotificationDaemon, vec![START])]
    }
}
