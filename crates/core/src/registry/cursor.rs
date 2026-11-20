// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Cursor & Tablet page (`man 5 sway-input`): the cursor theme and size and when
//! the cursor hides, for every seat (`seat *`), then where drawing tablets and touchscreens point.
//!
//! The theme and size are also copied to GSettings, which GTK apps read instead of asking sway.

use super::look::INTERFACE;
use super::read::{self, ReadError};
use super::{ConfigFile, Context, Directive, Page, RenderError, Scope, Setting, Unit, Value, When};
use crate::command::input::{InputType, MapTo, ToolMode};
use crate::command::{SwayCmd, SwayText, Target, seat};
use crate::i18n::Text;
use crate::msg;
use crate::registry::{Choice, ValueError};

/// The cursor size sway uses unless told otherwise.
pub const DEFAULT_SIZE: u32 = 24;

/// The name of sway's own default theme: with no `xcursor_theme`, wlroots loads the theme called
/// `default` (often a link to the distribution's theme), and falls back to its built-in cursor.
pub const DEFAULT_THEME: &str = "default";

const THEME: &str = "cursor.theme";
const SIZE: &str = "cursor.size";
const HIDE: &str = "cursor.hide";
const HIDE_AFTER: &str = "cursor.hide_after";
const HIDE_WHEN_TYPING: &str = "cursor.hide_when_typing";
const TABLET_MAP: &str = "input.tablet_tool.map_to_output";
const TABLET_TOOL_MODE: &str = "input.tablet_tool.tool_mode";
const TOUCH_MAP: &str = "input.touch.map_to_output";

/// The seconds of inactivity before the cursor hides, unless set.
const DEFAULT_HIDE_AFTER: f64 = 5.0;

const TOOL_MODES: &[Choice] = &[
    Choice::new("absolute", msg!("Like a pen on paper")),
    Choice::new("relative", msg!("Like a mouse")),
];

/// The value of a "Map to" setting that leaves the mapping to sway: a built-in device to the
/// built-in screen, others to the whole desktop.
pub const MAP_AUTOMATIC: &str = "";
/// The value of a "Map to" setting for the whole desktop.
pub const MAP_DESKTOP: &str = "*";

/// A cursor theme name sway and GSettings can take.
fn check_theme(theme: &str) -> Result<(), ValueError> {
    SwayText::new(theme)
        .map(|_| ())
        .map_err(ValueError::NotSwayText)
}

/// An output name a mapping can name: empty (automatic), `*` or text sway can take.
fn check_output(output: &str) -> Result<(), ValueError> {
    if output.is_empty() {
        return Ok(());
    }
    SwayText::new(output)
        .map(|_| ())
        .map_err(ValueError::NotSwayText)
}

fn xcursor_theme(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let theme = SwayText::new(ctx.text(THEME)?).map_err(|error| RenderError::InvalidValue {
        key: THEME,
        error: ValueError::NotSwayText(error),
    })?;
    Ok(vec![seat::xcursor_theme(&theme, ctx.integer(SIZE)?)?])
}

/// An `xcursor_theme <theme> [<size>]` line: the size is read with `strtoul` and must end there.
fn read_xcursor_theme(args: &[String]) -> Result<(String, i64), ReadError> {
    let (theme, size) = match args {
        [theme] => (theme, i64::from(DEFAULT_SIZE)),
        [theme, size] => (theme, read::whole(size)?),
        _ => {
            return Err(ReadError::Rejected(
                Text::new(msg!("expected a theme and a size, got {count} arguments"))
                    .arg("count", args.len()),
            ));
        }
    };
    Ok((theme.clone(), size))
}

fn hide_cursor(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let milliseconds = if ctx.bool(HIDE)? {
        let seconds = ctx.decimal(HIDE_AFTER)?;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the setting holds 0.1 to 3600 seconds"
        )]
        let milliseconds = (seconds * 1000.0).round() as u32;
        milliseconds.max(seat::MIN_HIDE_TIMEOUT_MS)
    } else {
        0
    };
    Ok(vec![seat::hide_cursor(milliseconds)?])
}

