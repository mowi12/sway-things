// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{
    AccelProfile, ButtonMap, ClickMethod, DragLock, Input, InputError, InputSelector, InputType,
    ScrollButton, ScrollMethod, SendEvents, XkbSwitch,
};
use crate::command::{CommandError, Criteria, CriteriaKey, OutOfRange, SwayCmd, Target, TextError};
use crate::version::SwayVersion;

fn touchpad() -> Input {
    Input::new(InputSelector::Type(InputType::Touchpad))
}

fn keyboard() -> Input {
    Input::new(InputSelector::Type(InputType::Keyboard))
}

/// Renders for both targets and checks that they agree (no text that needs target-specific
/// quoting).
fn rendered(cmd: &SwayCmd) -> String {
    let config = cmd.render(Target::Config).unwrap();
    assert_eq!(cmd.render(Target::Ipc).unwrap(), config);
    config
}

#[test]
fn every_pointer_option_renders() {
    let t = touchpad();
    let cases = [
        (t.pointer_accel(-1.0).unwrap(), "pointer_accel -1"),
        (t.pointer_accel(0.25).unwrap(), "pointer_accel 0.25"),
        (t.pointer_accel(1.0).unwrap(), "pointer_accel 1"),
        (
            t.accel_profile(AccelProfile::Adaptive),
            "accel_profile adaptive",
        ),
        (t.accel_profile(AccelProfile::Flat), "accel_profile flat"),
        (t.natural_scroll(true), "natural_scroll enabled"),
        (t.natural_scroll(false), "natural_scroll disabled"),
        (t.scroll_factor(0.0).unwrap(), "scroll_factor 0"),
        (t.scroll_factor(1.5).unwrap(), "scroll_factor 1.5"),
        (t.left_handed(true), "left_handed enabled"),
        (t.middle_emulation(false), "middle_emulation disabled"),
        (t.scroll_method(ScrollMethod::None), "scroll_method none"),
        (
            t.scroll_method(ScrollMethod::TwoFinger),
            "scroll_method two_finger",
        ),
        (t.scroll_method(ScrollMethod::Edge), "scroll_method edge"),
        (
            t.scroll_method(ScrollMethod::OnButtonDown),
            "scroll_method on_button_down",
        ),
        (
            t.scroll_button(ScrollButton::Disable),
            "scroll_button disable",
        ),
        (t.scroll_button(ScrollButton::Left), "scroll_button button1"),
        (
            t.scroll_button(ScrollButton::Middle),
            "scroll_button button2",
        ),
        (
            t.scroll_button(ScrollButton::Right),
            "scroll_button button3",
        ),
        (t.scroll_button(ScrollButton::Side), "scroll_button button8"),
        (
            t.scroll_button(ScrollButton::Extra),
            "scroll_button button9",
        ),
        (t.tap(true), "tap enabled"),
        (t.tap_button_map(ButtonMap::Lrm), "tap_button_map lrm"),
        (t.tap_button_map(ButtonMap::Lmr), "tap_button_map lmr"),
        (t.drag(false), "drag disabled"),
        (t.drag_lock(DragLock::Disabled), "drag_lock disabled"),
        (t.drag_lock(DragLock::Enabled), "drag_lock enabled"),
        (
            t.drag_lock(DragLock::EnabledSticky),
            "drag_lock enabled_sticky",
        ),
        (t.dwt(true), "dwt enabled"),
        (t.dwtp(false), "dwtp disabled"),
        (t.click_method(ClickMethod::None), "click_method none"),
        (
            t.click_method(ClickMethod::ButtonAreas),
            "click_method button_areas",
        ),
        (
            t.click_method(ClickMethod::Clickfinger),
            "click_method clickfinger",
        ),
        (
            t.clickfinger_button_map(ButtonMap::Lmr),
            "clickfinger_button_map lmr",
        ),
        (t.events(SendEvents::Enabled), "events enabled"),
        (t.events(SendEvents::Disabled), "events disabled"),
        (
            t.events(SendEvents::DisabledOnExternalMouse),
            "events disabled_on_external_mouse",
        ),
    ];
    for (cmd, expected) in cases {
        assert_eq!(rendered(&cmd), format!("input type:touchpad {expected}"));
    }
}

