// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The processes Sway Things starts for the session (the clipboard history's watcher, a
//! recorder, the night light), found in `/proc` again.
//!
//! A process counts only if it belongs to this sway session: the same `XDG_RUNTIME_DIR` and
//! `WAYLAND_DISPLAY` in its environment. A caller without either (a TTY, ssh) finds nothing, so
//! it can never end the watcher or the recorder of another session.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A process of this session, found in `/proc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Its process number.
    pub pid: u32,
    /// Its arguments, without the program, byte for byte.
    pub args: Vec<OsString>,
    /// The value of the marker looked for, if it has it.
    pub marker: Option<String>,
}

/// The session this process belongs to: `XDG_RUNTIME_DIR` and `WAYLAND_DISPLAY`, as entries of
/// an environment; `None` without either.
fn session() -> Option<[Vec<u8>; 2]> {
    let entry = |variable: &str| {
        let value = std::env::var_os(variable).filter(|value| !value.is_empty())?;
        let mut entry = OsString::from(format!("{variable}="));
        entry.push(value);
        Some(entry.into_vec())
    };
    Some([entry("XDG_RUNTIME_DIR")?, entry("WAYLAND_DISPLAY")?])
}

/// The processes named `name` of this user in this session, each with the value of `marker`
/// in its environment if it has it. Nothing without a session ([`session`]). Reads `/proc`;
/// nothing is run.
pub fn find(name: &str, marker: &str) -> Vec<Found> {
    find_in(session(), name, marker)
}

/// The processes of this user in this session named one of `names`, each with its name, as
/// [`find`] finds them, in one pass over `/proc`.
pub fn find_any(names: &[&str], marker: &str) -> Vec<(String, Found)> {
    scan(session(), names, marker)
}

/// [`find`] for the session `session` (the two entries of the environment that make it); nothing
/// for `None`, never every session.
pub(crate) fn find_in(session: Option<[Vec<u8>; 2]>, name: &str, marker: &str) -> Vec<Found> {
    scan(session, &[name], marker)
        .into_iter()
        .map(|(_, found)| found)
        .collect()
}

/// The processes of `session` named one of `names`, with their names.
fn scan(session: Option<[Vec<u8>; 2]>, names: &[&str], marker: &str) -> Vec<(String, Found)> {
    use std::os::unix::fs::MetadataExt as _;
    let Some(session) = session else {
        return Vec::new();
    };
    let Ok(me) = std::fs::metadata("/proc/self").map(|meta| meta.uid()) else {
        return Vec::new();
    };
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let prefix = format!("{marker}=");
    processes
        .filter_map(Result::ok)
        .filter_map(|process| {
            let pid: u32 = process.file_name().to_str()?.parse().ok()?;
            let path = process.path();
            if process.metadata().ok()?.uid() != me {
                return None;
            }
            let comm = std::fs::read_to_string(path.join("comm")).ok()?;
            let name = comm.trim_end();
            if !names.contains(&name) {
                return None;
            }
            let name = name.to_owned();
            let environ = std::fs::read(path.join("environ")).ok()?;
            let mut entries = environ.split(|byte| *byte == 0);
            if !session
                .iter()
                .all(|wanted| entries.clone().any(|entry| entry == wanted.as_slice()))
            {
                return None;
            }
            let marker = entries
                .find_map(|entry| entry.strip_prefix(prefix.as_bytes()))
                .map(|value| String::from_utf8_lossy(value).into_owned());
            let cmdline = std::fs::read(path.join("cmdline")).ok()?;
            let args = cmdline
                .split(|byte| *byte == 0)
                .skip(1)
                .filter(|arg| !arg.is_empty())
                .map(|arg| OsString::from_vec(arg.to_vec()))
                .collect();
            Some((name, Found { pid, args, marker }))
        })
        .collect()
}

/// The processes of [`find`] that have `marker`: those Sway Things started.
pub fn marked(name: &str, marker: &str) -> Vec<Found> {
    find(name, marker)
        .into_iter()
        .filter(|process| process.marker.is_some())
        .collect()
}

/// Sends `signal` (`TERM`, `INT`) to the processes `pids` with `kill`.
pub fn signal(pids: &[u32], signal: &str) {
    if pids.is_empty() {
        return;
    }
    let _ = Command::new("kill")
        .args(["-s", signal])
        .args(pids.iter().map(ToString::to_string))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Whether the process `pid` still runs.
pub fn runs(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
}
