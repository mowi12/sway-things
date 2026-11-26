// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! JSON with comments, as waybar reads its config.
//!
//! waybar parses its config with jsoncpp's `CharReaderBuilder` at its defaults: `//` and `/* */`
//! comments are allowed, and so are commas before a closing bracket; before parsing, waybar
//! replaces every `\x` with `\u00`, so `\x41` is `A`. [`parse`] turns such a text into plain JSON
//! and hands it to `serde_json`; a key given twice keeps its last value there, as in jsoncpp.
//! [`write`] gives the JSON waybar reads back, with comment lines at the top.

use std::fmt;

use serde_json::Value;

use crate::i18n::Text;
use crate::msg;

/// The most a config is read: waybar's own are a few kilobytes, and so are rich ones.
pub const MAX_SIZE: usize = 4 * 1024 * 1024;

/// Why a text isn't a config waybar can read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsoncError {
    /// The text is larger than [`MAX_SIZE`].
    TooBig,
    /// A `/*` comment doesn't end.
    Comment {
        /// The line it starts on, counted from 1.
        line: usize,
    },
    /// The JSON is broken, with what `serde_json` says.
    Syntax {
        /// The line, counted from 1.
        line: usize,
        /// The column, counted from 1.
        column: usize,
        /// What is wrong, in `serde_json`'s words.
        reason: String,
    },
}

impl JsoncError {
    /// What is wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::TooBig => Text::new(msg!("The file is larger than 4 MiB")),
            Self::Comment { line } => {
                Text::new(msg!("A comment that starts on line {line} doesn't end"))
                    .arg("line", line)
            }
            Self::Syntax {
                line,
                column,
                reason,
            } => Text::new(msg!("Line {line}, column {column}: {reason}"))
                .arg("line", line)
                .arg("column", column)
                .text("reason", Text::data(reason.clone())),
        }
    }
}

impl fmt::Display for JsoncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for JsoncError {}

/// Reads `text` as waybar does: comments and trailing commas allowed, `\x` read as `\u00`.
///
/// # Errors
///
/// [`JsoncError`] for a text that is too big, a comment that doesn't end or broken JSON.
pub fn parse(text: &str) -> Result<Value, JsoncError> {
    if text.len() > MAX_SIZE {
        return Err(JsoncError::TooBig);
    }
    // jsoncpp skips a UTF-8 byte order mark at the start.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let plain = strip(text)?;
    serde_json::from_str(&plain).map_err(|error| JsoncError::Syntax {
        line: error.line(),
        column: error.column(),
        reason: error.to_string(),
    })
}

/// `text` without comments and commas before `]` or `}`, with `\x` as `\u00` inside strings.
/// Comments become spaces and keep their line breaks, so `serde_json`'s line numbers are those of
/// the file.
fn strip(text: &str) -> Result<String, JsoncError> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut line = 1;
    // Where the last comma outside a string was written to `out`, while only blanks and
    // comments followed it.
    let mut comma: Option<usize> = None;
    while let Some(char) = chars.next() {
        match char {
            '"' => {
                comma = None;
                out.push('"');
                while let Some(char) = chars.next() {
                    match char {
                        '\\' => match chars.next() {
                            Some('x') => out.push_str("\\u00"),
                            Some(next) => {
                                out.push('\\');
                                out.push(next);
                            }
                            None => out.push('\\'),
                        },
                        '"' => {
                            out.push('"');
                            break;
                        }
                        '\n' => {
                            line += 1;
                            out.push('\n');
                        }
                        other => out.push(other),
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for char in chars.by_ref() {
                    if char == '\n' {
                        line += 1;
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let start = line;
                let mut ended = false;
                let mut star = false;
                for char in chars.by_ref() {
                    if char == '\n' {
                        line += 1;
                        out.push('\n');
                    }
                    if star && char == '/' {
                        ended = true;
                        break;
                    }
                    star = char == '*';
                }
                if !ended {
                    return Err(JsoncError::Comment { line: start });
                }
                out.push(' ');
            }
            ',' => {
                comma = Some(out.len());
                out.push(',');
            }
            ']' | '}' => {
                if let Some(at) = comma.take() {
                    out.replace_range(at..=at, " ");
                }
                out.push(char);
            }
            '\n' => {
                line += 1;
                out.push('\n');
            }
            blank if blank.is_whitespace() => out.push(blank),
            other => {
                comma = None;
                out.push(other);
            }
        }
    }
    Ok(out)
}

/// `value` as JSON with two spaces of indentation, after `comments` as `//` lines (each line of
/// them one comment line), ending with a line break. Every string is escaped by `serde_json`, so
/// what waybar reads back is exactly `value`.
pub fn write(comments: &str, value: &Value) -> String {
    let mut text = String::new();
    for line in comments.lines() {
        if line.is_empty() {
            text.push_str("//\n");
        } else {
            text.push_str("// ");
            text.push_str(line);
            text.push('\n');
        }
    }
    // Serializing a `Value` can't fail: its keys are strings.
    text.push_str(&serde_json::to_string_pretty(value).unwrap_or_default());
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn comments_and_trailing_commas_are_read_as_waybar_reads_them() {
        let text = r#"// -*- mode: jsonc -*-
{
    // "layer": "top", // Waybar at top layer
    "height": 30, // Waybar height
    /* a block
       comment */ "url": "http://example.org/*not a comment*/", // still a string
    "modules-left": [
        "sway/workspaces",
        "sway/mode",
    ],
    "escaped": "\x41\"//",
}
"#;
        let value = parse(text).unwrap();
        assert_eq!(
            value,
            json!({
                "height": 30,
                "url": "http://example.org/*not a comment*/",
                "modules-left": ["sway/workspaces", "sway/mode"],
                "escaped": "A\"//",
            })
        );
    }

    #[test]
    fn a_byte_order_mark_is_skipped() {
        assert_eq!(parse("\u{feff}{\"a\": 1}").unwrap(), json!({"a": 1}));
    }

    #[test]
    fn a_key_given_twice_keeps_its_last_value() {
        assert_eq!(parse(r#"{"a": 1, "a": 2}"#).unwrap(), json!({"a": 2}));
    }

    #[test]
    fn errors_name_the_line() {
        assert_eq!(
            parse("{\n\n/* open").unwrap_err(),
            JsoncError::Comment { line: 3 }
        );
        let error = parse("{\n  // fine\n  \"a\": x\n}").unwrap_err();
        assert!(
            matches!(error, JsoncError::Syntax { line: 3, .. }),
            "{error:?}"
        );
        assert!(parse(&" ".repeat(MAX_SIZE + 1)).is_err());
    }

    #[test]
    fn written_json_reads_back_unchanged() {
        let value = json!({
            "format": "<span color='#fff'>{}</span> \"quoted\" \\ // /* */",
            "list": [1, 2.5, true, null],
        });
        let text = write("Generated.\n\nDO NOT EDIT.", &value);
        assert!(text.starts_with("// Generated.\n//\n// DO NOT EDIT.\n{"));
        assert_eq!(parse(&text).unwrap(), value);
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_text_never_panics(text in ".{0,200}") {
            let _ = parse(&text);
        }
    }
}
