// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A line of the list of bindings: what the binding does on the left, its keys as keycaps on the
//! right, and between them, only when there is something to say, a warning sign (not in effect),
//! a lock (can't be changed here) and the button that puts back the user's binding. The reasons are the tooltips;
//! the whole story is in the dialog a click on the line opens.

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bindings::{ClashKind, Listing, Row, RowKind, edit};
use sway_things_core::sway_config::bindings::Keymap;

use super::{clash_text, keycaps, labels, locked_reason, store, title_of};
use crate::i18n::{msg, tr};
use crate::model::Model;
use crate::style;

/// A row of the list and the box with its content.
pub(super) struct Built {
    /// The row to put in the list: activating it opens the dialog.
    pub item: gtk::ListBoxRow,
    /// Its content, with the style class `st-shortcut-row`.
    pub content: gtk::Box,
}

/// The row of the binding `row` at `index` of `listing`.
pub(super) fn build(
    model: &Model,
    listing: &Listing,
    index: usize,
    row: &Row,
    keymap: Option<&dyn Keymap>,
    writable: bool,
) -> Built {
    let (title, is_command) = title_of(row);
    let title_label = gtk::Label::new(Some(&title));
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title_label.add_css_class("st-shortcut-title");
    if is_command {
        title_label.add_css_class("st-shortcut-command");
        title_label.set_tooltip_text(Some(&title));
    }
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.add_css_class("st-shortcut-row");
    content.append(&title_label);
    let removed = matches!(row.kind, RowKind::Removed { .. });
    let mut said = Vec::new();
    if removed {
        let tag = gtk::Label::new(Some(tr(msg!("Removed"))));
        tag.add_css_class("st-shortcut-tag");
        tag.add_css_class(style::DIM);
        content.append(&tag);
    } else if let Some(sign) = clash_sign(row) {
        said.push(
            sign.tooltip_text()
                .map(|text| text.to_string())
                .unwrap_or_default(),
        );
        content.append(&sign);
    }
    if let Some(reason) = locked_reason(row) {
        let lock = gtk::Image::from_icon_name("changes-prevent-symbolic");
        lock.add_css_class("st-shortcut-lock");
        lock.add_css_class(style::DIM);
        lock.set_tooltip_text(Some(&reason));
        content.append(&lock);
        said.push(reason);
    }
    if edit::can_restore(row) {
        content.append(&restore_button(model, index, writable));
    }
    let keys = labels(row, listing, keymap);
    let caps = keycaps(&keys);
    caps.set_halign(gtk::Align::End);
    if removed {
        caps.add_css_class("st-keycaps-off");
    }
    content.append(&caps);
    let item = gtk::ListBoxRow::new();
    item.set_child(Some(&content));
    item.set_activatable(true);
    item.add_css_class("st-shortcut-item");
    let mut name = format!("{title}, {}", keys.join(" + "));
    if removed {
        name.push_str(", ");
        name.push_str(tr(msg!("Removed")));
    }
    item.update_property(&[gtk::accessible::Property::Label(&name)]);
    if !said.is_empty() {
        item.update_property(&[gtk::accessible::Property::Description(&said.join(" "))]);
    }
    Built { item, content }
}

/// The warning sign of a row whose binding isn't in effect, or only on the lock screen, with
/// the clashes in words as its tooltip. A binding that wins over another needs no sign: its
/// dialog tells.
fn clash_sign(row: &Row) -> Option<gtk::Image> {
    let problem = !row.in_effect
        || row.clashes.iter().any(|clash| {
            matches!(
                clash.kind,
                ClashKind::ReplacedBy
                    | ClashKind::RemovedBy
                    | ClashKind::Shadowed
                    | ClashKind::LockScreenOnly
            )
        });
    if !problem {
        return None;
    }
    let mut text: Vec<String> = row.clashes.iter().map(clash_text).collect();
    if text.is_empty() {
        text.push(tr(msg!("Not in effect")).to_owned());
    }
    let sign = gtk::Image::from_icon_name("dialog-warning-symbolic");
    sign.add_css_class("st-shortcut-warning");
    sign.set_tooltip_text(Some(&text.join("\n")));
    Some(sign)
}

/// The button that puts back the user's binding that Sway Things changed or removes.
fn restore_button(model: &Model, index: usize, writable: bool) -> gtk::Button {
    let button = gtk::Button::from_icon_name("edit-undo-symbolic");
    button.add_css_class("flat");
    button.add_css_class("circular");
    button.set_valign(gtk::Align::Center);
    button.set_sensitive(writable);
    let title = tr(msg!("Back to Your Config's Binding"));
    button.set_tooltip_text(Some(title));
    button.update_property(&[gtk::accessible::Property::Label(title)]);
    button.connect_clicked(glib::clone!(
        #[weak]
        model,
        move |_| store(&model, |listing, items| edit::restore(
            listing, items, index
        ))
    ));
    button
}
