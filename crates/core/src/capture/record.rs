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
//! recorder that ends at once can say why. wf-recorder is asked for a codec, and for the next of
//! [`CODECS`] if it ends at once with one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::CaptureError;
use super::shot::Target;
use super::tools::{Program, WF_RECORDER, WL_SCREENREC};
use crate::session::process;

/// The environment variable that marks a recorder Sway Things started; its value is the id of
/// the notification that says it records.
pub const MARKER: &str = "SWAY_THINGS_RECORDING";

/// The video formats, by the extension of the file name; the first is the default. wf-recorder
/// records H.264 into either, or VP9 into `.mkv` ([`CODECS`]); wl-screenrec encodes H.264 by
/// default. `.webm` would need VP8 or VP9, which wl-screenrec can't encode, so it isn't offered.
pub const EXTENSIONS: [&str; 2] = ["mp4", "mkv"];

/// A video codec wf-recorder is asked for (`-c`), and the container its file needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Codec {
    /// The name of ffmpeg's encoder.
    pub encoder: &'static str,
    /// The extension of the container the codec needs; `None` keeps the name's, any of
    /// [`EXTENSIONS`].
    pub container: Option<&'static str>,
}

/// The codecs wf-recorder is asked for, in this order, until it records with one: H.264 from
/// libx264, else from libopenh264 (distributions without libx264 have it), else VP9 into
/// `.mkv`. Without `-c`, wf-recorder takes the encoder it was built with, which need not be one
/// the container takes (openSUSE's is utvideo, which `.mp4` can't hold).
pub const CODECS: [Codec; 3] = [
    Codec {
        encoder: "libx264",
        container: None,
    },
    Codec {
        encoder: "libopenh264",
        container: None,
    },
    Codec {
        encoder: "libvpx-vp9",
        container: Some("mkv"),
    },
];

/// The pixel format wf-recorder is asked for (`-x`): the one every encoder of [`CODECS`] takes
/// and players play. wf-recorder picks it by itself only for the codec it was built with.
pub const PIXEL_FORMAT: &str = "yuv420p";

/// The video filter wf-recorder is given (`-F`): crops a width or height that is odd by one
/// pixel, since `yuv420p` halves both for the colors and the H.264 encoders refuse odd sizes.
/// slurp selects any size, and a scaled display can make one in pixels. wf-recorder encodes the
/// size the filter puts out.
pub const EVEN_SIZE: &str = "crop=trunc(iw/2)*2:trunc(ih/2)*2:0:0";

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

/// The recorder's arguments: what to record, the codec, the file and the sound. wf-recorder and
/// wl-screenrec take the same: `-g <geometry>` or `-o <display>`, `-f <file>`, `--audio`. With
/// `codec` (for wf-recorder), it gets `-c <encoder>`, `-x` [`PIXEL_FORMAT`] and `-F`
/// [`EVEN_SIZE`]. The file is made empty beforehand, so wf-recorder gets `-y`: it would ask
/// before writing over it, with nobody to answer.
pub fn args(
    recorder: Program,
    target: &Target,
    file: &Path,
    audio: bool,
    codec: Option<Codec>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = target.grim_args().into_iter().map(OsString::from).collect();
    if let Some(codec) = codec {
        for arg in ["-c", codec.encoder, "-x", PIXEL_FORMAT, "-F", EVEN_SIZE] {
            args.push(arg.into());
        }
    }
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

/// The codecs `recorder` is tried with, in order: [`CODECS`] for wf-recorder; none for
/// wl-screenrec, which records H.264 by default.
pub fn codecs(recorder: Program) -> &'static [Codec] {
    if recorder == WF_RECORDER {
        &CODECS
    } else {
        &[]
    }
}

