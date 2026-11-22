// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The ink and toner levels of a printer: on its line, a small upright bar per supply in its
//! colors, with the levels as the tooltip; in its details, a bar per supply with its name and
//! level as text, which screen readers read too.

use gtk::prelude::*;
use sway_things_core::printers::{Level, Marker};

use crate::i18n::{msg, tr, trf};
use crate::style;

/// The width and height of a level bar.
const BAR: (i32, i32) = (56, 8);
/// The width and height of a small upright bar on a printer's line.
const SMALL_BAR: (i32, i32) = (6, 22);

/// Fills `container` with a line per supply in `markers`, replacing what was there.
pub fn fill(container: &gtk::Box, markers: &[Marker]) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    container.set_visible(!markers.is_empty());
    for marker in markers {
        container.append(&line(marker));
    }
}

/// Fills `container` with a small upright bar per supply in `markers`, replacing what was there;
/// the names and levels are its tooltip.
pub fn fill_compact(container: &gtk::Box, markers: &[Marker]) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    container.set_visible(!markers.is_empty());
    for marker in markers {
        container.append(&bar(marker, SMALL_BAR, true));
    }
    let said: Vec<String> = markers
        .iter()
        .map(|marker| format!("{}: {}", name_of(marker), level_text(marker)))
        .collect();
    container.set_tooltip_text((!said.is_empty()).then(|| said.join("\n")).as_deref());
}

/// The name of a supply, or _Supply_ if it has none.
pub fn name_of(marker: &Marker) -> String {
    if marker.name.is_empty() {
        tr(msg!("Supply")).to_owned()
    } else {
        marker.name.clone()
    }
}

/// The level of `marker` in words: `55 %`, `Some left`, `Unknown`, with `low` or `full` when it
/// needs attention.
pub fn level_text(marker: &Marker) -> String {
    let level = match marker.level {
        Level::Percent(percent) => trf(msg!("{percent} %"), &[("percent", &percent)]),
        // TRANSLATORS: A supply of a printer whose exact level isn't known, but isn't empty.
        Level::SomeLeft => tr(msg!("Some left")).to_owned(),
        // TRANSLATORS: The level of a supply of a printer that isn't known.
        Level::Unknown => tr(msg!("Level unknown")).to_owned(),
    };
    if !marker.needs_attention() {
        return level;
    }
    let attention = if marker.is_waste() {
        // TRANSLATORS: A waste container of a printer that is (almost) full.
        tr(msg!("full"))
    } else {
        // TRANSLATORS: A supply of a printer that is low.
        tr(msg!("low"))
    };
    format!("{level}, {attention}")
}

/// A supply: the bar, its name and its level.
fn line(marker: &Marker) -> gtk::Box {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.append(&bar(marker, BAR, false));
    let text = gtk::Label::new(Some(&format!(
        "{}: {}",
        name_of(marker),
        level_text(marker)
    )));
    text.set_xalign(0.0);
    text.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.add_css_class(style::DIM);
    line.append(&text);
    line
}

/// The bar of `marker`, `size` large, filled from the bottom if `upright`, else from the left.
fn bar(marker: &Marker, size: (i32, i32), upright: bool) -> gtk::DrawingArea {
    let bar = gtk::DrawingArea::builder()
        .content_width(size.0)
        .content_height(size.1)
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    let colors = marker.colors.clone();
    let fraction = match marker.level {
        Level::Percent(percent) => f64::from(percent) / 100.0,
        Level::SomeLeft => 0.5,
        Level::Unknown => 0.0,
    };
    let known = marker.level != Level::Unknown;
    bar.set_draw_func(move |area, cairo, width, height| {
        draw(
            area,
            cairo,
            (width, height),
            &colors,
            (fraction, known),
            upright,
        );
    });
    bar
}

/// Draws a bar: its outline in the text color, the level filled in the supply's colors (in bands
/// for a cartridge with several), from the bottom if `upright`, else from the left; nothing
/// filled for an unknown level.
fn draw(
    area: &gtk::DrawingArea,
    cairo: &gtk::cairo::Context,
    (width, height): (i32, i32),
    colors: &[(u8, u8, u8)],
    (fraction, known): (f64, bool),
    upright: bool,
) {
    let width = f64::from(width);
    let height = f64::from(height);
    let fg = area.color();
    let outline = |cairo: &gtk::cairo::Context| {
        cairo.set_source_rgba(
            f64::from(fg.red()),
            f64::from(fg.green()),
            f64::from(fg.blue()),
            0.6,
        );
        cairo.rectangle(0.5, 0.5, width - 1.0, height - 1.0);
        cairo.set_line_width(1.0);
        let _ = cairo.stroke();
    };
    if known {
        let fraction = fraction.clamp(0.0, 1.0);
        let fill: &[(u8, u8, u8)] = if colors.is_empty() {
            &[(0x80, 0x80, 0x80)]
        } else {
            colors
        };
        let count = f64::from(u32::try_from(fill.len()).unwrap_or(1));
        for (i, (r, g, b)) in fill.iter().enumerate() {
            let i = f64::from(u32::try_from(i).unwrap_or(0));
            cairo.set_source_rgb(
                f64::from(*r) / 255.0,
                f64::from(*g) / 255.0,
                f64::from(*b) / 255.0,
            );
            if upright {
                let filled = (height - 2.0) * fraction;
                let band = (width - 2.0) / count;
                cairo.rectangle(1.0 + band * i, height - 1.0 - filled, band, filled);
            } else {
                let filled = (width - 2.0) * fraction;
                let band = (height - 2.0) / count;
                cairo.rectangle(1.0, 1.0 + band * i, filled, band);
            }
            let _ = cairo.fill();
        }
    }
    outline(cairo);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(kind: &str, level: Level) -> Marker {
        Marker {
            name: "Black".to_owned(),
            kind: kind.to_owned(),
            colors: Vec::new(),
            level,
            low: 10,
            high: 90,
        }
    }

    #[test]
    fn levels_are_said_in_words() {
        assert_eq!(level_text(&marker("toner", Level::Percent(55))), "55 %");
        assert_eq!(level_text(&marker("toner", Level::Percent(5))), "5 %, low");
        assert_eq!(
            level_text(&marker("waste-toner", Level::Percent(95))),
            "95 %, full"
        );
        assert_eq!(level_text(&marker("ink", Level::SomeLeft)), "Some left");
        assert_eq!(level_text(&marker("ink", Level::Unknown)), "Level unknown");
    }
}
