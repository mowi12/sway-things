// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `wallpaper start` and `wallpaper next`, which change the picture of a wallpaper collection,
//! and the timers that follow a change of one.

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

/// `wallpaper start` and `wallpaper next`, for the display of `--device` (one named by its name
/// is the one stored by its identifier) or, without one, for every display that shows a
/// collection (`start`) or the collection of every display (`next`).
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
    let moment = match command {
        WallpaperCommand::Start { .. } => Moment::Login,
        WallpaperCommand::Next { due: true, .. } => Moment::Timer,
        WallpaperCommand::Next { due: false, .. } => Moment::Now,
    };
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
    // At login, every display that shows a collection; else the one asked for.
    let displays: Vec<Option<String>> = if moment == Moment::Login && device.is_none() {
        wallpaper::shown(&state)
            .into_iter()
            .map(|shown| shown.output)
            .collect()
    } else {
        vec![device.map(ToOwned::to_owned)]
    };
    let mut exit = Exit::Ok;
    for display in &displays {
        let display = display.as_deref();
        let shown = wallpaper::shown_on(&state, display);
        let result = wallpaper::next(session.applier.client(), &state, display, moment).await;
        if let Err(error) = result {
            if moment == Moment::Timer && shown.is_none() {
                // The display no longer shows a collection.
                wallpaper::unschedule(display).await;
                continue;
            }
            io.error(error.text().translated());
            exit = Exit::Failure;
            continue;
        }
        if let (Some(shown), Moment::Login) = (shown, moment) {
            if wallpaper::has_timers() {
                let started =
                    wallpaper::schedule(display, shown.collection.change, &wallpaper::program())
                        .await;
                if let Err(error) = started {
                    io.warn(error.text().translated());
                }
            }
        }
    }
    exit
}