/// `name` (ending in one of [`EXTENSIONS`]) with the extension of the container `codec` needs.
pub fn with_container(name: &str, codec: Option<Codec>) -> String {
    match (
        codec.and_then(|codec| codec.container),
        name.rsplit_once('.'),
    ) {
        (Some(container), Some((stem, _))) => format!("{stem}.{container}"),
        _ => name.to_owned(),
    }
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
        .flat_map(|recorder| process::marked(recorder.name, MARKER))
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

/// The name of a new recording's file in the folder of `settings`: after the pattern, with a
/// video format's extension ([`EXTENSIONS`]). [`record`] makes the file.
///
/// # Errors
///
/// [`CaptureError::Time`] without the local time, [`CaptureError::FileName`] for a pattern that
/// makes no name.
pub fn file_name(settings: &super::Settings) -> Result<String, CaptureError> {
    let now = gio::glib::DateTime::now_local().map_err(|_| CaptureError::Time)?;
    let name = super::name::file_name(&settings.recording_file_name, &now)
        .ok_or(CaptureError::FileName)?;
    Ok(super::name::with_extension(&name, &EXTENSIONS).0)
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

/// How an attempt to start a recorder failed.
#[derive(Debug)]
pub(super) enum Failure {
    /// It ended within the start check: it can't record with this codec (no encoder, or a
    /// container that refuses it), so the next one is tried.
    Ended(CaptureError),
    /// Anything else, which another codec doesn't help with.
    Other(CaptureError),
}

/// Starts a recording of `target` with `recorder`, detached, marked with the notification
/// `notification`, into a new file `name` (from [`file_name`]) in `folder`, and returns the
/// file. wf-recorder is tried with each of [`CODECS`] in turn until one still runs a moment
/// later. Each attempt makes its file empty with `create_new` first, so no other file is ever
/// written over, with the extension its codec needs ([`with_container`]), numbered if a file of
/// that name exists, and removes it if the recorder ended. Makes the folder.
///
/// # Blocking
///
/// Waits 0.7 s for each attempt (at most 2.1 s), to see that the recorder still runs.
///
/// # Errors
///
/// [`CaptureError::Missing`] without the recorder, [`CaptureError::Write`] if the folder or the
/// file can't be made, [`CaptureError::Failed`] if it can't start, or ended at once with every
/// codec, with the last line of its log of the last attempt.
pub fn record(
    recorder: Program,
    target: &Target,
    folder: &Path,
    name: &str,
    audio: bool,
    notification: u32,
) -> Result<PathBuf, CaptureError> {
    record_with(recorder, target, folder, name, audio, |args| {
        start(recorder, args, notification)
    })
}

/// [`record`], with `start` to start the recorder with its arguments.
///
/// # Blocking
///
/// Waits for `setsid` to start the recorder.
pub(super) fn record_with(
    recorder: Program,
    target: &Target,
    folder: &Path,
    name: &str,
    audio: bool,
    mut start: impl FnMut(&[OsString]) -> Result<(), Failure>,
) -> Result<PathBuf, CaptureError> {
    let codecs = codecs(recorder);
    let attempts: Vec<Option<Codec>> = if codecs.is_empty() {
        vec![None]
    } else {
        codecs.iter().copied().map(Some).collect()
    };
    let mut ended = None;
    for codec in attempts {
        let name = with_container(name, codec);
        let file =
            super::name::write_new(folder, &name, &[]).map_err(|error| CaptureError::Write {
                path: folder.join(&name),
                error: error.to_string(),
            })?;
        let failure = match start(&args(recorder, target, &file, audio, codec)) {
            Ok(()) => return Ok(file),
            Err(failure) => failure,
        };
        // Only the file this attempt made goes.
        let _ = std::fs::remove_file(&file);
        match failure {
            Failure::Ended(error) => ended = Some(error),
            Failure::Other(error) => return Err(error),
        }
    }
    Err(ended.unwrap_or_else(|| CaptureError::Failed {
        program: recorder.name.to_owned(),
        message: String::new(),
    }))
}

/// Starts `recorder` with `args`, detached, marked with the notification `notification`, and
/// checks a moment later that it still runs.
///
/// # Blocking
///
/// Waits 0.7 s.
fn start(recorder: Program, args: &[OsString], notification: u32) -> Result<(), Failure> {
    let path = recorder.require().map_err(Failure::Other)?;
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
        .map_err(|error| Failure::Other(failed(error.to_string())))?;
    if !status.success() {
        return Err(Failure::Other(failed(status.to_string())));
    }
    // A recorder that can't record (no encoder, a container that refuses it, a file it can't
    // write) ends at once.
    std::thread::sleep(Duration::from_millis(700));
    if running().is_empty() {
        let said = log
            .and_then(|log| std::fs::read(log).ok())
            .map(|text| super::tools::last_line(&text))
            .unwrap_or_default();
        return Err(Failure::Ended(failed(said)));
    }
    Ok(())
}

/// Stops `recording` with `SIGINT`, as Ctrl+C would, and waits until the recorder ended (at most
/// ten seconds), so its file is complete. Returns whether it ended.
///
/// # Blocking
///
/// Waits until the recorder ended, at most ten seconds.
pub fn stop(recording: &Recording) -> bool {
    process::signal(&[recording.pid], "INT");
    let start = Instant::now();
    while process::runs(recording.pid) && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    !process::runs(recording.pid)
}
