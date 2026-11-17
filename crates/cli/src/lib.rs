// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line of Sway Things: `get`, `set`, `reset`, `show-generated`, `doctor` and
//! `open`.
//!
//! [`run`] carries out a [`Command`] on a [`Session`], so the tests run it with a stand-in for
//! sway and a validator of their own; [`run_for_user`] opens the session of the user who runs
//! `sway-things`. Everything goes through the core, like in the app.

mod doctor;
mod output;
mod prompt;
mod spinner;

use std::fmt;
use std::io::{self, Write};
use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{ExitCode, Stdio};

use anstream::{AutoStream, ColorChoice};

use clap::builder::PossibleValuesParser;
use clap::{Parser, Subcommand};
use sway_things_core::apply::{Applied, Applier, ApplyError, Effect, Trial, Tried};
use sway_things_core::command::input::InputSelector;
use sway_things_core::command::output::OutputSelector;
use sway_things_core::conflict::Known;
use sway_things_core::doctor::{Checkup, Found, Installed};
use sway_things_core::generate::{self, Locations, Outcome, SwayValidator, Validate};
use sway_things_core::i18n::{tr, trf};
use sway_things_core::include::{IncludeStatus, WriteAccess};
use sway_things_core::ipc::Client;
use sway_things_core::ipc::Compositor;
use sway_things_core::msg;
use sway_things_core::network::NetworkManager;
use sway_things_core::outputs::{self, Outputs};
use sway_things_core::registry::{
    Apply, ConfigFile, DeviceError, Link, Registry, Scope, Setting, Value,
};
use sway_things_core::state::{Layer, Paths, State, StateError};
use sway_things_core::sway_config::Disk;
use sway_things_core::system::{self, Details};

pub use doctor::{DoctorArgs, Look, doctor, doctor_with};
pub use prompt::{Prompt, Terminal};
pub use spinner::Spinner;

/// Read and change the settings of Sway Things.
// The help texts stay English: the man pages and completions are generated from them, and clap's
// own words ("Usage:", "Options:", its errors) can't be translated.
#[derive(Debug, Parser)]
#[command(name = "sway-things", version, long_about = None)]
pub struct Cli {
    /// What to do; without a command, `open`.
    #[command(subcommand)]
    pub command: Option<Command>,
    /// When to use colors: `--color auto|always|never`. Only `doctor` colors its report.
    #[command(flatten)]
    pub color: colorchoice_clap::Color,
}

/// A command of `sway-things`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Show the value in effect and where it comes from; without a key, every stored setting
    Get {
        /// The setting, e.g. windows.gaps.inner
        key: Option<String>,
        /// The input device or display: an input device by its identifier as `swaymsg -t
        /// get_inputs` shows it, a display by its make, model and serial as `swaymsg -t
        /// get_outputs` shows them, or in a sway session by its name
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
        /// The input device or display: an input device by its identifier as `swaymsg -t
        /// get_inputs` shows it, a display by its make, model and serial as `swaymsg -t
        /// get_outputs` shows them, or in a sway session by its name
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
        /// The input device or display: an input device by its identifier as `swaymsg -t
        /// get_inputs` shows it, a display by its make, model and serial as `swaymsg -t
        /// get_outputs` shows them, or in a sway session by its name
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
    /// Check why settings might not take effect: the compositor, your sway config and its include
    /// line, the settings files, the generated files and the lines of your config that win
    ///
    /// The report starts with the system details for bug reports: the versions of Sway Things,
    /// the compositor, GTK (from sway-things-gui --version), the distribution, NetworkManager
    /// (while it runs) and the kernel, and whether nm-connection-editor and blueman-manager are
    /// installed.
    Doctor(DoctorArgs),
    /// Start the app, or bring its window up if it runs, at a page or a setting
    Open {
        /// A page, e.g. `keyboard`, or a setting, e.g. `input.keyboard.repeat_rate`
        link: Option<String>,
    },
    /// Run a hook; the generated sway config runs these
    #[command(subcommand)]
    Hook(Hook),
}

