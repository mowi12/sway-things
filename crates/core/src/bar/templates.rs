// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Templates of custom modules the page offers: a module definition, and for most a script.
//!
//! The scripts ship inside the program (`crates/core/src/bar/templates/*.sh`) and are copied on
//! request to `~/.config/sway-things/waybar/scripts/`, where they are the user's: waybar runs
//! them, Sway Things never does, and a file there that isn't a copy of a template is never
//! written over. The module definition names the script by its path, quoted for the shell waybar
//! runs `exec` and the click commands with. No template reaches the network itself; only the
//! check for updates runs `checkupdates`, which syncs a copy of the package lists.

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
    /// shell.
    pub module: &'static str,
}

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
    },
    Template {
        id: "camera",
        title: msg!("Camera in use"),
        description: msg!("A camera icon while any program has a camera open"),
        group: Group::General,
        needs: &["fuser"],
        script: Some(("camera.sh", include_str!("templates/camera.sh"))),
        module: r#"{"exec": "$SCRIPT", "return-type": "json", "interval": 3}"#,
    },
    Template {
        id: "updates",
        title: msg!("Updates available"),
        description: msg!("How many package updates there are, hidden while there are none"),
        group: Group::General,
        needs: &[],
        script: Some(("updates.sh", include_str!("templates/updates.sh"))),
        module: r#"{"exec": "$SCRIPT", "return-type": "json", "interval": 1800}"#,
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
    /// the package lists).
    pub fn uses_network(&self) -> bool {
        self.id == "updates"
    }

    /// The name of its module, `custom/<id>`.
    pub fn module_name(&self) -> String {
        format!("custom/{}", self.id)
    }

    /// Where its script goes among `files`, if it has one.
    pub fn script_path(&self, files: &WaybarFiles) -> Option<PathBuf> {
        self.script.map(|(name, _)| files.scripts().join(name))
    }

    /// Its module definition with the script at its place among `files`.
    pub fn definition(&self, files: &WaybarFiles) -> Map<String, Value> {
        let script = self
            .script_path(files)
            .map(|path| process::shell_path(&path))
            .unwrap_or_default();
        let mut definition = match serde_json::from_str::<Value>(self.module) {
            Ok(Value::Object(definition)) => definition,
            _ => Map::new(),
        };
        for value in definition.values_mut() {
            if let Value::String(text) = value {
                *text = text.replace("$SCRIPT", &script);
            }
        }
        definition
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

/// The template with `id`.
pub fn find(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|template| template.id == id)
}

/// What a module of a template shows on a calm day, for the preview, with its classes: the
/// stopped timer.
pub fn sample(name: &str) -> Option<(String, Vec<&'static str>)> {
    match name {
        "custom/pomodoro" => Some(("󰔛".to_owned(), vec!["idle", "next-work"])),
        _ => None,
    }
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
                let exec = definition["exec"].as_str().unwrap();
                assert_eq!(exec, format!("'/x/sway things/waybar/scripts/{name}'"));
            }
            for value in definition.values() {
                assert!(!value.to_string().contains("$SCRIPT"), "{}", template.id);
            }
        }
    }

    #[test]
    fn the_stopped_timer_is_the_sample_of_the_pomodoro() {
        assert_eq!(
            sample("custom/pomodoro"),
            Some(("󰔛".to_owned(), vec!["idle", "next-work"]))
        );
        assert_eq!(sample("custom/media"), None);
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
