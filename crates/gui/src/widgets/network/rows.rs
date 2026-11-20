// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows of the Network page: the known and other Wi-Fi networks, the cables and the VPNs, made
//! once per network and updated in place from each snapshot.
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::network::ActiveConnection;
use sway_things_core::network::ActiveState;
use sway_things_core::network::Connection;
use sway_things_core::network::Connectivity;
use sway_things_core::network::Device;
use sway_things_core::network::DeviceState;
use sway_things_core::network::Network;
use sway_things_core::network::SavedNetwork;
use sway_things_core::network::Snapshot;
use sway_things_core::network::signal_level;

use super::NetRow;
use super::NetworkPage;
use super::text::first_address;
use super::text::hex;
use super::text::security_text;
use super::text::signal_icon;
use super::text::signal_label;
use super::text::vpn_kind;
use super::update_menu;
use crate::i18n::msg;
use crate::i18n::tr;
use crate::i18n::trf;
use crate::style;
use crate::widgets::list::arrange;
use crate::widgets::list::list_row;
use crate::widgets::list::remove_row;
use crate::widgets::list::row;
use crate::widgets::list::set_hint;

impl NetworkPage {
    /// Shows the snapshot: the sections and their rows.
    pub(super) fn render(&self) {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return;
        };
        if !snapshot.networking_enabled {
            self.show_missing(
                "network-offline-symbolic",
                tr(msg!("Networking is turned off")),
                tr(msg!(
                    "NetworkManager has networking turned off, for Wi-Fi, cables and VPNs alike."
                )),
                true,
            );
            return;
        }
        let wifi_devices: Vec<&Device> = snapshot.wifi_devices().collect();
        let wired: Vec<&Device> = snapshot.ethernet_devices().collect();
        let vpns: Vec<&Connection> = snapshot.vpns().collect();
        if wifi_devices.is_empty() && wired.is_empty() && vpns.is_empty() {
            self.show_missing(
                "network-wired-disconnected-symbolic",
                tr(msg!("No network adapter")),
                tr(msg!(
                    "Plug in a network adapter; this page updates by itself."
                )),
                false,
            );
            return;
        }
        imp.missing.set_visible(false);
        imp.controls.set_visible(true);

        imp.portal
            .set_visible(snapshot.connectivity == Connectivity::Portal);
        imp.portal_open
            .set_visible(snapshot.connectivity_uri.is_some());

        let running = imp.running.borrow().clone();
        let mut keys: Vec<String> = Vec::new();
        let device = imp
            .selected
            .borrow()
            .as_deref()
            .and_then(|path| snapshot.device(path))
            .cloned();
        let (known, others) = self.render_wifi(snapshot, device.as_ref(), &running, &mut keys);
        let wired_rows = self.render_wired(&wired, &running, &mut keys);
        let vpn_rows = self.render_vpns(snapshot, &vpns, &running, &mut keys);

