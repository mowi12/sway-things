// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generates the `input` fixtures in `ci/integration/` from the input and seat builders and checks
//! that the committed files are up to date. `scripts/check-integration.sh` feeds them to a real sway:
//! `input.conf` (every option and value that works in a config file) must pass `sway --validate`,
//! and every command in `input-ipc.txt` (every one that works over IPC) must succeed.
//!
//! After changing the builders, regenerate the files with
//! `SWAY_THINGS_UPDATE_FIXTURES=1 cargo test -p sway-things-core`.

mod common;

use common::{Result, push_line};
use sway_things_core::command::input::{
    AccelProfile, ButtonMap, ClickMethod, DragLock, Input, InputSelector, InputType, MapTo,
    ScrollButton, ScrollMethod, SendEvents, ToolMode, XkbSwitch,
};
use sway_things_core::command::{Availability, SwayCmd, SwayText, Target, seat};

/// A device identifier with characters that need quoting, including the `$mod` variable that
/// `ci/integration/sway.conf` defines.
const DEVICE: &str = r#"1:2:Fake"Mouse'$mod;x"#;

fn pointer_commands(input: &Input) -> Result<Vec<SwayCmd>> {
    let mut commands = vec![
        input.pointer_accel(-1.0)?,
        input.pointer_accel(0.25)?,
        input.scroll_factor(1.5)?,
        input.natural_scroll(true),
        input.left_handed(false),
        input.middle_emulation(true),
        input.tap(true),
        input.drag(true),
        input.dwt(true),
        input.dwtp(false),
    ];
    for profile in [AccelProfile::Adaptive, AccelProfile::Flat] {
        commands.push(input.accel_profile(profile));
    }
    for method in [
        ScrollMethod::None,
        ScrollMethod::TwoFinger,
        ScrollMethod::Edge,
        ScrollMethod::OnButtonDown,
    ] {
        commands.push(input.scroll_method(method));
    }
    for button in [
        ScrollButton::Disable,
        ScrollButton::Left,
        ScrollButton::Middle,
        ScrollButton::Right,
        ScrollButton::Side,
        ScrollButton::Extra,
    ] {
        commands.push(input.scroll_button(button));
    }
    for map in [ButtonMap::Lrm, ButtonMap::Lmr] {
        commands.push(input.tap_button_map(map));
        commands.push(input.clickfinger_button_map(map));
    }
    for lock in [
        DragLock::Disabled,
        DragLock::Enabled,
        DragLock::EnabledSticky,
    ] {
        commands.push(input.drag_lock(lock));
    }
    for method in [
        ClickMethod::None,
        ClickMethod::ButtonAreas,
        ClickMethod::Clickfinger,
    ] {
        commands.push(input.click_method(method));
    }
    for events in [
        SendEvents::Disabled,
        SendEvents::DisabledOnExternalMouse,
        SendEvents::Enabled,
    ] {
        commands.push(input.events(events));
    }
    Ok(commands)
}

fn keyboard_commands(input: &Input) -> Result<Vec<SwayCmd>> {
    Ok(vec![
        input.xkb_layout(&["us", "de"])?,
        input.xkb_variant(&["", "nodeadkeys"])?,
        input.xkb_options(&["grp:alt_shift_toggle", "caps:escape"])?,
        input.xkb_model("pc105")?,
        input.xkb_variant::<&str>(&[])?,
        input.xkb_options::<&str>(&[])?,
        input.xkb_model("")?,
        input.xkb_numlock(true),
        input.xkb_capslock(false),
        input.repeat_delay(300),
        input.repeat_rate(30),
        input.repeat_rate(1),
        input.xkb_switch_layout(XkbSwitch::Next),
        input.xkb_switch_layout(XkbSwitch::Prev),
        input.xkb_switch_layout(XkbSwitch::Index(0)),
    ])
}

fn all_commands() -> Result<Vec<SwayCmd>> {
    let touchpad = Input::new(InputSelector::Type(InputType::Touchpad));
    let keyboard = Input::new(InputSelector::Type(InputType::Keyboard));
    let device = Input::new(InputSelector::device(DEVICE)?);
    let all = Input::new(InputSelector::All);
    let mut commands = pointer_commands(&touchpad)?;
    commands.extend(keyboard_commands(&keyboard)?);
    commands.push(device.tap(true));
    commands.push(device.xkb_layout(&["us"])?);
    commands.push(all.repeat_rate(25));
    for input_type in [
        InputType::Pointer,
        InputType::Touch,
        InputType::TabletTool,
        InputType::TabletPad,
        InputType::Switch,
    ] {
        commands.push(Input::new(InputSelector::Type(input_type)).events(SendEvents::Enabled));
    }
    let tablet = Input::new(InputSelector::Type(InputType::TabletTool));
    let touch = Input::new(InputSelector::Type(InputType::Touch));
    // An output name with characters that need quoting.
    let output = MapTo::Output(SwayText::new(r#"Fake "Display" $mod"#)?);
    commands.extend([
        tablet.map_to_output(&output),
        tablet.map_to_output(&MapTo::Desktop),
        touch.map_to_output(&MapTo::Output(SwayText::new("eDP-1")?)),
        touch.map_to_output(&MapTo::Desktop),
        tablet.tool_mode(ToolMode::Relative),
        tablet.tool_mode(ToolMode::Absolute),
    ]);
    // A theme name with characters that need quoting.
    commands.extend([
        seat::xcursor_theme(&SwayText::new(r#"Fake "Cursors" $mod"#)?, 48)?,
        seat::xcursor_theme(&SwayText::new("default")?, 24)?,
        seat::hide_cursor(2500)?,
        seat::hide_cursor(0)?,
        seat::hide_cursor_when_typing(true),
        seat::hide_cursor_when_typing(false),
    ]);
    Ok(commands)
}

fn fixture(commands: &[SwayCmd], target: Target) -> Result<String> {
    let unavailable = match target {
        Target::Ipc => Availability::ConfigOnly,
        Target::Config => Availability::RuntimeOnly,
    };
    let mut out = common::header("input_fixtures.rs", false);
    for cmd in commands
        .iter()
        .filter(|cmd| cmd.availability() != unavailable)
    {
        push_line(&mut out, &cmd.render(target)?);
    }
    Ok(out)
}

#[test]
fn fixtures_are_up_to_date() -> Result<()> {
    let commands = all_commands()?;
    common::check(&[
        ("input.conf", fixture(&commands, Target::Config)?),
        ("input-ipc.txt", fixture(&commands, Target::Ipc)?),
    ])
}
