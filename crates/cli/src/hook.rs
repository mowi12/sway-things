// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The hooks generated config runs: `gsettings` and `wallpaper` in the session, `displays`,
//! `workspaces`, `mirror` and `new-display` through [`sway_things_core::outputs::hooks`],
//! `clipboard` through [`sway_things_core::capture::clipboard`], and `launch` and `autostart`
//! through [`sway_things_core::apps`], which need neither the settings nor sway.

use std::io::Write;

use sway_things_core::apps;
use sway_things_core::capture::clipboard::{self, Synced};
use sway_things_core::capture::{self as capture, HISTORY, Settings};
use sway_things_core::generate::Validate;
use sway_things_core::i18n::{Text, trf};
use sway_things_core::ipc::Client;
use sway_things_core::msg;
use sway_things_core::outputs::hooks::{self, HookEnv};
use sway_things_core::outputs::{self};
use sway_things_core::registry::Registry;
use sway_things_core::state::{Paths, State};

use super::{Exit, Hook, Io, Session, fail_with};

/// `hook displays`: switches a display on if none is on ([`hooks::displays`]). Quiet when there
/// is nothing to do; it says on stderr which display it switched on, for sway's log.
pub(super) async fn hook_displays(client: &mut Client, env: &HookEnv, err: &mut dyn Write) -> Exit {
    match hooks::displays(client, env).await {
        Ok(Some(name)) => {
            let _ = writeln!(
                err,
                "{}",
                trf(
                    // TRANSLATORS: `sway-things hook displays` switched on a display because no
                    // other one was on.
                    msg!("No display was on, so {display} was switched on."),
                    &[("display", &name)]
                )
            );
            Exit::Ok
        }
        Ok(None) => Exit::Ok,
        Err(error) => fail_with(err, &error.text()),
    }
}

/// `hook new-display`: asks about a display plugged in to a combination set up here and
/// remembers the answer ([`hooks::new_display`]). Without an answer, nothing changes: the display
/// extends the desktop. Why it didn't ask goes to stderr, for kanshi's log.
async fn hook_new_display(client: &mut Client, env: &HookEnv, err: &mut dyn Write) -> Exit {
    match hooks::new_display(client, env).await {
        Ok(hooks::NewDisplay::Nothing | hooks::NewDisplay::Remembered) => Exit::Ok,
        Ok(hooks::NewDisplay::NotAsked(text)) => {
            let _ = writeln!(err, "{}", text.translated());
            Exit::Ok
        }
        Ok(hooks::NewDisplay::NotRemembered) => Exit::Failure,
        Err(error) => fail_with(err, &error.text()),
    }
}

/// `hook launch <ID>`: starts the installed app `id` as its desktop entry says, and returns at
/// once.
async fn hook_launch(id: &str, err: &mut dyn Write) -> Exit {
    let Some(dirs) = apps::Dirs::from_env() else {
        return fail_with(err, &Text::from(msg!("HOME isn't set")));
    };
    match apps::launch::launch(&dirs, id).await {
        Ok(()) => Exit::Ok,
        Err(error) => fail_with(err, &error.text()),
    }
}

/// `hook autostart`: starts the XDG autostart entries through the systemd user session.
async fn hook_autostart(err: &mut dyn Write) -> Exit {
    match apps::autostart::start_with_systemd().await {
        Ok(()) => Exit::Ok,
        Err(error) => {
            let _ = writeln!(err, "{}", super::error_line(&error));
            Exit::Failure
        }
    }
}

/// A hook of the apps (`launch`, `autostart`), which runs the same for the user and in a
/// session; `None` for the other hooks.
async fn run_app_hook(hook: &Hook, err: &mut dyn Write) -> Option<Exit> {
    match hook {
        Hook::Launch { id } => Some(hook_launch(id, err).await),
        Hook::Autostart => Some(hook_autostart(err).await),
        Hook::Gsettings
        | Hook::Displays
        | Hook::Workspaces
        | Hook::Mirror
        | Hook::NewDisplay
        | Hook::Clipboard
        | Hook::Wallpaper => None,
    }
}

/// kanshi's files for `doctor`, if kanshi is installed.
pub(super) fn kanshi_files() -> Option<outputs::kanshi::KanshiFiles> {
    outputs::kanshi::KanshiFiles::from_env().filter(|_| outputs::kanshi::installed())
}

