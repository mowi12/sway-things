// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line of Sway Things: `get`, `set`, `reset` and `show-generated`.
//!
//! [`run`] carries out a [`Command`] on a [`Session`], so the tests run it with a stand-in for
//! sway and a validator of their own; [`run_for_user`] opens the session of the user who runs
//! `sway-things`. Everything goes through the core, like in the app.

mod output;
mod prompt;

use std::fmt;
use std::io::{self, Write};
use std::process::ExitCode;

use clap::builder::PossibleValuesParser;
use clap::{Parser, Subcommand};
use sway_things_core::apply::{Applied, Applier, ApplyError, Effect, Trial};
use sway_things_core::command::input::InputSelector;
use sway_things_core::generate::{Outcome, Validate};
use sway_things_core::include::{IncludeStatus, WriteAccess};
use sway_things_core::registry::{Apply, ConfigFile, Scope, Setting, Value};
use sway_things_core::state::{Layer, StateError};
use sway_things_core::sway_config::Disk;

pub use prompt::{Prompt, Terminal};

/// Read and change the settings of Sway Things.
#[derive(Debug, Parser)]
#[command(name = "sway-things", version, long_about = None)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// A command of `sway-things`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Show the value in effect and where it comes from; without a key, every stored setting
    Get {
        /// The setting, e.g. windows.gaps.inner
        key: Option<String>,
        /// The input device, by its identifier as `swaymsg -t get_inputs` shows it
        #[arg(long, value_name = "IDENTIFIER")]
        device: Option<String>,
        /// Print JSON for scripts
        #[arg(long)]
        json: bool,
    },
    /// Change a setting: live, in the settings files and in the generated sway config
    Set {
        /// The setting, e.g. windows.gaps.inner
        key: String,
        /// The new value: true or false, a number, a choice, a color as #rrggbb, text, or a list
        /// as items separated by commas (us,de)
        #[arg(allow_hyphen_values = true)]
        value: String,
        /// The input device, by its identifier as `swaymsg -t get_inputs` shows it
        #[arg(long, value_name = "IDENTIFIER")]
        device: Option<String>,
        /// Store the value for this machine only, in settings.d/<hostname>.toml
        #[arg(long, conflicts_with = "shared")]
        host: bool,
        /// Store the value in the shared settings.toml
        #[arg(long)]
        shared: bool,
        /// Change a setting that can lock you out without asking to keep it
        #[arg(long)]
        yes: bool,
        /// Replace generated files that were changed outside Sway Things
        #[arg(long)]
        overwrite: bool,
    },
    /// Remove the stored value of a setting and apply its default
    Reset {
        /// The setting, e.g. windows.gaps.inner
        key: String,
        /// The input device, by its identifier as `swaymsg -t get_inputs` shows it
        #[arg(long, value_name = "IDENTIFIER")]
        device: Option<String>,
        /// Reset a setting that can lock you out without asking to keep it
        #[arg(long)]
        yes: bool,
        /// Replace generated files that were changed outside Sway Things
        #[arg(long)]
        overwrite: bool,
    },
    /// Print the generated sway config files as they would be written, without writing them
    ShowGenerated {
        /// Only this file
        #[arg(value_parser = PossibleValuesParser::new(ConfigFile::ALL.map(ConfigFile::file_name)))]
        file: Option<String>,
    },
}

/// How `sway-things` ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// It did what was asked.
    Ok = 0,
    /// Something went wrong: sway rejected a command or the generated config, or a file can't be
    /// read or written.
    Failure = 1,
    /// The arguments are wrong: an unknown key, a value the setting doesn't take.
    Usage = 2,
    /// The user's sway config doesn't include the generated files yet, so nothing is changed.
    ReadOnly = 3,
    /// The change was refused or not kept: a risky setting without a terminal or `--yes`, a
    /// change that wasn't kept, a setting the compositor doesn't support, a generated file that
    /// was changed outside and not overwritten.
    Refused = 4,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        Self::from(exit as u8)
    }
}

/// What a command works on: the applier with the settings and the session, and the include status
/// of the user's sway config.
#[derive(Debug)]
pub struct Session<V> {
    /// The settings and the connection to sway, if it runs.
    pub applier: Applier<V>,
    /// Whether the user's config includes the generated files.
    pub status: IncludeStatus,
}

/// Runs `command` for the user who runs `sway-things`: with their settings files, their sway
/// config and, if there is one, their sway session.
pub async fn run_for_user(command: &Command) -> Exit {
    let mut err = io::stderr();
    let (applier, main) = match Applier::open().await {
        Ok(opened) => opened,
        Err(error) => {
            let _ = writeln!(err, "error: {error}");
            return Exit::Failure;
        }
    };
    let status = IncludeStatus::detect(main.path().as_deref(), &Disk, applier.locations());
    let mut session = Session { applier, status };
    run(
        command,
        &mut session,
        &mut Terminal,
        &mut io::stdout().lock(),
        &mut err,
    )
    .await
}

