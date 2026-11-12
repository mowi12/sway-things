// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Text contrast as WCAG 2.2 measures it, read from what GTK actually draws.
//!
//! A widget's snapshot is a tree of render nodes. Every text node says where a text is; the color
//! nodes painted before it, under its center, are its background (each stop of a gradient is a
//! candidate, the worst counts), faded by group opacity on the way down. The text color is read
//! from the rendered pixels: within the text's bounds, the pixel that differs most from the
//! background is the core of a glyph stroke, after every opacity and blend. Antialiasing can only
//! lower that contrast, so the check errs on the strict side. Text node colors aren't used: GTK
//! versions disagree on what their getter returns. The measures themselves are the core's
//! ([`sway_things_core::theme`]).

use gtk::gsk::{self, prelude::*};
use gtk::{gdk, graphene, pango};

pub use sway_things_core::theme::{LARGE_TEXT, NORMAL_TEXT, Rgb, ratio};

/// The color GTK paints, without its alpha.
pub(crate) fn rgb(color: gdk::RGBA) -> Rgb {
    Rgb {
        r: f64::from(color.red()),
        g: f64::from(color.green()),
        b: f64::from(color.blue()),
    }
}

/// One run of text as drawn, with the colors it ends up in.
#[derive(Debug, Clone)]
pub struct TextSample {
    /// Where the text is, in the coordinates of the snapshot's root.
    pub bounds: graphene::Rect,
    /// The font, as Pango describes it.
    pub font: String,
    /// Whether WCAG counts it as large text.
    pub large: bool,
    /// The text and background color with the lowest contrast: over a gradient, the stop closest
    /// to the text.
    pub colors: Result<Colors, Unknown>,
}

/// The text and background color a user sees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    /// The text color after opacity, over the background.
    pub foreground: Rgb,
    /// The background.
    pub background: Rgb,
}

impl Colors {
    /// Their contrast ratio.
    pub fn ratio(self) -> f64 {
        ratio(self.foreground, self.background)
    }
}

impl TextSample {
    /// The contrast ratio the text needs.
    pub fn required(&self) -> f64 {
        if self.large { LARGE_TEXT } else { NORMAL_TEXT }
    }

    /// The contrast ratio of the text, if its background is known.
    pub fn ratio(&self) -> Option<f64> {
        self.colors.as_ref().ok().map(|colors| colors.ratio())
    }
}

/// Why the background of a text couldn't be determined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unknown {
    /// Nothing opaque is painted under the text.
    NoOpaqueBackground,
    /// The text is drawn over a node whose colors can't be read, such as an image.
    Painted(String),
}

/// Something painted before a text.
#[derive(Debug, Clone)]
struct Fill {
    bounds: graphene::Rect,
    paint: Paint,
}

#[derive(Debug, Clone)]
enum Paint {
    /// One color, or the stops of a gradient: each with its alpha after opacity.
    Colors(Vec<(Rgb, f64)>),
    Unknown(String),
}

/// What the walk carries down the tree.
#[derive(Clone)]
struct Context {
    transform: gsk::Transform,
    opacity: f64,
    /// The visible area, in root coordinates; text outside it isn't seen.
    clip: Option<graphene::Rect>,
}

impl Context {
    fn clipped(&self, clip: &graphene::Rect) -> Self {
        let clip = self.transform.transform_bounds(clip);
        let clip = match &self.clip {
            Some(outer) => outer
                .intersection(&clip)
                .unwrap_or_else(graphene::Rect::zero),
            None => clip,
        };
        Self {
            clip: Some(clip),
            ..self.clone()
        }
    }
}

/// The rendered image of a snapshot, to read colors from.
#[derive(Debug, Clone)]
pub struct Pixels {
    width: usize,
    height: usize,
    stride: usize,
    data: Vec<u8>,
    /// Texture pixels per unit of the snapshot's coordinates.
    scale: f32,
}

