// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The clipboard history: cliphist keeps what is copied, text and images.
//!
//! `wl-paste --watch cliphist -max-items <n> store` hands it every copy. It is the user's tool,
//! started detached for the session (`setsid -f`, like `wl-mirror`) with `SWAY_THINGS_CLIPBOARD`
//! in its environment: by `sway-things hook clipboard`, which `80-hooks.conf` runs at every start
//! and reload of sway while the history is on, and by the app when the switch changes. [`sync`]
//! makes it so that exactly the watcher the settings ask for runs, so running it twice changes
//! nothing; it ends with the session's Wayland display.
//!
//! [`pick`] shows `cliphist list` in the launcher that is installed and puts the entry picked
//! back on the clipboard (`cliphist decode`, then `wl-copy`), each a process of its own with
//! the data passed between them, without a shell or a pipe line.
//!
//! cliphist doesn't keep what is copied as sensitive: wl-paste (from wl-clipboard 2.3.0) tells
//! it with `CLIPBOARD_STATE=sensitive` when the copying app offers
//! `x-kde-passwordManagerHint`, as `KeePassXC` does, and cliphist (from 0.5.0) then stores
//! nothing ([`skips_passwords`]).

use std::ffi::{OsStr, OsString};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::tools::{self, CLIPHIST, WL_COPY, WL_PASTE};
use super::{CaptureError, HISTORY, MAX_ENTRIES, Settings};
use crate::msg;
use crate::session::process;
use crate::session::{Lock, LockError};
use crate::state::State;

/// The lock held while the watcher is started or ended.
const LOCK: &str = "sway-things-clipboard.lock";

/// The environment variable that marks the watcher Sway Things started.
pub const MARKER: &str = "SWAY_THINGS_CLIPBOARD";

/// The arguments of `wl-paste` for the history, keeping `max` entries.
pub fn watcher_args(max: u32) -> Vec<OsString> {
    [
        "--watch",
        "cliphist",
        "-max-items",
        &max.to_string(),
        "store",
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

/// Whether the arguments of a `wl-paste` are a watcher that stores into cliphist.
fn stores_to_cliphist(args: &[OsString]) -> bool {
    let has = |word: &str| args.iter().any(|arg| arg == word);
    has("--watch") && has("cliphist") && has("store")
}

/// What [`sync`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Synced {
    /// Watchers that were ended.
    pub ended: usize,
    /// Whether one was started.
    pub started: bool,
    /// The process number of a watcher of the user's own (a `wl-paste --watch cliphist store`
    /// Sway Things didn't start) that runs in this session; then Sway Things runs none.
    pub foreign: Option<u32>,
}

/// Why [`sync`] did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncError {
    /// No `XDG_RUNTIME_DIR` or no `WAYLAND_DISPLAY`: not in a session, so nothing is started or
    /// ended.
    NoSession,
    /// Another command kept the lock for longer than a few seconds.
    Busy,
}

impl SyncError {
    /// What is wrong, to show.
    pub fn text(self) -> crate::i18n::Text {
        match self {
            Self::NoSession => msg!(
                "Not in a sway session (XDG_RUNTIME_DIR or WAYLAND_DISPLAY isn't set), so the \
                 clipboard history's watcher is neither started nor ended"
            )
            .into(),
            Self::Busy => {
                msg!("Another command of Sway Things is starting the clipboard history's watcher")
                    .into()
            }
        }
    }
}

/// [`sync_with`] for the settings of `state`.
///
/// # Blocking
///
/// Waits up to three seconds for another process's change of the history to end.
///
/// # Errors
///
/// Those of [`sync_with`].
pub fn sync(state: &State) -> Result<Synced, SyncError> {
    let on = super::is_on(state, HISTORY);
    let max = Settings::of(state).max_entries;
    sync_with(|| (on, max))
}

