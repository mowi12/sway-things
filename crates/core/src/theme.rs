// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Colors as WCAG 2.2 measures them, and the accent the app takes from sway's focus color.
//!
//! The accent is the `client.focused` child border. Where it fills a control (a switch, a
//! selection), it is used as it is, with black or white on it, whichever reads better
//! ([`Accent::on_fill`]). Where it is drawn on the window background (accent text, a focus
//! ring), its lightness is shifted until it reaches [`NORMAL_TEXT`] against that background
//! ([`Accent::on_background`]), so it stays readable on light and dark themes alike.
//!
//! ```
//! use sway_things_core::command::Color;
//! use sway_things_core::theme::{Accent, Rgb, ratio, NORMAL_TEXT};
//!
//! let dark = Rgb::from_u8(0x24, 0x24, 0x24);
//! let accent = Accent::new(Color::rgb(0x28, 0x55, 0x77), dark);
//! assert_eq!(accent.fill.to_string(), "#285577");
//! assert_eq!(accent.on_fill.to_string(), "#ffffff");
//! assert!(ratio(accent.on_background, dark) >= NORMAL_TEXT);
//! ```

use std::fmt;

use crate::command::Color;

/// The contrast WCAG AA asks of normal text.
pub const NORMAL_TEXT: f64 = 4.5;

/// The contrast WCAG AA asks of large text (18 pt, or 14 pt bold).
pub const LARGE_TEXT: f64 = 3.0;

/// An opaque color with channels from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    /// Red
    pub r: f64,
    /// Green
    pub g: f64,
    /// Blue
    pub b: f64,
}

impl Rgb {
    /// Black.
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    };

    /// White.
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
    };

    /// A color from 8-bit channels.
    pub fn from_u8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: f64::from(r) / 255.0,
            g: f64::from(g) / 255.0,
            b: f64::from(b) / 255.0,
        }
    }

    /// The relative luminance (WCAG 2.2, definition "relative luminance").
    pub fn luminance(self) -> f64 {
        fn linear(channel: f64) -> f64 {
            if channel <= 0.040_45 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// `color` with `alpha` painted over this color.
    #[must_use]
    pub fn under(self, color: Self, alpha: f64) -> Self {
        let alpha = alpha.clamp(0.0, 1.0);
        let mix = |top: f64, bottom: f64| top * alpha + bottom * (1.0 - alpha);
        Self {
            r: mix(color.r, self.r),
            g: mix(color.g, self.g),
            b: mix(color.b, self.b),
        }
    }

    /// The color as hue (0 … 1), saturation and lightness.
    fn to_hsl(self) -> (f64, f64, f64) {
        let max = self.r.max(self.g).max(self.b);
        let min = self.r.min(self.g).min(self.b);
        let lightness = f64::midpoint(max, min);
        let delta = max - min;
        if delta == 0.0 {
            return (0.0, 0.0, lightness);
        }
        let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
        #[expect(
            clippy::float_cmp,
            reason = "`max` is one of the channels, so it equals that one exactly"
        )]
        let sector = if max == self.r {
            ((self.g - self.b) / delta).rem_euclid(6.0)
        } else if max == self.g {
            (self.b - self.r) / delta + 2.0
        } else {
            (self.r - self.g) / delta + 4.0
        };
        (sector / 6.0, saturation, lightness)
    }

    /// The color of `hue` (0 … 1), `saturation` and `lightness`.
    fn from_hsl(hue: f64, saturation: f64, lightness: f64) -> Self {
        let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
        let sector = hue * 6.0;
        let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
        let (red, green, blue) = match sector {
            s if s < 1.0 => (chroma, second, 0.0),
            s if s < 2.0 => (second, chroma, 0.0),
            s if s < 3.0 => (0.0, chroma, second),
            s if s < 4.0 => (0.0, second, chroma),
            s if s < 5.0 => (second, 0.0, chroma),
            _ => (chroma, 0.0, second),
        };
        let offset = lightness - chroma / 2.0;
        Self {
            r: (red + offset).clamp(0.0, 1.0),
            g: (green + offset).clamp(0.0, 1.0),
            b: (blue + offset).clamp(0.0, 1.0),
        }
    }
}

impl From<Color> for Rgb {
    /// The color without its alpha.
    fn from(color: Color) -> Self {
        let [r, g, b] = color.channels();
        Self::from_u8(r, g, b)
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the channel is clamped to 0..=255 first"
        )]
        let byte = |channel: f64| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
        write!(
            f,
            "#{:02x}{:02x}{:02x}",
            byte(self.r),
            byte(self.g),
            byte(self.b)
        )
    }
}

/// The contrast ratio of two colors, from 1 (none) to 21 (black on white).
pub fn ratio(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (a.luminance(), b.luminance());
    let (light, dark) = if a > b { (a, b) } else { (b, a) };
    (light + 0.05) / (dark + 0.05)
}

/// Black or white, whichever has more contrast on `fill`.
pub fn text_on(fill: Rgb) -> Rgb {
    if ratio(Rgb::BLACK, fill) >= ratio(Rgb::WHITE, fill) {
        Rgb::BLACK
    } else {
        Rgb::WHITE
    }
}

