// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the rows do: connect, switch on and off, forget, edit and show the details of a network,
//! a cable or a VPN, with the dialogs that ask first.
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::delegate;
use sway_things_core::network::ActiveState;
use sway_things_core::network::Device;
use sway_things_core::network::DeviceState;
use sway_things_core::network::Network;
use sway_things_core::network::NetworkManager;
use sway_things_core::network::band;
use sway_things_core::network::signal_level;

use super::HIDDEN;
use super::NetworkPage;
use super::Target;
use super::dialogs;
use super::dialogs::PasswordSpec;
use super::refresh;
use super::set_running;
use super::text::device_details;
use super::text::failed;
use super::text::hex;
use super::text::ip_details;
use super::text::security_name;
use super::text::signal_quality;
use super::text::vpn_kind;
use crate::i18n::msg;
use crate::i18n::tr;
use crate::i18n::trf;

impl NetworkPage {
    /// What the row with `key` stands for now.
    pub(super) fn target(&self, key: &str) -> Option<Target> {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        if let Some(path) = key.strip_prefix("wired:") {
            return snapshot.device(path).cloned().map(Target::Wired);
        }
        if let Some(path) = key.strip_prefix("vpn:") {
            let vpn = snapshot.connection(path)?.clone();
            let active = snapshot.active_of(path).cloned();
            return Some(Target::Vpn(vpn, active));
        }
        let device = snapshot.device(imp.selected.borrow().as_deref()?)?.clone();
        if let Some(ssid) = key.strip_prefix("net:") {
            return snapshot
                .networks(&device.path)
                .into_iter()
                .find(|n| hex(&n.ssid) == ssid)
                .map(|network| Target::Network(network, device));
        }
        let ssid = key.strip_prefix("away:")?;
        snapshot
            .networks_away(&device.path)
            .into_iter()
            .find(|n| hex(&n.ssid) == ssid)
            .map(Target::Away)
    }

    /// The UUID of the saved connection the active connection at `active` uses.
    pub(super) fn connection_of(&self, active: Option<&str>) -> Option<String> {
        let snapshot = self.imp().snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        let active = snapshot.active_connection(active?)?;
        snapshot
            .connection(&active.connection)
            .map(|c| c.uuid.clone())
    }

    /// The UUID of the first of the saved connections at `paths`.
    pub(super) fn uuid_of(&self, paths: &[String]) -> Option<String> {
        let snapshot = self.imp().snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        paths
            .iter()
            .find_map(|path| snapshot.connection(path))
            .map(|c| c.uuid.clone())
    }

    /// What a network row's button does: connect (asking for a password if needed), disconnect,
    /// or set up a network the page can't connect to by itself.
    pub(super) fn main_action(&self, key: &str) {
        let Some(Target::Network(network, device)) = self.target(key) else {
            return;
        };
        if self.imp().running.borrow().contains_key(key) {
            return;
        }
        if network.active.is_some() {
            let name = network.name.clone();
            self.run(key, tr(msg!("Disconnecting…")), move |nm| async move {
                nm.disconnect_device(&device.path).await.map_err(|error| {
                    failed(
                        msg!("Couldn't disconnect from “{network}”: {error}"),
                        &name,
                        &error,
                    )
                })
            });
        } else if network.saved.is_empty() && !network.security.supported() {
            self.open_editor(delegate::NEW_WIFI);
        } else if network.saved.is_empty() && network.security.needs_password() {
            self.ask_password(key, network, device);
        } else {
            self.connect(key, network, device, None);
        }
    }

