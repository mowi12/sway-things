// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The participants that write whole files or parts of them beyond the registry's lines: the
//! bindings, the autostart entries, the bar, and the wallpaper's start line.

use super::{Context, Line, Participant, Source};
use crate::command::Target;
use crate::registry::{ConfigFile, Page, Setting};
use crate::state::State;
use crate::sway_config::bindings::Origin;

/// The bindings of the Keyboard Shortcuts page in `50-bindings.conf`
/// ([`crate::bindings::ours::file_lines`]). An item that can't be written becomes a comment.
pub(super) struct Bindings;

impl Participant for Bindings {
    fn binds(&self, key: &str) -> bool {
        crate::registry::shortcuts::KEYS.contains(&key)
            || crate::capture::shortcuts::KEYS.contains(&key)
            || crate::apps::launchers::KEYS.contains(&key)
    }

    fn lines(&self, ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        let items = crate::bindings::ours::items(ctx.state);
        crate::bindings::ours::file_lines(ctx.state)
            .into_iter()
            .map(|file_line| {
                // An item that can't be written is named, so the comment and `doctor` say which.
                let item = match file_line.origin {
                    Origin::Item(index) => items.get(index),
                    _ => None,
                };
                let source = Source::of(vec![crate::bindings::ours::key(&file_line)]);
                let text = file_line
                    .command()
                    .map_err(|error| match item {
                        Some(item) => format!("{item}: {}", error.text().english()),
                        None => error.text().english(),
                    })
                    .and_then(|command| {
                        command
                            .render(Target::Config)
                            .map_err(|error| error.to_string())
                    });
                let line = match text {
                    Ok(text) => Line::plain(text, source),
                    Err(reason) => Line::comment(reason, source),
                };
                (ConfigFile::Bindings, line)
            })
            .collect()
    }
}

/// The runner of the XDG autostart entries and the entries of the Apps & Autostart page in
/// `70-autostart.conf` ([`crate::apps::autostart::file_lines`]). An item that can't be written
/// becomes a comment.
pub(super) struct Autostart;

impl Participant for Autostart {
    fn lines(&self, ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        crate::apps::autostart::file_lines(ctx.state)
            .into_iter()
            .map(|file_line| {
                let source = Source::of(vec![file_line.key]);
                (ConfigFile::Autostart, Line::of(file_line.command, source))
            })
            .collect()
    }
}

/// `75-bar.conf`: with waybar, the hook that starts it with our config unless something else
/// starts it, the user's bars it takes over and the keys that show it; with swaybar, its
/// `bar sway-things …` lines.
pub(super) struct Bar;

impl Participant for Bar {
    fn lines(&self, ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        use crate::bar::{Backend, swaybar};
        use crate::registry::bar as keys;
        let state = ctx.state;
        let lines = match Backend::of(state) {
            Backend::None => Vec::new(),
            Backend::Waybar => {
                let mut lines = vec![Line::rendered(
                    &crate::command::hook(crate::command::Hook::Waybar),
                    Source::of(vec![keys::BACKEND]),
                )];
                // The user's bars it takes over: the first runs our waybar, the others nothing.
                let taken = match state.get(keys::TAKE_OVER, None) {
                    Some(crate::registry::Value::List(ids)) => ids.clone(),
                    _ => Vec::new(),
                };
                for (at, id) in taken.iter().enumerate() {
                    let command = if at == 0 {
                        crate::command::bar::take_over(id)
                    } else {
                        crate::command::bar::silence(id)
                    };
                    let source = Source::of(vec![keys::BACKEND, keys::TAKE_OVER]);
                    let rendered = command
                        .map_err(|error| error.to_string())
                        .and_then(|command| {
                            command
                                .render(Target::Config)
                                .map_err(|error| error.to_string())
                        });
                    lines.push(match rendered {
                        Ok(text) => Line::plain(text, source),
                        Err(error) => Line::comment_on(keys::TAKE_OVER, error, source),
                    });
                }
                if let Some(key) = crate::bar::hold_key(state) {
                    for command in crate::command::bar::waybar_hold(key) {
                        let source = Source::of(vec![keys::VISIBILITY, keys::MODIFIER]);
                        lines.push(match command.render(Target::Config) {
                            Ok(text) => Line::plain(text, source),
                            Err(error) => {
                                Line::comment_on(keys::VISIBILITY, error.to_string(), source)
                            }
                        });
                    }
                }
                lines
            }
            Backend::Swaybar => swaybar::lines(state, None)
                .into_iter()
                .map(|line| match line {
                    swaybar::Line::Command(key, command) => {
                        Line::rendered(&command, Source::of(vec![key]))
                    }
                    swaybar::Line::Skipped(key, reason) => {
                        Line::comment(reason, Source::of(vec![key]))
                    }
                })
                .collect(),
        };
        lines
            .into_iter()
            .map(|line| (ConfigFile::Bar, line))
            .collect()
    }
}

/// The line at the end of `20-output.conf` that starts the collections' wallpapers: one for every
/// output with a collection, naming none ([`crate::wallpaper::start_command`]).
pub(super) struct Wallpaper;

impl Participant for Wallpaper {
    fn prepares_for(&self, setting: &Setting) -> bool {
        setting.page() == Page::Wallpaper
    }

    fn prepare(&self, state: &State) {
        // A link that can't be made leaves the color in place of the picture.
        let _ = crate::wallpaper::prepare(state);
    }

    fn lines(&self, ctx: &Context<'_>) -> Vec<(ConfigFile, Line)> {
        let Some(first) = crate::wallpaper::shown(ctx.state).into_iter().next() else {
            return Vec::new();
        };
        let Ok(text) = crate::wallpaper::start_command().render(Target::Config) else {
            return Vec::new();
        };
        let source = Source {
            keys: vec![crate::wallpaper::COLLECTION],
            device: first.output,
        };
        vec![(ConfigFile::Output, Line::plain(text, source))]
    }
}
