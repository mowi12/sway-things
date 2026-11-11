// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading a setting's value from a line of the user's config, as sway reads it.
//!
//! Each sway setting has a [`ReadFn`] next to its render function. It gets the arguments of the
//! user's directive and accepts what sway accepts: the helpers here follow sway's own parsing
//! (`parse_boolean`, `strtol`, `atoi`, `parse_color` in sway's `common/util.c` and the commands).

use std::fmt;

use super::Value;
use crate::command::Color;

/// Reads the value of a setting from the arguments of a user's directive: those after the
/// directive's name and, for a directive with a [first argument](super::Directive::argument),
/// after that.
pub type ReadFn = fn(&[String]) -> Result<Value, ReadError>;

/// Why a value couldn't be read from a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// sway refuses the line, so it changes nothing.
    Rejected(String),
    /// sway takes the line, but the value is one the setting can't hold, e.g. out of its range.
    Unsupported(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(reason) => write!(f, "sway rejects the line: {reason}"),
            Self::Unsupported(reason) => write!(f, "Sway Things can't show this value: {reason}"),
        }
    }
}

impl std::error::Error for ReadError {}

fn rejected(reason: impl Into<String>) -> ReadError {
    ReadError::Rejected(reason.into())
}

/// The only argument: sway's `checkarg(…, EXPECTED_EQUAL_TO, 1)`.
pub(super) fn one(args: &[String]) -> Result<&str, ReadError> {
    match args {
        [arg] => Ok(arg),
        _ => Err(rejected(format!("expected 1 argument, got {}", args.len()))),
    }
}

/// The first argument, further ones are ignored: `checkarg(…, EXPECTED_AT_LEAST, 1)`.
pub(super) fn first(args: &[String]) -> Result<&str, ReadError> {
    args.first()
        .map(String::as_str)
        .ok_or_else(|| rejected("expected an argument"))
}

/// sway's `parse_boolean`: a few words are true, `toggle` flips `current`, everything else is
/// false.
pub(super) fn boolean(word: &str, current: bool) -> bool {
    const TRUE: [&str; 7] = ["1", "yes", "on", "true", "enable", "enabled", "active"];
    if TRUE
        .iter()
        .any(|word_true| word.eq_ignore_ascii_case(word_true))
    {
        true
    } else if word.eq_ignore_ascii_case("toggle") {
        !current
    } else {
        false
    }
}

/// C's `strtol` in base 10: blanks, a sign and digits, and the rest of the text after them. Text
/// without digits is 0 with all of it left over. `None` if the number doesn't fit an `i32`, where
/// sway's `int` would wrap.
pub(super) fn strtol(text: &str) -> Option<(i64, &str)> {
    let trimmed = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, unsigned) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let digits = unsigned.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return Some((0, text));
    }
    let (number, rest) = unsigned.split_at(digits);
    let magnitude: i64 = number.parse().ok()?;
    let value = if negative { -magnitude } else { magnitude };
    i32::try_from(value)
        .ok()
        .map(|value| (i64::from(value), rest))
}

/// A whole number with nothing after it, as sway checks it with `strtol` and its end pointer.
pub(super) fn whole(text: &str) -> Result<i64, ReadError> {
    match strtol(text) {
        Some((value, "")) if !text.is_empty() => Ok(value),
        Some(_) => Err(rejected(format!("not a whole number: {text}"))),
        None => Err(ReadError::Unsupported(format!("too large: {text}"))),
    }
}

/// C's `atoi`: the number at the start, whatever follows; 0 without one.
pub(super) fn atoi(text: &str) -> Result<i64, ReadError> {
    strtol(text)
        .map(|(value, _)| value)
        .ok_or_else(|| ReadError::Unsupported(format!("too large: {text}")))
}

/// sway's `parse_float`: the whole text must be a number.
pub(super) fn float(text: &str) -> Result<f64, ReadError> {
    match text.parse::<f64>() {
        Ok(value) if !value.is_nan() => Ok(value),
        _ => Err(rejected(format!("not a number: {text}"))),
    }
}

