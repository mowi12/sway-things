// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The hooks of the command line, `sway-things hook <name>`: what the core writes into the
//! generated sway files, kanshi's profiles, the notification daemons' configs and the autostart
//! entries, and the command line parses. Their names are spelled out only here; the command line
//! maps its arguments to [`Hook`] and [`QuickSwitch`], and its tests parse every name.

/// What every hook's command line starts with.
const PREFIX: &str = "sway-things hook";

/// A hook of `sway-things hook`, by name. Its arguments, if it takes any, are the caller's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hook {
    /// `gsettings`: copies the stored settings with a GSettings key to GSettings.
    Gsettings,
    /// `displays`: switches a display on if none is on.
    Displays,
    /// `workspaces`: puts the workspaces on the displays connected now.
    Workspaces,
    /// `mirror`: shows displays on others as remembered.
    Mirror,
    /// `new-display`: asks how a display plugged in to a known combination is used.
    NewDisplay,
    /// `wallpaper`: shows the next picture of the collections whose time has come.
    Wallpaper,
    /// `clipboard`: starts or ends the watcher of the clipboard history.
    Clipboard,
    /// `night-light`: starts or ends the night light's tool.
    NightLight,
    /// `launch <id>`: starts an app by its desktop file ID.
    Launch,
    /// `autostart`: starts the XDG autostart entries through systemd.
    Autostart,
    /// `notification-click [--mako <id>]`: brings a clicked notification's app to the front.
    NotificationClick,
    /// `notification-daemon`: starts the chosen notification daemon unless one runs.
    NotificationDaemon,
    /// `timezone`: sets the time zone where the computer is.
    Timezone,
    /// `quick <switch> [action]`: a button of swaync's panel ([`QuickSwitch`]).
    Quick,
}

impl Hook {
    /// Every hook.
    pub const ALL: [Self; 14] = [
        Self::Gsettings,
        Self::Displays,
        Self::Workspaces,
        Self::Mirror,
        Self::NewDisplay,
        Self::Wallpaper,
        Self::Clipboard,
        Self::NightLight,
        Self::Launch,
        Self::Autostart,
        Self::NotificationClick,
        Self::NotificationDaemon,
        Self::Timezone,
        Self::Quick,
    ];

    /// Its name on the command line: `night-light`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gsettings => "gsettings",
            Self::Displays => "displays",
            Self::Workspaces => "workspaces",
            Self::Mirror => "mirror",
            Self::NewDisplay => "new-display",
            Self::Wallpaper => "wallpaper",
            Self::Clipboard => "clipboard",
            Self::NightLight => "night-light",
            Self::Launch => "launch",
            Self::Autostart => "autostart",
            Self::NotificationClick => "notification-click",
            Self::NotificationDaemon => "notification-daemon",
            Self::Timezone => "timezone",
            Self::Quick => "quick",
        }
    }

    /// The command line that runs it, `sway-things hook <name>`, followed by `args`, each after a
    /// space: for a program that runs it (kanshi, mako, swaync, an autostart entry), or to name
    /// it to the user. The caller quotes the arguments as the program needs.
    pub fn command_line(self, args: &[&str]) -> String {
        let mut line = format!("{PREFIX} {}", self.name());
        for arg in args {
            line.push(' ');
            line.push_str(arg);
        }
        line
    }
}

/// Whether `text` runs a hook somewhere: a line of a generated file or of kanshi's profiles.
pub fn runs_hook(text: &str) -> bool {
    text.contains(&format!("{PREFIX} "))
}

/// A switch of swaync's button grid, the first argument of `sway-things hook quick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuickSwitch {
    /// `dnd`: Do Not Disturb of the notification daemon.
    Dnd,
    /// `wifi`: Wi-Fi.
    Wifi,
    /// `bluetooth`: Bluetooth.
    Bluetooth,
    /// `sound`: the sound of the default output.
    Sound,
    /// `microphone`: the default microphone.
    Microphone,
}

impl QuickSwitch {
    /// Every switch.
    pub const ALL: [Self; 5] = [
        Self::Dnd,
        Self::Wifi,
        Self::Bluetooth,
        Self::Sound,
        Self::Microphone,
    ];

    /// Its name on the command line, also the name of swaync's button preset: `wifi`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dnd => "dnd",
            Self::Wifi => "wifi",
            Self::Bluetooth => "bluetooth",
            Self::Sound => "sound",
            Self::Microphone => "microphone",
        }
    }

    /// The switch whose name is `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|switch| switch.name() == name)
    }

    /// The command line that toggles it: `sway-things hook quick wifi`.
    pub fn toggle_line(self) -> String {
        Hook::Quick.command_line(&[self.name()])
    }

    /// The command line that prints its state, `true` or `false`: `sway-things hook quick wifi
    /// state`.
    pub fn state_line(self) -> String {
        Hook::Quick.command_line(&[self.name(), QUICK_STATE])
    }
}

/// The action of `sway-things hook quick` that prints the switch's state.
pub const QUICK_STATE: &str = "state";
