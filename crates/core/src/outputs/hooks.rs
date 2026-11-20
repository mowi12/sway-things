// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The hooks that generated config runs, so nothing of Sway Things has to keep running:
//! `sway-things hook displays`, `hook workspaces` and `hook mirror`. The command line only
//! connects and prints; what each hook does is here, so the user's run and the tests run the same
//! code.
//!
//! A hook reads the settings files and the remembered displays from [`HookEnv`] and talks to sway
//! over the client it is given. It never writes the settings; it writes only kanshi's file of
//! ours ([`KanshiFiles::refresh`]) and the remembered displays (the workspaces a picture took
//! along).

use std::path::PathBuf;
use std::time::Duration;

use crate::command::SwayCmd;
use crate::ipc::{Client, IpcError};
use crate::outputs::kanshi::{self, Inclusion, KanshiFiles};
use crate::outputs::profiles::{Profile, Profiles};
use crate::outputs::{self, mirror, workspaces};
use crate::registry::Registry;
use crate::state::{Paths, State};

/// What a hook reads besides sway: the settings files (for the placement rule and the
/// remembered displays) and kanshi's files.
#[derive(Debug, Clone)]
pub struct HookEnv {
    /// The settings files of the user; without them, a hook does only what needs none.
    pub paths: Option<Paths>,
    /// kanshi's config and our file for it, if kanshi is installed. Without them a hook leaves
    /// kanshi out entirely, also a kanshi that runs, so a test never reaches the machine's.
    pub kanshi: Option<KanshiFiles>,
}

impl HookEnv {
    /// The files of the user who runs the hook, from the environment as the app finds them.
    pub fn from_env() -> Self {
        Self {
            paths: Paths::from_env().ok(),
            kanshi: KanshiFiles::from_env().filter(|_| kanshi::installed()),
        }
    }

    fn profiles(&self) -> Option<Profiles> {
        self.paths
            .as_ref()
            .and_then(|paths| Profiles::load(paths.displays()).ok())
    }

    /// Whether kanshi sets up the displays: installed, as far as the hook knows, and running.
    fn kanshi_runs(&self) -> bool {
        self.kanshi.is_some() && outputs::kanshi_runs()
    }

    fn state(&self) -> Option<State> {
        self.paths
            .clone()
            .map(|paths| State::load(Registry::builtin(), paths))
    }
}

/// `hook displays`, run by `20-output.conf` at every start and reload of sway: makes sure
/// kanshi can read its config and applies its profile again, switches a display on if none is
/// on, and starts the mirrors again. Returns the display it switched on.
///
/// # Errors
///
/// The IPC error of switching a display on; the other steps are best effort.
pub async fn displays(client: &mut Client, env: &HookEnv) -> Result<Option<String>, IpcError> {
    // The file of ours for kanshi is there before anything else: kanshi rejects its whole config
    // over an include it can't find, and then sets up nothing at all. Where it was missing (or
    // emptied) and kanshi gave up over it, kanshi is started now that it can read its config.
    if let Some(profiles) = env.profiles() {
        if refresh_kanshi(env, &profiles).await {
            async_io::Timer::after(Duration::from_millis(500)).await;
            if !env.kanshi_runs() {
                let _ = client.run(&SwayCmd::new("exec").keyword("kanshi")).await;
                async_io::Timer::after(Duration::from_millis(500)).await;
            }
        }
    }
    // Then, sway having just read its config: kanshi applies its profile again over whatever
    // the config set for the displays. Best effort; without kanshictl kanshi keeps what it has.
    if env.kanshi_runs() {
        kanshi::reload().await;
    }
    // A display is on in the end, whatever kanshi did.
    let ensured = outputs::ensure_one_on(client).await;
    // The mirrors may be gone: started again (nothing changes where kanshi's profile did it
    // already). The workspaces are placed by `hook workspaces`, which runs at the same time.
    let _ = mirror(client, env).await;
    ensured
}

