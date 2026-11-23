// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The parts of the dialog of a binding ([`Editor`](super::Editor)) that don't need its state: the
//! modifiers held, the keys taken with them, the details of a binding, the text of a clash with
//! the keys pressed, and the apps it offers.

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bindings::{
    BindCommand, DEFAULT_MODE, Device, Keymap, Listing, Modifiers, Origin, Row, RowKind,
};

use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;

/// The modifiers of `state`, as sway names them.
pub(super) fn held_in(state: gdk::ModifierType) -> Modifiers {
    [
        (gdk::ModifierType::SUPER_MASK, Modifiers::SUPER),
        (gdk::ModifierType::ALT_MASK, Modifiers::ALT),
        (gdk::ModifierType::CONTROL_MASK, Modifiers::CTRL),
        (gdk::ModifierType::SHIFT_MASK, Modifiers::SHIFT),
    ]
    .into_iter()
    .filter(|(mask, _)| state.contains(*mask))
    .fold(Modifiers::NONE, |all, (_, modifier)| all.union(modifier))
}

/// The modifier the key `keyval` is, if it is one.
pub(super) fn modifier_key(keyval: gdk::Key) -> Modifiers {
    match keyval.name().as_deref() {
        Some("Super_L" | "Super_R" | "Meta_L" | "Meta_R") => Modifiers::SUPER,
        Some("Alt_L" | "Alt_R") => Modifiers::ALT,
        Some("Control_L" | "Control_R") => Modifiers::CTRL,
        Some("Shift_L" | "Shift_R") => Modifiers::SHIFT,
        _ => Modifiers::NONE,
    }
}

/// The keys in use with exactly the modifiers `held` in the default mode, each with what it
/// does, sorted by key.
pub(super) fn taken(
    listing: &Listing,
    held: Modifiers,
    keymap: Option<&dyn Keymap>,
) -> Vec<(String, String)> {
    let mut keys: Vec<(String, String)> = listing
        .rows()
        .iter()
        .filter(|row| {
            row.mode == DEFAULT_MODE
                && row.in_effect
                && row.line.combo.device == Device::Keyboard
                && row.line.combo.modifiers == held
                && row.line.combo.keys.len() == 1
        })
        .filter_map(|row| {
            let labels = row.line.labels(listing.modifier(), keymap);
            let skip = row.line.combo.modifiers.labels().len();
            let key = labels
                .get(skip..)?
                .iter()
                .map(sway_things_core::i18n::Text::translated)
                .collect::<Vec<_>>()
                .join(" + ");
            Some((key, super::title_of(row).0))
        })
        .collect();
    keys.sort_by(|a, b| {
        (a.0.chars().count(), a.0.to_lowercase()).cmp(&(b.0.chars().count(), b.0.to_lowercase()))
    });
    keys.dedup_by(|a, b| a.0 == b.0);
    keys
}

/// The details of `row`: its command, its options in words, where it is with _Open in Editor_,
/// and what Sway Things made of it.
pub(super) fn details(
    model: &Model,
    row: &Row,
    listing: &Listing,
    keymap: Option<&dyn Keymap>,
    read_only: bool,
) -> gtk::Box {
    let grid = gtk::Grid::new();
    grid.set_column_spacing(16);
    grid.set_row_spacing(8);
    let mut at = 0;
    let mut add = |name: Msg, value: &gtk::Widget| {
        let name = gtk::Label::new(Some(tr(name)));
        name.set_xalign(0.0);
        name.set_yalign(0.0);
        name.add_css_class(style::DIM);
        grid.attach(&name, 0, at, 1, 1);
        grid.attach(value, 1, at, 1, 1);
        at += 1;
    };
    let command = value(row.line.command.as_deref().unwrap_or_default());
    command.add_css_class("monospace");
    add(msg!("Command"), command.upcast_ref());
    let options = super::options(row.line.flags, row.line.code);
    if !options.is_empty() {
        add(msg!("Options"), value(&options.join(" · ")).upcast_ref());
    }
    let place = super::place(&row.source);
    let from = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let place_label = value(&place);
    from.append(&place_label);
    if row.source.origin == Origin::User && !row.source.file.as_os_str().is_empty() {
        let open = gtk::Button::with_label(tr(msg!("Open in Editor")));
        open.set_halign(gtk::Align::Start);
        open.add_css_class("st-shortcut-open");
        if read_only {
            open.add_css_class("suggested-action");
        }
        let file = row.source.file.to_path_buf();
        let line = row.source.line;
        open.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |button| {
                let reporter = model.clone();
                crate::editor::open(button, Some(&model), &file, Some(line), move |error| {
                    if let Some(error) = error {
                        reporter.emit_problem(error);
                    }
                });
            }
        ));
        from.append(&open);
    }
    add(msg!("Defined in"), from.upcast_ref());
    // What Sway Things made of it; a binding of the user's as it is says no more than its place
    // and the reason it can't be changed.
    let status = super::status_lines(row, listing, keymap).pop();
    if let Some(status) = status.filter(|_| row.kind != RowKind::Hand) {
        add(msg!("Status"), value(&status).upcast_ref());
    }
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_top(10);
    content.append(&grid);
    if row.kind == RowKind::Hand && !read_only {
        let note = value(tr(msg!(
            "Your config stays as it is: a change removes this binding in Sway Things' own file \
             with unbindsym and binds the new keys there."
        )));
        note.add_css_class(style::DIM);
        content.append(&note);
    }
    content
}