/// sway's `parse_color`: six or eight hex digits, with or without `#`. An opaque alpha is the
/// same color as none.
pub(super) fn color(text: &str) -> Result<Color, ReadError> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    let invalid = || rejected(format!("not a color: {text}"));
    if !matches!(digits.len(), 6 | 8) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let channel = |index: usize| {
        digits
            .get(index..index + 2)
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
            .ok_or_else(invalid)
    };
    let (red, green, blue) = (channel(0)?, channel(2)?, channel(4)?);
    match digits.get(6..) {
        Some("") | None => Ok(Color::rgb(red, green, blue)),
        Some(_) => match channel(6)? {
            0xff => Ok(Color::rgb(red, green, blue)),
            alpha => Ok(Color::rgba(red, green, blue, alpha)),
        },
    }
}

/// A choice whose id is the word sway takes: `exact` compares as sway's `strcmp`, otherwise as
/// `strcasecmp`.
pub(super) fn choice(word: &str, ids: &[&str], exact: bool) -> Result<Value, ReadError> {
    ids.iter()
        .find(|id| {
            if exact {
                word == **id
            } else {
                word.eq_ignore_ascii_case(id)
            }
        })
        .map(|id| Value::Choice((*id).to_owned()))
        .ok_or_else(|| rejected(format!("expected one of {}, got {word}", ids.join(", "))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn booleans_follow_sway() {
        for word in ["1", "yes", "ON", "True", "enable", "enabled", "active"] {
            assert!(boolean(word, false), "{word}");
        }
        for word in ["0", "no", "off", "disabled", "anything", ""] {
            assert!(!boolean(word, true), "{word}");
        }
        assert!(boolean("toggle", false));
        assert!(!boolean("Toggle", true));
    }

    #[test]
    fn numbers_follow_strtol_and_atoi() {
        assert_eq!(strtol("8"), Some((8, "")));
        assert_eq!(strtol(" -12px"), Some((-12, "px")));
        assert_eq!(strtol("+3"), Some((3, "")));
        assert_eq!(strtol("px"), Some((0, "px")));
        assert_eq!(strtol("99999999999"), None);
        assert_eq!(whole("42"), Ok(42));
        assert!(matches!(whole("4x"), Err(ReadError::Rejected(_))));
        assert!(matches!(whole(""), Err(ReadError::Rejected(_))));
        assert_eq!(atoi("250ms"), Ok(250));
        assert_eq!(atoi("fast"), Ok(0));
        assert_eq!(float("-0.5"), Ok(-0.5));
        assert!(float("nan").is_err());
        assert!(float("0.5x").is_err());
    }

    #[test]
    fn colors_follow_parse_color() {
        assert_eq!(color("#285577"), Ok(Color::rgb(0x28, 0x55, 0x77)));
        assert_eq!(color("285577"), Ok(Color::rgb(0x28, 0x55, 0x77)));
        assert_eq!(color("#285577FF"), Ok(Color::rgb(0x28, 0x55, 0x77)));
        assert_eq!(color("#28557780"), Ok(Color::rgba(0x28, 0x55, 0x77, 0x80)));
        for text in ["#28557", "#2855777", "#28557g", "red", "", "#"] {
            assert!(color(text).is_err(), "{text}");
        }
    }

    #[test]
    fn choices_compare_like_sway() {
        assert_eq!(
            choice("Tabbed", &["default", "tabbed"], false),
            Ok(Value::Choice("tabbed".to_owned()))
        );
        assert!(choice("Yes", &["yes", "no"], true).is_err());
    }

    #[test]
    fn argument_counts_are_checked() {
        let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert_eq!(one(&args(&["a"])), Ok("a"));
        assert!(one(&args(&["a", "b"])).is_err());
        assert!(one(&args(&[])).is_err());
        assert_eq!(first(&args(&["a", "b"])), Ok("a"));
        assert!(first(&args(&[])).is_err());
    }
}
