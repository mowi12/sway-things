// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The values a setting can hold and the value types that check them.

use std::fmt;

use super::Msg;
use crate::command::Color;

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

/// One of the values of a [`Kind::Choice`] setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// The value as stored and as typed on the command line, e.g. `always`.
    pub id: &'static str,
    /// The translatable label.
    pub label: Msg,
}

impl Choice {
    /// A choice with its stored `id` and its English `label`.
    pub const fn new(id: &'static str, label: &'static str) -> Self {
        Self {
            id,
            label: Msg::new(label),
        }
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
            Self::Color { default } => Value::Color(*default),
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
    /// The value isn't one of the setting's choices.
    UnknownChoice {
        /// The choices the setting has.
        choices: &'static [Choice],
    },
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongType { expected, found } => {
                write!(f, "expected a {expected} value, found a {found} value")
            }
            Self::IntegerOutOfRange { min, max } => {
                write!(f, "must be between {min} and {max}")
            }
            Self::DecimalOutOfRange { min, max } => {
                write!(f, "must be between {min} and {max}")
            }
            Self::NotAColor => f.write_str("must be a color like #rrggbb or #rrggbbaa"),
            Self::UnknownChoice { choices } => {
                f.write_str("must be one of: ")?;
                for (index, choice) in choices.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    f.write_str(choice.id)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ValueError {}
