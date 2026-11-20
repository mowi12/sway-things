// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A run of sections of per-device settings of one input type, or of per-output settings, with
//! the selector for the device or output their rows are for, and the hint in their place while
//! no device of the type is there.
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::view::Source;

use crate::model::Model;
use crate::style;
use crate::widgets::device_selector::{Instances, none_connected_of};
use crate::widgets::list::list_row;
use crate::widgets::{DeviceSelector, GroupRow, SettingRow};

/// A run of sections of per-device settings of one input type, or of per-output settings, with
/// the selector for the device or output their rows are for.
pub(super) struct DeviceGroup {
    pub(super) kind: Instances,
    pub(super) selector: DeviceSelector,
    /// Shown in place of the rows while there is nothing to set.
    pub(super) hint: gtk::Label,
    /// The header and the list of each section.
    pub(super) sections: Vec<(gtk::Widget, gtk::ListBox)>,
    pub(super) rows: Vec<SettingRow>,
    /// The groups of settings among the rows, whose summary follows the device too.
    pub(super) group_rows: Vec<GroupRow>,
    /// Whether the sections being built still belong to the group.
    pub(super) open: bool,
}

impl DeviceGroup {
    /// A group of type `kind` that starts with the section of `heading` and `list`. The selector
    /// becomes the first row of the list.
    pub(super) fn new(kind: Instances, heading: gtk::Widget, list: &gtk::ListBox) -> Self {
        let selector = DeviceSelector::of(kind);
        list.append(&list_row(&selector));
        let hint = gtk::Label::new(Some(none_connected_of(kind)));
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        hint.add_css_class("st-section-hint");
        hint.add_css_class(style::DIM);
        hint.set_visible(false);
        Self {
            kind,
            selector,
            hint,
            sections: vec![(heading, list.clone())],
            rows: Vec::new(),
            group_rows: Vec::new(),
            open: true,
        }
    }

    /// Points the rows at the device picked, and keeps the device list and what is shown current
    /// with `model`.
    pub(super) fn follow(self, model: &Model) {
        let rows: Vec<glib::WeakRef<SettingRow>> =
            self.rows.iter().map(ObjectExt::downgrade).collect();
        let group_rows: Vec<glib::WeakRef<GroupRow>> =
            self.group_rows.iter().map(ObjectExt::downgrade).collect();
        self.selector.connect_device_changed(glib::clone!(
            #[strong]
            rows,
            move |_, device| {
                for row in rows.iter().filter_map(glib::WeakRef::upgrade) {
                    row.set_device(device);
                }
                for group in group_rows.iter().filter_map(glib::WeakRef::upgrade) {
                    group.refresh();
                }
            }
        ));
        let sections: Vec<(glib::WeakRef<gtk::Widget>, glib::WeakRef<gtk::ListBox>)> = self
            .sections
            .iter()
            .map(|(heading, list)| (heading.downgrade(), list.downgrade()))
            .collect();
        let kind = self.kind;
        let update = glib::clone!(
            #[weak(rename_to = selector)]
            self.selector,
            #[weak(rename_to = hint)]
            self.hint,
            move |model: &Model| {
                let connected = match kind {
                    Instances::Devices(kind) => {
                        let Some(devices) = model.devices() else {
                            return;
                        };
                        selector.update(&devices);
                        devices.of_type(kind).next().is_some()
                    }
                    // The value for every output also applies to screens plugged in later.
                    Instances::Outputs => {
                        let Some(outputs) = model.outputs() else {
                            return;
                        };
                        selector.update_outputs(&outputs);
                        true
                    }
                };
                let stored = rows
                    .iter()
                    .filter_map(glib::WeakRef::upgrade)
                    .filter_map(|row| row.setting())
                    .filter_map(|setting| model.view(setting, None))
                    .any(|view| matches!(view.source, Source::Stored { .. }));
                let shown = !model.devices_known() || connected || stored;
                for (index, (heading, list)) in sections.iter().enumerate() {
                    if let Some(heading) = heading.upgrade() {
                        heading.set_visible(shown || index == 0);
                    }
                    if let Some(list) = list.upgrade() {
                        list.set_visible(shown);
                    }
                }
                hint.set_visible(!shown);
            }
        );
        update(model);
        model.connect_changed(update);
    }
}
