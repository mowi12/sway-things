// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The note below a switch of the on-screen display of SwayOSD: while the switch is on and
//! `swayosd-server` doesn't run, the keys do nothing, so it says so and how to start the server
//! with sway. Sway Things never starts it itself.

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bindings::ours::Group;
use sway_things_core::registry::Value;
use sway_things_core::swayosd::{self, Server};

use super::label;
use super::tilde;
use crate::i18n::{msg, tr, trf};
use crate::model::Model;

/// The note below the switch of the on-screen display of `group`, following `model` and checked
/// again whenever it is shown and the window comes back to the front.
pub(super) fn note(model: &Model, group: Group) -> gtk::Widget {
    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    icon.set_valign(gtk::Align::Start);
    icon.add_css_class("st-shortcut-warning");
    let text = label("", &[]);
    let note = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    note.add_css_class("st-shortcut-clash");
    note.add_css_class("st-shortcut-osd");
    note.append(&icon);
    note.append(&text);
    note.set_visible(false);
    let follow = std::rc::Rc::new(glib::clone!(
        #[weak]
        note,
        #[weak]
        text,
        move |model: &Model| {
            let said = server_text(model, group);
            note.set_visible(said.is_some());
            if let Some(said) = said {
                text.set_text(&said);
                note.update_property(&[gtk::accessible::Property::Label(&said)]);
            }
        }
    ));
    follow(model);
    let changed = follow.clone();
    model.connect_changed(move |model| changed(model));
    // Started from a terminal meanwhile: checked again when shown and when the window comes back.
    let window_followed = std::cell::Cell::new(false);
    let weak = model.downgrade();
    note.connect_map(move |note| {
        let Some(model) = weak.upgrade() else { return };
        follow(&model);
        let window = note.root().and_downcast::<gtk::Window>();
        if let (Some(window), false) = (window, window_followed.replace(true)) {
            let follow = follow.clone();
            let weak = weak.clone();
            window.connect_is_active_notify(move |window| {
                if let (true, Some(model)) = (window.is_active(), weak.upgrade()) {
                    follow(&model);
                }
            });
        }
    });
    note.upcast()
}

/// What the note says while the display of `group` is on and its server doesn't run; `None`
/// otherwise.
pub(crate) fn server_text(model: &Model, group: Group) -> Option<String> {
    let setting = sway_things_core::registry::Registry::builtin().get(group.osd)?;
    let on = model
        .view(setting, None)
        .is_some_and(|view| view.value == Value::Bool(true));
    if !on {
        return None;
    }
    let status = model.status()?;
    let locations = model.locations()?;
    Some(
        match swayosd::server(status.config(), locations.directory()) {
            Server::Runs => return None,
            Server::StartedByConfig { file, line } => trf(
                msg!(
                    "swayosd-server isn't running, so these keys do nothing. Your config starts \
                     it with sway at {place}: start it now with swayosd-server, or log in again."
                ),
                &[("place", &format!("{}:{line}", tilde(&file)))],
            ),
            Server::Unit(unit) => trf(
                msg!(
                    "swayosd-server isn't running, so these keys do nothing. The service {unit} \
                     should start it: see systemctl --user status {unit}."
                ),
                &[("unit", &unit)],
            ),
            Server::Missing => tr(msg!(
                "swayosd-server isn't running, so these keys do nothing. Add exec \
                 swayosd-server to your sway config to start it with sway, and start it now \
                 with swayosd-server."
            ))
            .to_owned(),
        },
    )
}

/// Whether the switch of the on-screen display `setting` belongs on the page: while
/// `swayosd-client` is installed, or while it is stored, so it can be switched off.
pub(super) fn has_row(model: &Model, setting: &sway_things_core::registry::Setting) -> bool {
    swayosd::installed_in(model.search_path().as_deref())
        || model.stored_in(setting, None).is_some()
}