/// A `hide_cursor <ms>` line: 0 never hides, sway raises other timeouts below 100 ms to 100.
fn read_hide_cursor(args: &[String]) -> Result<Option<f64>, ReadError> {
    match read::whole(read::one(args)?)? {
        0 => Ok(None),
        #[expect(
            clippy::cast_precision_loss,
            reason = "milliseconds that fit an int are exact"
        )]
        ms if ms > 0 => Ok(Some(ms.max(100) as f64 / 1000.0)),
        ms => Err(ReadError::Unsupported(
            Text::new(msg!("a negative timeout: {ms}")).arg("ms", ms),
        )),
    }
}

/// The "Map to" settings: automatic renders no config line, since sway maps by itself then, but
/// over IPC it maps to the whole desktop to end an earlier mapping (sway re-applies the built-in
/// screen of a built-in device at the next reload).
fn map_to_output(ctx: &Context<'_>, key: &'static str) -> Result<Vec<SwayCmd>, RenderError> {
    let target = match ctx.text(key)?.as_str() {
        MAP_AUTOMATIC if ctx.target() == Target::Config => return Ok(Vec::new()),
        MAP_AUTOMATIC | MAP_DESKTOP => MapTo::Desktop,
        output => {
            MapTo::Output(
                SwayText::new(output).map_err(|error| RenderError::InvalidValue {
                    key,
                    error: ValueError::NotSwayText(error),
                })?,
            )
        }
    };
    Ok(vec![ctx.input()?.map_to_output(&target)])
}

fn read_map_to_output(args: &[String]) -> Result<Value, ReadError> {
    Ok(Value::Text(read::one(args)?.to_owned()))
}

pub(super) fn settings() -> Vec<Setting> {
    let mut settings = cursor();
    settings.extend(tablets());
    settings
}

#[expect(
    clippy::too_many_lines,
    reason = "one builder per setting, read best as a list"
)]
fn cursor() -> Vec<Setting> {
    let theme_directive = Directive::seat("xcursor_theme");
    let hide_directive = Directive::seat("hide_cursor").except(&["when-typing"]);
    let group = ("cursor.xcursor_theme", msg!("Cursor theme"));
    let hide_group = ("cursor.hide_cursor", msg!("Hide after inactivity"));
    vec![
        Setting::text(THEME, Page::CursorTablet, DEFAULT_THEME)
            .section(msg!("Cursor"))
            .group(group.0, group.1)
            .title(msg!("Theme"))
            .subtitle(msg!(
                "Also for GTK apps; other apps get it when they start again"
            ))
            .keywords(&[
                msg!("cursor"),
                msg!("pointer"),
                msg!("mouse pointer"),
                msg!("xcursor"),
                msg!("arrow"),
            ])
            .check_text(check_theme)
            .gsettings(INTERFACE, "cursor-theme")
            .sway(theme_directive, ConfigFile::Input, xcursor_theme, |args| {
                Ok(Value::Text(read_xcursor_theme(args)?.0))
            })
            .build(),
        Setting::integer(
            SIZE,
            Page::CursorTablet,
            i64::from(DEFAULT_SIZE),
            i64::from(seat::MIN_CURSOR_SIZE),
            i64::from(seat::MAX_CURSOR_SIZE),
        )
        .section(msg!("Cursor"))
        .group(group.0, group.1)
        .title(msg!("Size"))
        .subtitle(msg!("The sizes the theme comes in"))
        .slider(16.0, 128.0, 8.0)
        .unit(Unit::Pixels)
        .keywords(&[
            msg!("cursor"),
            msg!("pointer"),
            msg!("large cursor"),
            msg!("bigger"),
            msg!("accessibility"),
        ])
        .gsettings(INTERFACE, "cursor-size")
        .sway(theme_directive, ConfigFile::Input, xcursor_theme, |args| {
            Ok(Value::Integer(read_xcursor_theme(args)?.1))
        })
        .build(),
        Setting::toggle(HIDE, Page::CursorTablet, false)
            .section(msg!("Cursor"))
            .group(hide_group.0, hide_group.1)
            .title(msg!("Hide after inactivity"))
            .subtitle(msg!("Hides the cursor while the mouse doesn't move"))
            .keywords(&[
                msg!("cursor"),
                msg!("pointer"),
                msg!("hide"),
                msg!("timeout"),
                msg!("unclutter"),
            ])
            .sway(hide_directive, ConfigFile::Input, hide_cursor, |args| {
                Ok(Value::Bool(read_hide_cursor(args)?.is_some()))
            })
            .build(),
        Setting::decimal(
            HIDE_AFTER,
            Page::CursorTablet,
            DEFAULT_HIDE_AFTER,
            0.1,
            3600.0,
        )
        .section(msg!("Cursor"))
        .group(hide_group.0, hide_group.1)
        .title(msg!("Seconds"))
        .subtitle(msg!(
            "How long the mouse doesn't move before the cursor hides"
        ))
        .slider(1.0, 30.0, 0.5)
        .unit(Unit::Seconds)
        .keywords(&[
            msg!("cursor"),
            msg!("pointer"),
            msg!("hide"),
            msg!("timeout"),
        ])
        .depends_on(HIDE, When::On)
        .sway(hide_directive, ConfigFile::Input, hide_cursor, |args| {
            Ok(Value::Decimal(
                read_hide_cursor(args)?.unwrap_or(DEFAULT_HIDE_AFTER),
            ))
        })
        .build(),
        Setting::toggle(HIDE_WHEN_TYPING, Page::CursorTablet, false)
            .section(msg!("Cursor"))
            .title(msg!("Hide while typing"))
            .subtitle(msg!(
                "Hides the cursor when a key is pressed, until the mouse moves"
            ))
            .keywords(&[
                msg!("cursor"),
                msg!("pointer"),
                msg!("hide"),
                msg!("keyboard"),
                msg!("typing"),
            ])
            .sway(
                Directive::seat("hide_cursor").arguments(&["when-typing"]),
                ConfigFile::Input,
                |ctx| {
                    Ok(vec![seat::hide_cursor_when_typing(
                        ctx.bool(HIDE_WHEN_TYPING)?,
                    )])
                },
                |args| Ok(Value::Bool(read::boolean(read::one(args)?, true))),
            )
            .build(),
    ]
}