        let mut rows = imp.rows.borrow_mut();
        rows.retain(|key, row| {
            let keep = keys.contains(key);
            if !keep {
                remove_row(&row.row);
            }
            keep
        });
        let pick = |keys: &[String]| -> Vec<gtk::ListBoxRow> {
            keys.iter()
                .filter_map(|key| rows.get(key).map(|row| row.row.clone()))
                .collect()
        };
        let mut other_rows = pick(&others);
        other_rows.push(imp.hidden_row.clone());
        arrange(&imp.known, &pick(&known));
        arrange(&imp.others, &other_rows);
        arrange(&imp.wired, &pick(&wired_rows));
        arrange(&imp.vpns, &pick(&vpn_rows));
    }

    /// Shows the Wi-Fi section and the networks of `device`; the keys of the known and the other
    /// networks' rows, in order. Adds every key it shows to `keys`.
    pub(super) fn render_wifi(
        &self,
        snapshot: &Snapshot,
        device: Option<&Device>,
        running: &std::collections::HashMap<String, &'static str>,
        keys: &mut Vec<String>,
    ) -> (Vec<String>, Vec<String>) {
        let imp = self.imp();
        imp.wifi.set_visible(device.is_some());
        imp.wifi_hint.set_visible(device.is_none());
        let on = snapshot.wifi_enabled && snapshot.wifi_hardware_enabled && device.is_some();
        let networks = device
            .map(|d| snapshot.networks(&d.path))
            .unwrap_or_default();

        imp.power.set_active(snapshot.wifi_enabled);
        imp.power.set_state(snapshot.wifi_enabled);
        imp.power.set_sensitive(snapshot.wifi_hardware_enabled);
        let current = networks.iter().find(|n| n.active.is_some());
        imp.power_subtitle.set_text(
            &if !snapshot.wifi_enabled || !snapshot.wifi_hardware_enabled {
                tr(msg!("Off")).to_owned()
            } else {
                match current {
                    Some(n)
                        if n.active
                            .as_ref()
                            .is_some_and(|a| a.state == ActiveState::Activated) =>
                    {
                        trf(msg!("Connected to “{network}”"), &[("network", &n.name)])
                    }
                    Some(n) => trf(msg!("Connecting to “{network}”…"), &[("network", &n.name)]),
                    None => tr(msg!("Not connected")).to_owned(),
                }
            },
        );

        imp.search.set_sensitive(on);
        // The regular scans of automatic mode; in manual mode the spinner is in the Search button.
        let searching = on && imp.scan.borrow().is_some();
        let started = searching && !imp.searching.is_visible();
        imp.searching.set_visible(searching);
        if started {
            self.new_spinner();
        }

        let (mut known, mut others) = (Vec::new(), Vec::new());
        if let (true, Some(device)) = (on, device) {
            for network in &networks {
                let key = format!("net:{}", hex(&network.ssid));
                let row = self.ensure_row(&key, false);
                self.update_network_row(&row, network, running.get(&key).copied());
                if network.saved.is_empty() && network.active.is_none() {
                    others.push(key.clone());
                } else {
                    known.push(key.clone());
                }
                keys.push(key);
            }
            for network in snapshot.networks_away(&device.path) {
                let key = format!("away:{}", hex(&network.ssid));
                let row = self.ensure_row(&key, false);
                self.update_away_row(&row, &network, running.get(&key).copied());
                known.push(key.clone());
                keys.push(key);
            }
        }
        let wifi = device.is_some();
        self.show_off((wifi && !on).then_some(!snapshot.wifi_hardware_enabled));
        imp.known_heading.set_visible(on);
        imp.known.set_visible(on && !known.is_empty());
        set_hint(
            &imp.known_hint,
            if !on {
                None
            } else if known.is_empty() {
                Some(tr(msg!(
                    "No saved networks yet. Pick one under Other networks to connect to it."
                )))
            } else {
                None
            },
        );
        imp.others_heading.set_visible(on);
        imp.others.set_visible(on);
        set_hint(
            &imp.others_hint,
            (on && others.is_empty()).then_some(tr(msg!("No other networks found yet."))),
        );
        (known, others)
    }

    /// Updates the rows of the wired `devices`; their keys. Adds them to `keys`.
    pub(super) fn render_wired(
        &self,
        devices: &[&Device],
        running: &std::collections::HashMap<String, &'static str>,
        keys: &mut Vec<String>,
    ) -> Vec<String> {
        let imp = self.imp();
        imp.wired_heading.set_visible(!devices.is_empty());
        imp.wired.set_visible(!devices.is_empty());
        let mut shown = Vec::new();
        for device in devices {
            let key = format!("wired:{}", device.path);
            let row = self.ensure_row(&key, true);
            let title = if devices.len() > 1 {
                trf(
                    msg!("Wired ({interface})"),
                    &[("interface", &device.interface)],
                )
            } else {
                tr(msg!("Wired")).to_owned()
            };
            self.update_wired_row(&row, device, &title, running.get(&key).copied());
            shown.push(key.clone());
            keys.push(key);
        }
        shown
    }

    /// Updates the rows of the `vpns`; their keys. Adds them to `keys`.
    pub(super) fn render_vpns(
        &self,
        snapshot: &Snapshot,
        vpns: &[&Connection],
        running: &std::collections::HashMap<String, &'static str>,
        keys: &mut Vec<String>,
    ) -> Vec<String> {
        let imp = self.imp();
        imp.vpn_heading.set_visible(!vpns.is_empty());
        imp.vpns.set_visible(!vpns.is_empty());
        let mut shown = Vec::new();
        for vpn in vpns {
            let key = format!("vpn:{}", vpn.path);
            let row = self.ensure_row(&key, true);
            self.update_vpn_row(
                &row,
                vpn,
                snapshot.active_of(&vpn.path),
                running.get(&key).copied(),
            );
            shown.push(key.clone());
            keys.push(key);
        }
        shown
    }

    /// The row with `key`, made if it is new: with a switch (`switched`) or a button.
    pub(super) fn ensure_row(&self, key: &str, switched: bool) -> NetRow {
        if let Some(row) = self.imp().rows.borrow().get(key) {
            return row.clone();
        }
        let row = self.new_row(key, switched);
        self.imp()
            .rows
            .borrow_mut()
            .insert(key.to_owned(), row.clone());
        row
    }

    /// A new row for `key`; the `update_*_row` functions fill it.
    pub(super) fn new_row(&self, key: &str, switched: bool) -> NetRow {
        let icon = gtk::Image::builder().pixel_size(16).build();
        let name = gtk::Label::new(None);
        let status = gtk::Label::new(None);
        status.add_css_class(style::DIM);
        let button = gtk::Button::new();
        button.set_valign(gtk::Align::Center);
        button.set_visible(!switched);
        let weak = self.downgrade();
        let id = key.to_owned();
        button.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.main_action(&id);
            }
        });
        let switch = gtk::Switch::new();
        switch.set_valign(gtk::Align::Center);
        switch.set_visible(switched);
        let weak = self.downgrade();
        let id = key.to_owned();
        switch.connect_state_set(move |_, on| {
            weak.upgrade()
                .map_or(glib::Propagation::Proceed, |page| page.switched(&id, on))
        });

        let menu_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&menu_box));
        let item = |label: &str, action: fn(&Self, &str)| {
            let item = gtk::Button::with_label(label);
            item.add_css_class("flat");
            if let Some(child) = item.child().and_downcast::<gtk::Label>() {
                child.set_xalign(0.0);
            }
            let weak = self.downgrade();
            let id = key.to_owned();
            item.connect_clicked(glib::clone!(
                #[weak]
                popover,
                move |_| {
                    popover.popdown();
                    if let Some(page) = weak.upgrade() {
                        action(&page, &id);
                    }
                }
            ));
            menu_box.append(&item);
            item
        };
        let details = item(tr(msg!("Details…")), Self::show_details);
        let edit = item(tr(msg!("Edit…")), Self::edit);
        let forget = item(tr(msg!("Forget…")), Self::forget);
        let menu = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .popover(&popover)
            .valign(gtk::Align::Center)
            .tooltip_text(tr(msg!("More actions")))
            .build();
        menu.add_css_class("flat");

        let line = row(
            &name,
            &status,
            &[button.upcast_ref(), switch.upcast_ref(), menu.upcast_ref()],
        );
        line.prepend(&icon);
        NetRow {
            row: list_row(&line),
            icon,
            name,
            status,
            button,
            switch,
            menu,
            details,
            edit,
            forget,
        }
    }

    /// Fills `row` from the Wi-Fi `network`; `running` is what runs on it.
    pub(super) fn update_network_row(
        &self,
        row: &NetRow,
        network: &Network,
        running: Option<&'static str>,
    ) {
        let editor = self.imp().editor.get();
        let level = signal_level(network.strength);
        row.icon.set_icon_name(Some(signal_icon(level)));
        row.icon.set_visible(true);
        row.icon
            .update_property(&[gtk::accessible::Property::Label(signal_label(level))]);
        // Untrusted: plain text, never markup.
        row.name.set_text(&network.name);
        let active = network.active.as_ref().map(|a| a.state);
        let state = running.or(match active {
            Some(ActiveState::Activated) => Some(tr(msg!("Connected"))),
            Some(ActiveState::Activating | ActiveState::Unknown) => Some(tr(msg!("Connecting…"))),
            Some(ActiveState::Deactivating) => Some(tr(msg!("Disconnecting…"))),
            _ if !network.saved.is_empty() => Some(tr(msg!("Saved"))),
            _ => None,
        });
        let mut status: Vec<&str> = state.into_iter().collect();
        status.push(security_text(network.security));
        let set_up = network.saved.is_empty() && !network.security.supported();
        if set_up && !editor {
            status.push(tr(msg!("needs nm-connection-editor")));
        }
        row.status.set_text(&status.join(" \u{b7} "));
        let label = match running {
            Some(running) => running,
            None if active.is_some_and(|a| a != ActiveState::Deactivated) => tr(msg!("Disconnect")),
            None if set_up => tr(msg!("Set Up…")),
            None => tr(msg!("Connect")),
        };
        row.button.set_label(label);
        row.button.set_visible(!set_up || editor);
        row.button.set_sensitive(running.is_none());
        row.button
            .update_property(&[gtk::accessible::Property::Label(&trf(
                // TRANSLATORS: The button of a network for screen readers: "Connect Home".
                msg!("{action} {network}"),
                &[("action", &label), ("network", &network.name)],
            ))]);
        let saved = !network.saved.is_empty();
        update_menu(
            row,
            &network.name,
            [
                active == Some(ActiveState::Activated),
                saved && editor,
                saved,
            ],
            running.is_none(),
        );
    }

    /// Fills `row` from the saved network that isn't around.
    pub(super) fn update_away_row(
        &self,
        row: &NetRow,
        network: &SavedNetwork,
        running: Option<&'static str>,
    ) {
        // Keeps the names in line with the other rows'.
        row.icon.set_icon_name(None);
        row.icon.set_visible(true);
        row.name.set_text(&network.name);
        row.status
            .set_text(running.unwrap_or(tr(msg!("Not in range"))));
        row.button.set_visible(false);
        update_menu(
            row,
            &network.name,
            [false, self.imp().editor.get(), true],
            running.is_none(),
        );
    }

    /// Fills `row` from the wired `device`, titled `title`.
    pub(super) fn update_wired_row(
        &self,
        row: &NetRow,
        device: &Device,
        title: &str,
        running: Option<&'static str>,
    ) {
        row.icon.set_visible(false);
        row.name.set_text(title);
        let state = running.unwrap_or(tr(match device.state {
            DeviceState::Connected => msg!("Connected"),
            DeviceState::Connecting | DeviceState::NeedAuth => msg!("Connecting…"),
            DeviceState::Disconnecting => msg!("Disconnecting…"),
            DeviceState::Unavailable => msg!("Cable unplugged"),
            DeviceState::Failed => msg!("Couldn't connect"),
            _ => msg!("Off"),
        }));
        let mut status = vec![state.to_owned()];
        if running.is_none() && device.state == DeviceState::Connected {
            if let Some(speed) = device.speed {
                status.push(trf(msg!("{speed} Mb/s"), &[("speed", &speed)]));
            }
            if let Some(address) = first_address(device.ip4.as_ref()) {
                status.push(address);
            }
        }
        row.status.set_text(&status.join(" \u{b7} "));
        let on = matches!(
            device.state,
            DeviceState::Connected
                | DeviceState::Connecting
                | DeviceState::NeedAuth
                | DeviceState::Failed
        );
        row.switch.set_active(on);
        row.switch.set_state(on);
        // Without a cable there is nothing to switch: the row only says so.
        row.switch
            .set_visible(device.state != DeviceState::Unavailable);
        row.switch.set_sensitive(running.is_none());
        row.switch
            .update_property(&[gtk::accessible::Property::Label(title)]);
        let uuid = self.connection_of(device.active_connection.as_deref());
        update_menu(
            row,
            title,
            [
                device.state == DeviceState::Connected,
                uuid.is_some() && self.imp().editor.get(),
                false,
            ],
            running.is_none(),
        );
    }

    /// Fills `row` from the VPN `vpn`, `active` while it is on.
    pub(super) fn update_vpn_row(
        &self,
        row: &NetRow,
        vpn: &Connection,
        active: Option<&ActiveConnection>,
        running: Option<&'static str>,
    ) {
        row.icon.set_visible(false);
        row.name.set_text(&vpn.name);
        let state = running.unwrap_or(tr(match active.map(|a| a.state) {
            Some(ActiveState::Activated) => msg!("Connected"),
            Some(ActiveState::Activating | ActiveState::Unknown) => msg!("Connecting…"),
            Some(ActiveState::Deactivating) => msg!("Disconnecting…"),
            _ => msg!("Off"),
        }));
        let mut status = vec![vpn_kind(&vpn.kind).to_owned(), state.to_owned()];
        if running.is_none() {
            if let Some(address) = first_address(active.and_then(|a| a.ip4.as_ref())) {
                status.push(address);
            }
        }
        row.status.set_text(&status.join(" \u{b7} "));
        let on = active.is_some_and(|a| a.state != ActiveState::Deactivated);
        row.switch.set_active(on);
        row.switch.set_state(on);
        row.switch.set_sensitive(running.is_none());
        row.switch
            .update_property(&[gtk::accessible::Property::Label(&vpn.name)]);
        update_menu(
            row,
            &vpn.name,
            [
                active.is_some_and(|a| a.state == ActiveState::Activated),
                self.imp().editor.get(),
                false,
            ],
            running.is_none(),
        );
    }
}
