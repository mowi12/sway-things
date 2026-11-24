// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `sway-things notifications …` and the hooks the notification daemon runs: Do Not Disturb
//! for a key or a bar module, the daemon that runs, a test notification, the click that shows
//! a notification's app, and the switches of swaync's button grid.

use std::fmt;
use std::io::Write;

use clap::{Subcommand, ValueEnum};
use sway_things_core::bluetooth::Bluez;
use sway_things_core::i18n::{tr, trf};
use sway_things_core::ipc::Client;
use sway_things_core::msg;
use sway_things_core::network::NetworkManager;
use sway_things_core::notifications::bus::Error as BusError;
use sway_things_core::notifications::{Daemon, Notifications, Server, click};
use sway_things_core::sound::Sound;

use sway_things_core::generate::Validate;
use sway_things_core::registry::Value;

use crate::change::{persist, write_access};
use crate::{Exit, Hook, Io, Session, error_line, fail_with};

/// A command of `sway-things notifications`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum NotificationsCommand {
    /// Show whether Do Not Disturb is on, or switch it: for mako, swaync and dunst
    Dnd {
        /// on, off or toggle; without, print the state (on or off)
        state: Option<Switch>,
    },
    /// Show which notification daemon runs and whether Do Not Disturb is on
    Status,
    /// Send a test notification through the daemon that runs
    Test,
    /// Read a swaync config.json (yours, by default) into the settings of the Notifications
    /// page as a starting point; your file isn't changed
    ImportSwaync {
        /// The config to read; by default ~/.config/swaync/config.json
        file: Option<std::path::PathBuf>,
    },
}

/// How to switch something on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Switch {
    /// Switch it on
    On,
    /// Switch it off
    Off,
    /// Switch it to the other state
    Toggle,
}

/// A switch of swaync's button grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Quick {
    /// Do Not Disturb of the notification daemon
    Dnd,
    /// Wi-Fi (NetworkManager)
    Wifi,
    /// Bluetooth (the first adapter)
    Bluetooth,
    /// The sound of the default output (on is not muted)
    Sound,
    /// The default microphone (on is not muted)
    Microphone,
}

/// What to do with a switch of swaync's button grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum QuickAction {
    /// Switch it on
    On,
    /// Switch it off
    Off,
    /// Switch it to the other state
    Toggle,
    /// Print true or false, for swaync's update-command
    State,
}

/// `notifications dnd|status|test` against the daemon on the session bus. `import-swaync`
/// changes settings and runs in a session ([`import_swaync`]).
pub(super) async fn notifications(
    command: &NotificationsCommand,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    let bus = match Notifications::session().await {
        Ok(bus) => bus,
        Err(error) => return fail_with(err, &error.text()),
    };
    match command {
        NotificationsCommand::Dnd { state } => dnd(&bus, *state, out, err).await,
        NotificationsCommand::Status => status(&bus, out, err).await,
        NotificationsCommand::Test => test(&bus, err).await,
        NotificationsCommand::ImportSwaync { .. } => Exit::Usage,
    }
}

/// `notifications import-swaync [<file>]`: stores what a swaync config sets as the settings of
/// the Notifications page, and the options without a setting as its other options.
pub(super) async fn import_swaync<V: Validate>(
    session: &mut Session<V>,
    file: Option<&std::path::Path>,
    io: &mut Io<'_>,
) -> Exit {
    use sway_things_core::notifications::{files::Files, swaync};
    use sway_things_core::registry::notifications::swaync::OTHER;
    let path = if let Some(path) = file {
        path.to_owned()
    } else if let Some(files) = Files::from_env() {
        files.swaync_user_config
    } else {
        io.error(tr(msg!("no home directory")));
        return Exit::Failure;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            io.error(trf(
                msg!("{file} can't be read: {error}"),
                &[("file", &path.display()), ("error", &error)],
            ));
            return Exit::Failure;
        }
    };
    let imported = match swaync::import(&text, &swaync::default_config()) {
        Ok(imported) => imported,
        Err(error) => {
            io.error(trf(
                msg!("{file} isn't a swaync config: {error}"),
                &[("file", &path.display()), ("error", &error)],
            ));
            return Exit::Failure;
        }
    };
    let Some(access) = write_access(session, io) else {
        return Exit::ReadOnly;
    };
    let mut values = imported.values.clone();
    values.push((OTHER, Value::Text(imported.other.clone())));
    for (key, value) in values {
        if let Err(error) = session.applier.set(key, None, value, &access).await {
            io.error(error.text().translated());
            return Exit::Failure;
        }
        io.out(key);
    }
    persist(session, false, &access, io).await
}