impl Pixels {
    /// The pixels of `texture`, a rendering of a snapshot that is `width` units wide.
    pub fn new(texture: &gdk::Texture, width: f32) -> Self {
        let mut downloader = gdk::TextureDownloader::new(texture);
        downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
        let (bytes, stride) = downloader.download_bytes();
        let pixels = |size: i32| usize::try_from(size).unwrap_or_default();
        #[expect(
            clippy::cast_precision_loss,
            reason = "texture widths are far below 2^24, where f32 starts to round"
        )]
        let scale = if width > 0.0 {
            texture.width() as f32 / width
        } else {
            1.0
        };
        Self {
            width: pixels(texture.width()),
            height: pixels(texture.height()),
            stride,
            data: bytes.to_vec(),
            scale,
        }
    }

    /// The colors of the pixels within `bounds`.
    fn within(&self, bounds: &graphene::Rect) -> Vec<Rgb> {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "the coordinate is clamped to the texture, whose size is far below 2^24"
        )]
        let index =
            |value: f32, limit: usize| (value * self.scale).clamp(0.0, limit as f32) as usize;
        let (left, right) = (
            index(bounds.x(), self.width),
            index(bounds.x() + bounds.width(), self.width),
        );
        let (top, bottom) = (
            index(bounds.y(), self.height),
            index(bounds.y() + bounds.height(), self.height),
        );
        let mut colors = Vec::new();
        for y in top..bottom {
            for x in left..right {
                let offset = y * self.stride + x * 4;
                if let Some([r, g, b, _]) = self.data.get(offset..offset + 4) {
                    colors.push(Rgb::from_u8(*r, *g, *b));
                }
            }
        }
        colors
    }
}

/// Every text in `node` with its colors, reading the text colors from `pixels`, its rendering.
pub fn text_samples(node: &gsk::RenderNode, pixels: &Pixels) -> Vec<TextSample> {
    let mut fills = Vec::new();
    let mut samples = Vec::new();
    let context = Context {
        transform: gsk::Transform::new(),
        opacity: 1.0,
        clip: None,
    };
    walk(node, &context, pixels, &mut fills, &mut samples);
    samples
}

fn walk(
    node: &gsk::RenderNode,
    context: &Context,
    pixels: &Pixels,
    fills: &mut Vec<Fill>,
    samples: &mut Vec<TextSample>,
) {
    if let Some(container) = node.downcast_ref::<gsk::ContainerNode>() {
        for index in 0..container.n_children() {
            walk(&container.child(index), context, pixels, fills, samples);
        }
        return;
    }
    if let Some((child, inner)) = wrapped(node, context) {
        walk(&child, &inner, pixels, fills, samples);
        return;
    }
    let bounds = || context.transform.transform_bounds(&node.bounds());
    match node.node_type() {
        gsk::RenderNodeType::ColorNode => {
            if let Some(color) = node.downcast_ref::<gsk::ColorNode>() {
                fills.push(Fill {
                    bounds: bounds(),
                    paint: Paint::Colors(vec![paint(color.color(), context)]),
                });
            }
        }
        gsk::RenderNodeType::LinearGradientNode => {
            if let Some(gradient) = node.downcast_ref::<gsk::LinearGradientNode>() {
                let stops = gradient.color_stops();
                fills.push(Fill {
                    bounds: bounds(),
                    paint: Paint::Colors(
                        stops
                            .iter()
                            .map(|stop| paint(*stop.color(), context))
                            .collect(),
                    ),
                });
            }
        }
        gsk::RenderNodeType::TextNode => {
            if let Some(text) = node.downcast_ref::<gsk::TextNode>() {
                let bounds = bounds();
                let visible = context
                    .clip
                    .is_none_or(|clip| clip.contains_point(&center(&bounds)));
                if visible {
                    samples.push(sample(text, bounds, pixels, fills));
                }
            }
        }
        // Lines and shadows around boxes, never under text.
        gsk::RenderNodeType::BorderNode
        | gsk::RenderNodeType::OutsetShadowNode
        | gsk::RenderNodeType::InsetShadowNode => {}
        other => fills.push(Fill {
            bounds: bounds(),
            paint: Paint::Unknown(format!("{other:?}")),
        }),
    }
}

/// The child of a node that wraps one, with the context it is drawn in.
fn wrapped(node: &gsk::RenderNode, context: &Context) -> Option<(gsk::RenderNode, Context)> {
    if let Some(transform) = node.downcast_ref::<gsk::TransformNode>() {
        let inner = Context {
            transform: context
                .transform
                .clone()
                .transform(Some(&transform.transform())),
            ..context.clone()
        };
        Some((transform.child(), inner))
    } else if let Some(opacity) = node.downcast_ref::<gsk::OpacityNode>() {
        let inner = Context {
            opacity: context.opacity * f64::from(opacity.opacity()),
            ..context.clone()
        };
        Some((opacity.child(), inner))
    } else if let Some(clip) = node.downcast_ref::<gsk::ClipNode>() {
        Some((clip.child(), context.clipped(&clip.clip())))
    } else if let Some(clip) = node.downcast_ref::<gsk::RoundedClipNode>() {
        Some((clip.child(), context.clipped(clip.clip().bounds())))
    } else if let Some(debug) = node.downcast_ref::<gsk::DebugNode>() {
        Some((debug.child(), context.clone()))
    } else {
        node.downcast_ref::<gsk::ShadowNode>()
            .map(|shadow| (shadow.child(), context.clone()))
    }
}

