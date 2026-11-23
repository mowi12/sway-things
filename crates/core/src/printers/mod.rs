// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Printers through CUPS: the print queues with their state and supplies, the jobs, adding a
//! driverless printer, and what can be done with them.
//!
//! [`Cups`] talks IPP to the local CUPS over its domain socket (or `localhost:631`, never another
//! host), with an IPP encoder of its own ([`ipp`]) and the HTTP/1.1 it needs. CUPS lets members
//! of its administrator group change printers over the socket (it checks the peer's
//! credentials); for anyone else, changes go to cups-pk-helper on the system bus, where polkit
//! asks for an administrator's password. Sway Things never asks for or keeps a password itself.
//! [`Discovery`] finds driverless printers (IPP Everywhere, `AirPrint`, IPP over USB) through Avahi
//! on the system bus; Sway Things never contacts a printer itself, CUPS does.
//!
//! Nothing here is stored by Sway Things: the queues, the default printer and the jobs are
//! CUPS's own. Names, models, locations, job titles and messages come from printers and other
//! users: they are data, shown as plain text and never put into commands.

mod admin;
mod cups;
mod discovery;
mod http;
pub mod ipp;
#[cfg(any(test, feature = "test-support"))]
pub mod stand_in;
mod test_page;

use std::fmt;
use std::fmt::Write as _;

/// Strings from printers and other users can hold anything: they are cleaned like the names the
/// D-Bus services report.
pub(crate) use crate::dbus::clean;
use crate::i18n::{Msg, Text};
use crate::msg;

pub use admin::Bus;
pub use cups::Cups;
pub use discovery::{Discovered, Discovery, Found};
pub use http::{Endpoint, Loopback};

/// What CUPS reports: the print queues, the jobs that aren't done, and the default printer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// The print queues, sorted by their title.
    pub printers: Vec<Printer>,
    /// The jobs that aren't done yet, oldest first.
    pub jobs: Vec<Job>,
    /// CUPS's default printer, for every user.
    pub default: Option<String>,
    /// The user's own default printer from `~/.cups/lpoptions` (`lpoptions -d`), which apps use
    /// instead of CUPS's; only while that printer exists. [`Cups::snapshot`] leaves it out: set
    /// it with [`set_user_default`](Self::set_user_default) from [`own_default`], read when it
    /// may have changed rather than with every snapshot.
    pub user_default: Option<String>,
}

impl Snapshot {
    /// Sets the user's own default printer to `name` (from [`own_default`]), if it exists.
    pub fn set_user_default(&mut self, name: Option<&str>) {
        self.user_default = name.and_then(|name| self.printer(name).map(|p| p.name.clone()));
    }

    /// The printer apps print to by default: the user's own default, else CUPS's.
    pub fn effective_default(&self) -> Option<&str> {
        self.user_default.as_deref().or(self.default.as_deref())
    }

    /// The printer named `name` (CUPS's names are case-insensitive).
    pub fn printer(&self, name: &str) -> Option<&Printer> {
        self.printers
            .iter()
            .find(|printer| printer.name.eq_ignore_ascii_case(name))
    }

    /// The jobs of the printer `name`, oldest first.
    pub fn jobs_of<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Job> + 'a {
        self.jobs
            .iter()
            .filter(move |job| job.printer.eq_ignore_ascii_case(name))
    }

    /// The printers the page lists: all but the temporary queues that hold no job.
    pub fn listed(&self) -> impl Iterator<Item = &Printer> {
        self.printers
            .iter()
            .filter(|printer| !printer.temporary || self.jobs_of(&printer.name).next().is_some())
    }
}

/// The state of a printer, after IPP's `printer-state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrinterState {
    /// Ready for a job.
    Idle,
    /// Printing.
    Processing,
    /// Stopped: paused, or stopped by an error.
    Stopped,
}

/// How serious a reason of a printer's state is: the suffix of its keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Just so you know (`-report`).
    Report,
    /// Something to look after soon (`-warning`).
    Warning,
    /// The printer can't print (`-error`, or no suffix).
    Error,
}

/// Why a printer is in its state: one of `printer-state-reasons`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    /// What the reason is.
    pub kind: ReasonKind,
    /// How serious it is.
    pub severity: Severity,
}

