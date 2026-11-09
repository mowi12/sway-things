// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{
    BorderStyle, ClientClass, Gaps, GapsScope, HideEdgeBorders, SmartBorders, SmartGaps,
    TitleAlign, TitleColors, WindowColors, WindowError, Workspace,
};
use crate::command::{
    Color, CommandError, Criteria, CriteriaKey, OutOfRange, SwayCmd, Target, TextError,
};

/// Renders for both targets and checks that they agree.
fn rendered(cmd: &SwayCmd) -> String {
    let config = cmd.render(Target::Config).unwrap();
    assert_eq!(cmd.render(Target::Ipc).unwrap(), config);
    config
}

const COLORS: WindowColors = WindowColors {
    border: Color::rgb(0x4c, 0x78, 0x99),
    background: Color::rgb(0x28, 0x55, 0x77),
    text: Color::rgb(0xff, 0xff, 0xff),
    indicator: Color::rgb(0x2e, 0x9e, 0xf4),
    child_border: Color::rgba(0x28, 0x55, 0x77, 0xcc),
};

#[test]
fn client_colors_render() {
    for (class, name) in [
        (ClientClass::Focused, "focused"),
        (ClientClass::FocusedInactive, "focused_inactive"),
        (ClientClass::Unfocused, "unfocused"),
        (ClientClass::Urgent, "urgent"),
    ] {
        assert_eq!(
            rendered(&super::client(class, &COLORS)),
            format!("client.{name} #4c7899 #285577 #ffffff #2e9ef4 #285577cc")
        );
    }
    let title = TitleColors {
        border: COLORS.border,
        background: COLORS.background,
        text: COLORS.text,
    };
    assert_eq!(
        rendered(&super::client_focused_tab_title(&title)),
        "client.focused_tab_title #4c7899 #285577 #ffffff"
    );
}

#[test]
fn borders_render() {
    let cases = [
        (
            super::default_border(BorderStyle::None),
            "default_border none",
        ),
        (
            super::default_border(BorderStyle::Normal(2)),
            "default_border normal 2",
        ),
        (
            super::default_floating_border(BorderStyle::Pixel(3)),
            "default_floating_border pixel 3",
        ),
        (
            super::hide_edge_borders(HideEdgeBorders::None, false),
            "hide_edge_borders none",
        ),
        (
            super::hide_edge_borders(HideEdgeBorders::Vertical, false),
            "hide_edge_borders vertical",
        ),
        (
            super::hide_edge_borders(HideEdgeBorders::Horizontal, false),
            "hide_edge_borders horizontal",
        ),
        (
            super::hide_edge_borders(HideEdgeBorders::Both, true),
            "hide_edge_borders --i3 both",
        ),
        (
            super::hide_edge_borders(HideEdgeBorders::Smart, false),
            "hide_edge_borders smart",
        ),
        (
            super::hide_edge_borders(HideEdgeBorders::SmartNoGaps, true),
            "hide_edge_borders --i3 smart_no_gaps",
        ),
        (super::smart_borders(SmartBorders::On), "smart_borders on"),
        (
            super::smart_borders(SmartBorders::NoGaps),
            "smart_borders no_gaps",
        ),
        (super::smart_borders(SmartBorders::Off), "smart_borders off"),
    ];
    for (cmd, expected) in cases {
        assert_eq!(rendered(&cmd), expected);
    }
}

#[test]
fn gaps_render() {
    let sides = [
        (Gaps::Inner, "inner"),
        (Gaps::Outer, "outer"),
        (Gaps::Horizontal, "horizontal"),
        (Gaps::Vertical, "vertical"),
        (Gaps::Top, "top"),
        (Gaps::Right, "right"),
        (Gaps::Bottom, "bottom"),
        (Gaps::Left, "left"),
    ];
    for (which, name) in sides {
        assert_eq!(
            rendered(&super::gaps(which, 4).unwrap()),
            format!("gaps {name} 4")
        );
    }
    assert_eq!(
        rendered(&super::gaps(Gaps::Outer, -4).unwrap()),
        "gaps outer -4"
    );
    let all = super::gaps_runtime(Gaps::Inner, GapsScope::All, 8).unwrap();
    assert_eq!(all.render(Target::Ipc).unwrap(), "gaps inner all set 8");
    assert_eq!(
        all.render(Target::Config),
        Err(CommandError::Unavailable {
            target: Target::Config
        })
    );
    let current = super::gaps_runtime(Gaps::Left, GapsScope::Current, -2).unwrap();
    assert_eq!(
        current.render(Target::Ipc).unwrap(),
        "gaps left current set -2"
    );
    for (cmd, expected) in [
        (super::smart_gaps(SmartGaps::On), "smart_gaps on"),
        (super::smart_gaps(SmartGaps::Off), "smart_gaps off"),
        (
            super::smart_gaps(SmartGaps::InverseOuter),
            "smart_gaps inverse_outer",
        ),
    ] {
        assert_eq!(rendered(&cmd), expected);
    }
}