fn tablets() -> Vec<Setting> {
    vec![
        Setting::text(TABLET_MAP, Page::CursorTablet, MAP_AUTOMATIC)
            .section(msg!("Drawing tablet"))
            .title(msg!("Screen"))
            .subtitle(msg!("The screen the tablet's surface covers"))
            .keywords(&[
                msg!("tablet"),
                msg!("wacom"),
                msg!("pen"),
                msg!("stylus"),
                msg!("map to output"),
                msg!("monitor"),
            ])
            .scope(Scope::PerDevice(InputType::TabletTool))
            .host_by_default()
            .check_text(check_output)
            .sway(
                Directive::input("map_to_output"),
                ConfigFile::Input,
                |ctx| map_to_output(ctx, TABLET_MAP),
                read_map_to_output,
            )
            .build(),
        Setting::choice(TABLET_TOOL_MODE, Page::CursorTablet, "absolute", TOOL_MODES)
            .section(msg!("Drawing tablet"))
            .title(msg!("Pen moves the pointer"))
            .subtitle(msg!(
                "Like a pen points at a spot on the screen, or like a mouse pushes the pointer"
            ))
            .keywords(&[
                msg!("tablet"),
                msg!("wacom"),
                msg!("pen"),
                msg!("stylus"),
                msg!("tool mode"),
                msg!("absolute"),
                msg!("relative"),
            ])
            .scope(Scope::PerDevice(InputType::TabletTool))
            .sway(
                Directive::input("tool_mode").arguments(&["*"]),
                ConfigFile::Input,
                |ctx| {
                    let mode = match ctx.choice(TABLET_TOOL_MODE)? {
                        "relative" => ToolMode::Relative,
                        _ => ToolMode::Absolute,
                    };
                    Ok(vec![ctx.input()?.tool_mode(mode)])
                },
                |args| read::choice(read::one(args)?, &["absolute", "relative"], false),
            )
            .build(),
        Setting::text(TOUCH_MAP, Page::CursorTablet, MAP_AUTOMATIC)
            .section(msg!("Touchscreen"))
            .title(msg!("Screen"))
            .subtitle(msg!("The screen your touches land on"))
            .keywords(&[
                msg!("touchscreen"),
                msg!("touch"),
                msg!("map to output"),
                msg!("monitor"),
            ])
            .scope(Scope::PerDevice(InputType::Touch))
            .host_by_default()
            .check_text(check_output)
            .sway(
                Directive::input("map_to_output"),
                ConfigFile::Input,
                |ctx| map_to_output(ctx, TOUCH_MAP),
                read_map_to_output,
            )
            .build(),
    ]
}
