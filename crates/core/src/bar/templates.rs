// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Templates of custom modules the page offers: a module definition, and for most a script.
//!
//! The scripts ship inside the program (`crates/core/src/bar/templates/*.sh`) and are copied on
//! request to `~/.config/sway-things/waybar/scripts/`, where they are the user's: waybar runs
//! them, Sway Things never does, and a file there that isn't a copy of a template is never
//! written over. The module definition names the script by its path, quoted for the shell waybar
//! runs `exec` and the click commands with, followed by plain words.
//!
//! Sway Things itself never reaches the network, and neither do most templates. The few that do
//! (the check for updates and the unofficial usage modules of command line agents,
//! [`Group::AgentUsage`], which ask their vendor's endpoint with the login the vendor's own
//! command line tool keeps) say what they send where in [`Template::network`], and the page adds
//! one only after the user confirmed that. They never pose as another program (no borrowed
//! User-Agent) and never start another vendor's tool in the background.
//!
//! A template may add several modules ([`Template::parts`]): the agents' pill is a drawer group
//! whose members show the main agent, the other near its limit, and each in detail, all from one
//! script and one shared cache.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{BarError, WaybarFiles, process};
use crate::i18n::Msg;
use crate::msg;

/// The line every script of a template has near its top; a script without it isn't one of
/// ours and is never written over.
pub const MARKER: &str = "# Sway Things template:";

/// The groups the templates are shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Everyday modules, offline.
    General,
    /// Unofficial usage limits of command line agents: they use the tool's own login and an
    /// undocumented endpoint that may change or break.
    AgentUsage,
}

/// A template of a custom module.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Template {
    /// Its id, which is also the name of its module: `custom/<id>`.
    pub id: &'static str,
    /// Its name to show.
    pub title: Msg,
    /// What it shows.
    pub description: Msg,
    /// Its group.
    pub group: Group,
    /// The programs it needs, beyond a POSIX shell.
    pub needs: &'static [&'static str],
    /// The script: its file name and text, if it has one.
    pub script: Option<(&'static str, &'static str)>,
    /// The module definition as JSON; `$SCRIPT` stands for the script's path, quoted for a
    /// shell, and for a template with [`Template::parts`], `$MAIN` and `$OTHER` for the agents,
    /// the main one first ([`Template::chooses_main`]).
    pub module: &'static str,
    /// The name of its item in the lanes, if it isn't `custom/<id>`.
    pub name: Option<&'static str>,
    /// The definitions of the other modules it adds, by name (the members of its group), with
    /// the same placeholders.
    pub parts: &'static [(&'static str, &'static str)],
    /// What it sends where over the network, and how often, if it reaches the network at all:
    /// the page asks before it adds it ([`Template::uses_network`]).
    pub network: Option<Msg>,
}

/// The script of the usage modules of command line agents.
const AI_USAGE: (&str, &str) = ("ai-usage.sh", include_str!("templates/ai-usage.sh"));

/// The agents the usage modules know, by their id in the script: Claude Code and Codex.
pub const AGENTS: [&str; 2] = ["claude", "codex"];

