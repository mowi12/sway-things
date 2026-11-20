// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bindings Sway Things writes: the items of `shortcuts.bindings`, the groups of media keys,
//! and the lines of `50-bindings.conf` they make.
//!
//! An item is one line in sway's syntax, as people write it: `bindsym $mod+Return exec foot`,
//! `unbindsym Mod4+d`. [`keys::MOD`] stands for the main modifier of `shortcuts.modifier`. A
//! binding of the user's is removed with an `unbindsym` item, and changed with a change item:
//! the unbind, `=>` and the binding that takes its place (`unbindsym Mod4+d => bindsym $mod+p
//! exec menu`). The user's file is never touched.
//!
//! sway rejects an unbind of a binding it doesn't have, and the generated file is checked with
//! `sway --validate` on its own. So each unbind comes after a placeholder binding of the same
//! keys (`bindsym --no-warn Mod4+d nop`) that replaces the user's quietly, if it is there, and is
//! removed right away: the unbind always succeeds, also once the user's line is gone.

use super::keys::{self, Modifiers};
use super::line::{Flags, Line, LineError};
use super::table::Origin;
use crate::command::SwayCmd;
use crate::i18n::Msg;
use crate::msg;
use crate::registry::{Value, ValueError};
use crate::state::State;
use crate::sway_config::line::split_args;

/// The key of the main modifier of the bindings Sway Things writes.
pub const MODIFIER: &str = "shortcuts.modifier";
/// The key of the bindings Sway Things writes and the user's it removes or changes.
pub const BINDINGS: &str = "shortcuts.bindings";
/// The key of the switch of the volume keys.
pub const VOLUME: &str = "shortcuts.media.volume";
/// The key of the switch of the brightness keys.
pub const BRIGHTNESS: &str = "shortcuts.media.brightness";
/// The key of the switch of the media player keys.
pub const PLAYER: &str = "shortcuts.media.player";

/// The modifier `shortcuts.modifier` stands for, by the id of its choice.
pub fn modifier_of(id: &str) -> Modifiers {
    match id {
        "alt" => Modifiers::ALT,
        _ => Modifiers::SUPER,
    }
}

/// The main modifier stored in `state`.
pub fn modifier(state: &State) -> Modifiers {
    match state.get(MODIFIER, None) {
        Some(Value::Choice(id)) => modifier_of(id),
        _ => Modifiers::SUPER,
    }
}

/// What separates the unbind of a change item from the binding that takes its place.
pub const CHANGE: &str = "=>";

/// An item of `shortcuts.bindings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// One line: a binding of Sway Things, or an unbind of a binding of the user's.
    Line(Line),
    /// A binding of the user's changed: `from` unbinds it, `to` binds what takes its place.
    Change {
        /// The unbind of the user's binding.
        from: Line,
        /// The binding in its place.
        to: Line,
    },
}

impl Item {
    /// The binding it makes, if any: the line of a binding, the new binding of a change.
    pub fn binding(&self) -> Option<&Line> {
        match self {
            Self::Line(line) if !line.unbind => Some(line),
            Self::Line(_) => None,
            Self::Change { to, .. } => Some(to),
        }
    }

    /// The unbind it makes, if any.
    pub fn unbinding(&self) -> Option<&Line> {
        match self {
            Self::Line(line) if line.unbind => Some(line),
            Self::Line(_) => None,
            Self::Change { from, .. } => Some(from),
        }
    }

    /// The text of the item, as [`parse`] reads it.
    pub fn text(&self) -> String {
        match self {
            Self::Line(line) => item_text(line),
            Self::Change { from, to } => format!("{} {CHANGE} {}", item_text(from), item_text(to)),
        }
    }
}

/// Reads an item of `shortcuts.bindings`: a line, or a change (`unbindsym … => bindsym …`). A
/// command whose first word sway doesn't know is read too (see [`parse_strict`]).
///
/// # Errors
///
/// Those of [`parse_item`]; for a change, [`LineError::NotABinding`] unless it is an unbind
/// followed by a binding.
pub fn parse(text: &str) -> Result<Item, LineError> {
    parse_as(text, false)
}

/// [`parse`], and every command must start with a command sway or SwayFX knows: what is written
/// to the generated file and what `sway-things set` takes.
///
/// # Errors
///
/// Those of [`parse`], and [`super::CommandError::UnknownCommand`].
pub fn parse_strict(text: &str) -> Result<Item, LineError> {
    parse_as(text, true)
}

fn parse_as(text: &str, strict: bool) -> Result<Item, LineError> {
    let separator = format!(" {CHANGE} ");
    if let Some((from, to)) = text.split_once(&separator) {
        // An unbind has no command, so a separator after one can't be part of a command.
        if let Ok(from) = parse_line(from, strict)
            .map_err(|_| ())
            .and_then(|line| if line.unbind { Ok(line) } else { Err(()) })
        {
            let to = parse_line(to, strict)?;
            if to.unbind {
                return Err(LineError::NotABinding);
            }
            return Ok(Item::Change { from, to });
        }
    }
    parse_line(text, strict).map(Item::Line)
}