/// A hook the notification daemon or the generated config runs (`notification-click`,
/// `notification-daemon`, `quick`), with `client`, the sway of the daemon's session if there is
/// one. The same code runs for the user and in the tests.
pub(super) async fn run_notification_hook(
    hook: &Hook,
    client: Option<&mut Client>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    let bus = Notifications::session().await.ok();
    let environment = |name: &str| std::env::var(name).ok();
    match *hook {
        Hook::NotificationClick { mako } => {
            notification_click(bus.as_ref(), client, mako, &environment, err).await
        }
        Hook::NotificationDaemon => notification_daemon(bus.as_ref(), err).await,
        Hook::Quick { which, action } => {
            quick(which, action, bus.as_ref(), &environment, out, err).await
        }
        Hook::Gsettings
        | Hook::Displays
        | Hook::Workspaces
        | Hook::Mirror
        | Hook::NewDisplay
        | Hook::Clipboard
        | Hook::NightLight
        | Hook::Timezone
        | Hook::Launch { .. }
        | Hook::Autostart
        | Hook::Wallpaper => Exit::Ok,
    }
}

/// A hook of the notification daemon for the user who runs `sway-things`: `notification-click`
/// gets the sway of the session the daemon shows on, never one found elsewhere.
pub(super) async fn notification_hook_for_user(hook: &Hook, err: &mut dyn Write) -> Exit {
    let mut client = if matches!(hook, Hook::NotificationClick { .. }) && same_session() {
        Client::connect().await.ok()
    } else {
        None
    };
    run_notification_hook(hook, client.as_mut(), &mut std::io::stdout().lock(), err).await
}

/// Whether `$SWAYSOCK` is the sway of the session the notification daemon shows on
/// ([`click::same_session`]).
fn same_session() -> bool {
    match (
        std::env::var_os("SWAYSOCK"),
        std::env::var_os("XDG_RUNTIME_DIR"),
    ) {
        (Some(socket), Some(runtime)) => click::same_session(
            std::path::Path::new(&socket),
            std::path::Path::new(&runtime),
            std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        ),
        _ => false,
    }
}

/// What the doctor checks of the notification daemon: the one that runs on the session bus
/// (asked without starting one), the files for it and the daemons installed. `None` without a
/// session bus (at a TTY), where no daemon could run.
pub(super) async fn daemon_look() -> Option<sway_things_core::doctor::DaemonLook> {
    use sway_things_core::notifications::{files::Files, starts};
    let files = Files::from_env()?;
    let bus = Notifications::session().await.ok()?;
    let server = bus.server().await.ok()?;
    let running_config = server
        .as_ref()
        .and_then(|server| server.pid)
        .and_then(starts::config_arg);
    Some(sway_things_core::doctor::DaemonLook {
        files,
        server,
        activatable: starts::activatable(),
        running_config,
        installed: Daemon::KNOWN
            .into_iter()
            .filter(|daemon| daemon.installed())
            .collect(),
    })
}