/// A hook of `sway-things hook`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum Hook {
    /// Copy the stored settings that GTK apps read from GSettings (cursor theme and size) to
    /// GSettings; run at login and at each reload of sway
    Gsettings,
    /// Switch a display on if none is on (and have kanshi apply its profile again, if it runs):
    /// run at every start and reload of sway while Sway Things stores anything for a display, so
    /// it never leaves you without one
    Displays,
}

/// The binary of the app.
pub const GUI: &str = "sway-things-gui";

/// The environment variable that names the binary of the app, ahead of the one next to
/// `sway-things` and the one in `PATH`.
pub const GUI_ENV: &str = "SWAY_THINGS_GUI";

/// How `sway-things` ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// It did what was asked.
    Ok = 0,
    /// Something went wrong: sway rejected a command or the generated config, a file can't be
    /// read or written, or the app can't be found or started. For `doctor`: it couldn't do its
    /// job, e.g. `--fix` couldn't write the files.
    Failure = 1,
    /// The arguments are wrong: an unknown key or page, a value the setting doesn't take.
    Usage = 2,
    /// The user's sway config doesn't include the generated files yet, so nothing is changed.
    ReadOnly = 3,
    /// The change was refused or not kept: a risky setting without a terminal or `--yes`, a
    /// change that wasn't kept, a setting the compositor doesn't support, a generated file that
    /// was changed outside and not overwritten.
    Refused = 4,
    /// `doctor` ran and found at least one problem, also after `--fix`. Warnings don't count.
    Problems = 5,
}

impl Exit {
    /// Every exit status, in the order of its code.
    pub const ALL: [Self; 6] = [
        Self::Ok,
        Self::Failure,
        Self::Usage,
        Self::ReadOnly,
        Self::Refused,
        Self::Problems,
    ];

