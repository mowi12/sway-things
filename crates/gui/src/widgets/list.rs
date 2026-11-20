// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The building blocks of a section as every page draws it: a heading, a bordered list of rows,
//! a row with its title and subtitle on the left and its controls on the right, and the hint in
//! place of an empty list. The pages built from the registry and the live pages (Bluetooth,
//! Network, Sound) use the same ones, so they look alike.

use gtk::prelude::*;

use crate::style;

/// A section heading as on the pages built from the registry; `first` has no space above it.
pub fn heading(title: &str, first: bool) -> gtk::Label {
    let label = gtk::Label::new(Some(&title.to_uppercase()));
    style_heading(&label, first);
    label
}

/// Styles `label` as a section heading; `first` has no space above it.
pub fn style_heading(label: &gtk::Label, first: bool) {
    label.set_xalign(0.0);
    label.set_margin_start(4);
    label.set_margin_bottom(8);
    label.set_margin_top(if first { 0 } else { 24 });
    label.set_accessible_role(gtk::AccessibleRole::Heading);
    label.add_css_class("st-section-heading");
    label.add_css_class(style::DIM);
}

/// A bordered list for the rows of a section named `title`.
pub fn section_list(title: &str) -> gtk::ListBox {
    let list = gtk::ListBox::new();
    section(&list, title);
    list
}

/// Makes `list` a bordered list of a section named `title`.
pub fn section(list: &gtk::ListBox, title: &str) {
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("st-section");
    list.update_property(&[gtk::accessible::Property::Label(title)]);
}

/// Styles `label` as the hint shown in place of an empty list, hidden until it has a text.
pub fn hint(label: &gtk::Label) {
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.add_css_class(style::DIM);
    label.add_css_class("st-section-hint");
    label.set_visible(false);
}

/// Shows `text` in `label`, or hides it.
pub fn set_hint(label: &gtk::Label, text: Option<&str>) {
    label.set_text(text.unwrap_or_default());
    label.set_visible(text.is_some());
}

/// `child` as a row of a section list. The row itself takes no focus; its controls do.
pub fn list_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let item = gtk::ListBoxRow::new();
    item.set_child(Some(child));
    plain_row(&item);
    item
}

/// Makes `row` a row that takes no focus and can't be activated; its controls can.
pub fn plain_row(row: &gtk::ListBoxRow) {
    row.set_activatable(false);
    row.set_focusable(false);
}

/// A row with `title` above `subtitle` on the left and `controls` on the right.
pub fn row(title: &gtk::Label, subtitle: &gtk::Label, controls: &[&gtk::Widget]) -> gtk::Box {
    for label in [title, subtitle] {
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    }
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(title);
    text.append(subtitle);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.append(&text);
    for control in controls {
        line.append(*control);
    }
    line
}

/// Puts `rows` into `list` in this order, moving only what is out of place, so a row with the
/// focus keeps it unless it moves.
pub fn arrange(list: &gtk::ListBox, rows: &[gtk::ListBoxRow]) {
    let mut current = Vec::new();
    let mut child = list.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        current.push(widget);
    }
    let wanted: Vec<gtk::Widget> = rows.iter().map(|row| row.clone().upcast()).collect();
    if current == wanted {
        return;
    }
    let focus = list.root().and_then(|root| root.focus()).filter(|focus| {
        rows.iter()
            .any(|row| focus.is_ancestor(row) || focus == row)
    });
    for row in rows {
        remove_row(row);
    }
    for row in rows {
        list.append(row);
    }
    if let Some(focus) = focus {
        focus.grab_focus();
    }
}

/// Takes `row` out of the list it is in.
pub fn remove_row(row: &gtk::ListBoxRow) {
    if let Some(list) = row.parent().and_downcast::<gtk::ListBox>() {
        list.remove(row);
    }
}

/// The texts of the labels in `widget` and below it, in order.
pub fn labels_in(widget: &gtk::Widget) -> Vec<String> {
    let mut texts = Vec::new();
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(label) = current.downcast_ref::<gtk::Label>() {
            texts.push(label.text().to_string());
        }
        texts.extend(labels_in(&current));
        child = current.next_sibling();
    }
    texts
}
