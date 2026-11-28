// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The values a setting can hold and the value types that check them.

use std::fmt;
use std::str::FromStr;

use super::Msg;
use super::words::SplitError;
use crate::command::{Color, TextError};
use crate::i18n::Text;
use crate::msg;

/// The value of a setting.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// On or off.
    Bool(bool),
    /// A whole number.
    Integer(i64),
    /// A decimal number.
    Decimal(f64),
    /// The id of one of the setting's choices, e.g. `always`.
    Choice(String),
    /// A color.
    Color(Color),
    /// Free text, e.g. a font description.
    Text(String),
    /// An ordered list of texts, e.g. keyboard layouts.
    List(Vec<String>),
}

impl Value {
    /// The name of the value's type, for error messages.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "boolean",
            Self::Integer(_) => "integer",
            Self::Decimal(_) => "decimal",
            Self::Choice(_) => "choice",
            Self::Color(_) => "color",
            Self::Text(_) => "text",
            Self::List(_) => "list",
        }
    }
}

/// The value as it is typed on the command line, which [`Kind::parse`] reads back: `true`, `8`,
/// `0.5`, a choice's id, `#rrggbb`, the text itself, or the items of a list joined by commas.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(value) => value.fmt(f),
            Self::Integer(value) => value.fmt(f),
            Self::Decimal(value) => value.fmt(f),
            Self::Choice(text) | Self::Text(text) => f.write_str(text),
            Self::Color(color) => color.fmt(f),
            Self::List(items) => f.write_str(&items.join(",")),
        }
    }
}

/// One of the values of a [`Kind::Choice`] setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// The value as stored and as typed on the command line, e.g. `always`.
    pub id: &'static str,
    /// The translatable label.
    pub label: Msg,
}

impl Choice {
    /// A choice with its stored `id` and its `label`.
    pub const fn new(id: &'static str, label: Msg) -> Self {
        Self { id, label }
    }
}

/// The value type of a setting, with its default and limits.
///
/// The default is what sway (or libinput, or SwayFX) does when nothing is configured, so a setting
/// that isn't stored shows the value that is in effect without Sway Things.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// On or off.
    Bool {
        /// The default.
        default: bool,
    },
    /// A whole number within `min ..= max`.
    Integer {
        /// The default.
        default: i64,
        /// The smallest accepted value.
        min: i64,
        /// The largest accepted value.
        max: i64,
    },
    /// A decimal number within `min ..= max`.
    Decimal {
        /// The default.
        default: f64,
        /// The smallest accepted value.
        min: f64,
        /// The largest accepted value.
        max: f64,
    },
    /// One of a fixed set of choices.
    Choice {
        /// The id of the default choice.
        default: &'static str,
        /// The choices, in display order.
        choices: &'static [Choice],
    },
    /// A color.
    Color {
        /// The default.
        default: Color,
        /// Whether the color may be translucent (`#rrggbbaa`). sway's `output … bg` only takes
        /// `#rrggbb`.
        alpha: bool,
    },
    /// Free text.
    Text {
        /// The default.
        default: &'static str,
    },
    /// An ordered list of texts.
    List {
        /// The default.
        default: &'static [&'static str],
    },
}

impl Kind {
    /// The default value.
    pub fn default_value(&self) -> Value {
        match self {
            Self::Bool { default } => Value::Bool(*default),
            Self::Integer { default, .. } => Value::Integer(*default),
            Self::Decimal { default, .. } => Value::Decimal(*default),
            Self::Choice { default, .. } => Value::Choice((*default).to_owned()),
            Self::Color { default, .. } => Value::Color(*default),
            Self::Text { default } => Value::Text((*default).to_owned()),
            Self::List { default } => {
                Value::List(default.iter().map(|item| (*item).to_owned()).collect())
            }
        }
    }

