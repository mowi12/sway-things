// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The stylesheet of waybar, made from the style settings, and the presets of the page.
//!
//! waybar draws with GTK 3, so the rules are GTK 3 CSS. Every value comes from a checked setting
//! (numbers, colors) or is quoted and escaped (the font family, the URL of the user's stylesheet),
//! so a value can't end a rule or start another. Each module is a child of a `widget` in one of
//! the boxes `.modules-left`, `.modules-center` and `.modules-right`, which is how one rule
//! reaches every module; workspaces are buttons in `#workspaces`.

use std::fmt::Write as _;

use super::value;
use crate::command::Color;
use crate::i18n::Msg;
use crate::msg;
use crate::registry::{Registry, Value, bar as keys, windows};
use crate::state::State;
use crate::sway_config::SwayConfig;

/// The colors of a generated stylesheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    /// The bar's background, or the pills' with that shape.
    pub background: Color,
    /// The text.
    pub text: Color,
    /// Under the pointer.
    pub hover: Color,
    /// The focused workspace and the binding mode.
    pub focused: Color,
    /// A battery running low, a disconnected network.
    pub warning: Color,
    /// Critical states.
    pub critical: Color,
    /// An urgent workspace.
    pub urgent: Color,
}

impl Colors {
    /// The colors of the style settings.
    pub fn custom(state: &State) -> Self {
        let color = |key| match value(state, key) {
            Value::Color(color) => color,
            _ => Color::rgb(0, 0, 0),
        };
        Self {
            background: color(keys::BACKGROUND),
            text: color(keys::TEXT),
            hover: color(keys::HOVER),
            focused: color(keys::FOCUSED),
            warning: color(keys::WARNING),
            critical: color(keys::CRITICAL),
            urgent: color(keys::URGENT),
        }
    }

    /// The colors of sway's windows: the background of unfocused title bars, the text of the
    /// focused one, the focused title bar for the focused workspace, focused-inactive ones under
    /// the pointer, the urgent one for urgent and critical states. Each as a row of the Windows
    /// page shows it: stored, else the last line of the user's config `sway` that sets it, else
    /// sway's default.
    pub fn sway(state: &State, sway: Option<&SwayConfig>) -> Self {
        let color = |key: &str| {
            let registry = Registry::builtin();
            let stored = state.get(key, None).cloned();
            let from_config = || {
                let setting = registry.get(key)?;
                sway?
                    .directives
                    .iter()
                    .rev()
                    .find_map(|directive| setting.read(directive).and_then(Result::ok))
            };
            let value = stored.or_else(from_config).or_else(|| {
                registry
                    .get(key)
                    .map(crate::registry::Setting::default_value)
            });
            match value {
                Some(Value::Color(color)) => color,
                _ => Color::rgb(0, 0, 0),
            }
        };
        Self {
            background: color(windows::UNFOCUSED_BACKGROUND),
            text: color(windows::FOCUSED_TEXT),
            hover: color(windows::FOCUSED_INACTIVE_BACKGROUND),
            focused: color(windows::FOCUSED_BACKGROUND),
            warning: Color::rgb(0xf0, 0x93, 0x2b),
            critical: color(windows::URGENT_BACKGROUND),
            urgent: color(windows::URGENT_BACKGROUND),
        }
    }
}

/// `color` as GTK 3 CSS, with `opacity` multiplied into its alpha.
pub fn css_color(color: Color, opacity: f64) -> String {
    let [red, green, blue] = color.channels();
    let alpha = f64::from(color.alpha().unwrap_or(0xff)) / 255.0 * opacity.clamp(0.0, 1.0);
    format!("rgba({red}, {green}, {blue}, {alpha:.3})")
}

