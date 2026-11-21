// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Bar page: which bar runs, where it stands, its modules and its style.
//!
//! Only the blur behind waybar has a sway directive of its own (`layer_effects`, SwayFX). The
//! others become waybar's config and stylesheet, which Sway Things writes to its own directory
//! (handover, see [`crate::bar`]), or the `bar` lines of swaybar in `75-bar.conf`. A fresh bar
//! has the common items (workspaces, window title, volume, network, battery, clock, tray) with
//! their definitions and layout from the `config.jsonc` waybar ships (0.16), and waybar's
//! built-in values where that file sets nothing.

use super::read::{self, ReadError};
use super::{Apply, Choice, ConfigFile, Directive, Font, Page, Scope, Setting, Value, When};
use crate::command::{Color, SwayText, effects};
use crate::msg;
use crate::registry::ValueError;
use crate::version::SwayFxVersion;

/// The key of the bar program.
pub const BACKEND: &str = "bar.backend";
/// The key of the edge the bar stands at.
pub const POSITION: &str = "bar.position";
/// The key of the height.
pub const HEIGHT: &str = "bar.height";
/// The key of when the bar shows: always, or while a key is held.
pub const VISIBILITY: &str = "bar.visibility";
/// The key of the key that shows the bar while held.
pub const MODIFIER: &str = "bar.modifier";
/// The key of the displays that show the bar.
pub const OUTPUTS: &str = "bar.outputs";
/// The key of the layer.
pub const LAYER: &str = "bar.layer";
/// The key of whether the bar reserves its space.
pub const EXCLUSIVE: &str = "bar.exclusive";
/// The key of the margin above the bar.
pub const MARGIN_TOP: &str = "bar.margin_top";
/// The key of the margin right of the bar.
pub const MARGIN_RIGHT: &str = "bar.margin_right";
/// The key of the margin below the bar.
pub const MARGIN_BOTTOM: &str = "bar.margin_bottom";
/// The key of the margin left of the bar.
pub const MARGIN_LEFT: &str = "bar.margin_left";
/// The key of the space between modules.
pub const SPACING: &str = "bar.spacing";
/// The key of the modules on the left (or at the top of a vertical bar).
pub const MODULES_LEFT: &str = "bar.modules_left";
/// The key of the modules in the center.
pub const MODULES_CENTER: &str = "bar.modules_center";
/// The key of the modules on the right (or at the bottom of a vertical bar).
pub const MODULES_RIGHT: &str = "bar.modules_right";
/// The key of the look the style starts from.
pub const THEME: &str = "bar.style.theme";
/// The key of the shape: one bar or a pill per module.
pub const SHAPE: &str = "bar.style.shape";
/// The key of the font.
pub const FONT: &str = "bar.style.font";
/// The key of the room inside each module, left and right.
pub const PADDING: &str = "bar.style.padding";
/// The key of the corner radius.
pub const RADIUS: &str = "bar.style.radius";
/// The key of the opacity of the background.
pub const OPACITY: &str = "bar.style.opacity";
/// The key of the background color.
pub const BACKGROUND: &str = "bar.style.background";
/// The key of the text color.
pub const TEXT: &str = "bar.style.text";
/// The key of the color under the pointer.
pub const HOVER: &str = "bar.style.hover";
/// The key of the color of the focused workspace.
pub const FOCUSED: &str = "bar.style.focused";
/// The key of the warning color.
pub const WARNING: &str = "bar.style.warning";
/// The key of the critical color.
pub const CRITICAL: &str = "bar.style.critical";
/// The key of the color of an urgent workspace.
pub const URGENT: &str = "bar.style.urgent";
/// The key of the user's own stylesheet, imported after ours.
pub const EXTRA_CSS: &str = "bar.style.extra_css";
/// The key of the blur behind waybar (SwayFX).
pub const BLUR: &str = "bar.blur";
/// The key of swaybar's status command.
pub const STATUS_COMMAND: &str = "bar.swaybar.status_command";

/// The layer namespace of waybar's surfaces (waybar's config can rename it with `name`, which
/// Sway Things doesn't set).
pub const WAYBAR_NAMESPACE: &str = "waybar";

/// The bar programs.
pub const BACKENDS: &[Choice] = &[
    Choice::new("none", msg!("None: keep your own bar")),
    Choice::new("waybar", msg!("Waybar")),
    Choice::new("swaybar", msg!("swaybar (built into sway)")),
];

