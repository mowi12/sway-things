// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The checks of the Environment page: Sway Things' files there (the environment.d file and the
//! portal configuration) and the XDG desktop portals, the same four steps as the page's
//! _Details_ dialog: installed, configured, the session's variables, running.

use super::{Code, Finding, Severity};
use crate::apps::Dirs;
use crate::environment::portals::{self, Check, Component};
use crate::generate::other::Problem;
use crate::generate::{Locations, Manifest, Ownership};
use crate::i18n::Text;
use crate::msg;
use crate::packages::Manager;
use crate::state::State;

/// What the checks of the environment look at besides the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentCheck {
    /// The XDG directories and the session's desktop.
    pub dirs: Dirs,
    /// The portals, as [`portals::check`] found them.
    pub portals: Check,
    /// The package manager of the distribution, for the install command of a missing portal.
    pub packages: Option<Manager>,
}

impl EnvironmentCheck {
    /// The check for this user. `None` without a home directory.
    pub async fn for_user() -> Option<Self> {
        let dirs = Dirs::from_env()?;
        let portals = portals::check(&dirs).await;
        Some(Self {
            dirs,
            portals,
            packages: Manager::detect(),
        })
    }
}

/// Adds the findings about the environment to `findings`.
pub(super) fn findings(
    check: &EnvironmentCheck,
    state: &State,
    locations: &Locations,
    findings: &mut Vec<Finding>,
) {
    let manifest = Manifest::load(locations.manifest());
    for file in [
        crate::environment::wanted(state, &check.dirs),
        portals::wanted(state, &check.dirs),
    ] {
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
            Finding::new(Code::EnvironmentFile, Severity::Problem, problem.text())
                .in_file(&file.path),
        );
    }
    portal_findings(&check.portals, check.packages, state, findings);
}

/// The findings of the four steps of the portal check.
fn portal_findings(
    check: &Check,
    packages: Option<Manager>,
    state: &State,
    findings: &mut Vec<Finding>,
) {
    for component in &check.missing {
        findings.push(
            Finding::new(
                Code::PortalMissing,
                Severity::Warning,
                Text::new(msg!("{package} isn't installed: {purpose} don't work."))
                    .arg("package", component.package())
                    .text("purpose", Text::new(purpose(*component))),
            )
            .with_hint(install_hint(component.package(), packages)),
        );
    }
    // Without the backends, no configuration helps: installing them comes first.
    let backends_installed = !check
        .missing
        .iter()
        .any(|component| matches!(component, Component::Wlr | Component::Gtk));
    if backends_installed && check.config_wrong() {
        findings.push(config_finding(check, state));
    }
    if check.variables_missing() {
        let on = matches!(
            state.get(crate::registry::environment::PASS_SESSION_VARIABLES, None),
            Some(crate::registry::Value::Bool(true))
        );
        let hint = if on {
            Text::new(msg!(
                "Sway Things passes them at login: log out and in again, or reload sway."
            ))
        } else {
            Text::new(msg!(
                "Switch on \"Pass session variables to systemd and D-Bus\" on the Environment \
                 page, or run `sway-things set environment.pass_session_variables true`."
            ))
        };
        findings.push(
            Finding::new(
                Code::SessionVariables,
                Severity::Warning,
                msg!(
                    "The systemd user manager lacks WAYLAND_DISPLAY or XDG_CURRENT_DESKTOP, so the \
                     portals it starts don't find the session."
                ),
            )
            .with_hint(hint),
        );
    }
    if check.running == Some(false) {
        findings.push(
            Finding::new(
                Code::PortalNotRunning,
                Severity::Warning,
                msg!("No portal answers on the session bus."),
            )
            .with_hint(msg!(
                "xdg-desktop-portal starts when an app asks for it. If screen sharing still \
                 fails, `journalctl --user -u xdg-desktop-portal` says why."
            )),
        );
    }
    if check.ok() {
        findings.push(Finding::new(
            Code::PortalsReady,
            Severity::Ok,
            msg!("The portals are installed and set up: wlr shares the screen, gtk opens files."),
        ));
    }
}

/// What a component is needed for.
const fn purpose(component: Component) -> crate::i18n::Msg {
    match component {
        Component::Portal => msg!("screen sharing, file pickers of sandboxed apps and the style"),
        Component::Wlr => msg!("screen sharing and screenshots of apps"),
        Component::Gtk => msg!("file pickers of sandboxed apps and the dark style"),
    }
}

