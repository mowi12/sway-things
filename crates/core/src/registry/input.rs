// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Mouse & Touchpad and Keyboard pages (`man 5 sway-input`).
//!
//! The defaults of libinput options depend on the device (a touchpad without buttons taps by
//! default); the values here are the common ones.

use super::read::{self, ReadError};
use super::{Apply, ConfigFile, Directive, Page, Scope, Setting, Value};
use crate::command::input::InputType;

const TAP: &str = "input.touchpad.tap";
const POINTER_ACCEL: &str = "input.touchpad.pointer_accel";
const REPEAT_DELAY: &str = "input.keyboard.repeat_delay";
const XKB_LAYOUT: &str = "input.keyboard.xkb_layout";

pub(super) fn settings() -> Vec<Setting> {
    let touchpad = Scope::PerDevice(InputType::Touchpad);
    let keyboard = Scope::PerDevice(InputType::Keyboard);
    vec![
        Setting::toggle(TAP, Page::MouseTouchpad, false)
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
            .title("Pointer speed")
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
        Setting::integer(REPEAT_DELAY, Page::Keyboard, 600, 0, i64::from(u16::MAX))
            .title("Repeat delay")
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