/// When the bar shows.
pub const VISIBILITIES: &[Choice] = &[
    Choice::new("always", msg!("Always")),
    Choice::new("hold", msg!("While a key is held")),
];

/// The keys that can show the bar while held, by sway's names.
pub const MODIFIERS: &[Choice] = &[
    Choice::new("Mod4", msg!("Super (the logo key)")),
    Choice::new("Mod1", msg!("Alt")),
    Choice::new("Control", msg!("Ctrl")),
    Choice::new("Shift", msg!("Shift")),
];

/// The edges a bar stands at.
pub const POSITIONS: &[Choice] = &[
    Choice::new("top", msg!("Top")),
    Choice::new("bottom", msg!("Bottom")),
    Choice::new("left", msg!("Left")),
    Choice::new("right", msg!("Right")),
];

/// The layers of `wlr-layer-shell` a bar can be on.
pub const LAYERS: &[Choice] = &[
    Choice::new("bottom", msg!("Below floating windows")),
    Choice::new("top", msg!("Above floating windows")),
    Choice::new("overlay", msg!("Above everything, also fullscreen")),
];

/// The looks a style starts from.
pub const THEMES: &[Choice] = &[
    Choice::new("waybar", msg!("Waybar's own look")),
    Choice::new("sway", msg!("Same as your windows")),
    Choice::new("custom", msg!("Your own colors")),
    Choice::new("plain", msg!("None: only your own stylesheet")),
];

/// The shapes of a generated style.
pub const SHAPES: &[Choice] = &[
    Choice::new("bar", msg!("Connected bar")),
    Choice::new("pills", msg!("Islands")),
];

/// The items on the left of a fresh bar: the workspaces and the binding mode.
pub const DEFAULT_LEFT: &[&str] = &["sway/workspaces", "sway/mode"];
/// The item in the center of a fresh bar: the window title.
pub const DEFAULT_CENTER: &[&str] = &["sway/window"];
/// The items on the right of a fresh bar: what most people look at, the tray at the end. The
/// battery shows only where there is one.
pub const DEFAULT_RIGHT: &[&str] = &["pulseaudio", "network", "battery", "clock", "tray"];

/// The longest a module name, a font or a path may be.
const MAX_LENGTH: usize = 512;

/// The background waybar's own stylesheet gives the bar, without its transparency.
pub const DEFAULT_BACKGROUND: Color = Color::rgb(0x2b, 0x30, 0x3b);

/// A module's name as a module list takes it: `clock`, `custom/media`, `battery#bat2`.
pub(crate) fn check_module(name: &str) -> Result<(), ValueError> {
    if crate::bar::is_module_name(name) {
        Ok(())
    } else {
        Err(ValueError::NotAModule)
    }
}

/// A font description or nothing (the bar's default font).
fn check_font(font: &str) -> Result<(), ValueError> {
    if font.len() > MAX_LENGTH || font.chars().any(char::is_control) {
        return Err(ValueError::NotAFont);
    }
    Ok(())
}

/// An absolute path to a stylesheet, or nothing.
fn check_path(path: &str) -> Result<(), ValueError> {
    if path.is_empty() {
        return Ok(());
    }
    if !path.starts_with('/') || path.len() > MAX_LENGTH || path.chars().any(char::is_control) {
        return Err(ValueError::NotAPath);
    }
    Ok(())
}

/// A command swaybar can run, or nothing.
fn check_status_command(command: &str) -> Result<(), ValueError> {
    if command.is_empty() {
        return Ok(());
    }
    SwayText::new(command)
        .map(|_| ())
        .map_err(ValueError::NotSwayText)
}

/// `layer_effects waybar blur enable|disable`, with `… blur_ignore_transparent enable` when on:
/// blur behind waybar, but not behind its fully transparent parts (between pills).
fn read_blur(args: &[String]) -> Result<Value, ReadError> {
    // SwayFX reads the state with `parse_boolean(…, true)` (`layer_criteria_parse`).
    Ok(Value::Bool(read::boolean(read::first(args)?, true)))
}

fn bar_section(setting: super::SettingBuilder) -> super::SettingBuilder {
    setting
        .section(msg!("Bar"))
        .apply(Apply::ToolReload)
        .depends_on(BACKEND, When::NoneOf(&["none"]))
}

