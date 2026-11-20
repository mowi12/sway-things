// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Screen recording that one key starts and stops.
//!
//! The recorder (wf-recorder, else wl-screenrec) is started detached in a session of its own
//! (`setsid -f`, like `wl-mirror`), with `SWAY_THINGS_RECORDING` in its environment, and the
//! command line ends. The same key runs the command line again, which finds the recorder in
//! `/proc` by its name, the marker and the sway session, and stops it with `SIGINT`, as Ctrl+C
//! would, so it finishes the file. Nothing of Sway Things runs meanwhile; the recorder is the
//! user's tool. Its standard error goes to `$XDG_RUNTIME_DIR/sway-things-recording.log`, so a
//! recorder that ends at once can say why.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::CaptureError;
use super::shot::Target;
use super::tools::{Program, WF_RECORDER, WL_SCREENREC};

/// The environment variable that marks a recorder Sway Things started; its value is the id of
/// the notification that says it records.
pub const MARKER: &str = "SWAY_THINGS_RECORDING";

/// The video formats, by the extension of the file name; the first is the default. Both
/// recorders encode H.264 by default, which these containers take; `.webm` would need VP8 or VP9,
/// which wl-screenrec can't encode, so it isn't offered.
pub const EXTENSIONS: [&str; 2] = ["mp4", "mkv"];

/// A recording that runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    /// The recorder's process number.
    pub pid: u32,
    /// The file it writes, from its arguments.
    pub file: Option<PathBuf>,
    /// The notification that says it records, by its id; 0 without one.
    pub notification: u32,
}

/// The recorder's arguments: what to record, the file and the sound. wf-recorder and
/// wl-screenrec take the same: `-g <geometry>` or `-o <display>`, `-f <file>`, `--audio`. The
/// file is made empty beforehand ([`new_file`]), so wf-recorder gets `-y`: it would ask before
/// writing over it, with nobody to answer.
pub fn args(recorder: Program, target: &Target, file: &Path, audio: bool) -> Vec<OsString> {
    let mut args: Vec<OsString> = target.grim_args().into_iter().map(OsString::from).collect();
    if recorder == WF_RECORDER {
        args.push("-y".into());
    }
    args.push("-f".into());
    args.push(file.as_os_str().to_owned());
    if audio {
        args.push("--audio".into());
    }
    args
}

/// The file in the arguments `args` of a recorder (after `-f`).
fn file_of(args: &[OsString]) -> Option<PathBuf> {
    args.iter()
        .position(|arg| arg == "-f" || arg == "--file" || arg == "--filename")
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from)
}

/// The recordings Sway Things started in this session.
pub fn running() -> Vec<Recording> {
    [WF_RECORDER, WL_SCREENREC]
        .iter()
        .flat_map(|recorder| super::process::marked(recorder.name, MARKER))
        .map(|process| Recording {
            pid: process.pid,
            file: file_of(&process.args),
            notification: process
                .marker
                .and_then(|marker| marker.parse().ok())
                .unwrap_or(0),
        })
        .collect()
}

/// A new, empty file in the folder of `settings` for a recording: named after the pattern,
/// with a video format's extension, numbered if a file of that name exists, made with
/// `create_new`, so no other file is ever written over. Makes the folder.
///
/// # Errors
///
/// [`CaptureError::Time`] without the local time, [`CaptureError::FileName`] for a pattern that
/// makes no name, [`CaptureError::Write`] if the folder or the file can't be made.
pub fn new_file(settings: &super::Settings) -> Result<PathBuf, CaptureError> {
    let now = gio::glib::DateTime::now_local().map_err(|_| CaptureError::Time)?;
    let name = super::name::file_name(&settings.recording_file_name, &now)
        .ok_or(CaptureError::FileName)?;
    let (name, _) = super::name::with_extension(&name, &EXTENSIONS);
    super::name::write_new(&settings.folder, &name, &[]).map_err(|error| CaptureError::Write {
        path: settings.folder.join(&name),
        error: error.to_string(),
    })
}

/// Where the id of the last notification about a recording is kept, so the next one takes its
/// place instead of a stale one staying.
fn notification_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join("sway-things-recording.notification"))
}

/// The id of the last notification about a recording; 0 if none is known.
pub fn last_notification() -> u32 {
    notification_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Notes `id` as the last notification about a recording.
pub fn remember_notification(id: u32) {
    if let Some(path) = notification_path() {
        let _ = std::fs::write(path, id.to_string());
    }
}

/// The log of the recorder's standard error.
fn log_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join("sway-things-recording.log"))
}

/// Starts `recorder` with `args`, detached, marked with the notification `notification`, and
/// checks a moment later that it still runs.
///
/// # Errors
///
/// [`CaptureError::Missing`] without the recorder, [`CaptureError::Failed`] if it can't start
/// or ended at once, with the last line of its log.
pub fn start(recorder: Program, args: &[OsString], notification: u32) -> Result<(), CaptureError> {
    let path = recorder.require()?;
    let failed = |message: String| CaptureError::Failed {
        program: recorder.name.to_owned(),
        message,
    };
    let log = log_path();
    let stderr = log
        .as_ref()
        .and_then(|log| std::fs::File::create(log).ok())
        .map_or_else(Stdio::null, Stdio::from);
    // `setsid -f` starts it in a session of its own and returns at once, so it neither ends with
    // this process nor stays behind as a child nobody waits for.
    let status = Command::new("setsid")
        .arg("-f")
        .arg(path)
        .args(args)
        .env(MARKER, notification.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .status()
        .map_err(|error| failed(error.to_string()))?;
    if !status.success() {
        return Err(failed(status.to_string()));
    }
    // A recorder that can't record (no encoder, a file it can't write) ends at once.
    std::thread::sleep(Duration::from_millis(700));
    if running().is_empty() {
        let said = log
            .and_then(|log| std::fs::read(log).ok())
            .map(|text| super::tools::last_line(&text))
            .unwrap_or_default();
        return Err(failed(said));
    }
    Ok(())
}

/// Stops `recording` with `SIGINT`, as Ctrl+C would, and waits until the recorder ended (at most
/// ten seconds), so its file is complete. Returns whether it ended.
pub fn stop(recording: &Recording) -> bool {
    super::process::signal(&[recording.pid], "INT");
    let start = Instant::now();
    while super::process::runs(recording.pid) && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    !super::process::runs(recording.pid)
}
