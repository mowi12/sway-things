// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Mouse & Touchpad and Keyboard pages (`man 5 sway-input`).
//!
//! The defaults of libinput options depend on the device (a touchpad without buttons taps by
//! default); the values here are the common ones, and the subtitles of those that vary say so.
//! Pointers and touchpads share most options: [`common`] makes them for either type.

use super::read::{self, ReadError};
use super::{
    Apply, Choice, ConfigFile, Context, Directive, Page, RenderError, Scope, Setting, Value,
};
use crate::command::SwayCmd;
use crate::command::input::{
    AccelProfile, ButtonMap, ClickMethod, DragLock, InputType, ScrollButton, ScrollMethod,
    SendEvents,
};

const POINTER_EVENTS: &str = "input.pointer.events";
const TOUCHPAD_EVENTS: &str = "input.touchpad.events";
const TOUCHPAD_EVENTS_CHOICES: &[Choice] = &[
    Choice::new("enabled", "On"),
    Choice::new("disabled", "Off"),
    Choice::new(
        "disabled_on_external_mouse",
        "Off while a mouse is connected",
    ),
];
const KEYBOARD_EVENTS: &str = "input.keyboard.events";
const TAP: &str = "input.touchpad.tap";
const TAP_BUTTON_MAP: &str = "input.touchpad.tap_button_map";
const DRAG: &str = "input.touchpad.drag";
const DRAG_LOCK: &str = "input.touchpad.drag_lock";
const DWT: &str = "input.touchpad.dwt";
const DWTP: &str = "input.touchpad.dwtp";
const CLICK_METHOD: &str = "input.touchpad.click_method";
const CLICKFINGER_BUTTON_MAP: &str = "input.touchpad.clickfinger_button_map";

/// The key of an option that pointers and touchpads share, for each of the two types.
#[derive(Clone, Copy)]
struct Shared {
    pointer: &'static str,
    touchpad: &'static str,
}

impl Shared {
    /// The key for the devices of `input_type`.
    const fn of(self, input_type: InputType) -> &'static str {
        match input_type {
            InputType::Touchpad => self.touchpad,
            _ => self.pointer,
        }
    }

    /// The key of the setting `ctx` renders.
    fn at(self, ctx: &Context<'_>) -> &'static str {
        match ctx.scope {
            Scope::PerDevice(input_type) => self.of(input_type),
            _ => self.pointer,
        }
    }
}

const POINTER_ACCEL: Shared = Shared {
    pointer: "input.pointer.pointer_accel",
    touchpad: "input.touchpad.pointer_accel",
};
const ACCEL_PROFILE: Shared = Shared {
    pointer: "input.pointer.accel_profile",
    touchpad: "input.touchpad.accel_profile",
};
const NATURAL_SCROLL: Shared = Shared {
    pointer: "input.pointer.natural_scroll",
    touchpad: "input.touchpad.natural_scroll",
};
const SCROLL_FACTOR: Shared = Shared {
    pointer: "input.pointer.scroll_factor",
    touchpad: "input.touchpad.scroll_factor",
};
const SCROLL_METHOD: Shared = Shared {
    pointer: "input.pointer.scroll_method",
    touchpad: "input.touchpad.scroll_method",
};
const SCROLL_BUTTON: Shared = Shared {
    pointer: "input.pointer.scroll_button",
    touchpad: "input.touchpad.scroll_button",
};
const LEFT_HANDED: Shared = Shared {
    pointer: "input.pointer.left_handed",
    touchpad: "input.touchpad.left_handed",
};
const MIDDLE_EMULATION: Shared = Shared {
    pointer: "input.pointer.middle_emulation",
    touchpad: "input.touchpad.middle_emulation",
};

