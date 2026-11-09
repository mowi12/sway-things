// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

/// External text that may become an argument of a sway command: device identifiers, output, seat
/// and workspace names, font descriptions, XKB values.
///
/// Every `SwayText` can be rendered for every [`Target`](super::Target). Text that can't be is
/// rejected here:
///
/// - empty text,
/// - control characters, which would end a config line or change how sway splits it,
/// - backslashes, because sway's IPC parser keeps them and treats them as escapes at the same time,
///   so a backslash next to a quote can't be represented reliably.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SwayText(String);

impl SwayText {
    /// Checks `text` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns a [`TextError`] if `text` is empty or contains a control character or a backslash.
    pub fn new(text: impl Into<String>) -> Result<Self, TextError> {
        let text = text.into();
        if text.is_empty() {
            return Err(TextError::Empty);
        }
        for (index, character) in text.char_indices() {
            if character.is_control() {
                return Err(TextError::ControlCharacter { index, character });
            }
            if character == '\\' {
                return Err(TextError::Backslash { index });
            }
        }
        Ok(Self(text))
    }

    /// The text as given.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Renders the text as one argument of an IPC command.
    ///
    /// sway strips quotes from an argument that starts with one, and replaces `$variables`
    /// afterwards. The text is split into runs quoted with `"` or `'`, so a quote character always
    /// sits inside the other kind of quotes, and every `$` is doubled.
    pub(super) fn render_ipc(&self) -> String {
        let quote_for = |character: char| match character {
            '"' => Some('\''),
            '\'' => Some('"'),
            _ => None,
        };
        let mut open = self.0.chars().find_map(quote_for).unwrap_or('"');
        let mut out = String::with_capacity(self.0.len() + 2);
        out.push(open);
        for character in self.0.chars() {
            if let Some(quote) = quote_for(character) {
                if quote != open {
                    out.push(open);
                    out.push(quote);
                    open = quote;
                }
            }
            push_escaped_dollar(&mut out, character);
        }
        out.push(open);
        out
    }

    /// Renders the text for an IPC command inside another IPC command, such as the command of
    /// `for_window`: sway unquotes the outer command's arguments, joins them and parses the inner
    /// command again later. The IPC rendering is rendered once more; it contains neither control
    /// characters nor backslashes, so it is valid text itself.
    pub(super) fn render_ipc_nested(&self) -> String {
        Self(self.render_ipc()).render_ipc()
    }

    /// Renders the text as one argument of a config line.
    ///
    /// sway replaces `$variables` in the whole line, then strips the quotes and unescapes `\"`. The
    /// text is quoted with `"`, inner `"` become `\"` and every `$` is doubled.
    pub(super) fn render_config(&self) -> String {
        let mut out = String::with_capacity(self.0.len() + 2);
        out.push('"');
        for character in self.0.chars() {
            if character == '"' {
                out.push('\\');
            }
            push_escaped_dollar(&mut out, character);
        }
        out.push('"');
        out
    }
}

fn push_escaped_dollar(out: &mut String, character: char) {
    if character == '$' {
        out.push('$');
    }
    out.push(character);
}

impl fmt::Display for SwayText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<&str> for SwayText {
    type Error = TextError;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        Self::new(text)
    }
}

impl TryFrom<String> for SwayText {
    type Error = TextError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(text)
    }
}

/// Why a string can't be a [`SwayText`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextError {
    /// The text is empty.
    Empty,
    /// The text contains a control character (newline, tab, NUL, …).
    ControlCharacter {
        /// Byte offset of the character.
        index: usize,
        /// The character.
        character: char,
    },
    /// The text contains a backslash.
    Backslash {
        /// Byte offset of the backslash.
        index: usize,
    },
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("text is empty"),
            Self::ControlCharacter { index, character } => {
                write!(f, "control character {character:?} at byte {index}")
            }
            Self::Backslash { index } => write!(f, "backslash at byte {index}"),
        }
    }
}

impl std::error::Error for TextError {}
