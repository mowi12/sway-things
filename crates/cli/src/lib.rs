// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line of Sway Things: `get`, `set`, `reset`, `show-generated`, `doctor` and
//! `open`.
//!
//! [`run`] carries out a [`Command`] on a [`Session`], so the tests run it with a stand-in for
//! sway and a validator of their own; [`run_for_user`] opens the session of the user who runs
//! `sway-things`. Everything goes through the core, like in the app.

mod capture;
mod change;
mod displays;
mod doctor;
mod hook;
mod output;
mod prompt;
mod spinner;
mod wallpaper;

use std::fmt;
use std::io::{self, Write};
use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{ExitCode, Stdio};

use anstream::{AutoStream, ColorChoice};

use change::{Change, Reset, fail, get, reset, set, show_generated};
use clap::builder::PossibleValuesParser;
use clap::{Parser, Subcommand};
use displays::{Displays, follow_combination, note_kanshi};
use hook::{hook_clipboard, hook_for_user, kanshi_files, read_history, run_hook};
use sway_things_core::apply::Applier;
use sway_things_core::conflict::Known;
use sway_things_core::doctor::{Checkup, Found, Installed};
use sway_things_core::generate::{self, Locations, SwayValidator, Validate};
use sway_things_core::gsettings::Gio;
use sway_things_core::i18n::{tr, trf};
use sway_things_core::include::IncludeStatus;
use sway_things_core::ipc::Client;
use sway_things_core::ipc::Compositor;
use sway_things_core::msg;
use sway_things_core::network::NetworkManager;
use sway_things_core::outputs::{self, Outputs};
use sway_things_core::printers::Cups;
use sway_things_core::registry::{ConfigFile, Link, Registry};
use sway_things_core::sound::Sound;
use sway_things_core::state::{Layer, Paths, State};
use sway_things_core::sway_config::Disk;
use sway_things_core::system::{self, Details};
use wallpaper::{follow_collections, wallpaper};

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
        /// The wallpaper collection, by its name
        #[arg(long, value_name = "NAME", conflicts_with = "device")]
        collection: Option<String>,
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
        /// The wallpaper collection, by its name
        #[arg(long, value_name = "NAME", conflicts_with = "device")]
        collection: Option<String>,
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
        /// The wallpaper collection, by its name
        #[arg(long, value_name = "NAME", conflicts_with = "device")]
        collection: Option<String>,
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
    /// the compositor, GTK (from sway-things-gui --version), the distribution, NetworkManager, the
    /// sound server and CUPS (while they run) and the kernel, whether cups-pk-helper is
    /// installed, and whether the tools the pages delegate to (nm-connection-editor,
    /// blueman-manager, pwvucontrol, pavucontrol, system-config-printer) and those of
    /// screenshots, the clipboard history and recording (grim, slurp, wl-copy, cliphist, swappy,
    /// satty, wf-recorder, wl-screenrec) are installed.
    Doctor(DoctorArgs),
    /// Start the app, or bring its window up if it runs, at a page or a setting
    Open {
        /// A page, e.g. `keyboard`, or a setting, e.g. `input.keyboard.repeat_rate`
        link: Option<String>,
    },
    /// Run a hook; the generated sway config runs these
    #[command(subcommand)]
    Hook(Hook),
    /// Take a screenshot with grim, and save and copy it as the Screenshots & Clipboard page says;
    /// the screenshot keys run this
    ///
    /// The folder, the file name pattern, whether the picture is saved, copied or both, and what
    /// follows (a notification with Open and Edit, or the editor) come from the settings. A
    /// selection canceled with Escape takes no picture and is no error.
    Screenshot {
        /// What to capture: a region you select (or a window you click) with slurp, the focused
        /// window, the focused display or all displays
        #[arg(value_enum)]
        area: capture::ShotArea,
    },
    /// Start a recording of the screen with wf-recorder or wl-screenrec, or stop the one that runs
    /// so its file is complete; the recording key runs this
    Record {
        /// What to record; without it, what the Screenshots & Clipboard page says
        #[arg(value_enum)]
        area: Option<capture::RecordArea>,
    },
    /// Pick an entry of the clipboard history, or clear it
    #[command(subcommand)]
    Clipboard(capture::Clipboard),
    /// Change the picture of a wallpaper collection
    ///
    /// A display shows a collection when wallpaper.collection names one. The generated sway
    /// config runs `hook wallpaper` at login; a systemd user timer runs `wallpaper next --due`
    /// for collections that change daily or after an interval.
    #[command(subcommand)]
    Wallpaper(WallpaperCommand),
}