const ACCEL_PROFILES: &[Choice] = &[
    Choice::new("adaptive", "Adaptive"),
    Choice::new("flat", "Flat"),
];
/// Mice and trackpoints scroll with a held button or not at all; touchpads also with two
/// fingers or along the edge.
const POINTER_SCROLL_METHODS: &[Choice] = &[
    Choice::new("none", "None"),
    Choice::new("on_button_down", "While a button is held"),
];
const TOUCHPAD_SCROLL_METHODS: &[Choice] = &[
    Choice::new("two_finger", "Two fingers"),
    Choice::new("edge", "Along the edge"),
    Choice::new("on_button_down", "While a button is held"),
    Choice::new("none", "None"),
];
const SCROLL_BUTTONS: &[Choice] = &[
    Choice::new("button2", "Middle"),
    Choice::new("button3", "Right"),
    Choice::new("button1", "Left"),
    Choice::new("button8", "Back"),
    Choice::new("button9", "Forward"),
    Choice::new("disable", "None"),
];
const BUTTON_MAPS: &[Choice] = &[
    Choice::new("lrm", "Left, right, middle"),
    Choice::new("lmr", "Left, middle, right"),
];
const DRAG_LOCKS: &[Choice] = &[
    Choice::new("disabled", "Off"),
    Choice::new("enabled", "Until a short pause"),
    Choice::new("enabled_sticky", "Until the next tap"),
];
const CLICK_METHODS: &[Choice] = &[
    Choice::new("button_areas", "Areas at the bottom"),
    Choice::new("clickfinger", "Number of fingers"),
    Choice::new("none", "None"),
];
const XKB_LAYOUT: &str = "input.keyboard.xkb_layout";
const XKB_VARIANT: &str = "input.keyboard.xkb_variant";
const XKB_MODEL: &str = "input.keyboard.xkb_model";
const XKB_OPTIONS: &str = "input.keyboard.xkb_options";
const REPEAT_DELAY: &str = "input.keyboard.repeat_delay";
const REPEAT_RATE: &str = "input.keyboard.repeat_rate";
const XKB_NUMLOCK: &str = "input.keyboard.xkb_numlock";
const XKB_CAPSLOCK: &str = "input.keyboard.xkb_capslock";

/// sway's words for `events` in a config file (`toggle` only works at runtime).
fn read_events(args: &[String]) -> Result<SendEvents, ReadError> {
    let word = read::first(args)?;
    [
        ("enabled", SendEvents::Enabled),
        ("disabled", SendEvents::Disabled),
        (
            "disabled_on_external_mouse",
            SendEvents::DisabledOnExternalMouse,
        ),
    ]
    .into_iter()
    .find(|(name, _)| word.eq_ignore_ascii_case(name))
    .map(|(_, events)| events)
    .ok_or_else(|| {
        ReadError::Rejected(format!(
            "expected enabled, disabled or disabled_on_external_mouse, got {word}"
        ))
    })
}

/// A device switched on or off: on means `events enabled`.
fn on_off_events(ctx: &Context<'_>, key: &'static str) -> Result<Vec<SwayCmd>, RenderError> {
    let events = if ctx.bool(key)? {
        SendEvents::Enabled
    } else {
        SendEvents::Disabled
    };
    Ok(vec![ctx.input()?.events(events)])
}

/// An on/off `events` setting read from a line: only `enabled` is on. A touchpad mode on a
/// device of another type is one the setting can't show.
fn read_on_off_events(args: &[String]) -> Result<Value, ReadError> {
    match read_events(args)? {
        SendEvents::Enabled => Ok(Value::Bool(true)),
        SendEvents::Disabled => Ok(Value::Bool(false)),
        SendEvents::DisabledOnExternalMouse => Err(ReadError::Unsupported(
            "only touchpads can be off while a mouse is connected".to_owned(),
        )),
    }
}

/// An on/off option as sway reads it: `parse_boolean(…, true)`, so `toggle` is off.
fn read_switch(args: &[String]) -> Result<Value, ReadError> {
    Ok(Value::Bool(read::boolean(read::first(args)?, true)))
}

/// `pointer_accel`: sway rejects a speed outside [-1, 1].
fn read_accel(args: &[String]) -> Result<Value, ReadError> {
    let speed = read::float(read::first(args)?)?;
    if (-1.0..=1.0).contains(&speed) {
        Ok(Value::Decimal(speed))
    } else {
        Err(ReadError::Rejected(format!(
            "out of range [-1, 1]: {speed}"
        )))
    }
}

