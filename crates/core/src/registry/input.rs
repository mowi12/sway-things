// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Mouse & Touchpad and Keyboard pages (`man 5 sway-input`).
//!
//! The defaults of libinput options depend on the device (a touchpad without buttons taps by
//! default); the values here are the common ones.

use super::read::{self, ReadError};
use super::{
    Apply, Choice, ConfigFile, Context, Directive, Page, RenderError, Scope, Setting, Value,
};
use crate::command::SwayCmd;
use crate::command::input::{InputType, SendEvents};

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
const POINTER_ACCEL: &str = "input.touchpad.pointer_accel";
const REPEAT_DELAY: &str = "input.keyboard.repeat_delay";
const XKB_LAYOUT: &str = "input.keyboard.xkb_layout";

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

pub(super) fn settings() -> Vec<Setting> {
    let mut settings = mouse_touchpad();
    settings.extend(keyboard());
    settings
}

/// The settings of the Mouse & Touchpad page.
fn mouse_touchpad() -> Vec<Setting> {
    let pointer = Scope::PerDevice(InputType::Pointer);
    let touchpad = Scope::PerDevice(InputType::Touchpad);
    vec![
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
        Setting::toggle(TAP, Page::MouseTouchpad, false)
            .section("Touchpad")
            .title("Tap to click")
            .subtitle("Tap the touchpad instead of pressing it")
            .keywords(&["touchpad", "trackpad"])
            .scope(touchpad)
            .sway(
                Directive::input("tap"),
                ConfigFile::Input,
                |ctx| Ok(vec![ctx.input()?.tap(ctx.bool(TAP)?)]),
                // `toggle` is false here: sway reads it with `parse_boolean(…, true)`.
                |args| Ok(Value::Bool(read::boolean(read::first(args)?, true))),
            )
            .build(),
        Setting::decimal(POINTER_ACCEL, Page::MouseTouchpad, 0.0, -1.0, 1.0)
            .section("Touchpad")
            .title("Pointer speed")
            .slider(-1.0, 1.0, 0.05)
            .keywords(&["touchpad", "trackpad", "acceleration", "sensitivity"])
            .scope(touchpad)
            .sway(
                Directive::input("pointer_accel"),
                ConfigFile::Input,
                |ctx| {
                    Ok(vec![
                        ctx.input()?.pointer_accel(ctx.decimal(POINTER_ACCEL)?)?,
                    ])
                },
                |args| {
                    let speed = read::float(read::first(args)?)?;
                    if (-1.0..=1.0).contains(&speed) {
                        Ok(Value::Decimal(speed))
                    } else {
                        Err(ReadError::Rejected(format!(
                            "out of range [-1, 1]: {speed}"
                        )))
                    }
                },
            )
            .build(),
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
    ]
}

/// The settings of the Keyboard page.
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
        Setting::integer(REPEAT_DELAY, Page::Keyboard, 600, 0, i64::from(u16::MAX))
            .section("Typing")
            .title("Repeat delay")
            .slider(100.0, 2000.0, 10.0)
            .subtitle("Milliseconds a key is held before it repeats")
            .keywords(&["keyboard", "typing"])
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
        Setting::list(XKB_LAYOUT, Page::Keyboard, &["us"])
            .section("Layouts")
            .title("Layouts")
            .keywords(&["keyboard", "language", "xkb"])
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
    ]
}
