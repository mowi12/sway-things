// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The items of `shortcuts.bindings` as text: one binding line in sway's syntax, or a change of a
//! binding of the user's (`unbindsym … => bindsym …`), read and written back.
//!
//! It imports neither the registry nor the state, so the registry's check of the items
//! ([`crate::registry::shortcuts::check_item`]) reads them here without depending on the modules
//! built on it. [`super::ours`] re-exports everything.

use super::keys::{self, Modifiers};
use super::line::{Line, LineError};
use crate::sway_config::line::split_args;

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
