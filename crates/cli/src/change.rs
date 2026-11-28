// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The commands that read and change settings: `get`, `set`, `reset` and `show-generated`,
//! with the safety net for risky settings and the saving afterwards.

use sway_things_core::apply::{Applied, ApplyError, Effect, InEffect, Trial, Tried, in_effect};
use sway_things_core::command::input::InputSelector;
use sway_things_core::command::output::OutputSelector;
use sway_things_core::conflict::Known;
use sway_things_core::generate::{Outcome, Validate};
use sway_things_core::i18n::{tr, trf};
use sway_things_core::include::WriteAccess;
use sway_things_core::msg;
use sway_things_core::paths::Layer;
use sway_things_core::registry::{Apply, ConfigFile, DeviceError, Setting, Value};
use sway_things_core::state::StateError;
use sway_things_core::view::SettingView;

use super::{Exit, Io, Prompt, Session, output};

pub(super) fn get<V: Validate>(
    session: &Session<V>,
    key: Option<&str>,
    device: Option<&str>,
    known: Known<'_>,
    json: bool,
    io: &mut Io<'_>,
) -> Exit {
    let state = session.applier.state();
    let desktop = session.applier.gsettings();
    // What the app shows: while kanshi sets up the displays, their shared value and no line.
    let effect = InEffect {
        pending: session.applier.pending(),
        kanshi: known
            .outputs
            .filter(|_| session.applier.displays().is_kanshi()),
    };
    let shown = |setting, device| {
        let view = SettingView::with_desktop(
            setting,
            device,
            state,
            &session.status,
            session.applier.compositor(),
            known,
            desktop.as_ref(),
        );
        let view = in_effect(view, setting, device, state, &effect);
        output::Shown::of(setting, device, view, &session.status, known)
    };
    let paths = state.paths();
    if let Some(key) = key {
        let setting = match lookup(session, key, device) {
            Ok(setting) => setting,
            Err(error) => return fail(io, &error.into()),
        };
        let shown = shown(setting, device);
        if json {
            io.out(shown.json(paths));
        } else {
            io.out(shown.text(paths));
        }
        return Exit::Ok;
    }
    if let Some(device) = device {
        if InputSelector::device(device).is_err() && OutputSelector::output(device).is_err() {
            let error = StateError::InvalidDevice {
                device: device.to_owned(),
            };
            return fail(io, &error.into());
        }
    }
    let all: Vec<output::Shown> = state
        .entries()
        .into_iter()
        .filter(|entry| device.is_none_or(|device| entry.device == Some(device)))
        .map(|entry| shown(entry.setting, entry.device))
        .collect();
    if json {
        io.out(serde_json::Value::Array(
            all.iter().map(|shown| shown.json(paths)).collect(),
        ));
    } else {
        output::list(&all, paths, io.out);
    }
    Exit::Ok
}

/// The setting `key`, checked against `device` like a change would be.
pub(super) fn lookup<V: Validate>(
    session: &Session<V>,
    key: &str,
    device: Option<&str>,
) -> Result<&'static Setting, StateError> {
    let setting =
        session
            .applier
            .state()
            .registry()
            .get(key)
            .ok_or_else(|| StateError::UnknownKey {
                key: key.to_owned(),
            })?;
    if let Some(device) = device {
        match setting.check_device(device) {
            Ok(()) => {}
            Err(DeviceError::NotPerDevice) => {
                return Err(StateError::NotPerDevice { key: setting.key() });
            }
            Err(DeviceError::Invalid) => {
                return Err(StateError::InvalidDevice {
                    device: device.to_owned(),
                });
            }
        }
    }
    Ok(setting)
}

/// A change asked for with `set`.
pub(super) struct Change<'a> {
    pub(super) key: &'a str,
    pub(super) device: Option<&'a str>,
    pub(super) text: &'a str,
    pub(super) layer: Option<Layer>,
    pub(super) yes: bool,
}

