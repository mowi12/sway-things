// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Expanding the argument of `include` the way sway does, without a shell.
//!
//! sway passes the argument to `wordexp(3)`, which treats it like words on a shell command line:
//! it splits at blanks, removes quotes, replaces `~` and `$VARIABLE`, and expands globs. It also
//! runs `$(commands)`. This module does the same except for running anything: what would need a
//! command, another user's home directory or a shell operator is an [`ExpandError`].

use std::fmt;
use std::path::{Path, PathBuf};

use super::Files;

/// A pattern with more path segments than this isn't expanded.
const MAX_SEGMENTS: usize = 64;

/// At most this many paths come out of one pattern.
const MAX_PATHS: usize = 4096;

/// Why an include path can't be expanded without running a shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpandError {
    /// The path contains `$(…)` or backticks: sway would run a command to get it.
    CommandSubstitution,
    /// The path uses something this parser doesn't follow: `~user`, or `${VAR…}` with an
    /// operator such as `:-`.
    Unsupported,
    /// `wordexp` rejects the path, so sway includes nothing: an unquoted `|`, `&`, `;`, `<`, `>`,
    /// `(`, `)`, `{`, `}` or line break, or a quote or `${` that isn't closed.
    Invalid,
    /// The pattern is too long or matches too many files to follow.
    TooBig,
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CommandSubstitution => "the path needs a command to be run",
            Self::Unsupported => "the path uses ~user or a ${…} operator",
            Self::Invalid => "the path is not valid for sway",
            Self::TooBig => "the pattern matches too many files",
        })
    }
}

impl std::error::Error for ExpandError {}

/// A character of a word, and whether it still has its special meaning for globs (it wasn't
/// quoted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Atom {
    char: char,
    special: bool,
}

impl Atom {
    fn literal(char: char) -> Self {
        Self {
            char,
            special: false,
        }
    }

    fn is_glob(self) -> bool {
        self.special && matches!(self.char, '*' | '?' | '[')
    }
}

/// Expands `pattern`, the argument of an `include` in a config file in `directory`, into the
/// paths sway would try to load, in sway's order. Relative paths are in `directory`.
///
/// A path without glob characters is returned whether it exists or not, and so is a glob that
/// matches nothing (as the pattern itself): sway then finds no such file.
///
/// # Errors
///
/// [`ExpandError`] if the paths can't be known without running a shell, or sway includes nothing.
pub fn expand(
    pattern: &str,
    directory: &Path,
    files: &dyn Files,
) -> Result<Vec<PathBuf>, ExpandError> {
    let mut paths = Vec::new();
    for word in words(pattern, files)? {
        let literal = || directory.join(word.iter().map(|atom| atom.char).collect::<String>());
        if word.iter().any(|atom| atom.is_glob()) {
            let matches = glob(&word, directory, files)?;
            if matches.is_empty() {
                paths.push(literal());
            } else {
                paths.extend(matches);
            }
        } else {
            paths.push(literal());
        }
        if paths.len() > MAX_PATHS {
            return Err(ExpandError::TooBig);
        }
    }
    Ok(paths)
}

