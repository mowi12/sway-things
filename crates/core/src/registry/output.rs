// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Displays page (`man 5 sway-output`): one value per output.
//!
//! Where sway decides by itself while nothing is set (the preferred mode, the position next to the
//! other outputs, a scale for the density of the panel), the setting's default is "nothing set":
//! an empty mode or position, and a scale that renders no command until it is stored. sway has no
//! command that goes back to its own choice, so removing such a value takes effect after a reload
//! of sway. The app shows what sway runs with instead of the default
//! ([`Output::session_value`](crate::outputs::Output::session_value)).

use super::read::{self, ReadError};
use super::{
    Apply, Choice, ConfigFile, Context, Directive, Page, RenderError, Scope, Setting, Value,
    ValueError,
};
use crate::command::SwayCmd;
use crate::command::output::{self as cmd, MAX_SCALE, MIN_SCALE, Mode, Transform};
use crate::i18n::{Msg, Text};
use crate::msg;
use crate::outputs::{ENABLED, MODE, POSITION, SCALE, TRANSFORM};

const TRANSFORMS: &[Choice] = &[
    Choice::new("normal", msg!("None")),
    Choice::new("90", msg!("90° clockwise")),
    Choice::new("180", msg!("180°")),
    Choice::new("270", msg!("90° counterclockwise")),
    Choice::new("flipped", msg!("Flipped")),
    Choice::new("flipped-90", msg!("Flipped, 90° clockwise")),
    Choice::new("flipped-180", msg!("Flipped, 180°")),
    Choice::new("flipped-270", msg!("Flipped, 90° counterclockwise")),
];

/// The settings of the Displays page, in the order of the page.
pub(super) fn settings() -> Vec<Setting> {
    let section = msg!("Display");
    vec![
        Setting::toggle(ENABLED, Page::Displays, true)
            .section(section)
            .title(msg!("Use this display"))
            .subtitle(msg!(
                "A display that is off gives its workspaces and windows to the others. If no other \
                 display is on when sway starts or reloads, Sway Things switches it on again"
            ))
            .keywords(&[
                msg!("monitor"),
                msg!("screen"),
                msg!("disable"),
                msg!("off"),
            ])
            .scope(Scope::PerOutput)
            .one_at_a_time()
            .apply(Apply::Risky)
            .sway(
                Directive::output("enable"),
                ConfigFile::Output,
                |ctx| Ok(vec![ctx.output()?.enable(ctx.bool(ENABLED)?)]),
                read_enable,
            )
            .build(),
        Setting::text(MODE, Page::Displays, "")
            .section(section)
            .title(msg!("Resolution and refresh rate"))
            .subtitle(msg!("Unless set, the display's preferred mode"))
            .keywords(&[
                msg!("resolution"),
                msg!("refresh rate"),
                msg!("hz"),
                msg!("mode"),
                msg!("monitor"),
            ])
            .scope(Scope::PerOutput)
            .one_at_a_time()
            .apply(Apply::Risky)
            .check_text(check_mode)
            .sway(
                Directive::output("mode"),
                ConfigFile::Output,
                render_mode,
                read_mode,
            )
            .build(),
        Setting::decimal(SCALE, Page::Displays, 1.0, MIN_SCALE, MAX_SCALE)
            .section(section)
            .title(msg!("Scale"))
            .subtitle(msg!(
                "How big everything is on this display. Unless set, sway picks a scale for the \
                 pixel density"
            ))
            .keywords(&[msg!("zoom"), msg!("hidpi"), msg!("size"), msg!("dpi")])
            .slider(0.5, 3.0, 0.05)
            .scope(Scope::PerOutput)
            .sway(
                Directive::output("scale"),
                ConfigFile::Output,
                render_scale,
                read_scale,
            )
            .build(),
        Setting::choice(TRANSFORM, Page::Displays, "normal", TRANSFORMS)
            .section(section)
            .title(msg!("Rotation"))
            .keywords(&[msg!("transform"), msg!("orientation"), msg!("portrait")])
            .scope(Scope::PerOutput)
            // A turned screen is hard to point at: tried with the safety net, for one display.
            .apply(Apply::Risky)
            .one_at_a_time()
            .sway(
                Directive::output("transform"),
                ConfigFile::Output,
                render_transform,
                read_transform,
            )
            .build(),
        Setting::text(POSITION, Page::Displays, "")
            .section(section)
            .title(msg!("Position"))
            .subtitle(msg!(
                "The top left corner in the layout, in logical pixels. Unless set, sway places \
                 the display right of the others"
            ))
            .keywords(&[msg!("arrange"), msg!("layout"), msg!("left"), msg!("right")])
            .scope(Scope::PerOutput)
            .one_at_a_time()
            .check_text(check_position)
            .sway(
                Directive::output("pos"),
                ConfigFile::Output,
                render_position,
                read_position,
            )
            .build(),
    ]
}