/// A hook for the user who runs `sway-things`: one of the apps as it is, one of the displays with
/// their settings files and the session of `$SWAYSOCK` or, for a hook kanshi runs, sway's newest
/// socket ([`hooks::connect`]).
pub(super) async fn hook_for_user(hook: &Hook, err: &mut dyn Write) -> Exit {
    if let Some(exit) = run_app_hook(hook, err).await {
        return exit;
    }
    let mut client = match hooks::connect().await {
        Ok(client) => client,
        Err(error) => return fail_with(err, &error.text()),
    };
    run_display_hook(hook, &mut client, &HookEnv::from_env(), err).await
}

/// A hook of the displays (`displays`, `workspaces`, `mirror`, `new-display`) on `client` with
/// the files of `env`, as [`outputs::hooks`] runs it.
pub(super) async fn run_display_hook(
    hook: &Hook,
    client: &mut Client,
    env: &HookEnv,
    err: &mut dyn Write,
) -> Exit {
    let done = match hook {
        Hook::Displays => return hook_displays(client, env, err).await,
        Hook::NewDisplay => return hook_new_display(client, env, err).await,
        Hook::Workspaces => hooks::workspaces(client, env).await,
        Hook::Mirror => hooks::mirror(client, env).await,
        Hook::Gsettings
        | Hook::Wallpaper
        | Hook::Clipboard
        | Hook::Launch { .. }
        | Hook::Autostart => {
            return Exit::Ok;
        }
    };
    match done {
        Ok(_) => Exit::Ok,
        Err(error) => fail_with(err, &error.text()),
    }
}

/// A hook in a session that runs already (as the tests do; for the user, `run_for_user` runs
/// them without loading the settings).
pub(super) async fn run_hook<V: Validate>(
    hook: &Hook,
    session: &mut Session<V>,
    io: &mut Io<'_>,
) -> Exit {
    match hook {
        Hook::Gsettings => return hook_gsettings(session, io),
        Hook::Wallpaper => return super::wallpaper::hook_wallpaper(session, io).await,
        // Asks the user through the notification server: only for the user's session.
        Hook::NewDisplay => return Exit::Ok,
        Hook::Clipboard => {
            let state = session.applier.state();
            let history = (
                capture::is_on(state, HISTORY),
                Settings::of(state).max_entries,
            );
            return hook_clipboard(|| history, io.err);
        }
        Hook::Displays
        | Hook::Workspaces
        | Hook::Mirror
        | Hook::Launch { .. }
        | Hook::Autostart => {}
    }
    if let Some(exit) = run_app_hook(hook, io.err).await {
        return exit;
    }
    let env = HookEnv {
        paths: Some(session.applier.state().paths().clone()),
        kanshi: session.applier.kanshi_files().cloned(),
    };
    match session.applier.client() {
        Some(client) => run_display_hook(hook, client, &env, io.err).await,
        None => Exit::Ok,
    }
}

/// `hook gsettings`: writes every stored setting with a GSettings key to GSettings. Keys Sway
/// Things doesn't store stay as they are.
pub(super) fn hook_gsettings<V: Validate>(session: &Session<V>, io: &mut Io<'_>) -> Exit {
    let failed = session.applier.copy_stored_to_gsettings();
    for (_, error) in &failed {
        io.error(error.text().translated());
    }
    if failed.is_empty() {
        Exit::Ok
    } else {
        Exit::Failure
    }
}

/// The switch and the number of entries of the clipboard history in the settings files of the
/// user who runs `sway-things`; off if they can't be read.
pub(super) fn read_history() -> (bool, u32) {
    let settings = Settings::load();
    let on = Paths::from_env()
        .is_ok_and(|paths| capture::is_on(&State::load(Registry::builtin(), paths), HISTORY));
    (on, settings.max_entries)
}

/// `hook clipboard`: has the watcher of the history run as `history` says (the switch and the
/// number of entries), read once the lock is held ([`clipboard::sync_with`]). Says on stderr why
/// it starts none.
pub(super) fn hook_clipboard(history: impl FnOnce() -> (bool, u32), err: &mut dyn Write) -> Exit {
    match clipboard::sync_with(history) {
        Ok(Synced {
            foreign: Some(pid), ..
        }) => {
            let _ = writeln!(
                err,
                "{}",
                trf(
                    msg!(
                        "A clipboard history of your own runs already (wl-paste, process \
                         {pid}), so Sway Things starts none"
                    ),
                    &[("pid", &pid)],
                )
            );
            Exit::Ok
        }
        Ok(_) => Exit::Ok,
        Err(error) => fail_with(err, &error.text()),
    }
}