/// `scroll_factor`: sway rejects a negative one; the setting holds up to [`MAX_SCROLL_FACTOR`].
fn read_scroll_factor(args: &[String]) -> Result<Value, ReadError> {
    let factor = read::float(read::first(args)?)?;
    if factor < 0.0 {
        Err(ReadError::Rejected(format!(
            "the scroll factor can't be negative: {factor}"
        )))
    } else if factor > MAX_SCROLL_FACTOR {
        Err(ReadError::Unsupported(format!(
            "a scroll factor above {MAX_SCROLL_FACTOR}: {factor}"
        )))
    } else {
        Ok(Value::Decimal(factor))
    }
}

/// The largest scroll factor the setting holds; sway sets no limit.
const MAX_SCROLL_FACTOR: f64 = 10.0;

/// `scroll_button`: the buttons the setting offers. sway also takes other button names and event
/// codes, which the setting can't show.
fn read_scroll_button(args: &[String]) -> Result<Value, ReadError> {
    let word = read::first(args)?;
    if SCROLL_BUTTONS.iter().any(|choice| choice.id == word) {
        Ok(Value::Choice(word.to_owned()))
    } else {
        Err(ReadError::Unsupported(format!(
            "a scroll button the setting doesn't offer: {word}"
        )))
    }
}

/// `drag_lock`: `enabled_sticky` (sway 1.11), otherwise on or off as `parse_boolean` reads it.
fn read_drag_lock(args: &[String]) -> Result<Value, ReadError> {
    let word = read::first(args)?;
    let id = if word == "enabled_sticky" {
        "enabled_sticky"
    } else if read::boolean(word, true) {
        "enabled"
    } else {
        "disabled"
    };
    Ok(Value::Choice(id.to_owned()))
}

fn button_map(id: &str) -> ButtonMap {
    if id == "lmr" {
        ButtonMap::Lmr
    } else {
        ButtonMap::Lrm
    }
}

pub(super) fn settings() -> Vec<Setting> {
    let mut settings = mouse_touchpad();
    settings.extend(keyboard());
    settings
}

/// The settings of the Mouse & Touchpad page: each section starts with switching the devices on
/// and off, then the options both types share, then those of touchpads only.
fn mouse_touchpad() -> Vec<Setting> {
    let pointer = Scope::PerDevice(InputType::Pointer);
    let touchpad = Scope::PerDevice(InputType::Touchpad);
    let mut settings = vec![
        Setting::toggle(POINTER_EVENTS, Page::MouseTouchpad, true)
            .section("Mouse")
            .title("Enabled")
            .subtitle("Off ignores the mouse until it is turned on again")
            .keywords(&["mouse", "trackpoint", "disable", "turn off"])
            .scope(pointer)
            .apply(Apply::Risky)
            .sway(
                Directive::input("events"),
                ConfigFile::Input,
                |ctx| on_off_events(ctx, POINTER_EVENTS),
                read_on_off_events,
            )
            .build(),
    ];
    settings.extend(common(InputType::Pointer));
    settings.push(
        Setting::choice(
            TOUCHPAD_EVENTS,
            Page::MouseTouchpad,
            "enabled",
            TOUCHPAD_EVENTS_CHOICES,
        )
        .section("Touchpad")
        .title("Enabled")
        .subtitle("Off ignores the touchpad until it is turned on again")
        .keywords(&["touchpad", "trackpad", "disable", "turn off", "palm"])
        .scope(touchpad)
        .apply(Apply::Risky)
        .sway(
            Directive::input("events"),
            ConfigFile::Input,
            |ctx| {
                let events = match ctx.choice(TOUCHPAD_EVENTS)? {
                    "disabled" => SendEvents::Disabled,
                    "disabled_on_external_mouse" => SendEvents::DisabledOnExternalMouse,
                    _ => SendEvents::Enabled,
                };
                Ok(vec![ctx.input()?.events(events)])
            },
            |args| {
                let id = match read_events(args)? {
                    SendEvents::Enabled => "enabled",
                    SendEvents::Disabled => "disabled",
                    SendEvents::DisabledOnExternalMouse => "disabled_on_external_mouse",
                };
                Ok(Value::Choice(id.to_owned()))
            },
        )
        .build(),
    );
    settings.extend(touchpad_only());
    settings.extend(common(InputType::Touchpad));
    settings
}