/// What a reason of a printer's state is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasonKind {
    /// Paused by someone (`paused`).
    Paused,
    /// Out of paper (`media-empty`).
    MediaEmpty,
    /// Paper low (`media-low`).
    MediaLow,
    /// Paper jam (`media-jam`).
    MediaJam,
    /// Waiting for paper to be loaded (`media-needed`).
    MediaNeeded,
    /// Toner low (`toner-low`).
    TonerLow,
    /// Out of toner (`toner-empty`).
    TonerEmpty,
    /// Ink or another supply low (`marker-supply-low`).
    SupplyLow,
    /// Ink or another supply empty (`marker-supply-empty`).
    SupplyEmpty,
    /// The waste container almost full (`marker-waste-almost-full`).
    WasteAlmostFull,
    /// The waste container full (`marker-waste-full`).
    WasteFull,
    /// A door or cover open (`door-open`, `cover-open`, `interlock-open`).
    DoorOpen,
    /// Can't be reached (`offline`).
    Offline,
    /// CUPS is connecting to it (`connecting-to-device`).
    Connecting,
    /// The output tray is full (`output-area-full`).
    OutputFull,
    /// A tray is missing (`input-tray-missing`, `output-tray-missing`).
    TrayMissing,
    /// CUPS lacks a filter for the printer (`cups-missing-filter`).
    MissingFilter,
    /// The printer is off (`shutdown`).
    Shutdown,
    /// Another reason: its keyword without the suffix.
    Other(String),
}

impl Reason {
    /// The reason of the keyword `keyword` (`media-empty-error`); `None` for `none`.
    pub fn parse(keyword: &str) -> Option<Self> {
        let keyword = keyword.trim();
        if keyword.is_empty() || keyword == "none" {
            return None;
        }
        let (base, severity) = if let Some(base) = keyword.strip_suffix("-error") {
            (base, Severity::Error)
        } else if let Some(base) = keyword.strip_suffix("-warning") {
            (base, Severity::Warning)
        } else if let Some(base) = keyword.strip_suffix("-report") {
            (base, Severity::Report)
        } else {
            (keyword, Severity::Error)
        };
        let kind = match base {
            "paused" => ReasonKind::Paused,
            "media-empty" => ReasonKind::MediaEmpty,
            "media-low" => ReasonKind::MediaLow,
            "media-jam" => ReasonKind::MediaJam,
            "media-needed" => ReasonKind::MediaNeeded,
            "toner-low" => ReasonKind::TonerLow,
            "toner-empty" => ReasonKind::TonerEmpty,
            "marker-supply-low" | "developer-low" => ReasonKind::SupplyLow,
            "marker-supply-empty" | "developer-empty" => ReasonKind::SupplyEmpty,
            "marker-waste-almost-full" => ReasonKind::WasteAlmostFull,
            "marker-waste-full" => ReasonKind::WasteFull,
            "door-open" | "cover-open" | "interlock-open" => ReasonKind::DoorOpen,
            "offline" => ReasonKind::Offline,
            "connecting-to-device" => ReasonKind::Connecting,
            "output-area-full" => ReasonKind::OutputFull,
            "input-tray-missing" | "output-tray-missing" => ReasonKind::TrayMissing,
            "cups-missing-filter" => ReasonKind::MissingFilter,
            "shutdown" => ReasonKind::Shutdown,
            other => ReasonKind::Other(clean(other)),
        };
        // CUPS reports a printer it can't reach and its own connecting as reports, yet they say
        // why nothing prints.
        let severity = match kind {
            ReasonKind::Offline | ReasonKind::Paused => Severity::Error,
            _ => severity,
        };
        Some(Self { kind, severity })
    }