/// `text` as a CSS string in double quotes.
pub fn css_string(text: &str) -> String {
    let mut out = String::from("\"");
    for char in text.chars() {
        match char {
            '"' | '\\' => {
                out.push('\\');
                out.push(char);
            }
            // A line break or another control character can't be in a CSS string; the
            // setting's check keeps them out, and they are dropped here too.
            control if control.is_control() => {}
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// The `file://` URL of the absolute `path`, with the bytes a URL can't hold percent-encoded, or
/// `None` for a path that isn't absolute or has a control character.
pub fn file_url(path: &str) -> Option<String> {
    if !path.starts_with('/') || path.chars().any(char::is_control) {
        return None;
    }
    let mut url = String::from("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            url.push(char::from(byte));
        } else {
            let _ = write!(url, "%{byte:02X}");
        }
    }
    Some(url)
}

/// A Pango font description split for CSS: the family, the weight, whether italic, and the
/// size with its unit (`pt`, or `px` for an absolute size).
#[derive(Debug, Clone, PartialEq)]
pub struct CssFont {
    /// The family, e.g. `Noto Sans`.
    pub family: String,
    /// The weight, 100 … 900, if the description names one.
    pub weight: Option<u16>,
    /// Whether it names an italic or oblique style.
    pub italic: bool,
    /// The size and its unit, e.g. `(12.0, "pt")`.
    pub size: Option<(f64, &'static str)>,
}

/// Splits a Pango font description such as `Noto Sans Bold 11` or `Mono 12px`: the size at the
/// end, style and weight words before it, the rest the family.
pub fn css_font(description: &str) -> Option<CssFont> {
    let mut words: Vec<&str> = description.split_whitespace().collect();
    let mut size = None;
    if let Some(last) = words.last() {
        let (number, unit) = match last.strip_suffix("px") {
            Some(number) => (number, "px"),
            None => (*last, "pt"),
        };
        if let Ok(number) = number.parse::<f64>() {
            if number.is_finite() && number > 0.0 && number <= 1000.0 {
                size = Some((number, unit));
                words.pop();
            }
        }
    }
    let mut weight = None;
    let mut italic = false;
    while let Some(word) = words.last() {
        let found = match word.to_ascii_lowercase().as_str() {
            "thin" => Some(100),
            "ultra-light" | "extra-light" => Some(200),
            "light" => Some(300),
            "book" | "regular" | "normal" => Some(400),
            "medium" => Some(500),
            "semi-bold" | "demi-bold" => Some(600),
            "bold" => Some(700),
            "ultra-bold" | "extra-bold" => Some(800),
            "heavy" | "black" => Some(900),
            "italic" | "oblique" => {
                italic = true;
                None
            }
            _ => break,
        };
        if found.is_some() {
            weight = found;
        }
        words.pop();
    }
    let family = words.join(" ").trim_end_matches(',').to_owned();
    (!family.is_empty() || size.is_some()).then_some(CssFont {
        family,
        weight,
        italic,
        size,
    })
}

/// The stylesheet for the style settings of `state`: waybar's own, or one made from the shape,
/// font, padding, corners, opacity and the colors (sway's or the user's own; `sway` is the
/// user's sway config, for the colors it sets).
pub fn stylesheet(state: &State, sway: Option<&SwayConfig>) -> String {
    let theme = match value(state, keys::THEME) {
        Value::Choice(id) => id,
        _ => String::new(),
    };
    let font = match value(state, keys::FONT) {
        Value::Text(font) => css_font(&font),
        _ => None,
    };
    let colors = match theme.as_str() {
        "waybar" => {
            // waybar's own look, with the font of the Bar page after it, so it wins.
            let mut css = super::modules::WAYBAR_DEFAULT_STYLE.to_owned();
            if let Some(font) = &font {
                let _ = write!(css, "\n/* The font of the Bar page */\n{}", font_rule(font));
            }
            return css;
        }
        "plain" => return String::new(),
        "sway" => Colors::sway(state, sway),
        _ => Colors::custom(state),
    };
    let integer = |key| match value(state, key) {
        Value::Integer(number) => number,
        _ => 0,
    };
    let opacity = match value(state, keys::OPACITY) {
        Value::Decimal(opacity) => opacity,
        _ => 1.0,
    };
    let pills = matches!(value(state, keys::SHAPE), Value::Choice(id) if id == "pills");
    generated(&Look {
        colors,
        opacity,
        pills,
        padding: integer(keys::PADDING),
        radius: integer(keys::RADIUS),
        font,
    })
}

/// What a generated stylesheet is made from.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// The colors.
    pub colors: Colors,
    /// The opacity of the background.
    pub opacity: f64,
    /// A pill per module instead of one bar.
    pub pills: bool,
    /// Room left and right inside each module.
    pub padding: i64,
    /// The corner radius.
    pub radius: i64,
    /// The font, or the default one.
    pub font: Option<CssFont>,
}

/// The declarations of `font`, one per line.
fn font_declarations(font: &CssFont) -> String {
    let mut css = String::new();
    if !font.family.is_empty() {
        let _ = writeln!(css, "    font-family: {};", css_string(&font.family));
    }
    if let Some((size, unit)) = font.size {
        let _ = writeln!(css, "    font-size: {size}{unit};");
    }
    if let Some(weight) = font.weight {
        let _ = writeln!(css, "    font-weight: {weight};");
    }
    if font.italic {
        let _ = writeln!(css, "    font-style: italic;");
    }
    css
}

/// A rule that gives everything `font`.
fn font_rule(font: &CssFont) -> String {
    format!("* {{\n{}}}\n", font_declarations(font))
}

/// The size of the font `description` in pixels: `px` as it is, points at 96 dpi; `None`
/// without a size.
pub fn font_size_px(description: &str) -> Option<f64> {
    let (size, unit) = css_font(description)?.size?;
    Some(if unit == "px" { size } else { size * 4.0 / 3.0 })
}

/// The font `description` with its size set to `px` pixels, or without a size for `None`:
/// `Noto Sans Bold 11` and 16 give `Noto Sans Bold 16px`.
pub fn font_with_size(description: &str, px: Option<u32>) -> String {
    let mut words: Vec<&str> = description.split_whitespace().collect();
    if let Some(last) = words.last() {
        let number = last.strip_suffix("px").unwrap_or(last);
        if number
            .parse::<f64>()
            .is_ok_and(|size| size.is_finite() && size > 0.0)
        {
            words.pop();
        }
    }
    let size = px.map(|px| format!("{px}px"));
    if let Some(size) = &size {
        words.push(size);
    }
    words.join(" ")
}

/// The look of the bar `state` sets, for a preview: the colors, shape and sizes of its
/// stylesheet; waybar's own look in its colors, and none (only the user's stylesheet) in
/// waybar's too, since nothing else can be known.
pub fn preview(state: &State, sway: Option<&SwayConfig>) -> Look {
    let theme = match value(state, keys::THEME) {
        Value::Choice(id) => id,
        _ => String::new(),
    };
    let integer = |key| match value(state, key) {
        Value::Integer(number) => number,
        _ => 0,
    };
    let font = match value(state, keys::FONT) {
        Value::Text(font) => css_font(&font),
        _ => None,
    };
    if theme == "waybar" || theme == "plain" {
        // `window#waybar` and `#workspaces button.focused` in waybar's own `style.css`.
        return Look {
            colors: Colors {
                background: keys::DEFAULT_BACKGROUND,
                text: Color::rgb(0xff, 0xff, 0xff),
                hover: Color::rgba(0, 0, 0, 0x33),
                focused: Color::rgb(0x64, 0x72, 0x7d),
                warning: Color::rgb(0xf0, 0x93, 0x2b),
                critical: Color::rgb(0xf5, 0x3c, 0x3c),
                urgent: Color::rgb(0xeb, 0x4d, 0x4b),
            },
            opacity: 0.5,
            pills: false,
            padding: 10,
            radius: 0,
            font,
        };
    }
    let colors = if theme == "sway" {
        Colors::sway(state, sway)
    } else {
        Colors::custom(state)
    };
    let opacity = match value(state, keys::OPACITY) {
        Value::Decimal(opacity) => opacity,
        _ => 1.0,
    };
    Look {
        colors,
        opacity,
        pills: matches!(value(state, keys::SHAPE), Value::Choice(id) if id == "pills"),
        padding: integer(keys::PADDING),
        radius: integer(keys::RADIUS),
        font,
    }
}

/// `state` with preset `id` applied, as [`preset_values`] and [`PRESET_KEYS`] say: for its
/// preview, and to tell whether it is the preset in use.
pub fn with_preset(state: &State, id: &str) -> State {
    let mut state = state.clone();
    let values = preset_values(id);
    for key in PRESET_KEYS {
        // In memory only; the values a preset holds are checked by the tests.
        let _ = match values.iter().find(|(preset_key, _)| preset_key == key) {
            Some((_, value)) => state.set(key, None, value.clone(), None),
            None => state.reset(key, None).map(|_| ()),
        };
    }
    state
}

/// The preset whose values `state` has, if one has them all (blur aside where `swayfx` is
/// false, since presets leave it be there).
pub fn current_preset(state: &State, swayfx: bool) -> Option<&'static str> {
    PRESETS
        .iter()
        .filter(|preset| swayfx || !preset.swayfx)
        .find(|preset| {
            let applied = with_preset(state, preset.id);
            PRESET_KEYS
                .iter()
                .filter(|key| swayfx || **key != keys::BLUR)
                .all(|key| value(&applied, key) == value(state, key))
        })
        .map(|preset| preset.id)
}

/// The rules of a generated stylesheet.
pub fn generated(look: &Look) -> String {
    let Colors {
        background,
        text,
        hover,
        focused,
        warning,
        critical,
        urgent,
    } = look.colors;
    let (padding, radius) = (look.padding, look.radius);
    let mut css = String::new();
    let _ = writeln!(css, "* {{\n    min-height: 0;");
    if let Some(font) = &look.font {
        css.push_str(&font_declarations(font));
    }
    css.push_str("}\n\n");
    let modules =
        ".modules-left > widget > *,\n.modules-center > widget > *,\n.modules-right > widget > *";
    if look.pills {
        let _ = write!(
            css,
            "window#waybar {{\n    background: transparent;\n    color: {text};\n}}\n\n\
             {modules} {{\n    background-color: {background};\n    border: 1px solid {border};\n    \
             border-radius: {radius}px;\n    margin: 0 2px;\n    padding: 0 {padding}px;\n}}\n\n",
            text = css_color(text, 1.0),
            background = css_color(background, look.opacity),
            border = css_color(text, 0.1),
        );
    } else {
        let _ = write!(
            css,
            "window#waybar {{\n    background-color: {background};\n    border-radius: {radius}px;\n    \
             color: {text};\n}}\n\n{modules} {{\n    padding: 0 {padding}px;\n    border-radius: {radius}px;\n}}\n\n",
            background = css_color(background, look.opacity),
            text = css_color(text, 1.0),
        );
    }
    let hovered = ".modules-left > widget > *:hover,\n.modules-center > widget > *:hover,\n.modules-right > widget > *:hover";
    let _ = write!(
        css,
        "{hovered} {{\n    background-color: {hover};\n}}\n\n\
         button {{\n    border: none;\n    border-radius: {radius}px;\n    box-shadow: none;\n    min-height: 0;\n}}\n\n\
         #workspaces {{\n    padding: 0;\n}}\n\n\
         #workspaces button {{\n    padding: 0 {half}px;\n    background: transparent;\n    color: {text};\n}}\n\n\
         #workspaces button:hover {{\n    background: {hover};\n}}\n\n\
         #workspaces button.focused,\n#workspaces button.active,\n#mode {{\n    background-color: {focused};\n    color: {text};\n}}\n\n\
         #workspaces button.urgent {{\n    background-color: {urgent};\n}}\n\n",
        hover = css_color(hover, 1.0),
        half = (padding / 2).max(2),
        text = css_color(text, 1.0),
        focused = css_color(focused, 1.0),
        urgent = css_color(urgent, 1.0),
    );
    let state = |class: &str| {
        format!(
            ".modules-left > widget > .{class},\n.modules-center > widget > .{class},\n.modules-right > widget > .{class}"
        )
    };
    let _ = write!(
        css,
        "{warning_selector},\n#network.disconnected,\n#pulseaudio.muted,\n#wireplumber.muted {{\n    color: {warning};\n}}\n\n\
         {critical_selector},\n#temperature.critical {{\n    color: {critical};\n}}\n\n\
         #battery.critical:not(.charging) {{\n    background-color: {critical};\n    color: {text};\n}}\n\n\
         tooltip {{\n    background-color: {tooltip};\n    border-radius: {tooltip_radius}px;\n}}\n\n\
         tooltip label {{\n    color: {text};\n}}\n\n\
         #tray > .passive {{\n    -gtk-icon-effect: dim;\n}}\n\n\
         #tray > .needs-attention {{\n    -gtk-icon-effect: highlight;\n}}\n",
        warning_selector = state("warning"),
        critical_selector = state("critical"),
        warning = css_color(warning, 1.0),
        critical = css_color(critical, 1.0),
        text = css_color(text, 1.0),
        tooltip = css_color(background, look.opacity.max(0.9)),
        tooltip_radius = radius.min(12),
    );
    css
}

/// A preset of the page: values for several style settings at once, stored as one change. A
/// preset never changes which items the bar shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    /// Its id.
    pub id: &'static str,
    /// Its name to show.
    pub title: Msg,
    /// What it looks like, in a few words.
    pub description: Msg,
    /// Whether it needs SwayFX (for the blur behind the bar).
    pub swayfx: bool,
}