/// A command of `sway-things wallpaper`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum WallpaperCommand {
    /// Show the next picture of the collection
    Next {
        /// The display, as for `get`; without one, the collection of every display
        #[arg(long, value_name = "IDENTIFIER")]
        device: Option<String>,
        /// Only if its time has come; without a sway session, stop the timer instead
        #[arg(long)]
        due: bool,
    },
}

impl WallpaperCommand {
    /// The display the command is for, as given.
    fn device(&self) -> Option<&str> {
        match self {
            Self::Next { device, .. } => device.as_deref(),
        }
    }
}

/// A hook of `sway-things hook`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Hook {
    /// Copy the stored settings that GTK apps read from GSettings (cursor theme and size) to
    /// GSettings; run at login and at each reload of sway
    Gsettings,
    /// Switch a display on if none is on (and have kanshi apply its profile again, if it runs):
    /// run at every start and reload of sway while Sway Things stores anything for a display, so
    /// it never leaves you without one
    Displays,
    /// Put the workspaces on the displays connected now as the Workspaces page says: run at
    /// every start and reload of sway while a rule places them, and by kanshi when it applied a
    /// profile
    Workspaces,
    /// Show displays on others as remembered for the displays connected now (wl-mirror), and end
    /// the mirrors of other combinations: run by kanshi when it applied a profile, and from a key
    /// binding of yours to bring back a picture that went
    Mirror,
    /// Ask once whether a display plugged in to a combination set up on the Displays page shows
    /// the same picture or extends the desktop, and remember the answer: run by kanshi when one
    /// display more than such a combination is connected
    NewDisplay,
    /// Show the next picture of each wallpaper collection whose time has come, and start the
    /// timers of those that change daily or after an interval: run when sway starts while a
    /// display shows a collection
    Wallpaper,
    /// Start the watcher of the clipboard history (wl-paste --watch cliphist store) if the
    /// Screenshots & Clipboard page has the history on, and end it if not: run at every start and
    /// reload of sway while it is on
    Clipboard,
    /// Start an installed app by its desktop file ID, as its desktop entry says (in a terminal
    /// if it asks for one): the autostart entries of Sway Things that are apps run this
    Launch {
        /// The desktop file ID, e.g. `org.mozilla.firefox.desktop`
        id: String,
    },
    /// Hand the session's variables to the systemd user session and start the XDG autostart
    /// entries through it (xdg-desktop-autostart.target): run at login while Sway Things starts
    /// them with systemd
    Autostart,
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
        Command::Hook(
            hook @ (Hook::Displays
            | Hook::Workspaces
            | Hook::Mirror
            | Hook::NewDisplay
            | Hook::Launch { .. }
            | Hook::Autostart),
        ) => {
            return hook_for_user(hook, &mut err).await;
        }
        Command::Hook(Hook::Clipboard) => return hook_clipboard(read_history, &mut err),
        Command::Screenshot { area } => return capture::screenshot(*area, &mut err).await,
        Command::Record { area } => return capture::record(*area, &mut err).await,
        Command::Clipboard(action) => return capture::clipboard(*action, &mut err).await,
        _ => {}
    }
    let (mut applier, main) = match Applier::open().await {
        Ok(opened) => opened,
        Err(error) => {
            return fail_with(&mut err, &error.text());
        }
    };
    let status = IncludeStatus::detect(main.path().as_deref(), &Disk, applier.locations());
    // While kanshi sets up the displays, no output lines of ours contradict it.
    applier.set_displays(generate::Displays::detect(&status, applier.locations()));
    // A change of the bindings is sent to sway as the difference to what the config binds.
    applier.set_user_config(&status);
    let mut session = Session { applier, status };
    let exit = run(
        command,
        &mut session,
        &mut Terminal,
        &mut io::stdout().lock(),
        &mut err,
    )
    .await;
    // The clipboard history's watcher follows its switch at once, in the session it runs in.
    if let (Command::Set { key, .. } | Command::Reset { key, .. }, Exit::Ok) = (command, exit) {
        if sway_things_core::capture::clipboard::KEYS.contains(&key.as_str())
            && session.applier.client().is_some()
        {
            if let Err(error) = sway_things_core::capture::clipboard::sync(session.applier.state())
            {
                let _ = writeln!(err, "{}", error_line(&error.text().translated()));
            }
        }
    }
    exit
}

