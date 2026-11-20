// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The outputs (displays) of the session: what is connected, how it is set up now, what comes and
//! goes, and which outputs only have stored settings.
//!
//! [`Outputs`] is the list, built from `GET_OUTPUTS`. [`OutputWatch`] keeps it current: it reads
//! the outputs again whenever sway sends an `output` event, and never polls. Outputs that aren't
//! connected but have values in the settings files stay listed, so their values can still be seen
//! and removed.
//!
//! Settings are stored for an output by its [identifier](Output::identifier): make, model and
//! serial as sway joins them (`Samsung Electric Company LF27T850 H4LR100370`), which stays the
//! same when a dock or another port gives the output another name (`DP-4`). An output without a
//! serial is stored by its name: two monitors of one model couldn't be told apart otherwise.
//! Every one of these strings is external: they are shown, and only reach a command through
//! [`OutputSelector::output`](crate::command::output::OutputSelector::output).
//!
//! ```no_run
//! use sway_things_core::ipc::Client;
//! use sway_things_core::outputs::OutputWatch;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let sway = Client::connect().await?;
//! let mut watch = OutputWatch::start(&sway).await?;
//! for output in watch.outputs().iter() {
//!     println!("{} ({})", output.label(), output.name());
//! }
//! while let Some(change) = watch.changed().await {
//!     println!("{:?}", change?);
//! }
//! # Ok(())
//! # }
//! ```

pub mod hooks;
pub mod kanshi;
pub mod kanshi_config;
pub mod layout;
pub mod mirror;
pub mod modes;
pub mod profiles;
#[cfg(test)]
mod tests;
pub mod workspaces;

use std::collections::{BTreeSet, VecDeque};

use crate::command::output::{Mode, Transform};
use crate::devices::Lockout;
use crate::ipc::types::{self, Event, EventType};
use crate::ipc::{Client, Events, IpcError};
use crate::registry::{Scope, Value};
use crate::state::State;

pub use crate::registry::output::{ENABLED, MODE, POSITION, SCALE, TRANSFORM};

/// The command the generated config runs at every start and reload of sway while Sway Things
/// stores anything for an output: `exec_always sway-things hook displays` ([`ensure_one_on`]).
pub fn guard_command() -> crate::command::SwayCmd {
    crate::command::hook("displays")
}

/// Switches an output on if none is on but one is connected, so an output stored as off never
/// leaves the user without a display: a laptop's panel, switched off at the dock, when sway
/// starts without the dock. The first connected output in sway's order is switched on, for this
/// session only; nothing is stored. Returns the name of the output switched on, or `None` if one
/// is on, comes on, or none comes.
///
/// While none is on, it waits until sway has reported no change of its outputs for
/// [`QUIET`]: at a login at the dock, sway reports the panel (off) before the dock's monitors,
/// which come a few seconds later, and switching the panel on meanwhile would undo what the user
/// stored. While sway reports no output at all, it waits for at most [`WAIT_FOR_OUTPUTS`]; while
/// sway keeps reporting changes, it switches one on after that time anyway. It ends with sway.
///
/// # Errors
///
/// The errors of [`Client::subscribe`], [`Client::outputs`] and [`Client::run`].
pub async fn ensure_one_on(client: &mut Client) -> Result<Option<String>, IpcError> {
    // First the subscription, so an output that comes meanwhile isn't missed.
    let events = client.subscribe(&[EventType::Output]).await?;
    // One wait for the next event across the loop: a wait given up halfway through reading an
    // event would leave the rest of it in the stream.
    let mut next = Box::pin(next_output_event(events));
    let started = std::time::Instant::now();
    let mut quiet = false;
    loop {
        let outputs = Outputs::new(&client.outputs().await?);
        if outputs.enabled().next().is_some() {
            return Ok(None);
        }
        let usable = outputs.connected.iter().find_map(|output| {
            let selector = crate::command::output::OutputSelector::output(&output.name).ok()?;
            Some((output.name.clone(), selector))
        });
        let Some((name, selector)) = usable else {
            // Nothing connected (any more): the outputs that come must settle again.
            quiet = false;
            let Some(left) = WAIT_FOR_OUTPUTS.checked_sub(started.elapsed()) else {
                return Ok(None);
            };
            match wait_for(&mut next, left).await? {
                Some(events) => next = Box::pin(next_output_event(events)),
                // sway is gone, or no output came at all.
                None => return Ok(None),
            }
            continue;
        };
        // sway reports changes all the time: it has had enough time to settle.
        if quiet || started.elapsed() >= WAIT_FOR_OUTPUTS {
            client
                .run(&crate::command::output::Output::new(selector).enable(true))
                .await?;
            return Ok(Some(name));
        }
        // Wait for sway to settle: a quiet moment without changes of its outputs.
        let timer = async {
            async_io::Timer::after(QUIET).await;
            Waited::Quiet
        };
        let event = async { Waited::Changed((&mut next).await) };
        match futures_lite::future::or(event, timer).await {
            Waited::Quiet => quiet = true,
            Waited::Changed((_, Ok(false))) => return Ok(None),
            Waited::Changed((_, Err(error))) => return Err(error),
            Waited::Changed((events, Ok(true))) => next = Box::pin(next_output_event(events)),
        }
    }
}