    /// The name of the value type, for error messages.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Bool { .. } => "boolean",
            Self::Integer { .. } => "integer",
            Self::Decimal { .. } => "decimal",
            Self::Choice { .. } => "choice",
            Self::Color { .. } => "color",
            Self::Text { .. } => "text",
            Self::List { .. } => "list",
        }
    }

    /// Reads a value of this type from `text` as it is typed on the command line, and checks it
    /// like [`Kind::check`]: `true` or `false`, a number, a choice's id, a color as `#rrggbb` or
    /// `#rrggbbaa`, any text, or a list as its items separated by commas (`us,de`, like sway's
    /// `xkb_layout`). Blanks around an item and empty items at the end are dropped, so an empty
    /// text is an empty list; empty items before others stay, because a list can line up with
    /// another one (`,nodeadkeys`: no variant for the first layout).
    ///
    /// # Errors
    ///
    /// A [`ValueError`] that says what is wrong with the text.
    pub fn parse(&self, text: &str) -> Result<Value, ValueError> {
        let unparsable = || ValueError::Unparsable {
            expected: self.type_name(),
        };
        let value = match self {
            Self::Bool { .. } => match text {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => return Err(unparsable()),
            },
            Self::Integer { .. } => Value::Integer(text.parse().map_err(|_| unparsable())?),
            Self::Decimal { .. } => Value::Decimal(text.parse().map_err(|_| unparsable())?),
            Self::Choice { .. } => Value::Choice(text.to_owned()),
            Self::Color { .. } => {
                Value::Color(Color::from_str(text).map_err(|_| ValueError::NotAColor)?)
            }
            Self::Text { .. } => Value::Text(text.to_owned()),
            Self::List { .. } => {
                let mut items: Vec<String> =
                    text.split(',').map(|item| item.trim().to_owned()).collect();
                while items.last().is_some_and(String::is_empty) {
                    items.pop();
                }
                Value::List(items)
            }
        };
        self.check(&value)?;
        Ok(value)
    }

    /// Checks that `value` has this type and is within its limits.
    ///
    /// Text and list values are only checked for their type here. What sway accepts in them
    /// depends on the directive, so the command builders check that when the setting is rendered.
    ///
    /// # Errors
    ///
    /// A [`ValueError`] that says what is wrong with the value.
    pub fn check(&self, value: &Value) -> Result<(), ValueError> {
        match (self, value) {
            (Self::Color { alpha: false, .. }, Value::Color(color))
                if color.alpha().is_some_and(|alpha| alpha != u8::MAX) =>
            {
                Err(ValueError::Translucent)
            }
            (Self::Bool { .. }, Value::Bool(_))
            | (Self::Color { .. }, Value::Color(_))
            | (Self::Text { .. }, Value::Text(_))
            | (Self::List { .. }, Value::List(_)) => Ok(()),
            (Self::Integer { min, max, .. }, Value::Integer(value)) => {
                if (min..=max).contains(&value) {
                    Ok(())
                } else {
                    Err(ValueError::IntegerOutOfRange {
                        min: *min,
                        max: *max,
                    })
                }
            }
            (Self::Decimal { min, max, .. }, Value::Decimal(value)) => {
                // NaN is in no range, so it fails here too.
                if (min..=max).contains(&value) {
                    Ok(())
                } else {
                    Err(ValueError::DecimalOutOfRange {
                        min: *min,
                        max: *max,
                    })
                }
            }
            (Self::Choice { choices, .. }, Value::Choice(id)) => {
                if choices.iter().any(|choice| choice.id == id) {
                    Ok(())
                } else {
                    Err(ValueError::UnknownChoice { choices })
                }
            }
            _ => Err(ValueError::WrongType {
                expected: self.type_name(),
                found: value.type_name(),
            }),
        }
    }
}

