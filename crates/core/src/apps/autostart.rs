// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Everything that starts with the session, in one list:
//!
//! - **Sway Things' entries** (`autostart.entries`): a command or an app, written as `exec`
//!   lines to `70-autostart.conf`. An item is `exec <command line>` or `app <desktop file ID>`;
//!   `# ` in front switches it off. An app is started by `sway-things hook launch <ID>`, which
//!   reads its desktop entry at that moment, so an update of the app changes nothing here, and its
//!   command line is never turned into a shell string.
//! - **The user's own `exec` and `exec_always` lines**, read only, with file and line.
//! - **XDG autostart entries** of `$XDG_CONFIG_HOME/autostart` and `$XDG_CONFIG_DIRS/autostart`
//!   (an ID in the first directory hides the others), each with whether it starts in this
//!   session: `Hidden`, `OnlyShowIn`, `NotShowIn` and `TryExec` decide, as for the runners.
//! - **The runner:** sway doesn't start XDG autostart entries itself. systemd's
//!   `xdg-desktop-autostart.target` (through a systemd user session) or `dex --autostart` does,
//!   started by the user's config, their session, or by Sway Things (`autostart.runner`).
//!
//! A system entry is switched off with Sway Things' own override in `$XDG_CONFIG_HOME/autostart`
//! (`Hidden=true`, `autostart.hidden`), only where no file of that name exists; it starts with
//! the generated files' header, and only such a file is ever changed or deleted
//! ([`super::files`]).

use std::path::{Path, PathBuf};

use super::Dirs;
use super::entry::DesktopEntry;
use super::installed::{shows_in, single_line};
use crate::command::{SwayCmd, SwayText};
use crate::generate::HEADER;
use crate::registry::Value;
pub use crate::registry::apps::{
    ENTRIES, Entry, HIDDEN, Item, RUNNER, RUNNERS, check_entry, check_hidden,
};
use crate::state::State;
use crate::sway_config::Directive;

/// The name of the systemd user unit Sway Things writes to start `xdg-desktop-autostart.target`,
/// which refuses to be started by hand (`RefuseManualStart=yes`).
pub const SYSTEMD_UNIT: &str = "sway-things-autostart.target";
/// systemd's target of the XDG autostart entries.
pub const SYSTEMD_TARGET: &str = "xdg-desktop-autostart.target";

/// A runner of XDG autostart entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Runner {
    /// systemd's `xdg-desktop-autostart.target`.
    Systemd,
    /// `dex --autostart`.
    Dex,
}

impl Runner {
    /// The runner stored in `state`, if any.
    pub fn stored(state: &State) -> Option<Self> {
        match state.get(RUNNER, None) {
            Some(Value::Choice(id)) => Self::from_id(id),
            _ => None,
        }
    }

    /// The runner of the choice `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "systemd" => Some(Self::Systemd),
            "dex" => Some(Self::Dex),
            _ => None,
        }
    }

    /// The id of its choice.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::Dex => "dex",
        }
    }

    /// The command line of `70-autostart.conf` that starts it: for systemd a hook that hands
    /// the session's environment to systemd first, so the apps find the display.
    pub const fn line(self) -> &'static str {
        match self {
            Self::Systemd => "sway-things hook autostart",
            Self::Dex => "dex --autostart --environment sway",
        }
    }
}

/// The items of `autostart.entries` stored in `state`, as written.
pub fn items(state: &State) -> Vec<String> {
    match state.get(ENTRIES, None) {
        Some(Value::List(items)) => items.clone(),
        _ => Vec::new(),
    }
}

/// The IDs of `autostart.hidden` stored in `state`.
pub fn hidden(state: &State) -> Vec<String> {
    match state.get(HIDDEN, None) {
        Some(Value::List(ids)) => ids.clone(),
        _ => Vec::new(),
    }
}

