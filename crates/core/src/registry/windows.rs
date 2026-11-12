// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Windows page (`man 5 sway`); the SwayFX effects are in `effects`.
//!
//! The colors of a window state (`client.focused`, …) are five settings of one directive, one
//! section each. Defaults are sway's own (`config_defaults` in `sway/config.c`).

use super::read::{self, ReadError};
use super::{Choice, ConfigFile, Context, Directive, Page, RenderError, Setting, Value, When};
use crate::command::window::{
    self, BorderStyle, ClientClass, Gaps, GapsScope, HideEdgeBorders, SmartBorders, SmartGaps,
    TitleAlign, TitleColors, WindowColors,
};
use crate::command::{Color, SwayCmd, Target};

const GAPS_INNER: &str = "windows.gaps.inner";
const GAPS_OUTER: &str = "windows.gaps.outer";
const SMART_GAPS: &str = "windows.smart_gaps";
const FONT: &str = "windows.font";
const TITLE_ALIGN: &str = "windows.title_align";
const TITLEBAR_BORDER: &str = "windows.titlebar.border_thickness";
const TITLEBAR_PADDING_H: &str = "windows.titlebar.padding_horizontal";
const TITLEBAR_PADDING_V: &str = "windows.titlebar.padding_vertical";
const BORDER_STYLE: &str = "windows.border.style";
const BORDER_WIDTH: &str = "windows.border.width";
const FLOATING_BORDER_STYLE: &str = "windows.floating_border.style";
const FLOATING_BORDER_WIDTH: &str = "windows.floating_border.width";
const HIDE_EDGE_BORDERS: &str = "windows.hide_edge_borders";
const HIDE_LONE_TAB: &str = "windows.hide_lone_tab";
const SMART_BORDERS: &str = "windows.smart_borders";
const TAB_TITLE: [&str; 3] = [
    "windows.focused_tab_title.border",
    "windows.focused_tab_title.background",
    "windows.focused_tab_title.text",
];

/// The colors of one window state: its `client.*` directive, section, keys and sway's defaults.
struct State {
    class: ClientClass,
    directive: &'static str,
    section: &'static str,
    group: &'static str,
    keywords: &'static [&'static str],
    keys: [&'static str; 5],
    defaults: [Color; 5],
}

/// The window states, in the order of the page.
const STATES: [State; 4] = [
    State {
        class: ClientClass::Focused,
        directive: "client.focused",
        section: "Focused window",
        group: "windows.focused",
        keywords: &["color", "focus", "active", "theme"],
        keys: [
            "windows.focused.border",
            "windows.focused.background",
            "windows.focused.text",
            "windows.focused.indicator",
            "windows.focused.child_border",
        ],
        defaults: [
            Color::rgb(0x4c, 0x78, 0x99),
            Color::rgb(0x28, 0x55, 0x77),
            Color::rgb(0xff, 0xff, 0xff),
            Color::rgb(0x2e, 0x9e, 0xf4),
            Color::rgb(0x28, 0x55, 0x77),
        ],
    },
    State {
        class: ClientClass::FocusedInactive,
        directive: "client.focused_inactive",
        section: "Inactive window",
        group: "windows.focused_inactive",
        keywords: &["color", "focused inactive", "theme"],
        keys: [
            "windows.focused_inactive.border",
            "windows.focused_inactive.background",
            "windows.focused_inactive.text",
            "windows.focused_inactive.indicator",
            "windows.focused_inactive.child_border",
        ],
        defaults: [
            Color::rgb(0x33, 0x33, 0x33),
            Color::rgb(0x5f, 0x67, 0x6a),
            Color::rgb(0xff, 0xff, 0xff),
            Color::rgb(0x48, 0x4e, 0x50),
            Color::rgb(0x5f, 0x67, 0x6a),
        ],
    },
    State {
        class: ClientClass::Unfocused,
        directive: "client.unfocused",
        section: "Unfocused windows",
        group: "windows.unfocused",
        keywords: &["color", "inactive", "theme"],
        keys: [
            "windows.unfocused.border",
            "windows.unfocused.background",
            "windows.unfocused.text",
            "windows.unfocused.indicator",
            "windows.unfocused.child_border",
        ],
        defaults: [
            Color::rgb(0x33, 0x33, 0x33),
            Color::rgb(0x22, 0x22, 0x22),
            Color::rgb(0x88, 0x88, 0x88),
            Color::rgb(0x29, 0x2d, 0x2e),
            Color::rgb(0x22, 0x22, 0x22),
        ],
    },
    State {
        class: ClientClass::Urgent,
        directive: "client.urgent",
        section: "Urgent window",
        group: "windows.urgent",
        keywords: &["color", "attention", "alert", "theme"],
        keys: [
            "windows.urgent.border",
            "windows.urgent.background",
            "windows.urgent.text",
            "windows.urgent.indicator",
            "windows.urgent.child_border",
        ],
        defaults: [
            Color::rgb(0x2f, 0x34, 0x3a),
            Color::rgb(0x90, 0x00, 0x00),
            Color::rgb(0xff, 0xff, 0xff),
            Color::rgb(0x90, 0x00, 0x00),
            Color::rgb(0x90, 0x00, 0x00),
        ],
    },
];