fn check_mode(text: &str) -> Result<(), ValueError> {
    if text.is_empty() || Mode::parse(text).is_ok() {
        Ok(())
    } else {
        Err(ValueError::NotAMode)
    }
}

fn check_position(text: &str) -> Result<(), ValueError> {
    if text.is_empty() || cmd::parse_position(text).is_ok() {
        Ok(())
    } else {
        Err(ValueError::NotAPosition)
    }
}

/// The mode, unless it is empty: sway picks the preferred one then.
fn render_mode(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let text = ctx.text(MODE)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![ctx.output()?.mode(Mode::parse(&text)?)])
}

/// The position, unless it is empty: sway places the output itself then.
fn render_position(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let text = ctx.text(POSITION)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let (x, y) = cmd::parse_position(&text)?;
    Ok(vec![ctx.output()?.position(x, y)])
}

/// The scale, once it is stored: until then sway picks one for the pixel density.
fn render_scale(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    if !ctx.is_stored(SCALE) {
        return Ok(Vec::new());
    }
    Ok(vec![ctx.output()?.scale(ctx.decimal(SCALE)?)?])
}

fn render_transform(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let id = ctx.choice(TRANSFORM)?;
    let transform = Transform::from_name(id).ok_or(RenderError::InvalidValue {
        key: TRANSFORM,
        error: ValueError::UnknownChoice {
            choices: TRANSFORMS,
        },
    })?;
    Ok(vec![ctx.output()?.transform(transform)])
}

/// `enable`, `disable` or `toggle`, which the config parser files under `enable` with the word
/// as its argument. `toggle` depends on the state at the time.
fn read_enable(args: &[String]) -> Result<Value, ReadError> {
    match read::one(args)? {
        word if word.eq_ignore_ascii_case("enable") => Ok(Value::Bool(true)),
        word if word.eq_ignore_ascii_case("disable") => Ok(Value::Bool(false)),
        word => Err(ReadError::Unsupported(
            Text::new(msg!(
                "{word} depends on whether the display is on at the time"
            ))
            .arg("word", word),
        )),
    }
}

/// `mode` as sway reads it: `<width>x<height>[@<rate>Hz]` or `<width> <height>`. A custom mode
/// (`--custom`) is one the setting doesn't offer.
fn read_mode(args: &[String]) -> Result<Value, ReadError> {
    let rejected = |reason: Msg| ReadError::Rejected(reason.into());
    let first = read::first(args)?;
    if first == "--custom" {
        return Err(ReadError::Unsupported(
            msg!("a custom mode the display doesn't offer").into(),
        ));
    }
    let (width, rest) = read::strtol(first).ok_or_else(|| rejected(msg!("invalid mode width")))?;
    let (height, refresh) = if rest.is_empty() {
        let height = args
            .get(1)
            .ok_or_else(|| rejected(msg!("missing mode height")))?;
        (read::whole(height)?, None)
    } else {
        let rest = rest
            .strip_prefix('x')
            .ok_or_else(|| rejected(msg!("invalid mode width")))?;
        let (height, rest) =
            read::strtol(rest).ok_or_else(|| rejected(msg!("invalid mode height")))?;
        let refresh = if rest.is_empty() {
            None
        } else {
            let rate = rest
                .strip_prefix('@')
                .ok_or_else(|| rejected(msg!("invalid mode height")))?;
            let hertz = rate
                .get(rate.len().saturating_sub(2)..)
                .filter(|unit| unit.eq_ignore_ascii_case("hz"))
                .and_then(|_| rate.get(..rate.len() - 2))
                .and_then(|number| number.parse::<f64>().ok())
                .filter(|hertz| hertz.is_finite())
                .ok_or_else(|| rejected(msg!("invalid mode refresh rate")))?;
            Some(hertz)
        };
        (height, refresh)
    };
    let unsupported = || {
        ReadError::Unsupported(
            Text::new(msg!("a mode the setting can't hold: {mode}")).arg("mode", first),
        )
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked to be within u32 first; sway rounds to millihertz the same way"
    )]
    let millihertz = match refresh {
        Some(hertz) if hertz > 0.0 && hertz * 1000.0 < f64::from(u32::MAX) => {
            Some((hertz * 1000.0).round() as u32)
        }
        Some(_) => return Err(unsupported()),
        None => None,
    };
    let size = |pixels: i64| u32::try_from(pixels).map_err(|_| unsupported());
    let mode = Mode::new(size(width)?, size(height)?, millihertz).map_err(|_| unsupported())?;
    Ok(Value::Text(mode.to_string()))
}

/// `pos` as sway reads it: `<x>,<y>` or `<x> <y>`.
fn read_position(args: &[String]) -> Result<Value, ReadError> {
    let rejected = |reason: Msg| ReadError::Rejected(reason.into());
    let first = read::first(args)?;
    let (x, rest) = read::strtol(first).ok_or_else(|| {
        ReadError::Unsupported(Text::new(msg!("too large: {text}")).arg("text", first))
    })?;
    let y = if rest.is_empty() {
        let y = args
            .get(1)
            .ok_or_else(|| rejected(msg!("missing position argument (y)")))?;
        read::whole(y)?
    } else {
        let y = rest
            .strip_prefix(',')
            .ok_or_else(|| rejected(msg!("invalid position x")))?;
        read::whole(y)?
    };
    Ok(Value::Text(format!("{x},{y}")))
}

