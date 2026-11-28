// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A line of a generated file and the settings it comes from, with the helpers every
//! participant makes its lines with.

use crate::command::{SwayCmd, Target};

/// The stored settings a line of a generated file comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The keys of the stored settings. Several, if they are values of one directive.
    pub keys: Vec<&'static str>,
    /// The input device the line is for, or `None` for the whole session or input type.
    pub device: Option<String>,
}

impl Source {
    /// The settings `keys`, for the whole session.
    pub fn of(keys: Vec<&'static str>) -> Self {
        Self { keys, device: None }
    }

    /// No stored setting: a line Sway Things writes for what runs (kanshi, say).
    pub fn none() -> Self {
        Self::of(Vec::new())
    }
}

/// A line of a generated file below the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The text, without the line break.
    pub text: String,
    /// The settings the line comes from.
    pub source: Source,
    /// `None` for a command. Otherwise the line is a comment that names the settings and says why
    /// their command is left out: the compositor doesn't support it, or a stored value can't be
    /// turned into a command.
    pub skipped: Option<String>,
}

impl Line {
    /// A line written as it is: a command rendered already, or a comment that says what Sway
    /// Things does there.
    pub fn plain(text: impl Into<String>, source: Source) -> Self {
        Self {
            text: text.into(),
            source,
            skipped: None,
        }
    }

    /// The comment in place of the command of `source`'s settings, saying why it is left out.
    pub fn comment(reason: impl Into<String>, source: Source) -> Self {
        let reason = reason.into();
        Self {
            text: format!("# {}: {reason}", source.keys.join(", ")),
            source,
            skipped: Some(reason),
        }
    }

    /// [`Line::comment`] that names `name` in place of the keys of `source`.
    pub fn comment_on(name: &str, reason: impl Into<String>, source: Source) -> Self {
        let reason = reason.into();
        Self {
            text: format!("# {name}: {reason}"),
            source,
            skipped: Some(reason),
        }
    }

    /// `command` for a config file; the [comment](Line::comment) if it can't be rendered.
    pub fn rendered(command: &SwayCmd, source: Source) -> Self {
        match command.render(Target::Config) {
            Ok(text) => Self::plain(text, source),
            Err(error) => Self::comment(error.to_string(), source),
        }
    }

    /// The line of `command`, or the comment for what made it fail.
    pub fn of<E: std::fmt::Display>(command: Result<SwayCmd, E>, source: Source) -> Self {
        match command {
            Ok(command) => Self::rendered(&command, source),
            Err(error) => Self::comment(error.to_string(), source),
        }
    }
}
