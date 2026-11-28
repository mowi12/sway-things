// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! environment.d's syntax (environment.d(5)), as systemd's environment.d generator reads it.
//!
//! A file is lines of `NAME=value`; `#` or `;` at the start of a line makes it a comment. The
//! value loses its quotes and backslashes first: quotes count only at its start (`"a" "b"` is
//! `ab`), a backslash keeps the next character (in double quotes only `"`, `\`, `` ` `` and
//! `$`), and a line ending in `\` goes on in the next. Then its variables are expanded: `$NAME`,
//! `${NAME}`, `${NAME:-default}`, `${NAME:+alternate}`, with `$$` for a `$`. A line with an
//! invalid name or an empty value is ignored.
//!
//! [`parse`] reads a file as the generator does, [`check_value`] is stricter for a value typed
//! in Sway Things: one line, quotes closed, every `${` closed and well formed. [`expand`]
//! expands a value.

use super::is_variable_name;
use crate::i18n::{Msg, Text};
use crate::msg;

/// A variable a file sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    /// Its name.
    pub name: String,
    /// Its value without quotes and backslashes, not yet expanded.
    pub value: String,
    /// The line its name is on, from 1.
    pub line: usize,
}

/// How a text ends, read as environment.d.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// After a complete line.
    Complete,
    /// Inside quotes.
    Quote,
    /// After a backslash.
    Backslash,
}

/// What [`parse`] read from a file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parsed {
    /// The variables it sets, in their order.
    pub assignments: Vec<Assignment>,
    /// How it ends: a file that ends inside quotes still sets what it read.
    pub end: Option<End>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    PreKey,
    Key,
    PreValue,
    Value,
    ValueEscape,
    Single,
    Double,
    DoubleEscape,
    Comment,
}

/// The characters a backslash keeps as they are in double quotes; before any other it is kept.
const DOUBLE_QUOTED_ESCAPES: [char; 4] = ['"', '\\', '`', '$'];

fn is_newline(char: char) -> bool {
    matches!(char, '\n' | '\r')
}

fn is_blank(char: char) -> bool {
    matches!(char, ' ' | '\t')
}

/// Reads a file a character at a time, as systemd's generator does.
struct Reader {
    parsed: Parsed,
    state: State,
    key: String,
    value: String,
    /// Where trailing blanks of an unquoted value start, to be cut off at the end of the line.
    blanks: Option<usize>,
    line: usize,
    key_line: usize,
}

impl Reader {
    fn new() -> Self {
        Self {
            parsed: Parsed::default(),
            state: State::PreKey,
            key: String::new(),
            value: String::new(),
            blanks: None,
            line: 1,
            key_line: 1,
        }
    }

    /// Ends the line: the variable is set if its name is valid and its value isn't empty.
    fn push(&mut self) {
        if self.state == State::Value {
            if let Some(at) = self.blanks.take() {
                self.value.truncate(at);
            }
        }
        let name = self.key.trim_end();
        let value = std::mem::take(&mut self.value);
        if is_variable_name(name)
            && !value.is_empty()
            && !value
                .chars()
                .any(|char| char.is_control() && char != '\n' && char != '\t')
        {
            self.parsed.assignments.push(Assignment {
                name: name.to_owned(),
                value,
                line: self.key_line,
            });
        }
        self.key.clear();
    }

    fn step(&mut self, char: char) {
        match self.state {
            State::PreKey => {
                if char == '#' || char == ';' {
                    self.state = State::Comment;
                } else if !is_blank(char) && !is_newline(char) {
                    self.state = State::Key;
                    self.key_line = self.line;
                    self.key.push(char);
                }
            }
            State::Key => {
                if is_newline(char) {
                    self.state = State::PreKey;
                    self.key.clear();
                } else if char == '=' {
                    self.state = State::PreValue;
                } else {
                    self.key.push(char);
                }
            }
            State::PreValue => match char {
                _ if is_newline(char) => {
                    self.push();
                    self.state = State::PreKey;
                }
                '\'' => self.state = State::Single,
                '"' => self.state = State::Double,
                '\\' => self.state = State::ValueEscape,
                _ if is_blank(char) => {}
                _ => {
                    self.state = State::Value;
                    self.blanks = None;
                    self.value.push(char);
                }
            },
            State::Value => self.unquoted(char),
            State::ValueEscape => {
                self.state = State::Value;
                if !is_newline(char) {
                    self.value.push(char);
                }
            }
            State::Single => {
                if char == '\'' {
                    self.state = State::PreValue;
                } else {
                    self.value.push(char);
                }
            }
            State::Double => match char {
                '"' => self.state = State::PreValue,
                '\\' => self.state = State::DoubleEscape,
                _ => self.value.push(char),
            },
            State::DoubleEscape => {
                self.state = State::Double;
                if DOUBLE_QUOTED_ESCAPES.contains(&char) {
                    self.value.push(char);
                } else if !is_newline(char) {
                    self.value.push('\\');
                    self.value.push(char);
                }
            }
            State::Comment => {
                if is_newline(char) {
                    self.state = State::PreKey;
                }
            }
        }
        if char == '\n' {
            self.line += 1;
        }
    }