/// Reads one line of an item of `shortcuts.bindings`.
///
/// # Errors
///
/// A [`LineError`] if it isn't a binding line Sway Things can write: another command, a `mode`
/// prefix, keys or a device that need quotes, or a command it can't write back exactly.
pub fn parse_item(text: &str) -> Result<Line, LineError> {
    parse_line(text, false)
}

fn parse_line(text: &str, strict: bool) -> Result<Line, LineError> {
    if text.contains('\\') || text.chars().any(char::is_control) {
        return Err(LineError::Command(if text.contains('\\') {
            super::CommandError::Backslash
        } else {
            super::CommandError::ControlCharacter
        }));
    }
    let words: Vec<String> = split_args(text.as_bytes())
        .iter()
        .map(|word| String::from_utf8_lossy(word).into_owned())
        .collect();
    let Some((name, args)) = words.split_first() else {
        return Err(LineError::NotABinding);
    };
    // The combination is read with the default modifier; `resolve` reads it again for the one
    // chosen.
    let resolved: Vec<String> = args
        .iter()
        .map(|arg| {
            if arg.split('+').any(|part| part == keys::MOD) {
                keys::with_mod(arg, Modifiers::SUPER)
            } else {
                arg.clone()
            }
        })
        .collect();
    let mut line = Line::read(name, &resolved)?;
    if let Some(keys) = args.get(super::line::keys_index(args)) {
        line.keys.clone_from(keys);
    }
    // Both renderings must work, which checks the keys, the device and the command.
    line.config(Modifiers::SUPER, false)?;
    if let Some(command) = &line.command {
        let command = if strict {
            super::BindCommand::checked(command)?
        } else {
            super::BindCommand::new(command)?
        };
        line.command = Some(command.as_str().to_owned());
    }
    Ok(line)
}

/// The text of a line of an item, as [`parse_item`] reads it.
pub fn item_text(line: &Line) -> String {
    let mut words = vec![line.verb().to_owned()];
    words.extend(line.flags.words().into_iter().map(str::to_owned));
    if let Some(device) = &line.input {
        words.push(format!("--input-device={device}"));
    }
    words.push(line.keys.clone());
    if let Some(command) = &line.command {
        words.push(command.clone());
    }
    words.join(" ")
}

/// The check of the items of `shortcuts.bindings`, for the registry: what can be read at all. A
/// command sway doesn't know passes here, so one typo in `settings.toml` doesn't drop every
/// binding; its item becomes a comment of the generated file, which `doctor` reports, and
/// `sway-things set` and the app refuse it ([`parse_strict`]).
///
/// # Errors
///
/// [`ValueError::NotABinding`] with the reason of [`parse`].
pub fn check_item(text: &str) -> Result<(), ValueError> {
    parse(text)
        .map(|_| ())
        .map_err(|error| ValueError::NotABinding(error.kind()))
}

/// `line` with [`keys::MOD`] replaced by `modifier`, its keys read again.
pub fn resolve(line: &Line, modifier: Modifiers) -> Line {
    let keys = keys::with_mod(&line.keys, modifier);
    let combo = keys::parse(&keys, line.code, line.combo.device == keys::Device::Mouse)
        .unwrap_or_else(|_| line.combo.clone());
    Line {
        keys,
        combo,
        ..line.clone()
    }
}

/// A group of media keys that a switch binds, with the program it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    /// The key of its switch.
    pub key: &'static str,
    /// The program the bindings run, which must be installed.
    pub tool: &'static str,
    /// The keys and command lines, all `--locked`, so they work on the lock screen too.
    pub keys: &'static [(&'static str, &'static str)],
    /// What the switch is called.
    pub title: Msg,
}

/// The groups of media keys, in the order of their switches.
pub const GROUPS: [Group; 3] = [
    Group {
        key: VOLUME,
        tool: "wpctl",
        keys: &[
            (
                "XF86AudioRaiseVolume",
                "wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+",
            ),
            (
                "XF86AudioLowerVolume",
                "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-",
            ),
            (
                "XF86AudioMute",
                "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle",
            ),
            (
                "XF86AudioMicMute",
                "wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle",
            ),
        ],
        title: msg!("Volume keys"),
    },
    Group {
        key: BRIGHTNESS,
        tool: "brightnessctl",
        keys: &[
            ("XF86MonBrightnessUp", "brightnessctl set 5%+"),
            (
                "XF86MonBrightnessDown",
                "brightnessctl --min-value=1 set 5%-",
            ),
        ],
        title: msg!("Brightness keys"),
    },
    Group {
        key: PLAYER,
        tool: "playerctl",
        keys: &[
            ("XF86AudioPlay", "playerctl play-pause"),
            ("XF86AudioPause", "playerctl play-pause"),
            ("XF86AudioNext", "playerctl next"),
            ("XF86AudioPrev", "playerctl previous"),
            ("XF86AudioStop", "playerctl stop"),
        ],
        title: msg!("Media player keys"),
    },
];

