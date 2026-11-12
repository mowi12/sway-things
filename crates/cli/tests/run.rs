// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The commands run in-process against settings files in a temporary directory, a stand-in for
//! sway and a validator that accepts everything or reports the errors it is given.

#![allow(
    clippy::unwrap_used,
    reason = "helpers of the tests: a step that fails fails the test"
)]

use std::fs;
use std::future::{Future, ready};
use std::io;
use std::path::{Path, PathBuf};

use async_io::block_on;
use clap::Parser;
use sway_things_cli::{Cli, Command, DoctorArgs, Exit, Prompt, Session, doctor, run};
use sway_things_core::apply::{Applier, Countdown};
use sway_things_core::doctor::{Checkup, Found, Installed};
use sway_things_core::generate::{ConfigError, Locations, Validate};
use sway_things_core::include::IncludeStatus;
use sway_things_core::ipc::Compositor;
use sway_things_core::ipc::stand_in::{FakeSway, SWAY_1_10, obedient};
use sway_things_core::registry::Registry;
use sway_things_core::state::{Paths, State};
use sway_things_core::sway_config::Disk;
use sway_things_core::version::SwayVersion;

/// Accepts every file, or reports `errors` for each.
#[derive(Default)]
struct Validator {
    errors: Vec<ConfigError>,
}

impl Validate for Validator {
    fn validate(&self, _staged: &Path) -> impl Future<Output = io::Result<Vec<ConfigError>>> {
        ready(Ok(self.errors.clone()))
    }
}

/// Answers the question to keep a risky change with `keep`, if `interactive`.
struct Answer {
    interactive: bool,
    keep: bool,
    asked: usize,
}

impl Answer {
    fn new(interactive: bool, keep: bool) -> Self {
        Self {
            interactive,
            keep,
            asked: 0,
        }
    }
}

impl Prompt for Answer {
    fn is_interactive(&self) -> bool {
        self.interactive
    }

    fn keep(&mut self, _countdown: Countdown) -> impl Future<Output = bool> {
        self.asked += 1;
        ready(self.keep)
    }
}

const INCLUDE: &str = "include {dir}/config/sway/sway-things/*.conf\n";

struct Setup {
    dir: tempfile::TempDir,
    session: Session<Validator>,
}

/// What a command printed and how it ended.
#[derive(Debug)]
struct Ran {
    exit: Exit,
    out: String,
    err: String,
}

impl Setup {
    /// Settings files with `shared` in settings.toml, and a sway config with `config` (where
    /// `{dir}` stands for the temporary directory), in the session of `sway` or without one.
    fn new(shared: &str, config: &str, sway: Option<&FakeSway>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("config/sway-things/settings.d")).unwrap();
        fs::write(root.join("config/sway-things/settings.toml"), shared).unwrap();
        let main = root.join("config/sway/config");
        fs::create_dir_all(main.parent().unwrap()).unwrap();
        fs::write(&main, config.replace("{dir}", &root.display().to_string())).unwrap();

        let paths = Paths::new(
            &root.join("config/sway-things"),
            &root.join("state/sway-things"),
            "laptop",
        )
        .unwrap();
        let locations = Locations::new(
            &root.join("config/sway/sway-things"),
            &root.join("state/sway-things"),
        );
        let state = State::load(Registry::builtin(), paths);
        let status = IncludeStatus::detect(Some(&main), &Disk, &locations);
        let applier = match sway {
            Some(sway) => {
                Applier::with_session(state, locations, Validator::default(), sway.connect())
            }
            None => Applier::without_session(
                state,
                locations,
                Validator::default(),
                Compositor::Sway {
                    version: SwayVersion::new(1, 10, 1),
                },
            ),
        };
        Self {
            dir,
            session: Session { applier, status },
        }
    }

    fn run_with(&mut self, args: &[&str], prompt: &mut Answer) -> Ran {
        let cli = Cli::try_parse_from(std::iter::once("sway-things").chain(args.iter().copied()))
            .unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let exit = block_on(run(
            &cli.command,
            &mut self.session,
            prompt,
            &mut out,
            &mut err,
        ));
        Ran {
            exit,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    fn run(&mut self, args: &[&str]) -> Ran {
        self.run_with(args, &mut Answer::new(false, false))
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative)).unwrap_or_default()
    }

    fn settings(&self) -> String {
        self.read("config/sway-things/settings.toml")
    }

    fn generated(&self, file: &str) -> String {
        self.read(&format!("config/sway/sway-things/{file}"))
    }
}