/// The next `output` event of `events`, then `events` again: `Ok(false)` when sway is gone.
async fn next_output_event(mut events: Events) -> (Events, Result<bool, IpcError>) {
    loop {
        let result = match events.next().await {
            None => Ok(false),
            Some(Ok(Event::Output(_))) => Ok(true),
            Some(Ok(_) | Err(IpcError::InvalidReply { .. })) => continue,
            Some(Err(error)) => Err(error),
        };
        return (events, result);
    }
}

/// Waits for `next` for at most `wait`: the events back after an `output` event, `None` when
/// sway is gone or the time is up.
async fn wait_for(
    next: &mut std::pin::Pin<
        Box<impl std::future::Future<Output = (Events, Result<bool, IpcError>)>>,
    >,
    wait: std::time::Duration,
) -> Result<Option<Events>, IpcError> {
    let timer = async {
        async_io::Timer::after(wait).await;
        Waited::Quiet
    };
    let event = async { Waited::Changed(next.await) };
    match futures_lite::future::or(event, timer).await {
        Waited::Quiet | Waited::Changed((_, Ok(false))) => Ok(None),
        Waited::Changed((_, Err(error))) => Err(error),
        Waited::Changed((events, Ok(true))) => Ok(Some(events)),
    }
}

/// How waiting for sway's outputs to settle ended.
enum Waited {
    /// sway reported something: the events back and whether it was a change of its outputs.
    Changed((Events, Result<bool, IpcError>)),
    /// Nothing changed for the time waited.
    Quiet,
}

/// How long [`ensure_one_on`] waits for sway to report an output at all.
pub const WAIT_FOR_OUTPUTS: std::time::Duration = std::time::Duration::from_secs(60);
/// How long sway's outputs must stay unchanged before [`ensure_one_on`] switches one on.
pub const QUIET: std::time::Duration = std::time::Duration::from_secs(3);

/// Whether kanshi runs for this session: a process named `kanshi` of the same user, on the same
/// Wayland display as this one (`XDG_RUNTIME_DIR` and `WAYLAND_DISPLAY` in its environment), or
/// any kanshi of the user when this process has no Wayland display (the command line on a
/// TTY). Reads `/proc`; nothing is run and nothing is written.
pub fn kanshi_runs() -> bool {
    !session_processes("kanshi").is_empty()
}

/// Whether a kanshi of this session runs as a unit of its own (a systemd user service, say):
/// its control group is named after it. One that sway started stands in the group of the
/// session. Reads `/proc`; nothing is run.
pub fn kanshi_runs_as_unit() -> bool {
    session_processes("kanshi").iter().any(|process| {
        std::fs::read_to_string(process.join("cgroup")).is_ok_and(|groups| {
            groups
                .lines()
                .filter_map(|line| line.rsplit('/').next())
                .any(|unit| unit.contains("kanshi"))
        })
    })
}