    /// The reason in words, as the page shows it.
    pub fn text(&self) -> Text {
        let msg: Msg = match &self.kind {
            ReasonKind::Paused => msg!("Paused"),
            ReasonKind::MediaEmpty => msg!("Out of paper"),
            ReasonKind::MediaLow => msg!("Paper low"),
            ReasonKind::MediaJam => msg!("Paper jam"),
            ReasonKind::MediaNeeded => msg!("Load paper"),
            ReasonKind::TonerLow => msg!("Toner low"),
            ReasonKind::TonerEmpty => msg!("Out of toner"),
            ReasonKind::SupplyLow => msg!("Ink low"),
            ReasonKind::SupplyEmpty => msg!("Out of ink"),
            ReasonKind::WasteAlmostFull => msg!("Waste container almost full"),
            ReasonKind::WasteFull => msg!("Waste container full"),
            ReasonKind::DoorOpen => msg!("Door open"),
            ReasonKind::Offline => msg!("Offline"),
            ReasonKind::Connecting => msg!("Connecting"),
            ReasonKind::OutputFull => msg!("Output tray full"),
            ReasonKind::TrayMissing => msg!("Tray missing"),
            ReasonKind::MissingFilter => msg!("A filter for this printer is missing"),
            ReasonKind::Shutdown => msg!("Turned off"),
            ReasonKind::Other(keyword) => {
                return Text::new(msg!("Printer problem: {reason}")).arg("reason", keyword);
            }
        };
        msg.into()
    }
}

/// How much of a supply is left, from `marker-levels`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// The percentage left (0 to 100).
    Percent(u8),
    /// Some is left, how much isn't known (-3).
    SomeLeft,
    /// Unknown (-1, -2).
    Unknown,
}

impl Level {
    fn from_ipp(level: i32) -> Self {
        match level {
            -3 => Self::SomeLeft,
            0..=100 => Self::Percent(u8::try_from(level).unwrap_or(100)),
            101.. => Self::Percent(100),
            _ => Self::Unknown,
        }
    }
}

/// A supply of a printer: a toner or ink cartridge, or the waste container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// Its name, as the printer reports it (`Black Toner`).
    pub name: String,
    /// Its kind, an IPP keyword (`toner`, `ink`, `ink-cartridge`, `waste-toner`, …).
    pub kind: String,
    /// Its colors: one for a plain cartridge, several for a cartridge with more inks.
    pub colors: Vec<(u8, u8, u8)>,
    /// How much is left.
    pub level: Level,
    /// The level at or below which the printer calls it low.
    pub low: i32,
    /// The level at or above which a waste container is called full.
    pub high: i32,
}

impl Marker {
    /// Whether it collects waste: the level is how full it is, not how much is left.
    pub fn is_waste(&self) -> bool {
        self.kind.starts_with("waste")
    }

    /// Whether it needs attention: a supply low, or a waste container almost full.
    pub fn needs_attention(&self) -> bool {
        match self.level {
            Level::Percent(level) if self.is_waste() => {
                self.high > 0 && i32::from(level) >= self.high
            }
            Level::Percent(level) => i32::from(level) <= self.low.max(0),
            Level::SomeLeft | Level::Unknown => false,
        }
    }
}

/// A print queue.
#[allow(
    clippy::struct_excessive_bools,
    reason = "the flags are CUPS's own printer attributes, independent of each other"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printer {
    /// CUPS's name of the queue, which the actions take.
    pub name: String,
    /// Its description (`printer-info`), which the page shows as its title.
    pub info: String,
    /// Where it is (`printer-location`).
    pub location: String,
    /// Its make and model.
    pub model: String,
    /// Where CUPS sends its jobs (`device-uri`); empty if CUPS doesn't say.
    pub device_uri: String,
    /// Its state.
    pub state: PrinterState,
    /// Why it is in that state, most serious first.
    pub reasons: Vec<Reason>,
    /// CUPS's message about its state; may be empty.
    pub message: String,
    /// Whether it takes new jobs.
    pub accepting: bool,
    /// Whether CUPS shares it on the network.
    pub shared: bool,
    /// Whether CUPS or the print dialog set it up on its own, for a while (`printer-is-temporary`).
    pub temporary: bool,
    /// Whether it is a class of printers rather than one.
    pub class: bool,
    /// Its supplies.
    pub markers: Vec<Marker>,
}

impl Printer {
    /// Its title: the description, else the name.
    pub fn title(&self) -> &str {
        if self.info.is_empty() {
            &self.name
        } else {
            &self.info
        }
    }

    /// Whether someone paused it.
    pub fn paused(&self) -> bool {
        self.state == PrinterState::Stopped
            && self
                .reasons
                .iter()
                .any(|reason| reason.kind == ReasonKind::Paused)
    }

