// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The XDG desktop portals: what screen sharing, screenshots of other apps, the file pickers of
//! sandboxed apps and the dark style need on sway, checked in four steps, and Sway Things' own
//! portal configuration.
//!
//! 1. **Installed:** xdg-desktop-portal (its D-Bus service and program), and the backends
//!    xdg-desktop-portal-wlr and -gtk (their `.portal` files and programs).
//! 2. **Configured:** which backend serves `ScreenCast`, `Screenshot`, `FileChooser` and
//!    `Settings`, as xdg-desktop-portal picks it (portals.conf(5)): in the first of
//!    `$XDG_CONFIG_HOME`, `$XDG_CONFIG_DIRS`, `/etc`, `$XDG_DATA_HOME`, `$XDG_DATA_DIRS` and
//!    `/usr/share` that has one, the `<desktop>-portals.conf` of the first desktop of
//!    `XDG_CURRENT_DESKTOP` that has one, else `portals.conf`. Where it picks nothing, a backend
//!    whose `UseIn` names the desktop, and as the last resort gtk.
//! 3. **Session variables:** `WAYLAND_DISPLAY` and `XDG_CURRENT_DESKTOP` in the environment of
//!    the systemd user manager, which starts the portals.
//! 4. **Running:** `org.freedesktop.portal.Desktop` has an owner on the session bus.
//!
//! The fix of step 2 is Sway Things' own `~/.config/xdg-desktop-portal/sway-portals.conf`, made
//! like the `sway-portals.conf` Arch and Fedora ship with sway: gtk by default, wlr for
//! `ScreenCast` and `Screenshot`, and no `Inhibit` (gtk's needs services sway doesn't have, so
//! apps fall back to Wayland's idle inhibitor). It is only written where
//! the user has no configuration of their own there that it would hide or replace.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::apps::Dirs;
use crate::generate::HEADER;
use crate::generate::other::Wanted;
use crate::registry::Value;
use crate::registry::environment::PORTAL_CONFIG;
use crate::state::State;

/// The name of Sway Things' portal configuration, read for the desktop `sway`.
pub const FILE_NAME: &str = "sway-portals.conf";

/// The interfaces the check names, by their short names.
pub const INTERFACES: [&str; 4] = ["ScreenCast", "Screenshot", "FileChooser", "Settings"];

/// The lines of Sway Things' portal configuration, for a user's own file.
pub const LINES: &str = concat!(
    "[preferred]\n",
    "default=gtk\n",
    "org.freedesktop.impl.portal.ScreenCast=wlr\n",
    "org.freedesktop.impl.portal.Screenshot=wlr\n",
    "org.freedesktop.impl.portal.Inhibit=none",
);

/// The packages of the portals, as every distribution Sway Things supports names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    /// xdg-desktop-portal itself.
    Portal,
    /// The wlroots backend: `ScreenCast` and `Screenshot`.
    Wlr,
    /// The GTK backend: `FileChooser`, `Settings` and more.
    Gtk,
}

impl Component {
    /// Every component, in the order of the check.
    pub const ALL: [Self; 3] = [Self::Portal, Self::Wlr, Self::Gtk];

    /// The package that installs it.
    pub const fn package(self) -> &'static str {
        match self {
            Self::Portal => "xdg-desktop-portal",
            Self::Wlr => "xdg-desktop-portal-wlr",
            Self::Gtk => "xdg-desktop-portal-gtk",
        }
    }

    /// The name of its D-Bus service.
    const fn bus_name(self) -> &'static str {
        match self {
            Self::Portal => "org.freedesktop.portal.Desktop",
            Self::Wlr => "org.freedesktop.impl.portal.desktop.wlr",
            Self::Gtk => "org.freedesktop.impl.portal.desktop.gtk",
        }
    }

    /// The name of its `.portal` file, for a backend.
    const fn portal_file(self) -> Option<&'static str> {
        match self {
            Self::Portal => None,
            Self::Wlr => Some("wlr.portal"),
            Self::Gtk => Some("gtk.portal"),
        }
    }

    /// The name of its systemd user unit.
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Portal => "xdg-desktop-portal.service",
            Self::Wlr => "xdg-desktop-portal-wlr.service",
            Self::Gtk => "xdg-desktop-portal-gtk.service",
        }
    }
}