/// The processes named `name` of this user in this session (the same `XDG_RUNTIME_DIR` and
/// `WAYLAND_DISPLAY` in their environment, as far as this process has them): their `/proc`
/// directories. Reads `/proc`; nothing is run.
pub(crate) fn session_processes(name: &str) -> Vec<std::path::PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let Ok(me) = std::fs::metadata("/proc/self").map(|meta| meta.uid()) else {
        return Vec::new();
    };
    let ours = ["XDG_RUNTIME_DIR", "WAYLAND_DISPLAY"].map(|variable| {
        std::env::var_os(variable).map(|value| {
            let mut entry = std::ffi::OsString::from(format!("{variable}="));
            entry.push(value);
            entry.into_encoded_bytes()
        })
    });
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    processes
        .filter_map(Result::ok)
        .filter(|process| {
            let path = process.path();
            let mine = process.metadata().is_ok_and(|meta| meta.uid() == me);
            let named = mine
                && std::fs::read_to_string(path.join("comm"))
                    .is_ok_and(|comm| comm.trim_end() == name);
            named
                && std::fs::read(path.join("environ")).is_ok_and(|environ| {
                    ours.iter().flatten().all(|entry| {
                        environ
                            .split(|byte| *byte == 0)
                            .any(|variable| variable == entry.as_slice())
                    })
                })
        })
        .map(|process| process.path())
        .collect()
}

/// A lock of the session, so two hooks (or a hook and the app) don't do one thing at once: a
/// file in `$XDG_RUNTIME_DIR`, made only if it isn't there, holding a token of its own (the
/// process number and a stamp). It is removed when dropped, only while it still holds that token.
/// One whose process is gone, or that is older than [`SessionLock::STALE`], was left behind and is
/// taken over: moved aside first (only one process can move a file), and put back if what was
/// moved was another, newer lock.
#[derive(Debug)]
pub struct SessionLock {
    path: std::path::PathBuf,
    token: String,
}

impl SessionLock {
    /// How old a lock may get before it counts as left behind, whoever holds it.
    pub const STALE: std::time::Duration = std::time::Duration::from_secs(600);

    /// Takes the lock named `name`; `None` while another process holds it, or without
    /// `$XDG_RUNTIME_DIR`.
    pub fn take(name: &str) -> Option<Self> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from)?;
        Self::take_in(&dir, name)
    }

    /// Takes the lock named `name`, waiting for it while another process holds it, at most
    /// `wait`. `None` if it wasn't free by then, or without `$XDG_RUNTIME_DIR`.
    pub async fn wait_for(name: &str, wait: std::time::Duration) -> Option<Self> {
        let until = std::time::Instant::now() + wait;
        loop {
            if let Some(lock) = Self::take(name) {
                return Some(lock);
            }
            if std::env::var_os("XDG_RUNTIME_DIR").is_none() || std::time::Instant::now() >= until {
                return None;
            }
            async_io::Timer::after(std::time::Duration::from_millis(100)).await;
        }
    }

    /// [`Self::take`] with the lock in `dir`.
    pub(crate) fn take_in(dir: &std::path::Path, name: &str) -> Option<Self> {
        use std::io::Write as _;
        static TAKEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = dir.join(name);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let count = TAKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let token = format!("{} {stamp} {count}", std::process::id());
        // Twice: once more after a lock left behind was moved aside.
        for _ in 0..2 {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                if file.write_all(token.as_bytes()).is_err() {
                    let _ = std::fs::remove_file(&path);
                    return None;
                }
                return Some(Self { path, token });
            }
            let held = std::fs::read_to_string(&path).ok()?;
            if !Self::left_behind(&path, &held) {
                return None;
            }
            let aside = dir.join(format!("{name}.{}", token.replace(' ', "-")));
            if std::fs::rename(&path, &aside).is_err() {
                // Someone else moved it first.
                continue;
            }
            let moved = std::fs::read_to_string(&aside).unwrap_or_default();
            if moved != held {
                // Another process took it over and made a new lock meanwhile: put back (never
                // over a lock that is there again).
                let _ = std::fs::hard_link(&aside, &path);
                let _ = std::fs::remove_file(&aside);
                return None;
            }
            let _ = std::fs::remove_file(&aside);
        }
        None
    }

    /// Whether the lock at `path` holding `held` was left behind: its process is gone, or it is
    /// older than [`Self::STALE`] (or empty and older than ten seconds: its process ended before
    /// it wrote its token).
    fn left_behind(path: &std::path::Path, held: &str) -> bool {
        let age = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .unwrap_or_default();
        if age > Self::STALE {
            return true;
        }
        match held.split(' ').next().filter(|pid| !pid.is_empty()) {
            Some(pid) => {
                pid.bytes().all(|byte| byte.is_ascii_digit())
                    && !std::path::Path::new("/proc").join(pid).exists()
            }
            None => age > std::time::Duration::from_secs(10),
        }
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        // Only while it is still ours: a lock taken over meanwhile belongs to someone else.
        if std::fs::read_to_string(&self.path).is_ok_and(|held| held == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// sway writes an output's identifier into a buffer of 128 bytes, so a longer one is cut off and
/// can't be matched reliably.
const MAX_IDENTIFIER: usize = 127;

/// A rectangle of the layout, in logical pixels: the size of the mode, turned by the transform and
/// divided by the scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    /// The left edge.
    pub x: i32,
    /// The top edge.
    pub y: i32,
    /// The width.
    pub width: i32,
    /// The height.
    pub height: i32,
}

impl Rect {
    /// The right edge: the first column right of the rectangle.
    pub fn right(self) -> i32 {
        self.x.saturating_add(self.width)
    }

    /// The bottom edge: the first row below the rectangle.
    pub fn bottom(self) -> i32 {
        self.y.saturating_add(self.height)
    }
}

/// An output.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    name: String,
    identifier: String,
    make: String,
    model: String,
    serial: String,
    connected: bool,
    enabled: bool,
    focused: bool,
    modes: Vec<Mode>,
    mode: Option<Mode>,
    rect: Option<Rect>,
    scale: Option<f64>,
    transform: Option<Transform>,
}

