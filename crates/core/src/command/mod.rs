// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed sway commands.
//!
//! Every command Sway Things sends over IPC or writes into a generated config file is built as a
//! [`SwayCmd`]. External strings only enter as [`SwayText`] or as [`Criteria`] values, never as raw
//! strings, and [`SwayCmd::render`] quotes and escapes them for the [`Target`]. sway parses IPC
//! commands and config lines differently, so the two targets render the same value differently. The
//! rules are described in the wiki (Architecture → Sway command builders).

mod color;
mod criteria;
pub mod input;
#[cfg(test)]
mod model;
#[cfg(test)]
mod tests;
mod text;
pub mod window;

use std::fmt;

use crate::version::{SwayFxVersion, SwayVersion};

pub use color::{Color, ColorError};
pub use criteria::{Criteria, CriteriaKey};
pub use text::{SwayText, TextError};

/// Where a rendered command goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// The payload of an IPC `RUN_COMMAND` message.
    Ipc,
    /// One line of a generated config file.
    Config,
}

/// Where a command can be used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Availability {
    /// In config files and over IPC.
    #[default]
    Anywhere,
    /// Only in config files, e.g. `xwayland`, which sway can't change at runtime.
    ConfigOnly,
    /// Only over IPC, e.g. `xkb_switch_layout`. sway defers such commands while it reads the config
    /// and later parses them like IPC commands, so the config rendering would be wrong for them.
    RuntimeOnly,
}

/// How a directive spells a boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoolStyle {
    /// `enabled` / `disabled` (most `input` options).
    EnabledDisabled,
    /// `enable` / `disable` (e.g. `tiling_drag`).
    EnableDisable,
    /// `yes` / `no` (e.g. `focus_follows_mouse`).
    YesNo,
    /// `on` / `off` (e.g. `smart_gaps`).
    OnOff,
}

impl BoolStyle {
    const fn keyword(self, value: bool) -> &'static str {
        match (self, value) {
            (Self::EnabledDisabled, true) => "enabled",
            (Self::EnabledDisabled, false) => "disabled",
            (Self::EnableDisable, true) => "enable",
            (Self::EnableDisable, false) => "disable",
            (Self::YesNo, true) => "yes",
            (Self::YesNo, false) => "no",
            (Self::OnOff, true) => "on",
            (Self::OnOff, false) => "off",
        }
    }
}

/// Commands whose arguments sway doesn't unquote (in config files or over IPC). A text argument
/// would keep its quotes, so these commands take none.
const KEEPS_QUOTES: &[&str] = &[
    "exec",
    "exec_always",
    "mode",
    "bindsym",
    "bindcode",
    "bindswitch",
    "bindgesture",
    "set",
    "for_window",
];

#[derive(Debug, Clone, PartialEq)]
enum Part {
    Keyword(&'static str),
    Int(i64),
    Float(f64),
    Color(Color),
    Text(SwayText),
    Criteria(Criteria),
    Command(Box<SwayCmd>),
}

/// A sway command built from typed parts.
///
/// ```
/// use sway_things_core::command::{BoolStyle, SwayCmd, SwayText, Target};
///
/// let device = SwayText::new("1267:12377:ELAN1300:00 04F3:3059 Touchpad")?;
/// let cmd = SwayCmd::new("input")
///     .text(device)
///     .keyword("tap")
///     .bool(true, BoolStyle::EnabledDisabled);
/// assert_eq!(
///     cmd.render(Target::Ipc)?,
///     r#"input "1267:12377:ELAN1300:00 04F3:3059 Touchpad" tap enabled"#
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SwayCmd {
    parts: Vec<Part>,
    availability: Availability,
    min_sway: SwayVersion,
    min_swayfx: Option<SwayFxVersion>,
    criteria: Option<Criteria>,
}

impl SwayCmd {
    /// Starts a command with its name, e.g. `input` or `client.focused`.
    pub fn new(name: &'static str) -> Self {
        Self {
            parts: vec![Part::Keyword(name)],
            availability: Availability::Anywhere,
            min_sway: SwayVersion::OLDEST_SUPPORTED,
            min_swayfx: None,
            criteria: None,
        }
    }

    /// `for_window <criteria> <command>`: runs `command` on every new window that matches
    /// `criteria`.
    ///
    /// sway stores the inner command and parses it again, like an IPC command, when a window
    /// appears. Its text is therefore quoted twice over IPC, and its `$` doubled twice in a config
    /// line.
    ///
    /// # Errors
    ///
    /// The inner command can't contain criteria or another `for_window`
    /// ([`CommandError::NestedCriteria`]) and must work at runtime
    /// ([`CommandError::Unavailable`]).
    pub fn for_window(criteria: Criteria, command: Self) -> Result<Self, CommandError> {
        command.check_inner()?;
        let (min_sway, min_swayfx) = (command.min_sway, command.min_swayfx);
        Ok(Self {
            parts: vec![
                Part::Keyword("for_window"),
                Part::Criteria(criteria),
                Part::Command(Box::new(command)),
            ],
            availability: Availability::Anywhere,
            min_sway,
            min_swayfx,
            criteria: None,
        })
    }