/// A backend's `.portal` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backend {
    /// Its name: the file's name without `.portal`, as portals.conf names it.
    pub name: String,
    /// The short names of the interfaces it implements (`ScreenCast`).
    pub interfaces: Vec<String>,
    /// The desktops it is meant for, lowercase.
    pub use_in: Vec<String>,
}

/// The directories of the data files: `$XDG_DATA_HOME`, `$XDG_DATA_DIRS`, `/usr/share`.
fn data_dirs(dirs: &Dirs) -> Vec<PathBuf> {
    let mut found = vec![dirs.data_home.clone()];
    found.extend(dirs.data_dirs.iter().cloned());
    found.push(PathBuf::from("/usr/share"));
    found.dedup();
    let mut unique = Vec::new();
    for dir in found {
        if !unique.contains(&dir) {
            unique.push(dir);
        }
    }
    unique
}

/// The values of `key` in the group `group` of a key file (a `.portal` file, a D-Bus service,
/// portals.conf): the last line of it wins.
fn key_file_value<'a>(text: &'a str, group: &str, key: &str) -> Option<&'a str> {
    let mut current = "";
    let mut value = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            current = name;
        } else if current == group {
            if let Some((found, rest)) = line.split_once('=') {
                if found.trim() == key {
                    value = Some(rest.trim());
                }
            }
        }
    }
    value
}

/// The `;`-separated items of a key file's list.
fn list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// The backends in the directories `portals` ([`portal_dirs`]), sorted by name; a name in an
/// earlier directory hides the same name in a later one.
///
/// # Blocking
///
/// Reads the directories of the portal backends.
pub fn backends(portals: &[PathBuf]) -> Vec<Backend> {
    let mut found: HashMap<String, Backend> = HashMap::new();
    for dir in portals {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file) = entry.file_name().into_string() else {
                continue;
            };
            let Some(name) = file.strip_suffix(".portal") else {
                continue;
            };
            if found.contains_key(name) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let interfaces = key_file_value(&text, "portal", "Interfaces")
                .map(list)
                .unwrap_or_default()
                .into_iter()
                .map(|interface| {
                    interface
                        .strip_prefix("org.freedesktop.impl.portal.")
                        .unwrap_or(&interface)
                        .to_owned()
                })
                .collect();
            let use_in = key_file_value(&text, "portal", "UseIn")
                .map(list)
                .unwrap_or_default()
                .into_iter()
                .map(|desktop| desktop.to_lowercase())
                .collect();
            found.insert(
                name.to_owned(),
                Backend {
                    name: name.to_owned(),
                    interfaces,
                    use_in,
                },
            );
        }
    }
    let mut backends: Vec<Backend> = found.into_values().collect();
    backends.sort_by(|a, b| a.name.cmp(&b.name));
    backends
}

/// Whether `component` is installed: its D-Bus service in `data` (the directories D-Bus looks
/// in: `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`) names a program that exists, and a backend has its
/// `.portal` file.
pub fn installed(component: Component, data: &[PathBuf], backends: &[Backend]) -> bool {
    if let Some(file) = component.portal_file() {
        let name = file.trim_end_matches(".portal");
        if !backends.iter().any(|backend| backend.name == name) {
            return false;
        }
    }
    data.iter().any(|dir| {
        let service = dir
            .join("dbus-1/services")
            .join(format!("{}.service", component.bus_name()));
        std::fs::read_to_string(service).is_ok_and(|text| {
            key_file_value(&text, "D-BUS Service", "Exec")
                .and_then(|exec| exec.split_whitespace().next())
                .is_some_and(|program| Path::new(program).is_file())
        })
    })
}