fn placement(setting: super::SettingBuilder) -> super::SettingBuilder {
    setting
        .section(msg!("Placement"))
        .apply(Apply::ToolReload)
        .depends_on(BACKEND, When::NoneOf(&["none"]))
}

fn waybar_only(setting: super::SettingBuilder, section: super::Msg) -> super::SettingBuilder {
    setting
        .section(section)
        .apply(Apply::ToolReload)
        .depends_on(BACKEND, When::OneOf(&["waybar"]))
}

fn style(setting: super::SettingBuilder) -> super::SettingBuilder {
    setting
        .section(msg!("Look"))
        .apply(Apply::ToolReload)
        .depends_on(THEME, When::NoneOf(&["waybar", "plain"]))
}

fn color(setting: super::SettingBuilder) -> super::SettingBuilder {
    setting
        .section(msg!("Colors"))
        .apply(Apply::ToolReload)
        .depends_on(THEME, When::OneOf(&["custom"]))
}

fn margin(key: &'static str, title: super::Msg) -> Setting {
    waybar_only(
        Setting::integer(key, Page::Bar, 0, 0, 500),
        msg!("Placement"),
    )
    .title(title)
    .slider(0.0, 64.0, 1.0)
    .unit(super::Unit::Pixels)
    .keywords(&[msg!("margin"), msg!("gap"), msg!("floating")])
    .build()
}

