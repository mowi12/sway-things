// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The style sheet of the app.
//!
//! It covers structure only (spacing, sizes, the dimmed text level). Colors come from the GTK
//! theme, so light, dark and custom themes keep working.

use gtk::gdk;

/// The CSS of the app.
pub const CSS: &str = include_str!("style.css");

/// The CSS of states that carry meaning, above the user's own `gtk.css`: a display that is off.
pub const STATES: &str = include_str!("style-states.css");

/// The style class for secondary text. It replaces the theme's `dim-label`, which is too faint for
/// WCAG AA contrast in GTK's own theme.
pub const DIM: &str = "st-dim";

/// The style class of a row a deep link points to: tinted with the accent for a moment.
pub const HIGHLIGHT: &str = "st-highlight";

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
    let states = gtk::CssProvider::new();
    states.load_from_string(STATES);
    gtk::style_context_add_provider_for_display(
        &display,
        &states,
        gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
    );
}