impl Output {
    /// A connected output, from sway's description of it. Its identifier is filled in by
    /// [`Outputs::update`], which knows the others.
    fn connected(output: &types::Output) -> Self {
        let mode = |mode: &types::Mode| {
            let size = |pixels: i32| u32::try_from(pixels).ok();
            Mode::new(
                size(mode.width)?,
                size(mode.height)?,
                u32::try_from(mode.refresh).ok(),
            )
            .ok()
        };
        let mut modes: Vec<Mode> = Vec::with_capacity(output.modes.len());
        for found in output.modes.iter().filter_map(mode) {
            if !modes.contains(&found) {
                modes.push(found);
            }
        }
        let enabled = output.active;
        Self {
            name: output.name.clone(),
            identifier: String::new(),
            make: output.make.clone(),
            model: output.model.clone(),
            serial: output.serial.clone(),
            connected: true,
            enabled,
            focused: output.focused,
            modes,
            mode: output
                .current_mode
                .as_ref()
                .and_then(mode)
                .filter(|_| enabled),
            rect: enabled.then_some(Rect {
                x: output.rect.x,
                y: output.rect.y,
                width: output.rect.width,
                height: output.rect.height,
            }),
            // sway keeps the scale as a 32-bit float: 1.3 is reported as 1.2999999523. To six
            // places, it is the scale that was set again.
            scale: output
                .scale
                .filter(|scale| enabled && *scale > 0.0)
                .map(|scale| (scale * 1e6).round() / 1e6),
            transform: output
                .transform
                .as_deref()
                .and_then(Transform::from_name)
                .filter(|_| enabled),
        }
    }