    /// The exit code.
    pub const fn code(self) -> u8 {
        self as u8
    }
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        Self::from(exit.code())
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

/// Whether to color what goes to `stream`: `--color`, `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`
/// and whether it is a terminal decide, as [`AutoStream::choice`] tells.
fn color_for(stream: &impl anstream::stream::RawStream) -> bool {
    AutoStream::choice(stream) != ColorChoice::Never
}

/// Runs `command` for the user who runs `sway-things`: with their settings files, their sway
/// config and, if there is one, their sway session. `open` needs neither.
pub async fn run_for_user(command: &Command) -> Exit {
    let mut err = io::stderr();
    match command {
        Command::Doctor(args) => return doctor_for_user(args, &mut err).await,
        Command::Open { link } => return open(link.as_deref(), &mut err),
        Command::Hook(Hook::Displays) => return hook_displays(&mut err).await,
        _ => {}
    }
    let (mut applier, main) = match Applier::open().await {
        Ok(opened) => opened,
        Err(error) => {
            let _ = writeln!(err, "{}", error_line(&error.text().translated()));
            return Exit::Failure;
        }
    };
    // While kanshi sets up the displays, no output lines of ours contradict it.
    applier.set_displays(generate::Displays::detect());
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

/// What the command line needs to know of the displays to find the device a command names.
struct Displays {
    /// The outputs of the session, if there is one and the command reads or writes settings.
    outputs: Option<Outputs>,
}

impl Displays {
    /// The displays for `command`. A display named by its name can't be told apart from one
    /// stored by that name if sway can't list its displays: a command that writes for one then
    /// fails, with the error on `io`.
    async fn of<V: Validate>(
        command: &Command,
        session: &mut Session<V>,
        io: &mut Io<'_>,
    ) -> Result<Self, Exit> {
        let listed = match (command, session.applier.client()) {
            (Command::Get { .. } | Command::Set { .. } | Command::Reset { .. }, Some(client)) => {
                Some(client.outputs().await.map(|found| Outputs::new(&found)))
            }
            _ => None,
        };
        if let (
            Some(Err(error)),
            Command::Set { key, device, .. } | Command::Reset { key, device, .. },
        ) = (&listed, command)
        {
            if device.is_some() && is_for_displays(Some(key)) {
                io.error(error_line(error));
                return Err(Exit::Failure);
            }
        }
        Ok(Self {
            outputs: listed.and_then(Result::ok),
        })
    }

    /// `device` as settings are stored for it ([`resolve_display`]): a connected display named
    /// by its name is that display; anything else, such as a value stored by a name, stays as
    /// given.
    fn resolve(&self, key: Option<&str>, device: Option<&str>) -> Option<String> {
        resolve_display(key, device, self.outputs.as_ref())
    }
}

/// `device` as settings are stored for it: for a setting of displays (or without a key), a
/// connected display named by its name (`DP-4`) is its identifier (make, model and serial).
/// Anything else stays as given.
fn resolve_display(
    key: Option<&str>,
    device: Option<&str>,
    outputs: Option<&Outputs>,
) -> Option<String> {
    let device = device?;
    let identifier = outputs
        .filter(|_| is_for_displays(key))
        .and_then(|outputs| outputs.find(device))
        .map(|output| output.identifier().to_owned());
    Some(identifier.unwrap_or_else(|| device.to_owned()))
}

/// After a change of a display's setting that went through while kanshi sets up the displays:
/// says that it is live only.
fn note_kanshi<V: Validate>(session: &Session<V>, key: &str, exit: Exit, io: &mut Io<'_>) {
    if exit == Exit::Ok
        && session.applier.displays() == generate::Displays::Kanshi
        && is_for_displays(Some(key))
    {
        io.note(tr(msg!(
            "kanshi sets up the displays: the change takes effect now and isn't stored."
        )));
    }
}

/// Whether `key` is a setting of displays, or no key is given (every setting of a device).
fn is_for_displays(key: Option<&str>) -> bool {
    key.is_none_or(|key| {
        Registry::builtin()
            .get(key)
            .is_some_and(|setting| setting.scope() == Scope::PerOutput)
    })
}

/// `hook displays`: switches a display on if none is on. Quiet when there is nothing to do; it
/// says on stderr which display it switched on, for sway's log.
async fn hook_displays(err: &mut dyn Write) -> Exit {
    let mut client = match Client::connect().await {
        Ok(client) => client,
        Err(error) => {
            let _ = writeln!(err, "{}", error_line(&error));
            return Exit::Failure;
        }
    };
    let ensured = outputs::ensure_one_on(&mut client).await;
    // Then, sway having just read its config: kanshi applies its profile again over whatever
    // the config set for the displays. Best effort; without kanshictl kanshi keeps what it has.
    if outputs::kanshi_runs() {
        let _ = std::process::Command::new("kanshictl")
            .arg("reload")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    match ensured {
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
        Err(error) => {
            let _ = writeln!(err, "{}", error_line(&error));
            Exit::Failure
        }
    }
}

/// `doctor` for the user who runs `sway-things`. It works without a session and without sway,
/// when the others can't: then the installed `sway` validates the generated files, if there is
/// one. On a terminal, the report is colored and a spinner tells what it checks; never for
/// `--json`.
async fn doctor_for_user(args: &DoctorArgs, err: &mut dyn Write) -> Exit {
    let stdout = io::stdout();
    let look = if args.json {
        Look::default()
    } else {
        Look {
            color: color_for(&stdout),
            spinner: Spinner::stderr(color_for(&io::stderr())),
        }
    };
    let (paths, locations) = match Paths::from_env()
        .and_then(|paths| Locations::from_env().map(|locations| (paths, locations)))
    {
        Ok(found) => found,
        Err(error) => {
            look.spinner.clear();
            let _ = writeln!(err, "{}", error_line(&error.text().translated()));
            return Exit::Failure;
        }
    };
    look.spinner.say(tr(msg!("Loading the settings files…")));
    let state = State::load(Registry::builtin(), paths);
    let validator = SwayValidator::new();
    look.spinner.say(tr(msg!("Asking sway for its version…")));
    let (found, main) = Found::detect(&validator).await;
    look.spinner.say(tr(msg!("Reading your sway config…")));
    let status = IncludeStatus::detect(main.path().as_deref(), &Disk, &locations);
    look.spinner.say(tr(msg!("Collecting the system details…")));
    let system = system_details(found.compositor()).await;
    let outputs = match Client::connect().await {
        Ok(mut client) => client
            .outputs()
            .await
            .ok()
            .map(|found| Outputs::new(&found)),
        Err(_) => None,
    };
    let checkup = Checkup {
        found: &found,
        state: &state,
        locations: &locations,
        status: &status,
        validator: &validator,
        outputs: outputs.as_ref(),
        displays: generate::Displays::detect(),
    };
    doctor_with(&checkup, Some(&system), args, &mut stdout.lock(), &look).await
}

/// The system details for `doctor`, with `compositor`. The GTK version is the app's, from
/// `sway-things-gui --version`; NetworkManager is asked on the system bus.
async fn system_details(compositor: Option<Compositor>) -> Details {
    let gtk = find_gui().and_then(|gui| {
        let output = std::process::Command::new(gui)
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        output
            .status
            .success()
            .then(|| system::gtk_from_app_version(&text))?
    });
    let network_manager = NetworkManager::connect().await.ok();
    Details::read(compositor, gtk, network_manager.as_ref()).await
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
    // The doctor reports the problems of the settings files itself; `open` doesn't read them.
    if !matches!(
        command,
        Command::Doctor(_) | Command::Open { .. } | Command::Hook(Hook::Displays)
    ) {
        for problem in session.applier.state().problems() {
            io.warn(problem);
        }
    }
    // The outputs of the session: what they run with, for the values that nothing sets, and
    // the identifier of a display named by its name.
    let displays = match Displays::of(command, session, &mut io).await {
        Ok(displays) => displays,
        Err(exit) => return exit,
    };
    let resolved = |key: Option<&str>, device: Option<&str>| displays.resolve(key, device);
    match command {
        Command::Get { key, device, json } => {
            let device = resolved(key.as_deref(), device.as_deref());
            let known = Known {
                devices: None,
                outputs: displays.outputs.as_ref(),
            };
            get(
                session,
                key.as_deref(),
                device.as_deref(),
                known,
                *json,
                &mut io,
            )
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
            let device = resolved(Some(key), device.as_deref());
            let change = Change {
                key,
                device: device.as_deref(),
                text: value,
                layer,
                yes: *yes,
            };
            let exit = set(session, &change, *overwrite, prompt, &mut io).await;
            note_kanshi(session, key, exit, &mut io);
            exit
        }
        Command::Reset {
            key,
            device,
            yes,
            overwrite,
        } => {
            let device = resolved(Some(key), device.as_deref());
            let reset_of = Reset {
                key,
                device: device.as_deref(),
                yes: *yes,
            };
            let exit = reset(session, &reset_of, *overwrite, prompt, &mut io).await;
            note_kanshi(session, key, exit, &mut io);
            exit
        }
        Command::ShowGenerated { file } => show_generated(session, file.as_deref(), &mut io),
        Command::Doctor(args) => doctor_in(session, args, io.out).await,
        Command::Open { link } => open(link.as_deref(), io.err),
        Command::Hook(Hook::Gsettings) => hook_gsettings(session, &mut io),
        Command::Hook(Hook::Displays) => match session.applier.client() {
            Some(client) => match outputs::ensure_one_on(client).await {
                Ok(_) => Exit::Ok,
                Err(error) => {
                    io.error(error_line(&error));
                    Exit::Failure
                }
            },
            None => Exit::Ok,
        },
    }
}

/// `hook gsettings`: writes every stored setting with a GSettings key to GSettings. Keys Sway
/// Things doesn't store stay as they are.
fn hook_gsettings<V: Validate>(session: &Session<V>, io: &mut Io<'_>) -> Exit {
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

/// `open`: starts the app at `link`, or without one, and returns at once. A running app brings
/// its window up instead. An unknown link is reported with the pages or keys there are, and the
/// app isn't started.
#[allow(
    clippy::zombie_processes,
    reason = "the app outlives sway-things, which ends right after starting it"
)]
pub fn open(link: Option<&str>, err: &mut dyn Write) -> Exit {
    if let Some(text) = link {
        if let Err(error) = Link::parse(text, Registry::builtin()) {
            let _ = writeln!(err, "{}", error_line(&error.text().translated()));
            return Exit::Usage;
        }
    }
    let Some(gui) = find_gui() else {
        let text = trf(
            msg!(
                "can't find {program}: install it next to sway-things or in PATH, or name it in \
                 {variable}"
            ),
            &[("program", &GUI), ("variable", &GUI_ENV)],
        );
        let _ = writeln!(err, "{}", error_line(&text));
        return Exit::Failure;
    };
    let mut command = std::process::Command::new(&gui);
    command
        .args(link)
        .stdin(Stdio::null())
        // A group of its own, so Ctrl+C in the terminal that ran `sway-things open` leaves it.
        .process_group(0);
    match command.spawn() {
        Ok(_) => Exit::Ok,
        Err(error) => {
            let text = trf(
                msg!("can't start {program}: {error}"),
                &[("program", &gui.display()), ("error", &error)],
            );
            let _ = writeln!(err, "{}", error_line(&text));
            Exit::Failure
        }
    }
}

/// The binary of the app: the one `SWAY_THINGS_GUI` names, the one next to `sway-things`, or the
/// first in `PATH`.
fn find_gui() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os(GUI_ENV).filter(|named| !named.is_empty()) {
        return Some(PathBuf::from(named));
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(GUI)));
    if let Some(beside) = beside.filter(|path| path.is_file()) {
        return Some(beside);
    }
    let paths = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&paths)
        .map(|dir| dir.join(GUI))
        .find(|path| path.is_file())
}

