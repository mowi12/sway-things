// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sway Things' environment.d file: the environment variables of every setting that declares
//! some ([`Setting::environment`](crate::registry::Setting::environment)), in
//! `$XDG_CONFIG_HOME/environment.d/50-sway-things.conf`.
//!
//! The systemd user manager reads environment.d (environment.d(5)) when it starts and at each
//! `daemon-reload`, for the services it starts from then on; a session that runs the
//! environment.d generator before sway starts (uwsm, openSUSEway, Fedora's start-sway) gives
//! them to sway and every app. So a change applies after the next login, and Sway Things runs
//! `systemctl --user daemon-reload` after the file changed, so the user manager doesn't keep the
//! old variables when it outlives a quick logout.
//!
//! The file is kept like every file of another program ([`crate::manifest`]): created
//! where none is, replaced while it is as Sway Things wrote it, deleted once no setting writes a
//! variable; someone else's file is never touched. [`session`] finds out whether the session
//! read it.

pub mod files;
pub mod portals;
pub mod screencast;
pub mod session;
pub mod syntax;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use crate::apps::Dirs;
use crate::manifest::HEADER;
use crate::manifest::Wanted;
use crate::registry::environment::{VARIABLES, is_variable_name};
use crate::registry::{Registry, Variable};
use crate::state::State;

/// The name of the file, sorted among the files of environment.d: the systemd documentation
/// asks for a two-digit prefix, and a file of the user's with a later name wins over it.
pub const FILE_NAME: &str = "50-sway-things.conf";

/// How many lines the header of the file has: its first variable is on the line after.
pub const HEADER_LINES: usize = {
    let bytes = HEADER.as_bytes();
    let mut count = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            count += 1;
        }
        index += 1;
    }
    count
};

/// The path of the file for `dirs`.
pub fn path(dirs: &Dirs) -> PathBuf {
    dirs.config_home.join("environment.d").join(FILE_NAME)
}

/// The variables the settings in `state` write, in the order of the registry, the user's own
/// ([`VARIABLES`]) last. A name written by two settings keeps the later value. A variable with a name environment.d can't take, or with
/// a value that holds a line break or another control character, is left out.
pub fn variables(registry: &Registry, state: &State) -> Vec<Variable> {
    let mut variables: Vec<Variable> = Vec::new();
    let editor = registry.iter().filter(|setting| setting.key() == VARIABLES);
    let others = registry.iter().filter(|setting| setting.key() != VARIABLES);
    for setting in others.chain(editor) {
        let Some(mapping) = setting.environment() else {
            continue;
        };
        let value = state
            .get(setting.key(), None)
            .cloned()
            .unwrap_or_else(|| setting.default_value());
        for variable in mapping.variables(&value) {
            if !is_variable_name(&variable.name) || variable.value.chars().any(char::is_control) {
                continue;
            }
            variables.retain(|other| other.name != variable.name);
            variables.push(variable);
        }
    }
    variables
}

/// The value as environment.d writes it: as it is where that is unambiguous, else in double
/// quotes, in which a backslash keeps `"`, `\` and `` ` `` as they are. environment.d expands
/// `$` after taking the quotes away, also in quotes, so a `$` is written as `$$`.
pub fn quote(value: &str) -> String {
    let plain = !value.is_empty()
        && value.chars().all(|char| {
            char.is_ascii_alphanumeric()
                || matches!(
                    char,
                    '_' | '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-'
                )
        });
    if plain {
        return value.to_owned();
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for char in value.chars() {
        match char {
            '"' | '\\' | '`' => quoted.push('\\'),
            '$' => quoted.push('$'),
            _ => {}
        }
        quoted.push(char);
    }
    quoted.push('"');
    quoted
}

/// The line of `variable` in the file.
pub fn line(variable: &Variable) -> String {
    if variable.raw {
        format!("{}={}", variable.name, variable.value)
    } else {
        format!("{}={}", variable.name, quote(&variable.value))
    }
}

/// The text of the file for `variables`: the header and a line per variable. `None` without
/// variables: the file isn't needed.
pub fn text(variables: &[Variable]) -> Option<String> {
    if variables.is_empty() {
        return None;
    }
    let mut text = HEADER.to_owned();
    for variable in variables {
        text.push_str(&line(variable));
        text.push('\n');
    }
    Some(text)
}

/// The file the settings in `state` want, for `dirs`.
pub fn wanted(state: &State, dirs: &Dirs) -> Wanted {
    Wanted {
        path: path(dirs),
        text: text(&variables(state.registry(), state)),
    }
}

/// Tells the systemd user manager to read environment.d again, for the services it starts from
/// now on.
///
/// # Errors
///
/// What `systemctl` reported, if it failed or couldn't be run.
pub async fn reload_user_manager() -> Result<(), String> {
    crate::systemd::run(&["daemon-reload"]).await
}