/// A line of `70-autostart.conf`: the command, or why there is none, and its setting.
#[derive(Debug, Clone, PartialEq)]
pub struct FileLine {
    /// The setting it comes from.
    pub key: &'static str,
    /// The command; `Err` with the reason for an item that can't be written, which becomes a
    /// comment.
    pub command: Result<SwayCmd, String>,
}

/// The lines of `70-autostart.conf` for `state`: the runner, then the entries switched on.
pub fn file_lines(state: &State) -> Vec<FileLine> {
    let mut lines = Vec::new();
    if let Some(runner) = Runner::stored(state) {
        let command = SwayText::new(runner.line())
            .map(|line| SwayCmd::exec(line, false))
            .map_err(|error| error.to_string());
        lines.push(FileLine {
            key: RUNNER,
            command,
        });
    }
    for item in items(state) {
        match Entry::parse(&item) {
            Ok(entry) if !entry.on => {}
            Ok(entry) => lines.push(FileLine {
                key: ENTRIES,
                command: entry
                    .command()
                    .map_err(|error| format!("{item}: {}", error.text().english())),
            }),
            Err(error) => lines.push(FileLine {
                key: ENTRIES,
                command: Err(format!("{item}: {}", error.text().english())),
            }),
        }
    }
    lines
}

/// An `exec` or `exec_always` line of the user's config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandExec {
    /// The file.
    pub file: PathBuf,
    /// The line number, from 1.
    pub line: usize,
    /// Whether it is `exec_always`, which runs at every reload too.
    pub always: bool,
    /// The command line, as written.
    pub command: String,
    /// The runner of XDG autostart entries it starts, if it starts one.
    pub runner: Option<Runner>,
}

/// The `exec` and `exec_always` lines of the user's config `directives` (without the generated
/// files, which `ours` holds), at the top level.
pub fn hand_written(directives: &[Directive], ours: &Path) -> Vec<HandExec> {
    directives
        .iter()
        .filter(|directive| !directive.file.starts_with(ours))
        .filter_map(|directive| {
            let always = if directive.is(&[], "exec_always") {
                true
            } else if directive.is(&[], "exec") {
                false
            } else {
                return None;
            };
            let command = directive.args.join(" ");
            let runner = runner_of(&command);
            Some(HandExec {
                file: directive.file.to_path_buf(),
                line: directive.line,
                always,
                command,
                runner,
            })
        })
        .collect()
}

/// The runner a command line starts: `dex` with `-a` or `--autostart`, or something that names
/// `xdg-desktop-autostart.target` or Sway Things' unit. A session target (`sway-session.target`
/// of sway-systemd, `graphical-session.target`) doesn't count: whether it wants the target, the
/// target being active tells ([`Systemd::active`]).
pub fn runner_of(command: &str) -> Option<Runner> {
    let words: Vec<&str> = command
        .split(|character: char| {
            character.is_whitespace() || matches!(character, ';' | '&' | '|' | '"' | '\'')
        })
        .filter(|word| !word.is_empty())
        .collect();
    let dex = words
        .iter()
        .position(|word| word.rsplit('/').next() == Some("dex"));
    if let Some(at) = dex {
        // `-a` or `--autostart` as an argument of its own, or among short options (`-ae`).
        if words.iter().skip(at + 1).any(|word| {
            *word == "--autostart"
                || (word.starts_with('-')
                    && !word.starts_with("--")
                    && word.len() > 1
                    && word[1..].bytes().all(|byte| byte.is_ascii_alphabetic())
                    && word[1..].contains('a'))
        }) {
            return Some(Runner::Dex);
        }
    }
    // Only what starts the target itself counts: a session target may or may not want it, which
    // the target being active in the session tells better ([`Systemd::active`]).
    let systemd = [SYSTEMD_TARGET, SYSTEMD_UNIT];
    words
        .iter()
        .any(|word| systemd.contains(word))
        .then_some(Runner::Systemd)
}

