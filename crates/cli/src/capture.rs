// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `screenshot`, `record` and `clipboard`: what the keys of the Screenshots & Clipboard page run
//! (`hook clipboard` is in [`crate::hook`]).
//!
//! They load only the settings files (not the user's sway config) and ask sway only what they
//! need, so a key press is quick. A key press has no terminal to show an error in, so errors also
//! go to a notification; a selection canceled with Escape is no error.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sway_things_core::capture::notify::{Notification, Notifier};
use sway_things_core::capture::shot::{self, Area, Shot};
use sway_things_core::capture::tools::{self, SATTY, SWAPPY};
use sway_things_core::capture::{
    Afterwards, CaptureError, Settings, clipboard as history, open_file, record as rec,
};
use sway_things_core::i18n::{Msg, tr, trf};
use sway_things_core::msg;
use sway_things_core::outputs::hooks;
use sway_things_core::session::process::{Lock, LockError};

use crate::{Exit, error_line};

/// How long a notification with buttons is listened to before the command ends.
const LISTEN: Duration = Duration::from_secs(120);

/// What `screenshot` captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ShotArea {
    /// A region you select, or a window you click
    Region,
    /// The focused window
    Window,
    /// The display with the focus
    Display,
    /// Every display
    All,
}

impl From<ShotArea> for Area {
    fn from(area: ShotArea) -> Self {
        match area {
            ShotArea::Region => Self::Region,
            ShotArea::Window => Self::Window,
            ShotArea::Display => Self::Display,
            ShotArea::All => Self::All,
        }
    }
}

/// What `record` records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum RecordArea {
    /// A region you select, or a window you click
    Region,
    /// The display with the focus
    Display,
}

/// What `clipboard` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::Subcommand)]
pub enum Clipboard {
    /// Show the history in the launcher that is installed (fuzzel, wofi, rofi, tofi or bemenu) and
    /// copy the entry you pick again; the history key runs this
    Pick,
    /// Remove every entry of the history
    Clear,
}

/// The settings in effect, from the settings files; the defaults if they can't be read.
fn settings() -> Settings {
    Settings::load()
}

/// `path` with the home folder as `~`.
fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Says `error` on standard error and in a notification titled `title`, unless it is a
/// canceled selection. Returns the exit status.
async fn report(title: Msg, error: &CaptureError, err: &mut dyn Write) -> Exit {
    if *error == CaptureError::Canceled {
        return Exit::Ok;
    }
    let text = error.text().translated();
    let _ = writeln!(err, "{}", error_line(&text));
    if let Ok(notifier) = Notifier::connect().await {
        let _ = notifier
            .show(&Notification {
                summary: tr(title).to_owned(),
                body: text,
                timeout: -1,
                ..Notification::default()
            })
            .await;
    }
    Exit::Failure
}

/// `screenshot`: takes the screenshot, then notifies or opens the editor.
pub async fn screenshot(area: ShotArea, err: &mut dyn Write) -> Exit {
    let settings = settings();
    let mut client = hooks::connect().await.ok();
    let taken = match shot::target(area.into(), client.as_mut()).await {
        Ok(target) => {
            // Done with sway: the connection isn't kept open while the notification waits.
            drop(client);
            shot::take(&target, &settings)
        }
        Err(error) => Err(error),
    };
    let shot = match taken {
        Ok(shot) => shot,
        Err(error) => return report(msg!("Screenshot failed"), &error, err).await,
    };
    if let Some(error) = &shot.failed {
        // Half of it worked: the notification says which, and stderr why.
        let _ = writeln!(err, "{}", error_line(&error.text().translated()));
    }
    let partial = shot.failed.is_some();
    match settings.afterwards {
        Afterwards::Nothing if !partial => Exit::Ok,
        Afterwards::Edit if !partial => match edit(&shot) {
            Ok(()) => Exit::Ok,
            Err(error) => report(msg!("The editor didn't start"), &error, err).await,
        },
        _ => {
            notify_shot(shot, err).await;
            if partial { Exit::Failure } else { Exit::Ok }
        }
    }
}