/// Prints `text` as an error line on `err` and returns [`Exit::Failure`].
fn fail_with(err: &mut dyn Write, text: &sway_things_core::i18n::Text) -> Exit {
    let _ = writeln!(err, "{}", error_line(&text.translated()));
    Exit::Failure
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
            return fail_with(err, &error.text());
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
    let apps_check = sway_things_core::doctor::apps::AppsCheck::for_user().await;
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
        displays: generate::Displays::detect(&status, &locations)
            .with_profiles(outputs::profiles::Profiles::load(state.paths().displays()).is_ok()),
        kanshi: kanshi_files(),
        path: std::env::var_os("PATH"),
        gsettings: &Gio,
        apps: apps_check.as_ref(),
    };
    doctor_with(&checkup, Some(&system), args, &mut stdout.lock(), &look).await
}

/// The system details for `doctor`, with `compositor`. The GTK version is the app's, from
/// `sway-things-gui --version`; NetworkManager is asked on the system bus, CUPS on its socket.
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
    let sound = Sound::connect().ok();
    let cups = Cups::local().ok();
    Details::read(
        compositor,
        gtk,
        network_manager.as_ref(),
        sound.as_ref(),
        cups.as_ref(),
    )
    .await
}

/// Whether `command` warns of the problems of the settings files: the doctor reports them itself,
/// `open` doesn't read them, and neither do the hooks and keys that run in the background.
fn warns_of_problems(command: &Command) -> bool {
    !matches!(
        command,
        Command::Doctor(_)
            | Command::Open { .. }
            | Command::Hook(
                Hook::Displays
                    | Hook::Workspaces
                    | Hook::Mirror
                    | Hook::NewDisplay
                    | Hook::Clipboard
                    | Hook::Launch { .. }
                    | Hook::Autostart
            )
            | Command::Screenshot { .. }
            | Command::Record { .. }
            | Command::Clipboard(_)
    )
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
    if warns_of_problems(command) {
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
        Command::Get {
            key,
            device,
            collection,
            json,
        } => {
            let device = match collection {
                Some(collection) => Some(collection.clone()),
                None => resolved(key.as_deref(), device.as_deref()),
            };
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
            collection,
            host,
            shared,
            yes,
            overwrite,
        } => {
            let device = match displays.stored_for(key, device.as_deref(), collection.as_deref()) {
                Ok(device) => device,
                Err(error) => return fail(&mut io, &error.into()),
            };
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
            follow_combination(session, key, &mut io).await;
            let exit = set(session, &change, *overwrite, prompt, &mut io).await;
            note_kanshi(session, key, exit, false, &mut io).await;
            follow_collections(session, key, exit, &mut io).await
        }
        Command::Reset {
            key,
            device,
            collection,
            yes,
            overwrite,
        } => {
            let device = match displays.stored_for(key, device.as_deref(), collection.as_deref()) {
                Ok(device) => device,
                Err(error) => return fail(&mut io, &error.into()),
            };
            let reset_of = Reset {
                key,
                device: device.as_deref(),
                yes: *yes,
            };
            follow_combination(session, key, &mut io).await;
            let exit = reset(session, &reset_of, *overwrite, prompt, &mut io).await;
            note_kanshi(session, key, exit, true, &mut io).await;
            follow_collections(session, key, exit, &mut io).await
        }
        Command::ShowGenerated { file } => show_generated(session, file.as_deref(), &mut io),
        Command::Doctor(args) => doctor_in(session, args, io.out).await,
        Command::Open { link } => open(link.as_deref(), io.err),
        Command::Hook(hook) => run_hook(hook, session, &mut io).await,
        Command::Wallpaper(command) => wallpaper(session, command, &displays, &mut io).await,
        Command::Screenshot { area } => capture::screenshot(*area, io.err).await,
        Command::Record { area } => capture::record(*area, io.err).await,
        Command::Clipboard(action) => capture::clipboard(*action, io.err).await,
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
    let desktop = applier.gsettings();
    let checkup = Checkup {
        found: &found,
        state: applier.state(),
        locations: applier.locations(),
        status: &session.status,
        validator: applier.validator(),
        outputs: outputs.as_ref(),
        displays: applier.displays(),
        kanshi: applier.kanshi_files().cloned(),
        path: None,
        gsettings: desktop.as_ref(),
        apps: None,
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
    use super::displays::resolve_display;
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