/// Whether an XDG autostart entry starts in this session, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Starts {
    /// It starts, once a runner runs.
    Yes,
    /// `Hidden=true`: switched off.
    Hidden,
    /// `OnlyShowIn` names other desktops only.
    OnlyIn(Vec<String>),
    /// `NotShowIn` names this desktop.
    NotIn(Vec<String>),
    /// The program of `TryExec` isn't installed.
    NotInstalled(String),
    /// It has no `Exec`, or one that can't be read.
    NoCommand,
    /// It isn't an application (`Type=Link`).
    NotAnApp,
}

/// Where an XDG autostart entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// In `$XDG_CONFIG_HOME/autostart`, written by the user or an app.
    User,
    /// Sway Things' own override of a system entry.
    Ours,
    /// In a system directory of `$XDG_CONFIG_DIRS`.
    System,
}

/// An XDG autostart entry, the one that counts for its ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XdgEntry {
    /// The ID: its file name.
    pub id: String,
    /// The file that counts.
    pub path: PathBuf,
    /// Where it is.
    pub place: Place,
    /// Its name, localized; the ID without `.desktop` if it has none.
    pub name: String,
    /// Its icon.
    pub icon: Option<String>,
    /// Whether it starts.
    pub starts: Starts,
    /// The system entry of the same ID that the user's file or Sway Things' override hides.
    pub system: Option<PathBuf>,
    /// `X-GNOME-Autostart-enabled=false`, which only GNOME's session reads: dex and systemd
    /// start the entry anyway.
    pub gnome_disabled: bool,
    /// Its command line (`Exec`) as written, on one line, of the system entry for Sway Things'
    /// override; `None` without one. Only to show.
    pub exec: Option<String>,
}

/// Whether the text of a file is Sway Things' own override (or another file it made).
pub fn is_ours(text: &str) -> bool {
    text.starts_with(HEADER)
}

/// The text of Sway Things' override that switches off the entry `id`.
pub fn override_text(id: &str) -> String {
    format!(
        "{HEADER}# Switches off the system's autostart entry {id}; Sway Things deletes this file \
         to switch it on again.\n[Desktop Entry]\nType=Application\nName={id}\nHidden=true\n"
    )
}

/// The autostart directories in order: `$XDG_CONFIG_HOME/autostart`, then those of
/// `$XDG_CONFIG_DIRS`.
pub fn dirs_of(dirs: &Dirs) -> Vec<PathBuf> {
    dirs.config_search()
        .into_iter()
        .map(|dir| dir.join("autostart"))
        .collect()
}

/// The directory of the user's autostart entries and Sway Things' overrides.
pub fn user_dir(dirs: &Dirs) -> PathBuf {
    dirs.config_home.join("autostart")
}

