// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generates the key binding fixtures in `ci/integration/` from the builders and checks that the
//! committed files are up to date. `scripts/check-integration.sh` feeds them to a real sway:
//! `bindings.conf` must pass `sway --validate` on its own, and every command in
//! `bindings-ipc.txt` must succeed, in order: the unbinds follow their placeholders.
//!
//! After changing the builders, regenerate the files with
//! `SWAY_THINGS_UPDATE_FIXTURES=1 cargo test -p sway-things-core`.

mod common;

use common::{Result, push_line};
use sway_things_core::bindings::ours::{self, GROUPS};
use sway_things_core::bindings::{Action, Line, Modifiers};
use sway_things_core::command::{SwayCmd, Target};

/// Items as `shortcuts.bindings` stores them: every option, quotes, `;`, `$` and criteria in
/// commands, keycodes, mouse buttons, a device and a layout group.
const ITEMS: &[&str] = &[
    "unbindsym --to-code Mod4+d",
    "unbindcode --locked 121",
    "bindsym $mod+Shift+Return exec 'notify-send \"a b; c\" $HOME'",
    "bindsym --locked --no-repeat --release Mod1+Ctrl+x [app_id=\"foot\"] focus; kill",
    "bindsym --inhibited $mod+Shift+e exec \"swaynag -m 'Exit sway?' -B 'Yes' 'swaymsg exit'\"",
    "bindcode --release Mod4+38 focus left, move left",
    "bindsym --whole-window $mod+button3 floating toggle",
    "bindsym --border --exclude-titlebar button2 kill",
    "bindsym --input-device=1:1:AT_Translated_Set_2_keyboard $mod+F1 workspace number 1",
    "bindsym $mod+Group2+a exec 'echo $$$x'",
    "bindsym --to-code $mod+Prior move container to workspace number 11",
];

fn lines() -> Result<Vec<SwayCmd>> {
    let mut commands = Vec::new();
    for item in ITEMS {
        let line = ours::parse_item(item)?;
        if line.unbind {
            let placeholder = Line {
                unbind: false,
                command: Some("nop".to_owned()),
                ..line.clone()
            };
            commands.push(SwayCmd::binding(&placeholder, Modifiers::SUPER, true)?);
        }
        commands.push(SwayCmd::binding(&line, Modifiers::SUPER, false)?);
    }
    // What the editor makes of a program to run.
    let run = Action::Run("printf '%s' \"$1\" | wl-copy".to_owned()).command()?;
    let line = ours::parse_item(&format!("bindsym $mod+c {run}"))?;
    commands.push(SwayCmd::binding(&line, Modifiers::ALT, false)?);
    for group in GROUPS {
        for line in group.lines() {
            commands.push(SwayCmd::binding(&line, Modifiers::SUPER, true)?);
        }
    }
    Ok(commands)
}

fn fixture(commands: &[SwayCmd], target: Target) -> Result<String> {
    let mut out = common::header("bindings_fixtures.rs", false);
    for command in commands {
        push_line(&mut out, &command.render(target)?);
    }
    Ok(out)
}

#[test]
fn fixtures_are_up_to_date() -> Result<()> {
    let commands = lines()?;
    common::check(&[
        ("bindings.conf", fixture(&commands, Target::Config)?),
        ("bindings-ipc.txt", fixture(&commands, Target::Ipc)?),
    ])
}