    /// A character of an unquoted value.
    fn unquoted(&mut self, char: char) {
        if is_newline(char) {
            self.push();
            self.state = State::PreKey;
        } else if char == '\\' {
            self.state = State::ValueEscape;
            self.blanks = None;
        } else {
            if !is_blank(char) {
                self.blanks = None;
            } else if self.blanks.is_none() {
                self.blanks = Some(self.value.len());
            }
            self.value.push(char);
        }
    }

    fn finish(mut self) -> Parsed {
        let end = match self.state {
            State::PreKey | State::Key | State::Comment | State::PreValue | State::Value => {
                End::Complete
            }
            State::Single | State::Double | State::DoubleEscape => End::Quote,
            State::ValueEscape => End::Backslash,
        };
        if !matches!(self.state, State::PreKey | State::Key | State::Comment) {
            self.push();
        }
        self.parsed.end = Some(end);
        self.parsed
    }
}

/// The text of an environment.d file, read line by line as systemd's generator reads it: the
/// variables with a valid name and a value that isn't empty, in their order.
pub fn parse(text: &str) -> Parsed {
    let mut reader = Reader::new();
    for char in text.chars() {
        reader.step(char);
    }
    reader.finish()
}

/// The value written as `value` in an environment.d line, without quotes and backslashes, not
/// yet expanded.
pub fn unquote(value: &str) -> String {
    parse(&format!("N={value}"))
        .assignments
        .into_iter()
        .next()
        .map(|assignment| assignment.value)
        .unwrap_or_default()
}

/// Why a value typed in Sway Things can't be written into the environment.d file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxError {
    /// It has a line break or another control character.
    Control,
    /// A quote isn't closed.
    Quote,
    /// It ends in a backslash, which would join the next line.
    Backslash,
    /// It is empty, which environment.d ignores.
    Empty,
    /// A `${` isn't closed.
    Brace,
    /// A `${…}` isn't `${NAME}`, `${NAME:-default}` or `${NAME:+alternate}`.
    Expansion,
}

impl SyntaxError {
    /// What is wrong, to show.
    pub fn text(self) -> Text {
        let text: Msg = match self {
            Self::Control => msg!("a value can't have line breaks or control characters"),
            Self::Quote => msg!("a quote isn't closed"),
            Self::Backslash => msg!("a value can't end in a backslash"),
            Self::Empty => msg!("the value is empty, which environment.d ignores"),
            Self::Brace => msg!("a ${ isn't closed with }"),
            Self::Expansion => {
                msg!("write ${NAME}, ${NAME:-default} or ${NAME:+alternate}; $$ for a $")
            }
        };
        text.into()
    }
}

/// Checks a value typed in Sway Things, to be written into the environment.d file as it is:
/// one line, quotes closed, no backslash at the end, not empty, and every `${…}` well formed.
///
/// # Errors
///
/// What is wrong with it.
pub fn check_value(value: &str) -> Result<(), SyntaxError> {
    if value.chars().any(char::is_control) {
        return Err(SyntaxError::Control);
    }
    let parsed = parse(&format!("N={value}"));
    match parsed.end {
        Some(End::Quote) => return Err(SyntaxError::Quote),
        Some(End::Backslash) => return Err(SyntaxError::Backslash),
        Some(End::Complete) | None => {}
    }
    let Some(assignment) = parsed.assignments.first() else {
        return Err(SyntaxError::Empty);
    };
    check_expansions(&assignment.value, 0)
}

/// How deep `${…}` may nest in a default or an alternate: deeper ones are kept as they are.
const MAX_DEPTH: usize = 16;