/// Why a value doesn't fit a setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueError {
    /// The value has another type than the setting.
    WrongType {
        /// The type the setting takes.
        expected: &'static str,
        /// The type of the value.
        found: &'static str,
    },
    /// A whole number is outside the setting's range.
    IntegerOutOfRange {
        /// The smallest accepted value.
        min: i64,
        /// The largest accepted value.
        max: i64,
    },
    /// A decimal number is outside the setting's range, or not a number.
    DecimalOutOfRange {
        /// The smallest accepted value.
        min: f64,
        /// The largest accepted value.
        max: f64,
    },
    /// The text isn't a color in `#rrggbb` or `#rrggbbaa` notation.
    NotAColor,
    /// The color is translucent, but the setting only takes opaque colors.
    Translucent,
    /// The text can't be read as a value of the setting's type.
    Unparsable {
        /// The type the setting takes.
        expected: &'static str,
    },
    /// The value isn't one of the setting's choices.
    UnknownChoice {
        /// The choices the setting has.
        choices: &'static [Choice],
    },
    /// The text can't be split into a program and its arguments.
    NotACommand(SplitError),
    /// The text can't be an argument of a sway command, e.g. a cursor theme or output name.
    NotSwayText(TextError),
    /// The text isn't an output mode like `2560x1440@59.951Hz`.
    NotAMode,
    /// The text isn't a position in the layout like `1920,0`.
    NotAPosition,
    /// The text isn't the name of a theme's directory: empty, too long, or with a slash or a
    /// control character.
    NotAThemeName,
    /// The text isn't a font description: empty, too long, or with a control character.
    NotAFont,
    /// The text can't be the name of a wallpaper collection.
    NotACollectionName,
    /// The text isn't a binding line Sway Things can write (`bindsym $mod+Return exec foot`).
    NotABinding(crate::sway_config::bindings::LineErrorKind),
    /// The text isn't keys Sway Things can bind (`Print`, `$mod+Shift+v`).
    NotAKey,
    /// The keys would type a character: a key that types needs Super, Ctrl or Alt.
    TypingKey,
    /// The text isn't a folder: not absolute (or below `~/`), or with a line break.
    NotAFolder,
    /// The text isn't a file name pattern: empty, with a slash or a line break, or with a `%`
    /// that isn't a known conversion.
    NotAFileName,
    /// The text isn't a time of day as `HH:MM`, 24-hour.
    NotATime,
    /// The keys type a character: a key that types needs Super, Ctrl or Alt.
    TypingKeys,
    /// The text isn't a desktop file ID like `org.mozilla.firefox.desktop`.
    NotADesktopId,
    /// The text isn't an autostart entry of Sway Things: `exec <command>` or `app <desktop file
    /// ID>`, switched off with `# ` in front.
    NotAnAutostartEntry,
    /// The text isn't a JSON object (empty stands for none).
    NotAJsonObject,
    /// The text isn't a name swaync takes for a widget: letters, digits, `_` and `-`, maybe with
    /// `#` and a name for a second one of a kind.
    NotAWidget,
    /// The text has a control character or is too long for a notification daemon's config.
    NotDaemonText,
    /// The text isn't an environment variable the editor takes.
    NotAVariable(super::environment::VariableError),
    /// The text isn't the name of an output: letters, digits, `.`, `_` and `-`.
    NotAnOutputName,
    /// The text isn't the name of a UTF-8 locale like `de_DE.UTF-8`.
    NotALocale,
    /// The text isn't the name of a waybar module like `clock`, `custom/media` or `battery#bat2`.
    NotAModule,
    /// The text isn't an absolute path without control characters.
    NotAPath,
}