    /// An output that isn't connected, of which only the identifier in the settings files is
    /// known.
    fn disconnected(identifier: &str) -> Self {
        Self {
            name: identifier.to_owned(),
            identifier: identifier.to_owned(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            connected: false,
            enabled: false,
            focused: false,
            modes: Vec::new(),
            mode: None,
            rect: None,
            scale: None,
            transform: None,
        }
    }

    /// The make, model and serial as sway joins them to its identifier, if that fits sway's buffer.
    fn sway_identifier(&self) -> Option<String> {
        let identifier = format!("{} {} {}", self.make, self.model, self.serial);
        (identifier.len() <= MAX_IDENTIFIER).then_some(identifier)
    }

    /// The name sway gives the output, on DRM its connector (`DP-4`). It can change with the port
    /// or the dock. For an output that isn't connected, its identifier.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What settings are stored for and commands select the output by: make, model and serial
    /// (`Dell Inc. DELL U2723QE ABC123`), or its name if it reports no serial, another connected
    /// output has the same identifier, or it is too long for sway.
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// The name to show: make and model, without the serial, or for an output that isn't
    /// connected, its identifier. sway reports a missing make or model as `Unknown`.
    pub fn label(&self) -> String {
        if !self.connected {
            return self.identifier.clone();
        }
        let known = |part: &str| !part.is_empty() && part != "Unknown";
        match (known(&self.make), known(&self.model)) {
            (true, true) => format!("{} {}", self.make, self.model),
            (true, false) => self.make.clone(),
            (false, true) => self.model.clone(),
            (false, false) => self.name.clone(),
        }
    }

    /// Whether the output is a laptop's or tablet's own panel, by its connector (eDP, LVDS, DSI).
    pub fn is_built_in(&self) -> bool {
        ["eDP-", "LVDS-", "DSI-"]
            .iter()
            .any(|prefix| self.name.starts_with(prefix))
    }

    /// The brand people know the make by: its first word (_Samsung_ for "Samsung Electric
    /// Company"), or for makes that EDID names after the company behind the brand, the brand
    /// (_LG_ for "Goldstar"). `None` if the output doesn't report its make.
    pub fn brand(&self) -> Option<String> {
        if self.make.is_empty() || self.make == "Unknown" {
            return None;
        }
        let first = self
            .make
            .split(|c: char| c.is_whitespace() || c == ',')
            .find(|word| !word.is_empty())
            .unwrap_or(&self.make)
            .trim_end_matches('.');
        let known = [
            ("Goldstar", "LG"),
            ("Ancor", "ASUS"),
            ("Hewlett", "HP"),
            ("Microstep", "MSI"),
            ("Lenovo", "Lenovo"),
            ("BenQ", "BenQ"),
        ];
        Some(
            known
                .iter()
                .find(|(company, _)| first.eq_ignore_ascii_case(company))
                .map_or_else(|| first.to_owned(), |(_, brand)| (*brand).to_owned()),
        )
    }

    /// The make, as sway reports it (`Unknown` if the output doesn't say).
    pub fn make(&self) -> &str {
        &self.make
    }

    /// The model, as sway reports it.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The serial number, as sway reports it.
    pub fn serial(&self) -> &str {
        &self.serial
    }

    /// Whether the output is connected. If not, it is listed for its stored settings.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Whether the output is connected and switched on: it is part of the layout.
    pub fn is_enabled(&self) -> bool {
        self.connected && self.enabled
    }

    /// Whether the output has the focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// The modes the output offers, in sway's order, each once.
    pub fn modes(&self) -> &[Mode] {
        &self.modes
    }

    /// The mode in use, while the output is on.
    pub fn mode(&self) -> Option<Mode> {
        self.mode
    }

    /// Where the output is in the layout and how big, in logical pixels, while it is on.
    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    /// The scale in use, while the output is on.
    pub fn scale(&self) -> Option<f64> {
        self.scale
    }

    /// The transform in use, while the output is on.
    pub fn transform(&self) -> Option<Transform> {
        self.transform
    }

    /// What the output runs with now for the Displays setting `key`, as the setting stores it:
    /// what sway decided where nothing sets it (the preferred mode, an automatic position or
    /// scale). `None` for another key, and for an output that is off or not connected, except
    /// whether it is on.
    pub fn session_value(&self, key: &str) -> Option<Value> {
        if key == ENABLED {
            return self.connected.then_some(Value::Bool(self.enabled));
        }
        if !self.is_enabled() {
            return None;
        }
        match key {
            MODE => self.mode.map(|mode| Value::Text(mode.to_string())),
            POSITION => self
                .rect
                .map(|rect| Value::Text(format!("{},{}", rect.x, rect.y))),
            SCALE => self.scale.map(Value::Decimal),
            TRANSFORM => self
                .transform
                .map(|transform| Value::Choice(transform.name().to_owned())),
            _ => None,
        }
    }
}

/// How the list changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// An output was connected. The identifier is new in the list, or was listed as not
    /// connected.
    Added(String),
    /// An output was disconnected. It is gone from the list, or stays as not connected if it has
    /// stored settings.
    Removed(String),
    /// Something about a connected output changed: on or off, its mode, place, scale, transform
    /// or focus.
    Changed(String),
}