/// The titles of the five colors of a window state.
const COLOR_TITLES: [&str; 5] = [
    "Title bar border",
    "Title bar background",
    "Title bar text",
    "Split indicator",
    "Border",
];

/// The subtitles of the five colors of a window state.
const COLOR_SUBTITLES: [Option<&str>; 5] = [
    None,
    None,
    None,
    Some("The border edge where the next window opens"),
    Some("The border around the window"),
];

/// The default split indicator of `client.focused_tab_title`, which sway reads but ignores.
const TAB_TITLE_INDICATOR: Color = Color::rgb(0x2e, 0x9e, 0xf4);

/// The commands of window state `S` of [`STATES`].
fn render_state<const S: usize>(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let state = &STATES[S];
    let [border, background, text, indicator, child_border] = state.keys;
    Ok(vec![window::client(
        state.class,
        &WindowColors {
            border: ctx.color(border)?,
            background: ctx.color(background)?,
            text: ctx.color(text)?,
            indicator: ctx.color(indicator)?,
            child_border: ctx.color(child_border)?,
        },
    )])
}

/// Color `C` of a line of window state `S`, as sway reads it.
fn read_state<const S: usize, const C: usize>(args: &[String]) -> Result<Value, ReadError> {
    Ok(Value::Color(client_colors(args, STATES[S].defaults[3])?[C]))
}

/// Color `C` of a `client.focused_tab_title` line, as sway reads it.
fn read_tab_title<const C: usize>(args: &[String]) -> Result<Value, ReadError> {
    Ok(Value::Color(client_colors(args, TAB_TITLE_INDICATOR)?[C]))
}

fn render_tab_title(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    Ok(vec![window::client_focused_tab_title(&TitleColors {
        border: ctx.color(TAB_TITLE[0])?,
        background: ctx.color(TAB_TITLE[1])?,
        text: ctx.color(TAB_TITLE[2])?,
    })])
}

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

/// The settings of window state `S`.
fn state_settings<const S: usize>() -> Vec<Setting> {
    let state = &STATES[S];
    let reads = [
        read_state::<S, 0>,
        read_state::<S, 1>,
        read_state::<S, 2>,
        read_state::<S, 3>,
        read_state::<S, 4>,
    ];
    (0..5)
        .map(|index| {
            let mut builder =
                Setting::color(state.keys[index], Page::Windows, state.defaults[index])
                    .section(state.section)
                    .group(state.group, state.section)
                    .title(COLOR_TITLES[index])
                    .keywords(state.keywords);
            if let Some(subtitle) = COLOR_SUBTITLES[index] {
                builder = builder.subtitle(subtitle);
            }
            builder
                .sway(
                    Directive::top(state.directive),
                    ConfigFile::Appearance,
                    render_state::<S>,
                    reads[index],
                )
                .build()
        })
        .collect()
}

