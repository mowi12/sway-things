// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Windows page (`man 5 sway`); the SwayFX effects are in `effects`.

use super::{ConfigFile, Context, Directive, Page, RenderError, Setting};
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

fn focused(key: &'static str, default: Color, title: &'static str) -> Setting {
    Setting::color(key, Page::Windows, default)
        .title(title)
        .keywords(&["color", "focus", "active", "theme"])
        .sway(
            Directive::top("client.focused"),
            ConfigFile::Appearance,
            client_focused,
        )
        .build()
}

pub(super) fn settings() -> Vec<Setting> {
    vec![
        focused(
            FOCUSED_BORDER,
            Color::rgb(0x4c, 0x78, 0x99),
            "Focused window: title bar border",
        ),
        focused(
            FOCUSED_BACKGROUND,
            Color::rgb(0x28, 0x55, 0x77),
            "Focused window: title bar background",
        ),
        focused(
            FOCUSED_TEXT,
            Color::rgb(0xff, 0xff, 0xff),
            "Focused window: title bar text",
        ),
        focused(
            FOCUSED_INDICATOR,
            Color::rgb(0x2e, 0x9e, 0xf4),
            "Focused window: split indicator",
        ),
        focused(
            FOCUSED_CHILD_BORDER,
            Color::rgb(0x28, 0x55, 0x77),
            "Focused window: border",
        ),
        Setting::integer(GAPS_INNER, Page::Windows, 0, 0, i64::from(i16::MAX))
            .title("Inner gaps")
            .subtitle("Space between windows, in pixels")
            .keywords(&["spacing", "margin"])
            .sway(
                Directive::top("gaps").argument("inner"),
                ConfigFile::Appearance,
                gaps_inner,
            )
            .build(),
        Setting::text(FONT, Page::Windows, "monospace 10")
            .title("Title bar font")
            .subtitle("Font family and size, e.g. \"monospace 10\"")
            .keywords(&["typeface", "text size"])
            .sway(Directive::top("font"), ConfigFile::Appearance, |ctx| {
                Ok(vec![window::font(&ctx.text(FONT)?, false)?])
            })
            .build(),
    ]
}
