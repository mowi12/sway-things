// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! "Identify": the number of each output, shown big on that output for a moment, so it is
//! clear which tile of the canvas is which screen.
//!
//! Each number is a window of its own on the overlay layer of the `wlr-layer-shell` protocol
//! (gtk4-layer-shell), in the middle of its output. It takes no keyboard and no pointer, so
//! nothing the user does goes to it. The output is found among GDK's
//! monitors by its connector, the name sway gives it.

use std::time::Duration;

use gtk::gdk;
use gtk::prelude::*;
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};

/// How long the numbers stay.
pub const SHOWN_FOR: Duration = Duration::from_secs(3);

/// Whether the numbers can be shown: the compositor speaks `wlr-layer-shell`, which sway does.
pub fn is_supported() -> bool {
    gtk4_layer_shell::is_supported()
}

/// Shows `number` and `label` on the output with the connector `name`, for each entry, until
/// the windows it returns are destroyed. Outputs GDK doesn't know are skipped.
pub fn show(outputs: &[(String, usize, String)]) -> Vec<gtk::Window> {
    if !is_supported() {
        return Vec::new();
    }
    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let monitors = display.monitors();
    let mut shown = Vec::new();
    for (name, number, label) in outputs {
        let monitor = (0..monitors.n_items())
            .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
            .find(|monitor| monitor.connector().as_deref() == Some(name.as_str()));
        let Some(monitor) = monitor else {
            continue;
        };
        let window = number_window(*number, label);
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_namespace(Some("sway-things-identify"));
        window.set_monitor(Some(&monitor));
        window.present();
        shown.push(window);
    }
    shown
}

/// The window with `number` and `label`: plain text, the label being an external string.
fn number_window(number: usize, label: &str) -> gtk::Window {
    let shown_number = gtk::Label::new(Some(&number.to_string()));
    shown_number.add_css_class("st-identify-number");
    let name = gtk::Label::new(Some(label));
    name.add_css_class("st-identify-name");
    name.set_wrap(true);
    name.set_justify(gtk::Justification::Center);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&shown_number);
    content.append(&name);
    let window = gtk::Window::new();
    window.add_css_class("st-identify");
    window.set_child(Some(&content));
    window.set_can_target(false);
    // Clicks go through to what is below: the window takes no part of the screen's input.
    window.connect_realize(|window| {
        if let Some(surface) = window.surface() {
            surface.set_input_region(&gtk::cairo::Region::create());
        }
    });
    window
}