    /// Asks for the password of `network` and connects to it.
    pub(super) fn ask_password(&self, key: &str, network: Network, device: Device) {
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let id = key.to_owned();
        let spec = PasswordSpec {
            title: trf(
                msg!("Connect to “{network}”"),
                &[("network", &network.name)],
            ),
            body: tr(msg!("Enter the password of the Wi-Fi network.")),
            security: network.security,
            saved_note: true,
        };
        let dialog = dialogs::password(window.as_ref(), &spec, move |password| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            page.imp().dialog.replace(None);
            if let Some(password) = password {
                page.connect(&id, network, device, Some(password));
            }
        });
        self.imp().dialog.replace(Some(dialog.window().clone()));
    }

    /// Connects `device` to `network`, with `password` for a new one, and waits until it is
    /// connected or fails.
    pub(super) fn connect(
        &self,
        key: &str,
        network: Network,
        device: Device,
        password: Option<String>,
    ) {
        let name = network.name.clone();
        self.run_connection(key, move |nm| async move {
            let active = nm
                .connect_network(&device, &network, password.as_deref())
                .await;
            let result = match active {
                Ok(active) => nm.wait_connected(&active, Some(&device.path)).await,
                Err(error) => Err(error),
            };
            result.map_err(|error| {
                failed(
                    msg!("Couldn't connect to “{network}”: {error}"),
                    &name,
                    &error,
                )
            })
        });
    }

    /// Asks for a hidden network and connects to it.
    pub(super) fn ask_hidden(&self) {
        let Some(device) = self.imp().selected.borrow().clone() else {
            return;
        };
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let dialog = dialogs::hidden(window.as_ref(), move |hidden| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            page.imp().dialog.replace(None);
            let Some((ssid, security, password)) = hidden else {
                return;
            };
            page.run_connection(HIDDEN, move |nm| async move {
                let active = nm
                    .connect_hidden(&device, &ssid, security, password.as_deref())
                    .await;
                let result = match active {
                    Ok(active) => nm.wait_connected(&active, Some(&device)).await,
                    Err(error) => Err(error),
                };
                result.map_err(|error| {
                    failed(
                        msg!("Couldn't connect to “{network}”: {error}"),
                        &ssid,
                        &error,
                    )
                })
            });
        });
        self.imp().dialog.replace(Some(dialog.window().clone()));
    }

    /// The handler of a wired or VPN row's switch.
    pub(super) fn switched(&self, key: &str, on: bool) -> glib::Propagation {
        if self.imp().running.borrow().contains_key(key) {
            return glib::Propagation::Stop;
        }
        match self.target(key) {
            Some(Target::Wired(device)) => {
                let is_on = matches!(
                    device.state,
                    DeviceState::Connected
                        | DeviceState::Connecting
                        | DeviceState::NeedAuth
                        | DeviceState::Failed
                );
                if is_on == on {
                    return glib::Propagation::Proceed;
                }
                if on {
                    self.run_connection(key, move |nm| async move {
                        let result = match nm.connect_device(&device.path).await {
                            Ok(active) => nm.wait_connected(&active, Some(&device.path)).await,
                            Err(error) => Err(error),
                        };
                        result.map_err(|error| {
                            failed(msg!("Couldn't connect the cable: {error}"), "", &error)
                        })
                    });
                } else {
                    self.run(key, tr(msg!("Disconnecting…")), move |nm| async move {
                        nm.disconnect_device(&device.path).await.map_err(|error| {
                            failed(msg!("Couldn't disconnect the cable: {error}"), "", &error)
                        })
                    });
                }
            }
            Some(Target::Vpn(vpn, active)) => {
                let is_on = active
                    .as_ref()
                    .is_some_and(|a| a.state != ActiveState::Deactivated);
                if is_on == on {
                    return glib::Propagation::Proceed;
                }
                let name = vpn.name.clone();
                if on {
                    self.run_connection(key, move |nm| async move {
                        let result = match nm.activate(&vpn.path, None).await {
                            Ok(active) => nm.wait_connected(&active, None).await,
                            Err(error) => Err(error),
                        };
                        result.map_err(|error| {
                            failed(msg!("Couldn't connect “{vpn}”: {error}"), &name, &error)
                        })
                    });
                } else if let Some(active) = active {
                    self.run(key, tr(msg!("Disconnecting…")), move |nm| async move {
                        nm.deactivate(&active.path).await.map_err(|error| {
                            failed(msg!("Couldn't disconnect “{vpn}”: {error}"), &name, &error)
                        })
                    });
                }
            }
            _ => return glib::Propagation::Proceed,
        }
        // The switch shows the new position; its state follows once NetworkManager reports it.
        glib::Propagation::Stop
    }

    /// Forgets the saved network of the row with `key`, asking first.
    pub(super) fn forget(&self, key: &str) {
        let (name, saved) = match self.target(key) {
            Some(Target::Network(network, _)) => (network.name, network.saved),
            Some(Target::Away(network)) => (network.name, network.saved),
            _ => return,
        };
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let id = key.to_owned();
        glib::spawn_future_local(async move {
            if !dialogs::confirm_forget(&name, window.as_ref()).await {
                return;
            }
            if let Some(page) = weak.upgrade() {
                page.run(&id, tr(msg!("Forgetting…")), move |nm| async move {
                    nm.forget(&saved).await.map_err(|error| {
                        failed(msg!("Couldn't forget “{network}”: {error}"), &name, &error)
                    })
                });
            }
        });
    }

    /// Opens the saved connection of the row with `key` in nm-connection-editor.
    pub(super) fn edit(&self, key: &str) {
        let uuid = match self.target(key) {
            Some(Target::Network(network, _)) => self.uuid_of(&network.saved),
            Some(Target::Away(network)) => self.uuid_of(&network.saved),
            Some(Target::Wired(device)) => self.connection_of(device.active_connection.as_deref()),
            Some(Target::Vpn(vpn, _)) => Some(vpn.uuid),
            None => None,
        };
        match uuid.as_deref().and_then(delegate::edit_connection) {
            Some([option, uuid]) => self.open_editor(&[option.as_str(), uuid.as_str()]),
            None => self.show_error(tr(msg!(
                "This connection can't be opened in nm-connection-editor"
            ))),
        }
    }

    /// Shows the details of the connection of the row with `key`.
    pub(super) fn show_details(&self, key: &str) {
        let (title, rows) = match self.target(key) {
            Some(Target::Network(network, device)) => {
                let ap = self.imp().snapshot.borrow().as_ref().and_then(|s| {
                    s.access_points
                        .iter()
                        .find(|ap| ap.path == network.access_point)
                        .cloned()
                });
                let mut rows = vec![(
                    tr(msg!("Signal")),
                    trf(
                        msg!("{quality} ({percent} %)"),
                        &[
                            ("quality", &signal_quality(signal_level(network.strength))),
                            ("percent", &network.strength),
                        ],
                    ),
                )];
                if let Some(band) = ap.as_ref().and_then(|ap| band(ap.frequency)) {
                    rows.push((tr(msg!("Frequency")), band.to_owned()));
                }
                rows.push((
                    tr(msg!("Security")),
                    security_name(network.security).to_owned(),
                ));
                rows.extend(device_details(&device));
                (format!("“{}”", network.name), rows)
            }
            Some(Target::Wired(device)) => (tr(msg!("Wired")).to_owned(), device_details(&device)),
            Some(Target::Vpn(vpn, active)) => {
                let mut rows = vec![(tr(msg!("Type")), vpn_kind(&vpn.kind).to_owned())];
                if let Some(active) = active {
                    rows.extend(ip_details(active.ip4.as_ref(), active.ip6.as_ref()));
                }
                (vpn.name, rows)
            }
            _ => return,
        };
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let dialog = dialogs::details(window.as_ref(), &title, &rows);
        dialog.window().connect_destroy(move |_| {
            if let Some(page) = weak.upgrade() {
                page.imp().dialog.replace(None);
            }
        });
        self.imp().dialog.replace(Some(dialog.window().clone()));
    }

    /// Runs `action` for the row with `key`, which shows `label` meanwhile; its error shows at
    /// the top.
    pub(super) fn run<F, Fut>(&self, key: &str, label: &'static str, action: F)
    where
        F: FnOnce(NetworkManager) -> Fut + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + 'static,
    {
        let Some(nm) = self.imp().nm.borrow().clone() else {
            return;
        };
        let weak = self.downgrade();
        let key = key.to_owned();
        set_running(&weak, &key, Some(label));
        glib::spawn_future_local(async move {
            let result = action(nm.clone()).await;
            set_running(&weak, &key, None);
            if let Some(page) = weak.upgrade() {
                page.show_error(&result.err().unwrap_or_default());
            }
            refresh(&weak, &nm).await;
        });
    }

    /// Runs `action`, a connection the page starts, for the row with `key`: our agent stays
    /// registered until it has finished, so it can ask for a password again.
    pub(super) fn run_connection<F, Fut>(&self, key: &str, action: F)
    where
        F: FnOnce(NetworkManager) -> Fut + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + 'static,
    {
        let imp = self.imp();
        imp.pending.set(imp.pending.get() + 1);
        self.update_agent();
        let weak = self.downgrade();
        self.run(key, tr(msg!("Connecting…")), move |nm| async move {
            let result = action(nm).await;
            if let Some(page) = weak.upgrade() {
                let pending = &page.imp().pending;
                pending.set(pending.get().saturating_sub(1));
                page.update_agent();
            }
            result
        });
    }

    /// The handler of the Wi-Fi switch.
    pub(super) fn wifi_changed(&self, on: bool) -> glib::Propagation {
        let imp = self.imp();
        let enabled = imp.snapshot.borrow().as_ref().map(|s| s.wifi_enabled);
        let (Some(nm), Some(enabled)) = (imp.nm.borrow().clone(), enabled) else {
            return glib::Propagation::Proceed;
        };
        if enabled == on {
            return glib::Propagation::Proceed;
        }
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let result = nm.set_wifi_enabled(on).await;
            if let Some(page) = weak.upgrade() {
                page.show_error(&match result {
                    Ok(()) => String::new(),
                    Err(error) => trf(
                        if on {
                            msg!("Couldn't turn Wi-Fi on: {error}")
                        } else {
                            msg!("Couldn't turn Wi-Fi off: {error}")
                        },
                        &[("error", &error.text().translated())],
                    ),
                });
            }
            refresh(&weak, &nm).await;
        });
        glib::Propagation::Stop
    }

    /// Turns networking back on.
    pub(super) fn turn_on_networking(&self) {
        let Some(nm) = self.imp().nm.borrow().clone() else {
            return;
        };
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let result = nm.set_networking_enabled(true).await;
            if let Some(page) = weak.upgrade() {
                page.show_error(&match result {
                    Ok(()) => String::new(),
                    Err(error) => trf(
                        msg!("Couldn't turn networking on: {error}"),
                        &[("error", &error.text().translated())],
                    ),
                });
            }
            refresh(&weak, &nm).await;
        });
    }
}