/// The notification after a screenshot: the picture, where it went (and which half of _Copy
/// and save_ didn't work), _Open_ for a saved one and _Edit_ while an editor is installed;
/// waits for a button. A saved picture isn't kept in memory meanwhile: _Edit_ opens the file.
async fn notify_shot(mut shot: Shot, err: &mut dyn Write) {
    let Ok(notifier) = Notifier::connect().await else {
        return;
    };
    let summary = match (&shot.path, shot.copied, shot.failed.is_some()) {
        (Some(_), true, _) => msg!("Screenshot saved and copied"),
        (Some(_), false, true) => msg!("Screenshot saved, but not copied"),
        (Some(_), false, false) => msg!("Screenshot saved"),
        (None, _, true) => msg!("Screenshot copied, but not saved"),
        (None, _, false) => msg!("Screenshot copied"),
    };
    let mut body = shot
        .path
        .as_deref()
        .map_or_else(|| tr(msg!("Paste it where you need it")).to_owned(), tilde);
    if let Some(error) = &shot.failed {
        body = format!("{body}\n{}", error.text().translated());
    }
    let mut actions = Vec::new();
    if shot.path.is_some() {
        actions.push(("open".to_owned(), tr(msg!("Open")).to_owned()));
        shot.bytes = Vec::new();
    }
    if tools::editor().is_some() {
        actions.push(("edit".to_owned(), tr(msg!("Edit")).to_owned()));
    }
    let notification = Notification {
        summary: tr(summary).to_owned(),
        body,
        image: shot.path.clone(),
        actions,
        timeout: -1,
        replaces: 0,
    };
    match notifier.show_and_wait(&notification, LISTEN).await {
        Ok(Some(action)) if action == "open" => {
            if let Some(path) = &shot.path {
                if let Err(error) = open_file(path) {
                    let _ = writeln!(err, "{}", error_line(&error));
                }
            }
        }
        Ok(Some(action)) if action == "edit" => {
            if let Err(error) = edit(&shot) {
                report(msg!("The editor didn't start"), &error, err).await;
            }
        }
        _ => {}
    }
}

/// Opens the screenshot in swappy or satty: the saved file, which the editor saves over, or the
/// picture on its standard input, which the editor copies.
fn edit(shot: &Shot) -> Result<(), CaptureError> {
    let editor = tools::editor().ok_or(CaptureError::Missing {
        program: SWAPPY.name,
        package: SWAPPY.package,
    })?;
    let args: Vec<OsString> = match (&shot.path, editor == SATTY) {
        (Some(path), false) => vec!["-f".into(), path.into(), "-o".into(), path.into()],
        (None, false) => vec!["-f".into(), "-".into()],
        (Some(path), true) => {
            // satty fills `%` conversions into the output's name.
            let output = path.to_string_lossy().replace('%', "%%");
            vec![
                "--filename".into(),
                path.into(),
                "--output-filename".into(),
                output.into(),
                "--copy-command".into(),
                "wl-copy".into(),
            ]
        }
        (None, true) => vec![
            "--filename".into(),
            "-".into(),
            "--copy-command".into(),
            "wl-copy".into(),
        ],
    };
    spawn_with_input(editor, &args, shot.path.is_none().then_some(&shot.bytes))
}

/// Starts `program` with `args` and `input` on its standard input, and leaves it running.
fn spawn_with_input(
    program: tools::Program,
    args: &[OsString],
    input: Option<&Vec<u8>>,
) -> Result<(), CaptureError> {
    let path = program.require()?;
    let failed = |message: String| CaptureError::Failed {
        program: program.name.to_owned(),
        message,
    };
    let mut child = Command::new(path)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| failed(error.to_string()))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin
            .write_all(input)
            .map_err(|error| failed(error.to_string()))?;
    }
    // Not waited for: it runs on after this command ends, as its own program.
    drop(child);
    Ok(())
}