/// The three colors of the focused tab title. Unset, sway draws it like an inactive window.
fn tab_title_settings() -> Vec<Setting> {
    let inactive = &STATES[1].defaults;
    let reads = [
        read_tab_title::<0>,
        read_tab_title::<1>,
        read_tab_title::<2>,
    ];
    (0..3)
        .map(|index| {
            Setting::color(TAB_TITLE[index], Page::Windows, inactive[index])
                .section("Focused tab title")
                .group("windows.focused_tab_title", "Focused tab title")
                .title(COLOR_TITLES[index])
                .subtitle(
                    "The tab or stack title of a container with the focus inside. Unless set, \
                     like an inactive window",
                )
                .keywords(&["color", "tabbed", "stacking", "theme"])
                .sway(
                    Directive::top("client.focused_tab_title"),
                    ConfigFile::Appearance,
                    render_tab_title,
                    reads[index],
                )
                .build()
        })
        .collect()
}

const BORDER_STYLES: &[Choice] = &[
    Choice::new("normal", "Border and title bar"),
    Choice::new("pixel", "Border only"),
    Choice::new("none", "None"),
];

/// The border style of `style_key` with the width of `width_key`.
fn border_style(
    ctx: &Context<'_>,
    style_key: &'static str,
    width_key: &'static str,
) -> Result<BorderStyle, RenderError> {
    Ok(match ctx.choice(style_key)? {
        "none" => BorderStyle::None,
        "pixel" => BorderStyle::Pixel(ctx.integer(width_key)?),
        _ => BorderStyle::Normal(ctx.integer(width_key)?),
    })
}

fn render_border(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    Ok(vec![window::default_border(border_style(
        ctx,
        BORDER_STYLE,
        BORDER_WIDTH,
    )?)])
}

fn render_floating_border(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    Ok(vec![window::default_floating_border(border_style(
        ctx,
        FLOATING_BORDER_STYLE,
        FLOATING_BORDER_WIDTH,
    )?)])
}

/// The style of a `default_border` line: `none`, `normal` or `pixel`, compared as `strcmp`.
fn read_border_style(args: &[String]) -> Result<Value, ReadError> {
    read::choice(read::first(args)?, &["normal", "pixel", "none"], true)
}

/// The width of a `default_border` line: its second argument through `atoi`; a line without
/// one, or with more, leaves sway's default.
fn read_border_width(args: &[String]) -> Result<Value, ReadError> {
    read_border_style(args)?;
    match args {
        [_, width] => Ok(Value::Integer(read::atoi(width)?)),
        _ => Ok(Value::Integer(DEFAULT_BORDER_WIDTH)),
    }
}

/// sway's default border width.
const DEFAULT_BORDER_WIDTH: i64 = 2;

/// The border settings of one window kind: tiled (`default_border`) or floating.
fn border_settings(floating: bool) -> Vec<Setting> {
    let (style_key, width_key, directive, render, title, keywords): (
        _,
        _,
        _,
        super::RenderFn,
        _,
        &'static [&'static str],
    ) = if floating {
        (
            FLOATING_BORDER_STYLE,
            FLOATING_BORDER_WIDTH,
            "default_floating_border",
            render_floating_border,
            ["Floating windows", "Floating border width"],
            &["frame", "decoration", "floating"],
        )
    } else {
        (
            BORDER_STYLE,
            BORDER_WIDTH,
            "default_border",
            render_border,
            ["Tiled windows", "Tiled border width"],
            &["frame", "decoration", "tiling"],
        )
    };
    let (group, group_title) = if floating {
        ("windows.floating_border", "Floating window borders")
    } else {
        ("windows.border", "Tiled window borders")
    };
    vec![
        Setting::choice(style_key, Page::Windows, "normal", BORDER_STYLES)
            .section("Borders")
            .group(group, group_title)
            .title(title[0])
            .subtitle("Applies to windows opened from now on")
            .keywords(keywords)
            .sway(
                Directive::top(directive),
                ConfigFile::Appearance,
                render,
                read_border_style,
            )
            .build(),
        Setting::integer(width_key, Page::Windows, DEFAULT_BORDER_WIDTH, 0, 100)
            .section("Borders")
            .group(group, group_title)
            .title(title[1])
            .subtitle("In pixels, for windows opened from now on")
            .slider(0.0, 20.0, 1.0)
            .keywords(keywords)
            .depends_on(style_key, When::OneOf(&["normal", "pixel"]))
            .sway(
                Directive::top(directive),
                ConfigFile::Appearance,
                render,
                read_border_width,
            )
            .build(),
    ]
}

