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
use sway_things_cli::{
    Cli, Command, DoctorArgs, Exit, Hook, Look, Prompt, QuickAction, Session, Spinner, doctor,
    doctor_with, run,
};
use sway_things_core::apply::{Applier, Countdown};
use sway_things_core::command::{self, QuickSwitch};
use sway_things_core::delegate;
use sway_things_core::doctor::{Checkup, Found, Installed};
use sway_things_core::generate::{ConfigError, Locations, Validate};
use sway_things_core::gsettings::{MemoryGSettings, NoGSettings};
use sway_things_core::i18n::Text;
use sway_things_core::include::IncludeStatus;
use sway_things_core::ipc::Compositor;
use sway_things_core::ipc::stand_in::{FakeSway, SWAY_1_10, obedient};
use sway_things_core::notifications::mako;
use sway_things_core::registry::{Registry, Value};
use sway_things_core::state::{Paths, State};
use sway_things_core::sway_config::Disk;
use sway_things_core::system::Details;
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
            cli.command.as_ref().unwrap(),
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
fn a_value_for_one_display_is_set_and_listed_by_display() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(EMPTY, INCLUDE, Some(&sway));
    let ran = setup.run(&["set", "wallpaper.color", "#285577"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    let ran = setup.run(&[
        "set",
        "wallpaper.image",
        "~/Pictures/lake.jpg",
        "--device",
        "eDP-1",
    ]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(
        ran.out,
        "Set wallpaper.image --device eDP-1 to ~/Pictures/lake.jpg.\n"
    );
    assert_eq!(
        sway.commands(),
        [
            "output * bg #285577 solid_color",
            "output \"eDP-1\" bg \"~/'Pictures/lake.jpg'\" fill #285577"
        ]
    );
    assert_eq!(
        setup
            .generated("20-output.conf")
            .lines()
            .skip(2)
            .collect::<Vec<_>>(),
        [
            "output * bg #285577 solid_color",
            "output \"eDP-1\" bg \"~/'Pictures/lake.jpg'\" fill #285577"
        ]
    );

    let ran = setup.run(&["get"]);
    assert_eq!(
        ran.out,
        "wallpaper.color = #285577  (settings.toml)\n\
         \n\
         display eDP-1\n  \
         wallpaper.image = ~/Pictures/lake.jpg  (settings.d/laptop.toml)\n"
    );
    let ran = setup.run(&["get", "wallpaper.color", "--device", "eDP-1"]);
    assert_eq!(
        ran.out,
        "wallpaper.color --device eDP-1 = #285577  (settings.toml, for every display)\n"
    );
    let ran = setup.run(&["get", "--device", "eDP-1", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(json[0]["device"], "eDP-1");

    // `*` stands for every display, not for one.
    let ran = setup.run(&["reset", "wallpaper.image", "--device", "*"]);
    assert_eq!(ran.exit, Exit::Usage, "{ran:?}");
}

#[test]
fn the_settings_of_a_collection_are_named_with_collection() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(EMPTY, INCLUDE, Some(&sway));
    let ran = setup.run(&[
        "set",
        "collection.order",
        "in_order",
        "--collection",
        "Nature",
    ]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(
        ran.out,
        "Set collection.order --collection Nature to in_order.\n"
    );
    let ran = setup.run(&["get"]);
    assert_eq!(
        ran.out,
        "\ncollection Nature\n  collection.order = in_order  (settings.toml)\n"
    );

    // A collection only for the settings of one, and always for them.
    let ran = setup.run(&["set", "wallpaper.fill", "fit", "--collection", "Nature"]);
    assert_eq!(ran.exit, Exit::Usage, "{ran:?}");
    let ran = setup.run(&["reset", "collection.order"]);
    assert_eq!(ran.exit, Exit::Usage, "{ran:?}");
    assert_eq!(
        ran.err,
        "error: collection.order is a setting of a collection; name it with --collection\n"
    );
}

#[test]
fn the_wallpaper_hook_shows_a_picture_of_each_collection_at_login() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(EMPTY, INCLUDE, Some(&sway));
    let pictures = setup.path("pictures");
    fs::create_dir_all(&pictures).unwrap();
    for name in ["a.png", "b.png"] {
        fs::write(pictures.join(name), b"").unwrap();
    }
    let folder = pictures.display().to_string();
    for (key, value) in [
        ("collection.pictures", folder.as_str()),
        ("collection.order", "in_order"),
        ("collection.change", "login"),
    ] {
        let ran = setup.run(&["set", key, value, "--collection", "Nature"]);
        assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    }
    let ran = setup.run(&["set", "wallpaper.collection", "Nature"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    let link = setup.path("state/sway-things/wallpaper/all");
    assert!(
        setup
            .generated("20-output.conf")
            .ends_with("exec sway-things hook wallpaper\n")
    );
    let before = fs::read_link(&link).unwrap();

    // Each login moves on to the next picture and shows it.
    let ran = setup.run(&["hook", "wallpaper"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    let after = fs::read_link(&link).unwrap();
    assert_ne!(after, before);
    assert!(after.starts_with(&pictures), "{after:?}");
    assert_eq!(
        sway.commands().last().map(String::as_str),
        Some(format!("output * bg \"'{}'\" fill #000000", link.display()).as_str())
    );
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
fn an_environment_variable_is_checked_like_in_the_app() {
    let mut setup = Setup::new(EMPTY, INCLUDE, None);
    let ran = setup.run(&["set", "environment.variables", "HOME=/tmp"]);
    assert_eq!(ran.exit, Exit::Usage, "{ran:?}");
    assert!(ran.err.contains("the session sets it itself"), "{ran:?}");
    let ran = setup.run(&["set", "environment.variables", "PATH=/opt/bin"]);
    assert_eq!(ran.exit, Exit::Usage, "{ran:?}");
    let ran = setup.run(&[
        "set",
        "environment.variables",
        "EDITOR=vim, PATH=$HOME/.local/bin:$PATH",
    ]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert!(
        setup
            .settings()
            .contains("variables = [\"EDITOR=vim\", \"PATH=$HOME/.local/bin:$PATH\"]"),
        "{}",
        setup.settings()
    );
}

#[test]
fn a_reset_sends_the_value_of_the_users_config() {
    let sway = obedient(SWAY_1_10);
    let mut setup = Setup::new(
        "version = 1\n\n[windows.gaps]\ninner = 12\n",
        &format!("gaps inner 8\n{INCLUDE}"),
        Some(&sway),
    );
    let ran = setup.run(&["reset", "windows.gaps.inner"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(
        ran.out,
        "Reset windows.gaps.inner to your config's value.\n"
    );
    // To every open workspace: the config's line sets only the default of new ones.
    assert_eq!(sway.commands(), ["gaps inner 8", "gaps inner all set 8"]);
    assert_eq!(setup.settings(), EMPTY);
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

/// Parses a command line the core writes for a program to run, which splits it at spaces.
fn parse_written(line: &str) -> Result<Hook, String> {
    match Cli::try_parse_from(line.split(' ')) {
        Ok(Cli {
            command: Some(Command::Hook(hook)),
            ..
        }) => Ok(hook),
        Ok(cli) => Err(format!("{line} runs {:?}, not a hook", cli.command)),
        Err(error) => Err(format!("{line}: {error}")),
    }
}

#[test]
fn every_hook_the_core_writes_runs_that_hook() {
    for hook in command::Hook::ALL {
        let args: &[&str] = match hook {
            command::Hook::Launch => &["org.example.App.desktop"],
            command::Hook::Quick => &["dnd"],
            _ => &[],
        };
        let line = hook.command_line(args);
        assert_eq!(parse_written(&line).unwrap().name(), hook, "{line}");
    }
    for switch in QuickSwitch::ALL {
        for (line, action) in [
            (switch.toggle_line(), None),
            (switch.state_line(), Some(QuickAction::State)),
        ] {
            let Hook::Quick {
                which,
                action: parsed,
            } = parse_written(&line).unwrap()
            else {
                panic!("{line} isn't the quick hook");
            };
            assert_eq!((which.switch(), parsed), (switch, action), "{line}");
        }
    }
    // mako's click binding, with the id mako puts in for `$id`.
    let binding = mako::click_binding();
    let line = binding
        .strip_prefix("on-button-left=exec ")
        .unwrap()
        .replace("\"$id\"", "7");
    assert_eq!(
        parse_written(&line),
        Ok(Hook::NotificationClick { mako: Some(7) })
    );
}

#[test]
fn commands_parse_as_documented() {
    let cli = Cli::try_parse_from(["sway-things", "get", "--json"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Command::Get {
            key: None,
            device: None,
            collection: None,
            json: true
        })
    );
    let cli = Cli::try_parse_from(["sway-things"]).unwrap();
    assert_eq!(cli.command, None);
    let cli = Cli::try_parse_from(["sway-things", "hook", "gsettings"]).unwrap();
    assert_eq!(cli.command, Some(Command::Hook(Hook::Gsettings)));
    let cli = Cli::try_parse_from(["sway-things", "open", "input.keyboard.repeat_rate"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Command::Open {
            link: Some("input.keyboard.repeat_rate".to_owned())
        })
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
             ok       20-output.conf isn't needed: no stored setting goes into it.\n  \
             ok       30-appearance.conf is up to date.\n  \
             ok       sway --validate accepts 30-appearance.conf.\n  \
             ok       40-behavior.conf isn't needed: no stored setting goes into it.\n  \
             ok       50-bindings.conf isn't needed: no stored setting goes into it.\n  \
             ok       70-autostart.conf isn't needed: no stored setting goes into it.\n  \
             ok       75-bar.conf isn't needed: no stored setting goes into it.\n  \
             ok       80-hooks.conf isn't needed: no stored setting goes into it.\n  \
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
    assert_eq!(ran.exit, Exit::Problems, "{ran:?}");
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
    assert_eq!(ran.exit, Exit::Problems, "{ran:?}");
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
    assert_eq!(ran.exit, Exit::Problems, "{ran:?}");
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
    assert_eq!(ran.exit, Exit::Problems, "{ran:?}");
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
    assert_eq!(ran.exit, Exit::Problems, "{ran:?}");
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
            "  warning  The include line isn't last: 1 of your config lines comes after it and \
             wins where it sets the same thing as Sway Things.\n             \
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
    assert_eq!(ran.exit, Exit::Problems, "{ran:?}");
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
    assert_eq!(ran.exit, Exit::Problems, "{ran:?}");
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
    assert_eq!(files.len(), 9);
    assert_eq!(files[1]["outcome"], "not_created");
    assert_eq!(files[2]["outcome"], "written");
    assert_eq!(files[0]["outcome"], "not_created");
}

#[test]
fn doctor_without_sway_skips_validation() {
    let mut setup = Setup::new("version = 1\n\n[windows.gaps]\ninner = 8\n", INCLUDE, None);
    assert_eq!(setup.run(&["doctor", "--fix"]).exit, Exit::Ok);
    let found = Found::NoSession {
        reason: Text::data("SWAYSOCK isn't set"),
        installed: Installed::Missing(Text::data("No such file or directory (os error 2)")),
    };
    let applier = &setup.session.applier;
    let checkup = Checkup {
        found: &found,
        state: applier.state(),
        locations: applier.locations(),
        status: &setup.session.status,
        validator: applier.validator(),
        outputs: None,
        displays: sway_things_core::generate::Displays::Sway,
        kanshi: None,
        notifications: None,
        path: None,
        gsettings: &NoGSettings,
        apps: None,
        scanners: None,
        environment: None,
    };
    let mut out = Vec::new();
    let exit = block_on(doctor(&checkup, None, &DoctorArgs::default(), &mut out));
    let out = String::from_utf8(out).unwrap();
    assert_eq!(exit, Exit::Problems, "{out}");
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
    assert_eq!(
        block_on(doctor(&checkup, None, &args, &mut out)),
        Exit::Problems
    );
    assert!(
        String::from_utf8(out)
            .unwrap()
            .starts_with("Fix\n  Nothing for --fix to do.\n")
    );
}

/// The doctor's report on `setup` without a session, as `look` says.
fn doctor_looking(setup: &Setup, args: &DoctorArgs, look: &Look) -> (Exit, String) {
    let found = Found::NoSession {
        reason: Text::data("not in a sway session"),
        installed: Installed::Version(setup.session.applier.compositor()),
    };
    let applier = &setup.session.applier;
    let checkup = Checkup {
        found: &found,
        state: applier.state(),
        locations: applier.locations(),
        status: &setup.session.status,
        validator: applier.validator(),
        outputs: None,
        displays: sway_things_core::generate::Displays::Sway,
        kanshi: None,
        notifications: None,
        path: None,
        gsettings: &NoGSettings,
        apps: None,
        scanners: None,
        environment: None,
    };
    let mut out = Vec::new();
    let exit = block_on(doctor_with(&checkup, None, args, &mut out, look));
    (exit, String::from_utf8(out).unwrap())
}

/// The doctor's report on `setup`, plain and in color.
fn doctor_in_color(setup: &Setup, args: &DoctorArgs) -> (String, String) {
    let (exit, plain) = doctor_looking(setup, args, &Look::default());
    let (colored_exit, colored) = doctor_looking(setup, args, &in_color());
    assert_eq!(colored_exit, exit);
    (plain, colored)
}

fn in_color() -> Look {
    Look {
        color: true,
        spinner: Spinner::hidden(),
    }
}

#[test]
fn doctor_colors_only_the_words_that_matter() {
    let mut setup = Setup::new(
        "version = 1\n\n[windows.gaps]\ninner = 8\n",
        &format!("{INCLUDE}gaps inner 4\n"),
        None,
    );
    let (plain, colored) = doctor_in_color(&setup, &DoctorArgs::default());
    assert!(!plain.contains('\x1b'), "{plain}");
    // The same text and layout, only styled.
    assert_eq!(anstream::adapter::strip_str(&colored).to_string(), plain);
    let config = setup.path("config/sway/config").display().to_string();
    for styled in [
        "\x1b[1mCompositor\x1b[0m\n".to_owned(),
        "\n\x1b[1mSway config\x1b[0m\n".to_owned(),
        // Only the word is styled, not the padding of the column.
        "  \x1b[32mok\x1b[0m       Main config: \x1b[2m{config}\x1b[0m\n"
            .replace("{config}", &config),
        "  \x1b[33mwarning\x1b[0m  The include line isn't last".to_owned(),
        format!("             \x1b[2m{config}:2: gaps inner 4\x1b[0m\n"),
        "  \x1b[31mproblem\x1b[0m  30-appearance.conf is missing".to_owned(),
        "           \x1b[2mFix: \x1b[0m\x1b[36msway-things doctor --fix\x1b[0m\n".to_owned(),
        format!("your config wins: \x1b[2m{config}:2\x1b[0m comes after"),
        "\n\x1b[1m\x1b[31m1 problem\x1b[0m, \x1b[1m\x1b[33m2 warnings\x1b[0m.\n".to_owned(),
        "Run `\x1b[36msway-things doctor --fix\x1b[0m` to write".to_owned(),
    ] {
        assert!(colored.contains(&styled), "{styled:?} in {colored}");
    }

    let fix = DoctorArgs {
        fix: true,
        ..DoctorArgs::default()
    };
    let (exit, out) = doctor_looking(&setup, &fix, &in_color());
    assert_eq!(exit, Exit::Ok, "{out}");
    let appearance = setup.path("config/sway/sway-things/30-appearance.conf");
    assert!(
        out.starts_with(&format!(
            "\x1b[1mFix\x1b[0m\n  wrote \x1b[2m{}\x1b[0m\n",
            appearance.display()
        )),
        "{out}"
    );

    // Scripts never get colors.
    let json = DoctorArgs {
        json: true,
        ..DoctorArgs::default()
    };
    let (_, out) = doctor_looking(&setup, &json, &in_color());
    assert!(!out.contains('\x1b'), "{out}");

    fs::write(
        setup.path("config/sway/config"),
        INCLUDE.replace("{dir}", &setup.dir.path().display().to_string()),
    )
    .unwrap();
    setup.session.status = IncludeStatus::detect(
        Some(&setup.path("config/sway/config")),
        &Disk,
        setup.session.applier.locations(),
    );
    let (plain, colored) = doctor_in_color(&setup, &DoctorArgs::default());
    assert!(plain.ends_with("\nEverything is fine.\n"), "{plain}");
    assert!(
        colored.ends_with("\n\x1b[1m\x1b[32mEverything is fine.\x1b[0m\n"),
        "{colored}"
    );
}

#[test]
fn doctor_fails_when_it_cant_fix() {
    let mut setup = Setup::new("version = 1\n\n[windows.gaps]\ninner = 8\n", INCLUDE, None);
    // The manifest of the generated files can't be written.
    fs::create_dir_all(setup.path("state")).unwrap();
    fs::write(setup.path("state/sway-things"), "").unwrap();
    let ran = setup.run(&["doctor", "--fix"]);
    assert_eq!(ran.exit, Exit::Failure, "{ran:?}");
    assert!(
        ran.out
            .starts_with("Fix\n  The generated files couldn't be written: "),
        "{}",
        ran.out
    );
}

#[test]
fn doctor_starts_with_the_system_details() {
    let setup = Setup::new("version = 1\n", INCLUDE, None);
    let found = Found::NoSession {
        reason: Text::data("not in a sway session"),
        installed: Installed::Version(setup.session.applier.compositor()),
    };
    let applier = &setup.session.applier;
    let checkup = Checkup {
        found: &found,
        state: applier.state(),
        locations: applier.locations(),
        status: &setup.session.status,
        validator: applier.validator(),
        outputs: None,
        displays: sway_things_core::generate::Displays::Sway,
        kanshi: None,
        notifications: None,
        path: None,
        gsettings: &NoGSettings,
        apps: None,
        scanners: None,
        environment: None,
    };
    let system = Details {
        compositor: Some(setup.session.applier.compositor()),
        gtk: Some("4.18.6".to_owned()),
        distribution: Some("Debian GNU/Linux 13 (trixie)".to_owned()),
        network_manager: None,
        sound_server: Some("PulseAudio (on PipeWire 1.6.9)".to_owned()),
        upower: Some("1.91.5".to_owned()),
        cups: Some("2.4".to_owned()),
        cups_pk_helper: false,
        kernel: Some("6.12.48+deb13-amd64".to_owned()),
        tools: vec![
            (delegate::NM_CONNECTION_EDITOR, false),
            (delegate::BLUEMAN, true),
        ],
    };
    let mut out = Vec::new();
    block_on(doctor(
        &checkup,
        Some(&system),
        &DoctorArgs::default(),
        &mut out,
    ));
    let out = String::from_utf8(out).unwrap();
    let expected = format!(
        "System\n  Sway Things: {}\n  Compositor: sway 1.10.1\n  GTK: 4.18.6\n  \
         Distribution: Debian GNU/Linux 13 (trixie)\n  NetworkManager: not running\n  \
         Sound server: PulseAudio (on PipeWire 1.6.9)\n  UPower: 1.91.5\n  CUPS: 2.4\n  \
         cups-pk-helper: not installed\n  Kernel: 6.12.48+deb13-amd64\n  \
         nm-connection-editor: not installed\n  blueman-manager: installed\n\nCompositor\n",
        env!("CARGO_PKG_VERSION")
    );
    assert!(out.starts_with(&expected), "{out}");

    let mut out = Vec::new();
    let args = DoctorArgs {
        json: true,
        ..DoctorArgs::default()
    };
    block_on(doctor(&checkup, Some(&system), &args, &mut out));
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        json["system"],
        serde_json::json!({
            "sway_things": env!("CARGO_PKG_VERSION"),
            "compositor": "sway 1.10.1",
            "gtk": "4.18.6",
            "distribution": "Debian GNU/Linux 13 (trixie)",
            "network_manager": null,
            "sound_server": "PulseAudio (on PipeWire 1.6.9)",
            "upower": "1.91.5",
            "cups": "2.4",
            "cups_pk_helper": false,
            "kernel": "6.12.48+deb13-amd64",
            "tools": {"nm-connection-editor": false, "blueman-manager": true},
        })
    );

    // Without details, as on a session, the report starts with the checks.
    let mut out = Vec::new();
    block_on(doctor(&checkup, None, &args, &mut out));
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["system"], serde_json::Value::Null);
}

#[test]
fn the_cursor_reaches_gsettings_on_set_reset_and_with_the_hook() {
    let sway = obedient(SWAY_1_10);
    let shared = "version = 1\n\n[cursor]\ntheme = \"Adwaita\"\n";
    let mut setup = Setup::new(shared, INCLUDE, Some(&sway));
    let store = MemoryGSettings::default();
    setup.session.applier.set_gsettings(Box::new(store.clone()));
    let get = |key: &str| store.get("org.gnome.desktop.interface", key);

    // The hook writes what is stored and leaves the rest.
    let ran = setup.run(&["hook", "gsettings"]);
    assert_eq!(
        (ran.exit, ran.out.as_str(), ran.err.as_str()),
        (Exit::Ok, "", "")
    );
    assert_eq!(get("cursor-theme"), Some(Value::Text("Adwaita".to_owned())));
    assert_eq!(get("cursor-size"), None);

    let ran = setup.run(&["set", "cursor.size", "48"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(sway.commands(), [r#"seat * xcursor_theme "Adwaita" 48"#]);
    assert_eq!(get("cursor-size"), Some(Value::Integer(48)));
    assert_eq!(
        setup.generated("80-hooks.conf").lines().last(),
        Some("exec_always sway-things hook gsettings")
    );

    let ran = setup.run(&["reset", "cursor.theme"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(get("cursor-theme"), None);
}

#[test]
fn the_display_hooks_run_the_cores_hooks_in_the_session() {
    const ALL_OFF: &str = r#"[{"name": "eDP-1", "active": false, "primary": false, "make": "A",
        "model": "B", "serial": "C", "modes": []}]"#;
    let sway = sway_things_core::ipc::stand_in::with_outputs(&[ALL_OFF]);
    let mut setup = Setup::new(EMPTY, INCLUDE, Some(&sway));

    // The same hook the user's sway runs: it switches the only display on and says so.
    let ran = setup.run(&["hook", "displays"]);
    assert_eq!(ran.exit, Exit::Ok, "{ran:?}");
    assert_eq!(ran.err, "No display was on, so eDP-1 was switched on.\n");
    assert_eq!(sway.commands(), [r#"output "eDP-1" enable"#]);

    // No rule places the workspaces and no display mirrors: nothing more is sent.
    for hook in ["workspaces", "mirror"] {
        let ran = setup.run(&["hook", hook]);
        assert_eq!((ran.exit, ran.err.as_str()), (Exit::Ok, ""), "{hook}");
    }
    assert_eq!(sway.commands(), [r#"output "eDP-1" enable"#]);
}