/// Has exactly the watcher run that the settings ask for: none while the history is off (or
/// wl-paste or cliphist is missing, or a watcher of the user's own runs), else one that keeps as
/// many entries as set. Ends the others Sway Things started in this session. `settings` gives
/// the switch and the number of entries; it is read once the lock is held, so of the hook and
/// the app running at once the second acts on what is set by then.
///
/// # Blocking
///
/// Waits up to three seconds for another process's change of the history to end.
///
/// # Errors
///
/// [`SyncError::NoSession`] outside a sway session, [`SyncError::Busy`] if the lock stays held
/// for three seconds.
pub fn sync_with(settings: impl FnOnce() -> (bool, u32)) -> Result<Synced, SyncError> {
    let in_session = ["XDG_RUNTIME_DIR", "WAYLAND_DISPLAY"]
        .iter()
        .all(|variable| std::env::var_os(variable).is_some_and(|value| !value.is_empty()));
    if !in_session {
        return Err(SyncError::NoSession);
    }
    let _lock = Lock::wait(LOCK, Duration::from_secs(3)).map_err(|error| match error {
        LockError::NoRuntimeDir => SyncError::NoSession,
        LockError::Busy => SyncError::Busy,
    })?;
    let (on, max) = settings();
    let running = process::find(WL_PASTE.name, MARKER);
    let foreign = running
        .iter()
        .find(|found| found.marker.is_none() && stores_to_cliphist(&found.args))
        .map(|found| found.pid);
    let wanted = (on && foreign.is_none() && WL_PASTE.installed() && CLIPHIST.installed())
        .then(|| watcher_args(max));
    let mut synced = Synced {
        foreign,
        ..Synced::default()
    };
    let mut kept = false;
    let unwanted: Vec<u32> = running
        .into_iter()
        .filter(|found| found.marker.is_some())
        .filter(|found| {
            if !kept && wanted.as_ref() == Some(&found.args) {
                kept = true;
                false
            } else {
                true
            }
        })
        .map(|found| found.pid)
        .collect();
    synced.ended = unwanted.len();
    process::signal(&unwanted, "TERM");
    if let (Some(args), false, Some(path)) = (wanted, kept, WL_PASTE.find()) {
        synced.started = Command::new("setsid")
            .arg("-f")
            .arg(path)
            .args(args)
            .env(MARKER, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
    }
    Ok(synced)
}

/// Whether the watcher of the history Sway Things started runs.
pub fn watching() -> bool {
    !process::marked(WL_PASTE.name, MARKER).is_empty()
}

/// Whether a watcher of the user's own (`wl-paste --watch cliphist store` not started by Sway
/// Things) runs in this session.
pub fn foreign_watcher() -> Option<u32> {
    process::find(WL_PASTE.name, MARKER)
        .into_iter()
        .find(|found| found.marker.is_none() && stores_to_cliphist(&found.args))
        .map(|found| found.pid)
}

/// The setting keys whose change [`sync`] follows.
pub const KEYS: [&str; 2] = [HISTORY, MAX_ENTRIES];

/// The media type of `bytes` by their first bytes, for the pictures cliphist keeps (PNG, JPEG,
/// GIF, BMP, WebP); `None` for anything else, whose type wl-copy finds itself.
pub fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"BM") && bytes.len() > 14 {
        Some("image/bmp")
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

/// What a launcher's exit means: whether it was closed without a pick. Every supported launcher
/// ends with status 1 and says nothing then; anything else is a failure to report.
pub fn canceled(status: Option<i32>, stderr: &[u8]) -> bool {
    status == Some(1) && stderr.iter().all(u8::is_ascii_whitespace)
}

/// Runs the launcher on `list` and returns the line picked.
///
/// # Blocking
///
/// Waits until the user picks an entry in the launcher or closes it.
fn launch(launcher: tools::Launcher, list: &[u8]) -> Result<Vec<u8>, CaptureError> {
    use std::io::Write as _;
    let path = launcher.program.require()?;
    let failed = |message: String| CaptureError::Failed {
        program: launcher.program.name.to_owned(),
        message,
    };
    let mut child = Command::new(path)
        .args(launcher.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| failed(error.to_string()))?;
    let output = std::thread::scope(|scope| {
        if let Some(mut stdin) = child.stdin.take() {
            scope.spawn(move || {
                let _ = stdin.write_all(list);
            });
        }
        child.wait_with_output()
    })
    .map_err(|error| failed(error.to_string()))?;
    if output.status.success() {
        return if output.stdout.iter().all(u8::is_ascii_whitespace) {
            Err(CaptureError::Canceled)
        } else {
            Ok(output.stdout)
        };
    }
    if canceled(output.status.code(), &output.stderr) {
        return Err(CaptureError::Canceled);
    }
    let said = tools::last_line(&output.stderr);
    Err(failed(if said.is_empty() {
        output.status.to_string()
    } else {
        said
    }))
}

/// Shows the history in the launcher that is installed and copies the entry picked again, with
/// the type of a picture it has.
///
/// # Blocking
///
/// Waits until the user picks an entry in the launcher or closes it.
///
/// # Errors
///
/// [`CaptureError::Canceled`] if nothing was picked, [`CaptureError::EmptyHistory`],
/// [`CaptureError::EmptyEntry`] for an entry without data, [`CaptureError::Missing`] for
/// cliphist, wl-copy or a launcher (fuzzel is suggested), and the errors of the programs.
pub fn pick() -> Result<(), CaptureError> {
    WL_COPY.require()?;
    let launcher = tools::launcher().ok_or(CaptureError::Missing {
        program: "fuzzel",
        package: "fuzzel",
    })?;
    let list = tools::run(CLIPHIST, &[OsStr::new("list")], None)?;
    if list.iter().all(u8::is_ascii_whitespace) {
        return Err(CaptureError::EmptyHistory);
    }
    let picked = launch(launcher, &list)?;
    let bytes = tools::run(CLIPHIST, &[OsStr::new("decode")], Some(&picked))?;
    if bytes.is_empty() {
        return Err(CaptureError::EmptyEntry);
    }
    super::shot::wl_copy(&bytes, image_type(&bytes))
}

/// Removes every entry of the history (`cliphist wipe`).
///
/// # Blocking
///
/// Waits for the program to end.
///
/// # Errors
///
/// [`CaptureError::Missing`] without cliphist, else its error.
pub fn clear() -> Result<(), CaptureError> {
    tools::run(CLIPHIST, &[OsStr::new("wipe")], None).map(|_| ())
}

/// The first version number (`2.3.0`, `v0.7.0`) in what `program` prints for `args`, on
/// standard output or error.
///
/// # Blocking
///
/// Waits for the clipboard tools to print their version.
fn version(program: tools::Program, args: &[&str]) -> Option<(u32, u32, u32)> {
    let output = Command::new(program.find()?)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    parse_version(&text)
}

/// The first version number in `text`: digits and dots after a space, a `v` or a line start.
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    text.split(|c: char| c.is_whitespace() || c == '\t')
        .map(|word| word.strip_prefix('v').unwrap_or(word))
        .find_map(|word| {
            let mut parts = word.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next().map_or(Some(0), |patch| {
                patch
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|digits| digits.parse().ok())
            })?;
            Some((major, minor, patch))
        })
}

/// Whether the history skips what a password manager copies: wl-clipboard 2.3.0 or newer and
/// cliphist 0.5.0 or newer. `None` if a version can't be told (a tool missing, a version it
/// doesn't print).
///
/// # Blocking
///
/// Waits for the clipboard tools to print their version.
pub fn skips_passwords() -> Option<bool> {
    let wl = version(WL_PASTE, &["--version"])?;
    let cliphist = version(CLIPHIST, &["version"])?;
    Some(wl >= (2, 3, 0) && cliphist >= (0, 5, 0))
}

/// The first line of the user's config that starts a clipboard history of its own: an `exec` or
/// `exec_always` that runs cliphist or clipman.
pub fn own_history(
    directives: &[crate::sway_config::Directive],
) -> Option<&crate::sway_config::Directive> {
    directives.iter().find(|directive| {
        matches!(directive.name.as_str(), "exec" | "exec_always")
            && directive.block.is_empty()
            && directive
                .args
                .iter()
                .any(|arg| arg.contains("cliphist") || arg.contains("clipman"))
    })
}
