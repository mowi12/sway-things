// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every variable environment.d sets, from every file, in the order systemd reads them, with
//! Sway Things' own file in its place: so the Environment page can show the variables set
//! elsewhere and say which line wins.
//!
//! systemd reads the `.conf` files of `~/.config/environment.d`, `/etc`, `/run`,
//! `/usr/local/lib` and `/usr/lib/environment.d`: a file in an earlier directory hides one of
//! the same name in a later one, the files are read sorted by name across the directories, and
//! a later line sets a variable again. A line whose value refers to its own name
//! (`PATH=$HOME/bin:$PATH`) builds on the one before instead of replacing it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::HEADER_LINES;
use super::syntax::{self, Assignment};
use crate::apps::Dirs;
use crate::registry::Variable;

/// The directories of the system that systemd reads after the user's, in its order.
const SYSTEM_DIRS: [&str; 4] = [
    "/etc/environment.d",
    "/run/environment.d",
    "/usr/local/lib/environment.d",
    "/usr/lib/environment.d",
];

/// Whose file a line is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Sway Things' file, as the settings make it.
    Ours,
    /// A file in the user's own `~/.config/environment.d`.
    User,
    /// A file of the system.
    System,
}

/// A line of an environment.d file that sets a variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The variable's name.
    pub name: String,
    /// Its value without quotes, not yet expanded.
    pub value: String,
    /// The file.
    pub file: PathBuf,
    /// The line in it, from 1.
    pub line: usize,
    /// Whose file it is.
    pub origin: Origin,
}

/// The directories systemd reads environment.d files from, the user's first.
pub fn search_dirs(dirs: &Dirs) -> Vec<PathBuf> {
    let mut search = vec![dirs.config_home.join("environment.d")];
    search.extend(SYSTEM_DIRS.iter().map(PathBuf::from));
    search
}

/// The `.conf` files in `search`, in the order systemd reads them: by name, the first directory
/// with a name winning.
///
/// # Blocking
///
/// Reads the directories of `environment.d`.
pub fn files(search: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: HashMap<String, PathBuf> = HashMap::new();
    for dir in search {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let conf = Path::new(&name)
                .extension()
                .is_some_and(|extension| extension == "conf");
            if conf && !found.contains_key(&name) {
                found.insert(name, entry.path());
            }
        }
    }
    let mut files: Vec<(String, PathBuf)> = found.into_iter().collect();
    files.sort();
    files.into_iter().map(|(_, path)| path).collect()
}

/// Every line that sets a variable, in the order systemd reads them: the files in `search`, and
/// `ours` (the variables Sway Things writes) at the place of its own file, `file`, whether that
/// exists yet or not. A file read from disk at `file` is left out: `ours` is what it will hold.
///
/// # Blocking
///
/// Reads the directories of `environment.d`.
pub fn lines(search: &[PathBuf], file: &Path, ours: &[Variable]) -> Vec<Line> {
    let user = search.first().cloned();
    let mut files = files(search);
    if !files.iter().any(|path| path == file) {
        files.push(file.to_owned());
        files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    }
    let mut lines = Vec::new();
    for path in files {
        if path == file {
            lines.extend(ours.iter().enumerate().map(|(index, variable)| Line {
                name: variable.name.clone(),
                value: if variable.raw {
                    syntax::unquote(&variable.value)
                } else {
                    variable.value.clone()
                },
                file: path.clone(),
                line: HEADER_LINES + index + 1,
                origin: Origin::Ours,
            }));
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let origin = if user.as_deref().is_some_and(|user| path.starts_with(user)) {
            Origin::User
        } else {
            Origin::System
        };
        lines.extend(syntax::parse(&text).assignments.into_iter().map(
            |Assignment { name, value, line }| Line {
                name,
                value,
                file: path.clone(),
                line,
                origin,
            },
        ));
    }
    lines
}

/// The lines of the user's session: the user's directories from `dirs`, `ours` in Sway Things'
/// file.
///
/// # Blocking
///
/// Reads the directories of `environment.d`.
pub fn of_session(dirs: &Dirs, ours: &[Variable]) -> Vec<Line> {
    lines(&search_dirs(dirs), &super::path(dirs), ours)
}

/// For each of `lines`, the index of the line that replaces its value: the first later line
/// setting the same variable without referring to it. A line that refers to it builds on it.
pub fn overridden_by(lines: &[Line]) -> Vec<Option<usize>> {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            lines
                .iter()
                .enumerate()
                .skip(index + 1)
                .find(|(_, later)| {
                    later.name == line.name && !syntax::refers_to(&later.value, &later.name)
                })
                .map(|(later, _)| later)
        })
        .collect()
}

/// The value each of `lines` gives its variable, expanded as systemd does: starting from
/// `base` (the user manager's environment), each line in order sees the lines before it.
pub fn expanded<S: std::hash::BuildHasher + Clone>(
    lines: &[Line],
    base: &HashMap<String, String, S>,
) -> Vec<String> {
    let mut environment = base.clone();
    lines
        .iter()
        .map(|line| {
            let value = syntax::expand(&line.value, &|name| environment.get(name).cloned());
            environment.insert(line.name.clone(), value.clone());
            value
        })
        .collect()
}

/// The environment of the systemd user manager (`systemctl --user show-environment`), or `None`
/// where it doesn't answer.
pub async fn user_manager() -> Option<HashMap<String, String>> {
    let output = crate::systemd::output(&["show-environment"]).await?;
    Some(parse_show_environment(&output))
}

/// The variables of `systemctl --user show-environment`: `NAME=value`, a value with special
/// characters as `$'…'` with C escapes.
pub fn parse_show_environment(output: &str) -> HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            let value = match value
                .strip_prefix("$'")
                .and_then(|quoted| quoted.strip_suffix('\''))
            {
                Some(quoted) => unescape_c(quoted),
                None => value.to_owned(),
            };
            Some((name.to_owned(), value))
        })
        .collect()
}

/// `text` with its C escapes (`\n`, `\t`, `\\`, `\'`, `\"`, `\xHH`) replaced.
fn unescape_c(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        rest = after;
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        let Some((&escaped, after)) = rest.split_first() else {
            bytes.push(byte);
            break;
        };
        rest = after;
        match escaped {
            b'n' => bytes.push(b'\n'),
            b't' => bytes.push(b'\t'),
            b'r' => bytes.push(b'\r'),
            b'x' => {
                let hex = rest.get(..2).and_then(|hex| std::str::from_utf8(hex).ok());
                if let Some(value) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    bytes.push(value);
                    rest = rest.get(2..).unwrap_or_default();
                }
            }
            other => bytes.push(other),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
