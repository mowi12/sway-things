// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The style sheet of the app.
//!
//! It covers structure only (spacing, sizes, the dimmed text level). Colors come from the GTK
//! theme, so light, dark and custom themes keep working.

use gtk::gdk;

/// The CSS of the app.
pub const CSS: &str = include_str!("style.css");

/// The style class for secondary text. It replaces the theme's `dim-label`, which is too faint for
/// WCAG AA contrast in GTK's own theme.
pub const DIM: &str = "st-dim";

/// Loads the style sheet for the default display. Call it once, after GTK is initialized.
pub fn load() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
