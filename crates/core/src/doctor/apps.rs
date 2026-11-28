// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The checks of the Apps & Autostart page: whether the default apps chosen here apply
//! (`XDG_CURRENT_DESKTOP` names sway, Sway Things' list can be written), and what starts with the
//! session: whether something runs the XDG autostart entries, or two things do.

use super::{Code, Finding, Severity};
use crate::apps::Dirs;
use crate::apps::autostart::{self, Runner, Runners, Starts, Systemd};
use crate::apps::files::{self, Problem};
use crate::apps::mime::Kind;
use crate::generate::Locations;
use crate::i18n::Text;
use crate::include::IncludeStatus;
use crate::manifest::{Manifest, Ownership};
use crate::msg;
use crate::state::State;

/// What the checks of the apps look at besides the settings: the XDG directories of the user
/// and what systemd says about its autostart target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppsCheck {
    /// The XDG directories and the session's desktop.
    pub dirs: Dirs,
    /// What the systemd user session says.
    pub systemd: Systemd,
}

impl AppsCheck {
    /// The check for this user: the directories of the environment and systemd's answer.
    /// `None` without a home directory.
    pub async fn for_user() -> Option<Self> {
        Some(Self {
            dirs: Dirs::from_env()?,
            systemd: autostart::systemd().await,
        })
    }
}

/// Adds the findings about the apps to `findings`.
///
/// # Blocking
///
/// Reads the user's autostart directory.
pub(super) fn findings(
    check: &AppsCheck,
    state: &State,
    status: &IncludeStatus,
    locations: &Locations,
    findings: &mut Vec<Finding>,
) {
    let dirs = &check.dirs;
    if Kind::ALL.iter().any(|kind| kind.stored(state).is_some()) {
        findings.push(desktop(dirs));
    }
    let manifest = Manifest::load(locations.manifest());
    let wanted = files::wanted(state, dirs);
    for file in &wanted {
        let Some(text) = &file.text else {
            continue;
        };
        let problem = match manifest.ownership(&file.path, text) {
            Ok(Ownership::Edited) => Problem::Edited(file.path.clone()),
            Ok(Ownership::Foreign) => Problem::Foreign(file.path.clone()),
            Ok(_) => continue,
            Err(error) => Problem::Io(file.path.clone(), error.to_string()),
        };
        findings.push(
            Finding::new(Code::AppsFile, Severity::Problem, problem.text()).in_file(&file.path),
        );
    }
    autostart_findings(check, state, status, locations, findings);
}

/// Whether `XDG_CURRENT_DESKTOP` names sway, so the default apps chosen here apply.
fn desktop(dirs: &Dirs) -> Finding {
    if dirs.is_sway() {
        return Finding::new(
            Code::DesktopSway,
            Severity::Ok,
            Text::new(msg!(
                "XDG_CURRENT_DESKTOP is {desktop}: apps read the default apps chosen in Sway \
                 Things (sway-mimeapps.list)."
            ))
            .arg("desktop", dirs.desktops.join(":")),
        );
    }
    let desktop = if dirs.desktops.is_empty() {
        Text::new(msg!("XDG_CURRENT_DESKTOP isn't set"))
    } else {
        Text::new(msg!("XDG_CURRENT_DESKTOP is {desktop}")).arg("desktop", dirs.desktops.join(":"))
    };
    Finding::new(
        Code::DesktopNotSway,
        Severity::Warning,
        Text::new(msg!(
            "{desktop}, so apps don't read sway-mimeapps.list: the default apps chosen in Sway \
             Things don't apply."
        ))
        .text("desktop", desktop),
    )
    .with_hint(msg!(
        "Set XDG_CURRENT_DESKTOP=sway before sway starts: in the line that starts it \
         (XDG_CURRENT_DESKTOP=sway sway), or with DesktopNames=sway in the session file of your \
         login manager (sway 1.11 and newer ship it)."
    ))
}

