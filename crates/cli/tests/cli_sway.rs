// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `sway-things` binary against a real sway: a setting set, read back, generated, validated
//! by `sway --validate` and reset.
//!
//! This test needs a running compositor, so a plain `cargo test` skips it.
//! `scripts/check-integration.sh` starts a headless sway whose config includes
//! `$XDG_RUNTIME_DIR/cli/sway/sway-things/*.conf` and runs it with `--ignored`. The test sets
//! `XDG_CONFIG_HOME` to `$XDG_RUNTIME_DIR/cli`, so the binary writes its files where sway reads
//! them.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn run(config_home: &Path, state_home: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_sway-things"))
        .args(args)
        .env("XDG_CONFIG_HOME", config_home)
        .env("XDG_STATE_HOME", state_home)
        .stdin(Stdio::null())
        .output()?)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn the_command_line_changes_a_setting_end_to_end() -> Result<()> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or("no XDG_RUNTIME_DIR")?;
    let config_home = PathBuf::from(runtime).join("cli");
    let state = tempfile::tempdir()?;
    let run = |args: &[&str]| run(&config_home, state.path(), args);
    let generated = config_home.join("sway/sway-things/30-appearance.conf");

    let output = run(&["set", "windows.gaps.inner", "7"])?;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(text(&output.stdout), "Set windows.gaps.inner to 7.\n");
    let written = fs::read_to_string(&generated)?;
    assert!(written.contains("\ngaps inner 7\n"), "{written}");

    let output = run(&["get", "windows.gaps.inner"])?;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        text(&output.stdout),
        "windows.gaps.inner = 7  (settings.toml)\n"
    );
    let output = run(&["show-generated", "30-appearance.conf"])?;
    assert_eq!(text(&output.stdout), written);

    // A risky setting without a terminal only changes with --yes.
    let output = run(&["set", "input.keyboard.xkb_layout", "us"])?;
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    let output = run(&["set", "input.keyboard.xkb_layout", "us", "--yes"])?;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let input = fs::read_to_string(config_home.join("sway/sway-things/10-input.conf"))?;
    assert!(input.contains("xkb_layout"), "{input}");

    let output = run(&["reset", "windows.gaps.inner"])?;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(!fs::read_to_string(&generated)?.contains("gaps inner"));
    Ok(())
}