fn modules(key: &'static str, default: &'static [&'static str], title: super::Msg) -> Setting {
    waybar_only(Setting::list(key, Page::Bar, default), msg!("Items"))
        .title(title)
        .check_text(check_module)
        .keywords(&[
            msg!("module"),
            msg!("item"),
            msg!("layout"),
            msg!("clock"),
            msg!("battery"),
            msg!("network"),
            msg!("pulseaudio"),
            msg!("wireplumber"),
            msg!("tray"),
            msg!("cpu"),
            msg!("memory"),
            msg!("temperature"),
            msg!("backlight"),
            msg!("bluetooth"),
            msg!("workspaces"),
            msg!("custom"),
            msg!("script"),
        ])
        .build()
}

#[expect(
    clippy::too_many_lines,
    reason = "one builder per setting, read best as a list"
)]
pub(super) fn settings() -> Vec<Setting> {
    let fx = SwayFxVersion::OLDEST_SUPPORTED;
    vec![
        Setting::choice(BACKEND, Page::Bar, "none", BACKENDS)
            .section(msg!("Bar"))
            .title(msg!("Bar program"))
            .subtitle(msg!(
                "Waybar gets a config and stylesheet of its own from Sway Things; yours stay as \
                 they are. swaybar is set up in Sway Things' sway include"
            ))
            .apply(Apply::ToolReload)
            .keywords(&[
                msg!("waybar"),
                msg!("swaybar"),
                msg!("panel"),
                msg!("status bar"),
                msg!("taskbar"),
            ])
            .build(),
        bar_section(Setting::choice(POSITION, Page::Bar, "top", POSITIONS))
            .title(msg!("Position"))
            .subtitle(msg!("swaybar stands only at the top or the bottom"))
            .keywords(&[msg!("edge"), msg!("top"), msg!("bottom"), msg!("vertical")])
            .build(),
        bar_section(Setting::choice(
            VISIBILITY,
            Page::Bar,
            "always",
            VISIBILITIES,
        ))
        .title(msg!("Show the bar"))
        .subtitle(msg!(
            "While a key is held, the bar lies over the windows and leaves them the whole screen \
             otherwise"
        ))
        .keywords(&[
            msg!("hide"),
            msg!("auto-hide"),
            msg!("autohide"),
            msg!("visibility"),
            msg!("modifier"),
        ])
        .build(),
        Setting::choice(MODIFIER, Page::Bar, "Mod4", MODIFIERS)
            .section(msg!("Bar"))
            .title(msg!("Key that shows it"))
            .apply(Apply::ToolReload)
            .depends_on(VISIBILITY, When::OneOf(&["hold"]))
            .keywords(&[msg!("modifier"), msg!("super"), msg!("alt")])
            .build(),
        bar_section(Setting::integer(HEIGHT, Page::Bar, 30, 0, 500))
            .title(msg!("Height"))
            .subtitle(msg!("0 fits the bar to its items"))
            .slider(0.0, 80.0, 1.0)
            .unit(super::Unit::Pixels)
            .keywords(&[
                msg!("size"),
                msg!("thickness"),
                msg!("width"),
                msg!("bigger"),
            ])
            .build(),
        waybar_only(
            Setting::choice(LAYER, Page::Bar, "bottom", LAYERS),
            msg!("Bar"),
        )
        .title(msg!("Layer"))
        .subtitle(msg!(
            "SwayFX blurs behind the bar only above floating windows or everything"
        ))
        .keywords(&[msg!("overlay"), msg!("above"), msg!("on top")])
        .build(),
        modules(MODULES_LEFT, DEFAULT_LEFT, msg!("Left")),
        modules(MODULES_CENTER, DEFAULT_CENTER, msg!("Center")),
        modules(MODULES_RIGHT, DEFAULT_RIGHT, msg!("Right")),
        Setting::choice(THEME, Page::Bar, "waybar", THEMES)
            .section(msg!("Look"))
            .title(msg!("Colors"))
            .subtitle(msg!(
                "Waybar's own look, the colors of your windows, or colors of your own; the rows \
                 below apply to the last two"
            ))
            .apply(Apply::ToolReload)
            .depends_on(BACKEND, When::NoneOf(&["none"]))
            .keywords(&[
                msg!("theme"),
                msg!("css"),
                msg!("stylesheet"),
                msg!("preset"),
                msg!("colors"),
                msg!("look"),
            ])
            .build(),
        style(Setting::choice(SHAPE, Page::Bar, "bar", SHAPES))
            .title(msg!("Style"))
            .subtitle(msg!("Islands leave the bar itself transparent"))
            .keywords(&[
                msg!("pill"),
                msg!("floating"),
                msg!("island"),
                msg!("shape"),
            ])
            .build(),
        style(Setting::decimal(OPACITY, Page::Bar, 1.0, 0.0, 1.0))
            .title(msg!("Opacity"))
            .subtitle(msg!("Below 1 your wallpaper shows through"))
            .slider(0.0, 1.0, 0.05)
            .keywords(&[msg!("transparency"), msg!("transparent"), msg!("alpha")])
            .build(),
        style(Setting::integer(RADIUS, Page::Bar, 0, 0, 999))
            .title(msg!("Rounded corners"))
            .slider(0.0, 32.0, 1.0)
            .unit(super::Unit::Pixels)
            .keywords(&[msg!("radius"), msg!("round")])
            .build(),
        style(Setting::integer(PADDING, Page::Bar, 10, 0, 100))
            .title(msg!("Room inside items"))
            .subtitle(msg!("Left and right of each item's text"))
            .slider(0.0, 32.0, 1.0)
            .unit(super::Unit::Pixels)
            .keywords(&[msg!("padding"), msg!("spacing"), msg!("room")])
            .build(),
        Setting::text(FONT, Page::Bar, "")
            .section(msg!("Look"))
            .title(msg!("Font"))
            .subtitle(msg!(
                "Also with waybar's own look; the size buttons above set its size"
            ))
            .placeholder(msg!("The default font"))
            .font(Font::Any)
            .check_text(check_font)
            .apply(Apply::ToolReload)
            .depends_on(THEME, When::NoneOf(&["plain"]))
            .keywords(&[msg!("font size"), msg!("typeface"), msg!("text size")])
            .build(),
        Setting::toggle(BLUR, Page::Bar, false)
            .section(msg!("Look"))
            .title(msg!("Blur behind the bar"))
            .subtitle(msg!(
                "Blurs what shows through a transparent background, not between islands"
            ))
            .keywords(&[msg!("frosted glass"), msg!("transparency")])
            .swayfx_only(fx)
            .depends_on(LAYER, When::OneOf(&["top", "overlay"]))
            .sway(
                Directive::top("layer_effects").arguments(&[WAYBAR_NAMESPACE, "blur"]),
                ConfigFile::Effects,
                |ctx| {
                    let on = ctx.bool(BLUR)?;
                    let mut commands = vec![effects::layer_blur(WAYBAR_NAMESPACE, on)];
                    // Never `blur_ignore_transparent disable`: SwayFX 0.6 (scenefx 0.5) crashes
                    // at the next frame when a mapped layer surface loses it, and a config line
                    // reaches the mapped surfaces too when sway reloads. With blur off it changes
                    // nothing, so it stays as it was.
                    if on {
                        commands.push(effects::layer_blur_ignore_transparent(
                            WAYBAR_NAMESPACE,
                            true,
                        ));
                    }
                    Ok(commands)
                },
                read_blur,
            )
            .build(),
        color(Setting::color(BACKGROUND, Page::Bar, DEFAULT_BACKGROUND))
            .title(msg!("Background"))
            .keywords(&[msg!("color")])
            .build(),
        color(Setting::color(
            TEXT,
            Page::Bar,
            Color::rgb(0xff, 0xff, 0xff),
        ))
        .title(msg!("Text"))
        .keywords(&[msg!("color"), msg!("foreground"), msg!("contrast")])
        .build(),
        color(Setting::color(HOVER, Page::Bar, Color::rgba(0, 0, 0, 0x33)))
            .title(msg!("Under the pointer"))
            .keywords(&[msg!("color"), msg!("hover")])
            .build(),
        color(Setting::color(
            FOCUSED,
            Page::Bar,
            Color::rgb(0x64, 0x72, 0x7d),
        ))
        .title(msg!("Focused workspace"))
        .keywords(&[msg!("color"), msg!("active"), msg!("accent")])
        .build(),
        color(Setting::color(
            WARNING,
            Page::Bar,
            Color::rgb(0xf0, 0x93, 0x2b),
        ))
        .title(msg!("Warning"))
        .subtitle(msg!("A battery running low, a hot CPU"))
        .keywords(&[msg!("color")])
        .build(),
        color(Setting::color(
            CRITICAL,
            Page::Bar,
            Color::rgb(0xf5, 0x3c, 0x3c),
        ))
        .title(msg!("Critical"))
        .keywords(&[msg!("color")])
        .build(),
        color(Setting::color(
            URGENT,
            Page::Bar,
            Color::rgb(0xeb, 0x4d, 0x4b),
        ))
        .title(msg!("Urgent workspace"))
        .keywords(&[msg!("color")])
        .build(),
        placement(Setting::list(OUTPUTS, Page::Bar, &[]))
            .title(msg!("Displays"))
            .subtitle(msg!(
                "The displays that show the bar; none picked means all"
            ))
            .scope(Scope::PerHost)
            .check_text(check_output_name)
            .keywords(&[msg!("monitor"), msg!("screen"), msg!("output")])
            .build(),
        waybar_only(
            Setting::toggle(EXCLUSIVE, Page::Bar, true),
            msg!("Placement"),
        )
        .title(msg!("Reserve space"))
        .subtitle(msg!(
            "Windows stay clear of the bar; off, the bar lies over them"
        ))
        .keywords(&[msg!("exclusive zone"), msg!("overlap")])
        .build(),
        margin(MARGIN_TOP, msg!("Margin above")),
        margin(MARGIN_RIGHT, msg!("Margin right")),
        margin(MARGIN_BOTTOM, msg!("Margin below")),
        margin(MARGIN_LEFT, msg!("Margin left")),
        waybar_only(
            Setting::integer(SPACING, Page::Bar, 4, 0, 200),
            msg!("Placement"),
        )
        .title(msg!("Space between items"))
        .slider(0.0, 32.0, 1.0)
        .unit(super::Unit::Pixels)
        .keywords(&[msg!("spacing"), msg!("gap")])
        .build(),
        waybar_only(Setting::text(EXTRA_CSS, Page::Bar, ""), msg!("Advanced"))
            .title(msg!("Your own stylesheet"))
            .subtitle(msg!(
                "A CSS file read after Sway Things' style, so its rules win; it stays yours and \
                 isn't checked"
            ))
            .placeholder(msg!("None"))
            .check_text(check_path)
            .keywords(&[msg!("css"), msg!("style.css"), msg!("import")])
            .build(),
        Setting::text(STATUS_COMMAND, Page::Bar, "")
            .section(msg!("Advanced"))
            .title(msg!("Status command"))
            .subtitle(msg!(
                "swaybar only: its output is the text at the right end; sway runs it with sh"
            ))
            .placeholder(msg!("None"))
            .apply(Apply::ToolReload)
            .depends_on(BACKEND, When::OneOf(&["swaybar"]))
            .check_text(check_status_command)
            .keywords(&[msg!("i3status"), msg!("i3blocks"), msg!("status_command")])
            .build(),
    ]
}

/// An output's name or identifier as waybar's `output` and swaybar's `output` take it.
fn check_output_name(name: &str) -> Result<(), ValueError> {
    SwayText::new(name)
        .map(|_| ())
        .map_err(ValueError::NotSwayText)
}
