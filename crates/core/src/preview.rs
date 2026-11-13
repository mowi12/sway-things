// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window preview of the Windows page: a focused and an unfocused window side by side, as
//! sway draws them with the values of the settings, as rectangles for the app to paint.
//!
//! [`Look::read`] gathers the values that matter, [`layout`] places the windows in the preview
//! at 1:1 logical pixels. The tree it shows is a horizontal split of two windows where the
//! focused one sits alone in a nested horizontal split, so sway draws its split indicator on its
//! right edge.
//!
//! Sizes are sway's: the title bar is the font's height plus the vertical padding above and
//! below, its border is drawn inside it, the gaps at the edge are the outer plus the inner gaps,
//! and the gap between the windows is the inner gap. Gaps, borders and padding above the caps
//! ([`MAX_GAP`], [`MAX_BORDER`], [`MAX_PADDING`]) are drawn at the cap, and the layout says it
//! is [not to scale](Layout::not_to_scale).
//!
//! On SwayFX it adds the effects that are cheap to draw: rounded corners, shadows and the
//! dimming of the unfocused window. Blur and animations aren't drawn, nor the settings that
//! change nothing with two windows (smart gaps and borders) or that are about the screen edge.

use crate::command::Color;
use crate::command::window::WindowColors;
use crate::registry::{Registry, Setting, Value};

/// The widest gap drawn at each side, in pixels.
pub const MAX_GAP: i64 = 48;
/// The widest border drawn, in pixels.
pub const MAX_BORDER: i64 = 24;
/// The widest title bar padding drawn, in pixels.
pub const MAX_PADDING: i64 = 24;

/// The titles of the focused and the unfocused window.
pub const TITLES: [&str; 2] = ["Terminal", "Editor"];

/// The border style of tiled windows (`default_border`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    /// A title bar and a border.
    Normal,
    /// A border only.
    Pixel,
    /// Neither.
    None,
}

/// Where the title sits in the title bar (`title_align`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleAlign {
    /// At the left.
    Left,
    /// In the middle.
    Center,
    /// At the right.
    Right,
}

/// The SwayFX effects the preview draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Effects {
    /// The corner radius, in pixels.
    pub corner_radius: i64,
    /// Whether windows without gaps get square corners.
    pub smart_corner_radius: bool,
    /// The shadows, if they are on.
    pub shadow: Option<Shadow>,
    /// How much the unfocused window is dimmed, from 0 to 1.
    pub dim: f64,
    /// The color it is dimmed with.
    pub dim_color: Color,
}

/// The shadows of SwayFX.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    /// Of the focused window.
    pub color: Color,
    /// Of the other windows.
    pub inactive_color: Color,
    /// How far it fades out, in pixels.
    pub blur_radius: i64,
    /// The offset to the right, in pixels.
    pub offset_x: f64,
    /// The offset down, in pixels.
    pub offset_y: f64,
}

/// What the preview shows: the values of the settings it draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// The colors of the focused window (`client.focused`).
    pub focused: WindowColors,
    /// The colors of the unfocused window (`client.unfocused`).
    pub unfocused: WindowColors,
    /// The border style of tiled windows.
    pub border: BorderStyle,
    /// The border width of tiled windows, in pixels.
    pub border_width: i64,
    /// The inner gaps, in pixels.
    pub gaps_inner: i64,
    /// The outer gaps, in pixels; negative ones take some of the inner gaps away.
    pub gaps_outer: i64,
    /// The title bar font as sway reads it, e.g. `monospace 10`.
    pub font: String,
    /// Where the title sits.
    pub title_align: TitleAlign,
    /// The title bar border, in pixels.
    pub titlebar_border: i64,
    /// The title bar padding left and right, in pixels.
    pub padding_horizontal: i64,
    /// The title bar padding above and below, in pixels.
    pub padding_vertical: i64,
    /// The SwayFX effects, or `None` on sway.
    pub effects: Option<Effects>,
}

/// The settings the preview reads.
pub const KEYS: &[&str] = &[
    "windows.focused.border",
    "windows.focused.background",
    "windows.focused.text",
    "windows.focused.indicator",
    "windows.focused.child_border",
    "windows.unfocused.border",
    "windows.unfocused.background",
    "windows.unfocused.text",
    "windows.unfocused.indicator",
    "windows.unfocused.child_border",
    "windows.border.style",
    "windows.border.width",
    "windows.gaps.inner",
    "windows.gaps.outer",
    "windows.font",
    "windows.title_align",
    "windows.titlebar.border_thickness",
    "windows.titlebar.padding_horizontal",
    "windows.titlebar.padding_vertical",
    "effects.corner_radius",
    "effects.smart_corner_radius",
    "effects.shadows",
    "effects.shadow_color",
    "effects.shadow_inactive_color",
    "effects.shadow_blur_radius",
    "effects.shadow_offset_x",
    "effects.shadow_offset_y",
    "effects.dim_inactive",
    "effects.dim_inactive_colors.unfocused",
];