/// Runs `command` on `session`. Results go to `out`, errors and warnings to `err`; `prompt` asks
/// whether to keep a risky change.
pub async fn run<V: Validate>(
    command: &Command,
    session: &mut Session<V>,
    prompt: &mut impl Prompt,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    let mut io = Io { out, err };
    for problem in session.applier.state().problems() {
        io.warn(problem);
    }
    match command {
        Command::Get { key, device, json } => {
            get(session, key.as_deref(), device.as_deref(), *json, &mut io)
        }
        Command::Set {
            key,
            value,
            device,
            host,
            shared,
            yes,
            overwrite,
        } => {
            let layer = match (host, shared) {
                (true, _) => Some(Layer::Host),
                (_, true) => Some(Layer::Shared),
                _ => None,
            };
            let change = Change {
                key,
                device: device.as_deref(),
                text: value,
                layer,
                yes: *yes,
            };
            set(session, &change, *overwrite, prompt, &mut io).await
        }
        Command::Reset {
            key,
            device,
            yes,
            overwrite,
        } => {
            let reset_of = Reset {
                key,
                device: device.as_deref(),
                yes: *yes,
            };
            reset(session, &reset_of, *overwrite, prompt, &mut io).await
        }
        Command::ShowGenerated { file } => show_generated(session, file.as_deref(), &mut io),
    }
}

/// Where the output goes. A closed pipe isn't worth an error of its own, so write errors are
/// dropped.
struct Io<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Io<'_> {
    fn out(&mut self, text: impl fmt::Display) {
        let _ = writeln!(self.out, "{text}");
    }

    fn note(&mut self, text: impl fmt::Display) {
        let _ = writeln!(self.err, "{text}");
    }

    fn warn(&mut self, text: impl fmt::Display) {
        let _ = writeln!(self.err, "warning: {text}");
    }

    fn error(&mut self, text: impl fmt::Display) -> &mut Self {
        let _ = writeln!(self.err, "error: {text}");
        self
    }
}

fn get<V: Validate>(
    session: &Session<V>,
    key: Option<&str>,
    device: Option<&str>,
    json: bool,
    io: &mut Io<'_>,
) -> Exit {
    let state = session.applier.state();
    let shown = |setting, device| {
        output::Shown::of(
            setting,
            device,
            state,
            &session.status,
            session.applier.compositor(),
        )
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
        if InputSelector::device(device).is_err() {
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
fn lookup<V: Validate>(
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
        if !matches!(setting.scope(), Scope::PerDevice(_)) {
            return Err(StateError::NotPerDevice { key: setting.key() });
        }
        if InputSelector::device(device).is_err() {
            return Err(StateError::InvalidDevice {
                device: device.to_owned(),
            });
        }
    }
    Ok(setting)
}

/// A change asked for with `set`.
struct Change<'a> {
    key: &'a str,
    device: Option<&'a str>,
    text: &'a str,
    layer: Option<Layer>,
    yes: bool,
}

async fn set<V: Validate>(
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
            io.error(format_args!("{}: {error}", setting.key()));
            return Exit::Usage;
        }
    };
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
    persist(session, overwrite, &access, io).await
}

/// Tries a risky change and asks whether to keep it. `Err` with the exit code if it wasn't kept.
async fn try_risky<V: Validate>(
    session: &mut Session<V>,
    setting: &'static Setting,
    device: Option<&str>,
    value: &Value,
    access: &WriteAccess,
    prompt: &mut impl Prompt,
    io: &mut Io<'_>,
) -> Result<Result<Applied, ApplyError>, Exit> {
    let trial = session
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
    keep_or_revert(session, setting, device, trial, access, prompt, io).await
}

/// Asks whether to keep the risky change that `trial` sent: keeps it, or reverts it.
async fn keep_or_revert<V: Validate>(
    session: &mut Session<V>,
    setting: &'static Setting,
    device: Option<&str>,
    trial: Trial,
    access: &WriteAccess,
    prompt: &mut impl Prompt,
    io: &mut Io<'_>,
) -> Result<Result<Applied, ApplyError>, Exit> {
    if let Some(pending) = session.applier.pending() {
        io.note(format_args!(
            "{} is now {}. It is reverted unless you keep it.",
            output::name(setting.key(), device),
            pending.value()
        ));
    }
    if prompt.keep(trial.countdown).await {
        return Ok(session.applier.keep(access));
    }
    match session.applier.revert().await {
        Ok(_) => io.note(format_args!("Reverted {}.", setting.key())),
        Err(error) => {
            io.error(format_args!(
                "{error}; the old value comes back when sway reloads its config"
            ));
            return Err(Exit::Failure);
        }
    }
    Err(Exit::Refused)
}