/// The presets of the page, in the order the page shows them.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "simple",
        title: msg!("Simple"),
        description: msg!("One calm dark bar"),
        swayfx: false,
    },
    Preset {
        id: "gnome",
        title: msg!("GNOME-like"),
        description: msg!("Black with white text, like GNOME's top bar"),
        swayfx: false,
    },
    Preset {
        id: "islands",
        title: msg!("Floating islands"),
        description: msg!("Rounded islands floating above your windows"),
        swayfx: false,
    },
    Preset {
        id: "minimal",
        title: msg!("Minimal"),
        description: msg!("Thin and quiet, the wallpaper shows through"),
        swayfx: false,
    },
    Preset {
        id: "contrast",
        title: msg!("Large & high contrast"),
        description: msg!("Big bold text, white on black"),
        swayfx: false,
    },
    Preset {
        id: "sway",
        title: msg!("Like your windows"),
        description: msg!("One bar in the colors of your windows"),
        swayfx: false,
    },
    Preset {
        id: "waybar",
        title: msg!("Classic Waybar"),
        description: msg!("Waybar's own look, as it ships"),
        swayfx: false,
    },
    Preset {
        id: "glass",
        title: msg!("Glass"),
        description: msg!("Dark glass islands, blurred behind"),
        swayfx: true,
    },
];

