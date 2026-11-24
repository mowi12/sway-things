// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Whether the sway session read environment.d.
//!
//! Only the systemd user manager reads environment.d by itself. sway gets the variables only
//! when whatever starts it runs the environment.d generator first (uwsm, openSUSEway's
//! `sway-run.sh`, Fedora's `start-sway`, GDM); sway started from a text console, greetd or SDDM
//! on Arch or Debian doesn't. So after a login, a variable of Sway Things' file that sway's
//! environment lacks although the file is older than sway says the session doesn't read it, and
//! the page tells the line that makes it do so.
//!
//! sway's environment is read from `/proc/<pid>/environ` of the sway that runs this session,
//! found by its IPC socket (`$SWAYSOCK` is `sway-ipc.<uid>.<pid>.sock`), not from the app's own:
//! an app started by the user manager has the user manager's environment. sway's start is when
//! it made the socket.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::registry::Variable;

/// Where systemd's environment.d generator is, on the distributions with a merged `/usr` and
/// the others.
const GENERATORS: [&str; 2] = [
    "/usr/lib/systemd/user-environment-generators/30-systemd-environment-d-generator",
    "/lib/systemd/user-environment-generators/30-systemd-environment-d-generator",
];

/// The sway session this process runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// sway's environment.
    pub variables: HashMap<String, String>,
    /// When sway started.
    pub started: SystemTime,
}

impl Session {
    /// The sway of `$SWAYSOCK`, if it runs and its environment can be read.
    pub fn of_this_session() -> Option<Self> {
        let socket = PathBuf::from(std::env::var_os("SWAYSOCK")?);
        Self::at(&socket, Path::new("/proc"))
    }

    /// The sway whose IPC socket is `socket`, its process looked up in `proc`.
    pub fn at(socket: &Path, proc: &Path) -> Option<Self> {
        let pid = pid_of(socket)?;
        let started = std::fs::metadata(socket).ok()?.modified().ok()?;
        let environ = std::fs::read(proc.join(pid.to_string()).join("environ")).ok()?;
        Some(Self {
            variables: parse_environ(&environ),
            started,
        })
    }
}

/// The process id in the name of sway's IPC socket, `sway-ipc.<uid>.<pid>.sock`.
pub fn pid_of(socket: &Path) -> Option<u32> {
    let name = socket.file_name()?.to_str()?;
    let rest = name.strip_prefix("sway-ipc.")?.strip_suffix(".sock")?;
    let (_, pid) = rest.split_once('.')?;
    pid.parse().ok()
}

/// The variables of a process's environment block: `NAME=value` separated by NUL bytes.
pub fn parse_environ(environ: &[u8]) -> HashMap<String, String> {
    environ
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (name, value) = entry.split_once('=')?;
            Some((name.to_owned(), value.to_owned()))
        })
        .collect()
}

/// The names of the variables of `ours` that `session` lacks, or has with another value,
/// although the file, last written at `written`, is older than the session: the session didn't
/// read environment.d. Variables in environment.d's own syntax are left out, since only the
/// generator knows what they expand to. Empty for a file written since the session started:
/// the change waits for the next login.
pub fn not_read(ours: &[Variable], written: SystemTime, session: &Session) -> Vec<String> {
    if written >= session.started {
        return Vec::new();
    }
    ours.iter()
        .filter(|variable| !variable.raw)
        .filter(|variable| session.variables.get(&variable.name) != Some(&variable.value))
        .map(|variable| variable.name.clone())
        .collect()
}

/// The environment.d generator of this system, or where most distributions have it.
pub fn generator() -> PathBuf {
    GENERATORS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(GENERATORS[0]))
}

/// The line to run before sway starts, in `~/.profile` or the script that starts sway, so the
/// session gets the variables of environment.d: the generator prints them shell-quoted, and
/// `set -a` exports them.
pub fn import_line(generator: &Path) -> String {
    format!("set -a; eval \"$({})\"; set +a", generator.display())
}