    /// `<criteria> <command>`: runs `command` now on every open window that matches `criteria`,
    /// e.g. `[app_id="^firefox\z"] opacity 0.8`. Only works over IPC; sway reports a failure if no
    /// window matches.
    ///
    /// # Errors
    ///
    /// Like [`SwayCmd::for_window`]: the command can't contain criteria or `for_window`, and must
    /// work at runtime.
    pub fn with_criteria(criteria: Criteria, command: Self) -> Result<Self, CommandError> {
        command.check_inner()?;
        Ok(Self {
            availability: Availability::RuntimeOnly,
            criteria: Some(criteria),
            ..command
        })
    }

    /// Checks that `self` can run inside `for_window` or after leading criteria.
    fn check_inner(&self) -> Result<(), CommandError> {
        if self.criteria.is_some()
            || self
                .parts
                .iter()
                .any(|part| matches!(part, Part::Criteria(_) | Part::Command(_)))
        {
            return Err(CommandError::NestedCriteria);
        }
        if self.availability == Availability::ConfigOnly {
            return Err(CommandError::Unavailable {
                target: Target::Ipc,
            });
        }
        Ok(())
    }

    /// Appends a keyword, e.g. `tap` or `type:touchpad`. Only ASCII letters, digits and `_ . : * +
    /// -` are allowed; rendering fails otherwise.
    #[must_use]
    pub fn keyword(mut self, keyword: &'static str) -> Self {
        self.parts.push(Part::Keyword(keyword));
        self
    }

    /// Appends external text, quoted and escaped for the target.
    #[must_use]
    pub fn text(mut self, text: SwayText) -> Self {
        self.parts.push(Part::Text(text));
        self
    }

    /// Appends an integer.
    #[must_use]
    pub fn int(mut self, value: i64) -> Self {
        self.parts.push(Part::Int(value));
        self
    }

    /// Appends a decimal number. Rendering fails if it isn't finite.
    #[must_use]
    pub fn float(mut self, value: f64) -> Self {
        self.parts.push(Part::Float(value));
        self
    }

    /// Appends a boolean in the directive's spelling.
    #[must_use]
    pub fn bool(self, value: bool, style: BoolStyle) -> Self {
        self.keyword(style.keyword(value))
    }

    /// Appends a color.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.parts.push(Part::Color(color));
        self
    }

    /// Marks the command as usable only in config files.
    #[must_use]
    pub fn config_only(mut self) -> Self {
        self.availability = Availability::ConfigOnly;
        self
    }

    /// Marks the command as usable only over IPC.
    #[must_use]
    pub fn runtime_only(mut self) -> Self {
        self.availability = Availability::RuntimeOnly;
        self
    }

    /// Where the command can be used.
    pub fn availability(&self) -> Availability {
        self.availability
    }

    /// Records that the command needs at least `version`, e.g. because of a value that older
    /// versions don't know. Rendering doesn't check it; whoever applies the command compares it
    /// with the running sway. Keeps the higher requirement if one is already set.
    #[must_use]
    pub fn requires_sway(mut self, version: SwayVersion) -> Self {
        self.min_sway = self.min_sway.max(version);
        self
    }

    /// The oldest sway that understands the command
    /// ([`SwayVersion::OLDEST_SUPPORTED`] unless raised with [`SwayCmd::requires_sway`]).
    pub fn min_sway(&self) -> SwayVersion {
        self.min_sway
    }

    /// Records that the command only works on SwayFX, at least `version`. Keeps the higher
    /// requirement if one is already set.
    #[must_use]
    pub fn requires_swayfx(mut self, version: SwayFxVersion) -> Self {
        self.min_swayfx = Some(
            self.min_swayfx
                .map_or(version, |current| current.max(version)),
        );
        self
    }

    /// The oldest SwayFX that understands the command, or `None` if vanilla sway does too.
    pub fn min_swayfx(&self) -> Option<SwayFxVersion> {
        self.min_swayfx
    }

    /// Renders the command for `target`.
    ///
    /// # Errors
    ///
    /// Returns a [`CommandError`] if the command isn't available for `target`, a keyword contains a
    /// character outside the allowed set, a number isn't finite, criteria are empty, or a command
    /// that keeps quotes has a text argument.
    pub fn render(&self, target: Target) -> Result<String, CommandError> {
        match (self.availability, target) {
            (Availability::ConfigOnly, Target::Ipc)
            | (Availability::RuntimeOnly, Target::Config) => {
                return Err(CommandError::Unavailable { target });
            }
            _ => {}
        }
        let command = self.render_as(match target {
            Target::Ipc => Rendering::Ipc,
            Target::Config => Rendering::Config,
        })?;
        match &self.criteria {
            Some(criteria) if criteria.is_empty() => Err(CommandError::EmptyCriteria),
            Some(criteria) => Ok(format!("{} {command}", criteria.render(target))),
            None => Ok(command),
        }
    }