/// The values preset `id` stores, by key. Settings it doesn't name are reset to their defaults
/// by the caller, so a preset always looks the same.
#[expect(
    clippy::too_many_lines,
    reason = "the presets' values, read best as a list"
)]
pub fn preset_values(id: &str) -> Vec<(&'static str, Value)> {
    let choice = |id: &str| Value::Choice(id.to_owned());
    let rgb = |red, green, blue| Value::Color(Color::rgb(red, green, blue));
    let rgba = |red, green, blue, alpha| Value::Color(Color::rgba(red, green, blue, alpha));
    match id {
        "simple" => vec![
            (keys::THEME, choice("custom")),
            (keys::SHAPE, choice("bar")),
            (keys::HEIGHT, Value::Integer(30)),
            (keys::PADDING, Value::Integer(10)),
            (keys::BACKGROUND, rgb(0x1f, 0x23, 0x28)),
            (keys::TEXT, rgb(0xe8, 0xe8, 0xe8)),
            (keys::HOVER, rgba(0xff, 0xff, 0xff, 0x1f)),
            (keys::FOCUSED, rgb(0x3a, 0x5f, 0x8a)),
            (keys::WARNING, rgb(0xe5, 0xa5, 0x0a)),
            (keys::CRITICAL, rgb(0xf6, 0x61, 0x51)),
            (keys::URGENT, rgb(0xc0, 0x1c, 0x28)),
        ],
        "gnome" => vec![
            (keys::THEME, choice("custom")),
            (keys::SHAPE, choice("bar")),
            (keys::HEIGHT, Value::Integer(32)),
            (keys::PADDING, Value::Integer(12)),
            (keys::RADIUS, Value::Integer(999)),
            (keys::BACKGROUND, rgb(0, 0, 0)),
            (keys::TEXT, rgb(0xff, 0xff, 0xff)),
            (keys::HOVER, rgba(0xff, 0xff, 0xff, 0x26)),
            (keys::FOCUSED, rgb(0x1a, 0x5f, 0xb4)),
            (keys::WARNING, rgb(0xf8, 0xe4, 0x5c)),
            (keys::CRITICAL, rgb(0xff, 0x7b, 0x63)),
            (keys::URGENT, rgb(0xc0, 0x1c, 0x28)),
        ],
        "islands" => vec![
            (keys::LAYER, choice("top")),
            (keys::HEIGHT, Value::Integer(32)),
            (keys::MARGIN_TOP, Value::Integer(6)),
            (keys::MARGIN_LEFT, Value::Integer(8)),
            (keys::MARGIN_RIGHT, Value::Integer(8)),
            (keys::SPACING, Value::Integer(6)),
            (keys::THEME, choice("custom")),
            (keys::SHAPE, choice("pills")),
            (keys::PADDING, Value::Integer(12)),
            (keys::RADIUS, Value::Integer(999)),
            (keys::OPACITY, Value::Decimal(0.9)),
            (keys::BACKGROUND, rgb(0x1e, 0x1e, 0x2e)),
            (keys::TEXT, rgb(0xcd, 0xd6, 0xf4)),
            (keys::HOVER, rgba(0xff, 0xff, 0xff, 0x1a)),
            (keys::FOCUSED, rgb(0x45, 0x47, 0x5a)),
            (keys::WARNING, rgb(0xf9, 0xe2, 0xaf)),
            (keys::CRITICAL, rgb(0xf3, 0x8b, 0xa8)),
            (keys::URGENT, rgb(0xf3, 0x8b, 0xa8)),
        ],
        "minimal" => vec![
            (keys::HEIGHT, Value::Integer(22)),
            (keys::SPACING, Value::Integer(2)),
            (keys::THEME, choice("custom")),
            (keys::SHAPE, choice("bar")),
            (keys::FONT, Value::Text("11px".to_owned())),
            (keys::PADDING, Value::Integer(6)),
            (keys::OPACITY, Value::Decimal(0.6)),
            (keys::BACKGROUND, rgb(0, 0, 0)),
            (keys::TEXT, rgb(0xff, 0xff, 0xff)),
            (keys::HOVER, rgba(0xff, 0xff, 0xff, 0x1a)),
            (keys::FOCUSED, rgb(0x40, 0x40, 0x40)),
            (keys::WARNING, rgb(0xf8, 0xe4, 0x5c)),
            (keys::CRITICAL, rgb(0xff, 0x7b, 0x63)),
            (keys::URGENT, rgb(0xc0, 0x1c, 0x28)),
        ],
        "contrast" => vec![
            (keys::HEIGHT, Value::Integer(44)),
            (keys::THEME, choice("custom")),
            (keys::SHAPE, choice("bar")),
            (keys::FONT, Value::Text("Sans Bold 18px".to_owned())),
            (keys::PADDING, Value::Integer(14)),
            (keys::BACKGROUND, rgb(0, 0, 0)),
            (keys::TEXT, rgb(0xff, 0xff, 0xff)),
            (keys::HOVER, rgb(0x33, 0x33, 0x33)),
            (keys::FOCUSED, rgb(0x1a, 0x5f, 0xb4)),
            (keys::WARNING, rgb(0xff, 0xd7, 0x00)),
            (keys::CRITICAL, rgb(0xff, 0x6e, 0x6e)),
            (keys::URGENT, rgb(0xff, 0x6e, 0x6e)),
        ],
        "glass" => vec![
            (keys::LAYER, choice("top")),
            (keys::HEIGHT, Value::Integer(30)),
            (keys::MARGIN_TOP, Value::Integer(6)),
            (keys::MARGIN_LEFT, Value::Integer(8)),
            (keys::MARGIN_RIGHT, Value::Integer(8)),
            (keys::SPACING, Value::Integer(0)),
            (keys::THEME, choice("custom")),
            (keys::SHAPE, choice("pills")),
            (keys::FONT, Value::Text("Noto Sans 12px".to_owned())),
            (keys::PADDING, Value::Integer(10)),
            (keys::RADIUS, Value::Integer(999)),
            (keys::OPACITY, Value::Decimal(0.45)),
            (keys::BACKGROUND, rgb(0x12, 0x12, 0x16)),
            (keys::TEXT, rgb(0xd4, 0xd4, 0xd8)),
            (keys::HOVER, rgba(0, 0, 0, 0x59)),
            (keys::FOCUSED, rgb(0x7b, 0x3f, 0xa0)),
            (keys::WARNING, rgb(0xff, 0x9f, 0x1c)),
            (keys::CRITICAL, rgb(0xff, 0x3b, 0x30)),
            (keys::URGENT, rgb(0xff, 0x3b, 0x30)),
            (keys::BLUR, Value::Bool(true)),
        ],
        "sway" => vec![(keys::THEME, choice("sway")), (keys::SHAPE, choice("bar"))],
        _ => Vec::new(),
    }
}

