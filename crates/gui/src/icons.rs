// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The icons the app brings along, compiled into the binary: symbolic icons from Lucide, which GTK
//! recolors with the text color like any symbolic icon of the theme.

use gtk::{gdk, gio, glib};

/// The globe of the link to the website.
pub const WEBSITE: &str = "st-globe-symbolic";
/// The code brackets of the link to the source code.
pub const SOURCE_CODE: &str = "st-code-symbolic";
/// The bug of the link to the issue tracker.
pub const BUG: &str = "st-bug-symbolic";

/// Where the icons are in the resource bundle.
const RESOURCE_PATH: &str = "/io/github/mowi12/SwayThings/icons";

/// Registers the icons with the icon theme of the default display. Call it once, after GTK is
/// initialized.
pub fn register() {
    let bundle = glib::Bytes::from_static(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/sway-things.gresource"
    )));
    match gio::Resource::from_data(&bundle) {
        Ok(resource) => gio::resources_register(&resource),
        Err(error) => {
            glib::g_warning!("sway-things", "the icons can't be loaded: {error}");
            return;
        }
    }
    if let Some(display) = gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_resource_path(RESOURCE_PATH);
    }
}