/// The options pointers and touchpads share, for the devices of `input_type`.
#[expect(
    clippy::too_many_lines,
    reason = "one builder per setting, read best as a list"
)]
fn common(input_type: InputType) -> Vec<Setting> {
    let scope = Scope::PerDevice(input_type);
    let touchpad = input_type == InputType::Touchpad;
    let section = if touchpad { "Touchpad" } else { "Mouse" };
    // The words of pointers or of touchpads.
    let keywords = |words: [&'static [&'static str]; 2]| words[usize::from(touchpad)];
    vec![
        Setting::decimal(POINTER_ACCEL.of(input_type), Page::MouseTouchpad, 0.0, -1.0, 1.0)
            .section(section)
            .title("Pointer speed")
            .slider(-1.0, 1.0, 0.05)
            .keywords(keywords([
                &["mouse", "trackpoint", "acceleration", "sensitivity"],
                &["touchpad", "trackpad", "acceleration", "sensitivity"],
            ]))
            .scope(scope)
            .sway(
                Directive::input("pointer_accel"),
                ConfigFile::Input,
                |ctx| {
                    Ok(vec![
                        ctx.input()?
                            .pointer_accel(ctx.decimal(POINTER_ACCEL.at(ctx))?)?,
                    ])
                },
                read_accel,
            )
            .build(),
        Setting::choice(
            ACCEL_PROFILE.of(input_type),
            Page::MouseTouchpad,
            "adaptive",
            ACCEL_PROFILES,
        )
        .section(section)
        .title("Acceleration")
        .subtitle("Adaptive moves farther the faster you move; flat keeps the same speed")
        .keywords(keywords([
            &["mouse", "trackpoint", "acceleration profile", "gaming"],
            &["touchpad", "trackpad", "acceleration profile"],
        ]))
        .scope(scope)
        .sway(
            Directive::input("accel_profile"),
            ConfigFile::Input,
            |ctx| {
                let profile = match ctx.choice(ACCEL_PROFILE.at(ctx))? {
                    "flat" => AccelProfile::Flat,
                    _ => AccelProfile::Adaptive,
                };
                Ok(vec![ctx.input()?.accel_profile(profile)])
            },
            |args| read::choice(read::first(args)?, &["adaptive", "flat"], false),
        )
        .build(),
        Setting::toggle(NATURAL_SCROLL.of(input_type), Page::MouseTouchpad, false)
            .section(section)
            .title("Natural scrolling")
            .subtitle("Content moves with your fingers, like on a phone")
            .keywords(keywords([
                &["mouse", "trackpoint", "reverse", "invert", "scroll direction"],
                &["touchpad", "trackpad", "reverse", "invert", "scroll direction"],
            ]))
            .scope(scope)
            .sway(
                Directive::input("natural_scroll"),
                ConfigFile::Input,
                |ctx| {
                    Ok(vec![
                        ctx.input()?
                            .natural_scroll(ctx.bool(NATURAL_SCROLL.at(ctx))?),
                    ])
                },
                read_switch,
            )
            .build(),
        Setting::decimal(
            SCROLL_FACTOR.of(input_type),
            Page::MouseTouchpad,
            1.0,
            0.0,
            MAX_SCROLL_FACTOR,
        )
        .section(section)
        .title("Scroll speed")
        .subtitle("How far one step of scrolling goes; 1 is unchanged")
        .slider(0.1, 5.0, 0.1)
        .keywords(keywords([
            &["mouse", "trackpoint", "scroll factor", "wheel"],
            &["touchpad", "trackpad", "scroll factor"],
        ]))
        .scope(scope)
        .sway(
            Directive::input("scroll_factor"),
            ConfigFile::Input,
            |ctx| {
                Ok(vec![
                    ctx.input()?
                        .scroll_factor(ctx.decimal(SCROLL_FACTOR.at(ctx))?)?,
                ])
            },
            read_scroll_factor,
        )
        .build(),
        Setting::choice(
            SCROLL_METHOD.of(input_type),
            Page::MouseTouchpad,
            if touchpad { "two_finger" } else { "none" },
            if touchpad {
                TOUCHPAD_SCROLL_METHODS
            } else {
                POINTER_SCROLL_METHODS
            },
        )
        .section(section)
        .title("Scroll method")
        .subtitle("Unless set, the device decides: trackpoints scroll while the middle button is held")
        .keywords(keywords([
            &["mouse", "trackpoint", "scrolling", "button scrolling"],
            &["touchpad", "trackpad", "scrolling", "two finger", "edge scrolling"],
        ]))
        .scope(scope)
        .sway(
            Directive::input("scroll_method"),
            ConfigFile::Input,
            |ctx| {
                let method = match ctx.choice(SCROLL_METHOD.at(ctx))? {
                    "two_finger" => ScrollMethod::TwoFinger,
                    "edge" => ScrollMethod::Edge,
                    "on_button_down" => ScrollMethod::OnButtonDown,
                    _ => ScrollMethod::None,
                };
                Ok(vec![ctx.input()?.scroll_method(method)])
            },
            |args| {
                read::choice(
                    read::first(args)?,
                    &["none", "two_finger", "edge", "on_button_down"],
                    false,
                )
            },
        )
        .build(),
        Setting::choice(
            SCROLL_BUTTON.of(input_type),
            Page::MouseTouchpad,
            "button2",
            SCROLL_BUTTONS,
        )
        .section(section)
        .title("Scroll button")
        .subtitle("The button to hold while scrolling with a held button")
        .keywords(keywords([
            &["mouse", "trackpoint", "scrolling", "button scrolling"],
            &["touchpad", "trackpad", "scrolling", "button scrolling"],
        ]))
        .scope(scope)
        .sway(
            Directive::input("scroll_button"),
            ConfigFile::Input,
            |ctx| {
                let button = match ctx.choice(SCROLL_BUTTON.at(ctx))? {
                    "disable" => ScrollButton::Disable,
                    "button1" => ScrollButton::Left,
                    "button3" => ScrollButton::Right,
                    "button8" => ScrollButton::Side,
                    "button9" => ScrollButton::Extra,
                    _ => ScrollButton::Middle,
                };
                Ok(vec![ctx.input()?.scroll_button(button)])
            },
            read_scroll_button,
        )
        .build(),
        Setting::toggle(LEFT_HANDED.of(input_type), Page::MouseTouchpad, false)
            .section(section)
            .title("Left-handed")
            .subtitle("Swaps the left and right buttons")
            .keywords(keywords([
                &["mouse", "trackpoint", "swap buttons", "primary button"],
                &["touchpad", "trackpad", "swap buttons", "primary button"],
            ]))
            .scope(scope)
            .sway(
                Directive::input("left_handed"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.left_handed(ctx.bool(LEFT_HANDED.at(ctx))?)]),
                read_switch,
            )
            .build(),
        Setting::toggle(MIDDLE_EMULATION.of(input_type), Page::MouseTouchpad, false)
            .section(section)
            .title("Middle-click emulation")
            .subtitle("Pressing left and right together clicks the middle button. Unless set, the device decides")
            .keywords(keywords([
                &["mouse", "trackpoint", "middle button", "paste"],
                &["touchpad", "trackpad", "middle button", "paste"],
            ]))
            .scope(scope)
            .sway(
                Directive::input("middle_emulation"),
                ConfigFile::Input,
                |ctx| {
                    Ok(vec![
                        ctx.input()?
                            .middle_emulation(ctx.bool(MIDDLE_EMULATION.at(ctx))?),
                    ])
                },
                read_switch,
            )
            .build(),
    ]
}

