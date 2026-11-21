// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! System details for bug reports: what Sway Things runs with, for the About section and
//! `sway-things doctor`.
//!
//! Only what helps sorting out a bug and can be read cheaply and reliably: the versions of Sway
//! Things, the compositor, GTK, the distribution, NetworkManager, the sound server, CUPS and the
//! kernel, whether cups-pk-helper is there, and whether the tools the pages delegate to are
//! installed. Not BlueZ (no version on D-Bus, and
//! `bluetoothd` lives in a different place per distribution) and not wlroots (neither sway's IPC
//! nor `sway --version` report it).

use std::fs;
use std::path::Path;

use crate::delegate::{self, Tool};
use crate::i18n::{Msg, Text};
use crate::ipc::Compositor;
use crate::msg;
use crate::network::{NetworkManager, clean};
use crate::printers::Cups;
use crate::sound::Sound;

/// The version of Sway Things.
pub const SWAY_THINGS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the distribution describes itself: the first that exists wins (os-release(5)).
const OS_RELEASE: [&str; 2] = ["/etc/os-release", "/usr/lib/os-release"];

/// The kernel release, as `uname -r` prints it.
const KERNEL_RELEASE: &str = "/proc/sys/kernel/osrelease";

/// The tools the pages delegate to, in the order they are listed.
pub const TOOLS: [Tool; 5] = [
    delegate::NM_CONNECTION_EDITOR,
    delegate::BLUEMAN,
    delegate::PWVUCONTROL,
    delegate::PAVUCONTROL,
    delegate::SYSTEM_CONFIG_PRINTER,
];

/// Where D-Bus finds the system services it starts on demand, cups-pk-helper among them.
const SYSTEM_SERVICES: [&str; 2] = [
    "/usr/share/dbus-1/system-services",
    "/usr/local/share/dbus-1/system-services",
];

/// The heading of the details in `doctor`.
pub fn heading() -> Msg {
    msg!("System")
}

/// The system details. A `None` is a value that couldn't be found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    /// The running compositor, or without a session the installed one.
    pub compositor: Option<Compositor>,
    /// The version of GTK the app runs with.
    pub gtk: Option<String>,
    /// The distribution's `PRETTY_NAME`.
    pub distribution: Option<String>,
    /// NetworkManager's version; `None` while it isn't running.
    pub network_manager: Option<String>,
    /// The sound server's name with its version (`PulseAudio (on PipeWire 1.6.9)`); `None` while
    /// none runs.
    pub sound_server: Option<String>,
    /// CUPS's version (`2.4`); `None` while it isn't running.
    pub cups: Option<String>,
    /// Whether cups-pk-helper is installed, which makes printer changes for users CUPS doesn't
    /// let make them.
    pub cups_pk_helper: bool,
    /// The kernel release.
    pub kernel: Option<String>,
    /// Whether each of [`TOOLS`] is installed, in that order.
    pub tools: Vec<(Tool, bool)>,
}

/// What a line of the details shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A version or a name.
    Text(String),
    /// A version or a name with words that are translated: `SwayFX 0.5 (based on sway 1.11)`.
    Translated(Text),
    /// It couldn't be found.
    Unknown,
    /// The service isn't running.
    NotRunning,
    /// The tool is installed.
    Installed,
    /// The tool isn't installed.
    NotInstalled,
}

impl Value {
    /// The value: the text, `unknown`, `not running`, `installed` or `not installed`.
    pub fn text(&self) -> Text {
        match self {
            Self::Text(text) => Text::data(text),
            Self::Translated(text) => text.clone(),
            // TRANSLATORS: A value of the system details: a version that couldn't be found.
            Self::Unknown => msg!("unknown").into(),
            // TRANSLATORS: A value of the system details: a service that isn't running.
            Self::NotRunning => msg!("not running").into(),
            // TRANSLATORS: A value of the system details: a tool that is installed.
            Self::Installed => msg!("installed").into(),
            // TRANSLATORS: A value of the system details: a tool that isn't installed.
            Self::NotInstalled => msg!("not installed").into(),
        }
    }
}