/// The templates, in the order the page shows them.
pub const TEMPLATES: &[Template] = &[
    Template {
        id: "pomodoro",
        title: msg!("Pomodoro timer"),
        description: msg!(
            "Work and break phases; click to start or pause, right click to reset, middle click for the next phase, scroll to change the time"
        ),
        group: Group::General,
        needs: &[],
        script: Some(("pomodoro.sh", include_str!("templates/pomodoro.sh"))),
        module: r#"{"exec": "$SCRIPT", "return-type": "json", "interval": 1, "signal": 10,
            "tooltip": true,
            "on-click": "$SCRIPT toggle", "on-click-right": "$SCRIPT reset",
            "on-click-middle": "$SCRIPT skip", "on-scroll-up": "$SCRIPT up",
            "on-scroll-down": "$SCRIPT down"}"#,
        name: None,
        parts: &[],
        network: None,
    },
    Template {
        id: "camera",
        title: msg!("Camera in use"),
        description: msg!("A camera icon while any program has a camera open"),
        group: Group::General,
        needs: &["fuser"],
        script: Some(("camera.sh", include_str!("templates/camera.sh"))),
        module: r#"{"exec": "$SCRIPT", "return-type": "json", "interval": 3}"#,
        name: None,
        parts: &[],
        network: None,
    },
    Template {
        id: "updates",
        title: msg!("Updates available"),
        description: msg!("How many package updates there are, hidden while there are none"),
        group: Group::General,
        needs: &[],
        script: Some(("updates.sh", include_str!("templates/updates.sh"))),
        module: r#"{"exec": "$SCRIPT", "return-type": "json", "interval": 1800}"#,
        name: None,
        parts: &[],
        network: Some(msg!(
            "On Arch, checkupdates downloads a fresh copy of the package lists from your mirrors every half hour. With apt, dnf or zypper it reads the lists as they were last refreshed and uses no network."
        )),
    },
    Template {
        id: "battery-notify",
        title: msg!("Low battery warning"),
        description: msg!("No text; a notification at 15 % and at 5 % while discharging"),
        group: Group::General,
        needs: &["notify-send"],
        script: Some((
            "battery-notify.sh",
            include_str!("templates/battery-notify.sh"),
        )),
        module: r#"{"exec": "$SCRIPT", "return-type": "json", "interval": 60}"#,
        name: None,
        parts: &[],
        network: None,
    },
    Template {
        id: "vpn",
        title: msg!("VPN status"),
        description: msg!("The VPN and WireGuard connections that are up; click to edit them"),
        group: Group::General,
        needs: &["nmcli"],
        script: Some(("vpn.sh", include_str!("templates/vpn.sh"))),
        module: r#"{"exec": "$SCRIPT", "return-type": "json", "interval": 30,
            "on-click": "nm-connection-editor"}"#,
        name: None,
        parts: &[],
        network: None,
    },
    Template {
        id: "system-load",
        title: msg!("Highest system load"),
        description: msg!(
            "The highest of processor, memory, temperature and disk use, all four in the tooltip"
        ),
        group: Group::General,
        needs: &[],
        script: Some(("system-load.sh", include_str!("templates/system-load.sh"))),
        module: r#"{"exec": "$SCRIPT", "return-type": "json", "interval": 5}"#,
        name: None,
        parts: &[],
        network: None,
    },
    Template {
        id: "power",
        title: msg!("Power menu"),
        description: msg!("Lock, suspend, log out, restart or shut down, asked first with swaynag"),
        group: Group::General,
        needs: &["swaynag"],
        script: None,
        module: r#"{"format": "⏻", "tooltip-format": "Power",
            "on-click": "swaynag -t warning -m 'Power' -Z 'Lock' 'loginctl lock-session' -Z 'Suspend' 'systemctl suspend' -Z 'Log out' 'swaymsg exit' -Z 'Restart' 'systemctl reboot' -Z 'Shut down' 'systemctl poweroff'"}"#,
        name: None,
        parts: &[],
        network: None,
    },
    Template {
        id: "notifications",
        title: msg!("Notification center"),
        description: msg!(
            "swaync's bell with the number of notifications; click to open, right click for Do Not Disturb"
        ),
        group: Group::General,
        needs: &["swaync-client"],
        script: None,
        module: r#"{"exec": "swaync-client -swb", "return-type": "json", "escape": true,
            "format": "{icon} {}", "format-icons": {"notification": "🔔", "none": "🔕",
            "dnd-notification": "🔕", "dnd-none": "🔕"},
            "on-click": "swaync-client -t -sw", "on-click-right": "swaync-client -d -sw"}"#,
        name: None,
        parts: &[],
        network: None,
    },
    Template {
        id: "ai-usage",
        title: msg!("Agents' usage pill (unofficial)"),
        description: msg!(
            "Unofficial: one pill with the usage limits of your main command line agent, Claude Code or Codex; the other joins it only near its limit, and hovering shows each in detail. Uses each tool's own login and undocumented endpoints that may change or break"
        ),
        group: Group::AgentUsage,
        needs: &["curl", "jq"],
        script: Some(AI_USAGE),
        module: r#"{"orientation": "inherit",
            "modules": ["group/ai-usage-head", "custom/ai-detail#$OTHER",
                "custom/ai-detail#$MAIN"],
            "drawer": {"transition-duration": 250, "transition-left-to-right": false,
                "children-class": "drawer-child"}}"#,
        name: Some("group/ai-usage"),
        parts: &[
            (
                "group/ai-usage-head",
                r#"{"orientation": "inherit", "modules": ["custom/ai-usage#$MAIN",
                    "custom/ai-alert#$OTHER"]}"#,
            ),
            (
                "custom/ai-usage#$MAIN",
                r#"{"exec": "$SCRIPT head $MAIN $OTHER", "return-type": "json",
                    "interval": 60, "signal": 9, "tooltip": true,
                    "on-click": "$SCRIPT refresh $MAIN", "on-click-right": "$SCRIPT open $MAIN"}"#,
            ),
            (
                "custom/ai-alert#$OTHER",
                r#"{"exec": "$SCRIPT alert $OTHER $MAIN $OTHER", "return-type": "json",
                    "interval": 60, "signal": 9, "tooltip": true,
                    "on-click": "$SCRIPT refresh $OTHER", "on-click-right": "$SCRIPT open $OTHER"}"#,
            ),
            (
                "custom/ai-detail#$MAIN",
                r#"{"exec": "$SCRIPT detail $MAIN $MAIN $OTHER", "return-type": "json",
                    "interval": 60, "signal": 9, "tooltip": true,
                    "on-click": "$SCRIPT refresh $MAIN", "on-click-right": "$SCRIPT open $MAIN"}"#,
            ),
            (
                "custom/ai-detail#$OTHER",
                r#"{"exec": "$SCRIPT detail $OTHER $MAIN $OTHER", "return-type": "json",
                    "interval": 60, "signal": 9, "tooltip": true,
                    "on-click": "$SCRIPT refresh $OTHER", "on-click-right": "$SCRIPT open $OTHER"}"#,
            ),
        ],
        network: Some(msg!(
            "At most every five minutes, and when you click it, it sends Claude Code's login from ~/.claude/.credentials.json to api.anthropic.com and the Codex CLI's login from ~/.codex/auth.json to chatgpt.com, and asks each for your usage. These endpoints are unofficial: they may change or stop working at any time."
        )),
    },
    Template {
        id: "claude-usage",
        title: msg!("Claude usage (unofficial)"),
        description: msg!(
            "Unofficial: uses Claude Code's own login and an undocumented endpoint that may change or break"
        ),
        group: Group::AgentUsage,
        needs: &["curl", "jq"],
        script: Some(AI_USAGE),
        module: r#"{"exec": "$SCRIPT show claude", "return-type": "json", "interval": 60,
            "signal": 9, "tooltip": true, "on-click": "$SCRIPT refresh claude",
            "on-click-right": "$SCRIPT open claude"}"#,
        name: Some("custom/ai-usage#claude"),
        parts: &[],
        network: Some(msg!(
            "At most every five minutes, and when you click it, it sends Claude Code's login from ~/.claude/.credentials.json to api.anthropic.com and asks for your usage. This endpoint is unofficial: it may change or stop working at any time."
        )),
    },
    Template {
        id: "codex-usage",
        title: msg!("Codex usage (unofficial)"),
        description: msg!(
            "Unofficial: uses the Codex CLI's own login and an undocumented endpoint that may change or break"
        ),
        group: Group::AgentUsage,
        needs: &["curl", "jq"],
        script: Some(AI_USAGE),
        module: r#"{"exec": "$SCRIPT show codex", "return-type": "json", "interval": 60,
            "signal": 9, "tooltip": true, "on-click": "$SCRIPT refresh codex",
            "on-click-right": "$SCRIPT open codex"}"#,
        name: Some("custom/ai-usage#codex"),
        parts: &[],
        network: Some(msg!(
            "At most every five minutes, and when you click it, it sends the Codex CLI's login from ~/.codex/auth.json to chatgpt.com and asks for your usage. This endpoint is unofficial: it may change or stop working at any time."
        )),
    },
];