pub(super) async fn set<V: Validate>(
    session: &mut Session<V>,
    change: &Change<'_>,
    overwrite: bool,
    prompt: &mut impl Prompt,
    io: &mut Io<'_>,
) -> Exit {
    let Some(access) = write_access(session, io) else {
        return Exit::ReadOnly;
    };
    let (key, device) = (change.key, change.device);
    let setting = match lookup(session, key, device) {
        Ok(setting) => setting,
        Err(error) => return fail(io, &error.into()),
    };
    let value = match setting.kind().parse(change.text) {
        Ok(value) => value,
        Err(error) => {
            io.error(format_args!(
                "{}: {}",
                setting.key(),
                error.text().translated()
            ));
            return Exit::Usage;
        }
    };
    // The file takes a binding of a command sway doesn't know, so one typo doesn't drop them all;
    // what is set here has to work.
    if let Value::List(items) = &value {
        if setting.key() == sway_things_core::bindings::ours::BINDINGS {
            for item in items {
                if let Err(error) = sway_things_core::bindings::ours::parse_strict(item) {
                    io.error(format_args!(
                        "{}: {item}: {}",
                        setting.key(),
                        error.text().translated()
                    ));
                    return Exit::Usage;
                }
            }
        }
    }
    let applied = if setting.apply() != Apply::Risky {
        session
            .applier
            .set(key, device, value.clone(), &access)
            .await
    } else if change.yes {
        session
            .applier
            .set_risky_unchecked(key, device, value.clone(), &access)
            .await
    } else if prompt.is_interactive() {
        match try_risky(session, setting, device, &value, &access, prompt, io).await {
            Ok(applied) => applied,
            Err(exit) => return exit,
        }
    } else {
        return refuse_risky(setting, io);
    };
    let applied = match applied {
        Ok(applied) => applied,
        Err(error) => return fail(io, &error),
    };
    if let Some(layer) = change.layer {
        if let Err(error) = session.applier.move_to(key, device, layer, &access) {
            return fail(io, &error);
        }
    }
    io.out(changed(setting, device, &value, &applied));
    warn_not_copied(&applied, io);
    persist(session, overwrite, &access, io).await
}

/// Warns about the GSettings keys a change couldn't be copied to: the change is made, but GTK
/// apps keep their own value.
pub(super) fn warn_not_copied(applied: &Applied, io: &mut Io<'_>) {
    for error in &applied.not_copied {
        io.warn(error.text().translated());
    }
}

/// Tries a risky change and asks whether to keep it. `Err` with the exit code if it wasn't kept.
pub(super) async fn try_risky<V: Validate>(
    session: &mut Session<V>,
    setting: &'static Setting,
    device: Option<&str>,
    value: &Value,
    access: &WriteAccess,
    prompt: &mut impl Prompt,
    io: &mut Io<'_>,
) -> Result<Result<Applied, ApplyError>, Exit> {
    let tried = session
        .applier
        .try_risky(
            setting.key(),
            device,
            value.clone(),
            &session.status,
            access,
        )
        .await
        .map_err(|error| fail(io, &error))?;
    match tried {
        Tried::Waits(trial) => {
            keep_or_revert(session, setting, device, trial, access, prompt, io).await
        }
        // Nothing to keep: there was no safety net to try it with.
        Tried::Stored(applied) => Ok(Ok(applied)),
        Tried::Unchanged => Ok(Ok(Applied {
            changed: false,
            effect: Effect::NotStored,
            not_copied: Vec::new(),
        })),
    }
}

/// Asks whether to keep the risky change that `trial` sent: keeps it, or reverts it.
pub(super) async fn keep_or_revert<V: Validate>(
    session: &mut Session<V>,
    setting: &'static Setting,
    device: Option<&str>,
    trial: Trial,
    access: &WriteAccess,
    prompt: &mut impl Prompt,
    io: &mut Io<'_>,
) -> Result<Result<Applied, ApplyError>, Exit> {
    if let Some(pending) = session.applier.pending() {
        io.note(trf(
            msg!("{setting} is now {value}. It is reverted unless you keep it."),
            &[
                ("setting", &output::name(setting.key(), device)),
                ("value", &pending.value()),
            ],
        ));
    }
    if prompt.keep(trial.countdown).await {
        return Ok(session.applier.keep(access));
    }
    match session.applier.revert().await {
        Ok(_) => io.note(trf(
            msg!("Reverted {setting}."),
            &[("setting", &setting.key())],
        )),
        Err(error) => {
            io.error(trf(
                msg!("{error}; the old value comes back when sway reloads its config"),
                &[("error", &error.text().translated())],
            ));
            return Err(Exit::Failure);
        }
    }
    Err(Exit::Refused)
}

/// Refuses a risky change without someone to ask or `--yes`.
pub(super) fn refuse_risky(setting: &Setting, io: &mut Io<'_>) -> Exit {
    io.error(trf(
        msg!(
            "{setting} can lock you out, so it is only changed after you keep it in a terminal; \
             pass --yes to change it without asking"
        ),
        &[("setting", &setting.key())],
    ));
    Exit::Refused
}

