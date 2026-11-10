// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One line of a sway config file, the way sway reads it: `split_args`, `strip_quotes` and
//! `unescape_string` (`common/stringop.c`), `do_var_replacement` (`sway/config.c`) and the steps of
//! `config_command` (`sway/commands.c`). These are unchanged from sway 1.10 to 1.12.
//!
//! The functions work on bytes like the C code, so every quirk carries over; the tests compare
//! them with the independent port the command builders are tested against.

const WHITESPACE: &[u8] = b" \x0c\n\r\t\x0b";

/// Commands whose arguments `config_command` doesn't unquote.
const KEEPS_QUOTES: &[&[u8]] = &[
    b"exec",
    b"exec_always",
    b"mode",
    b"bindsym",
    b"bindcode",
    b"bindswitch",
    b"bindgesture",
    b"set",
    b"for_window",
];

/// A `$variable` set in the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Variable {
    /// The name with its `$`.
    pub(super) name: Vec<u8>,
    pub(super) value: Vec<u8>,
}

pub(super) fn is_space(byte: u8) -> bool {
    WHITESPACE.contains(&byte)
}

fn at(bytes: &[u8], index: usize) -> u8 {
    bytes.get(index).copied().unwrap_or(0)
}

/// C strings end at the first NUL.
fn c_str(bytes: &[u8]) -> &[u8] {
    bytes
        .iter()
        .position(|&byte| byte == 0)
        .map_or(bytes, |end| &bytes[..end])
}

/// Removes blanks at both ends.
pub(super) fn strip_whitespace(input: &[u8]) -> &[u8] {
    let start = input.iter().take_while(|&&byte| is_space(byte)).count();
    let end = input.len()
        - input
            .iter()
            .rev()
            .take_while(|&&byte| is_space(byte))
            .count();
    &input[start..end.max(start)]
}

/// Splits a line into arguments at blanks outside quotes and brackets. Quotes stay.
pub(super) fn split_args(input: &[u8]) -> Vec<Vec<u8>> {
    let input = c_str(input);
    let mut argv = Vec::new();
    let (mut in_token, mut in_string, mut in_char, mut in_brackets, mut escaped) =
        (false, false, false, false, false);
    let mut start = 0;
    let mut end = 0;
    while at(input, start) != 0 {
        if !in_token {
            while is_space(at(input, end)) {
                end += 1;
            }
            start = end;
            in_token = true;
        }
        let byte = at(input, end);
        let add_token = if byte == b'"' && !in_char && !escaped {
            in_string = !in_string;
            false
        } else if byte == b'\'' && !in_string && !escaped {
            in_char = !in_char;
            false
        } else if byte == b'[' && !in_string && !in_char && !in_brackets && !escaped {
            in_brackets = true;
            false
        } else if byte == b']' && !in_string && !in_char && in_brackets && !escaped {
            in_brackets = false;
            false
        } else if byte == b'\\' {
            escaped = !escaped;
            false
        } else {
            byte == 0 || (!in_string && !in_char && !in_brackets && !escaped && is_space(byte))
        };
        if add_token {
            if end > start {
                argv.push(input[start..end].to_vec());
            }
            in_token = false;
            escaped = false;
            continue;
        }
        if byte != b'\\' {
            escaped = false;
        }
        end += 1;
    }
    argv
}

/// Removes the quotes that delimit strings.
pub(super) fn strip_quotes(input: &[u8]) -> Vec<u8> {
    let (mut in_str, mut in_chr, mut escaped) = (false, false, false);
    let mut out = Vec::with_capacity(input.len());
    for &byte in c_str(input) {
        if byte == b'\'' && !in_str && !escaped {
            in_chr = !in_chr;
        } else if byte == b'"' && !in_chr && !escaped {
            in_str = !in_str;
        } else if byte == b'\\' {
            escaped = !escaped;
            out.push(byte);
        } else {
            escaped = false;
            out.push(byte);
        }
    }
    out
}