impl Template {
    /// The item picker's category of it.
    pub fn category(&self) -> super::catalog::Category {
        use super::catalog::Category;
        match self.id {
            "camera" => Category::Privacy,
            "updates" | "battery-notify" | "system-load" => Category::System,
            "vpn" => Category::NetworkAudio,
            "power" | "notifications" => Category::Basics,
            _ => Category::Custom,
        }
    }

    /// Whether it may reach the network: the check for updates (`checkupdates` syncs a copy of
    /// the package lists) and the usage modules. What it sends where is [`Template::network`].
    pub fn uses_network(&self) -> bool {
        self.network.is_some()
    }

    /// The name of its item in the lanes: `custom/<id>`, or the one it names.
    pub fn module_name(&self) -> String {
        self.name
            .map_or_else(|| format!("custom/{}", self.id), ToOwned::to_owned)
    }

    /// Whether it asks which agent comes first: the pill of several agents.
    pub fn chooses_main(&self) -> bool {
        !self.parts.is_empty() && self.group == Group::AgentUsage
    }

    /// Where its script goes among `files`, if it has one.
    pub fn script_path(&self, files: &WaybarFiles) -> Option<PathBuf> {
        self.script.map(|(name, _)| files.scripts().join(name))
    }

    /// Its module definition with the script at its place among `files`.
    pub fn definition(&self, files: &WaybarFiles) -> Map<String, Value> {
        self.definitions(files, AGENTS[0])
            .into_iter()
            .next()
            .map(|(_, definition)| definition)
            .unwrap_or_default()
    }

