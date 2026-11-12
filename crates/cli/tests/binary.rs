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
use std::process::{Command, Output, Stdio};

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
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let out = text(&output.stdout);
    assert!(
        out.starts_with("Compositor\n  ok       sway 1.10.1 runs in this session.\n"),
        "{out}"
    );
    assert!(
        out.contains("  problem  Sway Things is read-only: "),
        "{out}"
    );
    assert!(out.ends_with("2 problems, no warnings.\nRun `sway-things doctor --fix` to write the generated files again.\n"), "{out}");

    let output = home.run(&["doctor", "--fix", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "problem");
    assert_eq!(json["fix"]["result"], "read_only");
    assert!(!home.dir.path().join("config/sway/sway-things").exists());
    assert!(!home.dir.path().join("state").exists());
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
        out.starts_with("\x1b[1mCompositor\x1b[0m\n  \x1b[32mok\x1b[0m       "),
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