/// Replaces C-style escapes (`\\n`, `\\"`, `\\x41`, …). `None` for a backslash at the very end, where
/// sway reads past the string.
pub(super) fn unescape_string(input: &[u8]) -> Option<Vec<u8>> {
    let mut text = c_str(input).to_vec();
    let len = text.len();
    let mut index = 0;
    while index < len && text[index] != 0 {
        if text[index] == b'\\' {
            index += 1;
            let replacement = match at(&text, index) {
                0 if index >= len => return None,
                // sway returns here without shifting out the NULs written so far, so the string
                // ends at the first one.
                b'0' => return Some(c_str(&text[..index - 1]).to_vec()),
                b'a' => Some(0x07),
                b'b' => Some(0x08),
                b'f' => Some(0x0c),
                b'n' => Some(b'\n'),
                b'r' => Some(b'\r'),
                b't' => Some(b'\t'),
                b'v' => Some(0x0b),
                b'\\' => Some(b'\\'),
                b'\'' => Some(b'\''),
                b'"' => Some(b'"'),
                b'?' => Some(b'?'),
                b'x' => {
                    let mut value = 0u8;
                    if at(&text, index + 1).is_ascii_digit() {
                        value = at(&text, index + 1) - b'0';
                        if at(&text, index + 2).is_ascii_digit() {
                            value = value
                                .wrapping_mul(0x10)
                                .wrapping_add(at(&text, index + 2) - b'0');
                            text[index + 2] = 0;
                        }
                        text[index + 1] = 0;
                    }
                    Some(value)
                }
                _ => None,
            };
            if let Some(replacement) = replacement {
                text[index - 1] = replacement;
                text[index] = 0;
            }
        }
        index += 1;
    }
    Some(text.into_iter().filter(|&byte| byte != 0).collect())
}

/// Replaces the variables in `input`. `variables` are tried in order, so the longest names come
/// first. `$$` becomes `$`, a `$` after a single backslash stays, and a name that isn't set stays
/// as written.
pub(super) fn replace_variables(input: &[u8], variables: &[Variable]) -> Vec<u8> {
    let text = c_str(input);
    let mut out = Vec::with_capacity(text.len());
    let mut index = 0;
    while let Some(&byte) = text.get(index) {
        if byte != b'$' {
            out.push(byte);
            index += 1;
            continue;
        }
        // What sway sees before the `$` is what has been written so far, replaced values
        // included.
        let escaped = match out.as_slice() {
            [.., b'\\', b'\\'] => false,
            [.., b'\\'] => true,
            _ => false,
        };
        if escaped {
            out.push(b'$');
            index += 1;
        } else if at(text, index + 1) == b'$' {
            out.push(b'$');
            index += 2;
        } else if let Some(variable) = variables
            .iter()
            .find(|variable| !variable.name.is_empty() && text[index..].starts_with(&variable.name))
        {
            // The value isn't looked at again, so a variable can't expand forever.
            out.extend_from_slice(&variable.value);
            index += variable.name.len();
        } else {
            out.push(b'$');
            index += 1;
        }
    }
    out
}

/// The arguments of a config line as sway's command handlers see them: variables replaced, quotes
/// removed (except for commands that keep them) and escapes resolved. `line` has no surrounding
/// blanks and isn't a block start or end.
///
/// The first argument is the command. For `set`, the name of the variable is the second argument,
/// with its `$`, and is never replaced.
pub(super) fn arguments(line: &[u8], variables: &[Variable]) -> Vec<Vec<u8>> {
    let mut argv = split_args(line);
    // The command itself can be stored in a variable.
    if argv.first().is_some_and(|name| name.first() == Some(&b'$')) {
        argv[0] = replace_variables(&argv[0], variables);
        argv = split_args(&argv.join(&b' '));
    }
    // sway finds its command handlers without regard to case.
    let is = |name: &[u8], command: &[u8]| name.eq_ignore_ascii_case(command);
    let is_set = argv.first().is_some_and(|name| is(name, b"set"));
    if is_set {
        // sway doubles the `$` so that the name isn't replaced by a variable with a shorter name.
        if let Some(name) = argv.get_mut(1).filter(|name| name.first() == Some(&b'$')) {
            name.insert(0, b'$');
        }
    }
    let joined = argv.join(&b' ');
    let mut argv = split_args(&replace_variables(&joined, variables));
    let keeps_quotes = argv
        .first()
        .is_some_and(|name| KEEPS_QUOTES.iter().any(|command| is(name, command)));
    for arg in argv.iter_mut().skip(if is_set { 2 } else { 1 }) {
        if !keeps_quotes && matches!(arg.first(), Some(b'"' | b'\'')) {
            *arg = strip_quotes(arg);
        }
        // Where sway would read past the end, the argument stays as it is.
        if let Some(unescaped) = unescape_string(arg) {
            *arg = unescaped;
        }
    }
    argv
}