    /// The definitions it adds, by name, its item's first, with the script at its place among
    /// `files` and `main` as the main agent (one of [`AGENTS`], else the first).
    pub fn definitions(
        &self,
        files: &WaybarFiles,
        main: &str,
    ) -> Vec<(String, Map<String, Value>)> {
        let script = self
            .script_path(files)
            .map(|path| process::shell_path(&path))
            .unwrap_or_default();
        let main = AGENTS
            .iter()
            .copied()
            .find(|agent| *agent == main)
            .unwrap_or(AGENTS[0]);
        let other = AGENTS
            .iter()
            .copied()
            .find(|agent| *agent != main)
            .unwrap_or("");
        let fill = |text: &str, script: &str| {
            text.replace("$SCRIPT", script)
                .replace("$MAIN", main)
                .replace("$OTHER", other)
        };
        let parse = |json: &str| {
            let mut definition = match serde_json::from_str::<Value>(json) {
                Ok(Value::Object(definition)) => definition,
                _ => Map::new(),
            };
            for value in definition.values_mut() {
                match value {
                    Value::String(text) => *text = fill(text, &script),
                    Value::Array(items) => {
                        for item in items {
                            if let Value::String(text) = item {
                                *text = fill(text, "");
                            }
                        }
                    }
                    _ => {}
                }
            }
            definition
        };
        let mut out = vec![(self.module_name(), parse(self.module))];
        for (name, json) in self.parts {
            out.push((fill(name, ""), parse(json)));
        }
        out
    }

    /// The programs it needs that aren't installed.
    pub fn missing(&self) -> Vec<&'static str> {
        self.needs
            .iter()
            .copied()
            .filter(|program| {
                crate::delegate::find_program(program, std::env::var_os("PATH").as_deref())
                    .is_none()
            })
            .collect()
    }

    /// Copies its script to `files` (executable). A copy that is there is replaced only while it
    /// is exactly one Sway Things shipped (this version or an older one, by its SHA-256 in
    /// [`SHIPPED`]); one the user changed stays as it is, and so does any other file there.
    ///
    /// # Errors
    ///
    /// [`BarError::Io`] if it can't be read or written.
    pub fn install(
        &self,
        files: &WaybarFiles,
        _access: &crate::include::WriteAccess,
    ) -> Result<Installed, BarError> {
        let (Some((name, text)), Some(path)) = (self.script, self.script_path(files)) else {
            return Ok(Installed::NoScript);
        };
        install_script(&path, name, text)
    }
}

/// The SHA-256 of every version of a template's script that Sway Things shipped before this
/// one, by file name. A copy with one of them is replaced by the version of now; the version of
/// now needs no entry. Add the hash of a script here when it changes after a release.
pub const SHIPPED: &[(&str, &str)] = &[];