/// Reads the values of the settings in [`KEYS`], each from `value` or else its default.
struct Values<F> {
    value: F,
}

impl<F: Fn(&'static Setting) -> Option<Value>> Values<F> {
    fn get(&self, key: &str) -> Option<Value> {
        let setting = Registry::builtin().get(key)?;
        (self.value)(setting).or_else(|| Some(setting.default_value()))
    }

    fn color(&self, key: &str) -> Color {
        match self.get(key) {
            Some(Value::Color(color)) => color,
            _ => Color::rgb(0, 0, 0),
        }
    }

    fn integer(&self, key: &str) -> i64 {
        match self.get(key) {
            Some(Value::Integer(value)) => value,
            _ => 0,
        }
    }

    fn decimal(&self, key: &str) -> f64 {
        match self.get(key) {
            Some(Value::Decimal(value)) => value,
            _ => 0.0,
        }
    }

    fn bool(&self, key: &str) -> bool {
        matches!(self.get(key), Some(Value::Bool(true)))
    }

    fn text(&self, key: &str) -> String {
        match self.get(key) {
            Some(Value::Choice(text) | Value::Text(text)) => text,
            _ => String::new(),
        }
    }

    fn window_colors(&self, state: &str) -> WindowColors {
        let color = |name: &str| self.color(&format!("windows.{state}.{name}"));
        WindowColors {
            border: color("border"),
            background: color("background"),
            text: color("text"),
            indicator: color("indicator"),
            child_border: color("child_border"),
        }
    }
}

impl Look {
    /// The look with the value of each setting from `value`, or its default where that gives
    /// `None`. With `swayfx` it has the effects.
    pub fn read(value: impl Fn(&'static Setting) -> Option<Value>, swayfx: bool) -> Self {
        let values = Values { value };
        let effects = swayfx.then(|| Effects {
            corner_radius: values.integer("effects.corner_radius"),
            smart_corner_radius: values.bool("effects.smart_corner_radius"),
            shadow: values.bool("effects.shadows").then(|| Shadow {
                color: values.color("effects.shadow_color"),
                inactive_color: values.color("effects.shadow_inactive_color"),
                blur_radius: values.integer("effects.shadow_blur_radius"),
                offset_x: values.decimal("effects.shadow_offset_x"),
                offset_y: values.decimal("effects.shadow_offset_y"),
            }),
            dim: values.decimal("effects.dim_inactive"),
            dim_color: values.color("effects.dim_inactive_colors.unfocused"),
        });
        Self {
            focused: values.window_colors("focused"),
            unfocused: values.window_colors("unfocused"),
            border: match values.text("windows.border.style").as_str() {
                "pixel" => BorderStyle::Pixel,
                "none" => BorderStyle::None,
                _ => BorderStyle::Normal,
            },
            border_width: values.integer("windows.border.width"),
            gaps_inner: values.integer("windows.gaps.inner"),
            gaps_outer: values.integer("windows.gaps.outer"),
            font: values.text("windows.font"),
            title_align: match values.text("windows.title_align").as_str() {
                "center" => TitleAlign::Center,
                "right" => TitleAlign::Right,
                _ => TitleAlign::Left,
            },
            titlebar_border: values.integer("windows.titlebar.border_thickness"),
            padding_horizontal: values.integer("windows.titlebar.padding_horizontal"),
            padding_vertical: values.integer("windows.titlebar.padding_vertical"),
            effects,
        }
    }

    /// The look of sway with nothing configured.
    pub fn defaults(swayfx: bool) -> Self {
        Self::read(|_| None, swayfx)
    }
}

/// A rectangle in the preview, in logical pixels from its top left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// The left edge.
    pub x: f64,
    /// The top edge.
    pub y: f64,
    /// The width, never negative.
    pub width: f64,
    /// The height, never negative.
    pub height: f64,
}