/// `record`: stops the recording that runs, or starts one. A lock of the session is held from
/// looking for a recording until a new one runs, so a second press meanwhile starts none.
pub async fn record(area: Option<RecordArea>, err: &mut dyn Write) -> Exit {
    let lock = match Lock::take("record", Duration::ZERO) {
        Ok(lock) => lock,
        // Another press is busy with it: this one does nothing.
        Err(LockError::Busy) => return report(msg!("Recording"), &CaptureError::Busy, err).await,
        Err(LockError::NoRuntimeDir) => {
            let error = CaptureError::Sway(msg!("Not in a sway session").into());
            return report(msg!("The recording didn't start"), &error, err).await;
        }
    };
    let running = rec::running();
    if !running.is_empty() {
        let stopped: Vec<(rec::Recording, bool)> = running
            .into_iter()
            .map(|recording| {
                let ended = rec::stop(&recording);
                (recording, ended)
            })
            .collect();
        // Stopped: another press may start a new one while this one shows where the file is.
        drop(lock);
        for (recording, ended) in &stopped {
            notify_stopped(recording, *ended).await;
        }
        return Exit::Ok;
    }
    let settings = settings();
    let region = area.map_or(settings.record_region, |area| area == RecordArea::Region);
    let started = start_recording(&settings, region).await;
    drop(lock);
    match started {
        Ok(()) => Exit::Ok,
        Err(error) => report(msg!("The recording didn't start"), &error, err).await,
    }
}

/// Says where the file of a stopped recording is, in place of the notification that said it
/// records, with _Open_.
async fn notify_stopped(recording: &rec::Recording, ended: bool) {
    let Ok(notifier) = Notifier::connect().await else {
        return;
    };
    let file = recording.file.as_deref().filter(|file| file.exists());
    let summary = if ended {
        msg!("Recording saved")
    } else {
        msg!("The recording is still being saved")
    };
    let notification = Notification {
        summary: tr(summary).to_owned(),
        body: file.map(tilde).unwrap_or_default(),
        actions: file
            .map(|_| vec![("open".to_owned(), tr(msg!("Open")).to_owned())])
            .unwrap_or_default(),
        timeout: -1,
        replaces: recording.notification,
        image: None,
    };
    if let (Ok(Some(_)), Some(file)) = (notifier.show_and_wait(&notification, LISTEN).await, file) {
        let _ = open_file(file);
    }
}

/// Starts a recording of a region (selected with slurp) or of the focused display, into a new
/// file of the folder, and says so in a notification that goes after the usual time, so it
/// isn't recorded for long; the page shows a recording that runs, and the key stops it.
async fn start_recording(settings: &Settings, region: bool) -> Result<(), CaptureError> {
    let recorder = tools::recorder().ok_or(CaptureError::Missing {
        program: tools::WF_RECORDER.name,
        package: tools::WF_RECORDER.package,
    })?;
    let area = if region { Area::Region } else { Area::Display };
    let target = {
        let mut client = hooks::connect().await.ok();
        shot::target(area, client.as_mut()).await?
    };
    let file = rec::new_file(settings)?;
    let notifier = Notifier::connect().await.ok();
    let body = if settings.recording_keys.is_empty() {
        tr(msg!("Run sway-things record again to stop it")).to_owned()
    } else {
        trf(
            msg!("Press {keys} again to stop it"),
            &[("keys", &settings.recording_keys)],
        )
    };
    let mut id = 0;
    if let Some(notifier) = &notifier {
        // In place of a notification of an earlier recording that may still show.
        id = notifier
            .show(&Notification {
                summary: tr(msg!("Recording the screen")).to_owned(),
                body,
                timeout: -1,
                replaces: rec::last_notification(),
                ..Notification::default()
            })
            .await
            .unwrap_or(0);
        rec::remember_notification(id);
    }
    let args = rec::args(recorder, &target, &file, settings.audio);
    let started = rec::start(recorder, &args, id);
    if started.is_err() {
        // The empty file made for it goes, and so does the notification; `report` says why.
        let _ = std::fs::remove_file(&file);
        if let Some(notifier) = &notifier {
            let _ = notifier.close(id).await;
        }
    }
    started
}

/// `clipboard pick` and `clipboard clear`.
pub async fn clipboard(action: Clipboard, err: &mut dyn Write) -> Exit {
    match action {
        Clipboard::Pick => match history::pick() {
            Ok(()) => Exit::Ok,
            Err(error) => report(msg!("Clipboard history"), &error, err).await,
        },
        Clipboard::Clear => match history::clear() {
            Ok(()) => Exit::Ok,
            Err(error) => {
                let _ = writeln!(err, "{}", error_line(&error.text().translated()));
                Exit::Failure
            }
        },
    }
}
