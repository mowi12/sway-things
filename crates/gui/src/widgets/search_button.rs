// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The _Search_ button of the Bluetooth and Network pages: a flat button with an icon and a label
//! at the right of the heading of the other devices or networks. While a search runs, a spinner takes the
//! icon's place and the label reads _Stop_, at the same width.

use crate::i18n::{msg, tr};
use gtk::prelude::*;

/// The names of the labels in their stack.
const SEARCH: &str = "search";
const STOP: &str = "stop";
const ICON: &str = "icon";
const SPINNER: &str = "spinner";

/// Makes `button` a _Search_ button.
pub fn setup(button: &gtk::Button) {
    let icons = gtk::Stack::new();
    icons.add_named(
        &gtk::Image::from_icon_name("view-refresh-symbolic"),
        Some(ICON),
    );
    let labels = gtk::Stack::builder().hhomogeneous(true).build();
    for (name, text) in [(SEARCH, msg!("Search")), (STOP, msg!("Stop"))] {
        labels.add_named(&gtk::Label::new(Some(tr(text))), Some(name));
    }
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&icons);
    content.append(&labels);
    button.set_child(Some(&content));
    button.add_css_class("flat");
}

/// `line`, the heading line with the margins of every heading line, and `button` over it: centered
/// on `heading`, at the right end. The button may reach up into the empty space above the heading
/// but never below the line, so it doesn't make the line taller and never covers the list below,
/// however tall the theme draws buttons.
pub fn over(line: &gtk::Box, heading: &gtk::Label, button: &gtk::Button) -> gtk::Overlay {
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(line));
    overlay.add_overlay(button);
    let heading = heading.clone();
    let below = line.margin_bottom();
    overlay.connect_get_child_position(move |overlay, child| {
        let (_, natural) = child.preferred_size();
        let (width, height) = (natural.width(), natural.height());
        let bottom = overlay.height();
        // The heading fills the line between its margins, its text centered.
        let center = bottom - below - heading.height() / 2;
        let y = (center - height / 2).min(bottom - height);
        Some(gtk::gdk::Rectangle::new(
            overlay.width() - width,
            y,
            width,
            height,
        ))
    });
    overlay
}

/// The stacks of the icon and of the label.
fn stacks(button: &gtk::Button) -> Option<(gtk::Stack, gtk::Stack)> {
    let content = button.child()?;
    let icons = content.first_child().and_downcast::<gtk::Stack>()?;
    let labels = icons.next_sibling().and_downcast::<gtk::Stack>()?;
    Some((icons, labels))
}

/// Shows the spinner and _Stop_ while a search runs, the icon and _Search_ otherwise; `tooltip`
/// says what the button does.
pub fn set_running(button: &gtk::Button, running: bool, tooltip: &str) {
    let text = if running { STOP } else { SEARCH };
    if let Some((icons, labels)) = stacks(button) {
        if let Some(old) = icons.child_by_name(SPINNER) {
            icons.remove(&old);
        }
        if running {
            // A new spinner each time: a spinner's animation doesn't restart by itself once it
            // has left the screen.
            let spinner = gtk::Spinner::new();
            spinner.set_spinning(true);
            icons.add_named(&spinner, Some(SPINNER));
            icons.set_visible_child_name(SPINNER);
        } else {
            icons.set_visible_child_name(ICON);
        }
        labels.set_visible_child_name(text);
    }
    button.update_property(&[gtk::accessible::Property::Label(text)]);
    button.set_tooltip_text((!tooltip.is_empty()).then_some(tooltip));
}

/// What the button reads: _Search_ or _Stop_.
pub fn text(button: &gtk::Button) -> String {
    stacks(button)
        .and_then(|(_, labels)| labels.visible_child_name())
        .map(|name| name.to_string())
        .unwrap_or_default()
}

/// Whether the button shows a turning spinner.
pub fn is_spinning(button: &gtk::Button) -> bool {
    stacks(button).is_some_and(|(icons, _)| {
        icons.visible_child_name().as_deref() == Some(SPINNER)
            && icons
                .visible_child()
                .and_downcast::<gtk::Spinner>()
                .is_some_and(|spinner| spinner.is_spinning())
    })
}
