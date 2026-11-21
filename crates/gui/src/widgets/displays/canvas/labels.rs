// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a tile says: the short name, the details and the number, fitted to the tile, and said to
//! screen readers.
use gtk::prelude::*;
use sway_things_core::outputs::{Output, Rect};

use super::GRIP;
use super::geometry::percent;
use crate::i18n::{msg, tr, trf};

/// The name a tile shows, short and readable: _Built-in_ for a laptop's panel, else the brand of
/// the make ([`Output::brand`]), else the model, else the connector. The number on the tile tells
/// two of a brand apart; the whole name is in the tooltip.
pub(crate) fn short_label(output: &Output) -> String {
    if !output.is_connected() {
        return output.label();
    }
    if output.is_built_in() {
        return tr(msg!("Built-in")).to_owned();
    }
    output.brand().unwrap_or_else(|| output.label())
}

/// Shows `number`, `label` (`short` on the tile) and `details` on a tile, and says it all to
/// screen readers.
pub(super) fn set_tile_labels(
    button: &gtk::ToggleButton,
    number: &str,
    label: &str,
    short: &str,
    details: &str,
    rect: Rect,
    off: bool,
) {
    let Some(content) = button.child().and_downcast::<gtk::Box>() else {
        return;
    };
    let labels: Vec<gtk::Label> =
        std::iter::successors(content.first_child(), WidgetExt::next_sibling)
            .filter_map(|child| child.downcast::<gtk::Label>().ok())
            .collect();
    if let [shown_number, name, shown_details] = labels.as_slice() {
        shown_number.set_text(number);
        name.set_text(short);
        shown_details.set_text(details);
    }
    if off {
        button.add_css_class("st-off");
    } else {
        button.remove_css_class("st-off");
    }
    button.set_tooltip_text(Some(label));
    let description = if off {
        trf(
            msg!("Display {number}, {label}, off"),
            &[("number", &number), ("label", &label)],
        )
    } else {
        trf(
            msg!("Display {number}, {label}, {details}, at {x}, {y}"),
            &[
                ("number", &number),
                ("label", &label),
                ("details", &details),
                ("x", &rect.x),
                ("y", &rect.y),
            ],
        )
    };
    button.update_property(&[gtk::accessible::Property::Label(&description)]);
}

/// Shows on `button`, `width` × `height` pixels, as many of its lines as fit: the number always,
/// then the name, then the details (clear of the `grip` if it has one): `full`, or only the
/// `scale` where that doesn't fit. A tile too small for one of them leaves it out, rather than
/// growing past the size of its display or cutting a line in half.
pub(super) fn fit_labels(
    button: &gtk::ToggleButton,
    (width, height): (i32, i32),
    grip: bool,
    full: &str,
    scale: f64,
) {
    let Some((number, name, details)) = tile_labels(button) else {
        return;
    };
    // On the tile with the grip, the last line is centered above the grip's corner: it keeps
    // clear of it on both sides.
    let clear = if grip { 2 * GRIP } else { 0 };
    let room = width - 10;
    // Details too long for the tile: its scale at least, so a change of it shows.
    let compact = ((scale - 1.0).abs() > 0.001).then(|| percent(scale));
    let shown = details.text();
    if shown == full || compact.as_deref() == Some(shown.as_str()) {
        let fits =
            |text: &str| details.create_pango_layout(Some(text)).pixel_size().0 <= room - clear;
        let wanted = match &compact {
            Some(compact) if !fits(full) => compact.as_str(),
            _ => full,
        };
        if shown != wanted {
            details.set_text(wanted);
        }
    }
    // The lines' heights from their texts and fonts, so nothing is shown and hidden again just
    // to measure it (which would lay the canvas out again on every frame).
    let size = |label: &gtk::Label| label.create_pango_layout(Some(&label.text())).pixel_size();
    // The tile's padding and border across and down (style.css), and the space between lines.
    let (frame, gap) = (6, 2);
    let (_, number_height) = size(&number);
    let (name_width, name_height) = size(&name);
    // A name that doesn't fit on one line wraps at its spaces; one without spaces would be cut.
    let wraps = name.text().contains(' ') && name_width <= 2 * room;
    let name_lines = if name_width <= room { 1 } else { 2 };
    let (details_width, details_height) = size(&details);
    let with_name = frame + number_height + gap + name_height * name_lines;
    let with_details = with_name + gap + details_height;
    let show_name = with_name <= height && (name_width <= room || wraps);
    let mut show_name = show_name;
    let mut show_details = show_name
        && with_details <= height
        && details_width <= room - clear
        && !details.text().is_empty();
    // The picked tile (the one with the grip) too small for both: its scale under the number
    // instead of its name, which the picker shows, so a change of the scale shows on it.
    if grip && !show_details {
        if let Some(compact) = &compact {
            let (compact_width, _) = details.create_pango_layout(Some(compact)).pixel_size();
            let fits = frame + number_height + gap + details_height <= height
                && compact_width <= room - clear;
            if fits {
                if details.text() != *compact {
                    details.set_text(compact);
                }
                show_name = false;
                show_details = true;
            }
        }
    }
    if name.is_visible() != show_name {
        name.set_visible(show_name);
    }
    if details.is_visible() != show_details {
        details.set_visible(show_details);
    }
}

/// The number, name and details labels of a tile.
pub(super) fn tile_labels(
    button: &gtk::ToggleButton,
) -> Option<(gtk::Label, gtk::Label, gtk::Label)> {
    let content = button.child().and_downcast::<gtk::Box>()?;
    let mut labels = std::iter::successors(content.first_child(), WidgetExt::next_sibling)
        .filter_map(|child| child.downcast::<gtk::Label>().ok());
    Some((labels.next()?, labels.next()?, labels.next()?))
}