/// The variable that makes xdg-desktop-portal look for its configuration and backends in one
/// directory only, as in its tests.
const PORTAL_DIR: &str = "XDG_DESKTOP_PORTAL_DIR";

/// The directories xdg-desktop-portal looks for its configuration in, in its order; only
/// `portal_dir` where `XDG_DESKTOP_PORTAL_DIR` names one.
pub fn config_dirs(dirs: &Dirs, portal_dir: Option<&Path>) -> Vec<PathBuf> {
    if let Some(dir) = portal_dir {
        return vec![dir.to_owned()];
    }
    let mut search = vec![dirs.config_home.clone()];
    search.extend(dirs.config_dirs.iter().cloned());
    search.push(PathBuf::from("/etc"));
    search.extend(data_dirs(dirs));
    let mut unique: Vec<PathBuf> = Vec::new();
    for dir in search.into_iter().map(|dir| dir.join("xdg-desktop-portal")) {
        if !unique.contains(&dir) {
            unique.push(dir);
        }
    }
    unique
}

/// The directories xdg-desktop-portal looks for backends (`.portal` files) in, in its order;
/// only `portal_dir` where `XDG_DESKTOP_PORTAL_DIR` names one.
pub fn portal_dirs(dirs: &Dirs, portal_dir: Option<&Path>) -> Vec<PathBuf> {
    if let Some(dir) = portal_dir {
        return vec![dir.to_owned()];
    }
    data_dirs(dirs)
        .into_iter()
        .map(|dir| dir.join("xdg-desktop-portal/portals"))
        .collect()
}

/// The configuration xdg-desktop-portal uses for `desktops` (as `XDG_CURRENT_DESKTOP` lists
/// them): in the first directory of `search` that has one, the `<desktop>-portals.conf` of the
/// first desktop that has one there, else its `portals.conf`.
pub fn winning_config(search: &[PathBuf], desktops: &[String]) -> Option<PathBuf> {
    search.iter().find_map(|dir| {
        desktops
            .iter()
            .map(|desktop| dir.join(format!("{}-portals.conf", desktop.to_lowercase())))
            .chain(std::iter::once(dir.join("portals.conf")))
            .find(|path| path.is_file())
    })
}

/// The `[preferred]` choices of a portals.conf: `default` and the interfaces by their short
/// names, each a list of backend names (`*` any, `none` none).
pub fn preferred(text: &str) -> HashMap<String, Vec<String>> {
    let mut choices = HashMap::new();
    let mut current = "";
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            current = name;
        } else if current == "preferred" {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let key = key
                    .strip_prefix("org.freedesktop.impl.portal.")
                    .unwrap_or(key);
                choices.insert(key.to_owned(), list(value));
            }
        }
    }
    choices
}

/// The backend that serves `interface`, as xdg-desktop-portal picks it: the first of the
/// configuration's `choices` for the interface that implements it, else the first of its
/// `default`; `none` in the list that applies means none. Where the configuration picks
/// nothing, the first backend whose `UseIn` names one of `desktops`, and as the last resort gtk.
/// `backends` are in the order of [`backends`].
pub fn backend_for<S: std::hash::BuildHasher>(
    interface: &str,
    choices: Option<&HashMap<String, Vec<String>, S>>,
    backends: &[Backend],
    desktops: &[String],
) -> Option<String> {
    let implements = |backend: &&Backend| backend.interfaces.iter().any(|name| name == interface);
    let pick = |names: &[String]| {
        names.iter().find_map(|name| {
            if name == "*" {
                backends.iter().find(implements)
            } else {
                backends
                    .iter()
                    .filter(implements)
                    .find(|backend| backend.name == *name)
            }
        })
    };
    if let Some(choices) = choices {
        let own = choices.get(interface);
        let default = choices.get("default");
        let none = |names: Option<&Vec<String>>| {
            names.is_some_and(|names| names.iter().any(|name| name == "none"))
        };
        if none(own) || (own.is_none() && none(default)) {
            return None;
        }
        if let Some(backend) = own
            .and_then(|names| pick(names))
            .or_else(|| default.and_then(|names| pick(names)))
        {
            return Some(backend.name.clone());
        }
    }
    desktops
        .iter()
        .find_map(|desktop| {
            backends.iter().filter(implements).find(|backend| {
                backend
                    .use_in
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(desktop))
            })
        })
        .or_else(|| {
            backends
                .iter()
                .filter(implements)
                .find(|backend| backend.name == "gtk")
        })
        .map(|backend| backend.name.clone())
}

