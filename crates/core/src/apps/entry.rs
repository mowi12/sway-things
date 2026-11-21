// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop entries (`.desktop` files) as the Desktop Entry Specification describes them: the
//! keys of the `[Desktop Entry]` group, strings with their escapes, localized strings, lists,
//! booleans, and the `Exec` command line with its quoting and field codes.
//!
//! Everything in a desktop entry is data written by someone else: names, icons and the command
//! line are never passed through a shell. The command line becomes a list of arguments
//! ([`DesktopEntry::command`]) that is started with an argument array.

use std::path::Path;

/// The largest desktop entry that is read; real ones are a few kilobytes.
pub const MAX_SIZE: u64 = 256 * 1024;

/// The keys of the `[Desktop Entry]` group of a desktop entry, as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopEntry {
    /// Each key (with its locale, like `Name[de]`) and its raw value, in the order of the file.
    keys: Vec<(String, String)>,
}

impl DesktopEntry {
    /// Reads the `[Desktop Entry]` group of `text`. Comments, blank lines, other groups and lines
    /// that aren't `key=value` are skipped. A key given twice keeps its last value, as `GLib` does.
    pub fn parse(text: &str) -> Self {
        let mut keys: Vec<(String, String)> = Vec::new();
        let mut in_group = false;
        for line in text.lines() {
            let line = line.trim_start();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                in_group = line.trim_end() == "[Desktop Entry]";
                continue;
            }
            if !in_group {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim_end().to_owned();
            let value = value.trim_start().to_owned();
            if let Some(slot) = keys.iter_mut().find(|(known, _)| *known == key) {
                slot.1 = value;
            } else {
                keys.push((key, value));
            }
        }
        Self { keys }
    }

    /// Reads the desktop entry at `path`: only a regular file of at most [`MAX_SIZE`] bytes in
    /// UTF-8 (invalid bytes are replaced).
    pub fn read(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > MAX_SIZE {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        Some(Self::parse(&String::from_utf8_lossy(&bytes)))
    }

    /// The raw value of `key`, without unescaping.
    pub fn raw(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, value)| value.as_str())
    }

    /// The string value of `key`, with the escapes `\s`, `\n`, `\t`, `\r` and `\\` resolved.
    pub fn string(&self, key: &str) -> Option<String> {
        self.raw(key).map(unescape)
    }

    /// The localized string value of `key` for the first of `languages` (`de_DE.UTF-8`,
    /// `sr@latin`) that has one, as the specification matches locales: `lang_COUNTRY@MODIFIER`,
    /// `lang_COUNTRY`, `lang@MODIFIER`, `lang`, then the key without a locale.
    pub fn localized(&self, key: &str, languages: &[String]) -> Option<String> {
        for language in languages {
            for variant in locale_variants(language) {
                if let Some(value) = self.string(&format!("{key}[{variant}]")) {
                    if !value.is_empty() {
                        return Some(value);
                    }
                }
            }
        }
        self.string(key)
    }

    /// The boolean value of `key`: `true` (or `1`, as `GLib` reads it) is true, everything else
    /// and a missing key false.
    pub fn boolean(&self, key: &str) -> bool {
        matches!(self.raw(key).map(str::trim), Some("true" | "1"))
    }

    /// Whether `key` is set to `false` (or `0`): unlike [`DesktopEntry::boolean`], a missing key
    /// isn't false.
    pub fn is_false(&self, key: &str) -> bool {
        matches!(self.raw(key).map(str::trim), Some("false" | "0"))
    }

    /// The list value of `key`: its items separated by `;` (an escaped `\;` is part of an
    /// item), each unescaped, empty items dropped.
    pub fn list(&self, key: &str) -> Vec<String> {
        let Some(raw) = self.raw(key) else {
            return Vec::new();
        };
        let mut items = Vec::new();
        let mut item = String::new();
        let mut chars = raw.chars();
        while let Some(character) = chars.next() {
            match character {
                '\\' => match chars.next() {
                    Some(';') => item.push(';'),
                    Some(other) => {
                        item.push('\\');
                        item.push(other);
                    }
                    None => item.push('\\'),
                },
                ';' => items.push(std::mem::take(&mut item)),
                other => item.push(other),
            }
        }
        items.push(item);
        items
            .into_iter()
            .map(|item| unescape(&item))
            .filter(|item| !item.is_empty())
            .collect()
    }

    /// The command line of `Exec` as arguments, with its field codes filled in for a start
    /// without files or URLs: `%f`, `%F`, `%u`, `%U` and the deprecated codes go, `%i` becomes
    /// `--icon` and the icon, `%c` the name (localized for `languages`), `%k` `path`, `%%` a
    /// `%`.
    ///
    /// # Errors
    ///
    /// An [`ExecError`] if there is no `Exec`, its quoting is broken, or nothing is left of it.
    pub fn command(&self, path: &Path, languages: &[String]) -> Result<Vec<String>, ExecError> {
        let exec = self.string("Exec").ok_or(ExecError::Missing)?;
        let words = split_exec(&exec)?;
        let name = self.localized("Name", languages).unwrap_or_default();
        let icon = self.string("Icon").filter(|icon| !icon.is_empty());
        let mut args = Vec::with_capacity(words.len());
        for word in words {
            match word.as_str() {
                "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
                "%i" => {
                    if let Some(icon) = &icon {
                        args.push("--icon".to_owned());
                        args.push(icon.clone());
                    }
                }
                _ => args.push(expand_codes(&word, &name, path)),
            }
        }
        if args.first().is_none_or(String::is_empty) {
            return Err(ExecError::Empty);
        }
        Ok(args)
    }
}

