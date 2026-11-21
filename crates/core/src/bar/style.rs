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
fn css_color(color: Color, opacity: f64) -> String {
    let [red, green, blue] = color.channels();
    let alpha = f64::from(color.alpha().unwrap_or(0xff)) / 255.0 * opacity.clamp(0.0, 1.0);
    format!("rgba({red}, {green}, {blue}, {alpha:.3})")
}

/// `text` as a CSS string in double quotes.
fn css_string(text: &str) -> String {
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
    let colors = match theme.as_str() {
        "waybar" => return super::modules::WAYBAR_DEFAULT_STYLE.to_owned(),
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
    let font = match value(state, keys::FONT) {
        Value::Text(font) => css_font(&font),
        _ => None,
    };
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

/// A preset of the page: values for several settings at once, stored as one change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    /// Its id.
    pub id: &'static str,
    /// Its name to show.
    pub title: Msg,
    /// What it looks like, in a few words.
    pub description: Msg,
}

/// The presets of the page.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "glass",
        title: msg!("Glass"),
        description: msg!(
            "Floating pills of dark glass on a transparent bar, blurred behind on SwayFX"
        ),
    },
    Preset {
        id: "sway",
        title: msg!("Sway"),
        description: msg!("One bar in the colors of your windows"),
    },
    Preset {
        id: "waybar",
        title: msg!("waybar"),
        description: msg!("waybar's own look, as it ships"),
    },
];

/// The values preset `id` stores, by key. Settings it doesn't name are reset to their defaults
/// by the caller, so a preset always looks the same.
pub fn preset_values(id: &str) -> Vec<(&'static str, Value)> {
    let choice = |id: &str| Value::Choice(id.to_owned());
    match id {
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
            (keys::BACKGROUND, Value::Color(Color::rgb(0x12, 0x12, 0x16))),
            (keys::TEXT, Value::Color(Color::rgb(0xd4, 0xd4, 0xd8))),
            (keys::HOVER, Value::Color(Color::rgba(0, 0, 0, 0x59))),
            (keys::FOCUSED, Value::Color(Color::rgb(0xb1, 0x62, 0xd5))),
            (keys::WARNING, Value::Color(Color::rgb(0xff, 0x9f, 0x1c))),
            (keys::CRITICAL, Value::Color(Color::rgb(0xff, 0x3b, 0x30))),
            (keys::URGENT, Value::Color(Color::rgb(0xff, 0x3b, 0x30))),
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
