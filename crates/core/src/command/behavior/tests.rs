// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::num::NonZeroU16;

use super::{
    FloatingModifier, FocusFollowsMouse, FocusOnWindowActivation, FocusWrapping, InhibitIdle,
    Modifier, MouseWarping, Orientation, PopupDuringFullscreen, SizeLimit, WorkspaceLayout,
    Xwayland,
};
use crate::command::{CommandError, Criteria, CriteriaKey, SwayCmd, Target};

/// Renders for both targets and checks that they agree.
fn rendered(cmd: &SwayCmd) -> String {
    let config = cmd.render(Target::Config).unwrap();
    assert_eq!(cmd.render(Target::Ipc).unwrap(), config);
    config
}

#[test]
fn focus_options_render() {
    let cases = [
        (
            super::focus_follows_mouse(FocusFollowsMouse::Yes),
            "focus_follows_mouse yes",
        ),
        (
            super::focus_follows_mouse(FocusFollowsMouse::No),
            "focus_follows_mouse no",
        ),
        (
            super::focus_follows_mouse(FocusFollowsMouse::Always),
            "focus_follows_mouse always",
        ),
        (
            super::mouse_warping(MouseWarping::Output),
            "mouse_warping output",
        ),
        (
            super::mouse_warping(MouseWarping::Container),
            "mouse_warping container",
        ),
        (
            super::mouse_warping(MouseWarping::None),
            "mouse_warping none",
        ),
        (
            super::focus_wrapping(FocusWrapping::Yes),
            "focus_wrapping yes",
        ),
        (
            super::focus_wrapping(FocusWrapping::No),
            "focus_wrapping no",
        ),
        (
            super::focus_wrapping(FocusWrapping::Force),
            "focus_wrapping force",
        ),
        (
            super::focus_wrapping(FocusWrapping::Workspace),
            "focus_wrapping workspace",
        ),
        (
            super::focus_on_window_activation(FocusOnWindowActivation::Smart),
            "focus_on_window_activation smart",
        ),
        (
            super::focus_on_window_activation(FocusOnWindowActivation::Urgent),
            "focus_on_window_activation urgent",
        ),
        (
            super::focus_on_window_activation(FocusOnWindowActivation::Focus),
            "focus_on_window_activation focus",
        ),
        (
            super::focus_on_window_activation(FocusOnWindowActivation::None),
            "focus_on_window_activation none",
        ),
    ];
    for (cmd, expected) in cases {
        assert_eq!(rendered(&cmd), expected);
    }
}

#[test]
fn floating_and_dragging_render() {
    let px = |value: u16| SizeLimit::Pixels(NonZeroU16::new(value).unwrap());
    let cases = [
        (
            super::floating_modifier(FloatingModifier::None),
            "floating_modifier none",
        ),
        (
            super::floating_modifier(FloatingModifier::Inverse(Modifier::Alt)),
            "floating_modifier Mod1 inverse",
        ),
        (
            super::floating_minimum_size(px(75), px(50)),
            "floating_minimum_size 75 x 50",
        ),
        (
            super::floating_minimum_size(SizeLimit::NoLimit, SizeLimit::Automatic),
            "floating_minimum_size -1 x 0",
        ),
        (
            super::floating_maximum_size(SizeLimit::NoLimit, SizeLimit::NoLimit),
            "floating_maximum_size -1 x -1",
        ),
        (super::tiling_drag(true), "tiling_drag enable"),
        (super::tiling_drag(false), "tiling_drag disable"),
        (super::tiling_drag_threshold(9), "tiling_drag_threshold 9"),
        (
            super::popup_during_fullscreen(PopupDuringFullscreen::Smart),
            "popup_during_fullscreen smart",
        ),
        (
            super::popup_during_fullscreen(PopupDuringFullscreen::Ignore),
            "popup_during_fullscreen ignore",
        ),
        (
            super::popup_during_fullscreen(PopupDuringFullscreen::LeaveFullscreen),
            "popup_during_fullscreen leave_fullscreen",
        ),
        (
            super::workspace_auto_back_and_forth(true),
            "workspace_auto_back_and_forth yes",
        ),
    ];
    for (cmd, expected) in cases {
        assert_eq!(rendered(&cmd), expected);
    }
    let modifiers = [
        (Modifier::Shift, "Shift"),
        (Modifier::Lock, "Lock"),
        (Modifier::Ctrl, "Ctrl"),
        (Modifier::Alt, "Mod1"),
        (Modifier::Mod2, "Mod2"),
        (Modifier::Mod3, "Mod3"),
        (Modifier::Super, "Mod4"),
        (Modifier::Mod5, "Mod5"),
    ];
    for (modifier, name) in modifiers {
        let cmd = super::floating_modifier(FloatingModifier::Normal(modifier));
        assert_eq!(rendered(&cmd), format!("floating_modifier {name} normal"));
    }
}

#[test]
fn config_only_options() {
    let cases = [
        (
            super::workspace_layout(WorkspaceLayout::Default),
            "workspace_layout default",
        ),
        (
            super::workspace_layout(WorkspaceLayout::Stacking),
            "workspace_layout stacking",
        ),
        (
            super::workspace_layout(WorkspaceLayout::Tabbed),
            "workspace_layout tabbed",
        ),
        (
            super::default_orientation(Orientation::Horizontal),
            "default_orientation horizontal",
        ),
        (
            super::default_orientation(Orientation::Vertical),
            "default_orientation vertical",
        ),
        (
            super::default_orientation(Orientation::Auto),
            "default_orientation auto",
        ),
        (super::xwayland(Xwayland::Enable), "xwayland enable"),
        (super::xwayland(Xwayland::Disable), "xwayland disable"),
        (super::xwayland(Xwayland::Force), "xwayland force"),
    ];
    for (cmd, expected) in cases {
        assert_eq!(cmd.render(Target::Config).unwrap(), expected);
        assert_eq!(
            cmd.render(Target::Ipc),
            Err(CommandError::Unavailable {
                target: Target::Ipc
            })
        );
    }
}

#[test]
fn inhibit_idle_is_wrapped() {
    for (mode, name) in [
        (InhibitIdle::Focus, "focus"),
        (InhibitIdle::Fullscreen, "fullscreen"),
        (InhibitIdle::Open, "open"),
        (InhibitIdle::Visible, "visible"),
        (InhibitIdle::None, "none"),
    ] {
        let cmd = super::inhibit_idle(mode);
        assert_eq!(
            cmd.render(Target::Ipc).unwrap(),
            format!("inhibit_idle {name}")
        );
        assert_eq!(
            cmd.render(Target::Config),
            Err(CommandError::Unavailable {
                target: Target::Config
            })
        );
    }
    let criteria = Criteria::new().exact(CriteriaKey::AppId, "mpv");
    let rule = SwayCmd::for_window(criteria, super::inhibit_idle(InhibitIdle::Fullscreen)).unwrap();
    assert_eq!(
        rule.render(Target::Config).unwrap(),
        r#"for_window [app_id="^mpv\\z"] inhibit_idle fullscreen"#
    );
    assert_eq!(
        rule.render(Target::Ipc).unwrap(),
        r#"for_window [app_id="^mpv\z"] inhibit_idle fullscreen"#
    );
}