/// Fills the field codes inside one argument: `%%` is `%`, `%c` the name, `%k` the path; any
/// other code (which the specification only allows as a whole argument) is dropped.
fn expand_codes(word: &str, name: &str, path: &Path) -> String {
    let mut out = String::with_capacity(word.len());
    let mut chars = word.chars();
    while let Some(character) = chars.next() {
        if character != '%' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('c') => out.push_str(name),
            Some('k') => out.push_str(&path.to_string_lossy()),
            _ => {}
        }
    }
    out
}

/// Why the command line of a desktop entry can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecError {
    /// The entry has no `Exec` key.
    Missing,
    /// A quote isn't closed, or a backslash ends the line.
    Quoting,
    /// Nothing is left of the command line.
    Empty,
}

/// Resolves the escapes of a string value: `\s` space, `\n` newline, `\t` tab, `\r` carriage
/// return, `\\` backslash. Any other backslash stays as it is.
pub fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// Splits the (already unescaped) value of `Exec` into arguments as the specification quotes
/// them: arguments are separated by spaces; inside double quotes a backslash escapes `"`, `` ` ``,
/// `$` and `\`. As `GLib` and systemd do, single quotes are read like a shell's and a backslash
/// outside quotes escapes the next character.
///
/// # Errors
///
/// [`ExecError::Quoting`] for a quote that isn't closed or a backslash at the end.
pub fn split_exec(exec: &str) -> Result<Vec<String>, ExecError> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = exec.chars();
    while let Some(character) = chars.next() {
        match character {
            ' ' | '\t' | '\n' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next().ok_or(ExecError::Quoting)? {
                        '"' => break,
                        '\\' => {
                            let next = chars.next().ok_or(ExecError::Quoting)?;
                            if !matches!(next, '"' | '`' | '$' | '\\') {
                                word.push('\\');
                            }
                            word.push(next);
                        }
                        other => word.push(other),
                    }
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next().ok_or(ExecError::Quoting)? {
                        '\'' => break,
                        other => word.push(other),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.push(chars.next().ok_or(ExecError::Quoting)?);
            }
            other => {
                in_word = true;
                word.push(other);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// The locale names a localized key is looked up under for `language` (`ll[_CC][.enc][@mod]`),
/// most specific first: `ll_CC@mod`, `ll_CC`, `ll@mod`, `ll`. The encoding is ignored.
pub fn locale_variants(language: &str) -> Vec<String> {
    let (rest, modifier) = match language.split_once('@') {
        Some((rest, modifier)) => (rest, Some(modifier)),
        None => (language, None),
    };
    let rest = rest.split('.').next().unwrap_or(rest);
    let (lang, country) = match rest.split_once('_') {
        Some((lang, country)) => (lang, Some(country)),
        None => (rest, None),
    };
    if lang.is_empty() {
        return Vec::new();
    }
    let mut variants = Vec::new();
    if let (Some(country), Some(modifier)) = (country, modifier) {
        variants.push(format!("{lang}_{country}@{modifier}"));
    }
    if let Some(country) = country {
        variants.push(format!("{lang}_{country}"));
    }
    if let Some(modifier) = modifier {
        variants.push(format!("{lang}@{modifier}"));
    }
    variants.push(lang.to_owned());
    variants
}

/// Whether `id` can be a desktop file ID Sway Things writes or passes on: a name ending in
/// `.desktop` of ASCII letters, digits, `-`, `_` and `.`, not starting with `-` or `.`. That is
/// what desktop file IDs consist of in practice, and none of these characters means anything to
/// a shell or to sway.
pub fn is_desktop_id(id: &str) -> bool {
    id.len() > ".desktop".len()
        && id.len() <= 255
        && id.ends_with(".desktop")
        && !id.starts_with(['-', '.'])
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