impl Details {
    /// Reads the details. `compositor` and `gtk` come from the caller, which knows them;
    /// NetworkManager is asked on `network_manager`'s bus, the sound server on the `sound`
    /// connection and CUPS through `cups`, if there are those.
    pub async fn read(
        compositor: Option<Compositor>,
        gtk: Option<String>,
        network_manager: Option<&NetworkManager>,
        sound: Option<&Sound>,
        cups: Option<&Cups>,
    ) -> Self {
        let network_manager = match network_manager {
            Some(nm) => nm.version().await,
            None => None,
        };
        let sound_server = match sound {
            Some(sound) => sound.server().await.ok().map(|server| server.label()),
            None => None,
        };
        let cups = match cups {
            Some(cups) => cups.version().await,
            None => None,
        };
        let cups_pk_helper = SYSTEM_SERVICES.iter().any(|dir| {
            Path::new(dir)
                .join("org.opensuse.CupsPkHelper.Mechanism.service")
                .exists()
        });
        Self {
            compositor,
            gtk,
            distribution: distribution(&OS_RELEASE.map(Path::new)),
            network_manager,
            sound_server,
            cups,
            cups_pk_helper,
            kernel: kernel(Path::new(KERNEL_RELEASE)),
            tools: TOOLS
                .iter()
                .map(|tool| (*tool, tool.find().is_some()))
                .collect(),
        }
    }

    /// The lines, each a label and a value, in the order of the copied block. Names of programs
    /// are labels that stay as they are.
    pub fn lines(&self) -> Vec<(Text, Value)> {
        let found = |value: Option<String>| value.map_or(Value::Unknown, Value::Text);
        let mut lines = vec![
            (
                Text::data("Sway Things"),
                Value::Text(SWAY_THINGS_VERSION.to_owned()),
            ),
            (
                // TRANSLATORS: A label of the system details: the running or installed sway.
                msg!("Compositor").into(),
                self.compositor.map_or(Value::Unknown, |compositor| {
                    Value::Translated(compositor.text())
                }),
            ),
            (Text::data("GTK"), found(self.gtk.clone())),
            (
                // TRANSLATORS: A label of the system details: the Linux distribution.
                msg!("Distribution").into(),
                found(self.distribution.clone()),
            ),
            (
                Text::data("NetworkManager"),
                self.network_manager
                    .clone()
                    .map_or(Value::NotRunning, Value::Text),
            ),
            (
                // TRANSLATORS: A label of the system details: PipeWire's or PulseAudio's server.
                msg!("Sound server").into(),
                self.sound_server
                    .clone()
                    .map_or(Value::NotRunning, Value::Text),
            ),
            (
                Text::data("CUPS"),
                self.cups.clone().map_or(Value::NotRunning, Value::Text),
            ),
            (
                Text::data("cups-pk-helper"),
                if self.cups_pk_helper {
                    Value::Installed
                } else {
                    Value::NotInstalled
                },
            ),
            (
                // TRANSLATORS: A label of the system details: the version of the Linux kernel.
                msg!("Kernel").into(),
                found(self.kernel.clone()),
            ),
        ];
        lines.extend(self.tools.iter().map(|(tool, installed)| {
            let value = if *installed {
                Value::Installed
            } else {
                Value::NotInstalled
            };
            (Text::data(tool.program), value)
        }));
        lines
    }

    /// The plain-text block to paste into a bug report: a `Label: value` line each. In English,
    /// for the maintainers who read the report.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for (label, value) in self.lines() {
            text.push_str(&label.english());
            text.push_str(": ");
            text.push_str(&value.text().english());
            text.push('\n');
        }
        text
    }
}

/// The `PRETTY_NAME` of the first of `files` that can be read, or its `NAME`.
fn distribution(files: &[&Path]) -> Option<String> {
    let text = files
        .iter()
        .find_map(|file| fs::read_to_string(file).ok())?;
    let field = |name: &str| os_release_field(&text, name).filter(|value| !value.is_empty());
    field("PRETTY_NAME").or_else(|| field("NAME"))
}

/// The value of `name` in the os-release text `text`: unquoted, with the escapes of double
/// quotes undone and control characters removed.
fn os_release_field(text: &str, name: &str) -> Option<String> {
    let value = text.lines().rev().find_map(|line| {
        let (key, value) = line.trim().split_once('=')?;
        (key == name).then_some(value)
    })?;
    let value = if let Some(inner) = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        let mut unescaped = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => unescaped.extend(chars.next()),
                c => unescaped.push(c),
            }
        }
        unescaped
    } else if let Some(inner) = value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        inner.to_owned()
    } else {
        value.to_owned()
    };
    Some(clean(&value))
}

/// The kernel release in `file`.
fn kernel(file: &Path) -> Option<String> {
    let release = clean(&fs::read_to_string(file).ok()?);
    (!release.is_empty()).then_some(release)
}

