// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generates the window behavior fixtures in `ci/integration/` from the builders and checks that
//! the committed files are up to date. `scripts/check-integration.sh` feeds them to a real sway:
//! `behavior.conf` must pass `sway --validate` on its own, and every command in
//! `behavior-ipc.txt` must succeed.
//!
//! `inhibit_idle` on its own needs an open window, which the headless sway doesn't have, so only the
//! `for_window` rule is included.
//!
//! After changing the builders, regenerate the files with
//! `SWAY_THINGS_UPDATE_FIXTURES=1 cargo test -p sway-things-core`.

mod common;

use std::num::NonZeroU16;

use common::{Result, push_line};
use sway_things_core::command::behavior::{
    self, FloatingModifier, FocusFollowsMouse, FocusOnWindowActivation, FocusWrapping, InhibitIdle,
    Modifier, MouseWarping, Orientation, PopupDuringFullscreen, SizeLimit, WorkspaceLayout,
    Xwayland,
};
use sway_things_core::command::{Availability, Criteria, CriteriaKey, SwayCmd, Target};

fn focus_commands() -> Vec<SwayCmd> {
    let mut commands = Vec::new();
    for mode in [
        FocusFollowsMouse::Yes,
        FocusFollowsMouse::Always,
        FocusFollowsMouse::No,
    ] {
        commands.push(behavior::focus_follows_mouse(mode));
    }
    for mode in [
        MouseWarping::Container,
        MouseWarping::None,
        MouseWarping::Output,
    ] {
        commands.push(behavior::mouse_warping(mode));
    }
    for mode in [
        FocusWrapping::No,
        FocusWrapping::Force,
        FocusWrapping::Workspace,
        FocusWrapping::Yes,
    ] {
        commands.push(behavior::focus_wrapping(mode));
    }
    for mode in [
        FocusOnWindowActivation::Smart,
        FocusOnWindowActivation::Focus,
        FocusOnWindowActivation::None,
        FocusOnWindowActivation::Urgent,
    ] {
        commands.push(behavior::focus_on_window_activation(mode));
    }
    commands
}

fn layout_and_floating_commands() -> Result<Vec<SwayCmd>> {
    let px = |value: u16| {
        NonZeroU16::new(value)
            .map(SizeLimit::Pixels)
            .ok_or("zero size")
    };
    let mut commands = Vec::new();
    for layout in [
        WorkspaceLayout::Stacking,
        WorkspaceLayout::Tabbed,
        WorkspaceLayout::Default,
    ] {
        commands.push(behavior::workspace_layout(layout));
    }
    for orientation in [
        Orientation::Horizontal,
        Orientation::Vertical,
        Orientation::Auto,
    ] {
        commands.push(behavior::default_orientation(orientation));
    }
    commands.push(behavior::floating_modifier(FloatingModifier::None));
    commands.push(behavior::floating_modifier(FloatingModifier::Inverse(
        Modifier::Alt,
    )));
    for modifier in [
        Modifier::Shift,
        Modifier::Lock,
        Modifier::Ctrl,
        Modifier::Mod2,
        Modifier::Mod3,
        Modifier::Mod5,
        Modifier::Super,
    ] {
        commands.push(behavior::floating_modifier(FloatingModifier::Normal(
            modifier,
        )));
    }
    commands.push(behavior::floating_minimum_size(
        SizeLimit::NoLimit,
        SizeLimit::Automatic,
    ));
    commands.push(behavior::floating_minimum_size(px(75)?, px(50)?));
    commands.push(behavior::floating_maximum_size(
        SizeLimit::NoLimit,
        SizeLimit::NoLimit,
    ));
    commands.push(behavior::floating_maximum_size(px(1920)?, px(1080)?));
    commands.push(behavior::floating_maximum_size(
        SizeLimit::Automatic,
        SizeLimit::Automatic,
    ));
    Ok(commands)
}

fn other_commands() -> Result<Vec<SwayCmd>> {
    let mut commands = vec![
        behavior::tiling_drag(false),
        behavior::tiling_drag(true),
        behavior::tiling_drag_threshold(9),
        behavior::workspace_auto_back_and_forth(true),
        behavior::workspace_auto_back_and_forth(false),
    ];
    for mode in [
        PopupDuringFullscreen::Ignore,
        PopupDuringFullscreen::LeaveFullscreen,
        PopupDuringFullscreen::Smart,
    ] {
        commands.push(behavior::popup_during_fullscreen(mode));
    }
    // sway.conf includes this file at startup, so it ends with `disable`: the containers have no
    // Xwayland to start.
    for mode in [Xwayland::Enable, Xwayland::Force, Xwayland::Disable] {
        commands.push(behavior::xwayland(mode));
    }
    let criteria = Criteria::new().exact(CriteriaKey::AppId, "mpv");
    commands.push(SwayCmd::for_window(
        criteria,
        behavior::inhibit_idle(InhibitIdle::Fullscreen),
    )?);
    Ok(commands)
}

fn fixture(commands: &[SwayCmd], target: Target) -> Result<String> {
    let unavailable = match target {
        Target::Ipc => Availability::ConfigOnly,
        Target::Config => Availability::RuntimeOnly,
    };
    let mut out = common::header("behavior_fixtures.rs", false);
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
    let mut commands = focus_commands();
    commands.extend(layout_and_floating_commands()?);
    commands.extend(other_commands()?);
    common::check(&[
        ("behavior.conf", fixture(&commands, Target::Config)?),
        ("behavior-ipc.txt", fixture(&commands, Target::Ipc)?),
    ])
}
