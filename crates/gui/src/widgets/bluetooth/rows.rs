// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows of the Bluetooth page, made once per device and updated in place from each snapshot,
//! and the placeholders while the radio is off, BlueZ is missing or fails.
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::bluetooth::{Device, Error};
use sway_things_core::i18n::Text;

use super::super::Placeholder;
use super::{Action, BluetoothPage, DeviceRow, update_row, visible_text};
use crate::i18n::{msg, tr, trf};
use crate::style;
use crate::widgets::list::{arrange, labels_in, list_row, remove_row, row, set_hint};

impl BluetoothPage {
    /// Shows the selected adapter, its devices and the rfkill block.
    pub(super) fn render(&self) {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return;
        };
        let selected = imp.selected.borrow().clone();
        let Some(adapter) = selected.as_deref().and_then(|path| snapshot.adapter(path)) else {
            self.show_missing(&Error::Unavailable(Text::data("")));
            return;
        };
        imp.missing.set_visible(false);
        imp.controls.set_visible(true);

        let block = imp.block.get().unwrap_or_default();
        imp.power.set_active(adapter.powered);
        imp.power.set_state(adapter.powered);
        imp.power.set_sensitive(!block.hard);
        imp.power_subtitle.set_text(&if adapter.powered {
            trf(
                msg!("Visible as “{name}” · {address}"),
                &[("name", &adapter.name), ("address", &adapter.address)],
            )
        } else {
            tr(msg!("Off")).to_owned()
        });
        imp.adapter_menu.set_sensitive(adapter.powered);
        imp.adapter_rename.set_sensitive(adapter.powered);
        imp.visible.set_active(adapter.discoverable);
        imp.visible.set_state(adapter.discoverable);
        imp.visible.set_sensitive(adapter.powered);
        imp.visible_subtitle
            .set_text(&visible_text(adapter.discoverable_timeout));
        self.show_off((!adapter.powered).then_some((block.hard, block.soft)));

        // In manual mode the spinner is in the Search button instead.
        let searching = adapter.powered && adapter.discovering && !imp.manual.get();
        imp.search.set_sensitive(adapter.powered);
        let started = searching && !imp.searching.is_visible();
        imp.searching.set_visible(searching);
        if started {
            self.new_spinner();
        }

        let devices: Vec<&Device> = snapshot.devices_of(&adapter.path).collect();
        let running = imp.running.borrow().clone();
        let canceled = imp.canceled.borrow().clone();
        let mut rows = imp.rows.borrow_mut();
        rows.retain(|path, row| {
            let keep = devices.iter().any(|device| device.path == *path);
            if !keep {
                remove_row(&row.row);
            }
            keep
        });
        for device in &devices {
            let row = rows
                .entry(device.path.clone())
                .or_insert_with(|| self.device_row(&device.path));
            update_row(
                row,
                device,
                running.get(&device.path).copied(),
                canceled.contains(&device.path),
                adapter.powered,
            );
        }
        let mine: Vec<&DeviceRow> = devices
            .iter()
            .filter(|device| device.paired)
            .filter_map(|device| rows.get(&device.path))
            .collect();
        let others: Vec<&DeviceRow> = devices
            .iter()
            .filter(|device| !device.paired)
            .filter_map(|device| rows.get(&device.path))
            .collect();
        let rows_of =
            |rows: &[&DeviceRow]| rows.iter().map(|row| row.row.clone()).collect::<Vec<_>>();
        arrange(&imp.mine, &rows_of(&mine));
        arrange(&imp.others, &rows_of(&others));
        let (mine, others) = (mine.len(), others.len());
        drop(rows);