impl Change {
    /// The identifier of the output.
    pub fn identifier(&self) -> &str {
        match self {
            Self::Added(identifier) | Self::Removed(identifier) | Self::Changed(identifier) => {
                identifier
            }
        }
    }
}

/// The outputs: the connected ones in sway's order (on before off), then those that only have
/// stored settings, by identifier. Outputs that aren't desktops (VR headsets) are left out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outputs {
    connected: Vec<Output>,
    /// The identifiers of the outputs with stored settings.
    stored: BTreeSet<String>,
    disconnected: Vec<Output>,
}

impl Outputs {
    /// The outputs of a reply to `GET_OUTPUTS`.
    pub fn new(outputs: &[types::Output]) -> Self {
        let mut list = Self::default();
        list.update(outputs);
        list
    }

    /// Replaces the connected outputs with those of a newer reply to `GET_OUTPUTS` and returns
    /// what changed: first the outputs that are gone, then the others in sway's order.
    pub fn update(&mut self, outputs: &[types::Output]) -> Vec<Change> {
        let mut connected: Vec<Output> = outputs
            .iter()
            .filter(|output| !output.non_desktop)
            .map(Output::connected)
            .collect();
        assign_identifiers(&mut connected);
        let mut changes: Vec<Change> = self
            .connected
            .iter()
            .filter(|old| !connected.iter().any(|new| new.identifier == old.identifier))
            .map(|old| Change::Removed(old.identifier.clone()))
            .collect();
        for new in &connected {
            match self
                .connected
                .iter()
                .find(|old| old.identifier == new.identifier)
            {
                None => changes.push(Change::Added(new.identifier.clone())),
                Some(old) if old != new => changes.push(Change::Changed(new.identifier.clone())),
                Some(_) => {}
            }
        }
        self.connected = connected;
        self.list_disconnected();
        changes
    }

    /// Lists the outputs that have values in `state` and aren't connected, and drops those whose
    /// values are gone. Call it after the state changed. Returns whether the list changed.
    pub fn set_stored(&mut self, state: &State) -> bool {
        let stored: BTreeSet<String> = state
            .entries()
            .into_iter()
            .filter(|entry| entry.setting.scope() == Scope::PerOutput)
            .filter_map(|entry| entry.device.map(ToOwned::to_owned))
            .collect();
        if stored == self.stored {
            return false;
        }
        self.stored = stored;
        let before = std::mem::take(&mut self.disconnected);
        self.list_disconnected();
        before != self.disconnected
    }

    fn list_disconnected(&mut self) {
        self.disconnected = self
            .stored
            .iter()
            .filter(|identifier| !self.is_connected(identifier))
            .map(|identifier| Output::disconnected(identifier))
            .collect();
    }

    fn is_connected(&self, identifier: &str) -> bool {
        self.connected
            .iter()
            .any(|output| output.identifier == identifier)
    }

    /// Every output: the connected ones in sway's order, then those that only have stored
    /// settings.
    pub fn iter(&self) -> impl Iterator<Item = &Output> {
        self.connected.iter().chain(&self.disconnected)
    }

    /// The output with this identifier.
    pub fn get(&self, identifier: &str) -> Option<&Output> {
        self.iter().find(|output| output.identifier == identifier)
    }

    /// The connected output with this name or identifier, as a line of the user's config or a
    /// command names it. sway compares both exactly.
    pub fn find(&self, name: &str) -> Option<&Output> {
        self.connected
            .iter()
            .find(|output| output.name == name || output.sway_identifier().as_deref() == Some(name))
    }

    /// The outputs that are on, in sway's order: the layout.
    pub fn enabled(&self) -> impl Iterator<Item = &Output> {
        self.connected.iter().filter(|output| output.enabled)
    }

    /// Whether switching off the output `identifier` would leave no output on.
    pub fn is_last_enabled(&self, identifier: &str) -> bool {
        let enabled: Vec<&Output> = self.enabled().collect();
        matches!(enabled.as_slice(), [only] if only.identifier == identifier)
    }