/// A value of the details: wrapping, selectable text.
fn value(text: &str) -> gtk::Label {
    let label = super::label(text, &[]);
    label.set_hexpand(true);
    label.set_selectable(true);
    label
}

/// A line with an icon and a wrapping text, with the style class `class`.
pub(super) fn icon_line(class: &str) -> (gtk::Box, gtk::Image, gtk::Label) {
    let icon = gtk::Image::from_icon_name("changes-prevent-symbolic");
    icon.set_valign(gtk::Align::Start);
    let text = super::label("", &[]);
    text.set_hexpand(true);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.add_css_class(class);
    line.append(&icon);
    line.append(&text);
    line.set_visible(false);
    (line, icon, text)
}

/// What a row the new binding clashes with says: what it does and where, and what saving does to
/// it.
pub(super) fn conflict_text(other: &Row, same: bool, fixed: bool) -> String {
    let what = super::title_of(other).0;
    let place = super::place(&other.source);
    let args: &[(&str, &dyn std::fmt::Display)] = &[("what", &what), ("place", &place)];
    match (same, fixed) {
        (true, true) => trf(
            msg!(
                "Taken by \u{201c}{what}\u{201d} ({place}), which can't be replaced here. Pick other keys."
            ),
            args,
        ),
        (true, false) => trf(
            msg!("In use by \u{201c}{what}\u{201d} ({place}). Saving puts this in its place."),
            args,
        ),
        (false, _) => trf(
            msg!("Also on these keys: \u{201c}{what}\u{201d} ({place})."),
            args,
        ),
    }
}

/// The drop-down of the apps, by name, and the command line of each, without the field codes of
/// its desktop entry.
pub(super) fn apps() -> (gtk::DropDown, Vec<String>) {
    let mut apps: Vec<(String, String)> = gtk::gio::AppInfo::all()
        .into_iter()
        .filter(gtk::gio::AppInfo::should_show)
        .filter_map(|app| {
            let line = app.commandline()?;
            let line = without_field_codes(&line.to_string_lossy());
            BindCommand::new(&format!("exec {line}"))
                .ok()
                .map(|_| (app.display_name().to_string(), line))
        })
        .collect();
    apps.sort_by_key(|(name, _)| name.to_lowercase());
    apps.dedup();
    let names: Vec<&str> = apps.iter().map(|(name, _)| name.as_str()).collect();
    let dropdown = gtk::DropDown::from_strings(&names);
    dropdown.set_enable_search(true);
    dropdown.set_search_match_mode(gtk::StringFilterMatchMode::Substring);
    (dropdown, apps.into_iter().map(|(_, line)| line).collect())
}

/// `line` (an `Exec` of a desktop entry) without its field codes (`%f`, `%U`, …); `%%` is `%`.
pub fn without_field_codes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '%' {
            if characters.next() == Some('%') {
                out.push('%');
            }
        } else {
            out.push(character);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_held_are_read() {
        let state = gdk::ModifierType::SUPER_MASK | gdk::ModifierType::SHIFT_MASK;
        assert_eq!(held_in(state), Modifiers::SUPER.union(Modifiers::SHIFT));
        assert_eq!(held_in(gdk::ModifierType::empty()), Modifiers::NONE);
        assert_eq!(modifier_key(gdk::Key::Super_L), Modifiers::SUPER);
        assert_eq!(modifier_key(gdk::Key::Control_R), Modifiers::CTRL);
        assert_eq!(modifier_key(gdk::Key::q), Modifiers::NONE);
    }

    #[test]
    fn field_codes_are_left_out() {
        assert_eq!(without_field_codes("firefox %u"), "firefox");
        assert_eq!(
            without_field_codes("app --name=%c %F --x"),
            "app --name= --x"
        );
        assert_eq!(without_field_codes("printf 100%%"), "printf 100%");
    }
}