/// The GTK version in the output of `sway-things-gui --version`: the line `GTK <version>`.
pub fn gtk_from_app_version(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let version = clean(line.strip_prefix("GTK ")?);
        let valid = !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.');
        valid.then_some(version)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::{SwayFxVersion, SwayVersion};

    fn details() -> Details {
        Details {
            compositor: Some(Compositor::SwayFx {
                version: SwayFxVersion::new(0, 5, 3),
                base: SwayVersion::new(1, 10, 1),
            }),
            gtk: Some("4.18.6".to_owned()),
            distribution: Some("openSUSE Tumbleweed".to_owned()),
            network_manager: Some("1.52.0".to_owned()),
            sound_server: Some("PulseAudio (on PipeWire 1.6.9)".to_owned()),
            cups: Some("2.4".to_owned()),
            cups_pk_helper: true,
            kernel: Some("6.17.7-1-default".to_owned()),
            tools: vec![
                (delegate::NM_CONNECTION_EDITOR, true),
                (delegate::BLUEMAN, false),
            ],
        }
    }

    #[test]
    fn the_block_lists_every_line_in_order() {
        assert_eq!(
            details().text(),
            format!(
                "Sway Things: {SWAY_THINGS_VERSION}\n\
                 Compositor: SwayFX 0.5.3 (based on sway 1.10.1)\n\
                 GTK: 4.18.6\n\
                 Distribution: openSUSE Tumbleweed\n\
                 NetworkManager: 1.52.0\n\
                 Sound server: PulseAudio (on PipeWire 1.6.9)\n\
                 CUPS: 2.4\n\
                 cups-pk-helper: installed\n\
                 Kernel: 6.17.7-1-default\n\
                 nm-connection-editor: installed\n\
                 blueman-manager: not installed\n"
            )
        );
    }

    #[test]
    fn missing_values_are_named() {
        let details = Details {
            compositor: None,
            gtk: None,
            distribution: None,
            network_manager: None,
            sound_server: None,
            cups: None,
            cups_pk_helper: false,
            kernel: None,
            ..details()
        };
        assert_eq!(
            details.text(),
            format!(
                "Sway Things: {SWAY_THINGS_VERSION}\n\
                 Compositor: unknown\n\
                 GTK: unknown\n\
                 Distribution: unknown\n\
                 NetworkManager: not running\n\
                 Sound server: not running\n\
                 CUPS: not running\n\
                 cups-pk-helper: not installed\n\
                 Kernel: unknown\n\
                 nm-connection-editor: installed\n\
                 blueman-manager: not installed\n"
            )
        );
    }

    #[test]
    fn os_release_values_are_unquoted() {
        let text = "NAME=\"openSUSE Tumbleweed\"\nID=opensuse-tumbleweed\n\
                    PRETTY_NAME=\"Say \\\"hi\\\" \\\\ \\$HOME\"\nVERSION_ID='20261115'\n";
        assert_eq!(
            os_release_field(text, "PRETTY_NAME").as_deref(),
            Some("Say \"hi\" \\ $HOME")
        );
        assert_eq!(
            os_release_field(text, "ID").as_deref(),
            Some("opensuse-tumbleweed")
        );
        assert_eq!(
            os_release_field(text, "VERSION_ID").as_deref(),
            Some("20261115")
        );
        assert_eq!(os_release_field(text, "VERSION"), None);
        assert_eq!(
            os_release_field("PRETTY_NAME=\"Evil\u{1b}[31m\u{7}\"\n", "PRETTY_NAME").as_deref(),
            Some("Evil [31m")
        );
    }

    #[test]
    fn the_distribution_comes_from_the_first_os_release() {
        let dir = tempfile::tempdir().unwrap();
        let etc = dir.path().join("os-release");
        let lib = dir.path().join("lib-os-release");
        let missing = dir.path().join("missing");
        fs::write(
            &lib,
            "NAME=Debian\nPRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\n",
        )
        .unwrap();
        assert_eq!(
            distribution(&[&missing, &lib]).as_deref(),
            Some("Debian GNU/Linux 13 (trixie)")
        );
        fs::write(&etc, "NAME=\"Arch Linux\"\nPRETTY_NAME=\"\"\n").unwrap();
        assert_eq!(distribution(&[&etc, &lib]).as_deref(), Some("Arch Linux"));
        assert_eq!(distribution(&[&missing]), None);
    }

    #[test]
    fn the_kernel_release_is_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("osrelease");
        fs::write(&file, "6.17.7-1-default\n").unwrap();
        assert_eq!(kernel(&file).as_deref(), Some("6.17.7-1-default"));
        fs::write(&file, "\n").unwrap();
        assert_eq!(kernel(&file), None);
        assert_eq!(kernel(&dir.path().join("missing")), None);
    }

    #[test]
    fn the_gtk_version_is_read_from_the_app() {
        assert_eq!(
            gtk_from_app_version("sway-things-gui 0.2.0\nGTK 4.18.6\n").as_deref(),
            Some("4.18.6")
        );
        assert_eq!(gtk_from_app_version("sway-things-gui 0.2.0\n"), None);
        assert_eq!(gtk_from_app_version("GTK 4.18; rm -rf\n"), None);
    }
}