impl Rect {
    fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
        }
    }

    /// Whether it covers nothing.
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// The size of the titles in the title bar font, as the app measures it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSize {
    /// The height of the font (ascent and descent), in pixels.
    pub height: f64,
    /// The widths of the [`TITLES`], in pixels.
    pub widths: [f64; 2],
}

/// The title bar of a window.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleBar {
    /// All of it, filled with the background color.
    pub area: Rect,
    /// Its border at the top, bottom, left and right, in the border color.
    pub edges: Vec<Rect>,
    /// Where the title may be drawn: within the horizontal padding.
    pub text_area: Rect,
    /// The top left corner of the title.
    pub text_x: f64,
    /// The top of the title.
    pub text_y: f64,
}

/// The shadow of a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowShadow {
    /// Its color.
    pub color: Color,
    /// How far it fades out, in pixels.
    pub blur_radius: f64,
    /// The offset to the right, in pixels.
    pub dx: f64,
    /// The offset down, in pixels.
    pub dy: f64,
}

/// One window of the preview with its decorations.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// Whether it has the focus.
    pub focused: bool,
    /// Its title, one of [`TITLES`].
    pub title: &'static str,
    /// Its colors.
    pub colors: WindowColors,
    /// The whole window with its decorations.
    pub area: Rect,
    /// Its title bar, with the `normal` border style.
    pub title_bar: Option<TitleBar>,
    /// Its border, each piece with its color: the child border, or the split indicator.
    pub borders: Vec<(Rect, Color)>,
    /// What the app in it draws.
    pub content: Rect,
    /// The radius of its corners, 0 for square ones.
    pub corner_radius: f64,
    /// Its shadow.
    pub shadow: Option<WindowShadow>,
    /// The color its content is dimmed with, and how much, from 0 to 1.
    pub dim: Option<(Color, f64)>,
}

/// The windows of the preview.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The focused window at the left, the unfocused one at the right.
    pub windows: [Window; 2],
    /// Whether a size was larger than drawn.
    pub not_to_scale: bool,
}

/// Pixels as a coordinate. The values are capped far below where `f64` loses precision.
fn px(value: i64) -> f64 {
    let value = value.clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    i32::try_from(value).map_or(0.0, f64::from)
}

/// `value` within `0 ..= cap`, and whether it was above the cap.
fn capped(value: i64, cap: i64) -> (f64, bool) {
    (px(value.clamp(0, cap)), value > cap)
}

/// Places the windows of `look` in a preview `width` × `height` pixels large, with the titles
/// of the `text` size.
pub fn layout(look: &Look, width: f64, height: f64, text: &TextSize) -> Layout {
    let edge_gap = look.gaps_outer.saturating_add(look.gaps_inner).max(0);
    let (edge, edge_capped) = capped(edge_gap, MAX_GAP);
    let (between, between_capped) = capped(look.gaps_inner, MAX_GAP);
    let (border, border_capped) = match look.border {
        BorderStyle::None => (0.0, false),
        BorderStyle::Normal | BorderStyle::Pixel => capped(look.border_width, MAX_BORDER),
    };
    let (vertical, vertical_capped) = capped(look.padding_vertical, MAX_PADDING);
    let (horizontal, horizontal_capped) = capped(look.padding_horizontal, MAX_PADDING);
    let (thickness, thickness_capped) = capped(look.titlebar_border, MAX_PADDING);
    let title_height = match look.border {
        BorderStyle::Normal => text.height.max(0.0) + 2.0 * vertical,
        BorderStyle::Pixel | BorderStyle::None => 0.0,
    };
    let not_to_scale = edge_capped
        || between_capped
        || border_capped
        || (look.border == BorderStyle::Normal
            && (vertical_capped || horizontal_capped || thickness_capped));

    // Whole pixels, so the edges of the windows stay sharp.
    let window_width = ((width - 2.0 * edge - between) / 2.0).floor().max(0.0);
    let window_height = (height - 2.0 * edge).max(0.0);
    let corner_radius = look.effects.as_ref().map_or(0.0, |effects| {
        if effects.smart_corner_radius && edge_gap == 0 {
            0.0
        } else {
            px(effects.corner_radius.max(0)).min(window_width.min(window_height) / 2.0)
        }
    });

    let window = |index: usize| {
        let focused = index == 0;
        let colors = if focused {
            look.focused
        } else {
            look.unfocused
        };
        let x = if focused {
            edge
        } else {
            edge + window_width + between
        };
        let area = Rect::new(x, edge, window_width, window_height);
        let parts = Parts {
            style: look.border,
            border,
            title_height,
            thickness,
            padding_h: horizontal,
            padding_v: vertical,
            align: look.title_align,
            text_width: text.widths[index],
        };
        let (title_bar, borders, content) = parts.place(area, &colors, focused);
        let shadow = look
            .effects
            .as_ref()
            .and_then(|effects| effects.shadow)
            .map(|shadow| WindowShadow {
                color: if focused {
                    shadow.color
                } else {
                    shadow.inactive_color
                },
                blur_radius: px(shadow.blur_radius.max(0)),
                dx: shadow.offset_x,
                dy: shadow.offset_y,
            });
        let dim = look
            .effects
            .as_ref()
            .filter(|effects| !focused && effects.dim > 0.0)
            .map(|effects| (effects.dim_color, effects.dim.min(1.0)));
        Window {
            focused,
            title: TITLES[index],
            colors,
            area,
            title_bar,
            borders,
            content,
            corner_radius,
            shadow,
            dim,
        }
    };
    Layout {
        windows: [window(0), window(1)],
        not_to_scale,
    }
}