/// Refuses a risky change without someone to ask or `--yes`.
fn refuse_risky(setting: &Setting, io: &mut Io<'_>) -> Exit {
    io.error(format_args!(
        "{} can lock you out, so it is only changed after you keep it in a terminal; pass \
         --yes to change it without asking",
        setting.key()
    ));
    Exit::Refused
}

/// A reset asked for with `reset`.
struct Reset<'a> {
    key: &'a str,
    device: Option<&'a str>,
    yes: bool,
}

async fn reset<V: Validate>(
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
    let applied = if setting.apply() != Apply::Risky {
        session.applier.reset(key, device, &access).await
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
            Ok(Some(trial)) => {
                match keep_or_revert(session, setting, device, trial, &access, prompt, io).await {
                    Ok(applied) => applied,
                    Err(exit) => return exit,
                }
            }
            Ok(None) => Ok(Applied {
                changed: false,
                effect: Effect::NotStored,
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
        io.out(format_args!(
            "{name} isn't stored by Sway Things; nothing to reset."
        ));
        return Exit::Ok;
    }
    let done = if setting.apply() == Apply::Risky {
        format!("Reset {name}.")
    } else {
        format!("Reset {name} to its default.")
    };
    match effect_note(&applied.effect) {
        Some(note) => io.out(format_args!("{done} {note}")),
        None => io.out(done),
    }
    persist(session, overwrite, &access, io).await
}

fn show_generated<V: Validate>(session: &Session<V>, file: Option<&str>, io: &mut Io<'_>) -> Exit {
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
        io.note("No settings are stored, so every generated file is empty.");
    }
    Exit::Ok
}

/// The permission to change settings, or `None` after telling the user how to get it.
fn write_access<V>(session: &Session<V>, io: &mut Io<'_>) -> Option<WriteAccess> {
    match session.status.write_access() {
        Ok(access) => Some(access),
        Err(read_only) => {
            io.error(&read_only).note(read_only.explanation());
            None
        }
    }
}

/// Saves the settings and writes the generated files, and reports what wasn't written.
async fn persist<V: Validate>(
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
    let mut exit = Exit::Ok;
    for (file, outcome) in persisted.problems() {
        match outcome {
            Outcome::Invalid(lines) => {
                io.error(format_args!(
                    "sway rejected the new {}, so the old one stays:",
                    file.file_name()
                ));
                for line in lines {
                    io.note(format_args!("  {}", output::invalid_line(line)));
                }
                exit = Exit::Failure;
            }
            Outcome::EditedOutside => {
                if let Some(problem) = outcome.problem(file) {
                    io.error(problem);
                }
                io.note("The settings are saved. Run the command again with --overwrite to replace the file.");
                if exit == Exit::Ok {
                    exit = Exit::Refused;
                }
            }
            _ => {
                if let Some(problem) = outcome.problem(file) {
                    io.error(problem);
                }
                exit = Exit::Failure;
            }
        }
    }
    exit
}

/// What `set` did, in a sentence.
fn changed(setting: &Setting, device: Option<&str>, value: &Value, applied: &Applied) -> String {
    let name = output::name(setting.key(), device);
    let mut text = if applied.changed {
        format!("Set {name} to {value}.")
    } else {
        format!("{name} is already {value}.")
    };
    if let Some(note) = effect_note(&applied.effect) {
        text.push(' ');
        text.push_str(&note);
    }
    text
}

/// When a change that isn't live takes effect.
fn effect_note(effect: &Effect) -> Option<String> {
    match effect {
        Effect::Live { .. } | Effect::Preference | Effect::NotStored => None,
        Effect::NoSession => Some("sway isn't running; it takes effect when sway starts.".into()),
        Effect::AfterReload => {
            Some("It takes effect when sway reloads its config (swaymsg reload).".into())
        }
        Effect::AfterToolReload => {
            Some("It takes effect when the tool behind it reloads its config.".into())
        }
        Effect::AfterRelogin => Some("It takes effect at the next login.".into()),
        Effect::Unsupported(unmet) => Some(format!("Nothing was sent: it {unmet}.")),
    }
}

/// Reports `error` and returns the exit code it stands for.
fn fail(io: &mut Io<'_>, error: &ApplyError) -> Exit {
    io.error(error);
    match error {
        ApplyError::State(
            StateError::UnknownKey { .. }
            | StateError::InvalidValue { .. }
            | StateError::NotPerDevice { .. }
            | StateError::InvalidDevice { .. },
        )
        | ApplyError::Render(_) => Exit::Usage,
        ApplyError::Unsupported { .. }
        | ApplyError::NoSession { .. }
        | ApplyError::Lockout { .. } => Exit::Refused,
        _ => Exit::Failure,
    }
}