const EMPTY: &str = "version = 1\n";
const LAYOUT: &str = "input.keyboard.xkb_layout";
const TOUCHPAD: &str = "1267:12377:ELAN1300:00_04F3:3059_Touchpad";

#[test]
fn a_change_is_sent_saved_generated_and_shown() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(EMPTY, INCLUDE, Some(&sway));
    let ran = setup.run(&["get", "windows.gaps.inner"]);
    assert_eq!(ran.out, "windows.gaps.inner = 0  (default)\n");
    assert_eq!(ran.exit, Exit::Ok);

    let ran = setup.run(&["set", "windows.gaps.inner", "8"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(ran.out, "Set windows.gaps.inner to 8.\n");
    assert_eq!(ran.err, "");
    assert_eq!(sway.commands(), ["gaps inner 8", "gaps inner all set 8"]);
    assert!(
        setup.settings().contains("inner = 8"),
        "{}",
        setup.settings()
    );
    assert!(
        setup
            .generated("30-appearance.conf")
            .contains("\ngaps inner 8\n")
    );

    let ran = setup.run(&["get", "windows.gaps.inner"]);
    assert_eq!(ran.out, "windows.gaps.inner = 8  (settings.toml)\n");
    let ran = setup.run(&["get", "windows.gaps.inner", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json["value"], 8);
    assert_eq!(json["source"]["kind"], "shared");
    assert_eq!(json["line"], "gaps inner 8");

    // The same value again changes nothing.
    let ran = setup.run(&["set", "windows.gaps.inner", "8"]);
    assert_eq!(ran.out, "windows.gaps.inner is already 8.\n");
}

#[test]
fn get_without_a_key_lists_every_stored_value_by_device() {
    let shared = format!(
        "version = 1\n\n[windows.gaps]\ninner = 8\n\n[input.touchpad]\ntap = true\n\n\
         [device.\"{TOUCHPAD}\".input.touchpad]\npointer_accel = -0.2\n"
    );
    let mut setup = Setup::new(&shared, INCLUDE, None);
    let ran = setup.run(&["get"]);
    assert_eq!(
        ran.out,
        format!(
            "input.touchpad.tap = true  (settings.toml)\n\
             windows.gaps.inner = 8  (settings.toml)\n\
             \n\
             device {TOUCHPAD}\n  \
             input.touchpad.pointer_accel = -0.2  (settings.toml)\n"
        )
    );
    let ran = setup.run(&["get", "--device", TOUCHPAD, "--json"]);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(json[0]["device"], TOUCHPAD);
    assert_eq!(json[0]["value"], -0.2);
}

#[test]
fn get_shows_the_users_config_and_where_it_wins() {
    let mut setup = Setup::new(
        "version = 1\n\n[windows.gaps]\ninner = 8\n",
        &format!("gaps outer 2\n{INCLUDE}gaps inner 4\ninput type:touchpad tap enabled\n"),
        None,
    );
    let config = setup.path("config/sway/config");
    let ran = setup.run(&["get", "windows.gaps.inner"]);
    assert_eq!(
        ran.out,
        format!(
            "windows.gaps.inner = 8  (settings.toml; your config wins: {}:3)\n",
            config.display()
        )
    );
    let ran = setup.run(&["get", "input.touchpad.tap", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json["value"], true);
    assert_eq!(json["source"]["kind"], "config");
    assert_eq!(json["source"]["line"], 4);
}

#[test]
fn nothing_changes_while_the_include_line_is_missing() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(EMPTY, "gaps inner 4\n", Some(&sway));
    let ran = setup.run(&["set", "windows.gaps.inner", "8"]);
    assert_eq!(ran.exit, Exit::ReadOnly);
    assert!(
        ran.err
            .starts_with("error: Add the include line to change settings\n"),
        "{}",
        ran.err
    );
    assert!(ran.err.contains("include "), "{}", ran.err);
    assert_eq!(
        setup.run(&["reset", "windows.gaps.inner"]).exit,
        Exit::ReadOnly
    );
    assert_eq!(sway.commands(), [] as [&str; 0]);
    assert_eq!(setup.settings(), EMPTY);

    // Reading works.
    let ran = setup.run(&["get", "windows.gaps.inner"]);
    assert_eq!(ran.exit, Exit::Ok);
    assert!(
        ran.out
            .starts_with("windows.gaps.inner = 4  (your config: ")
    );
    assert_eq!(setup.run(&["show-generated"]).exit, Exit::Ok);
}

#[test]
fn wrong_arguments_are_usage_errors() {
    let mut setup = Setup::new(EMPTY, INCLUDE, None);
    let ran = setup.run(&["set", "windows.gaps.wide", "8"]);
    assert_eq!(ran.exit, Exit::Usage);
    assert_eq!(ran.err, "error: unknown setting windows.gaps.wide\n");
    let ran = setup.run(&["set", "windows.gaps.inner", "eight"]);
    assert_eq!(ran.exit, Exit::Usage);
    assert_eq!(
        ran.err,
        "error: windows.gaps.inner: must be a whole number\n"
    );
    let ran = setup.run(&["get", "windows.gaps.inner", "--device", TOUCHPAD]);
    assert_eq!(ran.exit, Exit::Usage, "{ran:?}");
    assert_eq!(setup.run(&["get", "nope"]).exit, Exit::Usage);
    assert_eq!(setup.settings(), EMPTY);

    // A value may start with a minus.
    let ran = setup.run(&["set", "input.touchpad.pointer_accel", "-0.5"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert!(Cli::try_parse_from(["sway-things", "set", "--host", "--shared", "a", "b"]).is_err());
    assert!(Cli::try_parse_from(["sway-things", "show-generated", "nope.conf"]).is_err());
}

#[test]
fn a_risky_change_needs_a_terminal_or_yes() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(EMPTY, INCLUDE, Some(&sway));
    let ran = setup.run(&["set", LAYOUT, "us,de"]);
    assert_eq!(ran.exit, Exit::Refused);
    assert!(ran.err.contains("pass --yes"), "{}", ran.err);
    assert_eq!(sway.commands(), [] as [&str; 0]);

    let ran = setup.run(&["set", LAYOUT, "us,de", "--yes"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(ran.out, format!("Set {LAYOUT} to us,de.\n"));
    assert_eq!(
        sway.commands(),
        [r#"input type:keyboard xkb_layout "us,de""#]
    );
    assert!(setup.settings().contains(r#"xkb_layout = ["us", "de"]"#));

    // A reset too.
    let ran = setup.run(&["reset", LAYOUT]);
    assert_eq!(ran.exit, Exit::Refused);
    assert!(ran.err.contains("pass --yes"), "{}", ran.err);
    assert!(setup.settings().contains("xkb_layout"));
    let ran = setup.run(&["reset", LAYOUT, "--yes"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(ran.out, format!("Reset {LAYOUT}.\n"));
    assert_eq!(
        sway.commands()[1..],
        [r#"input type:keyboard xkb_layout "us""#]
    );
    assert_eq!(setup.settings(), EMPTY);
}

#[test]
fn a_risky_reset_removes_the_value_only_when_kept() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(
        "version = 1\n\n[input.keyboard]\nxkb_layout = [\"de\"]\n",
        INCLUDE,
        Some(&sway),
    );
    let mut no = Answer::new(true, false);
    let ran = setup.run_with(&["reset", LAYOUT], &mut no);
    assert_eq!(no.asked, 1);
    assert_eq!(ran.exit, Exit::Refused);
    assert!(
        ran.err.starts_with(&format!(
            "{LAYOUT} is now us. It is reverted unless you keep it.\n"
        )),
        "{}",
        ran.err
    );
    assert_eq!(
        sway.commands(),
        [
            r#"input type:keyboard xkb_layout "us""#,
            r#"input type:keyboard xkb_layout "de""#
        ]
    );
    assert!(setup.settings().contains(r#"xkb_layout = ["de"]"#));

    let ran = setup.run_with(&["reset", LAYOUT], &mut Answer::new(true, true));
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(ran.out, format!("Reset {LAYOUT}.\n"));
    assert_eq!(setup.settings(), EMPTY);

    // Nothing stored: nothing is tried or asked.
    let mut yes = Answer::new(true, true);
    let ran = setup.run_with(&["reset", LAYOUT], &mut yes);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(yes.asked, 0);
    assert_eq!(
        ran.out,
        format!("{LAYOUT} isn't stored by Sway Things; nothing to reset.\n")
    );
    assert_eq!(sway.commands().len(), 3);
}

#[test]
fn a_risky_change_is_stored_only_when_kept() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(EMPTY, INCLUDE, Some(&sway));
    let mut no = Answer::new(true, false);
    let ran = setup.run_with(&["set", LAYOUT, "de"], &mut no);
    assert_eq!(no.asked, 1);
    assert_eq!(ran.exit, Exit::Refused);
    assert!(
        ran.err.ends_with(&format!("Reverted {LAYOUT}.\n")),
        "{}",
        ran.err
    );
    assert_eq!(
        sway.commands(),
        [
            r#"input type:keyboard xkb_layout "de""#,
            r#"input type:keyboard xkb_layout "us""#
        ]
    );
    assert_eq!(setup.settings(), EMPTY);

    let ran = setup.run_with(&["set", LAYOUT, "de"], &mut Answer::new(true, true));
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(ran.out, format!("Set {LAYOUT} to de.\n"));
    assert!(setup.settings().contains(r#"xkb_layout = ["de"]"#));
}

#[test]
fn a_value_goes_to_the_file_asked_for() {
    let mut setup = Setup::new(EMPTY, INCLUDE, None);
    let ran = setup.run(&["set", "windows.gaps.inner", "8", "--host"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert!(
        setup
            .read("config/sway-things/settings.d/laptop.toml")
            .contains("inner = 8")
    );
    assert_eq!(setup.settings(), EMPTY);
    let ran = setup.run(&["get", "windows.gaps.inner"]);
    assert_eq!(
        ran.out,
        "windows.gaps.inner = 8  (settings.d/laptop.toml)\n"
    );

    // A device value goes to this machine unless asked otherwise.
    let ran = setup.run(&[
        "set",
        "input.touchpad.tap",
        "true",
        "--device",
        TOUCHPAD,
        "--shared",
    ]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert!(setup.settings().contains(TOUCHPAD));
}

#[test]
fn without_a_session_the_change_is_saved_for_the_next_start() {
    let mut setup = Setup::new(EMPTY, INCLUDE, None);
    let ran = setup.run(&["set", "windows.gaps.inner", "8"]);
    assert_eq!(ran.exit, Exit::Ok);
    assert_eq!(
        ran.out,
        "Set windows.gaps.inner to 8. sway isn't running; it takes effect when sway starts.\n"
    );
    assert!(
        setup
            .generated("30-appearance.conf")
            .contains("gaps inner 8")
    );
}

#[test]
fn a_reset_removes_the_value() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(
        "version = 1\n\n[windows.gaps]\ninner = 8\n",
        INCLUDE,
        Some(&sway),
    );
    let ran = setup.run(&["reset", "windows.gaps.inner"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(ran.out, "Reset windows.gaps.inner to its default.\n");
    assert_eq!(sway.commands(), ["gaps inner 0", "gaps inner all set 0"]);
    assert_eq!(setup.settings(), EMPTY);

    let ran = setup.run(&["reset", "windows.gaps.inner"]);
    assert_eq!(ran.exit, Exit::Ok);
    assert_eq!(
        ran.out,
        "windows.gaps.inner isn't stored by Sway Things; nothing to reset.\n"
    );
}

#[test]
fn files_that_werent_written_are_reported() {
    let mut setup = Setup::new(EMPTY, INCLUDE, None);
    setup.session.applier = rejecting(&setup);
    let ran = setup.run(&["set", "windows.gaps.inner", "8"]);
    assert_eq!(ran.exit, Exit::Failure, "{ran:?}");
    assert!(
        ran.err.contains(
            "error: sway rejected the new 30-appearance.conf, so the old one stays:\n  \
             line 3: no (from windows.gaps.inner)\n"
        ),
        "{}",
        ran.err
    );
    assert_eq!(setup.generated("30-appearance.conf"), "");

    // A file edited by hand is left alone until --overwrite.
    let mut setup = Setup::new(EMPTY, INCLUDE, None);
    assert_eq!(
        setup.run(&["set", "windows.gaps.inner", "8"]).exit,
        Exit::Ok
    );
    let file = setup.path("config/sway/sway-things/30-appearance.conf");
    fs::write(&file, "gaps inner 1\n").unwrap();
    let ran = setup.run(&["set", "windows.gaps.inner", "9"]);
    assert_eq!(ran.exit, Exit::Refused, "{ran:?}");
    assert!(ran.err.contains("--overwrite"), "{}", ran.err);
    assert!(setup.settings().contains("inner = 9"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "gaps inner 1\n");
    let ran = setup.run(&["set", "windows.gaps.inner", "9", "--overwrite"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert!(fs::read_to_string(&file).unwrap().contains("gaps inner 9"));
}

/// The applier of `setup` with a validator that rejects line 3 of every file.
fn rejecting(setup: &Setup) -> Applier<Validator> {
    let root = setup.dir.path();
    let paths = Paths::new(
        &root.join("config/sway-things"),
        &root.join("state/sway-things"),
        "laptop",
    )
    .unwrap();
    let validator = Validator {
        errors: vec![ConfigError {
            line: 3,
            message: "no".to_owned(),
        }],
    };
    Applier::without_session(
        State::load(Registry::builtin(), paths),
        setup.session.applier.locations().clone(),
        validator,
        setup.session.applier.compositor(),
    )
}

#[test]
fn show_generated_prints_the_files_without_writing_them() {
    let mut setup = Setup::new("version = 1\n\n[windows.gaps]\ninner = 8\n", INCLUDE, None);
    let ran = setup.run(&["show-generated", "30-appearance.conf"]);
    assert!(ran.out.starts_with("# Generated by Sway Things"));
    assert!(ran.out.ends_with("\ngaps inner 8\n"));
    let ran = setup.run(&["show-generated"]);
    let path = setup.path("config/sway/sway-things/30-appearance.conf");
    assert!(
        ran.out
            .starts_with(&format!("# ==> {} <==\n# Generated", path.display())),
        "{}",
        ran.out
    );
    assert!(!path.exists());

    let mut setup = Setup::new(EMPTY, INCLUDE, None);
    let ran = setup.run(&["show-generated"]);
    assert_eq!(ran.out, "");
    assert_eq!(
        ran.err,
        "No settings are stored, so every generated file is empty.\n"
    );
}

#[test]
fn problems_in_the_settings_files_are_warnings() {
    let mut setup = Setup::new(
        "version = 1\n[windows.gaps]\ninner = \"wide\"\n",
        INCLUDE,
        None,
    );
    let ran = setup.run(&["get", "windows.gaps.inner"]);
    assert_eq!(ran.exit, Exit::Ok);
    assert!(ran.err.starts_with("warning: "), "{}", ran.err);
}

#[test]
fn commands_parse_as_documented() {
    let cli = Cli::try_parse_from(["sway-things", "get", "--json"]).unwrap();
    assert_eq!(
        cli.command,
        Command::Get {
            key: None,
            device: None,
            json: true
        }
    );
}

#[test]
fn doctor_reports_a_setup_that_works() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(EMPTY, INCLUDE, Some(&sway));
    assert_eq!(
        setup.run(&["set", "windows.gaps.inner", "8"]).exit,
        Exit::Ok
    );
    let ran = setup.run(&["doctor"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(ran.err, "");
    let config = setup.path("config/sway/config");
    let appearance = setup.path("config/sway/sway-things/30-appearance.conf");
    assert_eq!(
        ran.out,
        format!(
            "Compositor\n  \
             ok       sway 1.10.1 runs in this session.\n\
             \n\
             Sway config\n  \
             ok       Main config: {config} (loaded by the running sway)\n  \
             ok       Sway Things is enabled: {config}:1 includes its files.\n\
             \n\
             Settings files\n  \
             ok       The settings files load without problems (1 stored value).\n\
             \n\
             Generated files\n  \
             ok       10-input.conf isn't needed: no stored setting goes into it.\n  \
             ok       30-appearance.conf is up to date.\n  \
             ok       sway --validate accepts 30-appearance.conf.\n  \
             ok       40-behavior.conf isn't needed: no stored setting goes into it.\n  \
             ok       90-effects.conf isn't needed: no stored setting goes into it.\n\
             \n\
             Conflicts\n  \
             ok       No line of your config sets a stored setting.\n\
             \n\
             Everything is fine.\n",
            config = config.display(),
        )
    );
    assert!(appearance.exists());
}

#[test]
fn doctor_shows_the_include_line_and_fixes_nothing_while_read_only() {
    let mut setup = Setup::new(
        "version = 1\n\n[windows.gaps]\ninner = 8\n",
        "gaps inner 4\n",
        None,
    );
    let ran = setup.run(&["doctor"]);
    assert_eq!(ran.exit, Exit::Failure, "{ran:?}");
    assert!(
        ran.out.contains(
            "  problem  Sway Things is read-only: your config doesn't include its files, so \
             nothing it generates is in effect.\n"
        ),
        "{}",
        ran.out
    );
    let line = format!(
        "\n               include {}/config/sway/sway-things/*.conf\n",
        setup.dir.path().display()
    );
    assert!(ran.out.contains(&line), "{}", ran.out);
    assert!(
        ran.out.contains(
            "  problem  30-appearance.conf is missing, so the settings that go into it have no \
             effect.\n           Fix: sway-things doctor --fix\n"
        ),
        "{}",
        ran.out
    );
    assert!(ran.out.ends_with(
        "2 problems, no warnings.\nRun `sway-things doctor --fix` to write the generated files again.\n"
    ), "{}", ran.out);

    let ran = setup.run(&["doctor", "--fix"]);
    assert_eq!(ran.exit, Exit::Failure, "{ran:?}");
    assert!(
        ran.out.starts_with(
            "Fix\n  Nothing was written: Sway Things is read-only until your config includes its \
             files.\n\nCompositor\n"
        ),
        "{}",
        ran.out
    );
    assert!(!setup.path("config/sway/sway-things").exists());
    assert_eq!(setup.read("config/sway/config"), "gaps inner 4\n");
}

#[test]
fn doctor_fixes_files_that_are_missing_or_out_of_date() {
    let mut setup = Setup::new("version = 1\n\n[windows.gaps]\ninner = 8\n", INCLUDE, None);
    let ran = setup.run(&["doctor"]);
    assert_eq!(ran.exit, Exit::Failure, "{ran:?}");
    assert!(
        ran.out.contains(
            "No sway session runs (not in a sway session). The installed compositor is sway 1.10.1."
        ),
        "{}",
        ran.out
    );
    let ran = setup.run(&["doctor", "--fix"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    let appearance = setup.path("config/sway/sway-things/30-appearance.conf");
    assert!(
        ran.out.starts_with(&format!(
            "Fix\n  wrote {}\n\nCompositor\n",
            appearance.display()
        )),
        "{}",
        ran.out
    );
    assert!(
        ran.out
            .contains("  ok       30-appearance.conf is up to date.\n")
    );
    assert!(
        setup
            .generated("30-appearance.conf")
            .ends_with("\ngaps inner 8\n")
    );

    let ran = setup.run(&["doctor", "--fix"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert!(
        ran.out.starts_with("Fix\n  Nothing for --fix to do.\n\n"),
        "{}",
        ran.out
    );

    // Changed outside: only replaced with --overwrite.
    fs::write(&appearance, "gaps inner 1\n").unwrap();
    let ran = setup.run(&["doctor"]);
    assert_eq!(ran.exit, Exit::Failure, "{ran:?}");
    assert!(
        ran.out
            .contains("  problem  30-appearance.conf was changed outside Sway Things.\n"),
        "{}",
        ran.out
    );
    assert!(ran.out.contains(
        "           Fix: sway-things doctor --fix --overwrite (your changes to the file are lost)\n"
    ), "{}", ran.out);
    let ran = setup.run(&["doctor", "--fix"]);
    assert_eq!(ran.exit, Exit::Failure, "{ran:?}");
    assert!(
        ran.out
            .starts_with("Fix\n  Only generated files that were changed outside"),
        "{}",
        ran.out
    );
    assert_eq!(fs::read_to_string(&appearance).unwrap(), "gaps inner 1\n");
    let ran = setup.run(&["doctor", "--fix", "--overwrite"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert!(
        setup
            .generated("30-appearance.conf")
            .ends_with("\ngaps inner 8\n")
    );
    assert!(Cli::try_parse_from(["sway-things", "doctor", "--overwrite"]).is_err());
}

#[test]
fn doctor_lists_the_users_lines_that_win() {
    let mut setup = Setup::new(
        "version = 1\n\n[windows.gaps]\ninner = 8\n",
        &format!("{INCLUDE}gaps inner 4\n"),
        None,
    );
    assert_eq!(setup.run(&["doctor", "--fix"]).exit, Exit::Ok);
    let ran = setup.run(&["doctor"]);
    // Warnings don't make it fail.
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    let config = setup.path("config/sway/config");
    assert!(
        ran.out.contains(&format!(
            "  warning  The include line isn't last: 1 of your config lines come after it and \
             win where they set the same thing as Sway Things.\n             \
             {config}:2: gaps inner 4\n",
            config = config.display()
        )),
        "{}",
        ran.out
    );
    assert!(
        ran.out.contains(&format!(
            "Conflicts\n  warning  windows.gaps.inner: your config wins: {}:2 comes after the \
             generated file (gaps inner 4).\n",
            config.display()
        )),
        "{}",
        ran.out
    );
    assert!(
        ran.out.ends_with("no problems, 2 warnings.\n"),
        "{}",
        ran.out
    );
}

#[test]
fn doctor_reports_the_settings_files_and_rejected_files() {
    let mut setup = Setup::new(
        "version = 1\n\n[windows.gaps]\ninner = 8\nwide = 3\n",
        INCLUDE,
        None,
    );
    assert_eq!(setup.run(&["doctor", "--fix"]).exit, Exit::Ok);
    setup.session.applier = rejecting(&setup);
    let ran = setup.run(&["doctor"]);
    assert_eq!(ran.exit, Exit::Failure, "{ran:?}");
    // In the report, not as warnings of their own.
    assert_eq!(ran.err, "");
    assert!(
        ran.out.contains("unknown setting windows.gaps.wide\n"),
        "{}",
        ran.out
    );
    assert!(
        ran.out.contains(
            "  problem  sway --validate rejects 30-appearance.conf; sway reports errors for it \
             at every start.\n             line 3: no (from windows.gaps.inner)\n"
        ),
        "{}",
        ran.out
    );
}

#[test]
fn doctor_prints_json_for_scripts() {
    let mut setup = Setup::new("version = 1\n\n[windows.gaps]\ninner = 8\n", INCLUDE, None);
    let ran = setup.run(&["doctor", "--json"]);
    assert_eq!(ran.exit, Exit::Failure, "{ran:?}");
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json["status"], "problem");
    assert_eq!(json["problems"], 1);
    assert_eq!(json["warnings"], 0);
    assert_eq!(json["fix"], serde_json::Value::Null);
    let missing = json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == "file_missing")
        .unwrap();
    assert_eq!(
        missing,
        &serde_json::json!({
            "section": "generated",
            "code": "file_missing",
            "status": "problem",
            "message": "30-appearance.conf is missing, so the settings that go into it have no effect.",
            "details": [],
            "hint": null,
            "file": setup.path("config/sway/sway-things/30-appearance.conf").display().to_string(),
            "line": null,
            "key": null,
            "device": null,
            "fix": "regenerate",
        })
    );

    let ran = setup.run(&["doctor", "--json", "--fix"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["fix"]["result"], "done");
    assert_eq!(json["fix"]["message"], serde_json::Value::Null);
    let files = json["fix"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 4);
    assert_eq!(files[1]["outcome"], "written");
    assert_eq!(files[0]["outcome"], "not_created");
}

#[test]
fn doctor_without_sway_skips_validation() {
    let mut setup = Setup::new("version = 1\n\n[windows.gaps]\ninner = 8\n", INCLUDE, None);
    assert_eq!(setup.run(&["doctor", "--fix"]).exit, Exit::Ok);
    let found = Found::NoSession {
        reason: "SWAYSOCK isn't set".to_owned(),
        installed: Installed::Missing("No such file or directory (os error 2)".to_owned()),
    };
    let applier = &setup.session.applier;
    let checkup = Checkup {
        found: &found,
        state: applier.state(),
        locations: applier.locations(),
        status: &setup.session.status,
        validator: applier.validator(),
    };
    let mut out = Vec::new();
    let exit = block_on(doctor(&checkup, &DoctorArgs::default(), &mut out));
    let out = String::from_utf8(out).unwrap();
    assert_eq!(exit, Exit::Failure, "{out}");
    assert!(
        out.starts_with(
            "Compositor\n  problem  No sway session runs (SWAYSOCK isn't set), and sway can't be \
             run: No such file or directory (os error 2).\n"
        ),
        "{out}"
    );
    assert!(
        out.contains(
            "  warning  The generated files weren't checked with sway --validate: sway can't be \
             run (No such file or directory (os error 2)).\n"
        ),
        "{out}"
    );
    assert!(!out.contains("sway --validate accepts"), "{out}");

    let mut out = Vec::new();
    let args = DoctorArgs {
        fix: true,
        ..DoctorArgs::default()
    };
    // Nothing is out of date that could be told without the compositor.
    assert_eq!(block_on(doctor(&checkup, &args, &mut out)), Exit::Failure);
    assert!(
        String::from_utf8(out)
            .unwrap()
            .starts_with("Fix\n  Nothing for --fix to do.\n")
    );
}