#[test]
fn every_keyboard_option_renders() {
    let k = keyboard();
    let cases = [
        (
            k.xkb_layout(&["us", "de"]).unwrap(),
            r#"xkb_layout "us,de""#,
        ),
        (
            k.xkb_variant(&["", "nodeadkeys"]).unwrap(),
            r#"xkb_variant ",nodeadkeys""#,
        ),
        (
            k.xkb_options(&["grp:alt_shift_toggle", "caps:escape"])
                .unwrap(),
            r#"xkb_options "grp:alt_shift_toggle,caps:escape""#,
        ),
        (k.xkb_model("pc105").unwrap(), r#"xkb_model "pc105""#),
        (k.xkb_model("").unwrap(), r#"xkb_model """#),
        (k.xkb_variant(&[""]).unwrap(), r#"xkb_variant """#),
        (k.xkb_variant::<&str>(&[]).unwrap(), r#"xkb_variant """#),
        (k.xkb_variant(&["", ""]).unwrap(), r#"xkb_variant ",""#),
        (k.xkb_options::<&str>(&[]).unwrap(), r#"xkb_options """#),
        (k.repeat_delay(300), "repeat_delay 300"),
        (k.repeat_rate(0), "repeat_rate 0"),
    ];
    for (cmd, expected) in cases {
        assert_eq!(rendered(&cmd), format!("input type:keyboard {expected}"));
    }
}

#[test]
fn config_only_and_runtime_only_options() {
    let k = keyboard();
    for (cmd, expected) in [
        (
            k.xkb_numlock(true),
            "input type:keyboard xkb_numlock enabled",
        ),
        (
            k.xkb_capslock(false),
            "input type:keyboard xkb_capslock disabled",
        ),
    ] {
        assert_eq!(cmd.render(Target::Config).unwrap(), expected);
        assert_eq!(
            cmd.render(Target::Ipc),
            Err(CommandError::Unavailable {
                target: Target::Ipc
            })
        );
    }
    for (switch, expected) in [
        (XkbSwitch::Next, "next"),
        (XkbSwitch::Prev, "prev"),
        (XkbSwitch::Index(2), "2"),
    ] {
        let cmd = k.xkb_switch_layout(switch);
        assert_eq!(
            cmd.render(Target::Ipc).unwrap(),
            format!("input type:keyboard xkb_switch_layout {expected}")
        );
        assert_eq!(
            cmd.render(Target::Config),
            Err(CommandError::Unavailable {
                target: Target::Config
            })
        );
    }
}

#[test]
fn selectors_render() {
    let all = Input::new(InputSelector::All);
    assert_eq!(rendered(&all.dwt(true)), "input * dwt enabled");
    let types = [
        (InputType::Touchpad, "touchpad"),
        (InputType::Pointer, "pointer"),
        (InputType::Keyboard, "keyboard"),
        (InputType::Touch, "touch"),
        (InputType::TabletTool, "tablet_tool"),
        (InputType::TabletPad, "tablet_pad"),
        (InputType::Switch, "switch"),
    ];
    for (input_type, name) in types {
        let cmd = Input::new(InputSelector::Type(input_type)).events(SendEvents::Enabled);
        assert_eq!(rendered(&cmd), format!("input type:{name} events enabled"));
    }
    let device = Input::new(InputSelector::device(r#"1:2:a"b$c"#).unwrap());
    let cmd = device.tap(true);
    assert_eq!(
        cmd.render(Target::Ipc).unwrap(),
        r#"input '1:2:a"b$$c' tap enabled"#
    );
    assert_eq!(
        cmd.render(Target::Config).unwrap(),
        r#"input "1:2:a\"b$$c" tap enabled"#
    );
}

#[test]
fn device_identifiers_are_checked() {
    for valid in [
        "1133:16500:Logitech_G305",
        "0:0:",
        "1:1:ELAN1300:00_04F3:3059_Touchpad",
    ] {
        assert!(InputSelector::device(valid).is_ok(), "{valid:?}");
    }
    for invalid in [
        "*",
        "type:keyboard",
        "1133:Logitech",
        ":1:x",
        "1:x:y",
        "1:2:with space",
    ] {
        assert_eq!(
            InputSelector::device(invalid),
            Err(InputError::NotADeviceIdentifier),
            "{invalid:?}"
        );
    }
    assert_eq!(
        InputSelector::device(r"1:2:back\slash"),
        Err(InputError::Text(TextError::Backslash { index: 8 }))
    );
}

#[test]
fn numbers_are_range_checked() {
    let t = touchpad();
    for speed in [-1.01, 1.01, f64::NAN, f64::INFINITY] {
        assert_eq!(
            t.pointer_accel(speed),
            Err(InputError::OutOfRange(OutOfRange {
                option: "pointer_accel",
                min: -1.0,
                max: 1.0
            })),
            "{speed}"
        );
    }
    for factor in [-0.1, f64::NAN, 1e39] {
        assert!(
            matches!(
                t.scroll_factor(factor),
                Err(InputError::OutOfRange(OutOfRange {
                    option: "scroll_factor",
                    ..
                }))
            ),
            "{factor}"
        );
    }
}

#[test]
fn xkb_lists_are_checked() {
    let k = keyboard();
    let none: [&str; 0] = [];
    assert_eq!(
        k.xkb_layout(&none),
        Err(InputError::EmptyList {
            option: "xkb_layout"
        })
    );
    assert_eq!(
        k.xkb_options(&["caps:escape", ""]),
        Err(InputError::EmptyListElement {
            option: "xkb_options",
            index: 1
        })
    );
    assert_eq!(
        k.xkb_layout(&["us", ""]),
        Err(InputError::EmptyListElement {
            option: "xkb_layout",
            index: 1
        })
    );
    assert_eq!(
        k.xkb_options(&["caps:escape,ctrl:nocaps"]),
        Err(InputError::CommaInListElement {
            option: "xkb_options",
            index: 0
        })
    );
    assert!(matches!(
        k.xkb_layout(&["us\n"]),
        Err(InputError::Text(TextError::ControlCharacter { .. }))
    ));
    assert!(matches!(
        k.xkb_model("pc\\105"),
        Err(InputError::Text(TextError::Backslash { .. }))
    ));
    let owned = vec![String::from("us"), String::from("de")];
    assert!(k.xkb_layout(&owned).is_ok());
}

#[test]
fn sticky_drag_lock_needs_sway_1_11() {
    let t = touchpad();
    let sticky = t.drag_lock(DragLock::EnabledSticky);
    assert_eq!(sticky.min_sway(), SwayVersion::new(1, 11, 0));
    assert_eq!(
        t.drag_lock(DragLock::Enabled).min_sway(),
        SwayVersion::OLDEST_SUPPORTED
    );
    let criteria = Criteria::new().exact(CriteriaKey::AppId, "x");
    let wrapped = SwayCmd::for_window(criteria, sticky).unwrap();
    assert_eq!(wrapped.min_sway(), SwayVersion::new(1, 11, 0));
    let lowered = wrapped.requires_sway(SwayVersion::OLDEST_SUPPORTED);
    assert_eq!(lowered.min_sway(), SwayVersion::new(1, 11, 0));
}

#[test]
fn input_types_have_sways_names() {
    for kind in InputType::ALL {
        assert_eq!(InputType::from_name(kind.name()), Some(kind));
        let selector = Input::new(InputSelector::Type(kind)).tap(true);
        assert_eq!(
            selector.render(Target::Ipc).unwrap(),
            format!("input type:{} tap enabled", kind.name())
        );
    }
    assert_eq!(InputType::from_name("touchpad"), Some(InputType::Touchpad));
    assert_eq!(InputType::from_name("unknown"), None);
    assert_eq!(InputType::from_name("Keyboard"), None);
}