    /// The most serious reason that stops it from printing, if there is one.
    pub fn problem(&self) -> Option<&Reason> {
        self.reasons
            .iter()
            .find(|reason| reason.severity == Severity::Error && reason.kind != ReasonKind::Paused)
    }

    /// Its state in words: `Ready`, `Printing`, `Paused`, `Out of paper`, …, followed by the
    /// warnings (`Ready · Toner low`).
    pub fn status(&self) -> Text {
        let main: Text = if self.paused() {
            msg!("Paused").into()
        } else if let Some(problem) = self.problem() {
            problem.text()
        } else {
            match self.state {
                PrinterState::Idle => msg!("Ready").into(),
                PrinterState::Processing => msg!("Printing").into(),
                PrinterState::Stopped if self.message.is_empty() => msg!("Stopped").into(),
                PrinterState::Stopped => {
                    Text::new(msg!("Stopped: {message}")).arg("message", &self.message)
                }
            }
        };
        let mut parts = vec![main];
        if !self.accepting {
            parts.push(msg!("Rejects new jobs").into());
        }
        parts.extend(
            self.reasons
                .iter()
                .filter(|reason| reason.severity == Severity::Warning)
                .map(Reason::text),
        );
        Text::join(parts, " · ")
    }

    /// The URI of the queue in requests to CUPS.
    pub(crate) fn uri(name: &str) -> String {
        format!("ipp://localhost/printers/{}", percent_encode(name))
    }
}

/// The state of a job, after IPP's `job-state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    /// Waiting for its turn.
    Pending,
    /// Held until someone releases it.
    Held,
    /// Printing.
    Processing,
    /// Stopped, with the printer.
    Stopped,
    /// Canceled.
    Canceled,
    /// Aborted by an error.
    Aborted,
    /// Printed.
    Completed,
}

impl JobState {
    fn from_ipp(state: i32) -> Self {
        match state {
            4 => Self::Held,
            5 => Self::Processing,
            6 => Self::Stopped,
            7 => Self::Canceled,
            8 => Self::Aborted,
            9 => Self::Completed,
            _ => Self::Pending,
        }
    }

    /// The state in words.
    pub fn text(self) -> Msg {
        match self {
            Self::Pending => msg!("Waiting"),
            Self::Held => msg!("Held"),
            Self::Processing => msg!("Printing"),
            Self::Stopped => msg!("Stopped"),
            Self::Canceled => msg!("Canceled"),
            Self::Aborted => msg!("Failed"),
            Self::Completed => msg!("Printed"),
        }
    }
}

/// A print job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// Its id, which the actions take.
    pub id: i32,
    /// The name of its printer.
    pub printer: String,
    /// Its title (the document's name); empty if CUPS keeps it private.
    pub title: String,
    /// Who printed it; empty if CUPS keeps it private.
    pub owner: String,
    /// Its state.
    pub state: JobState,
    /// When it was sent, in seconds since the epoch.
    pub created: Option<i64>,
}

/// What went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// CUPS isn't running or can't be reached. The text says why.
    Unavailable(Text),
    /// CUPS listens, but the user may not connect to it. The text says where.
    NoAccess(Text),
    /// CUPS is set up to use the print server named here, on another computer: Sway Things only
    /// manages the CUPS of this computer.
    Remote(String),
    /// The change needs an administrator: CUPS refused, and cups-pk-helper isn't there to ask
    /// for a password.
    NotAllowed(Authorization),
    /// cups-pk-helper (polkit) didn't allow it. The text is its own, so untrusted.
    Denied(String),
    /// CUPS refused or failed: its message (untrusted text, as data), or what Sway Things makes
    /// of it.
    Failed(Text),
}

/// What the user can do to make changes: for the hint shown when they can't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    /// The group whose members CUPS lets make changes (`lpadmin`, `wheel`).
    pub group: String,
}