/// What [`Template::install`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    /// The template has no script.
    NoScript,
    /// The script was copied (or a copy of an older version replaced).
    Written,
    /// The same version is there already.
    Unchanged,
    /// A file is there that isn't a version Sway Things shipped (the user changed it, or it is
    /// someone else's): it stays, and the module runs it.
    Kept(PathBuf),
}

/// The SHA-256 of `bytes`, as `sha256sum` prints it.
fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Writes `text` (the script `name`) to `path` with the executable bits, unless a file there
/// isn't a version Sway Things shipped.
fn install_script(path: &Path, name: &str, text: &str) -> Result<Installed, BarError> {
    use std::os::unix::fs::PermissionsExt as _;
    let io = |error: std::io::Error| BarError::Io(path.to_owned(), error.to_string());
    match std::fs::read(path) {
        Ok(old) if old == text.as_bytes() => return Ok(Installed::Unchanged),
        Ok(old) => {
            let hash = sha256(&old);
            let shipped = SHIPPED
                .iter()
                .any(|(file, shipped)| *file == name && *shipped == hash);
            if !shipped {
                return Ok(Installed::Kept(path.to_owned()));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io(error)),
    }
    crate::atomic::write(path, text).map_err(io)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(io)?;
    Ok(Installed::Written)
}

/// The pictures of the agents for the usage modules that `state` and `modules` show with
/// `lists`: each agent's `icons/<agent>.svg` next to the files of waybar, if it is there, by
/// its `file://` URL. The script shows the agent's name where there is none.
pub fn logos(
    state: &crate::state::State,
    modules: &super::Modules,
    lists: &[Vec<String>],
) -> Vec<(String, String)> {
    let shown = modules.shown(lists);
    let usage = shown.iter().any(|name| {
        let kind = super::kind_of(name);
        matches!(
            kind,
            "custom/ai-usage" | "custom/ai-alert" | "custom/ai-detail"
        )
    });
    let Some(files) = WaybarFiles::of(state).filter(|_| usage) else {
        return Vec::new();
    };
    AGENTS
        .iter()
        .filter_map(|agent| {
            let path = files.dir.join("icons").join(format!("{agent}.svg"));
            let url = super::style::file_url(path.to_str()?)?;
            path.is_file().then(|| ((*agent).to_owned(), url))
        })
        .collect()
}

/// The name of agent `id` of the usage modules.
pub fn agent_name(id: &str) -> Option<&'static str> {
    match id {
        "claude" => Some("Claude"),
        "codex" => Some("Codex"),
        _ => None,
    }
}

/// What a module of a template shows on a calm day, for the preview, with its classes: the
/// usage of an agent, nothing for an alert (it shows only near a limit), the stopped timer.
pub fn sample(name: &str) -> Option<(String, Vec<&'static str>)> {
    let (kind, instance) = name.split_once('#').unwrap_or((name, ""));
    let agent = AGENTS.iter().copied().find(|agent| *agent == instance);
    let mark = |agent: &str| agent_name(agent).unwrap_or("");
    match (kind, agent) {
        ("custom/ai-usage", Some(agent)) => {
            Some((format!("{} 󰅐 42%", mark(agent)), vec!["ok", agent]))
        }
        ("custom/ai-alert", Some(agent)) => Some((String::new(), vec!["idle", agent])),
        ("custom/ai-detail", Some(agent)) => {
            Some((format!("{} 󰅐 35%  󰃰 20%", mark(agent)), vec!["ok", agent]))
        }
        ("custom/pomodoro", None) => Some(("󰔛".to_owned(), vec!["idle", "next-work"])),
        _ => None,
    }
}