    /// What switching every output (`output` is `None`) or the output `output` (by its identifier
    /// or name) on or off would leave the user without: the last output that is on. `None` if
    /// one stays on, if the change switches on, or if none was on before (a session without
    /// outputs, which this change doesn't make worse).
    pub fn lockout(&self, output: Option<&str>, enabled: bool) -> Option<Lockout> {
        if enabled {
            return None;
        }
        let before = self.enabled().count();
        let after = self
            .enabled()
            .filter(|existing| {
                output.is_some_and(|name| {
                    existing.identifier != name
                        && existing.name != name
                        && existing.sway_identifier().as_deref() != Some(name)
                })
            })
            .count();
        (before > 0 && after == 0).then_some(Lockout::Display)
    }

    /// What switching every output (`output` is `None`) or one output on or off could leave the
    /// user without, when it isn't known which outputs are connected: switching off all of them
    /// leaves none, switching off one leaves the others.
    pub fn lockout_unknown(output: Option<&str>, enabled: bool) -> Option<Lockout> {
        (output.is_none() && !enabled).then_some(Lockout::Display)
    }
}

/// Gives each output of `connected` the identifier settings are stored by: make, model and serial
/// as sway joins them, or its name
///
/// - without a serial: two monitors of one model would share the identifier, and which one is
///   plugged in would decide what it stands for;
/// - while another connected output has the same identifier (a broken serial; once one of them
///   is gone, the other goes back to the identifier);
/// - if the identifier is longer than sway keeps.
fn assign_identifiers(connected: &mut [Output]) {
    let identifiers: Vec<Option<String>> = connected
        .iter()
        .map(|output| {
            let serial = output.serial.trim();
            let known = !serial.is_empty() && serial != "Unknown";
            output.sway_identifier().filter(|_| known)
        })
        .collect();
    for (index, output) in connected.iter_mut().enumerate() {
        let own = identifiers.get(index).cloned().flatten();
        let shared = own.as_ref().is_some_and(|own| {
            identifiers
                .iter()
                .enumerate()
                .any(|(other, identifier)| other != index && identifier.as_ref() == Some(own))
        });
        output.identifier = match own {
            Some(identifier) if !shared => identifier,
            _ => output.name.clone(),
        };
    }
}

/// The output list of a session, kept current with sway's `output` events.
#[derive(Debug)]
pub struct OutputWatch {
    outputs: Outputs,
    events: Events,
    /// A connection of its own, to read the outputs after each event.
    client: Client,
    /// Changes that weren't returned yet.
    pending: VecDeque<Change>,
}

impl OutputWatch {
    /// Subscribes to the `output` events of the session of `client` and reads its outputs, on a
    /// connection of its own.
    ///
    /// # Errors
    ///
    /// The errors of [`Client::connect_to`], [`Client::subscribe`] and [`Client::outputs`].
    pub async fn start(client: &Client) -> Result<Self, IpcError> {
        let mut client = Client::connect_to(client.socket()).await?;
        // First the subscription, so an output that is plugged in meanwhile isn't missed.
        let events = client.subscribe(&[EventType::Output]).await?;
        let outputs = Outputs::new(&client.outputs().await?);
        Ok(Self {
            outputs,
            events,
            client,
            pending: VecDeque::new(),
        })
    }

    /// The outputs as of the last event.
    pub fn outputs(&self) -> &Outputs {
        &self.outputs
    }

    /// Lists the outputs that only have values in `state`; see [`Outputs::set_stored`].
    pub fn set_stored(&mut self, state: &State) -> bool {
        self.outputs.set_stored(state)
    }

    /// Waits until the list changes and returns how; several changes at once come one by one.
    /// `None` once sway closed the connection, which it does when it exits.
    ///
    /// sway sends an `output` event without details for every change of the layout, so the
    /// outputs are read again and only real changes are returned.
    ///
    /// # Errors
    ///
    /// [`IpcError::InvalidReply`] for an event or an output list that can't be read; the list may
    /// be out of date then, and the events after it still come. After [`IpcError::Io`] or
    /// [`IpcError::Protocol`] there are no more events.
    pub async fn changed(&mut self) -> Option<Result<Change, IpcError>> {
        loop {
            if let Some(change) = self.pending.pop_front() {
                return Some(Ok(change));
            }
            match self.events.next().await? {
                Ok(Event::Output(_)) => match self.client.outputs().await {
                    Ok(outputs) => self.pending.extend(self.outputs.update(&outputs)),
                    Err(error) => return Some(Err(error)),
                },
                Ok(_) => {}
                Err(error) => return Some(Err(error)),
            }
        }
    }
}
