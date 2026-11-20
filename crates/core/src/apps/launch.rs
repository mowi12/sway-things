// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Starting an app by its desktop file ID, for `sway-things hook launch <ID>`: the autostart
//! entries of Sway Things that are apps run this, so the desktop entry is read when the app
//! starts, and its command line is started as an argument array, never through a shell.

use std::path::PathBuf;

use super::Dirs;
use super::entry::ExecError;
use super::installed::{self, App};
use crate::i18n::Text;
use crate::msg;

/// The terminals an app with `Terminal=true` can be started in, each with the argument before
/// the command, in the order they are tried. `xdg-terminal-exec` comes first: it follows the
/// default terminal (`sway-xdg-terminals.list`). Each takes the command as separate arguments
/// after the one given; terminals whose option takes one string (Tilix's `-e`) aren't here.
const TERMINALS: &[(&str, Option<&str>)] = &[
    ("xdg-terminal-exec", None),
    ("foot", None),
    ("alacritty", Some("-e")),
    ("kitty", None),
    ("ghostty", Some("-e")),
    // Console takes the rest of the line after `--` as the command, like `-e`.
    ("kgx", Some("--")),
    ("gnome-terminal", Some("--")),
    ("konsole", Some("-e")),
    ("xfce4-terminal", Some("-x")),
    ("xterm", Some("-e")),
];

/// Why an app can't be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// No installed app has the ID.
    NotFound(String),
    /// The app's entry says it is deleted (`Hidden=true`).
    Hidden(String),
    /// The program the entry's `TryExec` names isn't installed.
    NotInstalled(String),
    /// It has no `Exec` and is started through D-Bus, which failed.
    Activation(String),
    /// Its command line can't be used.
    Command(ExecError),
    /// It runs in a terminal, and none is installed.
    NoTerminal,
    /// The program couldn't be started.
    Spawn(String),
}

impl LaunchError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::NotFound(id) => Text::new(msg!("No installed app has the ID {id}")).arg("id", id),
            Self::Hidden(id) => {
                Text::new(msg!("The app {id} is switched off (Hidden=true)")).arg("id", id)
            }
            Self::NotInstalled(program) => Text::new(msg!(
                "The app isn't installed: its program {program} (TryExec) is missing"
            ))
            .arg("program", program),
            Self::Activation(error) => Text::new(msg!(
                "The app has no command line and couldn't be started through D-Bus: {error}"
            ))
            .arg("error", error),
            Self::Command(_) => msg!("The app's command line (Exec) can't be read").into(),
            Self::NoTerminal => msg!(
                "The app runs in a terminal, and none is installed (xdg-terminal-exec, foot, …)"
            )
            .into(),
            Self::Spawn(error) => {
                Text::new(msg!("The app can't be started: {error}")).arg("error", error)
            }
        }
    }
}

/// The arguments and the working directory that start `app`: its command line, in a terminal
/// if it asks for one.
///
/// # Errors
///
/// [`LaunchError::Command`] for a command line that can't be read, [`LaunchError::NoTerminal`].
pub fn command(dirs: &Dirs, app: &App) -> Result<(Vec<String>, Option<PathBuf>), LaunchError> {
    let mut args = app
        .entry
        .command(&app.path, &dirs.languages)
        .map_err(LaunchError::Command)?;
    if app.terminal {
        let (terminal, before) = TERMINALS
            .iter()
            .find(|(program, _)| dirs.find_program(program).is_some())
            .ok_or(LaunchError::NoTerminal)?;
        let mut wrapped = vec![(*terminal).to_owned()];
        wrapped.extend(before.map(str::to_owned));
        wrapped.append(&mut args);
        args = wrapped;
    }
    let dir = app
        .entry
        .string("Path")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute() && dir.is_dir());
    Ok((args, dir))
}

/// Starts the installed app of ID `id` in a process group of its own, without waiting for it.
/// An app without `Exec` that is `DBusActivatable` is activated over the session bus
/// (`org.freedesktop.Application.Activate`), as the Desktop Entry Specification describes.
///
/// # Errors
///
/// A [`LaunchError`] if there is no such app or it can't be started.
pub async fn launch(dirs: &Dirs, id: &str) -> Result<(), LaunchError> {
    use std::os::unix::process::CommandExt;
    let app = installed::lookup(dirs, id).map_err(|missing| match missing {
        installed::Missing::NotFound => LaunchError::NotFound(id.to_owned()),
        installed::Missing::Hidden => LaunchError::Hidden(id.to_owned()),
        installed::Missing::TryExec(program) => LaunchError::NotInstalled(program),
    })?;
    if app.entry.raw("Exec").is_none() && app.entry.boolean("DBusActivatable") {
        return activate(id).await.map_err(LaunchError::Activation);
    }
    let (args, dir) = command(dirs, &app)?;
    let (program, rest) = args
        .split_first()
        .ok_or(LaunchError::Command(ExecError::Empty))?;
    let mut command = std::process::Command::new(program);
    command
        .args(rest)
        .stdin(std::process::Stdio::null())
        .process_group(0);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| LaunchError::Spawn(error.to_string()))
}

/// The D-Bus name and object path of the app of desktop file ID `id`, as the specification
/// derives them: the ID without `.desktop`, and that with `.` as `/` and `-` as `_`.
pub fn bus_name_and_path(id: &str) -> Option<(String, String)> {
    let name = id.strip_suffix(".desktop")?;
    // A bus name has at least two elements.
    if !name.contains('.') || name.split('.').any(str::is_empty) {
        return None;
    }
    let path = format!("/{}", name.replace('.', "/").replace('-', "_"));
    Some((name.to_owned(), path))
}

/// Activates the app `id` over the session bus.
async fn activate(id: &str) -> Result<(), String> {
    let (name, path) = bus_name_and_path(id).ok_or_else(|| format!("{id} isn't a D-Bus name"))?;
    let connection = zbus::Connection::session()
        .await
        .map_err(|error| error.to_string())?;
    let path = zbus::zvariant::ObjectPath::try_from(path).map_err(|error| error.to_string())?;
    let platform_data: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> =
        std::collections::HashMap::new();
    connection
        .call_method(
            Some(name.as_str()),
            path,
            Some("org.freedesktop.Application"),
            "Activate",
            &(platform_data,),
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}