/// The template with `id`.
pub fn find(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|template| template.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_complete() {
        let files = WaybarFiles {
            dir: PathBuf::from("/x/sway things/waybar"),
        };
        for template in TEMPLATES {
            let definition = template.definition(&files);
            assert!(!definition.is_empty(), "{}", template.id);
            assert!(super::super::is_module_name(&template.module_name()));
            if let Some((name, text)) = template.script {
                assert!(text.starts_with("#!/bin/sh\n"), "{name}");
                assert!(
                    text.lines().take(6).any(|line| line.starts_with(MARKER)),
                    "{name}"
                );
                assert!(
                    text.contains(concat!("SPDX-License-", "Identifier: GPL-3.0-or-later")),
                    "{name}"
                );
                if let Some(exec) = definition.get("exec").and_then(Value::as_str) {
                    assert!(
                        exec.starts_with(&format!("'/x/sway things/waybar/scripts/{name}'")),
                        "{exec}"
                    );
                }
            }
            for value in definition.values() {
                assert!(!value.to_string().contains("$SCRIPT"), "{}", template.id);
            }
            if template.group == Group::AgentUsage {
                assert!(template.script.unwrap().1.contains("UNOFFICIAL"));
                assert!(template.uses_network(), "{}", template.id);
            }
            // Every template that reaches the network says what it sends where, so the page
            // asks before it adds it.
            let text = template.script.map_or("", |(_, text)| text);
            if text.contains("curl") || text.contains("checkupdates") {
                assert!(template.uses_network(), "{}", template.id);
            }
        }
    }

    #[test]
    fn the_agents_pill_is_a_drawer_of_the_main_agent_and_the_others() {
        let files = WaybarFiles {
            dir: PathBuf::from("/w"),
        };
        let pill = find("ai-usage").unwrap();
        assert!(pill.chooses_main());
        assert!(pill.uses_network());
        let definitions = pill.definitions(&files, "codex");
        let names: Vec<&str> = definitions.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "group/ai-usage",
                "group/ai-usage-head",
                "custom/ai-usage#codex",
                "custom/ai-alert#claude",
                "custom/ai-detail#codex",
                "custom/ai-detail#claude",
            ]
        );
        let (_, group) = &definitions[0];
        assert_eq!(
            group["modules"],
            serde_json::json!([
                "group/ai-usage-head",
                "custom/ai-detail#claude",
                "custom/ai-detail#codex"
            ])
        );
        assert_eq!(
            super::super::groups::drawer(group).map(|drawer| drawer.to_left),
            Some(true)
        );
        let (_, head) = &definitions[2];
        assert_eq!(head["exec"], "/w/scripts/ai-usage.sh head codex claude");
        assert_eq!(head["on-click"], "/w/scripts/ai-usage.sh refresh codex");
        assert_eq!(head["signal"], 9);
        // An unknown agent is the first one.
        assert_eq!(pill.definitions(&files, "x")[2].0, "custom/ai-usage#claude");
        // Every template that adds several modules adds a group that names them all.
        for template in TEMPLATES
            .iter()
            .filter(|template| !template.parts.is_empty())
        {
            let definitions = template.definitions(&files, "claude");
            let shown = {
                let mut modules = super::super::Modules::default();
                for (name, definition) in &definitions {
                    modules.set(name, definition.clone());
                }
                modules.shown(&[vec![template.module_name()]])
            };
            for (name, _) in &definitions {
                assert!(shown.contains(name), "{name}");
            }
        }
        assert_eq!(
            sample("custom/ai-alert#codex"),
            Some((String::new(), vec!["idle", "codex"]))
        );
    }

    #[test]
    fn scripts_are_installed_but_never_over_a_file_of_the_user() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let files = WaybarFiles::in_config_dir(dir.path());
        let access = crate::include::WriteAccess::for_tests();
        let pomodoro = find("pomodoro").unwrap();
        assert_eq!(pomodoro.install(&files, &access), Ok(Installed::Written));
        assert_eq!(pomodoro.install(&files, &access), Ok(Installed::Unchanged));
        let path = pomodoro.script_path(&files).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
        // An edited copy keeps the template's line, and still stays as it is.
        let (_, text) = pomodoro.script.unwrap();
        let edited = text.replace("work=25", "work=50");
        std::fs::write(&path, &edited).unwrap();
        assert_eq!(
            pomodoro.install(&files, &access),
            Ok(Installed::Kept(path.clone()))
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        std::fs::write(&path, "#!/bin/sh\necho mine\n").unwrap();
        assert_eq!(
            pomodoro.install(&files, &access),
            Ok(Installed::Kept(path.clone()))
        );
        assert_eq!(
            find("power").unwrap().install(&files, &access),
            Ok(Installed::NoScript)
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
