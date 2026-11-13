// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SwayFX effects on the Windows page, in `90-effects.conf`: corners, blur, shadows, dimming and
//! animations. Ranges and defaults are SwayFX's own (`sway/commands/*.c`, `config_defaults`, and
//! scenefx's `blur_data_get_default` for the blur).

use super::read::{self, ReadError};
use super::{ConfigFile, Directive, Page, Setting, Value, When};
use crate::command::effects::{self, DimClass};
use crate::command::{Color, SwayCmd};
use crate::msg;
use crate::version::SwayFxVersion;

const CORNER_RADIUS: &str = "effects.corner_radius";
const SMART_CORNER_RADIUS: &str = "effects.smart_corner_radius";
const BLUR: &str = "effects.blur";
const BLUR_PASSES: &str = "effects.blur_passes";
const BLUR_RADIUS: &str = "effects.blur_radius";
const BLUR_NOISE: &str = "effects.blur_noise";
const BLUR_BRIGHTNESS: &str = "effects.blur_brightness";
const BLUR_CONTRAST: &str = "effects.blur_contrast";
const BLUR_SATURATION: &str = "effects.blur_saturation";
const SHADOWS: &str = "effects.shadows";
const SHADOW_COLOR: &str = "effects.shadow_color";
const SHADOW_INACTIVE_COLOR: &str = "effects.shadow_inactive_color";
const SHADOW_BLUR_RADIUS: &str = "effects.shadow_blur_radius";
const SHADOW_OFFSET_X: &str = "effects.shadow_offset_x";
const SHADOW_OFFSET_Y: &str = "effects.shadow_offset_y";
const DIM_INACTIVE: &str = "effects.dim_inactive";
const DIM_UNFOCUSED: &str = "effects.dim_inactive_colors.unfocused";
const DIM_URGENT: &str = "effects.dim_inactive_colors.urgent";
const ANIMATION_DURATION: &str = "effects.animation_duration_ms";

/// SwayFX's default shadow colors: black at half opacity.
const SHADOW_DEFAULT: Color = Color::rgba(0, 0, 0, 0x7f);

/// A whole number within `0 ..= max`, as SwayFX checks it with `strtol` and its end pointer.
fn whole_within(text: &str, max: i64) -> Result<Value, ReadError> {
    match read::whole(text)? {
        value if (0..=max).contains(&value) => Ok(Value::Integer(value)),
        value => Err(ReadError::Rejected(format!(
            "out of range [0, {max}]: {value}"
        ))),
    }
}

/// A decimal number within `min ..= max`, as SwayFX reads it with `strtof`.
fn decimal_within(text: &str, min: f64, max: f64) -> Result<f64, ReadError> {
    match read::float(text)? {
        value if (min..=max).contains(&value) => Ok(value),
        value => Err(ReadError::Rejected(format!(
            "out of range [{min}, {max}]: {value}"
        ))),
    }
}

/// An on/off effect, read with `parse_boolean` like SwayFX.
fn read_on_off(args: &[String]) -> Result<Value, ReadError> {
    Ok(Value::Bool(read::boolean(read::first(args)?, true)))
}

/// A color effect: its first argument.
fn read_color(args: &[String]) -> Result<Value, ReadError> {
    Ok(Value::Color(read::color(read::first(args)?)?))
}

fn shadow_offset(ctx: &super::Context<'_>) -> Result<Vec<SwayCmd>, super::RenderError> {
    Ok(vec![effects::shadow_offset(
        ctx.decimal(SHADOW_OFFSET_X)?,
        ctx.decimal(SHADOW_OFFSET_Y)?,
    )?])
}

/// The offsets of a `shadow_offset` line: two decimals within −99.9 … 99.9.
fn read_shadow_offset(args: &[String]) -> Result<(f64, f64), ReadError> {
    match args {
        [x, y, ..] => Ok((
            decimal_within(x, -99.9, 99.9)?,
            decimal_within(y, -99.9, 99.9)?,
        )),
        _ => Err(ReadError::Rejected(format!(
            "expected 2 offsets, got {}",
            args.len()
        ))),
    }
}

/// A blur setting: on SwayFX, in the Blur section, only while blur is on.
fn blur_setting(builder: super::SettingBuilder) -> super::SettingBuilder {
    builder
        .section(msg!("Blur"))
        .swayfx_only(SwayFxVersion::OLDEST_SUPPORTED)
        .depends_on(BLUR, When::On)
}