/// How to install `package`: the command for the distribution's package manager.
pub fn install_hint(package: &str, packages: Option<Manager>) -> Text {
    match packages {
        Some(manager) => Text::new(msg!("Install it: {command}"))
            .arg("command", manager.install_command(&[package])),
        None => Text::new(msg!("Install the package {package}.")).arg("package", package),
    }
}

/// The finding about a configuration that doesn't pick wlr for screen sharing or nothing for
/// the file pickers.
fn config_finding(check: &Check, state: &State) -> Finding {
    let served = |interface: &str| {
        check
            .served
            .iter()
            .find(|(name, _)| *name == interface)
            .and_then(|(_, backend)| backend.clone())
            .unwrap_or_else(|| "none".to_owned())
    };
    let message = Text::new(msg!(
        "The portal configuration picks {screencast} for screen sharing and {file_chooser} for \
         the file pickers: screen sharing needs wlr, the file pickers a backend such as gtk."
    ))
    .arg("screencast", served("ScreenCast"))
    .arg("file_chooser", served("FileChooser"));
    let finding = Finding::new(Code::PortalConfig, Severity::Problem, message);
    let finding = match &check.config {
        Some(path) => finding.in_file(path),
        None => finding,
    };
    if check.can_configure {
        if portals::enabled(state) {
            finding.with_hint(msg!(
                "Sway Things' configuration is switched on: save the settings to write it."
            ))
        } else {
            finding.with_hint(msg!(
                "Create Sway Things' configuration on the Environment page (Portals, Details), \
                 or run `sway-things set environment.portal_config true`."
            ))
        }
    } else {
        let file = check
            .users_config
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        finding
            .with_details(portals::LINES.lines().map(Text::data).collect())
            .with_hint(
                Text::new(msg!(
                    "Your own {file} decides. For screen sharing, put the lines above into it."
                ))
                .arg("file", file),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use crate::state::Paths;

    fn load(toml: &str) -> (tempfile::TempDir, State) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("settings.toml"), format!("version = 1\n{toml}")).unwrap();
        let paths = Paths::new(&config, &dir.path().join("state"), "host").unwrap();
        let state = State::load(Registry::builtin(), paths);
        (dir, state)
    }

    fn codes(check: &Check, state: &State) -> Vec<(Code, Severity)> {
        let mut found = Vec::new();
        portal_findings(check, Some(Manager::Zypper), state, &mut found);
        found
            .iter()
            .map(|finding| (finding.code, finding.severity))
            .collect()
    }

    fn ready() -> Check {
        Check {
            served: vec![
                ("ScreenCast", Some("wlr".to_owned())),
                ("Screenshot", Some("wlr".to_owned())),
                ("FileChooser", Some("gtk".to_owned())),
                ("Settings", Some("gtk".to_owned())),
            ],
            session_variables: Some((true, true)),
            running: Some(true),
            can_configure: true,
            ..Check::default()
        }
    }

    #[test]
    fn ready_portals_are_reported_once() {
        let (_dir, state) = load("");
        assert_eq!(
            codes(&ready(), &state),
            [(Code::PortalsReady, Severity::Ok)]
        );
    }

    #[test]
    fn each_step_that_fails_is_reported() {
        let (_dir, state) = load("");
        let broken = Check {
            missing: vec![Component::Wlr, Component::Gtk],
            served: vec![("ScreenCast", None), ("FileChooser", None)],
            session_variables: Some((false, true)),
            running: Some(false),
            ..ready()
        };
        assert_eq!(
            codes(&broken, &state),
            [
                (Code::PortalMissing, Severity::Warning),
                (Code::PortalMissing, Severity::Warning),
                (Code::SessionVariables, Severity::Warning),
                (Code::PortalNotRunning, Severity::Warning),
            ]
        );
        let installed = Check {
            missing: Vec::new(),
            ..broken.clone()
        };
        assert_eq!(
            codes(&installed, &state)[0],
            (Code::PortalConfig, Severity::Problem)
        );
        let mut found = Vec::new();
        portal_findings(&broken, Some(Manager::Zypper), &state, &mut found);
        assert_eq!(
            found[0].hint.as_ref().map(Text::english),
            Some("Install it: sudo zypper install xdg-desktop-portal-wlr".to_owned())
        );
    }

    #[test]
    fn what_couldnt_be_asked_isnt_reported() {
        let (_dir, state) = load("");
        let unknown = Check {
            session_variables: None,
            running: None,
            ..ready()
        };
        assert_eq!(
            codes(&unknown, &state),
            [(Code::PortalsReady, Severity::Ok)]
        );
    }
}