/// `doctor` on an open session: the compositor is the one the applier generates for.
async fn doctor_in<V: Validate>(
    session: &mut Session<V>,
    args: &DoctorArgs,
    out: &mut dyn Write,
) -> Exit {
    let compositor = session.applier.compositor();
    let found = if session.applier.client().is_some() {
        Found::Session(compositor)
    } else {
        Found::NoSession {
            // TRANSLATORS: Why `sway-things doctor` found no sway session.
            reason: msg!("not in a sway session").into(),
            installed: Installed::Version(compositor),
        }
    };
    let outputs = match session.applier.client() {
        Some(client) => client
            .outputs()
            .await
            .ok()
            .map(|found| Outputs::new(&found)),
        None => None,
    };
    let applier = &session.applier;
    let checkup = Checkup {
        found: &found,
        state: applier.state(),
        locations: applier.locations(),
        status: &session.status,
        validator: applier.validator(),
        outputs: outputs.as_ref(),
        displays: generate::Displays::detect(),
    };
    doctor(&checkup, None, args, out).await
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
        // TRANSLATORS: A warning on the command line.
        let line = trf(msg!("warning: {message}"), &[("message", &text)]);
        let _ = writeln!(self.err, "{line}");
    }

    fn error(&mut self, text: impl fmt::Display) -> &mut Self {
        let _ = writeln!(self.err, "{}", error_line(&text));
        self
    }
}

