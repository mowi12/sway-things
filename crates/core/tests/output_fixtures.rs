// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generates the `output` fixtures in `ci/integration/` from the output builders and checks that
//! the committed files are up to date. `scripts/check-integration.sh` feeds them to a real sway:
//! `output.conf` must pass `sway --validate`, and every command in `output-ipc.txt` must succeed.
//!
//! The commands name an output that isn't connected, so sway only stores them: the headless
//! session the other tests run in keeps its one output as it is. Every output (`*`) only gets
//! values that leave it so.
//!
//! After changing the builders, regenerate the files with
//! `SWAY_THINGS_UPDATE_FIXTURES=1 cargo test -p sway-things-core`.

mod common;

use common::{Result, push_line};
use sway_things_core::command::output::{Mode, Output, OutputSelector, Transform};
use sway_things_core::command::{SwayCmd, Target};

/// An output identifier with characters that need quoting, including the `$mod` variable that
/// `ci/integration/sway.conf` defines and the comma that separates IPC commands.
const OUTPUT: &str = r#"Fake "Make" Co.,Ltd Model'$mod;x 0x1"#;

fn all_commands() -> Result<Vec<SwayCmd>> {
    let output = Output::new(OutputSelector::output(OUTPUT)?);
    let mut commands = vec![
        output.mode(Mode::parse("1920x1080")?),
        output.mode(Mode::parse("2560x1440@59.951Hz")?),
        output.mode(Mode::parse("1920x1080@60Hz")?),
        output.position(1920, 0),
        output.position(-1280, -304),
        output.scale(1.0)?,
        output.scale(1.25)?,
        output.scale(2.0)?,
        output.enable(false),
        output.enable(true),
    ];
    for transform in Transform::ALL {
        commands.push(output.transform(transform));
    }
    let named = Output::new(OutputSelector::output("DP-99")?);
    commands.push(named.position(0, 0));
    let all = Output::new(OutputSelector::All);
    commands.push(all.transform(Transform::Normal));
    commands.push(all.scale(1.0)?);
    Ok(commands)
}

fn fixture(commands: &[SwayCmd], target: Target) -> Result<String> {
    let mut out = common::header("output_fixtures.rs", false);
    for cmd in commands {
        push_line(&mut out, &cmd.render(target)?);
    }
    Ok(out)
}

#[test]
fn fixtures_are_up_to_date() -> Result<()> {
    let commands = all_commands()?;
    common::check(&[
        ("output.conf", fixture(&commands, Target::Config)?),
        ("output-ipc.txt", fixture(&commands, Target::Ipc)?),
    ])
}
