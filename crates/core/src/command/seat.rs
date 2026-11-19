// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builders for `seat * …` commands (`man 5 sway-input`), used by the Cursor & Tablet page.
//!
//! Sway Things sets every seat at once: `seat *` merges into each seat, and sway takes the cursor
//! theme of the default seat for `XCURSOR_THEME` and `XCURSOR_SIZE`.
//!
//! ```
//! use sway_things_core::command::{SwayText, Target, seat};
//!
//! let theme = seat::xcursor_theme(&SwayText::new("Adwaita")?, 32)?;
//! assert_eq!(theme.render(Target::Ipc)?, r#"seat * xcursor_theme "Adwaita" 32"#);
//! assert_eq!(seat::hide_cursor_when_typing(true).render(Target::Config)?,
//!     "seat * hide_cursor when-typing enable");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::{BoolStyle, OutOfRange, SwayCmd, SwayText};

/// The smallest cursor size the builders accept.
pub const MIN_CURSOR_SIZE: u32 = 8;
/// The largest cursor size the builders accept; Xcursor files store sizes as 32-bit numbers, but
/// no theme ships more than a few hundred pixels.
pub const MAX_CURSOR_SIZE: u32 = 512;

/// The shortest timeout sway takes for `hide_cursor`: it raises shorter ones (other than 0) to
/// this.
pub const MIN_HIDE_TIMEOUT_MS: u32 = 100;

fn command(option: &'static str) -> SwayCmd {
    SwayCmd::new("seat").keyword("*").keyword(option)
}

/// `xcursor_theme <theme> <size>`: the cursor theme and size sway draws, and the ones apps it
/// starts afterwards get in `XCURSOR_THEME` and `XCURSOR_SIZE`.
///
/// # Errors
///
/// [`OutOfRange`] for a size outside [`MIN_CURSOR_SIZE`] … [`MAX_CURSOR_SIZE`].
pub fn xcursor_theme(theme: &SwayText, size: u32) -> Result<SwayCmd, OutOfRange> {
    OutOfRange::check(
        "xcursor_theme",
        f64::from(size),
        f64::from(MIN_CURSOR_SIZE),
        f64::from(MAX_CURSOR_SIZE),
    )?;
    Ok(command("xcursor_theme")
        .text(theme.clone())
        .int(i64::from(size)))
}

/// `hide_cursor <ms>`: hides the cursor after this many milliseconds without movement; 0 never
/// hides it.
///
/// # Errors
///
/// [`OutOfRange`] for a timeout between 1 and 99 ms, which sway would quietly raise to
/// [`MIN_HIDE_TIMEOUT_MS`], or one above `i32::MAX`.
pub fn hide_cursor(milliseconds: u32) -> Result<SwayCmd, OutOfRange> {
    if milliseconds != 0 {
        OutOfRange::check(
            "hide_cursor",
            f64::from(milliseconds),
            f64::from(MIN_HIDE_TIMEOUT_MS),
            f64::from(i32::MAX),
        )?;
    }
    Ok(command("hide_cursor").int(i64::from(milliseconds)))
}

/// `hide_cursor when-typing enable|disable`: hides the cursor while a key is pressed.
pub fn hide_cursor_when_typing(enabled: bool) -> SwayCmd {
    command("hide_cursor")
        .keyword("when-typing")
        .bool(enabled, BoolStyle::EnableDisable)
}

#[cfg(test)]
mod tests;
