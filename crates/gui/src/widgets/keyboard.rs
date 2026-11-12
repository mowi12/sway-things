// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Keyboard page's own parts, which [`PageView`](super::PageView) adds to the rows built from
//! the registry:
//!
//! - below the layouts: the [`LayoutList`] and _Active layout_, which switches the layout in use
//!   now;
//! - the keyboard model as a drop-down of the models of the XKB rules;
//! - below the model, at the end of the section: _Try it_, a field to type in.
//!
//! Each part follows the device the row it belongs to is for.

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::command::input::InputType;
use sway_things_core::devices::Device;
use sway_things_core::registry::Setting;

use super::SettingRow;
use super::layout_list::{LAYOUTS, LayoutList};
use crate::model::Model;
use crate::style;
use crate::xkb;

/// The key of the keyboard model.
const MODEL: &str = "input.keyboard.xkb_model";

/// The choices of a text setting the page offers as a drop-down: the keyboard models, with the
/// default first. `None` for other settings.
pub fn choices(setting: &Setting) -> Option<Vec<(String, String)>> {
    (setting.key() == MODEL).then(|| {
        std::iter::once((String::new(), "Default (pc105)".to_owned()))
            .chain(
                xkb::rules()
                    .models
                    .iter()
                    .filter(|model| !model.extra)
                    .map(|model| {
                        let label = match &model.vendor {
                            Some(vendor) if !model.label().starts_with(vendor.as_str()) => {
                                format!("{vendor} {}", model.label())
                            }
                            _ => model.label().to_owned(),
                        };
                        (model.name.clone(), label)
                    }),
            )
            .collect()
    })
}

/// The widgets the page shows below `row`, each as a row of its own.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let Some(setting) = row.setting() else {
        return Vec::new();
    };
    match setting.key() {
        LAYOUTS => vec![
            LayoutList::new(model, row).upcast(),
            active_layout(model, row),
        ],
        MODEL => vec![try_it()],
        _ => Vec::new(),
    }
}

/// A row of the page's own that looks like a setting row: a title, a subtitle and `control` on
/// the right.
pub fn plain_row(title: &str, subtitle: Option<&str>, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 3);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    let title_label = gtk::Label::new(Some(title));
    title_label.add_css_class("st-row-title");
    texts.append(&title_label);
    if let Some(subtitle) = subtitle {
        let subtitle_label = gtk::Label::new(Some(subtitle));
        subtitle_label.add_css_class(style::DIM);
        texts.append(&subtitle_label);
    }
    let mut child = texts.first_child();
    while let Some(widget) = child {
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        }
        child = widget.next_sibling();
    }
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    content.add_css_class("st-plain-row");
    content.append(&texts);
    control.as_ref().set_valign(gtk::Align::Center);
    content.append(control);
    content
}

/// Calls `refresh` now, whenever the model changes and when `row` is pointed at another device.
pub fn follow(model: &Model, row: &SettingRow, refresh: impl Fn() + 'static) {
    let refresh = std::rc::Rc::new(refresh);
    refresh();
    model.connect_changed(glib::clone!(
        #[strong]
        refresh,
        move |_| refresh()
    ));
    row.connect_device_changed(move |_| refresh());
}

/// The keyboard whose layouts _Active layout_ shows for `device`: that keyboard, or for all of
/// them the first usable one.
fn keyboard_for(model: &Model, device: Option<&str>) -> Option<Device> {
    let devices = model.devices()?;
    match device {
        Some(device) => devices.get(device).cloned(),
        None => devices
            .of_type(InputType::Keyboard)
            .find(|keyboard| keyboard.is_usable_keyboard() && keyboard.layouts().is_some())
            .cloned(),
    }
}

/// _Active layout_: the layouts sway reports for the keyboard, with the one in use picked.
/// Picking another switches to it now, on that keyboard or on all of them.
fn active_layout(model: &Model, row: &SettingRow) -> gtk::Widget {
    let title = "Active layout";
    let labels = gtk::StringList::new(&[]);
    let dropdown = gtk::DropDown::new(Some(labels.clone()), gtk::Expression::NONE);
    dropdown.update_property(&[gtk::accessible::Property::Label(title)]);
    let updating = std::rc::Rc::new(std::cell::Cell::new(false));
    dropdown.connect_selected_notify(glib::clone!(
        #[weak]
        row,
        #[weak]
        model,
        #[strong]
        updating,
        move |dropdown| {
            if updating.get() {
                return;
            }
            if let Ok(index) = u8::try_from(dropdown.selected()) {
                model.switch_layout(row.device().as_deref(), index);
            }
        }
    ));
    let refresh = glib::clone!(
        #[weak]
        row,
        #[weak]
        model,
        #[weak]
        dropdown,
        move || {
            let keyboard = keyboard_for(&model, row.device().as_deref());
            let layouts = keyboard
                .as_ref()
                .filter(|keyboard| keyboard.is_connected())
                .and_then(Device::layouts);
            updating.set(true);
            let names: Vec<&str> = layouts
                .map(|layouts| layouts.names.iter().map(String::as_str).collect())
                .unwrap_or_default();
            let shown: Vec<String> = (0..labels.n_items())
                .filter_map(|index| labels.string(index).map(String::from))
                .collect();
            if shown != names {
                labels.splice(0, labels.n_items(), &names);
            }
            if let Some(active) = layouts
                .and_then(|layouts| layouts.active)
                .and_then(|active| u32::try_from(active).ok())
            {
                dropdown.set_selected(active);
            }
            updating.set(false);
            let reason = if !model.devices_known() {
                Some("Only in a running sway session")
            } else if layouts.is_none() {
                Some("The keyboard isn\u{2019}t connected")
            } else {
                None
            };
            dropdown.set_sensitive(reason.is_none());
            dropdown.set_tooltip_text(reason);
        }
    );
    follow(model, row, refresh);
    plain_row(
        title,
        Some("The layout in use now. sway starts with the first one again"),
        &dropdown,
    )
    .upcast()
}

/// _Try it_: a field to type in with the layouts and options in effect.
fn try_it() -> gtk::Widget {
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Type here to try the keyboard"));
    entry.set_hexpand(true);
    entry.update_property(&[gtk::accessible::Property::Label("Try it")]);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    content.add_css_class("st-plain-row");
    let title = gtk::Label::new(Some("Try it"));
    title.add_css_class("st-row-title");
    title.set_xalign(0.0);
    content.append(&title);
    content.append(&entry);
    content.upcast()
}