impl Error {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Unavailable(reason) => {
                Text::new(msg!("CUPS isn't available: {reason}")).text("reason", reason.clone())
            }
            Self::NoAccess(reason) => {
                Text::new(msg!("CUPS can't be reached: {reason}")).text("reason", reason.clone())
            }
            Self::Remote(server) => Text::new(msg!(
                "CUPS is set up to use the print server {server}. Sway Things only manages the \
                 printers of this computer."
            ))
            .arg("server", server),
            Self::NotAllowed(authorization) => Text::new(msg!(
                "Changing printers needs an administrator. Install cups-pk-helper, which asks for \
                 an administrator's password, or ask to be added to the group “{group}”."
            ))
            .arg("group", &authorization.group),
            Self::Denied(reason) => Text::new(msg!(
                "Not allowed: {reason}. If no password dialog showed up, no polkit agent is \
                 running: start one with sway, such as lxqt-policykit-agent or polkit-gnome."
            ))
            .arg("reason", reason),
            Self::Failed(reason) => reason.clone(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for Error {}

/// A printer to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPrinter {
    /// The queue's name: see [`queue_name`].
    pub name: String,
    /// Where the printer is: an `ipp://` or `ipps://` URI, see [`device_uri`].
    pub uri: String,
    /// Its description, the title the page shows.
    pub info: String,
    /// Where it stands.
    pub location: String,
}

/// The longest queue name CUPS takes, in bytes.
const MAX_NAME: usize = 127;

/// A queue name made from `model` (`HP LaserJet Pro M404` becomes `HP_LaserJet_Pro_M404`): only
/// `A-Z`, `a-z`, `0-9`, `_` and `-`, at most 127 bytes, and none of `taken` (CUPS's names are
/// case-insensitive), with `_2`, `_3`, … added if needed.
pub fn queue_name(model: &str, taken: &[&str]) -> String {
    let mut base = String::new();
    for c in model.chars() {
        if c.is_ascii_alphanumeric() || c == '-' {
            base.push(c);
        } else if !base.is_empty() && !base.ends_with('_') {
            base.push('_');
        }
    }
    let mut base = base.trim_end_matches('_').to_owned();
    if base.is_empty() {
        base.push_str("Printer");
    }
    base.truncate(MAX_NAME - 4);
    let free = |name: &str| !taken.iter().any(|other| other.eq_ignore_ascii_case(name));
    if free(&base) {
        return base;
    }
    // One of the first `taken.len() + 2` numbers is free.
    (2..taken.len() + 3)
        .map(|n| format!("{base}_{n}"))
        .find(|name| free(name))
        .unwrap_or(base)
}

/// Why an address can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressError {
    /// Nothing was typed.
    Empty,
    /// It isn't an `ipp://` or `ipps://` address of a printer.
    NotIpp,
}

impl AddressError {
    /// What is wrong, to show.
    pub fn text(self) -> Msg {
        match self {
            Self::Empty => msg!("Type the address of the printer"),
            Self::NotIpp => msg!(
                "Type a host name, an IP address, or an address starting with ipp:// or ipps://"
            ),
        }
    }
}

/// The device URI of the printer at the address `input`: an `ipp://` or `ipps://` URI as it is,
/// or a host name or IP address, which becomes `ipp://HOST/ipp/print` (the path of IPP
/// Everywhere).
///
/// # Errors
///
/// An [`AddressError`] if `input` is empty or no such address.
pub fn device_uri(input: &str) -> Result<String, AddressError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(AddressError::Empty);
    }
    if input
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '<' | '>' | '\\' | '`'))
    {
        return Err(AddressError::NotIpp);
    }
    let (scheme, rest) = match input.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("ipp".to_owned(), input),
    };
    if scheme != "ipp" && scheme != "ipps" {
        return Err(AddressError::NotIpp);
    }
    let (authority, path) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, "/ipp/print"),
    };
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host),
    };
    let host_ok = if authority.starts_with('[') {
        host.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '%'))
    } else {
        host.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
    };
    if host.is_empty() || !host_ok {
        return Err(AddressError::NotIpp);
    }
    let port = if authority.ends_with(']') {
        None
    } else {
        authority.rsplit_once(':').map(|(_, port)| port)
    };
    if port.is_some_and(|port| port.parse::<u16>().is_err()) {
        return Err(AddressError::NotIpp);
    }
    let path = if path == "/" { "/ipp/print" } else { path };
    Ok(format!("{scheme}://{authority}{path}"))
}