        imp.mine.set_visible(mine > 0);
        set_hint(
            &imp.mine_hint,
            (mine == 0).then_some(tr(msg!(
                "No paired devices yet. Pick one under Other devices to connect it."
            ))),
        );
        imp.others_heading.set_visible(adapter.powered);
        imp.others.set_visible(adapter.powered && others > 0);
        set_hint(
            &imp.others_hint,
            if !adapter.powered {
                None
            } else if others == 0 {
                Some(tr(msg!(
                    "No other devices found yet. Put the device you want to connect into pairing \
                     mode."
                )))
            } else {
                None
            },
        );
    }

    /// Shows the placeholder for a radio that is off (with whether it is hard and soft blocked)
    /// instead of the devices, or the devices for `None`.
    pub(super) fn show_off(&self, off: Option<(bool, bool)>) {
        let imp = self.imp();
        imp.devices.set_visible(off.is_none());
        imp.off.set_visible(off.is_some());
        if imp.off_shown.replace(off) == off {
            return;
        }
        while let Some(child) = imp.off.first_child() {
            imp.off.remove(&child);
        }
        let Some((hard, soft)) = off else {
            return;
        };
        let text = tr(if hard {
            msg!("It's turned off by a hardware switch or key. Use it to turn Bluetooth back on.")
        } else if soft {
            msg!(
                "It's blocked by software, e.g. airplane mode. Turning it on with the switch above \
                 lifts the block."
            )
        } else {
            msg!("Turn it on with the switch above to connect your devices and find new ones.")
        });
        let placeholder = Placeholder::new(
            "bluetooth-disabled-symbolic",
            tr(msg!("Bluetooth is off")),
            text,
        );
        placeholder.set_hexpand(true);
        imp.off.append(&placeholder);
    }

    /// The texts of the placeholder shown while the radio is off; `None` while the devices show.
    pub fn off_placeholder(&self) -> Option<Vec<String>> {
        let off = &self.imp().off;
        off.is_visible().then(|| labels_in(off.upcast_ref()))
    }

    /// Shows a placeholder for `error` (or for no adapter) instead of the controls.
    pub(super) fn show_missing(&self, error: &Error) {
        let imp = self.imp();
        let placeholder = match error {
            Error::Unavailable(reason) if reason.english().is_empty() => Placeholder::new(
                "bluetooth-disabled-symbolic",
                tr(msg!("No Bluetooth adapter")),
                tr(msg!(
                    "Plug in a Bluetooth adapter or turn it on in the firmware settings; this page \
                     updates by itself."
                )),
            ),
            Error::Unavailable(_) => Placeholder::new(
                "bluetooth-disabled-symbolic",
                tr(msg!("Bluetooth isn't available")),
                tr(msg!(
                    "The Bluetooth service isn't running. Start it with `systemctl start \
                     bluetooth`; this page updates by itself."
                )),
            ),
            other => Placeholder::new(
                "dialog-error-symbolic",
                tr(msg!("Bluetooth can't be read")),
                &other.text().translated(),
            ),
        };
        while let Some(child) = imp.missing.first_child() {
            imp.missing.remove(&child);
        }
        placeholder.set_hexpand(true);
        imp.missing.append(&placeholder);
        imp.controls.set_visible(false);
        imp.missing.set_visible(true);
    }

    /// Shows `message` under the adapter's controls, or hides it when empty.
    pub(super) fn show_error(&self, message: &str) {
        let error = &self.imp().error;
        error.set_text(message);
        error.set_visible(!message.is_empty());
    }

    /// A new row for the device at `path`; [`update_row`] fills it.
    pub(super) fn device_row(&self, path: &str) -> DeviceRow {
        // Decorative: the name and state say it all.
        let icon = gtk::Image::builder()
            .pixel_size(24)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();
        let name = gtk::Label::new(None);
        let status = gtk::Label::new(None);
        status.add_css_class(style::DIM);
        let button = gtk::Button::new();
        button.set_valign(gtk::Align::Center);
        let weak = self.downgrade();
        let device = path.to_owned();
        button.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.main_action(&device);
            }
        });

        let menu_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&menu_box));
        for (label, action) in [
            (msg!("Rename…"), Action::Rename),
            (msg!("Forget…"), Action::Forget),
        ] {
            let item = gtk::Button::with_label(tr(label));
            item.add_css_class("flat");
            if let Some(child) = item.child().and_downcast::<gtk::Label>() {
                child.set_xalign(0.0);
            }
            let weak = self.downgrade();
            let device = path.to_owned();
            item.connect_clicked(glib::clone!(
                #[weak]
                popover,
                move |_| {
                    popover.popdown();
                    if let Some(page) = weak.upgrade() {
                        page.run(action, &device);
                    }
                }
            ));
            menu_box.append(&item);
        }
        let menu = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .popover(&popover)
            .valign(gtk::Align::Center)
            .tooltip_text(tr(msg!("More actions")))
            .build();
        menu.add_css_class("flat");

        let line = row(&name, &status, &[button.upcast_ref(), menu.upcast_ref()]);
        line.prepend(&icon);
        DeviceRow {
            row: list_row(&line),
            icon,
            name,
            status,
            button,
            menu,
            paired: false,
        }
    }
}
