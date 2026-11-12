// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::str::FromStr;

/// A color in sway's notation: `#rrggbb`, or `#rrggbbaa` with alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    red: u8,
    green: u8,
    blue: u8,
    alpha: Option<u8>,
}

impl Color {
    /// An opaque color, rendered as `#rrggbb`.
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: None,
        }
    }

    /// The red, green and blue channels, without the alpha.
    pub const fn channels(self) -> [u8; 3] {
        [self.red, self.green, self.blue]
    }

    /// A color with alpha, rendered as `#rrggbbaa`.
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: Some(alpha),
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)?;
        if let Some(alpha) = self.alpha {
            write!(f, "{alpha:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Color {
    type Err = ColorError;

    /// Parses `#rrggbb` or `#rrggbbaa` (hex digits in either case).
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let digits = text.strip_prefix('#').ok_or(ColorError)?;
        if !matches!(digits.len(), 6 | 8) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ColorError);
        }
        let channel = |index: usize| {
            digits
                .get(index..index + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or(ColorError)
        };
        let alpha = if digits.len() == 8 {
            Some(channel(6)?)
        } else {
            None
        };
        Ok(Self {
            red: channel(0)?,
            green: channel(2)?,
            blue: channel(4)?,
            alpha,
        })
    }
}

/// The string isn't a color in `#rrggbb` or `#rrggbbaa` notation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorError;

impl fmt::Display for ColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a color as #rrggbb or #rrggbbaa")
    }
}

impl std::error::Error for ColorError {}
