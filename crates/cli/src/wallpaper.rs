// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `hook wallpaper` and `wallpaper next`, which change the picture of a wallpaper collection,
//! and the timers that follow a change of one. What they do is [`sway_things_core::wallpaper`];
//! this only connects it to the session and reports.

use sway_things_core::command::output::OutputSelector;
use sway_things_core::generate::Validate;
use sway_things_core::i18n::tr;
use sway_things_core::msg;
use sway_things_core::state::StateError;
use sway_things_core::wallpaper::{self, Moment};

use super::change::fail;
use super::displays::Displays;
use super::{Exit, Io, Session, WallpaperCommand};

/// After a change of a wallpaper or collection setting in a session: starts the timers of the
/// collections again, so they follow the change.
pub(super) async fn follow_collections<V: Validate>(
    session: &mut Session<V>,
    key: &str,
    exit: Exit,
    io: &mut Io<'_>,
) -> Exit {
    let wallpaper = key.starts_with("wallpaper.") || key.starts_with("collection.");
    if exit != Exit::Ok || !wallpaper || session.applier.client().is_none() {
        return exit;
    }
    if wallpaper::has_timers() {
        if let Err(error) =
            wallpaper::reschedule(session.applier.state(), &wallpaper::program()).await
        {
            io.warn(error.text().translated());
        }
    }
    exit
}

/// `hook wallpaper`, run by the generated config when sway starts: the next picture of every
/// display's collection whose time has come, and their timers ([`wallpaper::login`]). Outside a
/// sway session there is nothing to do.
pub(super) async fn hook_wallpaper<V: Validate>(session: &mut Session<V>, io: &mut Io<'_>) -> Exit {
    let state = session.applier.state().clone();
    let Some(client) = session.applier.client() else {
        return Exit::Ok;
    };
    let program = wallpaper::has_timers().then(wallpaper::program);
    let failed = wallpaper::login(client, &state, program.as_deref()).await;
    for error in &failed {
        io.error(error.text().translated());
    }
    if failed.is_empty() {
        Exit::Ok
    } else {
        Exit::Failure
    }
}

/// `wallpaper next`, for the display of `--device` (one named by its name is the one stored by
/// its identifier) or, without one, for the collection of every display. With `--due` (the
/// timer), only if its time has come; the timer of a display that no longer shows a collection,
/// or of a session that ended, is stopped.
pub(super) async fn wallpaper<V: Validate>(
    session: &mut Session<V>,
    command: &WallpaperCommand,
    displays: &Displays,
    io: &mut Io<'_>,
) -> Exit {
    if let Some(name) = command
        .device()
        .filter(|name| OutputSelector::output(name).is_err())
    {
        let error = StateError::InvalidDevice {
            device: name.to_owned(),
        };
        return fail(io, &error.into());
    }
    let device = displays.resolve(Some(wallpaper::COLLECTION), command.device());
    let device = device.as_deref();
    let WallpaperCommand::Next { due, .. } = command;
    let moment = if *due { Moment::Timer } else { Moment::Now };
    if session.applier.client().is_none() {
        if moment == Moment::Timer {
            // The session the timer was started for has ended.
            wallpaper::unschedule(device).await;
            return Exit::Ok;
        }
        io.error(tr(msg!("not in a sway session")));
        return Exit::Failure;
    }
    let state = session.applier.state().clone();
    let shown = wallpaper::shown_on(&state, device);
    match wallpaper::next(session.applier.client(), &state, device, moment).await {
        Ok(_) => Exit::Ok,
        Err(_) if moment == Moment::Timer && shown.is_none() => {
            // The display no longer shows a collection.
            wallpaper::unschedule(device).await;
            Exit::Ok
        }
        Err(error) => {
            io.error(error.text().translated());
            Exit::Failure
        }
    }
}