fn paint(color: gdk::RGBA, context: &Context) -> (Rgb, f64) {
    (rgb(color), f64::from(color.alpha()) * context.opacity)
}

fn sample(
    text: &gsk::TextNode,
    bounds: graphene::Rect,
    pixels: &Pixels,
    fills: &[Fill],
) -> TextSample {
    let center = center(&bounds);
    // Every color the background can have under the text: one, or one per gradient stop.
    let mut backgrounds: Result<Vec<Rgb>, Unknown> = Ok(Vec::new());
    for fill in fills
        .iter()
        .filter(|fill| fill.bounds.contains_point(&center))
    {
        backgrounds = match &fill.paint {
            Paint::Unknown(kind) => Err(Unknown::Painted(kind.clone())),
            Paint::Colors(colors) => {
                let below = backgrounds.as_deref().unwrap_or_default();
                let mut next = Vec::new();
                for (color, alpha) in colors {
                    if *alpha >= 1.0 {
                        next.push(*color);
                    } else {
                        next.extend(below.iter().map(|below| below.under(*color, *alpha)));
                    }
                }
                if next.is_empty() && backgrounds.is_err() {
                    backgrounds
                } else {
                    Ok(next)
                }
            }
        };
    }

    let description = text.font().describe();
    let drawn = pixels.within(&bounds);
    let colors = backgrounds.and_then(|backgrounds| {
        backgrounds
            .into_iter()
            .filter_map(|background| {
                // The glyph pixel that stands out most from this background.
                drawn
                    .iter()
                    .map(|pixel| Colors {
                        foreground: *pixel,
                        background,
                    })
                    .max_by(|a, b| a.ratio().total_cmp(&b.ratio()))
            })
            .min_by(|a, b| a.ratio().total_cmp(&b.ratio()))
            .ok_or(Unknown::NoOpaqueBackground)
    });
    TextSample {
        bounds,
        font: description.to_string(),
        large: is_large(&description),
        colors,
    }
}

fn center(bounds: &graphene::Rect) -> graphene::Point {
    graphene::Point::new(
        bounds.x() + bounds.width() / 2.0,
        bounds.y() + bounds.height() / 2.0,
    )
}

/// Whether WCAG counts text in this font as large: 18 pt (24 px), or 14 pt (18.66 px) and bold.
fn is_large(description: &pango::FontDescription) -> bool {
    let size = f64::from(description.size()) / f64::from(pango::SCALE);
    let points = if description.is_size_absolute() {
        size * 72.0 / 96.0
    } else {
        size
    };
    let bold = description.weight() >= pango::Weight::Bold;
    points >= 18.0 || (bold && points >= 14.0)
}

#[cfg(test)]
mod tests {
    use super::{Rgb, ratio};

    fn approx(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 0.01, "{actual} != {expected}");
    }

    #[test]
    fn black_on_white_is_21() {
        approx(
            ratio(Rgb::from_u8(0, 0, 0), Rgb::from_u8(255, 255, 255)),
            21.0,
        );
    }

    #[test]
    fn same_color_is_1() {
        let gray = Rgb::from_u8(0x77, 0x77, 0x77);
        approx(ratio(gray, gray), 1.0);
    }

    #[test]
    fn known_pairs_match_the_wcag_formula() {
        // #767676 on white is the classic lightest gray that passes 4.5:1.
        approx(
            ratio(Rgb::from_u8(0x76, 0x76, 0x76), Rgb::from_u8(255, 255, 255)),
            4.54,
        );
        // sway's default focus color under white text.
        approx(
            ratio(Rgb::from_u8(255, 255, 255), Rgb::from_u8(0x28, 0x55, 0x77)),
            7.91,
        );
    }

    #[test]
    fn ratio_is_symmetric() {
        let a = Rgb::from_u8(0x2e, 0x34, 0x36);
        let b = Rgb::from_u8(0xf6, 0xf5, 0xf4);
        approx(ratio(a, b), ratio(b, a));
    }

    #[test]
    fn alpha_mixes_toward_the_background() {
        let white = Rgb::from_u8(255, 255, 255);
        let black = Rgb::from_u8(0, 0, 0);
        assert_eq!(white.under(black, 1.0), black);
        assert_eq!(white.under(black, 0.0), white);
        approx(white.under(black, 0.5).r, 0.5);
    }

    #[test]
    fn dim_label_opacity_fails_in_gtk_light_theme() {
        // GTK's Default theme: #2e3436 text at opacity 0.55 on the window color #f6f5f4.
        let background = Rgb::from_u8(0xf6, 0xf5, 0xf4);
        let text = background.under(Rgb::from_u8(0x2e, 0x34, 0x36), 0.55);
        assert!(ratio(text, background) < 4.5);
    }
}