/// The sizes of a window's decorations.
struct Parts {
    style: BorderStyle,
    border: f64,
    title_height: f64,
    thickness: f64,
    padding_h: f64,
    padding_v: f64,
    align: TitleAlign,
    text_width: f64,
}

impl Parts {
    /// The title bar, the border pieces and the content of a window in `area`. The focused one
    /// has its split indicator on the right edge.
    fn place(
        &self,
        area: Rect,
        colors: &WindowColors,
        focused: bool,
    ) -> (Option<TitleBar>, Vec<(Rect, Color)>, Rect) {
        let Rect {
            x,
            y,
            width,
            height,
        } = area;
        let b = self.border.min(width / 2.0).min(height / 2.0);
        let right_color = if focused {
            colors.indicator
        } else {
            colors.child_border
        };
        match self.style {
            BorderStyle::None => (None, Vec::new(), area),
            BorderStyle::Pixel => {
                let borders = vec![
                    (Rect::new(x, y, width, b), colors.child_border),
                    (
                        Rect::new(x, y + b, b, height - 2.0 * b),
                        colors.child_border,
                    ),
                    (
                        Rect::new(x + width - b, y + b, b, height - 2.0 * b),
                        right_color,
                    ),
                    (Rect::new(x, y + height - b, width, b), colors.child_border),
                ];
                let content = Rect::new(x + b, y + b, width - 2.0 * b, height - 2.0 * b);
                (None, borders, content)
            }
            BorderStyle::Normal => {
                let title_height = self.title_height.min(height);
                let title = self.title_bar(Rect::new(x, y, width, title_height));
                let top = y + title_height;
                let b = b.min((height - title_height).max(0.0));
                let side = height - title_height - b;
                let borders = vec![
                    (Rect::new(x, top, b, side), colors.child_border),
                    (Rect::new(x + width - b, top, b, side), right_color),
                    (Rect::new(x, y + height - b, width, b), colors.child_border),
                ];
                let content = Rect::new(x + b, top, width - 2.0 * b, side);
                (Some(title), borders, content)
            }
        }
    }