/// `hook workspaces`: puts the workspaces on the displays by the rule of the Workspaces page,
/// as remembered for the displays connected now. Returns how many workspaces it moved.
///
/// # Errors
///
/// The IPC errors.
pub async fn workspaces(client: &mut Client, env: &HookEnv) -> Result<usize, IpcError> {
    let Some(state) = env.state() else {
        return Ok(0);
    };
    let profiles = env.profiles();
    // After a start or reload of sway, kanshi puts the displays in place a moment later: what
    // is left and right is read once they stand.
    if workspaces::Rule::of(&state).places() && env.kanshi_runs() {
        workspaces::settled(client, profiles.as_ref()).await?;
    }
    let placed = workspaces::apply(client, &state, profiles.as_ref()).await;
    if let Some(mut profiles) = profiles {
        // A picture of this combination that went (its window closed, say) comes back.
        let _ = mirror::heal(client, &mut profiles).await;
        // kanshi just applied a profile, maybe a copy of the user's own made before they
        // changed it: the copies are made again, and kanshi reads them if they changed (and
        // runs this hook once more, which then finds them as they are).
        let _ = refresh_kanshi(env, &profiles).await;
    }
    placed
}

/// `hook mirror`, run by kanshi after it set up a profile: ends the mirrors Sway Things started
/// and starts those of the displays connected now, as [`mirror::apply`]. Without remembered
/// profiles that mirror, nothing happens. Returns how many mirrors it started.
///
/// # Errors
///
/// The IPC errors of reading the outputs and of moving the workspaces of a mirrored display.
pub async fn mirror(client: &mut Client, env: &HookEnv) -> Result<usize, IpcError> {
    let Some(mut profiles) = env.profiles() else {
        return Ok(0);
    };
    if !profiles.iter().any(Profile::mirrors) {
        return Ok(0);
    }
    // The workspaces of a display that shows another one's picture go to that one first, noted
    // so they come back when it shows its own content again.
    let started = mirror::start(client, &mut profiles).await?;
    // Last: kanshi runs this hook again after reading its file, and finds everything noted.
    let _ = refresh_kanshi(env, &profiles).await;
    Ok(started)
}

/// Writes kanshi's file of ours again if it says something else than `profiles` do now, and has
/// kanshi read it: `wl-mirror` came or went since it was written, so a display stands apart for
/// a picture or no longer does, or the file was missing or emptied. Only a file of ours, where it
/// is there already or the user's kanshi config includes it. Returns whether it wrote the file.
async fn refresh_kanshi(env: &HookEnv, profiles: &Profiles) -> bool {
    let Some(files) = &env.kanshi else {
        return false;
    };
    // Where the user's config includes ours, the file must be there: kanshi rejects its whole
    // config over an include it can't find.
    if !files.ours.is_file() && files.inclusion() != Inclusion::Included {
        return false;
    }
    // With the hook for the workspaces while a rule places them, as the app writes it.
    let Some(state) = env.state() else {
        return false;
    };
    matches!(
        files
            .refresh(profiles, workspaces::Placing::of(&state))
            .await,
        Ok(true)
    )
}

/// The session for a hook kanshi runs: `$SWAYSOCK`, or where kanshi was started without it (a
/// systemd unit, say), sway's newest socket in `$XDG_RUNTIME_DIR`.
///
/// # Errors
///
/// The error of connecting.
pub async fn connect() -> Result<Client, IpcError> {
    if std::env::var_os("SWAYSOCK").is_some() {
        return Client::connect().await;
    }
    match newest_socket() {
        Some(socket) => Client::connect_to(socket).await,
        None => Client::connect().await,
    }
}

/// sway's newest IPC socket in `$XDG_RUNTIME_DIR`.
fn newest_socket() -> Option<PathBuf> {
    std::fs::read_dir(std::env::var_os("XDG_RUNTIME_DIR")?)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("sway-ipc.") && name.ends_with(".sock")
        })
        .max_by_key(|entry| entry.metadata().and_then(|meta| meta.modified()).ok())
        .map(|entry| entry.path())
}