/// Every XDG autostart entry that counts, by ID (the first directory wins), sorted by name.
pub fn xdg_entries(dirs: &Dirs) -> Vec<XdgEntry> {
    let mut by_id: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for dir in dirs_of(dirs) {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<String> = read
            .filter_map(Result::ok)
            .filter_map(|item| item.file_name().into_string().ok())
            .filter(|name| name.ends_with(".desktop"))
            .collect();
        names.sort();
        for name in names {
            let path = dir.join(&name);
            if !std::fs::metadata(&path).is_ok_and(|meta| meta.is_file()) {
                continue;
            }
            match by_id.iter_mut().find(|(id, _)| *id == name) {
                Some((_, paths)) => paths.push(path),
                None => by_id.push((name, vec![path])),
            }
        }
    }
    let user = user_dir(dirs);
    let mut entries: Vec<XdgEntry> = by_id
        .into_iter()
        .filter_map(|(id, paths)| {
            let path = paths.first()?.clone();
            let system = paths.get(1).cloned();
            let text = std::fs::read(&path).ok()?;
            let text = String::from_utf8_lossy(&text);
            let place = if !path.starts_with(&user) {
                Place::System
            } else if is_ours(&text) {
                Place::Ours
            } else {
                Place::User
            };
            // Sway Things' override shows the system entry it switches off, by its name.
            let shown = match (place, &system) {
                (Place::Ours, Some(system)) => DesktopEntry::read(system).unwrap_or_default(),
                _ => DesktopEntry::parse(&text),
            };
            let entry = DesktopEntry::parse(&text);
            Some(XdgEntry {
                name: shown
                    .localized("Name", &dirs.languages)
                    .map(|name| single_line(&name))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| id.trim_end_matches(".desktop").to_owned()),
                icon: shown.string("Icon").filter(|icon| !icon.is_empty()),
                starts: starts(&entry, &path, dirs),
                gnome_disabled: entry.is_false("X-GNOME-Autostart-enabled"),
                exec: shown
                    .string("Exec")
                    .map(|exec| single_line(&exec))
                    .filter(|exec| !exec.is_empty()),
                id,
                path,
                place,
                system,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    entries
}

/// Whether `entry` (at `path`) starts in the session of `dirs`.
fn starts(entry: &DesktopEntry, path: &Path, dirs: &Dirs) -> Starts {
    if entry.boolean("Hidden") {
        return Starts::Hidden;
    }
    if entry.raw("Type").is_some_and(|kind| kind != "Application") {
        return Starts::NotAnApp;
    }
    let only = entry.list("OnlyShowIn");
    let not = entry.list("NotShowIn");
    if !shows_in(&only, &not, &dirs.desktops) {
        return if only.is_empty() || not.iter().any(|name| dirs.desktops.contains(name)) {
            Starts::NotIn(not)
        } else {
            Starts::OnlyIn(only)
        };
    }
    if let Some(program) = entry
        .string("TryExec")
        .filter(|program| !program.is_empty())
    {
        if dirs.find_program(&program).is_none() {
            return Starts::NotInstalled(program);
        }
    }
    match entry.command(path, &dirs.languages) {
        Ok(args)
            if args
                .first()
                .is_some_and(|program| dirs.find_program(program).is_some()) =>
        {
            Starts::Yes
        }
        Ok(args) => Starts::NotInstalled(args.first().cloned().unwrap_or_default()),
        Err(_) => Starts::NoCommand,
    }
}

/// What a systemd user session says about the XDG autostart target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Systemd {
    /// Whether `systemctl --user` answers and knows `xdg-desktop-autostart.target`.
    pub available: bool,
    /// Whether the target is active: something in this session started the entries.
    pub active: bool,
    /// Whether Sway Things' own unit is active: Sway Things started the target.
    pub ours_active: bool,
}

/// Asks the systemd user session about `xdg-desktop-autostart.target`, with a timeout of two
/// seconds per question. Without `systemctl` or a user session, nothing is available.
pub async fn systemd() -> Systemd {
    let load =
        crate::systemd::output(&["show", "--property=LoadState", "--value", SYSTEMD_TARGET]).await;
    let available = load.as_deref().map(str::trim) == Some("loaded");
    if !available {
        return Systemd::default();
    }
    let active = crate::systemd::output(&["is-active", SYSTEMD_TARGET]).await;
    let ours = crate::systemd::output(&["is-active", SYSTEMD_UNIT]).await;
    Systemd {
        available,
        active: active.as_deref().map(str::trim) == Some("active"),
        ours_active: ours.as_deref().map(str::trim) == Some("active"),
    }
}

/// What runs the XDG autostart entries in this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runners {
    /// systemd's answer.
    pub systemd: Systemd,
    /// The user's lines that start a runner.
    pub lines: Vec<HandExec>,
    /// The runner Sway Things starts.
    pub ours: Option<Runner>,
    /// Whether `dex` is installed.
    pub dex_installed: bool,
}

impl Runners {
    /// Whether anything runs the entries.
    pub fn any(&self) -> bool {
        self.systemd.active || !self.lines.is_empty() || self.ours.is_some()
    }

    /// Whether more than one thing starts them, so each entry may start twice: Sway Things'
    /// runner next to the user's line or a session that started the target already (and not
    /// through Sway Things' own unit).
    ///
    /// Everything that starts systemd's target counts once: starting it again does nothing. The
    /// target started by Sway Things' unit before the runner was changed is left out until the
    /// next login ([`Runners::stale`]).
    pub fn twice(&self) -> bool {
        let dex = self
            .lines
            .iter()
            .filter(|line| line.runner == Some(Runner::Dex))
            .count()
            + usize::from(self.ours == Some(Runner::Dex));
        let systemd = self.ours == Some(Runner::Systemd)
            || self
                .lines
                .iter()
                .any(|line| line.runner == Some(Runner::Systemd))
            || (self.systemd.active && !self.stale());
        dex + usize::from(systemd) > 1
    }

    /// Whether systemd's target runs only because Sway Things started it before its runner was
    /// changed: the change applies at the next login.
    pub fn stale(&self) -> bool {
        self.systemd.ours_active && self.ours != Some(Runner::Systemd)
    }

    /// The runner to offer when nothing runs them: systemd if its target is there, else dex if
    /// it is installed.
    pub fn offer(&self) -> Option<Runner> {
        if self.any() {
            None
        } else if self.systemd.available {
            Some(Runner::Systemd)
        } else if self.dex_installed {
            Some(Runner::Dex)
        } else {
            None
        }
    }
}

/// The text of the systemd user unit Sway Things writes for [`Runner::Systemd`]: a target that
/// wants `xdg-desktop-autostart.target`, which can't be started by hand itself.
pub fn systemd_unit_text() -> String {
    format!(
        "{HEADER}[Unit]\nDescription=XDG autostart entries of the sway session (Sway Things)\n\
         Documentation=man:systemd.special(7)\nWants={SYSTEMD_TARGET}\nBefore={SYSTEMD_TARGET}\n"
    )
}

/// The path of that unit.
pub fn systemd_unit_path(dirs: &Dirs) -> PathBuf {
    dirs.config_home.join("systemd/user").join(SYSTEMD_UNIT)
}

/// The variables `sway-things hook autostart` hands to systemd before it starts the entries:
/// those of sway's own `50-systemd-user.conf` and of sway-systemd, which tell apps the display,
/// the session, the desktop and the cursor. Nothing else: the user manager is shared by every
/// session and service of the user, so `PATH` (which a login shell may set differently) and
/// secrets such as `SSH_AUTH_SOCK` stay as the user manager has them. Apps started by dex, in
/// contrast, get all of sway's environment.
pub const SESSION_VARIABLES: [&str; 8] = [
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "SWAYSOCK",
    "I3SOCK",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "XCURSOR_THEME",
    "XCURSOR_SIZE",
];

/// Starts the XDG autostart entries through systemd, for `sway-things hook autostart`, which
/// runs once at each start of sway:
///
/// 1. hands the session's variables ([`SESSION_VARIABLES`], those that are set) to the systemd
///    user manager, so the apps find this session's display and the generator's conditions
///    know the desktop;
/// 2. `daemon-reload`, so the generator reads the autostart entries as they are now, and
///    systemd knows Sway Things' unit if it was just written;
/// 3. stops Sway Things' unit and `xdg-desktop-autostart.target`, and waits: the user manager
///    can outlive a sway session (lingering, a login on a TTY or over ssh, a second session),
///    and a target that is still active wouldn't be started anew. Stopping the targets ends no
///    app: `Wants=` doesn't carry a stop, and the services of the entries aren't part of them;
/// 4. starts the unit, which starts the target and every entry's service that isn't running.
///
/// # Errors
///
/// What `systemctl` reported, if a step failed.
pub async fn start_with_systemd() -> Result<(), String> {
    let set: Vec<&str> = SESSION_VARIABLES
        .into_iter()
        .filter(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
        .collect();
    if !set.is_empty() {
        let mut args = vec!["import-environment"];
        args.extend(set);
        crate::systemd::run(&args).await?;
    }
    crate::systemd::run(&["daemon-reload"]).await?;
    crate::systemd::run(&["stop", SYSTEMD_UNIT, SYSTEMD_TARGET]).await?;
    crate::systemd::run(&["start", "--no-block", SYSTEMD_UNIT]).await
}