    fn render_as(&self, rendering: Rendering) -> Result<String, CommandError> {
        if let Some(Part::Keyword(name)) = self.parts.first() {
            if KEEPS_QUOTES.contains(name)
                && self.parts.iter().any(|part| matches!(part, Part::Text(_)))
            {
                return Err(CommandError::TextKeepsQuotes { command: name });
            }
        }
        let target = match rendering {
            Rendering::Ipc | Rendering::NestedIpc => Target::Ipc,
            Rendering::Config => Target::Config,
        };
        let mut rendered = Vec::with_capacity(self.parts.len());
        for part in &self.parts {
            rendered.push(match part {
                Part::Keyword(keyword) => {
                    if !is_keyword(keyword) {
                        return Err(CommandError::InvalidKeyword { keyword });
                    }
                    (*keyword).to_owned()
                }
                Part::Int(value) => value.to_string(),
                Part::Float(value) => {
                    if !value.is_finite() {
                        return Err(CommandError::NotFinite);
                    }
                    value.to_string()
                }
                Part::Color(color) => color.to_string(),
                Part::Text(text) => match rendering {
                    Rendering::Ipc => text.render_ipc(),
                    Rendering::Config => text.render_config(),
                    Rendering::NestedIpc => text.render_ipc_nested(),
                },
                Part::Criteria(criteria) => {
                    if criteria.is_empty() {
                        return Err(CommandError::EmptyCriteria);
                    }
                    criteria.render(target)
                }
                Part::Command(inner) => match rendering {
                    Rendering::Ipc => inner.render_as(Rendering::NestedIpc)?,
                    // A config line goes through one more `$` replacement than an IPC command.
                    Rendering::Config => inner.render_as(Rendering::Ipc)?.replace('$', "$$"),
                    Rendering::NestedIpc => return Err(CommandError::NestedCriteria),
                },
            });
        }
        Ok(rendered.join(" "))
    }
}

/// How the parts of a command are rendered.
#[derive(Debug, Clone, Copy)]
enum Rendering {
    Ipc,
    Config,
    /// An IPC command that sway parses once more after unquoting it, such as the command of
    /// `for_window`.
    NestedIpc,
}

fn is_keyword(keyword: &str) -> bool {
    !keyword.is_empty()
        && keyword.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'*' | b'+' | b'-')
        })
}

/// Why a command can't be built or rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    /// The command isn't available for this target.
    Unavailable {
        /// The requested target.
        target: Target,
    },
    /// A keyword is empty or contains a character outside ASCII letters, digits and `_ . : * + -`.
    InvalidKeyword {
        /// The keyword.
        keyword: &'static str,
    },
    /// A decimal number is infinite or NaN.
    NotFinite,
    /// Criteria have no match.
    EmptyCriteria,
    /// The command of `for_window` or after leading criteria contains criteria or `for_window`.
    NestedCriteria,
    /// A command whose arguments sway doesn't unquote has a text argument.
    TextKeepsQuotes {
        /// The command name.
        command: &'static str,
    },
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable {
                target: Target::Ipc,
            } => f.write_str("the command only works in config files"),
            Self::Unavailable {
                target: Target::Config,
            } => f.write_str("the command only works over IPC"),
            Self::InvalidKeyword { keyword } => write!(f, "invalid keyword {keyword:?}"),
            Self::NotFinite => f.write_str("number is not finite"),
            Self::EmptyCriteria => f.write_str("criteria have no match"),
            Self::NestedCriteria => {
                f.write_str("a command after criteria can't contain criteria or for_window")
            }
            Self::TextKeepsQuotes { command } => write!(f, "{command} can't take a text argument"),
        }
    }
}

impl std::error::Error for CommandError {}

/// A number is outside the range sway accepts (or NaN).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutOfRange {
    /// The option, e.g. `pointer_accel`.
    pub option: &'static str,
    /// The smallest accepted value.
    pub min: f64,
    /// The largest accepted value.
    pub max: f64,
}

impl OutOfRange {
    /// Checks that `value` is within `min ..= max`.
    pub(crate) fn check(option: &'static str, value: f64, min: f64, max: f64) -> Result<(), Self> {
        if (min..=max).contains(&value) {
            Ok(())
        } else {
            Err(Self { option, min, max })
        }
    }
}

impl fmt::Display for OutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} must be between {} and {}",
            self.option, self.min, self.max
        )
    }
}

impl std::error::Error for OutOfRange {}