/// The options of touchpads only.
#[expect(
    clippy::too_many_lines,
    reason = "one builder per setting, read best as a list"
)]
fn touchpad_only() -> Vec<Setting> {
    let touchpad = Scope::PerDevice(InputType::Touchpad);
    vec![
        Setting::toggle(TAP, Page::MouseTouchpad, false)
            .section("Touchpad")
            .title("Tap to click")
            .subtitle("Tap the touchpad instead of pressing it. Unless set, the device decides")
            .keywords(&["touchpad", "trackpad"])
            .scope(touchpad)
            .sway(
                Directive::input("tap"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.tap(ctx.bool(TAP)?)]),
                read_switch,
            )
            .build(),
        Setting::choice(TAP_BUTTON_MAP, Page::MouseTouchpad, "lrm", BUTTON_MAPS)
            .section("Touchpad")
            .title("Tapping with fingers")
            .subtitle("The buttons a tap with one, two and three fingers clicks")
            .keywords(&["touchpad", "trackpad", "tap button map", "two finger tap", "right click"])
            .scope(touchpad)
            .sway(
                Directive::input("tap_button_map"),
                ConfigFile::Input,
                |ctx| {
                    Ok(vec![
                        ctx.input()?
                            .tap_button_map(button_map(ctx.choice(TAP_BUTTON_MAP)?)),
                    ])
                },
                |args| read::choice(read::first(args)?, &["lrm", "lmr"], false),
            )
            .build(),
        Setting::toggle(DRAG, Page::MouseTouchpad, true)
            .section("Touchpad")
            .title("Tap and drag")
            .subtitle("Tap, then touch again and move to drag")
            .keywords(&["touchpad", "trackpad", "drag"])
            .scope(touchpad)
            .sway(
                Directive::input("drag"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.drag(ctx.bool(DRAG)?)]),
                read_switch,
            )
            .build(),
        Setting::choice(DRAG_LOCK, Page::MouseTouchpad, "disabled", DRAG_LOCKS)
            .section("Touchpad")
            .title("Drag lock")
            .subtitle("Keeps dragging when the finger is lifted")
            .keywords(&["touchpad", "trackpad", "drag"])
            .scope(touchpad)
            .sway(
                Directive::input("drag_lock"),
                ConfigFile::Input,
                |ctx| {
                    let lock = match ctx.choice(DRAG_LOCK)? {
                        "enabled" => DragLock::Enabled,
                        "enabled_sticky" => DragLock::EnabledSticky,
                        _ => DragLock::Disabled,
                    };
                    Ok(vec![ctx.input()?.drag_lock(lock)])
                },
                read_drag_lock,
            )
            .build(),
        Setting::toggle(DWT, Page::MouseTouchpad, true)
            .section("Touchpad")
            .title("Off while typing")
            .subtitle("Ignores the touchpad while you type")
            .keywords(&["touchpad", "trackpad", "disable while typing", "palm"])
            .scope(touchpad)
            .sway(
                Directive::input("dwt"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.dwt(ctx.bool(DWT)?)]),
                read_switch,
            )
            .build(),
        Setting::toggle(DWTP, Page::MouseTouchpad, true)
            .section("Touchpad")
            .title("Off while using the trackpoint")
            .subtitle("Ignores the touchpad while the trackpoint is used")
            .keywords(&["touchpad", "trackpad", "trackpoint", "disable while trackpointing"])
            .scope(touchpad)
            .sway(
                Directive::input("dwtp"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.dwtp(ctx.bool(DWTP)?)]),
                read_switch,
            )
            .build(),
        Setting::choice(CLICK_METHOD, Page::MouseTouchpad, "button_areas", CLICK_METHODS)
            .section("Touchpad")
            .title("Right click")
            .subtitle("Press the bottom right corner, or press with two fingers. Unless set, the device decides")
            .keywords(&["touchpad", "trackpad", "click method", "clickfinger", "button areas"])
            .scope(touchpad)
            .sway(
                Directive::input("click_method"),
                ConfigFile::Input,
                |ctx| {
                    let method = match ctx.choice(CLICK_METHOD)? {
                        "clickfinger" => ClickMethod::Clickfinger,
                        "none" => ClickMethod::None,
                        _ => ClickMethod::ButtonAreas,
                    };
                    Ok(vec![ctx.input()?.click_method(method)])
                },
                |args| {
                    read::choice(
                        read::first(args)?,
                        &["none", "button_areas", "clickfinger"],
                        false,
                    )
                },
            )
            .build(),
        Setting::choice(CLICKFINGER_BUTTON_MAP, Page::MouseTouchpad, "lrm", BUTTON_MAPS)
            .section("Touchpad")
            .title("Pressing with fingers")
            .subtitle("The buttons a press with one, two and three fingers clicks, when the number of fingers decides")
            .keywords(&["touchpad", "trackpad", "clickfinger button map", "right click"])
            .scope(touchpad)
            .sway(
                Directive::input("clickfinger_button_map"),
                ConfigFile::Input,
                |ctx| {
                    Ok(vec![
                        ctx.input()?
                            .clickfinger_button_map(button_map(ctx.choice(CLICKFINGER_BUTTON_MAP)?)),
                    ])
                },
                |args| read::choice(read::first(args)?, &["lrm", "lmr"], false),
            )
            .build(),
    ]
}