const HIDE_EDGE_CHOICES: &[Choice] = &[
    Choice::new("none", "Never"),
    Choice::new("vertical", "Left and right"),
    Choice::new("horizontal", "Top and bottom"),
    Choice::new("both", "All sides"),
];

/// A `hide_edge_borders` line as sway reads it: the edges to hide and whether `--i3` hides the
/// title bar of a lone tab. `smart` and `smart_no_gaps` hide no edges here; they turn on smart
/// borders, which is a setting of its own.
fn read_hide_edges(args: &[String]) -> Result<(&'static str, bool), ReadError> {
    let (lone_tab, rest) = match args {
        [flag, rest @ ..] if flag == "--i3" => (true, rest),
        _ => (false, args),
    };
    let edges = match read::first(rest)? {
        "none" | "smart" | "smart_no_gaps" => "none",
        "vertical" => "vertical",
        "horizontal" => "horizontal",
        "both" => "both",
        other => {
            return Err(ReadError::Rejected(format!(
                "expected none, vertical, horizontal, both, smart or smart_no_gaps, got {other}"
            )));
        }
    };
    Ok((edges, lone_tab))
}

fn render_hide_edges(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    let hide = match ctx.choice(HIDE_EDGE_BORDERS)? {
        "vertical" => HideEdgeBorders::Vertical,
        "horizontal" => HideEdgeBorders::Horizontal,
        "both" => HideEdgeBorders::Both,
        _ => HideEdgeBorders::None,
    };
    Ok(vec![window::hide_edge_borders(
        hide,
        ctx.bool(HIDE_LONE_TAB)?,
    )])
}

const SMART_BORDER_CHOICES: &[Choice] = &[
    Choice::new("off", "Always"),
    Choice::new("on", "Only with more than one window"),
    Choice::new("no_gaps", "Only with more than one window or with gaps"),
];

const SMART_GAP_CHOICES: &[Choice] = &[
    Choice::new("off", "Always"),
    Choice::new("on", "Only with more than one window"),
    Choice::new("inverse_outer", "Outer gaps only with one window"),
];

const TITLE_ALIGN_CHOICES: &[Choice] = &[
    Choice::new("left", "Left"),
    Choice::new("center", "Center"),
    Choice::new("right", "Right"),
];

/// `gaps <which>` only sets the default of new workspaces, so a live change also sets the gaps
/// of the open ones.
fn gaps(ctx: &Context<'_>, which: Gaps, key: &'static str) -> Result<Vec<SwayCmd>, RenderError> {
    let px = ctx.integer(key)?;
    let mut commands = vec![window::gaps(which, px)?];
    if ctx.target() == Target::Ipc {
        commands.push(window::gaps_runtime(which, GapsScope::All, px)?);
    }
    Ok(commands)
}

/// A `gaps inner|outer <px>` line: a whole number, optionally with `px`.
fn read_gaps(args: &[String], at_least: i64) -> Result<Value, ReadError> {
    match read::strtol(read::one(args)?) {
        Some((px, rest)) if rest.is_empty() || rest.eq_ignore_ascii_case("px") => {
            Ok(Value::Integer(px.max(at_least)))
        }
        Some(_) => Err(ReadError::Rejected(format!(
            "not a size in pixels: {}",
            args.join(" ")
        ))),
        None => Err(ReadError::Unsupported(format!(
            "too large: {}",
            args.join(" ")
        ))),
    }
}