impl Group {
    /// The group whose switch has the key `key`.
    pub fn of(key: &str) -> Option<Self> {
        GROUPS.into_iter().find(|group| group.key == key)
    }

    /// The bindings of the group.
    pub fn lines(self) -> Vec<Line> {
        self.keys
            .iter()
            .filter_map(|(keys, run)| {
                let command = super::Action::Run((*run).to_owned()).command().ok()?;
                Some(Line {
                    unbind: false,
                    code: false,
                    flags: Flags {
                        locked: true,
                        ..Flags::default()
                    },
                    input: None,
                    keys: (*keys).to_owned(),
                    combo: keys::parse(keys, false, false).ok()?,
                    command: Some(command.as_str().to_owned()),
                })
            })
            .collect()
    }

    /// Whether its program is installed (in `$PATH`).
    pub fn installed(self) -> bool {
        crate::delegate::find_program(self.tool, std::env::var_os("PATH").as_deref()).is_some()
    }
}

/// A line of `50-bindings.conf`: a binding of Sway Things, with its keys for the main modifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLine {
    /// Where it comes from.
    pub origin: Origin,
    /// The line; `Err` for an item that can't be written, which becomes a comment.
    pub line: Result<Line, LineError>,
    /// Whether it is written with `--no-warn`: it may replace a binding of the user's, which
    /// the page says.
    pub no_warn: bool,
}

impl FileLine {
    /// The command for the generated file.
    ///
    /// # Errors
    ///
    /// The [`LineError`] of a line that can't be written.
    pub fn command(&self) -> Result<SwayCmd, LineError> {
        let line = self.line.as_ref().map_err(Clone::clone)?;
        SwayCmd::binding(line, Modifiers::NONE, self.no_warn)
    }

    /// The setting the line comes from.
    pub fn key(&self) -> &'static str {
        match self.origin {
            Origin::Group(key) | Origin::Feature(key) => key,
            Origin::User | Origin::Item(_) | Origin::Placeholder(_) => BINDINGS,
        }
    }
}

/// The items of `shortcuts.bindings` stored in `state`, as written.
pub fn items(state: &State) -> Vec<String> {
    match state.get(BINDINGS, None) {
        Some(Value::List(items)) => items.clone(),
        _ => Vec::new(),
    }
}

/// The lines of `50-bindings.conf` for `state`, in order: the unbinds of the user's bindings
/// (each after its placeholder), the media keys switched on, the keys of other pages
/// ([`crate::capture::shortcuts`]), then the bindings of the items.
pub fn file_lines(state: &State) -> Vec<FileLine> {
    let modifier = modifier(state);
    let parsed: Vec<Result<Item, LineError>> =
        items(state).iter().map(|item| parse_strict(item)).collect();
    let mut lines = Vec::new();
    for (index, item) in parsed.iter().enumerate() {
        match item {
            Ok(item) => {
                if let Some(unbind) = item.unbinding() {
                    let line = resolve(unbind, modifier);
                    let mut placeholder = Line {
                        unbind: false,
                        command: Some("nop".to_owned()),
                        ..line.clone()
                    };
                    placeholder.flags.no_warn = false;
                    lines.push(FileLine {
                        origin: Origin::Placeholder(index),
                        line: Ok(placeholder),
                        no_warn: true,
                    });
                    lines.push(FileLine {
                        origin: Origin::Item(index),
                        line: Ok(line),
                        no_warn: false,
                    });
                }
            }
            Err(error) => lines.push(FileLine {
                origin: Origin::Item(index),
                line: Err(error.clone()),
                no_warn: false,
            }),
        }
    }
    for group in GROUPS {
        if state.get(group.key, None) == Some(&Value::Bool(true)) {
            lines.extend(group.lines().into_iter().map(|line| FileLine {
                origin: Origin::Group(group.key),
                line: Ok(line),
                no_warn: true,
            }));
        }
    }
    // The keys of other pages, such as those of screenshots.
    lines.extend(crate::capture::shortcuts::file_lines(state));
    for (index, item) in parsed.iter().enumerate() {
        if let Some(binding) = item.as_ref().ok().and_then(Item::binding) {
            lines.push(FileLine {
                origin: Origin::Item(index),
                line: Ok(resolve(binding, modifier)),
                no_warn: false,
            });
        }
    }
    lines
}