/// A `${…}` or `$NAME` at the start of a text.
enum Reference<'a> {
    /// `$$`: a `$`.
    Dollar,
    /// `$NAME` or `${NAME}`.
    Plain(&'a str),
    /// `${NAME:-default}`.
    Default(&'a str, &'a str),
    /// `${NAME:+alternate}`.
    Alternate(&'a str, &'a str),
    /// `${…}` of another form.
    Other(&'a str),
    /// `${` without its `}`.
    Open,
    /// A `$` that starts nothing.
    Lone,
}

/// The reference at the start of `text`, which starts with `$`, and how long it is.
fn reference(text: &str) -> (Reference<'_>, usize) {
    let rest = &text[1..];
    if rest.starts_with('$') {
        return (Reference::Dollar, 2);
    }
    if let Some(inner) = rest.strip_prefix('{') {
        let mut depth = 0_usize;
        for (at, char) in inner.char_indices() {
            match char {
                '{' => depth += 1,
                '}' if depth == 0 => {
                    let body = &inner[..at];
                    let length = at + 3;
                    let split = body.find(':').map(|colon| body.split_at(colon));
                    let shown = match split {
                        Some((name, operator)) if operator.starts_with(":-") => {
                            Reference::Default(name, &operator[2..])
                        }
                        Some((name, operator)) if operator.starts_with(":+") => {
                            Reference::Alternate(name, &operator[2..])
                        }
                        None if is_variable_name(body) => Reference::Plain(body),
                        _ => Reference::Other(body),
                    };
                    return (shown, length);
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        return (Reference::Open, text.len());
    }
    let length = rest
        .find(|char: char| !(char.is_ascii_alphanumeric() || char == '_'))
        .unwrap_or(rest.len());
    if length == 0 {
        (Reference::Lone, 1)
    } else {
        (Reference::Plain(&rest[..length]), length + 1)
    }
}

fn check_expansions(value: &str, depth: usize) -> Result<(), SyntaxError> {
    let mut rest = value;
    while let Some(at) = rest.find('$') {
        let (found, length) = reference(&rest[at..]);
        match found {
            Reference::Open => return Err(SyntaxError::Brace),
            Reference::Other(_) => return Err(SyntaxError::Expansion),
            Reference::Default(name, inner) | Reference::Alternate(name, inner) => {
                if !is_variable_name(name) {
                    return Err(SyntaxError::Expansion);
                }
                if depth < MAX_DEPTH {
                    check_expansions(inner, depth + 1)?;
                }
            }
            Reference::Plain(name) if !is_variable_name(name) => {
                return Err(SyntaxError::Expansion);
            }
            Reference::Dollar | Reference::Plain(_) | Reference::Lone => {}
        }
        rest = &rest[at + length..];
    }
    Ok(())
}

/// `value` (without quotes, as [`parse`] gives it) with its variables expanded, each looked up
/// with `lookup`; an unknown variable is empty. Like systemd: `${NAME:-default}` is the default
/// where the variable is unset or empty, `${NAME:+alternate}` the alternate where it isn't, `$$`
/// a `$`, and a `${` without its `}` stays as it is.
pub fn expand(value: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    expand_at(value, lookup, 0)
}

fn expand_at(value: &str, lookup: &dyn Fn(&str) -> Option<String>, depth: usize) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut rest = value;
    let set = |name: &str| lookup(name).filter(|value| !value.is_empty());
    while let Some(at) = rest.find('$') {
        expanded.push_str(&rest[..at]);
        let (found, length) = reference(&rest[at..]);
        match found {
            Reference::Dollar | Reference::Lone => expanded.push('$'),
            Reference::Plain(name) | Reference::Other(name) => {
                expanded.push_str(&lookup(name).unwrap_or_default());
            }
            Reference::Default(name, default) => match set(name) {
                Some(value) => expanded.push_str(&value),
                None if depth < MAX_DEPTH => {
                    expanded.push_str(&expand_at(default, lookup, depth + 1));
                }
                None => expanded.push_str(default),
            },
            Reference::Alternate(name, alternate) => {
                if set(name).is_some() {
                    if depth < MAX_DEPTH {
                        expanded.push_str(&expand_at(alternate, lookup, depth + 1));
                    } else {
                        expanded.push_str(alternate);
                    }
                }
            }
            Reference::Open => expanded.push_str(&rest[at..]),
        }
        rest = &rest[at + length..];
    }
    expanded.push_str(rest);
    expanded
}

/// Whether `value` (without quotes) refers to the variable `name`, so a line setting `name` to
/// it builds on the value before instead of replacing it: `PATH=$HOME/bin:$PATH`.
pub fn refers_to(value: &str, name: &str) -> bool {
    refers_at(value, name, 0)
}

fn refers_at(value: &str, name: &str, depth: usize) -> bool {
    let mut rest = value;
    while let Some(at) = rest.find('$') {
        let (found, length) = reference(&rest[at..]);
        let refers = match found {
            Reference::Plain(found) => found == name,
            Reference::Default(found, inner) | Reference::Alternate(found, inner) => {
                found == name || (depth < MAX_DEPTH && refers_at(inner, name, depth + 1))
            }
            Reference::Dollar | Reference::Other(_) | Reference::Open | Reference::Lone => false,
        };
        if refers {
            return true;
        }
        rest = &rest[at + length..];
    }
    false
}
