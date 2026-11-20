// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The hooks generated config runs: `gsettings` in the session, and `displays`, `workspaces` and
//! `mirror` through [`sway_things_core::outputs::hooks`].

use std::io::Write;

use sway_things_core::generate::Validate;
use sway_things_core::i18n::trf;
use sway_things_core::ipc::Client;
use sway_things_core::msg;
use sway_things_core::outputs::hooks::{self, HookEnv};
use sway_things_core::outputs::{self};

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

/// kanshi's files for `doctor`, if kanshi is installed.
pub(super) fn kanshi_files() -> Option<outputs::kanshi::KanshiFiles> {
    outputs::kanshi::KanshiFiles::from_env().filter(|_| outputs::kanshi::installed())
}

/// A hook of the displays for the user who runs `sway-things`: their settings files, and the
/// session of `$SWAYSOCK` or, for a hook kanshi runs, sway's newest socket ([`hooks::connect`]).
pub(super) async fn hook_for_user(hook: Hook, err: &mut dyn Write) -> Exit {
    let mut client = match hooks::connect().await {
        Ok(client) => client,
        Err(error) => return fail_with(err, &error.text()),
    };
    run_display_hook(hook, &mut client, &HookEnv::from_env(), err).await
}

/// A hook of the displays (`displays`, `workspaces`, `mirror`) on `client` with the files of
/// `env`, as [`outputs::hooks`] runs it.
pub(super) async fn run_display_hook(
    hook: Hook,
    client: &mut Client,
    env: &HookEnv,
    err: &mut dyn Write,
) -> Exit {
    let done = match hook {
        Hook::Displays => return hook_displays(client, env, err).await,
        Hook::Workspaces => hooks::workspaces(client, env).await,
        Hook::Mirror => hooks::mirror(client, env).await,
        Hook::Gsettings => return Exit::Ok,
    };
    match done {
        Ok(_) => Exit::Ok,
        Err(error) => fail_with(err, &error.text()),
    }
}

/// A hook in a session that runs already (as the tests do; for the user, `run_for_user` runs
/// them without loading the settings).
pub(super) async fn run_hook<V: Validate>(
    hook: Hook,
    session: &mut Session<V>,
    io: &mut Io<'_>,
) -> Exit {
    if hook == Hook::Gsettings {
        return hook_gsettings(session, io);
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
