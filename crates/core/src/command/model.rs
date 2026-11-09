// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A port of the parts of sway that parse commands, for tests: `split_args`, `strip_quotes`,
//! `argsep` and `unescape_string` (`common/stringop.c`), `do_var_replacement` (`sway/config.c`),
//! `criteria_parse` (`sway/criteria.c`), and the pipelines of `execute_command` (IPC) and
//! `config_command` / `read_config` (config files) in `sway/commands.c` and `sway/config.c`. The
//! functions are unchanged from sway 1.10 to 1.12.
//!
//! It works on bytes like the C code. Where sway would look up a `$variable`, the model fails
//! instead, because an argument must never depend on the user's variables.

pub(super) type Bytes = Vec<u8>;

/// A command as sway's handler sees it.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Parsed {
    pub criteria: Option<Vec<(Bytes, Bytes)>>,
    pub argv: Vec<Bytes>,
}

const WHITESPACE: &[u8] = b" \x0c\n\r\t\x0b";

/// Commands that `execute_command` doesn't unquote.
const IPC_KEEPS_QUOTES: &[&[u8]] = &[b"exec", b"exec_always", b"mode"];

/// Commands that `config_command` doesn't unquote.
const CONFIG_KEEPS_QUOTES: &[&[u8]] = &[
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

fn is_space(byte: u8) -> bool {
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

pub(super) fn split_args(input: &[u8]) -> Vec<Bytes> {
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

pub(super) fn strip_quotes(input: &[u8]) -> Bytes {
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

/// One step of `argsep`: the piece from `from` up to the next unquoted delimiter, the index after
/// the delimiter (or `None` at the end), and the matched delimiter (0 at the end).
fn argsep(input: &[u8], from: usize, delims: &[u8]) -> (Bytes, Option<usize>, u8) {
    let (mut in_string, mut in_char, mut escaped) = (false, false, false);
    let mut start = from;
    let mut end = from;
    loop {
        let interesting = (end..input.len()).find(|&index| {
            let byte = input[index];
            matches!(byte, b'"' | b'\'' | b'\\') || delims.contains(&byte)
        });
        let Some(interesting) = interesting else {
            return (input[start..].to_vec(), None, 0);
        };
        if escaped && interesting != end {
            escaped = false;
        }
        let byte = input[interesting];
        if byte == b'"' && !in_char && !escaped {
            in_string = !in_string;
            end = interesting + 1;
        } else if byte == b'\'' && !in_string && !escaped {
            in_char = !in_char;
            end = interesting + 1;
        } else if byte == b'\\' {
            escaped = !escaped;
            end = interesting + 1;
        } else if !in_string && !in_char && !escaped {
            end = interesting;
            if end > start {
                return (input[start..end].to_vec(), Some(end + 1), byte);
            }
            start += 1;
            end = start;
        } else {
            end += 1;
        }
    }
}

/// `do_var_replacement` without variables: fails where sway would look one up.
pub(super) fn var_replace(input: &[u8]) -> Result<Bytes, String> {
    let mut text = c_str(input).to_vec();
    let mut from = 0;
    while let Some(offset) = text[from..].iter().position(|&byte| byte == b'$') {
        let find = from + offset;
        if find > 0 && text[find - 1] == b'\\' && (find == 1 || text[find - 2] != b'\\') {
            from = find + 1;
            continue;
        }
        if at(&text, find + 1) == b'$' {
            text.remove(find);
            from = find + 1;
            continue;
        }
        return Err(format!(
            "variable lookup at byte {find} of {:?}",
            String::from_utf8_lossy(&text)
        ));
    }
    Ok(text)
}

pub(super) fn unescape_string(input: &[u8]) -> Result<Bytes, String> {
    let mut text = c_str(input).to_vec();
    let len = text.len();
    let mut index = 0;
    while index < len && text[index] != 0 {
        if text[index] == b'\\' {
            index += 1;
            let replacement = match at(&text, index) {
                0 if index >= len => return Err("backslash at the end".to_owned()),
                // sway returns here without shifting out the NULs written so far, so the string
                // ends at the first one.
                b'0' => return Ok(c_str(&text[..index - 1]).to_vec()),
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
    Ok(text.into_iter().filter(|&byte| byte != 0).collect())
}

/// `criteria_parse`: the token/value pairs (values after sway's unescaping; empty for `all`,
/// `floating` and `tiling` without a value) and the length of the raw criteria. Knows the tokens of
/// sway 1.10, the oldest supported version.
pub(super) fn criteria_parse(input: &[u8]) -> Result<(Vec<(Bytes, Bytes)>, usize), String> {
    const TOKENS: &[&[u8]] = &[
        b"all",
        b"app_id",
        b"class",
        b"con_id",
        b"con_mark",
        b"floating",
        b"id",
        b"instance",
        b"pid",
        b"shell",
        b"tiling",
        b"title",
        b"urgent",
        b"window_role",
        b"window_type",
        b"workspace",
    ];
    let input = c_str(input);
    let skip_spaces = |head: &mut usize| {
        while at(input, *head) == b' ' {
            *head += 1;
        }
    };
    let mut head = 0;
    skip_spaces(&mut head);
    if at(input, head) != b'[' {
        return Err("no criteria".to_owned());
    }
    head += 1;
    let mut pairs = Vec::new();
    while at(input, head) != 0 && at(input, head) != b']' {
        skip_spaces(&mut head);
        let name_start = head;
        while at(input, head).is_ascii_lowercase() || at(input, head) == b'_' {
            head += 1;
        }
        let name = input[name_start..head].to_vec();
        skip_spaces(&mut head);
        let mut value = None;
        if at(input, head) == b'=' {
            head += 1;
            skip_spaces(&mut head);
            let in_quotes = at(input, head) == b'"';
            if in_quotes {
                head += 1;
            }
            let value_start = head;
            if in_quotes {
                while at(input, head) != 0 && (at(input, head) != b'"' || input[head - 1] == b'\\')
                {
                    head += 1;
                }
                if at(input, head) == 0 {
                    return Err("quote mismatch in criteria".to_owned());
                }
            } else {
                while !matches!(at(input, head), 0 | b' ' | b']') {
                    head += 1;
                }
            }
            let raw = &input[value_start..head];
            if in_quotes {
                head += 1;
            }
            let mut unescaped = Vec::with_capacity(raw.len());
            let mut index = 0;
            while index < raw.len() {
                if raw[index] == b'\\' && at(raw, index + 1) == b'"' {
                    index += 1;
                }
                unescaped.push(raw[index]);
                index += 1;
            }
            value = Some(unescaped);
        }
        if !TOKENS.contains(&name.as_slice()) {
            return Err(format!(
                "token {:?} is not recognized",
                String::from_utf8_lossy(&name)
            ));
        }
        let value = match value {
            Some(value) => value,
            None if matches!(name.as_slice(), b"all" | b"floating" | b"tiling") => Vec::new(),
            None => return Err("token without value".to_owned()),
        };
        pairs.push((name, value));
        skip_spaces(&mut head);
    }
    if at(input, head) != b']' {
        return Err("no closing brace found in criteria".to_owned());
    }
    if pairs.is_empty() {
        return Err("criteria is empty".to_owned());
    }
    Ok((pairs, head + 1))
}

/// `execute_command` up to the handler call. Returns every command of the payload.
pub(super) fn execute_command(payload: &[u8]) -> Result<Vec<Parsed>, String> {
    let payload = c_str(payload);
    let mut commands = Vec::new();
    let mut head = Some(0);
    let mut matched = b';';
    let mut criteria: Option<Vec<(Bytes, Bytes)>> = None;
    while let Some(mut position) = head {
        while is_space(at(payload, position)) {
            position += 1;
        }
        if matched == b';' {
            criteria = None;
            if at(payload, position) == b'[' {
                let (pairs, len) = criteria_parse(&payload[position..])?;
                criteria = Some(pairs);
                position += len;
                while is_space(at(payload, position)) {
                    position += 1;
                }
            }
        }
        let (piece, rest, delim) = argsep(payload, position.min(payload.len()), b";,");
        head = rest;
        matched = delim;
        let trimmed: &[u8] = &piece[piece.iter().take_while(|&&byte| is_space(byte)).count()..];
        if trimmed.is_empty() {
            continue;
        }
        let mut argv = split_args(trimmed);
        let Some(name) = argv.first().cloned() else {
            continue;
        };
        if !IPC_KEEPS_QUOTES.contains(&name.as_slice()) {
            for arg in argv.iter_mut().skip(1) {
                if matches!(arg.first(), Some(b'"' | b'\'')) {
                    *arg = strip_quotes(arg);
                }
            }
        }
        let first = if name == b"set" { 2 } else { 1 };
        for arg in argv.iter_mut().skip(first) {
            *arg = var_replace(arg)?;
        }
        commands.push(Parsed {
            criteria: criteria.clone(),
            argv,
        });
    }
    Ok(commands)
}

/// One line of a config file through `read_config` and `config_command`, up to the handler call.
pub(super) fn config_command(line: &[u8]) -> Result<Vec<Bytes>, String> {
    if line.contains(&b'\n') || line.contains(&0) {
        return Err("more than one line".to_owned());
    }
    if line.ends_with(b"\\") {
        return Err("line continuation".to_owned());
    }
    let start = line.iter().take_while(|&&byte| is_space(byte)).count();
    let end = line.len()
        - line
            .iter()
            .rev()
            .take_while(|&&byte| is_space(byte))
            .count();
    let line = &line[start..end.max(start)];
    match line.first() {
        None => return Err("empty line".to_owned()),
        Some(b'#') => return Err("comment".to_owned()),
        _ => {}
    }
    if matches!(line.last(), Some(b'{' | b'}')) {
        return Err("line ends with a brace".to_owned());
    }
    let argv = split_args(line);
    match argv.last().map(Vec::as_slice) {
        Some(b"{" | b"}") => return Err("block start or end".to_owned()),
        None => return Err("empty line".to_owned()),
        _ => {}
    }
    if argv.first().is_some_and(|name| name.first() == Some(&b'$')) {
        return Err("command name is a variable".to_owned());
    }
    if argv.first().is_some_and(|name| name == b"set") {
        return Err("set is not modeled".to_owned());
    }
    let joined = argv.join(&b' ');
    let mut argv = split_args(&var_replace(&joined)?);
    let keeps_quotes = argv
        .first()
        .is_some_and(|name| CONFIG_KEEPS_QUOTES.contains(&name.as_slice()));
    for arg in argv.iter_mut().skip(1) {
        if !keeps_quotes && matches!(arg.first(), Some(b'"' | b'\'')) {
            *arg = strip_quotes(arg);
        }
        *arg = unescape_string(arg)?;
    }
    Ok(argv)
}