/// The settings a preset sets or resets: every style setting and the placement settings a
/// preset may set.
pub const PRESET_KEYS: &[&str] = &[
    keys::LAYER,
    keys::HEIGHT,
    keys::MARGIN_TOP,
    keys::MARGIN_RIGHT,
    keys::MARGIN_BOTTOM,
    keys::MARGIN_LEFT,
    keys::SPACING,
    keys::THEME,
    keys::SHAPE,
    keys::FONT,
    keys::PADDING,
    keys::RADIUS,
    keys::OPACITY,
    keys::BACKGROUND,
    keys::TEXT,
    keys::HOVER,
    keys::FOCUSED,
    keys::WARNING,
    keys::CRITICAL,
    keys::URGENT,
    keys::BLUR,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_split_for_css() {
        assert_eq!(
            css_font("Noto Sans Bold Italic 11"),
            Some(CssFont {
                family: "Noto Sans".into(),
                weight: Some(700),
                italic: true,
                size: Some((11.0, "pt")),
            })
        );
        assert_eq!(css_font("Mono 12px").unwrap().size, Some((12.0, "px")));
        assert_eq!(css_font("Cantarell").unwrap().size, None);
        assert_eq!(css_font(""), None);
    }

    #[test]
    fn values_can_t_break_out_of_the_css() {
        assert_eq!(css_string("a\"b\\c\nd}"), "\"a\\\"b\\\\cd}\"");
        assert_eq!(file_url("/a b/ä.css").unwrap(), "file:///a%20b/%C3%A4.css");
        assert_eq!(file_url("relative.css"), None);
        assert_eq!(file_url("/x\n.css"), None);
        let look = Look {
            colors: Colors {
                background: Color::rgb(1, 2, 3),
                text: Color::rgb(255, 255, 255),
                hover: Color::rgba(0, 0, 0, 0x33),
                focused: Color::rgb(4, 5, 6),
                warning: Color::rgb(7, 8, 9),
                critical: Color::rgb(10, 11, 12),
                urgent: Color::rgb(13, 14, 15),
            },
            opacity: 0.5,
            pills: true,
            padding: 10,
            radius: 999,
            font: css_font("Evil\"} * { color: red; } x 12"),
        };
        let css = generated(&look);
        assert!(css.contains("font-family: \"Evil\\\"} * { color: red; } x\";"));
        assert!(css.contains("background-color: rgba(1, 2, 3, 0.500);"));
        assert!(css.contains("rgba(0, 0, 0, 0.200)"));
        // Balanced braces outside strings: every rule ends.
        let mut depth = 0i32;
        let mut quoted = false;
        let mut escaped = false;
        for char in css.chars() {
            match char {
                _ if escaped => escaped = false,
                '\\' if quoted => escaped = true,
                '"' => quoted = !quoted,
                '{' if !quoted => depth += 1,
                '}' if !quoted => depth -= 1,
                _ => {}
            }
            assert!(depth >= 0);
        }
        assert_eq!(depth, 0);
    }

    fn state(dir: &std::path::Path) -> State {
        State::load(
            Registry::builtin(),
            crate::state::Paths::new(&dir.join("config"), &dir.join("state"), "host").unwrap(),
        )
    }

    #[test]
    fn font_sizes_are_read_and_set() {
        assert_eq!(font_size_px("Noto Sans 12px"), Some(12.0));
        assert_eq!(font_size_px("Noto Sans 12"), Some(16.0));
        assert_eq!(font_size_px("Noto Sans"), None);
        assert_eq!(
            font_with_size("Noto Sans Bold 11", Some(16)),
            "Noto Sans Bold 16px"
        );
        assert_eq!(font_with_size("", Some(13)), "13px");
        assert_eq!(font_with_size("Mono 12px", None), "Mono");
        assert_eq!(
            css_font(&font_with_size("", Some(13))).unwrap().size,
            Some((13.0, "px"))
        );
    }

    #[test]
    fn waybar_s_own_look_takes_the_font() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state(dir.path());
        assert_eq!(
            stylesheet(&state, None),
            super::super::modules::WAYBAR_DEFAULT_STYLE
        );
        state
            .set(keys::FONT, None, Value::Text("Inter 15px".into()), None)
            .unwrap();
        let css = stylesheet(&state, None);
        assert!(css.starts_with(super::super::modules::WAYBAR_DEFAULT_STYLE));
        assert!(css.ends_with("* {\n    font-family: \"Inter\";\n    font-size: 15px;\n}\n"));
    }

    #[test]
    fn a_preset_is_recognized_until_a_value_changes() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        // The defaults are waybar's own look.
        assert_eq!(current_preset(&state, false), Some("waybar"));
        let mut islands = with_preset(&state, "islands");
        assert_eq!(current_preset(&islands, false), Some("islands"));
        assert!(preview(&islands, None).pills);
        islands
            .set(keys::RADIUS, None, Value::Integer(4), None)
            .unwrap();
        assert_eq!(current_preset(&islands, false), None);
        // Glass needs SwayFX: on sway it isn't offered, and its blur isn't compared.
        let glass = with_preset(&state, "glass");
        assert_eq!(current_preset(&glass, true), Some("glass"));
        assert_eq!(current_preset(&glass, false), None);
        // Presets change the look, never the items.
        for preset in PRESETS {
            for (key, _) in preset_values(preset.id) {
                assert!(!key.starts_with("bar.modules"), "{key}");
            }
        }
    }

    #[test]
    fn presets_are_readable() {
        use crate::theme::{Rgb, ratio};
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let rgb = |color: Color| {
            let [red, green, blue] = color.channels();
            Rgb::from_u8(red, green, blue)
        };
        for preset in PRESETS.iter().filter(|preset| preset.id != "waybar") {
            let look = preview(&with_preset(&state, preset.id), None);
            let text = rgb(look.colors.text);
            for (name, fill) in [
                ("background", look.colors.background),
                ("focused", look.colors.focused),
            ] {
                let contrast = ratio(text, rgb(fill));
                assert!(
                    contrast >= 4.5,
                    "{}: text on {name} is {contrast:.2}",
                    preset.id
                );
            }
        }
    }

    #[test]
    fn presets_hold_values_their_settings_take() {
        let registry = Registry::builtin();
        for preset in PRESETS {
            for (key, value) in preset_values(preset.id) {
                assert!(PRESET_KEYS.contains(&key), "{key}");
                let setting = registry.get(key).unwrap();
                assert_eq!(setting.check(&value), Ok(()), "{key}");
            }
        }
    }
}
