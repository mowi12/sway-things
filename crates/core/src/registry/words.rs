// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Splitting a command into words like a POSIX shell, without expanding anything: the check of
//! the editor command's values, and how [`crate::editor`] runs it.

use std::fmt;

use crate::i18n::Text;
use crate::msg;

/// Why a command can't be split into words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitError {
    /// A single or double quote is opened and never closed.
    UnclosedQuote,
    /// The command ends in a backslash, which escapes nothing.
    TrailingBackslash,
}

impl SplitError {
    /// What is wrong with the command, to show after its name: "The editor command has a quote
    /// that isn't closed".
    pub fn text(&self) -> Text {
        match self {
            Self::UnclosedQuote => msg!("has a quote that isn't closed").into(),
            Self::TrailingBackslash => msg!("ends in a backslash that escapes nothing").into(),
        }
    }
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for SplitError {}

/// Splits `command` into words like a POSIX shell, without expanding anything: whitespace
/// separates words, single quotes keep everything up to the next one, double quotes keep
/// everything but `\"`, `\\`, `\$` and `` \` ``, and a backslash outside quotes keeps the
/// character after it. `$`, `~`, `*`, `;` and `|` are plain characters.
///
/// # Errors
///
/// [`SplitError`] for a quote that isn't closed or a backslash at the end.
pub fn split(command: &str) -> Result<Vec<String>, SplitError> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                let word = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err(SplitError::UnclosedQuote),
                    }
                }
            }
            '"' => {
                let word = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => word.push(c),
                            Some('\n') => {}
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err(SplitError::UnclosedQuote),
                        },
                        Some(c) => word.push(c),
                        None => return Err(SplitError::UnclosedQuote),
                    }
                }
            }
            '\\' => match chars.next() {
                Some('\n') => {}
                Some(c) => word.get_or_insert_with(String::new).push(c),
                None => return Err(SplitError::TrailingBackslash),
            },
            c if c.is_whitespace() => {
                if let Some(word) = word.take() {
                    words.push(word);
                }
            }
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    words.extend(word);
    Ok(words)
}