#[test]
fn inner_gaps_cant_be_negative() {
    let expected = Err(WindowError::OutOfRange(OutOfRange {
        option: "gaps inner",
        min: 0.0,
        max: f64::from(i16::MAX),
    }));
    assert_eq!(super::gaps(Gaps::Inner, -1), expected);
    assert_eq!(
        super::gaps_runtime(Gaps::Inner, GapsScope::All, -1),
        expected
    );
    assert_eq!(Workspace::new("1").unwrap().gaps(Gaps::Inner, -1), expected);
}

#[test]
fn workspace_gaps_render() {
    let mail = Workspace::new(r#"3: "mail" $1"#).unwrap();
    let cmd = mail.gaps(Gaps::Outer, -2).unwrap();
    assert_eq!(
        cmd.render(Target::Ipc).unwrap(),
        r#"workspace '3: "mail" $$1' gaps outer -2"#
    );
    assert_eq!(
        cmd.render(Target::Config).unwrap(),
        r#"workspace "3: \"mail\" $$1" gaps outer -2"#
    );
    for reserved in ["output", "GAPS"] {
        assert_eq!(
            Workspace::new(reserved),
            Err(WindowError::ReservedWorkspaceName)
        );
    }
    assert_eq!(Workspace::new(""), Err(WindowError::Text(TextError::Empty)));
}

#[test]
fn title_bar_renders() {
    let cases = [
        (
            super::font("DejaVu Sans Mono 10", false).unwrap(),
            r#"font "DejaVu Sans Mono 10""#,
        ),
        (
            super::font("monospace 10", true).unwrap(),
            r#"font "pango:monospace 10""#,
        ),
        (super::title_align(TitleAlign::Left), "title_align left"),
        (super::title_align(TitleAlign::Center), "title_align center"),
        (super::title_align(TitleAlign::Right), "title_align right"),
        (super::titlebar_padding(6, 4), "titlebar_padding 6 4"),
        (
            super::titlebar_border_thickness(1),
            "titlebar_border_thickness 1",
        ),
    ];
    for (cmd, expected) in cases {
        assert_eq!(rendered(&cmd), expected);
    }
    assert_eq!(
        super::font("pango:monospace 10", false),
        Err(WindowError::PangoPrefix)
    );
    assert_eq!(
        super::font("", true),
        Err(WindowError::Text(TextError::Empty))
    );
}

#[test]
fn opacity_is_checked_and_wrapped() {
    let criteria = || Criteria::new().exact(CriteriaKey::AppId, "foot");
    let opacity = super::opacity(0.8).unwrap();
    assert_eq!(opacity.render(Target::Ipc).unwrap(), "opacity 0.8");
    assert_eq!(
        opacity.render(Target::Config),
        Err(CommandError::Unavailable {
            target: Target::Config
        })
    );
    let later = SwayCmd::for_window(criteria(), opacity.clone()).unwrap();
    assert_eq!(
        later.render(Target::Ipc).unwrap(),
        r#"for_window [app_id="^foot\z"] opacity 0.8"#
    );
    assert_eq!(
        later.render(Target::Config).unwrap(),
        r#"for_window [app_id="^foot\\z"] opacity 0.8"#
    );
    let now = SwayCmd::with_criteria(criteria(), opacity).unwrap();
    assert_eq!(
        now.render(Target::Ipc).unwrap(),
        r#"[app_id="^foot\z"] opacity 0.8"#
    );
    for value in [-0.1, 1.1, f64::NAN] {
        assert!(
            matches!(super::opacity(value), Err(WindowError::OutOfRange(_))),
            "{value}"
        );
    }
}