/// The daemon that runs, or the error to print.
async fn server(bus: &Notifications) -> Result<Server, String> {
    match bus.server().await {
        Ok(Some(server)) => Ok(server),
        Ok(None) => Err(tr(msg!("no notification daemon runs")).to_owned()),
        Err(error) => Err(error.text().translated()),
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// `notifications dnd [on|off|toggle]`: prints the state afterwards, `on` or `off`.
pub async fn dnd(
    bus: &Notifications,
    switch: Option<Switch>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    let server = match server(bus).await {
        Ok(server) => server,
        Err(error) => {
            let _ = writeln!(err, "{}", error_line(&error));
            return Exit::Failure;
        }
    };
    let result = match switch {
        None => bus.dnd(&server).await,
        Some(Switch::On) => bus.set_dnd(&server, true).await,
        Some(Switch::Off) => bus.set_dnd(&server, false).await,
        Some(Switch::Toggle) => match bus.dnd(&server).await {
            Ok(on) => bus.set_dnd(&server, !on).await,
            Err(error) => Err(error),
        },
    };
    match result {
        Ok(on) => {
            let _ = writeln!(out, "{}", on_off(on));
            Exit::Ok
        }
        Err(error) => {
            let _ = writeln!(err, "{}", error_line(&error.text().translated()));
            Exit::Failure
        }
    }
}

/// `notifications status`: the daemon, its version and Do Not Disturb.
pub async fn status(bus: &Notifications, out: &mut dyn Write, err: &mut dyn Write) -> Exit {
    let server = match bus.server().await {
        Ok(Some(server)) => server,
        Ok(None) => {
            let _ = writeln!(out, "{}", tr(msg!("No notification daemon runs.")));
            return Exit::Ok;
        }
        Err(error) => {
            let _ = writeln!(err, "{}", error_line(&error.text().translated()));
            return Exit::Failure;
        }
    };
    let name = if server.daemon == Daemon::Other {
        server.name.clone()
    } else {
        server.daemon.title().to_owned()
    };
    // mako always reports 0.0.0.
    let line = if server.version.is_empty() || server.version == "0.0.0" {
        trf(msg!("Daemon: {daemon}"), &[("daemon", &name)])
    } else {
        trf(
            msg!("Daemon: {daemon} {version}"),
            &[("daemon", &name), ("version", &server.version)],
        )
    };
    let _ = writeln!(out, "{line}");
    let dnd = match bus.dnd(&server).await {
        Ok(on) => on_off(on).to_owned(),
        Err(BusError::Unsupported(_)) => tr(msg!("not supported")).to_owned(),
        Err(error) => error.text().translated(),
    };
    let _ = writeln!(
        out,
        "{}",
        trf(msg!("Do Not Disturb: {state}"), &[("state", &dnd)])
    );
    Exit::Ok
}

/// `notifications test`: one notification, to see how they look.
pub async fn test(bus: &Notifications, err: &mut dyn Write) -> Exit {
    let server = match server(bus).await {
        Ok(server) => server,
        Err(error) => {
            let _ = writeln!(err, "{}", error_line(&error));
            return Exit::Failure;
        }
    };
    match bus
        .notify(
            &server,
            tr(msg!("This is how notifications look")),
            tr(msg!(
                "Change them on the Notifications page of Sway Things."
            )),
        )
        .await
    {
        Ok(_) => Exit::Ok,
        Err(error) => {
            let _ = writeln!(err, "{}", error_line(&error.text().translated()));
            Exit::Failure
        }
    }
}

/// `hook notification-click`: shows the app of the notification that was clicked. swaync runs
/// it as a script with the notification in its environment (`SWAYNC_DESKTOP_ENTRY`,
/// `SWAYNC_APP_NAME`); mako runs it with `--mako <id>`, and the hook invokes the default action
/// mako's click would have. Always succeeds, so swaync doesn't report a failed script as a
/// notification; why nothing happened goes to `err`.
pub async fn notification_click(
    bus: Option<&Notifications>,
    client: Option<&mut Client>,
    mako: Option<u32>,
    environment: &dyn Fn(&str) -> Option<String>,
    err: &mut dyn Write,
) -> Exit {
    let mut say = |text: &dyn fmt::Display| {
        let _ = writeln!(err, "sway-things hook notification-click: {text}");
    };
    let server = match bus {
        Some(bus) => bus.server().await.ok().flatten(),
        None => None,
    };
    let (entry, app) = match (mako, bus, &server) {
        (Some(id), Some(bus), Some(server)) if server.daemon == Daemon::Mako => {
            let found = bus.mako_notification(server, id).await.ok().flatten();
            if let Err(error) = bus.mako_click(server, id).await {
                say(&error.text().translated());
            }
            found.unwrap_or_default()
        }
        (Some(_), _, _) => {
            say(&"mako doesn't run");
            return Exit::Ok;
        }
        (None, _, _) => (
            environment("SWAYNC_DESKTOP_ENTRY").unwrap_or_default(),
            environment("SWAYNC_APP_NAME").unwrap_or_default(),
        ),
    };
    if let (Some(bus), Some(server)) = (bus, &server) {
        if let Err(error) = bus.close_panel(server).await {
            say(&error.text().translated());
        }
    }
    let Some(client) = client else {
        say(&"no sway session of this session's notification daemon");
        return Exit::Ok;
    };
    let tree = match client.tree().await {
        Ok(tree) => tree,
        Err(error) => {
            say(&error.text().translated());
            return Exit::Ok;
        }
    };
    let workspaces = match client.workspaces().await {
        Ok(found) => click::Workspaces {
            focused: found
                .iter()
                .find(|workspace| workspace.focused)
                .map(|workspace| workspace.name.clone()),
            visible: found
                .iter()
                .filter(|workspace| workspace.visible)
                .map(|workspace| workspace.name.clone())
                .collect(),
        },
        Err(_) => click::Workspaces::default(),
    };
    let windows = click::windows(&tree);
    let Some(window) = click::find(&windows, &entry, &app, &workspaces) else {
        say(&format_args!(
            "no window for the desktop entry {entry:?} or the app {app:?}"
        ));
        return Exit::Ok;
    };
    let id = window.id;
    match click::focus(window) {
        Ok(focus) => {
            if let Err(error) = client.run(&focus).await {
                say(&error.text().translated());
                return Exit::Ok;
            }
        }
        Err(error) => {
            say(&error);
            return Exit::Ok;
        }
    }
    // Where the window is now that it has the focus: its workspace may have come to the screen,
    // a window from the scratchpad may show.
    let moved = client.tree().await.ok().and_then(|tree| {
        click::windows(&tree)
            .into_iter()
            .find(|window| window.id == id)
    });
    if let Some(warp) = moved.as_ref().and_then(click::warp) {
        if let Err(error) = client.run(&warp).await {
            say(&error.text().translated());
        }
    }
    Exit::Ok
}

/// `hook quick <switch> [on|off|toggle|state]`: a switch of swaync's button grid. Without an
/// action it takes the state swaync gives a toggle button (`SWAYNC_TOGGLE_STATE`), else
/// toggles. `state` prints `true` or `false`.
pub async fn quick(
    which: Quick,
    action: Option<QuickAction>,
    bus: Option<&Notifications>,
    environment: &dyn Fn(&str) -> Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    let action = action.unwrap_or_else(|| match environment("SWAYNC_TOGGLE_STATE").as_deref() {
        Some("true") => QuickAction::On,
        Some("false") => QuickAction::Off,
        _ => QuickAction::Toggle,
    });
    let result = switch_quick(which, action, bus).await;
    match result {
        Ok(on) => {
            if action == QuickAction::State {
                let _ = writeln!(out, "{on}");
            }
            Exit::Ok
        }
        Err(error) => {
            let _ = writeln!(err, "{}", error_line(&error));
            if action == QuickAction::State {
                let _ = writeln!(out, "false");
            }
            Exit::Failure
        }
    }
}

/// Reads and maybe changes a switch; returns whether it is on afterwards.
async fn switch_quick(
    which: Quick,
    action: QuickAction,
    bus: Option<&Notifications>,
) -> Result<bool, String> {
    let wanted = |on: bool| match action {
        QuickAction::On => Some(true),
        QuickAction::Off => Some(false),
        QuickAction::Toggle => Some(!on),
        QuickAction::State => None,
    };
    let text = |error: &dyn fmt::Display| error.to_string();
    match which {
        Quick::Dnd => {
            let bus = bus.ok_or_else(|| tr(msg!("no session bus")).to_owned())?;
            let server = server(bus).await?;
            let on = bus.dnd(&server).await.map_err(|e| e.text().translated())?;
            match wanted(on) {
                Some(new) => bus
                    .set_dnd(&server, new)
                    .await
                    .map_err(|e| e.text().translated()),
                None => Ok(on),
            }
        }
        Quick::Wifi => {
            let nm = NetworkManager::connect().await.map_err(|e| text(&e))?;
            let on = nm.snapshot().await.map_err(|e| text(&e))?.wifi_enabled;
            match wanted(on) {
                Some(new) => {
                    nm.set_wifi_enabled(new).await.map_err(|e| text(&e))?;
                    Ok(new)
                }
                None => Ok(on),
            }
        }
        Quick::Bluetooth => {
            let bluez = Bluez::connect().await.map_err(|e| text(&e))?;
            let snapshot = bluez.snapshot().await.map_err(|e| text(&e))?;
            let adapter = snapshot
                .adapters
                .first()
                .ok_or_else(|| tr(msg!("no Bluetooth adapter")).to_owned())?;
            match wanted(adapter.powered) {
                Some(new) => {
                    bluez
                        .set_powered(&adapter.path, new)
                        .await
                        .map_err(|e| text(&e))?;
                    Ok(new)
                }
                None => Ok(adapter.powered),
            }
        }
        Quick::Sound | Quick::Microphone => {
            let sound = Sound::connect().map_err(|e| text(&e))?;
            let snapshot = sound.snapshot().await.map_err(|e| text(&e))?;
            let output = which == Quick::Sound;
            let device = if output {
                snapshot.output()
            } else {
                snapshot.input()
            }
            .ok_or_else(|| tr(msg!("no sound device")).to_owned())?;
            let on = !device.muted;
            match wanted(on) {
                Some(new) => {
                    let result = if output {
                        sound.set_output_muted(device.index, !new).await
                    } else {
                        sound.set_input_muted(device.index, !new).await
                    };
                    result.map_err(|e| text(&e))?;
                    Ok(new)
                }
                None => Ok(on),
            }
        }
    }
}

/// `hook notification-daemon`: starts the notification daemon chosen on the Notifications page
/// (`notifications.start`), unless one runs already: swaync with the files of Sway Things,
/// written first if they aren't there. The generated config runs it whenever sway starts or
/// reloads.
pub async fn notification_daemon(bus: Option<&Notifications>, err: &mut dyn Write) -> Exit {
    use sway_things_core::notifications::files::Files;
    use sway_things_core::notifications::{Stored, mako, spawn, swaync};
    use sway_things_core::registry::Registry;
    use sway_things_core::registry::notifications::START;
    use sway_things_core::state::{Paths, State};
    let mut say = |text: &dyn fmt::Display| {
        let _ = writeln!(err, "{}", error_line(text));
    };
    let paths = match Paths::from_env() {
        Ok(paths) => paths,
        Err(error) => {
            say(&error.text().translated());
            return Exit::Failure;
        }
    };
    let state = State::load(Registry::builtin(), paths);
    let daemon = match state.get(START, None) {
        Some(Value::Choice(program)) => Daemon::from_program(program),
        _ => None,
    };
    let (Some(daemon), Some(files)) = (daemon, Files::from_env()) else {
        return Exit::Ok;
    };
    if let Some(bus) = bus {
        if matches!(bus.server().await, Ok(Some(_))) {
            return Exit::Ok;
        }
    }
    let stored = Stored::new(&state);
    // Our files first: mako rejects a config whose include it can't find, and swaync falls back
    // to the user's own config when a file it is given is missing.
    if daemon == Daemon::Mako && !files.mako_ours.is_file() {
        if let Err(error) = files.write_mako(&mako::generate(&stored), true) {
            say(&error.text().translated());
        }
    }
    let mut ours = true;
    if daemon == Daemon::Swaync {
        if !(files.swaync_config.is_file() && files.swaync_style.is_file()) {
            let config = swaync::config(&stored, swaync::default_config());
            let style = swaync::style(&stored, swaync::Gtk::installed());
            if let Err(error) = files.write_swaync(&config, &style) {
                say(&error.text().translated());
            }
        }
        ours = files.swaync_config.is_file() && files.swaync_style.is_file();
        if !ours {
            say(&tr(msg!(
                "the files of Sway Things for swaync aren't there, so swaync starts with its own \
                 config"
            )));
        }
    }
    match spawn(daemon, &files, ours) {
        Ok(()) => Exit::Ok,
        Err(error) => {
            say(&trf(
                msg!("{program} can't be started: {error}"),
                &[("program", &daemon.program()), ("error", &error)],
            ));
            Exit::Failure
        }
    }
}
