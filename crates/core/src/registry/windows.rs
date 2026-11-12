// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Windows page (`man 5 sway`); the SwayFX effects are in `effects`.

use super::read::{self, ReadError, ReadFn};
use super::{ConfigFile, Context, Directive, Page, RenderError, Setting, Value};
use crate::command::window::{self, ClientClass, Gaps, GapsScope, WindowColors};
use crate::command::{Color, SwayCmd, Target};

const GAPS_INNER: &str = "windows.gaps.inner";
const FOCUSED_BORDER: &str = "windows.focused.border";
const FOCUSED_BACKGROUND: &str = "windows.focused.background";
const FOCUSED_TEXT: &str = "windows.focused.text";
const FOCUSED_INDICATOR: &str = "windows.focused.indicator";
const FOCUSED_CHILD_BORDER: &str = "windows.focused.child_border";
const FONT: &str = "windows.font";

/// `gaps inner` only sets the default of new workspaces, so a live change also sets the gaps of
/// the open ones.
fn gaps_inner(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let px = ctx.integer(GAPS_INNER)?;
    let mut commands = vec![window::gaps(Gaps::Inner, px)?];
    if ctx.target() == Target::Ipc {
        commands.push(window::gaps_runtime(Gaps::Inner, GapsScope::All, px)?);
    }
    Ok(commands)
}

fn client_focused(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    Ok(vec![window::client(
        ClientClass::Focused,
        &WindowColors {
            border: ctx.color(FOCUSED_BORDER)?,
            background: ctx.color(FOCUSED_BACKGROUND)?,
            text: ctx.color(FOCUSED_TEXT)?,
            indicator: ctx.color(FOCUSED_INDICATOR)?,
            child_border: ctx.color(FOCUSED_CHILD_BORDER)?,
        },
    )])
}

/// The default split indicator of `client.focused`, which sway uses when a line leaves it out.
const FOCUSED_INDICATOR_DEFAULT: Color = Color::rgb(0x2e, 0x9e, 0xf4);

/// The five colors of a `client.*` line as sway reads them: three to five colors; without an
/// indicator the class's default, without a child border the background.
fn client_colors(args: &[String], default_indicator: Color) -> Result<[Color; 5], ReadError> {
    let [border, background, text, rest @ ..] = args else {
        return Err(ReadError::Rejected(format!(
            "expected 3 to 5 colors, got {}",
            args.len()
        )));
    };
    if rest.len() > 2 {
        return Err(ReadError::Rejected(format!(
            "expected 3 to 5 colors, got {}",
            args.len()
        )));
    }
    let background = read::color(background)?;
    let indicator = rest
        .first()
        .map_or(Ok(default_indicator), |hex| read::color(hex))?;
    let child_border = rest.get(1).map_or(Ok(background), |hex| read::color(hex))?;
    Ok([
        read::color(border)?,
        background,
        read::color(text)?,
        indicator,
        child_border,
    ])
}

fn focused(key: &'static str, default: Color, title: &'static str, read: ReadFn) -> Setting {
    Setting::color(key, Page::Windows, default)
        .section("Focused window")
        .group("windows.focused", "Focused window")
        .title(title)
        .keywords(&["color", "focus", "active", "theme"])
        .sway(
            Directive::top("client.focused"),
            ConfigFile::Appearance,
            client_focused,
            read,
        )
        .build()
}

pub(super) fn settings() -> Vec<Setting> {
    vec![
        focused(
            FOCUSED_BORDER,
            Color::rgb(0x4c, 0x78, 0x99),
            "Title bar border",
            |args| {
                Ok(Value::Color(
                    client_colors(args, FOCUSED_INDICATOR_DEFAULT)?[0],
                ))
            },
        ),
        focused(
            FOCUSED_BACKGROUND,
            Color::rgb(0x28, 0x55, 0x77),
            "Title bar background",
            |args| {
                Ok(Value::Color(
                    client_colors(args, FOCUSED_INDICATOR_DEFAULT)?[1],
                ))
            },
        ),
        focused(
            FOCUSED_TEXT,
            Color::rgb(0xff, 0xff, 0xff),
            "Title bar text",
            |args| {
                Ok(Value::Color(
                    client_colors(args, FOCUSED_INDICATOR_DEFAULT)?[2],
                ))
            },
        ),
        focused(
            FOCUSED_INDICATOR,
            FOCUSED_INDICATOR_DEFAULT,
            "Split indicator",
            |args| {
                Ok(Value::Color(
                    client_colors(args, FOCUSED_INDICATOR_DEFAULT)?[3],
                ))
            },
        ),
        focused(
            FOCUSED_CHILD_BORDER,
            Color::rgb(0x28, 0x55, 0x77),
            "Border",
            |args| {
                Ok(Value::Color(
                    client_colors(args, FOCUSED_INDICATOR_DEFAULT)?[4],
                ))
            },
        ),
        Setting::integer(GAPS_INNER, Page::Windows, 0, 0, i64::from(i16::MAX))
            .section("Gaps")
            .title("Inner gaps")
            .slider(0.0, 64.0, 1.0)
            .subtitle("Space between windows, in pixels")
            .keywords(&["spacing", "margin"])
            .sway(
                Directive::top("gaps").argument("inner"),
                ConfigFile::Appearance,
                gaps_inner,
                |args| match read::strtol(read::one(args)?) {
                    // sway takes a negative inner gap as 0.
                    Some((px, rest)) if rest.is_empty() || rest.eq_ignore_ascii_case("px") => {
                        Ok(Value::Integer(px.max(0)))
                    }
                    Some(_) => Err(ReadError::Rejected(format!(
                        "not a size in pixels: {}",
                        args.join(" ")
                    ))),
                    None => Err(ReadError::Unsupported(format!(
                        "too large: {}",
                        args.join(" ")
                    ))),
                },
            )
            .build(),
        Setting::text(FONT, Page::Windows, "monospace 10")
            .section("Title bars")
            .title("Title bar font")
            .subtitle("Font family and size, e.g. \"monospace 10\"")
            .keywords(&["typeface", "text size"])
            .sway(
                Directive::top("font"),
                ConfigFile::Appearance,
                |ctx| Ok(vec![window::font(&ctx.text(FONT)?, false)?]),
                |args| {
                    // sway joins the arguments; `pango:` only turns on markup.
                    let font = args.join(" ");
                    match font.strip_prefix("pango:").unwrap_or(&font) {
                        "" => Err(ReadError::Rejected("expected a font".to_owned())),
                        font => Ok(Value::Text(font.to_owned())),
                    }
                },
            )
            .build(),
    ]
}