/// The settings of the Keyboard page: switching keyboards on and off, the keymap (layouts with
/// their variants, the model), the XKB options, then typing.
#[expect(
    clippy::too_many_lines,
    reason = "one builder per setting, read best as a list"
)]
fn keyboard() -> Vec<Setting> {
    let keyboard = Scope::PerDevice(InputType::Keyboard);
    vec![
        Setting::toggle(KEYBOARD_EVENTS, Page::Keyboard, true)
            .section("Keyboard")
            .title("Enabled")
            .subtitle("Off ignores the keyboard until it is turned on again")
            .keywords(&["keyboard", "disable", "turn off"])
            .scope(keyboard)
            .apply(Apply::Risky)
            .sway(
                Directive::input("events"),
                ConfigFile::Input,
                |ctx| on_off_events(ctx, KEYBOARD_EVENTS),
                read_on_off_events,
            )
            .build(),
        Setting::list(XKB_LAYOUT, Page::Keyboard, &["us"])
            .section("Layouts")
            .title("Layouts")
            .subtitle("The keymaps you type with, in the order the layout switching goes through them")
            .keywords(&["keyboard", "language", "xkb", "keymap", "qwerty", "azerty", "qwertz"])
            .scope(keyboard)
            .apply(Apply::Risky)
            .sway(
                Directive::input("xkb_layout"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.xkb_layout(&ctx.list(XKB_LAYOUT)?)?]),
                |args| {
                    Ok(Value::List(
                        read::one(args)?.split(',').map(str::to_owned).collect(),
                    ))
                },
            )
            .build(),
        Setting::list(XKB_VARIANT, Page::Keyboard, &[])
            .section("Layouts")
            .title("Variants")
            .subtitle("One for each layout, e.g. Dvorak or without dead keys; empty keeps the layout as it is")
            .keywords(&["keyboard", "xkb", "keymap", "dvorak", "colemak", "dead keys", "nodeadkeys"])
            .scope(keyboard)
            .apply(Apply::Risky)
            .sway(
                Directive::input("xkb_variant"),
                ConfigFile::Input,
                render_variants,
                read_variants,
            )
            .build(),
        Setting::text(XKB_MODEL, Page::Keyboard, "")
            .section("Layouts")
            .title("Keyboard model")
            .subtitle("The physical keyboard, for keys only some models have. Empty uses the default model, pc105")
            .keywords(&["keyboard", "xkb", "hardware", "pc105"])
            .scope(keyboard)
            .sway(
                Directive::input("xkb_model"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.xkb_model(&ctx.text(XKB_MODEL)?)?]),
                |args| Ok(Value::Text(read::one(args)?.to_owned())),
            )
            .build(),
        Setting::list(XKB_OPTIONS, Page::Keyboard, &[])
            .section("Options")
            .title("XKB options")
            .subtitle("Extra key behavior, e.g. Caps Lock as Ctrl, a Compose key or the shortcut that switches layouts")
            .keywords(&[
                "keyboard",
                "xkb",
                "caps lock",
                "ctrl",
                "control",
                "escape",
                "compose",
                "alt",
                "super",
                "switch layout",
                "remap",
            ])
            .scope(keyboard)
            .apply(Apply::Risky)
            .sway(
                Directive::input("xkb_options"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.xkb_options(&ctx.list(XKB_OPTIONS)?)?]),
                // XKB skips empty options.
                |args| {
                    Ok(Value::List(
                        read::one(args)?
                            .split(',')
                            .filter(|option| !option.is_empty())
                            .map(str::to_owned)
                            .collect(),
                    ))
                },
            )
            .build(),
        Setting::integer(REPEAT_DELAY, Page::Keyboard, 600, 100, i64::from(u16::MAX))
            .section("Typing")
            .title("Repeat delay")
            .slider(100.0, 2000.0, 10.0)
            .subtitle("Milliseconds a key is held before it repeats")
            .keywords(&["keyboard", "typing", "key repeat", "hold"])
            .scope(keyboard)
            .sway(
                Directive::input("repeat_delay"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.repeat_delay(ctx.integer(REPEAT_DELAY)?)]),
                |args| match read::atoi(read::one(args)?)? {
                    delay if delay < 0 => Err(ReadError::Rejected(format!(
                        "the repeat delay can't be negative: {delay}"
                    ))),
                    delay => Ok(Value::Integer(delay)),
                },
            )
            .build(),
        Setting::integer(REPEAT_RATE, Page::Keyboard, 25, 1, i64::from(u16::MAX))
            .section("Typing")
            .title("Repeat rate")
            .slider(1.0, 100.0, 1.0)
            .subtitle("Characters per second while a key is held")
            .keywords(&["keyboard", "typing", "key repeat", "speed"])
            .scope(keyboard)
            .sway(
                Directive::input("repeat_rate"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.repeat_rate(ctx.integer(REPEAT_RATE)?)]),
                |args| match read::atoi(read::one(args)?)? {
                    rate if rate < 0 => Err(ReadError::Rejected(format!(
                        "the repeat rate can't be negative: {rate}"
                    ))),
                    0 => Err(ReadError::Unsupported(
                        "a repeat rate of 0 turns key repeat off".to_owned(),
                    )),
                    rate => Ok(Value::Integer(rate)),
                },
            )
            .build(),
        Setting::toggle(XKB_NUMLOCK, Page::Keyboard, false)
            .section("Typing")
            .title("Num Lock on at startup")
            .subtitle("Turns Num Lock on when sway sets up the keyboard")
            .keywords(&["keyboard", "numlock", "number pad", "numpad"])
            .scope(keyboard)
            .apply(Apply::Reload)
            .sway(
                Directive::input("xkb_numlock"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.xkb_numlock(ctx.bool(XKB_NUMLOCK)?)]),
                read_lock,
            )
            .build(),
        Setting::toggle(XKB_CAPSLOCK, Page::Keyboard, false)
            .section("Typing")
            .title("Caps Lock on at startup")
            .subtitle("Turns Caps Lock on when sway sets up the keyboard")
            .keywords(&["keyboard", "capslock", "caps lock", "uppercase"])
            .scope(keyboard)
            .apply(Apply::Reload)
            .sway(
                Directive::input("xkb_capslock"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.xkb_capslock(ctx.bool(XKB_CAPSLOCK)?)]),
                read_lock,
            )
            .build(),
    ]
}