/// An error message on the command line: `error: ` and `text`.
fn error_line(text: &dyn fmt::Display) -> String {
    // TRANSLATORS: An error on the command line.
    trf(msg!("error: {message}"), &[("message", text)])
}

fn get<V: Validate>(
    session: &Session<V>,
    key: Option<&str>,
    device: Option<&str>,
    known: Known<'_>,
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
            known,
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
            io.error(format_args!(
                "{}: {}",
                setting.key(),
                error.text().translated()
            ));
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
    warn_not_copied(&applied, io);
    persist(session, overwrite, &access, io).await
}

/// Warns about the GSettings keys a change couldn't be copied to: the change is made, but GTK
/// apps keep their own value.
fn warn_not_copied(applied: &Applied, io: &mut Io<'_>) {
    for error in &applied.not_copied {
        io.warn(error.text().translated());
    }
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
fn refuse_risky(setting: &Setting, io: &mut Io<'_>) -> Exit {
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
    let done = if setting.apply() == Apply::Risky {
        msg!("Reset {setting}.")
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
        io.note(tr(msg!(
            "No settings are stored, so every generated file is empty."
        )));
    }
    Exit::Ok
}

/// The permission to change settings, or `None` after telling the user how to get it.
fn write_access<V>(session: &Session<V>, io: &mut Io<'_>) -> Option<WriteAccess> {
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
    exit
}

/// What `set` did, in a sentence.
fn changed(setting: &Setting, device: Option<&str>, value: &Value, applied: &Applied) -> String {
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
fn sentences(first: &str, second: &str) -> String {
    // TRANSLATORS: Two sentences, one after the other, such as "Set gaps.inner to 4." and "It
    // takes effect at the next login."
    trf(
        msg!("{first} {second}"),
        &[("first", &first), ("second", &second)],
    )
}

/// When a change that isn't live takes effect.
fn effect_note(effect: &Effect) -> Option<String> {
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
                "It takes effect when the tool behind it reloads its config."
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
fn fail(io: &mut Io<'_>, error: &ApplyError) -> Exit {
    io.error(error.text().translated());
    match error {
        ApplyError::State(
            StateError::UnknownKey { .. }
            | StateError::InvalidValue { .. }
            | StateError::NotPerDevice { .. }
            | StateError::InvalidDevice { .. }
            | StateError::OneAtATime { .. },
        )
        | ApplyError::Render(_) => Exit::Usage,
        ApplyError::Unsupported { .. }
        | ApplyError::NoSession { .. }
        | ApplyError::Lockout { .. } => Exit::Refused,
        _ => Exit::Failure,
    }
}

#[cfg(test)]
mod tests {
    use super::Exit;

    #[test]
    fn all_lists_every_exit_code_once_in_order() {
        let codes: Vec<u8> = Exit::ALL.iter().map(|exit| exit.code()).collect();
        assert_eq!(codes, [0, 1, 2, 3, 4, 5]);
    }
}

#[cfg(test)]
mod display_tests {
    use super::resolve_display;
    use sway_things_core::outputs::Outputs;

    #[test]
    fn a_display_named_by_its_name_is_stored_by_its_identifier() {
        let json = r#"[{"name": "DP-4", "active": true, "primary": false, "make": "Dell",
            "model": "U2723QE", "serial": "ABC", "modes": []}]"#;
        let outputs = Outputs::new(&serde_json::from_str::<Vec<_>>(json).unwrap());
        let resolve = |key, device| resolve_display(key, device, Some(&outputs));
        assert_eq!(
            resolve(Some("output.scale"), Some("DP-4")).as_deref(),
            Some("Dell U2723QE ABC")
        );
        assert_eq!(
            resolve(None, Some("DP-4")).as_deref(),
            Some("Dell U2723QE ABC")
        );
        // Not connected, or not a setting of displays: as given.
        assert_eq!(
            resolve(Some("output.scale"), Some("DP-9")).as_deref(),
            Some("DP-9")
        );
        assert_eq!(
            resolve(Some("input.touchpad.tap"), Some("DP-4")).as_deref(),
            Some("DP-4")
        );
        assert_eq!(
            resolve_display(Some("output.scale"), Some("DP-4"), None).as_deref(),
            Some("DP-4")
        );
        assert_eq!(resolve(Some("output.scale"), None), None);
    }
}
