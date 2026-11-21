// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Bar page's own parts, which [`PageView`](super::PageView) adds below the rows built from
//! the registry:
//!
//! - below the bar program: how the bar runs ([`status`]): what starts waybar and the line to
//!   change there, a bar of the user's own next to ours, the import of the user's waybar config;
//! - below the displays: a check button per display;
//! - below each module list: its modules as chips ([`layout::ModuleArea`]), which move by
//!   dragging or with Alt and the arrow keys, open their settings ([`dialog`]) and are added from
//!   the catalog or a template;
//! - below the look: the presets.

mod dialog;
mod layout;
mod status;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bar::style::{PRESET_KEYS, PRESETS, preset_values};
use sway_things_core::registry::{Value, bar as keys};

use super::SettingRow;
use crate::i18n::{Msg, msg, tr};
use crate::model::Model;
use crate::style;

/// The widgets the Bar page shows below `row`.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let Some(setting) = row.setting() else {
        return Vec::new();
    };
    match setting.key() {
        keys::BACKEND => vec![status::Status::new(model).widget().clone().upcast()],
        keys::OUTPUTS => vec![outputs(model, row)],
        keys::MODULES_LEFT | keys::MODULES_CENTER | keys::MODULES_RIGHT => {
            vec![
                layout::ModuleArea::new(model, row)
                    .widget()
                    .clone()
                    .upcast(),
            ]
        }
        keys::THEME => vec![presets(model)],
        _ => Vec::new(),
    }
}

/// Calls `refresh` now and whenever `model` changes, while `widget` exists.
pub(crate) fn follow(model: &Model, widget: &impl IsA<gtk::Widget>, refresh: impl Fn() + 'static) {
    refresh();
    let weak = widget.as_ref().downgrade();
    let handler = Rc::new(RefCell::new(None));
    let id = model.connect_changed(move |_| {
        if weak.upgrade().is_some() {
            refresh();
        }
    });
    handler.replace(Some(id));
    widget.as_ref().connect_destroy(glib::clone!(
        #[weak]
        model,
        move |_| {
            if let Some(id) = handler.take() {
                model.disconnect(id);
            }
        }
    ));
}

/// The list a list setting's row shows.
pub(crate) fn list_of(row: &SettingRow) -> Vec<String> {
    match row.value() {
        Some(Value::List(items)) => items,
        _ => Vec::new(),
    }
}

/// A check button per display, connected or named in the list, below the displays row: the
/// bar shows on those checked, or on all while none is.
fn outputs(model: &Model, row: &SettingRow) -> gtk::Widget {
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_max_children_per_line(4);
    flow.set_row_spacing(4);
    flow.set_column_spacing(12);
    flow.add_css_class("st-bar-outputs");
    flow.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Displays that show the bar"
    )))]);
    let updating = Rc::new(std::cell::Cell::new(false));
    let refresh = glib::clone!(
        #[weak]
        flow,
        #[weak]
        row,
        #[weak]
        model,
        #[strong]
        updating,
        move || {
            updating.set(true);
            while let Some(child) = flow.first_child() {
                flow.remove(&child);
            }
            let chosen = list_of(&row);
            let mut names: Vec<String> = model
                .outputs()
                .map(|outputs| {
                    outputs
                        .iter()
                        .filter(|output| output.is_connected())
                        .map(|output| output.name().to_owned())
                        .collect()
                })
                .unwrap_or_default();
            for name in &chosen {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            for name in names {
                let check = gtk::CheckButton::with_label(&name);
                check.set_active(chosen.contains(&name));
                check.set_sensitive(row.locked().is_none());
                check.connect_toggled(glib::clone!(
                    #[weak]
                    row,
                    #[strong]
                    updating,
                    move |check| {
                        if updating.get() {
                            return;
                        }
                        let mut list = list_of(&row);
                        list.retain(|item| *item != name);
                        if check.is_active() {
                            list.push(name.clone());
                        }
                        row.change(Value::List(list));
                    }
                ));
                flow.append(&check);
            }
            updating.set(false);
        }
    );
    follow(model, &flow, refresh);
    flow.upcast()
}

/// What a preset does, as its tooltip says.
const PRESET_HINT: Msg = msg!("Sets the style and the placement it needs at once");

/// The presets below the look: each sets the style settings (and the placement it needs) at
/// once; the settings it doesn't name go back to their defaults.
fn presets(model: &Model) -> gtk::Widget {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.add_css_class("st-bar-presets");
    let label = gtk::Label::new(Some(tr(msg!("Presets"))));
    label.add_css_class(style::DIM);
    line.append(&label);
    for preset in PRESETS {
        let button = gtk::Button::with_label(tr(preset.title));
        button.set_tooltip_text(Some(&format!(
            "{}. {}",
            tr(preset.description),
            tr(PRESET_HINT)
        )));
        let id = preset.id;
        button.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |_| apply_preset(&model, id)
        ));
        line.append(&button);
    }
    let refresh = glib::clone!(
        #[weak]
        line,
        #[weak]
        model,
        move || {
            let writable = model.status().is_some_and(|status| status.is_active());
            line.set_sensitive(writable);
        }
    );
    follow(model, &line, refresh);
    line.upcast()
}

/// Stores the values of preset `id`, and resets the other settings a preset may set.
fn apply_preset(model: &Model, id: &str) {
    let values = preset_values(id);
    let swayfx = model
        .compositor()
        .is_some_and(sway_things_core::ipc::Compositor::is_swayfx);
    for &key in PRESET_KEYS {
        // Blur needs SwayFX; on sway the rest of the preset still applies.
        if key == keys::BLUR && !swayfx {
            continue;
        }
        match values.iter().find(|(preset_key, _)| *preset_key == key) {
            Some((_, value)) => model.set(key, None, value.clone()),
            None => model.reset(key, None),
        }
    }
}