/// `text` with everything but unreserved URI characters percent-encoded (RFC 3986).
pub(crate) fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// `text` with its percent-encoded bytes decoded; invalid encodings stay as they are.
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let (b'%', Some(decoded)) = (byte, hex) {
            out.push(decoded);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The colors of `marker-colors` value `value`: `#RRGGBB`, several of them run together for a
/// cartridge with more inks, or `none`.
fn parse_colors(value: &str) -> Vec<(u8, u8, u8)> {
    value
        .split('#')
        .filter_map(|hex| {
            let hex = hex.get(..6)?;
            let channel = |range: std::ops::Range<usize>| {
                hex.get(range).and_then(|h| u8::from_str_radix(h, 16).ok())
            };
            Some((channel(0..2)?, channel(2..4)?, channel(4..6)?))
        })
        .collect()
}

/// The user's own default printer, from the `Default` line of `~/.cups/lpoptions`, which
/// `lpoptions -d` writes. The file is only read.
pub fn own_default() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let text = std::fs::read_to_string(std::path::Path::new(&home).join(".cups/lpoptions")).ok()?;
    lpoptions_default(&text)
}

/// The printer name in the `Default` line of an lpoptions file (`Default name[/instance] …`).
fn lpoptions_default(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("Default")?;
        if !rest.starts_with([' ', '\t']) {
            return None;
        }
        let dest = rest.split_whitespace().next()?;
        let name = dest.split('/').next()?;
        (!name.is_empty()).then(|| name.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printer(state: PrinterState, reasons: &[&str]) -> Printer {
        Printer {
            name: "Office".to_owned(),
            info: String::new(),
            location: String::new(),
            model: String::new(),
            device_uri: String::new(),
            state,
            reasons: reasons.iter().filter_map(|r| Reason::parse(r)).collect(),
            message: String::new(),
            accepting: true,
            shared: false,
            temporary: false,
            class: false,
            markers: Vec::new(),
        }
    }

    #[test]
    fn reasons_are_read_with_their_severity() {
        assert_eq!(Reason::parse("none"), None);
        assert_eq!(
            Reason::parse("media-empty-error"),
            Some(Reason {
                kind: ReasonKind::MediaEmpty,
                severity: Severity::Error
            })
        );
        assert_eq!(
            Reason::parse("toner-low-warning").map(|r| r.severity),
            Some(Severity::Warning)
        );
        assert_eq!(
            Reason::parse("media-jam").map(|r| r.severity),
            Some(Severity::Error)
        );
        assert_eq!(
            Reason::parse("offline-report").map(|r| (r.kind, r.severity)),
            Some((ReasonKind::Offline, Severity::Error))
        );
        assert_eq!(
            Reason::parse("com.vendor-\u{7}odd-error").map(|r| r.kind),
            Some(ReasonKind::Other("com.vendor- odd".to_owned()))
        );
    }

    #[test]
    fn the_status_says_what_matters_most() {
        let text = |p: &Printer| p.status().english();
        assert_eq!(text(&printer(PrinterState::Idle, &["none"])), "Ready");
        assert_eq!(text(&printer(PrinterState::Processing, &[])), "Printing");
        assert_eq!(text(&printer(PrinterState::Stopped, &["paused"])), "Paused");
        assert_eq!(
            text(&printer(
                PrinterState::Processing,
                &["media-empty-error", "toner-low-warning"]
            )),
            "Out of paper · Toner low"
        );
        assert_eq!(
            text(&printer(PrinterState::Idle, &["offline-report"])),
            "Offline"
        );
        let mut rejecting = printer(PrinterState::Idle, &[]);
        rejecting.accepting = false;
        assert_eq!(text(&rejecting), "Ready · Rejects new jobs");
        let mut stopped = printer(PrinterState::Stopped, &[]);
        stopped.message = "Filter failed".to_owned();
        assert_eq!(text(&stopped), "Stopped: Filter failed");
    }

    #[test]
    fn levels_and_attention() {
        assert_eq!(Level::from_ipp(42), Level::Percent(42));
        assert_eq!(Level::from_ipp(150), Level::Percent(100));
        assert_eq!(Level::from_ipp(-1), Level::Unknown);
        assert_eq!(Level::from_ipp(-2), Level::Unknown);
        assert_eq!(Level::from_ipp(-3), Level::SomeLeft);
        let marker = |kind: &str, level| Marker {
            name: "x".to_owned(),
            kind: kind.to_owned(),
            colors: Vec::new(),
            level: Level::Percent(level),
            low: 10,
            high: 90,
        };
        assert!(marker("toner", 5).needs_attention());
        assert!(!marker("toner", 50).needs_attention());
        assert!(marker("waste-toner", 95).needs_attention());
        assert!(!marker("waste-toner", 5).needs_attention());
    }

    #[test]
    fn colors_are_read() {
        assert_eq!(parse_colors("#FF0000"), [(255, 0, 0)]);
        assert_eq!(
            parse_colors("#00FFFF#FF00FF#FFFF00"),
            [(0, 255, 255), (255, 0, 255), (255, 255, 0)]
        );
        assert_eq!(parse_colors("none"), []);
        assert_eq!(parse_colors("#12"), []);
    }

    #[test]
    fn queue_names_are_safe_and_unique() {
        let none: &[&str] = &[];
        assert_eq!(
            queue_name("HP LaserJet Pro M404", none),
            "HP_LaserJet_Pro_M404"
        );
        assert_eq!(
            queue_name("  Brother/HL-L2350DW (USB) ", none),
            "Brother_HL-L2350DW_USB"
        );
        assert_eq!(queue_name("Épson ☃", none), "pson");
        assert_eq!(queue_name("☃", none), "Printer");
        assert_eq!(queue_name("../etc/passwd", none), "etc_passwd");
        let taken = ["hp_laserjet", "HP_LaserJet_2"];
        assert_eq!(queue_name("HP LaserJet", &taken), "HP_LaserJet_3");
        assert!(queue_name(&"a".repeat(500), none).len() <= MAX_NAME);
    }

    #[test]
    fn addresses_become_ipp_uris() {
        assert_eq!(
            device_uri("192.168.1.20"),
            Ok("ipp://192.168.1.20/ipp/print".to_owned())
        );
        assert_eq!(
            device_uri(" printer.local:631 "),
            Ok("ipp://printer.local:631/ipp/print".to_owned())
        );
        assert_eq!(
            device_uri("IPPS://printer.local/ipp/print/2"),
            Ok("ipps://printer.local/ipp/print/2".to_owned())
        );
        assert_eq!(
            device_uri("ipp://[fe80::1]:631/"),
            Ok("ipp://[fe80::1]:631/ipp/print".to_owned())
        );
        assert_eq!(device_uri(""), Err(AddressError::Empty));
        assert_eq!(device_uri("http://x"), Err(AddressError::NotIpp));
        assert_eq!(device_uri("socket://x:9100"), Err(AddressError::NotIpp));
        assert_eq!(device_uri("ipp://"), Err(AddressError::NotIpp));
        assert_eq!(device_uri("ipp://a b/"), Err(AddressError::NotIpp));
        assert_eq!(device_uri("ipp://user@host/"), Err(AddressError::NotIpp));
        assert_eq!(device_uri("host:port"), Err(AddressError::NotIpp));
        assert_eq!(device_uri("$(reboot)"), Err(AddressError::NotIpp));
    }

    #[test]
    fn percent_encoding_round_trips() {
        let name = "Büro Drucker #2/x";
        let encoded = percent_encode(name);
        assert_eq!(encoded, "B%C3%BCro%20Drucker%20%232%2Fx");
        assert_eq!(percent_decode(&encoded), name);
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn the_user_default_must_be_a_printer() {
        let mut snapshot = Snapshot {
            printers: vec![printer(PrinterState::Idle, &[])],
            ..Snapshot::default()
        };
        snapshot.set_user_default(Some("office"));
        assert_eq!(snapshot.user_default.as_deref(), Some("Office"));
        snapshot.set_user_default(Some("Gone"));
        assert_eq!(snapshot.user_default, None);
    }

    #[test]
    fn the_user_default_is_read_from_lpoptions() {
        assert_eq!(
            lpoptions_default("Dest Other sides=two-sided\nDefault Office/draft media=A4\n"),
            Some("Office".to_owned())
        );
        assert_eq!(lpoptions_default("Dest Office\n"), None);
        assert_eq!(lpoptions_default("DefaultX y\n"), None);
    }
}