/// `color` with its lightness shifted away from `background` until it reaches `minimum`
/// contrast against it; `color` itself if it already does. Hue and saturation stay, so the
/// color stays recognizable. If no lightness reaches `minimum`, the one that comes closest.
pub fn readable_on(color: Rgb, background: Rgb, minimum: f64) -> Rgb {
    if ratio(color, background) >= minimum {
        return color;
    }
    let (hue, saturation, lightness) = color.to_hsl();
    // Lighter on a dark background, darker on a light one.
    let lighter = background.luminance() < 0.18;
    let mut best = color;
    for step in 1..=100 {
        let shift = f64::from(step) / 100.0;
        let shifted = if lighter {
            (lightness + shift).min(1.0)
        } else {
            (lightness - shift).max(0.0)
        };
        let candidate = Rgb::from_hsl(hue, saturation, shifted);
        if ratio(candidate, background) > ratio(best, background) {
            best = candidate;
        }
        if ratio(candidate, background) >= minimum {
            return candidate;
        }
    }
    best
}

/// The accent colors of the app, made from one color for one window background.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Accent {
    /// The color as it is: what accented controls are filled with.
    pub fill: Rgb,
    /// Black or white, for text and icons on [`Accent::fill`].
    pub on_fill: Rgb,
    /// The color where it is drawn on the window background (accent text, focus rings): readable
    /// there with at least [`NORMAL_TEXT`].
    pub on_background: Rgb,
}

impl Accent {
    /// The accent of `color` (its alpha left out) on a window with `background`.
    pub fn new(color: Color, background: Rgb) -> Self {
        let fill = Rgb::from(color);
        Self {
            fill,
            on_fill: text_on(fill),
            on_background: readable_on(fill, background, NORMAL_TEXT),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Accent, NORMAL_TEXT, Rgb, ratio, readable_on, text_on};
    use crate::command::Color;

    const LIGHT: Rgb = Rgb {
        r: 0.98,
        g: 0.98,
        b: 0.98,
    };
    const DARK: Rgb = Rgb {
        r: 0.14,
        g: 0.14,
        b: 0.14,
    };

    #[test]
    fn contrast_is_measured_as_wcag_defines_it() {
        assert!((ratio(Rgb::BLACK, Rgb::WHITE) - 21.0).abs() < 1e-9);
        assert!((ratio(LIGHT, LIGHT) - 1.0).abs() < 1e-9);
        // The order doesn't matter.
        let blue = Rgb::from_u8(0x28, 0x55, 0x77);
        assert!((ratio(blue, LIGHT) - ratio(LIGHT, blue)).abs() < 1e-9);
        assert_eq!(Rgb::from(Color::rgba(0x28, 0x55, 0x77, 0x80)), blue);
    }

    #[test]
    fn text_on_a_fill_is_black_or_white_whichever_reads_better() {
        assert_eq!(text_on(Rgb::from_u8(0x28, 0x55, 0x77)), Rgb::WHITE);
        assert_eq!(text_on(Rgb::from_u8(0xf0, 0xc6, 0x74)), Rgb::BLACK);
        // Every gray gets at least 4.5:1 from one of the two.
        for level in 0..=255 {
            let gray = Rgb::from_u8(level, level, level);
            assert!(ratio(text_on(gray), gray) >= NORMAL_TEXT, "{gray}");
        }
    }

    #[test]
    fn a_color_on_a_background_is_shifted_only_as_far_as_needed() {
        let blue = Rgb::from_u8(0x28, 0x55, 0x77);
        // Readable on light already: kept.
        assert_eq!(readable_on(blue, LIGHT, NORMAL_TEXT), blue);
        // Too dark on dark: lighter, readable, still blue.
        let lifted = readable_on(blue, DARK, NORMAL_TEXT);
        assert!(ratio(lifted, DARK) >= NORMAL_TEXT, "{lifted}");
        assert!(lifted.luminance() > blue.luminance());
        assert!(lifted.b > lifted.r, "{lifted}");
        // Too light on light: darker.
        let yellow = Rgb::from_u8(0xf0, 0xc6, 0x74);
        let darkened = readable_on(yellow, LIGHT, NORMAL_TEXT);
        assert!(ratio(darkened, LIGHT) >= NORMAL_TEXT, "{darkened}");
        assert!(darkened.luminance() < yellow.luminance());
    }

    #[test]
    fn every_accent_is_readable_on_light_and_dark() {
        for (r, g, b) in [
            (0x28, 0x55, 0x77),
            (0xff, 0xff, 0x00),
            (0x00, 0x00, 0x00),
            (0xff, 0xff, 0xff),
            (0x3b, 0x6e, 0x4f),
            (0xe0, 0x1b, 0x24),
        ] {
            for background in [LIGHT, DARK] {
                let accent = Accent::new(Color::rgb(r, g, b), background);
                assert!(
                    ratio(accent.on_background, background) >= NORMAL_TEXT,
                    "{} on {background}",
                    accent.on_background
                );
                let best = ratio(Rgb::BLACK, accent.fill).max(ratio(Rgb::WHITE, accent.fill));
                assert!(
                    (ratio(accent.on_fill, accent.fill) - best).abs() < 1e-9,
                    "text on {}",
                    accent.fill
                );
            }
        }
    }
}