/// A shadow setting: on SwayFX, in the Shadows section, only while shadows are on.
fn shadow_setting(builder: super::SettingBuilder) -> super::SettingBuilder {
    builder
        .section(msg!("Shadows"))
        .swayfx_only(SwayFxVersion::OLDEST_SUPPORTED)
        .depends_on(SHADOWS, When::On)
}

#[expect(
    clippy::too_many_lines,
    reason = "one builder per setting, read best as a list"
)]
pub(super) fn settings() -> Vec<Setting> {
    let fx = SwayFxVersion::OLDEST_SUPPORTED;
    vec![
        Setting::integer(CORNER_RADIUS, Page::Windows, 0, 0, 99)
            .section(msg!("Corners"))
            .title(msg!("Corner radius"))
            .subtitle(msg!(
                "Rounded window corners, in pixels, for windows opened from now on"
            ))
            .keywords(&[msg!("rounded"), msg!("round corners")])
            .swayfx_only(fx)
            .sway(
                Directive::top("corner_radius"),
                ConfigFile::Effects,
                |ctx| Ok(vec![effects::corner_radius(ctx.integer(CORNER_RADIUS)?)?]),
                |args| match read::whole(read::one(args)?)? {
                    radius @ 0..=99 => Ok(Value::Integer(radius)),
                    radius => Err(ReadError::Rejected(format!(
                        "out of range [0, 99]: {radius}"
                    ))),
                },
            )
            .build(),
        Setting::toggle(SMART_CORNER_RADIUS, Page::Windows, true)
            .section(msg!("Corners"))
            .title(msg!("Square corners without gaps"))
            .subtitle(msg!(
                "Round corners only where there are gaps around the window"
            ))
            .keywords(&[msg!("rounded"), msg!("smart corners")])
            .swayfx_only(fx)
            .depends_on(CORNER_RADIUS, When::Above(0.0))
            .sway(
                Directive::top("smart_corner_radius"),
                ConfigFile::Effects,
                |ctx| {
                    Ok(vec![effects::smart_corner_radius(
                        ctx.bool(SMART_CORNER_RADIUS)?,
                    )])
                },
                |args| Ok(Value::Bool(read::boolean(read::one(args)?, true))),
            )
            .build(),
        Setting::toggle(BLUR, Page::Windows, false)
            .section(msg!("Blur"))
            .title(msg!("Blur"))
            .subtitle(msg!("Blurs what is behind transparent windows"))
            .keywords(&[msg!("transparency"), msg!("frosted glass")])
            .swayfx_only(fx)
            .sway(
                Directive::top("blur"),
                ConfigFile::Effects,
                |ctx| Ok(vec![effects::blur(ctx.bool(BLUR)?)]),
                read_on_off,
            )
            .build(),
        blur_setting(Setting::integer(BLUR_PASSES, Page::Windows, 3, 0, 10))
            .title(msg!("Passes"))
            .subtitle(msg!(
                "More passes blur more strongly and cost more; 0 turns blur off"
            ))
            .keywords(&[msg!("blur strength")])
            .sway(
                Directive::top("blur_passes"),
                ConfigFile::Effects,
                |ctx| Ok(vec![effects::blur_passes(ctx.integer(BLUR_PASSES)?)?]),
                |args| whole_within(read::one(args)?, 10),
            )
            .build(),
        blur_setting(Setting::integer(BLUR_RADIUS, Page::Windows, 5, 0, 10))
            .title(msg!("Radius"))
            .subtitle(msg!("How far each pass blurs; 0 turns blur off"))
            .keywords(&[msg!("blur size")])
            .sway(
                Directive::top("blur_radius"),
                ConfigFile::Effects,
                |ctx| Ok(vec![effects::blur_radius(ctx.integer(BLUR_RADIUS)?)?]),
                |args| whole_within(read::first(args)?, 10),
            )
            .build(),
        blur_setting(Setting::decimal(BLUR_NOISE, Page::Windows, 0.02, 0.0, 1.0))
            .title(msg!("Noise"))
            .subtitle(msg!("Grain over the blur, against banding"))
            .keywords(&[msg!("grain"), msg!("dither")])
            .sway(
                Directive::top("blur_noise"),
                ConfigFile::Effects,
                |ctx| Ok(vec![effects::blur_noise(ctx.decimal(BLUR_NOISE)?)?]),
                |args| Ok(Value::Decimal(decimal_within(read::one(args)?, 0.0, 1.0)?)),
            )
            .build(),
        blur_setting(Setting::decimal(
            BLUR_BRIGHTNESS,
            Page::Windows,
            0.9,
            0.0,
            2.0,
        ))
        .title(msg!("Brightness"))
        .subtitle(msg!("1 keeps the brightness"))
        .sway(
            Directive::top("blur_brightness"),
            ConfigFile::Effects,
            |ctx| {
                Ok(vec![effects::blur_brightness(
                    ctx.decimal(BLUR_BRIGHTNESS)?,
                )?])
            },
            |args| Ok(Value::Decimal(decimal_within(read::one(args)?, 0.0, 2.0)?)),
        )
        .build(),
        blur_setting(Setting::decimal(
            BLUR_CONTRAST,
            Page::Windows,
            0.9,
            0.0,
            2.0,
        ))
        .title(msg!("Contrast"))
        .subtitle(msg!("1 keeps the contrast"))
        .sway(
            Directive::top("blur_contrast"),
            ConfigFile::Effects,
            |ctx| Ok(vec![effects::blur_contrast(ctx.decimal(BLUR_CONTRAST)?)?]),
            |args| Ok(Value::Decimal(decimal_within(read::one(args)?, 0.0, 2.0)?)),
        )
        .build(),
        blur_setting(Setting::decimal(
            BLUR_SATURATION,
            Page::Windows,
            1.1,
            0.0,
            2.0,
        ))
        .title(msg!("Saturation"))
        .subtitle(msg!("1 keeps the saturation"))
        .keywords(&[msg!("colorfulness")])
        .sway(
            Directive::top("blur_saturation"),
            ConfigFile::Effects,
            |ctx| {
                Ok(vec![effects::blur_saturation(
                    ctx.decimal(BLUR_SATURATION)?,
                )?])
            },
            |args| Ok(Value::Decimal(decimal_within(read::one(args)?, 0.0, 2.0)?)),
        )
        .build(),
        Setting::toggle(SHADOWS, Page::Windows, false)
            .section(msg!("Shadows"))
            .title(msg!("Shadows"))
            .subtitle(msg!("Drop shadows behind windows"))
            .keywords(&[msg!("drop shadow"), msg!("depth")])
            .swayfx_only(fx)
            .sway(
                Directive::top("shadows"),
                ConfigFile::Effects,
                |ctx| Ok(vec![effects::shadows(ctx.bool(SHADOWS)?)]),
                read_on_off,
            )
            .build(),
        shadow_setting(Setting::color(SHADOW_COLOR, Page::Windows, SHADOW_DEFAULT))
            .title(msg!("Color"))
            .subtitle(msg!("Of the focused window, usually partly transparent"))
            .keywords(&[msg!("shadow color")])
            .sway(
                Directive::top("shadow_color"),
                ConfigFile::Effects,
                |ctx| Ok(vec![effects::shadow_color(ctx.color(SHADOW_COLOR)?)]),
                read_color,
            )
            .build(),
        shadow_setting(Setting::color(
            SHADOW_INACTIVE_COLOR,
            Page::Windows,
            SHADOW_DEFAULT,
        ))
        .title(msg!("Color of other windows"))
        .subtitle(msg!("Unless set, it follows the color"))
        .keywords(&[msg!("shadow color"), msg!("inactive"), msg!("unfocused")])
        .sway(
            Directive::top("shadow_inactive_color"),
            ConfigFile::Effects,
            |ctx| {
                Ok(vec![effects::shadow_inactive_color(
                    ctx.color(SHADOW_INACTIVE_COLOR)?,
                )])
            },
            read_color,
        )
        .build(),
        shadow_setting(Setting::integer(
            SHADOW_BLUR_RADIUS,
            Page::Windows,
            20,
            0,
            99,
        ))
        .title(msg!("Softness"))
        .subtitle(msg!("How far the shadow fades out, in pixels"))
        .keywords(&[msg!("shadow blur"), msg!("shadow size")])
        .sway(
            Directive::top("shadow_blur_radius"),
            ConfigFile::Effects,
            |ctx| {
                Ok(vec![effects::shadow_blur_radius(
                    ctx.integer(SHADOW_BLUR_RADIUS)?,
                )?])
            },
            |args| whole_within(read::one(args)?, 99),
        )
        .build(),
        shadow_setting(Setting::decimal(
            SHADOW_OFFSET_X,
            Page::Windows,
            0.0,
            -99.9,
            99.9,
        ))
        .group("effects.shadow_offset", msg!("Shadow offset"))
        .title(msg!("Offset to the right"))
        .subtitle(msg!("In pixels; negative values move it left"))
        .slider(-20.0, 20.0, 0.5)
        .keywords(&[msg!("shadow position")])
        .sway(
            Directive::top("shadow_offset"),
            ConfigFile::Effects,
            shadow_offset,
            |args| Ok(Value::Decimal(read_shadow_offset(args)?.0)),
        )
        .build(),
        shadow_setting(Setting::decimal(
            SHADOW_OFFSET_Y,
            Page::Windows,
            0.0,
            -99.9,
            99.9,
        ))
        .group("effects.shadow_offset", msg!("Shadow offset"))
        .title(msg!("Offset down"))
        .subtitle(msg!("In pixels; negative values move it up"))
        .slider(-20.0, 20.0, 0.5)
        .keywords(&[msg!("shadow position")])
        .sway(
            Directive::top("shadow_offset"),
            ConfigFile::Effects,
            shadow_offset,
            |args| Ok(Value::Decimal(read_shadow_offset(args)?.1)),
        )
        .build(),
        Setting::decimal(DIM_INACTIVE, Page::Windows, 0.0, 0.0, 1.0)
            .section(msg!("Dim inactive"))
            .title(msg!("Dim other windows"))
            .subtitle(msg!(
                "0 leaves them as they are, 1 covers them with the dim color"
            ))
            .keywords(&[
                msg!("dimming"),
                msg!("fade"),
                msg!("unfocused"),
                msg!("inactive"),
            ])
            .swayfx_only(fx)
            .sway(
                Directive::top("default_dim_inactive"),
                ConfigFile::Effects,
                |ctx| {
                    Ok(vec![effects::default_dim_inactive(
                        ctx.decimal(DIM_INACTIVE)?,
                    )?])
                },
                |args| Ok(Value::Decimal(decimal_within(read::one(args)?, 0.0, 1.0)?)),
            )
            .build(),
        Setting::color(DIM_UNFOCUSED, Page::Windows, Color::rgb(0, 0, 0))
            .section(msg!("Dim inactive"))
            .title(msg!("Dim color"))
            .keywords(&[msg!("dimming"), msg!("unfocused")])
            .swayfx_only(fx)
            .depends_on(DIM_INACTIVE, When::Above(0.0))
            .sway(
                Directive::top("dim_inactive_colors.unfocused"),
                ConfigFile::Effects,
                |ctx| {
                    Ok(vec![effects::dim_inactive_color(
                        DimClass::Unfocused,
                        ctx.color(DIM_UNFOCUSED)?,
                    )])
                },
                read_color,
            )
            .build(),
        Setting::color(DIM_URGENT, Page::Windows, Color::rgb(0x90, 0, 0))
            .section(msg!("Dim inactive"))
            .title(msg!("Dim color of urgent windows"))
            .keywords(&[msg!("dimming"), msg!("urgent"), msg!("attention")])
            .swayfx_only(fx)
            .depends_on(DIM_INACTIVE, When::Above(0.0))
            .sway(
                Directive::top("dim_inactive_colors.urgent"),
                ConfigFile::Effects,
                |ctx| {
                    Ok(vec![effects::dim_inactive_color(
                        DimClass::Urgent,
                        ctx.color(DIM_URGENT)?,
                    )])
                },
                read_color,
            )
            .build(),
        Setting::integer(ANIMATION_DURATION, Page::Windows, 0, 0, 5000)
            .section(msg!("Animations"))
            .title(msg!("Animation length"))
            .subtitle(msg!(
                "Of opening and closing windows, in milliseconds; 0 turns animations off"
            ))
            .slider(0.0, 1000.0, 10.0)
            .keywords(&[msg!("animate"), msg!("transition"), msg!("speed")])
            .swayfx_only(SwayFxVersion::new(0, 6, 0))
            .sway(
                Directive::top("animation_duration_ms"),
                ConfigFile::Effects,
                |ctx| {
                    Ok(vec![effects::animation_duration_ms(
                        ctx.integer(ANIMATION_DURATION)?,
                    )?])
                },
                |args| {
                    // SwayFX reads the second argument if there are several.
                    let text = match args {
                        [_, second, ..] => second.as_str(),
                        _ => read::first(args)?,
                    };
                    let value = decimal_within(text, 0.0, 5000.0)?;
                    if value.fract() == 0.0 {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "a whole number within 0 … 5000"
                        )]
                        Ok(Value::Integer(value as i64))
                    } else {
                        Err(ReadError::Unsupported(format!(
                            "not whole milliseconds: {value}"
                        )))
                    }
                },
            )
            .build(),
    ]
}
