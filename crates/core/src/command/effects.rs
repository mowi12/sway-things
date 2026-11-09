// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builders for the SwayFX effects on the Windows page: rounded corners, blur, shadows, dimming
//! and animations. Directive names and ranges come from SwayFX 0.5, the oldest supported release.
//!
//! Every command records [`SwayFxVersion::OLDEST_SUPPORTED`] (or newer) in
//! [`SwayCmd::min_swayfx`]: vanilla sway rejects these directives.
//!
//! ```
//! use sway_things_core::command::{effects, Target};
//! use sway_things_core::version::SwayFxVersion;
//!
//! let blur = effects::blur_passes(3)?;
//! assert_eq!(blur.render(Target::Config)?, "blur_passes 3");
//! assert_eq!(blur.min_swayfx(), Some(SwayFxVersion::OLDEST_SUPPORTED));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::{BoolStyle, Color, OutOfRange, SwayCmd};
use crate::version::SwayFxVersion;

fn effect(directive: &'static str) -> SwayCmd {
    SwayCmd::new(directive).requires_swayfx(SwayFxVersion::OLDEST_SUPPORTED)
}

fn checked_int(directive: &'static str, value: u16, max: u16) -> Result<SwayCmd, OutOfRange> {
    OutOfRange::check(directive, f64::from(value), 0.0, f64::from(max))?;
    Ok(effect(directive).int(i64::from(value)))
}

fn checked_float(directive: &'static str, value: f64, max: f64) -> Result<SwayCmd, OutOfRange> {
    OutOfRange::check(directive, value, 0.0, max)?;
    Ok(effect(directive).float(value))
}

/// `corner_radius <px>` (0 … 99) for new windows. SwayFX raises the horizontal title bar padding
/// to at least the radius.
///
/// # Errors
///
/// [`OutOfRange`] above 99.
pub fn corner_radius(px: u8) -> Result<SwayCmd, OutOfRange> {
    checked_int("corner_radius", u16::from(px), 99)
}

/// `smart_corner_radius`: rounded corners only if there are gaps around the window.
pub fn smart_corner_radius(enabled: bool) -> SwayCmd {
    effect("smart_corner_radius").bool(enabled, BoolStyle::EnableDisable)
}

/// `blur`: blur behind transparent windows.
pub fn blur(enabled: bool) -> SwayCmd {
    effect("blur").bool(enabled, BoolStyle::EnableDisable)
}

/// `blur_passes` (0 … 10, 0 turns blur off).
///
/// # Errors
///
/// [`OutOfRange`] above 10.
pub fn blur_passes(passes: u8) -> Result<SwayCmd, OutOfRange> {
    checked_int("blur_passes", u16::from(passes), 10)
}

/// `blur_radius` (0 … 10, 0 turns blur off).
///
/// # Errors
///
/// [`OutOfRange`] above 10.
pub fn blur_radius(radius: u8) -> Result<SwayCmd, OutOfRange> {
    checked_int("blur_radius", u16::from(radius), 10)
}

/// `blur_noise` (0 … 1).
///
/// # Errors
///
/// [`OutOfRange`] outside 0 … 1 or for NaN.
pub fn blur_noise(noise: f64) -> Result<SwayCmd, OutOfRange> {
    checked_float("blur_noise", noise, 1.0)
}

/// `blur_brightness` (0 … 2, 1 keeps the brightness).
///
/// # Errors
///
/// [`OutOfRange`] outside 0 … 2 or for NaN.
pub fn blur_brightness(brightness: f64) -> Result<SwayCmd, OutOfRange> {
    checked_float("blur_brightness", brightness, 2.0)
}

/// `blur_contrast` (0 … 2, 1 keeps the contrast).
///
/// # Errors
///
/// [`OutOfRange`] outside 0 … 2 or for NaN.
pub fn blur_contrast(contrast: f64) -> Result<SwayCmd, OutOfRange> {
    checked_float("blur_contrast", contrast, 2.0)
}

/// `blur_saturation` (0 … 2, 1 keeps the saturation).
///
/// # Errors
///
/// [`OutOfRange`] outside 0 … 2 or for NaN.
pub fn blur_saturation(saturation: f64) -> Result<SwayCmd, OutOfRange> {
    checked_float("blur_saturation", saturation, 2.0)
}

/// `shadows`
pub fn shadows(enabled: bool) -> SwayCmd {
    effect("shadows").bool(enabled, BoolStyle::EnableDisable)
}

/// `shadow_color`, usually with alpha (SwayFX's default is `#0000007f`).
pub fn shadow_color(color: Color) -> SwayCmd {
    effect("shadow_color").color(color)
}

/// `shadow_inactive_color`: the shadow of unfocused windows.
pub fn shadow_inactive_color(color: Color) -> SwayCmd {
    effect("shadow_inactive_color").color(color)
}

/// `shadow_blur_radius` (0 … 99, 0 turns the blur off).
///
/// # Errors
///
/// [`OutOfRange`] above 99.
pub fn shadow_blur_radius(radius: u8) -> Result<SwayCmd, OutOfRange> {
    checked_int("shadow_blur_radius", u16::from(radius), 99)
}

/// `shadow_offset <x> <y>` in pixels (each −99.9 … 99.9).
///
/// # Errors
///
/// [`OutOfRange`] if either offset is outside −99.9 … 99.9 or NaN.
pub fn shadow_offset(x: f64, y: f64) -> Result<SwayCmd, OutOfRange> {
    OutOfRange::check("shadow_offset", x, -99.9, 99.9)?;
    OutOfRange::check("shadow_offset", y, -99.9, 99.9)?;
    Ok(effect("shadow_offset").float(x).float(y))
}

/// `default_dim_inactive` (0 … 1): how much unfocused windows are dimmed.
///
/// # Errors
///
/// [`OutOfRange`] outside 0 … 1 or for NaN.
pub fn default_dim_inactive(amount: f64) -> Result<SwayCmd, OutOfRange> {
    checked_float("default_dim_inactive", amount, 1.0)
}

/// Which windows `dim_inactive_colors.<class>` applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DimClass {
    /// `dim_inactive_colors.unfocused`
    Unfocused,
    /// `dim_inactive_colors.urgent`
    Urgent,
}

/// `dim_inactive_colors.<class>`: the color unfocused windows are dimmed with.
pub fn dim_inactive_color(class: DimClass, color: Color) -> SwayCmd {
    effect(match class {
        DimClass::Unfocused => "dim_inactive_colors.unfocused",
        DimClass::Urgent => "dim_inactive_colors.urgent",
    })
    .color(color)
}

/// `animation_duration_ms` (0 … 5000) for opening and closing windows. Needs SwayFX 0.6.
///
/// # Errors
///
/// [`OutOfRange`] above 5000.
pub fn animation_duration_ms(milliseconds: u16) -> Result<SwayCmd, OutOfRange> {
    Ok(checked_int("animation_duration_ms", milliseconds, 5000)?
        .requires_swayfx(SwayFxVersion::new(0, 6, 0)))
}

#[cfg(test)]
mod tests;