/// What runs the XDG autostart entries, and what starts with the session.
///
/// # Blocking
///
/// Reads the XDG autostart directories.
fn autostart_findings(
    check: &AppsCheck,
    state: &State,
    status: &IncludeStatus,
    locations: &Locations,
    findings: &mut Vec<Finding>,
) {
    let dirs = &check.dirs;
    let hand = autostart::hand_written(&status.config().directives, locations.directory());
    let runners = Runners {
        systemd: check.systemd,
        lines: hand
            .iter()
            .filter(|line| line.runner.is_some())
            .cloned()
            .collect(),
        ours: Runner::stored(state),
        dex_installed: dirs.find_program("dex").is_some(),
    };
    let entries = autostart::xdg_entries(dirs);
    let starting: Vec<Text> = entries
        .iter()
        .filter(|entry| entry.starts == Starts::Yes)
        .map(|entry| {
            Text::new(msg!("XDG autostart: {name} ({file})"))
                .arg("name", &entry.name)
                .arg("file", entry.path.display())
        })
        .collect();
    let mut details: Vec<Text> = autostart::items(state)
        .iter()
        .filter_map(|item| autostart::Entry::parse(item).ok())
        .filter(|entry| entry.on)
        .map(|entry| Text::new(msg!("Sway Things: {command}")).arg("command", entry.command_line()))
        .collect();
    details.extend(hand.iter().map(|line| {
        Text::new(msg!("{place}: {command}"))
            .arg("place", format!("{}:{}", line.file.display(), line.line))
            .arg("command", &line.command)
    }));
    if !runners.any() && !starting.is_empty() {
        findings.push(no_runner(&runners, starting));
        return;
    }
    if runners.twice() {
        findings.push(twice(&runners));
        return;
    }
    details.extend(starting);
    if runners.stale() {
        details.push(
            msg!(
                "systemd's target still runs from before the runner was changed; the change \
                 applies at the next login."
            )
            .into(),
        );
    }
    let message = if runners.systemd.active {
        Text::new(msg!(
            "systemd starts the XDG autostart entries (xdg-desktop-autostart.target is active)."
        ))
    } else if let Some(line) = runners.lines.first() {
        Text::new(msg!(
            "Your config starts the XDG autostart entries at {place}."
        ))
        .arg("place", format!("{}:{}", line.file.display(), line.line))
    } else if let Some(ours) = runners.ours {
        Text::new(msg!(
            "Sway Things starts the XDG autostart entries at login ({command})."
        ))
        .arg("command", ours.line())
    } else {
        Text::new(msg!(
            "Nothing starts the XDG autostart entries, and none would start in this session."
        ))
    };
    findings.push(Finding::new(Code::AutostartRunner, Severity::Ok, message).with_details(details));
}

/// Nothing runs the XDG autostart entries that would start, `starting`.
fn no_runner(runners: &Runners, starting: Vec<Text>) -> Finding {
    let hint = match runners.offer() {
        Some(Runner::Systemd) => Text::new(msg!(
            "Have Sway Things start them with systemd: sway-things set {key} systemd"
        ))
        .arg("key", autostart::RUNNER),
        Some(Runner::Dex) => Text::new(msg!(
            "Have Sway Things start them with dex: sway-things set {key} dex"
        ))
        .arg("key", autostart::RUNNER),
        None => msg!("Install dex, then have Sway Things start them on the Apps & Autostart page.")
            .into(),
    };
    Finding::new(
        Code::AutostartNoRunner,
        Severity::Warning,
        msg!(
            "Nothing starts the XDG autostart entries in this session: sway doesn't start them \
             itself, so these never start."
        ),
    )
    .with_details(starting)
    .with_hint(hint)
}

/// More than one thing runs the XDG autostart entries.
fn twice(runners: &Runners) -> Finding {
    let mut twice: Vec<Text> = runners
        .lines
        .iter()
        .map(|line| {
            Text::new(msg!("{place}: {command}"))
                .arg("place", format!("{}:{}", line.file.display(), line.line))
                .arg("command", &line.command)
        })
        .collect();
    if let Some(ours) = runners.ours {
        twice.push(Text::new(msg!("Sway Things: {command}")).arg("command", ours.line()));
    }
    if runners.systemd.active {
        twice.push(msg!("systemd: xdg-desktop-autostart.target is active").into());
    }
    Finding::new(
        Code::AutostartTwice,
        Severity::Warning,
        msg!("More than one thing starts the XDG autostart entries, so each may start twice."),
    )
    .with_details(twice)
    .with_hint(
        Text::new(msg!(
            "Keep one: remove the line from your config, or set {key} to none."
        ))
        .arg("key", autostart::RUNNER),
    )
}