/// Sorts `backends` like xdg-desktop-portal: those meant for the first of `desktops` that one
/// of them names first, then by name.
pub fn sort_backends(backends: &mut [Backend], desktops: &[String]) {
    let rank = |backend: &Backend| {
        desktops
            .iter()
            .position(|desktop| {
                backend
                    .use_in
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(desktop))
            })
            .unwrap_or(usize::MAX)
    };
    backends.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name)));
}

/// What the check found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Check {
    /// The components that aren't installed.
    pub missing: Vec<Component>,
    /// The configuration that decides, if there is one.
    pub config: Option<PathBuf>,
    /// The backend of each of [`INTERFACES`], in that order.
    pub served: Vec<(&'static str, Option<String>)>,
    /// Whether the user manager has `WAYLAND_DISPLAY` and `XDG_CURRENT_DESKTOP`; `None` where it
    /// didn't answer.
    pub session_variables: Option<(bool, bool)>,
    /// Whether a portal answers on the session bus; `None` where the bus couldn't be asked.
    pub running: Option<bool>,
    /// Sway Things' configuration could be made: the user has neither a `sway-portals.conf`
    /// nor a `portals.conf` of their own in `~/.config/xdg-desktop-portal`.
    pub can_configure: bool,
    /// The user's own configuration in `~/.config/xdg-desktop-portal`, if there is one.
    pub users_config: Option<PathBuf>,
}

impl Check {
    /// Whether the configuration needs a fix: `ScreenCast` isn't served by wlr, or nothing serves
    /// `FileChooser`.
    pub fn config_wrong(&self) -> bool {
        let backend = |interface: &str| {
            self.served
                .iter()
                .find(|(name, _)| *name == interface)
                .and_then(|(_, backend)| backend.as_deref())
        };
        backend("ScreenCast") != Some("wlr") || backend("FileChooser").is_none()
    }

    /// Whether the user manager lacks a session variable.
    pub fn variables_missing(&self) -> bool {
        self.session_variables
            .is_some_and(|(wayland, desktop)| !wayland || !desktop)
    }

    /// Whether every step passed, as far as it could be checked.
    pub fn ok(&self) -> bool {
        self.missing.is_empty()
            && !self.config_wrong()
            && !self.variables_missing()
            && self.running != Some(false)
    }
}

/// Whether the file at `path` is one Sway Things wrote: it starts with the header of the
/// generated files.
fn is_ours(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.starts_with(HEADER))
}

/// The user's own configuration in `~/.config/xdg-desktop-portal` that Sway Things'
/// `sway-portals.conf` would hide or replace: a `sway-portals.conf` it didn't write, the
/// `<desktop>-portals.conf` of a desktop of `XDG_CURRENT_DESKTOP`, or `portals.conf`.
pub fn users_config(dirs: &Dirs) -> Option<PathBuf> {
    let user_dir = dirs.config_home.join("xdg-desktop-portal");
    std::iter::once(FILE_NAME.to_owned())
        .chain(
            dirs.desktops
                .iter()
                .map(|desktop| format!("{}-portals.conf", desktop.to_lowercase())),
        )
        .chain(std::iter::once("portals.conf".to_owned()))
        .map(|name| user_dir.join(name))
        .find(|path| path.is_file() && !is_ours(path))
}