/// `scale` as sway reads it with `strtof`: the whole word is a number.
fn read_scale(args: &[String]) -> Result<Value, ReadError> {
    let scale = read::float(read::first(args)?)?;
    if (MIN_SCALE..=MAX_SCALE).contains(&scale) {
        Ok(Value::Decimal(scale))
    } else {
        Err(ReadError::Unsupported(
            Text::new(msg!("a scale outside {min} … {max}: {scale}"))
                .arg("min", MIN_SCALE)
                .arg("max", MAX_SCALE)
                .arg("scale", scale),
        ))
    }
}

/// `transform` as sway reads it, with `strcmp`. A direction after it turns relative to the
/// current transform, which sway refuses in a config file.
fn read_transform(args: &[String]) -> Result<Value, ReadError> {
    let word = read::first(args)?;
    let transform = Transform::from_name(word).ok_or_else(|| {
        ReadError::Rejected(
            Text::new(msg!("invalid output transform: {transform}")).arg("transform", word),
        )
    })?;
    if args
        .get(1)
        .is_some_and(|word| word == "clockwise" || word == "anticlockwise")
    {
        return Err(ReadError::Rejected(
            msg!("relative transforms can't be used in a config file").into(),
        ));
    }
    Ok(Value::Choice(transform.name().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{read_enable, read_mode, read_position, read_scale, read_transform};
    use crate::registry::{ReadError, Value};

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    fn text(text: &str) -> Value {
        Value::Text(text.to_owned())
    }

    #[test]
    fn modes_read_as_sway_reads_them() {
        let read = |words: &[&str]| read_mode(&args(words));
        assert_eq!(read(&["1920x1080"]), Ok(text("1920x1080")));
        assert_eq!(
            read(&["2560x1440@59.951Hz"]),
            Ok(text("2560x1440@59.951Hz"))
        );
        assert_eq!(read(&["1920x1080@60hz"]), Ok(text("1920x1080@60Hz")));
        assert_eq!(read(&["1920x1080@59.9999HZ"]), Ok(text("1920x1080@60Hz")));
        assert_eq!(read(&["1920", "1080"]), Ok(text("1920x1080")));
        // Further subcommands of the line don't count.
        assert_eq!(read(&["1920x1080", "pos", "0", "0"]), Ok(text("1920x1080")));
        assert!(matches!(
            read(&["--custom", "1920x1080"]),
            Err(ReadError::Unsupported(_))
        ));
        for words in [
            &["1920y1080"][..],
            &["1920x1080@60"],
            &["1920x1080fps"],
            &["1920"],
            &["1920", "1080x"],
        ] {
            assert!(
                matches!(read(words), Err(ReadError::Rejected(_))),
                "{words:?}"
            );
        }
        assert!(matches!(read(&["0x1080"]), Err(ReadError::Unsupported(_))));
    }

    #[test]
    fn positions_read_as_sway_reads_them() {
        let read = |words: &[&str]| read_position(&args(words));
        assert_eq!(read(&["1920,0"]), Ok(text("1920,0")));
        assert_eq!(read(&["-1280", "304"]), Ok(text("-1280,304")));
        assert!(matches!(read(&["1920"]), Err(ReadError::Rejected(_))));
        assert!(matches!(read(&["1920;0"]), Err(ReadError::Rejected(_))));
        assert!(matches!(read(&["1920,0x"]), Err(ReadError::Rejected(_))));
    }

    #[test]
    fn scales_transforms_and_switches_read() {
        assert_eq!(read_scale(&args(&["1.5"])), Ok(Value::Decimal(1.5)));
        assert!(matches!(
            read_scale(&args(&["0"])),
            Err(ReadError::Unsupported(_))
        ));
        assert!(matches!(
            read_scale(&args(&["2x"])),
            Err(ReadError::Rejected(_))
        ));
        assert_eq!(
            read_transform(&args(&["0"])),
            Ok(Value::Choice("normal".to_owned()))
        );
        assert_eq!(
            read_transform(&args(&["flipped-90"])),
            Ok(Value::Choice("flipped-90".to_owned()))
        );
        assert!(matches!(
            read_transform(&args(&["90", "clockwise"])),
            Err(ReadError::Rejected(_))
        ));
        assert!(matches!(
            read_transform(&args(&["Normal"])),
            Err(ReadError::Rejected(_))
        ));
        assert_eq!(read_enable(&args(&["disable"])), Ok(Value::Bool(false)));
        assert_eq!(read_enable(&args(&["enable"])), Ok(Value::Bool(true)));
        assert!(matches!(
            read_enable(&args(&["toggle"])),
            Err(ReadError::Unsupported(_))
        ));
    }
}