impl ValueError {
    /// What is wrong with the value, to show.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per error, read best as a list"
    )]
    pub fn text(&self) -> Text {
        match self {
            Self::WrongType { expected, found } => {
                Text::new(msg!("expected a {expected} value, found a {found} value"))
                    .text("expected", type_name(expected))
                    .text("found", type_name(found))
            }
            Self::IntegerOutOfRange { min, max } => {
                Text::new(msg!("must be between {min} and {max}"))
                    .arg("min", min)
                    .arg("max", max)
            }
            Self::DecimalOutOfRange { min, max } => {
                Text::new(msg!("must be between {min} and {max}"))
                    .arg("min", min)
                    .arg("max", max)
            }
            Self::NotAColor => msg!("must be a color like #rrggbb or #rrggbbaa").into(),
            Self::Translucent => msg!("must be an opaque color like #rrggbb").into(),
            Self::Unparsable {
                expected: "boolean",
            } => msg!("must be true or false").into(),
            Self::Unparsable {
                expected: "integer",
            } => msg!("must be a whole number").into(),
            Self::Unparsable {
                expected: "decimal",
            } => msg!("must be a number").into(),
            Self::Unparsable { expected } => {
                Text::new(msg!("must be a {expected} value")).text("expected", type_name(expected))
            }
            Self::UnknownChoice { choices } => {
                let ids: Vec<&str> = choices.iter().map(|choice| choice.id).collect();
                Text::new(msg!("must be one of: {choices}")).arg("choices", ids.join(", "))
            }
            Self::NotACommand(error) => error.text(),
            Self::NotSwayText(error) => error.text(),
            Self::NotAMode => {
                msg!("must be a mode like 2560x1440 or 2560x1440@59.951Hz, or empty").into()
            }
            Self::NotAPosition => msg!("must be a position like 1920,0, or empty").into(),
            Self::NotAThemeName => {
                msg!("must be the name of a theme's folder, without slashes or line breaks").into()
            }
            Self::NotAFont => {
                msg!("must be a font and size like \"Adwaita Sans 11\", without line breaks").into()
            }
            Self::NotACollectionName => msg!(
                "a collection name has 1 to 64 characters, no line breaks and no spaces at \
                 either end"
            )
            .into(),
            Self::NotABinding(kind) => kind.text(),
            Self::NotAVariable(error) => error.text(),
            Self::NotAnOutputName => {
                msg!("must be the name of a display like DP-1: letters, digits, '.', '_' and '-'")
                    .into()
            }
            Self::NotAKey => {
                msg!("must be keys like Print or $mod+Shift+v, joined by +, or empty").into()
            }
            Self::TypingKey => msg!(
                "a key that types needs Super, Ctrl or Alt; F1, Print and media keys work alone"
            )
            .into(),
            Self::NotAFolder => {
                msg!("must be a folder that starts with / or ~/, without line breaks, or empty")
                    .into()
            }
            Self::NotAFileName => msg!(
                "must be a file name without slashes or line breaks; % starts a date or time \
                 like %Y-%m-%d %H-%M-%S, and %% is a %"
            )
            .into(),
            Self::NotATime => msg!("must be a time as HH:MM, from 00:00 to 23:59").into(),
            Self::TypingKeys => msg!("keys that type a character need Super, Ctrl or Alt").into(),
            Self::NotADesktopId => msg!(
                "must be a desktop file ID like org.mozilla.firefox.desktop: letters, digits, \
                 -, _ and . ending in .desktop"
            )
            .into(),
            Self::NotAnAutostartEntry => msg!(
                "must be exec and a command line, or app and a desktop file ID; # in front \
                 switches it off"
            )
            .into(),
            Self::NotAJsonObject => msg!("must be a JSON object, or empty").into(),
            Self::NotAWidget => {
                msg!("must be a widget name like title or buttons-grid#quick").into()
            }
            Self::NotDaemonText => {
                msg!("must be at most 255 characters, without line breaks").into()
            }
            Self::NotALocale => {
                msg!("must be a UTF-8 locale like de_DE.UTF-8 or sr_RS.UTF-8@latin, or empty")
                    .into()
            }
            Self::NotAModule => msg!(
                "must be a module name like clock, custom/media or battery#bat2, without spaces"
            )
            .into(),
            Self::NotAPath => {
                msg!("must be a full path starting with /, without line breaks").into()
            }
        }
    }
}

/// The type `name` ([`Value::type_name`] or a TOML type), to show.
fn type_name(name: &str) -> Text {
    // TRANSLATORS: The types of values in "expected a {expected} value, found a {found} value".
    match name {
        "boolean" => msg!("boolean").into(),
        "integer" => msg!("integer").into(),
        "decimal" => msg!("decimal").into(),
        "choice" => msg!("choice").into(),
        "color" => msg!("color").into(),
        "text" => msg!("text").into(),
        "list" => msg!("list").into(),
        "string" => msg!("string").into(),
        "float" => msg!("float").into(),
        "array" => msg!("array").into(),
        "table" => msg!("table").into(),
        other => Text::data(other),
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for ValueError {}