/// The steps that need no program: what is installed and how it is configured, for `dirs`.
///
/// # Blocking
///
/// Reads the directories of the portal backends.
pub fn look(dirs: &Dirs) -> Check {
    let portal_dir = std::env::var_os(PORTAL_DIR)
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute());
    look_in(dirs, portal_dir.as_deref())
}

/// [`look`] with `portal_dir` as the value of `XDG_DESKTOP_PORTAL_DIR`.
///
/// # Blocking
///
/// Reads the directories of the portal backends.
pub fn look_in(dirs: &Dirs, portal_dir: Option<&Path>) -> Check {
    let mut backends = backends(&portal_dirs(dirs, portal_dir));
    sort_backends(&mut backends, &dirs.desktops);
    let mut data = vec![dirs.data_home.clone()];
    data.extend(dirs.data_dirs.iter().cloned());
    let missing = Component::ALL
        .into_iter()
        .filter(|component| !installed(*component, &data, &backends))
        .collect();
    let config = winning_config(&config_dirs(dirs, portal_dir), &dirs.desktops);
    let choices = config
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| preferred(&text));
    let served = INTERFACES
        .iter()
        .map(|interface| {
            (
                *interface,
                backend_for(interface, choices.as_ref(), &backends, &dirs.desktops),
            )
        })
        .collect();
    let users_config = users_config(dirs);
    Check {
        missing,
        config,
        served,
        session_variables: None,
        running: None,
        can_configure: users_config.is_none(),
        users_config,
    }
}

/// The variables of the user manager's environment the portals need.
pub fn session_variables<S: std::hash::BuildHasher>(
    environment: &HashMap<String, String, S>,
) -> (bool, bool) {
    let set = |name: &str| environment.get(name).is_some_and(|value| !value.is_empty());
    (set("WAYLAND_DISPLAY"), set("XDG_CURRENT_DESKTOP"))
}

/// Every step: [`look`], then the user manager's environment and the session bus.
///
/// # Blocking
///
/// Reads the directories of the portal backends.
pub async fn check(dirs: &Dirs) -> Check {
    let mut check = look(dirs);
    check.session_variables = super::files::user_manager()
        .await
        .map(|environment| session_variables(&environment));
    check.running = running().await;
    check
}

/// Whether `org.freedesktop.portal.Desktop` has an owner on the session bus; `None` where the
/// bus can't be asked.
pub async fn running() -> Option<bool> {
    let connection = zbus::Connection::session().await.ok()?;
    let bus = zbus::fdo::DBusProxy::new(&connection).await.ok()?;
    let name = zbus::names::BusName::try_from(Component::Portal.bus_name()).ok()?;
    bus.name_has_owner(name).await.ok()
}

/// The path of Sway Things' portal configuration for `dirs`.
pub fn path(dirs: &Dirs) -> PathBuf {
    dirs.config_home.join("xdg-desktop-portal").join(FILE_NAME)
}

/// The text of Sway Things' portal configuration.
pub fn text() -> String {
    format!("{HEADER}{LINES}\n")
}

/// Whether `environment.portal_config` is on in `state`.
pub fn enabled(state: &State) -> bool {
    matches!(state.get(PORTAL_CONFIG, None), Some(Value::Bool(true)))
}

/// The portal configuration the settings in `state` want: Sway Things' own while
/// `environment.portal_config` is on and the user has no configuration of their own there.
pub fn wanted(state: &State, dirs: &Dirs) -> Wanted {
    let on = enabled(state);
    let users_own = users_config(dirs).is_some();
    Wanted {
        path: path(dirs),
        text: (on && !users_own).then(text),
    }
}

/// Restarts the portals that run, so they read their configuration again; those that don't run
/// start with it when they are next needed.
///
/// # Errors
///
/// What `systemctl` reported.
pub async fn restart() -> Result<(), String> {
    let units: Vec<&str> = Component::ALL
        .iter()
        .map(|component| component.unit())
        .collect();
    let mut args = vec!["try-restart"];
    args.extend(units);
    crate::systemd::run(&args).await
}

#[cfg(test)]
mod tests;