/// A reset asked for with `reset`.
pub(super) struct Reset<'a> {
    pub(super) key: &'a str,
    pub(super) device: Option<&'a str>,
    pub(super) yes: bool,
    /// The devices and outputs the user's lines are matched with.
    pub(super) known: Known<'a>,
}

pub(super) async fn reset<V: Validate>(
    session: &mut Session<V>,
    reset_of: &Reset<'_>,
    overwrite: bool,
    prompt: &mut impl Prompt,
    io: &mut Io<'_>,
) -> Exit {
    let Some(access) = write_access(session, io) else {
        return Exit::ReadOnly;
    };
    let (key, device) = (reset_of.key, reset_of.device);
    let setting = match lookup(session, key, device) {
        Ok(setting) => setting,
        Err(error) => return fail(io, &error.into()),
    };
    let requirements = setting.requirements();
    // Nothing is sent for a setting the compositor doesn't support, so there is nothing to try.
    let supported = session
        .applier
        .compositor()
        .check(requirements.min_sway, requirements.min_swayfx)
        .is_ok();
    let to_config = setting.apply() != Apply::Risky
        && session
            .applier
            .users_line_for_reset(key, device, &session.status, reset_of.known)
            .is_some();
    let applied = if setting.apply() != Apply::Risky {
        session
            .applier
            .reset_to_config(key, device, &session.status, reset_of.known, &access)
            .await
    } else if reset_of.yes || !supported {
        session
            .applier
            .reset_risky_unchecked(key, device, &session.status, &access)
            .await
    } else if prompt.is_interactive() {
        match session
            .applier
            .try_risky_reset(key, device, &session.status, &access)
            .await
        {
            Ok(Tried::Waits(trial)) => {
                match keep_or_revert(session, setting, device, trial, &access, prompt, io).await {
                    Ok(applied) => applied,
                    Err(exit) => return exit,
                }
            }
            Ok(Tried::Stored(applied)) => Ok(applied),
            Ok(Tried::Unchanged) => Ok(Applied {
                changed: false,
                effect: Effect::NotStored,
                not_copied: Vec::new(),
            }),
            Err(error) => Err(error),
        }
    } else {
        return refuse_risky(setting, io);
    };
    let applied = match applied {
        Ok(applied) => applied,
        Err(error) => return fail(io, &error),
    };
    let name = output::name(key, device);
    if applied.effect == Effect::NotStored {
        io.out(trf(
            msg!("{setting} isn't stored by Sway Things; nothing to reset."),
            &[("setting", &name)],
        ));
        return Exit::Ok;
    }
    let alone = session.applier.state().registry().group(setting).count() == 1;
    let done = if setting.apply() == Apply::Risky {
        msg!("Reset {setting}.")
    } else if to_config && alone {
        msg!("Reset {setting} to your config's value.")
    } else if to_config {
        msg!("Reset {setting} and the other settings of its line to your config's value.")
    } else {
        msg!("Reset {setting} to its default.")
    };
    let done = trf(done, &[("setting", &name)]);
    match effect_note(&applied.effect) {
        Some(note) => io.out(sentences(&done, &note)),
        None => io.out(done),
    }
    warn_not_copied(&applied, io);
    persist(session, overwrite, &access, io).await
}

pub(super) fn show_generated<V: Validate>(
    session: &Session<V>,
    file: Option<&str>,
    io: &mut Io<'_>,
) -> Exit {
    let files = session.applier.generate();
    if let Some(name) = file {
        if let Some(file) = files.iter().find(|file| file.file().file_name() == name) {
            let _ = io.out.write_all(file.text().as_bytes());
        }
        return Exit::Ok;
    }
    let mut first = true;
    for file in files.iter().filter(|file| !file.is_empty()) {
        if !first {
            io.out("");
        }
        first = false;
        let path = session.applier.locations().path(file.file());
        io.out(format_args!("# ==> {} <==", path.display()));
        let _ = io.out.write_all(file.text().as_bytes());
    }
    if first {
        io.note(tr(msg!(
            "No settings are stored, so every generated file is empty."
        )));
    }
    Exit::Ok
}

/// The permission to change settings, or `None` after telling the user how to get it.
pub(super) fn write_access<V>(session: &Session<V>, io: &mut Io<'_>) -> Option<WriteAccess> {
    match session.status.write_access() {
        Ok(access) => Some(access),
        Err(read_only) => {
            io.error(read_only.text().translated())
                .note(read_only.explanation().translated());
            None
        }
    }
}

