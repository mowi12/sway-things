// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `sway-things` binary with the environment of a user: settings and sway config in a
//! temporary home, and a stand-in for sway in `$SWAYSOCK`. Only commands that write nothing run
//! here, because writing validates with the real `sway`; `cli_sway.rs` covers those.

#![allow(
    clippy::unwrap_used,
    reason = "helpers of the tests: a step that fails fails the test"
)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use sway_things_core::doctor::Section;
use sway_things_core::i18n::{self, DOMAIN};
use sway_things_core::ipc::stand_in::{FakeSway, obedient};

struct Home {
    dir: tempfile::TempDir,
    sway: FakeSway,
}

impl Home {
    /// A home with `shared` as settings.toml and `config` as the sway config the stand-in names
    /// as loaded (`{dir}` stands for the home).
    fn new(shared: &str, config: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().display().to_string();
        fs::create_dir_all(dir.path().join("config/sway-things")).unwrap();
        fs::create_dir_all(dir.path().join("config/sway")).unwrap();
        fs::write(dir.path().join("config/sway-things/settings.toml"), shared).unwrap();
        fs::write(
            dir.path().join("config/sway/config"),
            config.replace("{dir}", &root),
        )
        .unwrap();
        let version = format!(
            r#"{{"human_readable": "1.10.1", "variant": "sway", "major": 1, "minor": 10,
                "patch": 1, "loaded_config_file_name": "{root}/config/sway/config"}}"#
        );
        Self {
            sway: obedient(version),
            dir,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, &[])
    }

    /// Runs `args` with the environment variables `env`.
    fn run_with(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let root = self.dir.path();
        // Nothing of the environment of whoever runs the tests, so `sway` is never run.
        Command::new(env!("CARGO_BIN_EXE_sway-things"))
            .args(args)
            .env_clear()
            // No app to ask for the GTK version and no NetworkManager for the system details,
            // unless `env` says otherwise.
            .env("SWAY_THINGS_GUI", root.join("no-app"))
            .env("DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/nonexistent")
            .envs(env.iter().copied())
            .env("HOME", root)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("SWAYSOCK", self.sway.socket())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The checks of a doctor report `out`: what follows the system details.
fn checks(out: &str) -> &str {
    out.split_once("\n\n").map_or(out, |(_, checks)| checks)
}

const INCLUDE: &str = "include {dir}/config/sway/sway-things/*.conf\n";

#[test]
fn help_and_wrong_arguments() {
    let home = Home::new("version = 1\n", INCLUDE);
    let output = home.run(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stdout).contains("show-generated"));
    let output = home.run(&["frobnicate"]);
    assert_eq!(output.status.code(), Some(2));
    let output = home.run(&["set", "windows.gaps.inner"]);
    assert_eq!(output.status.code(), Some(2));
    let output = home.run(&["get", "windows.gaps.wide"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn get_and_show_generated_read_the_users_files() {
    let home = Home::new("version = 1\n\n[windows.gaps]\ninner = 8\n", INCLUDE);
    let output = home.run(&["get", "windows.gaps.inner"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        text(&output.stdout),
        "windows.gaps.inner = 8  (settings.toml)\n"
    );
    let output = home.run(&["show-generated", "30-appearance.conf"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(text(&output.stdout).ends_with("\ngaps inner 8\n"));
    assert!(!home.dir.path().join("config/sway/sway-things").exists());
}

#[test]
fn set_is_refused_while_read_only() {
    let home = Home::new("version = 1\n", "gaps inner 4\n");
    let output = home.run(&["set", "windows.gaps.inner", "8"]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(text(&output.stderr).contains("include "));
    assert_eq!(home.sway.commands(), [] as [&str; 0]);
}

#[test]
fn a_risky_change_without_a_terminal_needs_yes() {
    let home = Home::new("version = 1\n", INCLUDE);
    let output = home.run(&["set", "input.keyboard.xkb_layout", "de"]);
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    assert!(text(&output.stderr).contains("--yes"));
    assert_eq!(home.sway.commands(), [] as [&str; 0]);
    let output = home.run(&["reset", "input.keyboard.xkb_layout"]);
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    assert!(text(&output.stderr).contains("--yes"));
    assert_eq!(home.sway.commands(), [] as [&str; 0]);
}

#[test]
fn doctor_reports_without_writing() {
    // No generated file exists, so nothing is validated and `sway` is never run.
    let home = Home::new(
        "version = 1\n\n[windows.gaps]\ninner = 8\n",
        "gaps inner 4\n",
    );
    let output = home.run(&["doctor"]);
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    let out = text(&output.stdout);
    assert!(
        out.starts_with(&format!(
            "System\n  Sway Things: {}\n  Compositor: sway 1.10.1\n  GTK: unknown\n",
            env!("CARGO_PKG_VERSION")
        )),
        "{out}"
    );
    assert!(out.contains("\n  NetworkManager: not running\n"), "{out}");
    assert!(
        out.contains("\n  nm-connection-editor: not installed\n  blueman-manager: not installed\n"),
        "{out}"
    );
    assert!(
        checks(&out).starts_with("Compositor\n  ok       sway 1.10.1 runs in this session.\n"),
        "{out}"
    );
    assert!(
        out.contains("  problem  Sway Things is read-only: "),
        "{out}"
    );
    assert!(out.ends_with("2 problems, no warnings.\nRun `sway-things doctor --fix` to write the generated files again.\n"), "{out}");

    let output = home.run(&["doctor", "--fix", "--json"]);
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["system"]["compositor"], "sway 1.10.1");
    assert_eq!(json["system"]["gtk"], serde_json::Value::Null);
    assert_eq!(json["system"]["network_manager"], serde_json::Value::Null);
    assert_eq!(json["system"]["tools"]["blueman-manager"], false);
    assert_eq!(json["status"], "problem");
    assert_eq!(json["fix"]["result"], "read_only");
    assert!(!home.dir.path().join("config/sway/sway-things").exists());
    assert!(!home.dir.path().join("state").exists());
}

/// The pseudo-translation of `text`: recognizable, and different from the English text.
fn pseudo(text: &str) -> String {
    format!("[\u{2039}{}\u{203a}]", text.to_uppercase())
}

#[test]
fn doctor_speaks_the_language_of_the_locale() {
    // A catalog for the pseudo-locale `xx` with every heading of the report.
    let locales = tempfile::tempdir().unwrap();
    let messages_dir = locales.path().join("xx/LC_MESSAGES");
    fs::create_dir_all(&messages_dir).unwrap();
    let headings: Vec<(&str, String)> = Section::ALL
        .iter()
        .map(|section| (section.title().msgid(), pseudo(section.title().msgid())))
        .collect();
    let messages: Vec<(&str, &str)> = headings
        .iter()
        .map(|(msgid, text)| (*msgid, text.as_str()))
        .collect();
    fs::write(
        messages_dir.join(format!("{DOMAIN}.mo")),
        i18n::write_mo(&messages),
    )
    .unwrap();
    let home = Home::new("version = 1\n", "gaps inner 4\n");
    let locale_dir = locales.path().to_str().unwrap();
    let env = [(i18n::LOCALEDIR_VAR, locale_dir), ("LANG", "xx_XX.UTF-8")];

    let output = home.run_with(&["doctor"], &env);
    let out = text(&output.stdout);
    assert!(
        checks(&out).starts_with(&format!("{}\n", pseudo("Compositor"))),
        "{out}"
    );
    assert!(
        out.contains(&format!("\n{}\n", pseudo("Sway config"))),
        "{out}"
    );
    assert!(!out.contains("\nSway config\n"), "{out}");
    // Scripts read ids, which stay English.
    let output = home.run_with(&["doctor", "--json"], &env);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["findings"][0]["section"], "compositor");
    // In the C locale everything is English, also with a language list.
    let english = [
        (i18n::LOCALEDIR_VAR, locale_dir),
        ("LC_ALL", "C"),
        ("LANGUAGE", "xx"),
    ];
    let output = home.run_with(&["doctor"], &english);
    assert!(checks(&text(&output.stdout)).starts_with("Compositor\n"));
}

/// A catalog directory with the pseudo-translation of every message of the template, and the
/// environment that picks it.
fn pseudo_locale() -> (tempfile::TempDir, Vec<(&'static str, String)>) {
    let locales = tempfile::tempdir().unwrap();
    let messages_dir = locales.path().join("xx/LC_MESSAGES");
    fs::create_dir_all(&messages_dir).unwrap();
    let pot = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../po/sway-things.pot"
    ))
    .unwrap();
    fs::write(
        messages_dir.join(format!("{DOMAIN}.mo")),
        i18n::pseudo::catalog(&pot),
    )
    .unwrap();
    let dir = locales.path().to_str().unwrap().to_owned();
    let env = vec![
        (i18n::LOCALEDIR_VAR, dir),
        ("LANG", "xx_XX.UTF-8".to_owned()),
    ];
    (locales, env)
}

/// The lines of `text` that still have an English word once the `data` is taken out: in the
/// pseudo-locale every translated letter is accented, so a word of two or more ASCII letters
/// didn't go through a catalog.
fn untranslated(text: &str, data: &[String]) -> Vec<String> {
    // Longest first, so a path goes before the file name in it.
    let mut data: Vec<&str> = data
        .iter()
        .flat_map(|item| item.lines().chain(item.split('/')))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect();
    data.sort_by_key(|item| std::cmp::Reverse(item.len()));
    text.lines()
        .map(|line| {
            data.iter()
                .fold(line.to_owned(), |line, item| line.replace(item, " "))
        })
        .filter(|line| {
            line.split(|c: char| !c.is_ascii_alphabetic())
                .any(|word| word.len() > 1)
        })
        .collect()
}

/// Every string in `json` and the names of the tools, as data the output may show untranslated.
fn strings(json: &serde_json::Value, found: &mut Vec<String>) {
    match json {
        serde_json::Value::String(text) => found.push(text.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|item| strings(item, found)),
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                // The tools are named by their keys.
                if key == "tools" {
                    found.extend(
                        value
                            .as_object()
                            .into_iter()
                            .flat_map(|tools| tools.keys().cloned()),
                    );
                }
                strings(value, found);
            }
        }
        _ => {}
    }
}

/// The texts of the system details, which the core doesn't mark yet (#201).
const SYSTEM: &[&str] = &[
    "Sway Things",
    "Compositor",
    "GTK",
    "Distribution",
    "NetworkManager",
    "Kernel",
    "not installed",
    "not running",
    "installed",
    "unknown",
];

/// The errors of the system, which Rust's standard library only has in English.
const OS_ERRORS: &[&str] = &["No such file or directory (os error 2)"];

/// The commands the output names, which aren't translated.
const COMMANDS: &[&str] = &[
    "sway-things doctor --fix --overwrite",
    "sway-things doctor --fix",
];

#[test]
fn the_pseudo_locale_leaves_no_text_untranslated() {
    let (_locales, env) = pseudo_locale();
    let env: Vec<(&str, &str)> = env
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let mut cases: Vec<(Home, Vec<&str>)> = Vec::new();
    // Read-only, with a conflict and a missing file.
    let shared = "version = 1\n\n[windows.gaps]\ninner = 8\n";
    cases.push((Home::new(shared, "gaps inner 4\n"), vec!["doctor"]));
    cases.push((Home::new(shared, "gaps inner 4\n"), vec!["doctor", "--fix"]));
    // The config includes the generated files, and a line after it wins.
    let config = format!("{INCLUDE}gaps inner 4\n");
    cases.push((Home::new(shared, &config), vec!["doctor"]));
    cases.push((Home::new(shared, &config), vec!["get"]));
    cases.push((
        Home::new(shared, &config),
        vec!["get", "windows.gaps.inner"],
    ));
    cases.push((
        Home::new(shared, &config),
        vec!["get", "windows.gaps.outer"],
    ));
    cases.push((
        Home::new("version = 1\n", INCLUDE),
        vec!["set", "input.keyboard.xkb_layout", "de"],
    ));
    cases.push((
        Home::new("version = 1\n", INCLUDE),
        vec!["reset", "input.keyboard.xkb_layout"],
    ));
    cases.push((Home::new("version = 1\n", INCLUDE), vec!["show-generated"]));
    cases.push((Home::new("version = 1\n", INCLUDE), vec!["open"]));
    for (home, args) in &cases {
        // What the user typed and what the JSON output says are data: keys, values, paths, and
        // the core's findings (#201).
        let mut data: Vec<String> = SYSTEM
            .iter()
            .chain(COMMANDS)
            .chain(OS_ERRORS)
            .chain(args)
            .map(|text| (*text).to_owned())
            .collect();
        // The home, and the app `run_with` names in it.
        data.push(home.dir.path().join("no-app").display().to_string());
        let json_args: Vec<&str> = match args.first() {
            Some(&"doctor") => vec!["doctor", "--json"],
            Some(&"get") => [args.as_slice(), &["--json"]].concat(),
            _ => Vec::new(),
        };
        if !json_args.is_empty() {
            let output = home.run(&json_args);
            let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            strings(&json, &mut data);
        }
        let output = home.run_with(args, &env);
        for stream in [&output.stdout, &output.stderr] {
            let out = text(stream);
            assert_eq!(
                untranslated(&out, &data),
                [] as [&str; 0],
                "{args:?}: {out}"
            );
        }
        assert!(
            text(&output.stdout).contains(i18n::pseudo::OPEN)
                || text(&output.stderr).contains(i18n::pseudo::OPEN),
            "{args:?}: {output:?}"
        );
    }
}

#[test]
fn doctor_colors_only_when_asked() {
    let home = Home::new(
        "version = 1\n\n[windows.gaps]\ninner = 8\n",
        "gaps inner 4\n",
    );
    let plain = home.run(&["doctor"]);
    let plain_out = text(&plain.stdout);
    assert!(!plain_out.contains('\x1b'), "{plain_out}");
    // A pipe gets no spinner either.
    assert_eq!(text(&plain.stderr), "");

    let colored = home.run(&["doctor", "--color", "always"]);
    assert_eq!(colored.status.code(), plain.status.code());
    let out = text(&colored.stdout);
    assert!(
        out.starts_with("\x1b[1mSystem\x1b[0m\n  Sway Things: ")
            && checks(&out).starts_with("\x1b[1mCompositor\x1b[0m\n  \x1b[32mok\x1b[0m       "),
        "{out}"
    );
    assert!(out.contains("  \x1b[31mproblem\x1b[0m  "), "{out}");
    assert_eq!(
        text(&colored.stderr),
        "",
        "the spinner only draws on a terminal"
    );

    let forced = home.run_with(&["doctor"], &[("CLICOLOR_FORCE", "1")]);
    assert!(text(&forced.stdout).contains('\x1b'));
    for output in [
        home.run(&["doctor", "--color", "never"]),
        home.run_with(&["doctor"], &[("CLICOLOR_FORCE", "1"), ("NO_COLOR", "1")]),
        home.run_with(&["doctor", "--color", "never"], &[("CLICOLOR_FORCE", "1")]),
    ] {
        assert_eq!(text(&output.stdout), plain_out);
    }
    let json = home.run(&["doctor", "--json", "--color", "always"]);
    assert!(!json.stdout.contains(&0x1b));

    // The other commands take --color and print as before.
    let output = home.run(&["get", "windows.gaps.inner", "--color", "always"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        text(&output.stdout),
        "windows.gaps.inner = 8  (settings.toml)\n"
    );
}

/// A stand-in for `sway-things-gui` in `dir`: it writes its arguments, one per line, to the file
/// it returns.
fn stand_in_gui(dir: &Path) -> (PathBuf, PathBuf) {
    let gui = dir.join("gui");
    let args = dir.join("gui.args");
    fs::write(
        &gui,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.tmp\"\n/bin/mv \"$0.tmp\" \"$0.args\"\n",
    )
    .unwrap();
    fs::set_permissions(&gui, fs::Permissions::from_mode(0o755)).unwrap();
    (gui, args)
}

/// What the stand-in got, once it ran.
fn started_with(args: &Path) -> Option<String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if let Ok(text) = fs::read_to_string(args) {
            return Some(text);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn open_starts_the_app_at_a_link() {
    let home = Home::new("version = 1\n", INCLUDE);
    let (gui, args) = stand_in_gui(home.dir.path());
    let env = [("SWAY_THINGS_GUI", gui.to_str().unwrap())];

    let output = home.run_with(&["open", "input.keyboard.repeat_rate"], &env);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        started_with(&args).as_deref(),
        Some("input.keyboard.repeat_rate\n")
    );
    fs::remove_file(&args).unwrap();

    let output = home.run_with(&["open", "keyboard"], &env);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(started_with(&args).as_deref(), Some("keyboard\n"));
    fs::remove_file(&args).unwrap();

    // Without a command, and `open` without a link, the app starts without arguments.
    for command in [&[] as &[&str], &["open"]] {
        let output = home.run_with(command, &env);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(started_with(&args).as_deref(), Some("\n"));
        fs::remove_file(&args).unwrap();
    }
    assert_eq!(home.sway.commands(), [] as [&str; 0]);
}

#[test]
fn open_names_the_valid_links_and_starts_nothing() {
    let home = Home::new("version = 1\n", INCLUDE);
    let (gui, args) = stand_in_gui(home.dir.path());
    let env = [("SWAY_THINGS_GUI", gui.to_str().unwrap())];

    let output = home.run_with(&["open", "keyboards"], &env);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let err = text(&output.stderr);
    assert!(
        err.starts_with("error: unknown page \"keyboards\""),
        "{err}"
    );
    assert!(err.contains("mouse-touchpad, keyboard,"), "{err}");

    let output = home.run_with(&["open", "input.keyboard.repeat_speed"], &env);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let err = text(&output.stderr);
    assert!(err.contains("unknown setting"), "{err}");
    assert!(err.contains("input.keyboard.repeat_rate"), "{err}");

    std::thread::sleep(Duration::from_millis(200));
    assert!(!args.exists(), "the app was started");
}

#[test]
fn open_fails_without_the_app() {
    let home = Home::new("version = 1\n", INCLUDE);
    let missing = home.dir.path().join("no-such-gui");
    let output = home.run_with(&["open"], &[("SWAY_THINGS_GUI", missing.to_str().unwrap())]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        text(&output.stderr).starts_with("error: can't start "),
        "{output:?}"
    );
}