    /// The title bar in `area`.
    fn title_bar(&self, area: Rect) -> TitleBar {
        let Rect {
            x,
            y,
            width,
            height,
        } = area;
        let t = self.thickness.min(height / 2.0).min(width / 2.0);
        let edges = vec![
            Rect::new(x, y, width, t),
            Rect::new(x, y + height - t, width, t),
            Rect::new(x, y + t, t, height - 2.0 * t),
            Rect::new(x + width - t, y + t, t, height - 2.0 * t),
        ];
        let text_area = Rect::new(x + self.padding_h, y, width - 2.0 * self.padding_h, height);
        let start = match self.align {
            TitleAlign::Left => text_area.x,
            TitleAlign::Center => x + (width - self.text_width) / 2.0,
            TitleAlign::Right => text_area.x + text_area.width - self.text_width,
        };
        TitleBar {
            area,
            edges,
            text_area,
            // A title wider than the room starts at the left and is cut at the right.
            text_x: start.max(text_area.x),
            text_y: y + self.padding_v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: TextSize = TextSize {
        height: 14.0,
        widths: [60.0, 40.0],
    };

    fn look() -> Look {
        Look::defaults(false)
    }

    #[test]
    fn every_key_is_a_setting() {
        for key in KEYS {
            assert!(Registry::builtin().get(key).is_some(), "{key}");
        }
    }

    #[test]
    fn defaults_are_sways() {
        let look = look();
        assert_eq!(look.focused.child_border, Color::rgb(0x28, 0x55, 0x77));
        assert_eq!(look.unfocused.text, Color::rgb(0x88, 0x88, 0x88));
        assert_eq!(look.border, BorderStyle::Normal);
        assert_eq!(look.border_width, 2);
        assert_eq!(look.font, "monospace 10");
        assert_eq!((look.padding_horizontal, look.padding_vertical), (5, 4));
        assert_eq!(look.effects, None);
    }

    #[test]
    fn reads_values_and_falls_back_to_defaults() {
        let look = Look::read(
            |setting| match setting.key() {
                "windows.gaps.inner" => Some(Value::Integer(10)),
                "windows.title_align" => Some(Value::Choice("center".to_owned())),
                "windows.border.style" => Some(Value::Choice("pixel".to_owned())),
                _ => None,
            },
            false,
        );
        assert_eq!(look.gaps_inner, 10);
        assert_eq!(look.title_align, TitleAlign::Center);
        assert_eq!(look.border, BorderStyle::Pixel);
        assert_eq!(look.gaps_outer, 0);
    }

    #[test]
    fn normal_border_has_a_title_bar_and_the_indicator_on_the_focused_window() {
        let layout = layout(&look(), 400.0, 150.0, &TEXT);
        assert!(!layout.not_to_scale);
        let [focused, unfocused] = &layout.windows;
        assert!(focused.focused && !unfocused.focused);
        assert_eq!(focused.area, Rect::new(0.0, 0.0, 200.0, 150.0));
        assert_eq!(unfocused.area, Rect::new(200.0, 0.0, 200.0, 150.0));
        let title = focused.title_bar.as_ref().map(|title| title.area);
        // 14 px of font and 4 px of padding above and below.
        assert_eq!(title, Some(Rect::new(0.0, 0.0, 200.0, 22.0)));
        assert_eq!(focused.content, Rect::new(2.0, 22.0, 196.0, 126.0));
        let right = focused
            .borders
            .iter()
            .find(|(rect, _)| (rect.x - 198.0).abs() < 0.5);
        assert_eq!(
            right.map(|(_, color)| *color),
            Some(focused.colors.indicator)
        );
        assert!(
            unfocused
                .borders
                .iter()
                .all(|(_, color)| *color == unfocused.colors.child_border)
        );
    }

    #[test]
    fn pixel_border_surrounds_the_content() {
        let look = Look {
            border: BorderStyle::Pixel,
            border_width: 3,
            ..look()
        };
        let layout = layout(&look, 400.0, 150.0, &TEXT);
        let window = &layout.windows[1];
        assert_eq!(window.title_bar, None);
        assert_eq!(window.content, Rect::new(203.0, 3.0, 194.0, 144.0));
    }

    #[test]
    fn no_border_leaves_the_content() {
        let look = Look {
            border: BorderStyle::None,
            ..look()
        };
        let layout = layout(&look, 400.0, 150.0, &TEXT);
        assert_eq!(layout.windows[0].content, layout.windows[0].area);
        assert_eq!(layout.windows[0].borders, Vec::new());
    }

    #[test]
    fn gaps_add_up_at_the_edges() {
        let look = Look {
            gaps_inner: 10,
            gaps_outer: 5,
            ..look()
        };
        let layout = layout(&look, 400.0, 150.0, &TEXT);
        assert_eq!(layout.windows[0].area, Rect::new(15.0, 15.0, 180.0, 120.0));
        assert_eq!(layout.windows[1].area.x, 205.0);
        assert!(!layout.not_to_scale);
    }

    #[test]
    fn negative_outer_gaps_take_the_inner_ones_away() {
        let look = Look {
            gaps_inner: 10,
            gaps_outer: -20,
            ..look()
        };
        let layout = layout(&look, 400.0, 150.0, &TEXT);
        assert_eq!(layout.windows[0].area.x, 0.0);
    }

    #[test]
    fn large_sizes_are_capped_and_not_to_scale() {
        let look = Look {
            gaps_inner: 100,
            ..look()
        };
        let layout = layout(&look, 400.0, 150.0, &TEXT);
        assert!(layout.not_to_scale);
        assert_eq!(layout.windows[0].area.x, 48.0);
        let look = Look {
            border_width: 50,
            ..self::look()
        };
        assert!(super::layout(&look, 400.0, 150.0, &TEXT).not_to_scale);
        // Padding of a title bar that isn't drawn doesn't matter.
        let look = Look {
            border: BorderStyle::Pixel,
            padding_vertical: 50,
            ..self::look()
        };
        assert!(!super::layout(&look, 400.0, 150.0, &TEXT).not_to_scale);
    }

    #[test]
    fn titles_follow_the_alignment() {
        let place = |align| {
            let look = Look {
                title_align: align,
                ..look()
            };
            let layout = layout(&look, 400.0, 150.0, &TEXT);
            layout.windows[0]
                .title_bar
                .as_ref()
                .map(|title| (title.text_x, title.text_y))
        };
        assert_eq!(place(TitleAlign::Left), Some((5.0, 4.0)));
        assert_eq!(place(TitleAlign::Center), Some((70.0, 4.0)));
        assert_eq!(place(TitleAlign::Right), Some((135.0, 4.0)));
    }

    #[test]
    fn a_long_title_starts_at_the_padding() {
        let look = Look {
            title_align: TitleAlign::Right,
            ..look()
        };
        let text = TextSize {
            height: 14.0,
            widths: [500.0, 40.0],
        };
        let layout = layout(&look, 400.0, 150.0, &text);
        let title = layout.windows[0].title_bar.as_ref();
        assert_eq!(title.map(|title| title.text_x), Some(5.0));
        assert_eq!(title.map(|title| title.text_area.width), Some(190.0));
    }

    #[test]
    fn tiny_previews_stay_inside() {
        let look = Look {
            border_width: 20,
            gaps_inner: 40,
            ..look()
        };
        let layout = layout(&look, 30.0, 20.0, &TEXT);
        for window in &layout.windows {
            assert!(window.content.width >= 0.0 && window.content.height >= 0.0);
            for (rect, _) in &window.borders {
                assert!(rect.width >= 0.0 && rect.height >= 0.0);
            }
        }
    }

    fn swayfx(value: impl Fn(&str) -> Option<Value>) -> Look {
        Look::read(|setting| value(setting.key()), true)
    }

    #[test]
    fn effects_only_on_swayfx() {
        let corners = |key: &str| (key == "effects.corner_radius").then_some(Value::Integer(8));
        let sway = Look::read(|setting| corners(setting.key()), false);
        assert_eq!(
            layout(&sway, 400.0, 150.0, &TEXT).windows[0].corner_radius,
            0.0
        );
        let look = Look {
            gaps_inner: 4,
            ..swayfx(corners)
        };
        assert_eq!(
            layout(&look, 400.0, 150.0, &TEXT).windows[0].corner_radius,
            8.0
        );
    }

    #[test]
    fn smart_corner_radius_squares_windows_without_gaps() {
        let mut look = swayfx(|key| (key == "effects.corner_radius").then_some(Value::Integer(8)));
        assert_eq!(
            layout(&look, 400.0, 150.0, &TEXT).windows[0].corner_radius,
            0.0
        );
        if let Some(effects) = look.effects.as_mut() {
            effects.smart_corner_radius = false;
        }
        assert_eq!(
            layout(&look, 400.0, 150.0, &TEXT).windows[0].corner_radius,
            8.0
        );
    }

    #[test]
    fn shadows_and_dimming() {
        let inactive = Color::rgba(0x11, 0x22, 0x33, 0x80);
        let look = swayfx(|key| match key {
            "effects.shadows" => Some(Value::Bool(true)),
            "effects.shadow_inactive_color" => Some(Value::Color(inactive)),
            "effects.shadow_offset_y" => Some(Value::Decimal(3.0)),
            "effects.dim_inactive" => Some(Value::Decimal(0.4)),
            _ => None,
        });
        let layout = layout(&look, 400.0, 150.0, &TEXT);
        let [focused, unfocused] = &layout.windows;
        assert_eq!(
            focused
                .shadow
                .map(|shadow| (shadow.color, shadow.blur_radius, shadow.dy)),
            Some((Color::rgba(0, 0, 0, 0x7f), 20.0, 3.0))
        );
        assert_eq!(unfocused.shadow.map(|shadow| shadow.color), Some(inactive));
        assert_eq!(focused.dim, None);
        assert_eq!(unfocused.dim, Some((Color::rgb(0, 0, 0), 0.4)));
        let off = swayfx(|_| None);
        let layout = super::layout(&off, 400.0, 150.0, &TEXT);
        assert_eq!(layout.windows[0].shadow, None);
        assert_eq!(layout.windows[1].dim, None);
    }
}