/// A `titlebar_padding` line: the horizontal padding, and the vertical one or the same again.
fn read_titlebar_padding(args: &[String]) -> Result<(i64, i64), ReadError> {
    let size = |text: &str| match read::whole(text)? {
        px if px >= 0 => Ok(px),
        px => Err(ReadError::Rejected(format!("negative padding: {px}"))),
    };
    let horizontal = size(read::first(args)?)?;
    let vertical = match args.get(1) {
        Some(text) => size(text)?,
        None => horizontal,
    };
    Ok((horizontal, vertical))
}

fn render_titlebar_padding(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    Ok(vec![window::titlebar_padding(
        ctx.integer(TITLEBAR_PADDING_H)?,
        ctx.integer(TITLEBAR_PADDING_V)?,
    )])
}

#[expect(
    clippy::too_many_lines,
    reason = "one builder per setting, read best as a list"
)]
pub(super) fn settings() -> Vec<Setting> {
    let mut settings = state_settings::<0>();
    settings.extend(state_settings::<1>());
    settings.extend(state_settings::<2>());
    settings.extend(state_settings::<3>());
    settings.extend(tab_title_settings());
    settings.extend(border_settings(false));
    settings.extend(border_settings(true));
    settings.extend([
        Setting::choice(HIDE_EDGE_BORDERS, Page::Windows, "none", HIDE_EDGE_CHOICES)
            .section("Borders")
            .group("windows.edge_borders", "Edge borders")
            .title("Hide borders at the screen edge")
            .keywords(&["edges", "hide_edge_borders"])
            .sway(
                Directive::top("hide_edge_borders"),
                ConfigFile::Appearance,
                render_hide_edges,
                |args| Ok(Value::Choice(read_hide_edges(args)?.0.to_owned())),
            )
            .build(),
        Setting::toggle(HIDE_LONE_TAB, Page::Windows, false)
            .section("Borders")
            .group("windows.edge_borders", "Edge borders")
            .title("Hide the title bar of a lone tab")
            .subtitle("A tabbed or stacked container with one window shows no title bar")
            .keywords(&["tabbed", "stacking", "i3"])
            .sway(
                Directive::top("hide_edge_borders"),
                ConfigFile::Appearance,
                render_hide_edges,
                |args| Ok(Value::Bool(read_hide_edges(args)?.1)),
            )
            .build(),
        Setting::choice(SMART_BORDERS, Page::Windows, "off", SMART_BORDER_CHOICES)
            .section("Borders")
            .title("Show borders")
            .keywords(&["smart borders", "single window"])
            .sway(
                Directive::top("smart_borders"),
                ConfigFile::Appearance,
                |ctx| {
                    let smart = match ctx.choice(SMART_BORDERS)? {
                        "on" => SmartBorders::On,
                        "no_gaps" => SmartBorders::NoGaps,
                        _ => SmartBorders::Off,
                    };
                    Ok(vec![window::smart_borders(smart)])
                },
                |args| {
                    let word = read::one(args)?;
                    let id = if word == "no_gaps" {
                        "no_gaps"
                    } else if read::boolean(word, true) {
                        "on"
                    } else {
                        "off"
                    };
                    Ok(Value::Choice(id.to_owned()))
                },
            )
            .build(),
        Setting::integer(GAPS_INNER, Page::Windows, 0, 0, i64::from(i16::MAX))
            .section("Gaps")
            .title("Inner gaps")
            .slider(0.0, 64.0, 1.0)
            .subtitle("Space between windows, in pixels")
            .keywords(&["spacing", "margin"])
            .sway(
                Directive::top("gaps").argument("inner"),
                ConfigFile::Appearance,
                |ctx| gaps(ctx, Gaps::Inner, GAPS_INNER),
                // sway takes a negative inner gap as 0.
                |args| read_gaps(args, 0),
            )
            .build(),
        Setting::integer(
            GAPS_OUTER,
            Page::Windows,
            0,
            i64::from(i16::MIN),
            i64::from(i16::MAX),
        )
        .section("Gaps")
        .title("Outer gaps")
        .slider(0.0, 64.0, 1.0)
        .subtitle(
            "Space around the windows of a workspace, in pixels, added to the inner gaps; \
             negative values take some of them away",
        )
        .keywords(&["spacing", "margin", "padding", "screen edge"])
        .sway(
            Directive::top("gaps").argument("outer"),
            ConfigFile::Appearance,
            |ctx| gaps(ctx, Gaps::Outer, GAPS_OUTER),
            |args| read_gaps(args, i64::from(i16::MIN)),
        )
        .build(),
        Setting::choice(SMART_GAPS, Page::Windows, "off", SMART_GAP_CHOICES)
            .section("Gaps")
            .title("Show gaps")
            .keywords(&["smart gaps", "single window"])
            .sway(
                Directive::top("smart_gaps"),
                ConfigFile::Appearance,
                |ctx| {
                    let smart = match ctx.choice(SMART_GAPS)? {
                        "on" => SmartGaps::On,
                        "inverse_outer" => SmartGaps::InverseOuter,
                        _ => SmartGaps::Off,
                    };
                    Ok(vec![window::smart_gaps(smart)])
                },
                |args| {
                    let word = read::first(args)?;
                    let id = if word == "inverse_outer" {
                        "inverse_outer"
                    } else if read::boolean(word, false) {
                        "on"
                    } else {
                        "off"
                    };
                    Ok(Value::Choice(id.to_owned()))
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
        Setting::choice(TITLE_ALIGN, Page::Windows, "left", TITLE_ALIGN_CHOICES)
            .section("Title bars")
            .title("Title alignment")
            .keywords(&["title", "center"])
            .sway(
                Directive::top("title_align"),
                ConfigFile::Appearance,
                |ctx| {
                    let align = match ctx.choice(TITLE_ALIGN)? {
                        "center" => TitleAlign::Center,
                        "right" => TitleAlign::Right,
                        _ => TitleAlign::Left,
                    };
                    Ok(vec![window::title_align(align)])
                },
                |args| read::choice(read::first(args)?, &["left", "center", "right"], true),
            )
            .build(),
        // Before the padding: sway checks each against the other's value in effect, and a
        // thickness up to the default vertical padding of 4 fits before the padding changes.
        Setting::integer(TITLEBAR_BORDER, Page::Windows, 1, 0, 100)
            .section("Title bars")
            .title("Title bar border")
            .subtitle("In pixels, at most the padding")
            .slider(0.0, 10.0, 1.0)
            .keywords(&["title", "frame"])
            .sway(
                Directive::top("titlebar_border_thickness"),
                ConfigFile::Appearance,
                |ctx| {
                    Ok(vec![window::titlebar_border_thickness(
                        ctx.integer(TITLEBAR_BORDER)?,
                    )])
                },
                |args| match read::whole(read::one(args)?)? {
                    px if px >= 0 => Ok(Value::Integer(px)),
                    px => Err(ReadError::Rejected(format!("negative size: {px}"))),
                },
            )
            .build(),
        Setting::integer(TITLEBAR_PADDING_H, Page::Windows, 5, 0, 100)
            .section("Title bars")
            .group("windows.titlebar.padding", "Title bar padding")
            .title("Title bar padding, left and right")
            .subtitle("In pixels, at least the title bar border")
            .slider(0.0, 20.0, 1.0)
            .keywords(&["title", "spacing"])
            .sway(
                Directive::top("titlebar_padding"),
                ConfigFile::Appearance,
                render_titlebar_padding,
                |args| Ok(Value::Integer(read_titlebar_padding(args)?.0)),
            )
            .build(),
        Setting::integer(TITLEBAR_PADDING_V, Page::Windows, 4, 0, 100)
            .section("Title bars")
            .group("windows.titlebar.padding", "Title bar padding")
            .title("Title bar padding, top and bottom")
            .subtitle("In pixels, at least the title bar border")
            .slider(0.0, 20.0, 1.0)
            .keywords(&["title", "spacing", "height"])
            .sway(
                Directive::top("titlebar_padding"),
                ConfigFile::Appearance,
                render_titlebar_padding,
                |args| Ok(Value::Integer(read_titlebar_padding(args)?.1)),
            )
            .build(),
    ]);
    settings
}