/// Splits `pattern` into words like a shell: quotes removed, `~` and variables replaced.
fn words(pattern: &str, files: &dyn Files) -> Result<Vec<Vec<Atom>>, ExpandError> {
    let mut words = Vec::new();
    let mut word: Vec<Atom> = Vec::new();
    // A word can be empty (`""`), so having atoms doesn't tell whether one was started.
    let mut started = false;
    let mut chars = pattern.chars().peekable();
    while let Some(char) = chars.next() {
        match char {
            ' ' | '\t' => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            '\n' | '|' | '&' | ';' | '<' | '>' | '(' | ')' | '{' | '}' => {
                return Err(ExpandError::Invalid);
            }
            '`' => return Err(ExpandError::CommandSubstitution),
            '\\' => {
                word.push(Atom::literal(chars.next().ok_or(ExpandError::Invalid)?));
                started = true;
            }
            '\'' => {
                loop {
                    match chars.next().ok_or(ExpandError::Invalid)? {
                        '\'' => break,
                        char => word.push(Atom::literal(char)),
                    }
                }
                started = true;
            }
            '"' => {
                loop {
                    match chars.next().ok_or(ExpandError::Invalid)? {
                        '"' => break,
                        '`' => return Err(ExpandError::CommandSubstitution),
                        '\\' => match chars.peek() {
                            Some(&escaped @ ('$' | '`' | '"' | '\\')) => {
                                chars.next();
                                word.push(Atom::literal(escaped));
                            }
                            _ => word.push(Atom::literal('\\')),
                        },
                        '$' => match variable(&mut chars, files)? {
                            Some(value) => word.extend(value.chars().map(Atom::literal)),
                            None => word.push(Atom::literal('$')),
                        },
                        char => word.push(Atom::literal(char)),
                    }
                }
                started = true;
            }
            '$' => {
                if let Some(value) = variable(&mut chars, files)? {
                    // Unquoted, the value is split at blanks and its glob characters count.
                    for char in value.chars() {
                        if matches!(char, ' ' | '\t' | '\n') {
                            if started {
                                words.push(std::mem::take(&mut word));
                                started = false;
                            }
                        } else {
                            word.push(Atom {
                                char,
                                special: true,
                            });
                            started = true;
                        }
                    }
                } else {
                    word.push(Atom::literal('$'));
                    started = true;
                }
            }
            '~' if !started => {
                match chars.peek() {
                    None | Some('/' | ' ' | '\t') => {
                        let home = files.env("HOME").ok_or(ExpandError::Unsupported)?;
                        word.extend(home.chars().map(Atom::literal));
                    }
                    // `~user` needs the user database.
                    Some(_) => return Err(ExpandError::Unsupported),
                }
                started = true;
            }
            char => {
                word.push(Atom {
                    char,
                    special: true,
                });
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

/// Reads what follows a `$`: the value of `$NAME` or `${NAME}` (empty if unset), or `None` if
/// the `$` stands for itself.
fn variable(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    files: &dyn Files,
) -> Result<Option<String>, ExpandError> {
    let is_name_char = |char: char| char.is_ascii_alphanumeric() || char == '_';
    let mut name = String::new();
    match chars.peek() {
        Some('(') => return Err(ExpandError::CommandSubstitution),
        Some('{') => {
            chars.next();
            loop {
                match chars.next().ok_or(ExpandError::Invalid)? {
                    '}' => break,
                    char if is_name_char(char) => name.push(char),
                    // `${VAR:-default}` and the like.
                    _ => return Err(ExpandError::Unsupported),
                }
            }
            if name.is_empty() || name.starts_with(|char: char| char.is_ascii_digit()) {
                return Err(ExpandError::Unsupported);
            }
        }
        Some(&char) if char.is_ascii_alphabetic() || char == '_' => {
            while let Some(&char) = chars.peek().filter(|char| is_name_char(**char)) {
                name.push(char);
                chars.next();
            }
        }
        // `$1`, `$?` and other shell parameters mean nothing here.
        Some(char) if char.is_ascii_digit() || matches!(char, '?' | '#' | '*' | '@' | '!') => {
            return Err(ExpandError::Unsupported);
        }
        _ => return Ok(None),
    }
    Ok(Some(files.env(&name).unwrap_or_default()))
}

/// One element of a glob pattern for a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(char),
    /// `*`
    Any,
    /// `?`
    One,
    /// `[a-z]`, `[!abc]`: the ranges, and whether they are negated.
    Class(Vec<(char, char)>, bool),
}

/// The existing paths that match `word`, sorted within each directory like `glob(3)` does.
fn glob(word: &[Atom], directory: &Path, files: &dyn Files) -> Result<Vec<PathBuf>, ExpandError> {
    let absolute = word.first().is_some_and(|atom| atom.char == '/');
    let segments: Vec<&[Atom]> = word
        .split(|atom| atom.char == '/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.len() > MAX_SEGMENTS {
        return Err(ExpandError::TooBig);
    }
    let mut paths = vec![if absolute {
        PathBuf::from("/")
    } else {
        directory.to_owned()
    }];
    for segment in segments {
        let mut next = Vec::new();
        if segment.iter().any(|atom| atom.is_glob()) {
            let pattern = tokens(segment);
            for path in &paths {
                let mut names = files.list(path).unwrap_or_default();
                names.sort_unstable();
                next.extend(
                    names
                        .iter()
                        .filter(|name| matches(&pattern, name))
                        .map(|name| path.join(name)),
                );
                if next.len() > MAX_PATHS {
                    return Err(ExpandError::TooBig);
                }
            }
        } else {
            let name: String = segment.iter().map(|atom| atom.char).collect();
            next.extend(paths.iter().map(|path| path.join(&name)));
        }
        paths = next;
    }
    paths.retain(|path| files.exists(path));
    Ok(paths)
}

/// Reads the glob pattern of one path segment.
fn tokens(segment: &[Atom]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while let Some(&atom) = segment.get(index) {
        index += 1;
        if !atom.special {
            tokens.push(Token::Literal(atom.char));
            continue;
        }
        match atom.char {
            '*' => tokens.push(Token::Any),
            '?' => tokens.push(Token::One),
            '[' => match class(&segment[index..]) {
                Some((token, length)) => {
                    tokens.push(token);
                    index += length;
                }
                // A `[` without `]` stands for itself.
                None => tokens.push(Token::Literal('[')),
            },
            char => tokens.push(Token::Literal(char)),
        }
    }
    tokens
}

/// Reads a bracket expression after its `[`: the token and how many atoms it took, or `None` if
/// there is no closing `]`.
fn class(atoms: &[Atom]) -> Option<(Token, usize)> {
    let negated = atoms
        .first()
        .is_some_and(|atom| atom.special && matches!(atom.char, '!' | '^'));
    let start = usize::from(negated);
    // A `]` right after the opening is a member, not the end.
    let end = atoms
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, atom)| atom.special && atom.char == ']')
        .map(|(index, _)| index)?;
    let members = &atoms[start..end];
    let mut ranges = Vec::new();
    let mut index = 0;
    while let Some(&first) = members.get(index) {
        match (members.get(index + 1), members.get(index + 2)) {
            (Some(dash), Some(&last)) if dash.special && dash.char == '-' => {
                ranges.push((first.char, last.char));
                index += 3;
            }
            _ => {
                ranges.push((first.char, first.char));
                index += 1;
            }
        }
    }
    Some((Token::Class(ranges, negated), end + 1))
}

/// Whether the file name `name` matches `pattern`. A leading dot only matches a literal dot.
fn matches(pattern: &[Token], name: &str) -> bool {
    if name.starts_with('.') && pattern.first() != Some(&Token::Literal('.')) {
        return false;
    }
    let name: Vec<char> = name.chars().collect();
    let (mut token, mut char) = (0, 0);
    // Where to go back to when the pattern stops matching: the last `*` takes one more
    // character.
    let mut star: Option<(usize, usize)> = None;
    while char < name.len() {
        let matched = match pattern.get(token) {
            Some(Token::Any) => {
                star = Some((token, char));
                token += 1;
                continue;
            }
            Some(Token::Literal(literal)) => *literal == name[char],
            Some(Token::One) => true,
            Some(Token::Class(ranges, negated)) => {
                let inside = ranges
                    .iter()
                    .any(|(first, last)| (*first..=*last).contains(&name[char]));
                inside != *negated
            }
            None => false,
        };
        if matched {
            token += 1;
            char += 1;
        } else if let Some((star_token, star_char)) = star {
            token = star_token + 1;
            char = star_char + 1;
            star = Some((star_token, star_char + 1));
        } else {
            return false;
        }
    }
    pattern[token.min(pattern.len())..]
        .iter()
        .all(|token| *token == Token::Any)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use proptest::prelude::*;

    use super::{ExpandError, expand};
    use crate::sway_config::MemoryFiles;

    fn files() -> MemoryFiles {
        MemoryFiles::new()
            .var("HOME", "/home/me")
            .var("XDG_CONFIG_HOME", "/home/me/.config")
            .var("TWO", "a.conf  b.conf")
            .var("STAR", "*.conf")
            .file("/home/me/.config/sway/config", "")
            .file("/home/me/.config/sway/config.d/10-input.conf", "")
            .file("/home/me/.config/sway/config.d/20-output.conf", "")
            .file("/home/me/.config/sway/config.d/.hidden.conf", "")
            .file("/home/me/.config/sway/config.d/notes.txt", "")
            .file("/home/me/.config/sway/config.d/[x].conf", "")
            .file("/home/me/.config/sway/sway-things/10-input.conf", "")
            .file("/home/me/.config/sway/sway-things/90-effects.conf", "")
            .file("/home/me/.config/sway/my configs/a.conf", "")
            .file("/etc/sway/config.d/50-systemd-user.conf", "")
    }

    fn expanded(pattern: &str) -> Result<Vec<String>, ExpandError> {
        let paths = expand(pattern, Path::new("/home/me/.config/sway"), &files())?;
        Ok(paths
            .into_iter()
            .map(|path| path.to_str().unwrap().to_owned())
            .collect())
    }

    #[test]
    fn plain_paths_are_relative_to_the_including_file() {
        assert_eq!(
            expanded("config.d/10-input.conf").unwrap(),
            ["/home/me/.config/sway/config.d/10-input.conf"]
        );
        assert_eq!(
            expanded("/etc/sway/missing").unwrap(),
            ["/etc/sway/missing"]
        );
        // Whether a plain path exists is for sway to find out.
        assert_eq!(
            expanded("nope.conf").unwrap(),
            ["/home/me/.config/sway/nope.conf"]
        );
        assert_eq!(expanded("").unwrap(), Vec::<String>::new());
        assert_eq!(expanded("  \t ").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn tilde_and_variables_are_replaced() {
        let ours = [
            "/home/me/.config/sway/sway-things/10-input.conf",
            "/home/me/.config/sway/sway-things/90-effects.conf",
        ];
        for pattern in [
            "~/.config/sway/sway-things/*.conf",
            "$HOME/.config/sway/sway-things/*.conf",
            "${HOME}/.config/sway/sway-things/*.conf",
            "$XDG_CONFIG_HOME/sway/sway-things/*.conf",
            "\"$XDG_CONFIG_HOME\"/sway/sway-things/*.conf",
            "sway-things/*.conf",
            "/home/me/.config/sway/sway-things/*",
            "/home/me/.config/sway/sway-things/[0-9]0-*.conf",
            "/home/me/.config/sw?y/sway-th*/??-*.c[!x]nf",
        ] {
            assert_eq!(expanded(pattern).unwrap(), ours, "{pattern}");
        }
        // An unset variable is empty; a `$` without a name stands for itself.
        assert_eq!(expanded("$UNSET/etc/x").unwrap(), ["/etc/x"]);
        assert_eq!(expanded("a$").unwrap(), ["/home/me/.config/sway/a$"]);
        assert_eq!(expanded("~").unwrap(), ["/home/me"]);
        // A tilde in the middle of a word is just a tilde.
        assert_eq!(expanded("a~").unwrap(), ["/home/me/.config/sway/a~"]);
    }

    #[test]
    fn globs_match_like_a_shell() {
        // Sorted, without hidden files, only what exists.
        assert_eq!(
            expanded("config.d/*").unwrap(),
            [
                "/home/me/.config/sway/config.d/10-input.conf",
                "/home/me/.config/sway/config.d/20-output.conf",
                "/home/me/.config/sway/config.d/[x].conf",
                "/home/me/.config/sway/config.d/notes.txt",
            ]
        );
        assert_eq!(
            expanded("config.d/.*.conf").unwrap(),
            ["/home/me/.config/sway/config.d/.hidden.conf"]
        );
        assert_eq!(
            expanded("/etc/sway/config.d/* config.d/[12]0-*").unwrap(),
            [
                "/etc/sway/config.d/50-systemd-user.conf",
                "/home/me/.config/sway/config.d/10-input.conf",
                "/home/me/.config/sway/config.d/20-output.conf",
            ]
        );
        // Nothing matches: the pattern itself comes back, and sway finds no such file.
        assert_eq!(
            expanded("config.d/*.toml").unwrap(),
            ["/home/me/.config/sway/config.d/*.toml"]
        );
        assert_eq!(
            expanded("*/missing/*.conf").unwrap(),
            ["/home/me/.config/sway/*/missing/*.conf"]
        );
        // A `[` that is never closed stands for itself, and a class can hold a `]`.
        assert_eq!(
            expanded("config.d/[x*").unwrap(),
            ["/home/me/.config/sway/config.d/[x].conf"]
        );
        assert_eq!(
            expanded("config.d/[][]x[]]*").unwrap(),
            ["/home/me/.config/sway/config.d/[x].conf"]
        );
    }

    #[test]
    fn quotes_keep_blanks_and_globs_literal() {
        // sway already removed one level of quotes, so a path with a blank is two words.
        assert_eq!(
            expanded("my configs/a.conf").unwrap(),
            [
                "/home/me/.config/sway/my",
                "/home/me/.config/sway/configs/a.conf"
            ]
        );
        for pattern in [
            "'my configs'/*.conf",
            "\"my configs\"/*.conf",
            "my\\ configs/*.conf",
        ] {
            assert_eq!(
                expanded(pattern).unwrap(),
                ["/home/me/.config/sway/my configs/a.conf"],
                "{pattern}"
            );
        }
        for pattern in ["'config.d/*'", "\"config.d/*\"", "config.d/\\*"] {
            assert_eq!(
                expanded(pattern).unwrap(),
                ["/home/me/.config/sway/config.d/*"],
                "{pattern}"
            );
        }
        // No variables in single quotes; escapes in double quotes.
        assert_eq!(
            expanded("'$HOME' \"\\$HOME \\x\"").unwrap(),
            [
                "/home/me/.config/sway/$HOME",
                "/home/me/.config/sway/$HOME \\x"
            ]
        );
        // An unquoted value is split and globbed, a quoted one isn't.
        assert_eq!(
            expanded("$TWO").unwrap(),
            [
                "/home/me/.config/sway/a.conf",
                "/home/me/.config/sway/b.conf"
            ]
        );
        assert_eq!(
            expanded("\"$TWO\"").unwrap(),
            ["/home/me/.config/sway/a.conf  b.conf"]
        );
        assert_eq!(
            expanded("config.d/$STAR").unwrap(),
            [
                "/home/me/.config/sway/config.d/10-input.conf",
                "/home/me/.config/sway/config.d/20-output.conf",
                "/home/me/.config/sway/config.d/[x].conf",
            ]
        );
        assert_eq!(
            expanded("config.d/\"$STAR\"").unwrap(),
            ["/home/me/.config/sway/config.d/*.conf"]
        );
        assert_eq!(expanded("\"\"").unwrap(), ["/home/me/.config/sway/"]);
    }

    #[test]
    fn what_needs_a_shell_is_not_expanded() {
        for pattern in [
            "$(hostname).conf",
            "`hostname`.conf",
            "\"`id -un`\"",
            "\"$(x)\"",
        ] {
            assert_eq!(
                expanded(pattern),
                Err(ExpandError::CommandSubstitution),
                "{pattern}"
            );
        }
        for pattern in [
            "~root/config",
            "${HOME:-/x}/config",
            "${}",
            "$1",
            "${1}",
            "$?",
        ] {
            assert_eq!(
                expanded(pattern),
                Err(ExpandError::Unsupported),
                "{pattern}"
            );
        }
        for pattern in [
            "a | b", "a;b", "a&", "<a", "a>b", "(a)", "{a,b}", "a\nb", "'a", "\"a", "a\\", "${HOME",
        ] {
            assert_eq!(expanded(pattern), Err(ExpandError::Invalid), "{pattern}");
        }
        // Without a home directory, `~` can't be followed.
        let homeless = MemoryFiles::new();
        assert_eq!(
            expand("~/x", Path::new("/"), &homeless),
            Err(ExpandError::Unsupported)
        );
    }

    #[test]
    fn patterns_are_bounded() {
        let deep = "*/".repeat(100);
        assert_eq!(expanded(&deep), Err(ExpandError::TooBig));
        let mut many = MemoryFiles::new();
        for index in 0..5000 {
            many = many.file(&format!("/d/{index}.conf"), "");
        }
        assert_eq!(
            expand("/d/*", Path::new("/"), &many),
            Err(ExpandError::TooBig)
        );
        // A pattern that would take long with naive backtracking.
        let slow = MemoryFiles::new().file(&format!("/d/{}b", "a".repeat(2000)), "");
        let pattern = format!("/d/{}c", "*a".repeat(50));
        assert_eq!(
            expand(&pattern, Path::new("/"), &slow).unwrap(),
            [PathBuf::from(&pattern)]
        );
    }

    proptest! {
        #[test]
        fn any_pattern_expands_or_fails_cleanly(pattern in "\\PC{0,60}") {
            let _ = expanded(&pattern);
        }

        #[test]
        fn shell_like_patterns_expand_or_fail_cleanly(
            pattern in "[-a-z/.*?\\[\\]!^~$ {}'\"\\\\`()]{0,40}"
        ) {
            let _ = expanded(&pattern);
        }
    }
}