/// `xkb_variant`: one variant for each layout, at the same position. A shorter list is padded with
/// empty variants (the layout as it is), a longer one is refused: its extra variants have no
/// layout.
fn render_variants(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let layouts = ctx.list(XKB_LAYOUT)?.len();
    let mut variants = ctx.list(XKB_VARIANT)?;
    if variants.len() > layouts {
        return Err(RenderError::TooManyVariants {
            key: XKB_VARIANT,
            variants: variants.len(),
            layouts,
        });
    }
    variants.resize(layouts, String::new());
    Ok(vec![ctx.input()?.xkb_variant(&variants)?])
}

/// `xkb_variant` read from a line: split on commas like XKB, empty variants included. Empty ones
/// at the end are dropped, since rendering pads the list again.
fn read_variants(args: &[String]) -> Result<Value, ReadError> {
    let mut variants: Vec<String> = read::one(args)?.split(',').map(str::to_owned).collect();
    while variants.last().is_some_and(String::is_empty) {
        variants.pop();
    }
    Ok(Value::List(variants))
}

/// `xkb_numlock` and `xkb_capslock`: sway reads `parse_boolean(…, false)`, so `toggle` is on.
fn read_lock(args: &[String]) -> Result<Value, ReadError> {
    Ok(Value::Bool(read::boolean(read::first(args)?, false)))
}