/// Saves the settings and writes the generated files, and reports what wasn't written.
pub(super) async fn persist<V: Validate>(
    session: &mut Session<V>,
    overwrite: bool,
    access: &WriteAccess,
    io: &mut Io<'_>,
) -> Exit {
    let overwrite: &[ConfigFile] = if overwrite { &ConfigFile::ALL } else { &[] };
    let persisted = match session.applier.persist(overwrite, access).await {
        Ok(persisted) => persisted,
        Err(error) => return fail(io, &error),
    };
    // The files of the notification daemon are best effort: the settings are saved.
    if let Some(problem) = session.applier.take_daemon_problem() {
        io.warn(problem.translated());
    }
    let mut exit = Exit::Ok;
    for (file, outcome) in persisted.problems() {
        match outcome {
            Outcome::Invalid(lines) => {
                io.error(trf(
                    msg!("sway rejected the new {file}, so the old one stays:"),
                    &[("file", &file.file_name())],
                ));
                for line in lines {
                    io.note(format_args!("  {}", output::invalid_line(line)));
                }
                exit = Exit::Failure;
            }
            Outcome::EditedOutside => {
                if let Some(problem) = outcome.problem(file) {
                    io.error(problem.translated());
                }
                io.note(tr(msg!(
                    "The settings are saved. Run the command again with --overwrite to replace \
                     the file."
                )));
                if exit == Exit::Ok {
                    exit = Exit::Refused;
                }
            }
            _ => {
                if let Some(problem) = outcome.problem(file) {
                    io.error(problem.translated());
                }
                exit = Exit::Failure;
            }
        }
    }
    // The files of other programs (default apps, autostart overrides) are only ever Sway
    // Things' own; one of someone else's is left alone, which is worth a warning, not a failure.
    for problem in &persisted.others {
        io.warn(problem.text().translated());
    }
    exit
}

/// What `set` did, in a sentence.
pub(super) fn changed(
    setting: &Setting,
    device: Option<&str>,
    value: &Value,
    applied: &Applied,
) -> String {
    let name = output::name(setting.key(), device);
    let text = if applied.changed {
        msg!("Set {setting} to {value}.")
    } else {
        msg!("{setting} is already {value}.")
    };
    let text = trf(text, &[("setting", &name), ("value", value)]);
    match effect_note(&applied.effect) {
        Some(note) => sentences(&text, &note),
        None => text,
    }
}

/// Two sentences, one after the other.
pub(super) fn sentences(first: &str, second: &str) -> String {
    // TRANSLATORS: Two sentences, one after the other, such as "Set gaps.inner to 4." and "It
    // takes effect at the next login."
    trf(
        msg!("{first} {second}"),
        &[("first", &first), ("second", &second)],
    )
}

/// When a change that isn't live takes effect.
pub(super) fn effect_note(effect: &Effect) -> Option<String> {
    match effect {
        Effect::Live { .. } | Effect::Preference | Effect::NotStored => None,
        Effect::NoSession => Some(
            tr(msg!(
                "sway isn't running; it takes effect when sway starts."
            ))
            .to_owned(),
        ),
        Effect::AfterReload => Some(
            tr(msg!(
                "It takes effect when sway reloads its config (swaymsg reload)."
            ))
            .to_owned(),
        ),
        Effect::AfterToolReload => Some(
            tr(msg!(
                "The tool behind it reads its config again now, or when it starts."
            ))
            .to_owned(),
        ),
        Effect::AfterRelogin => Some(tr(msg!("It takes effect at the next login.")).to_owned()),
        // TRANSLATORS: {requirement} is what the setting needs, such as "needs sway 1.11".
        Effect::Unsupported(unmet) => Some(trf(
            msg!("Nothing was sent: it {requirement}."),
            &[("requirement", unmet)],
        )),
    }
}

/// Reports `error` and returns the exit code it stands for.
pub(super) fn fail(io: &mut Io<'_>, error: &ApplyError) -> Exit {
    io.error(error.text().translated());
    match error {
        ApplyError::State(
            StateError::UnknownKey { .. }
            | StateError::InvalidValue { .. }
            | StateError::NotPerDevice { .. }
            | StateError::InvalidDevice { .. }
            | StateError::OneAtATime { .. }
            | StateError::NotPerCollection { .. }
            | StateError::NoCollection { .. }
            | StateError::InvalidCollection { .. },
        )
        | ApplyError::Render(_) => Exit::Usage,
        ApplyError::Unsupported { .. }
        | ApplyError::NoSession { .. }
        | ApplyError::Lockout { .. } => Exit::Refused,
        _ => Exit::Failure,
    }
}
