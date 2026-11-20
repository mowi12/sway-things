// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Looking for devices: discovery while the page is on screen and the radio is on, or only for
//! thirty seconds after _Search_, and the hand-over to blueman-manager.
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::delegate;

use super::super::search_button;
use super::BluetoothPage;
use super::SEARCH_FOR;
use crate::i18n::msg;
use crate::i18n::tr;
use crate::i18n::trf;

impl BluetoothPage {
    /// Whether a turning spinner shows, next to the heading or in the Search button; for
    /// tests.
    pub fn is_searching(&self) -> bool {
        let imp = self.imp();
        let heading = imp.searching.is_visible()
            && imp
                .spinner
                .borrow()
                .as_ref()
                .is_some_and(gtk::Spinner::is_spinning);
        heading || search_button::is_spinning(&imp.search)
    }

    /// Whether the page looks for devices only after _Search_ (`true`) or also the whole time
    /// it is on screen and the radio is on (`false`, the default).
    pub fn set_manual_search(&self, manual: bool) {
        let imp = self.imp();
        imp.manual.set(manual);
        imp.search.set_visible(manual);
        if !manual {
            self.end_search();
        }
        self.render();
        self.update_discovery();
    }

    /// The _Search_ button next to "Other devices"; for tests.
    pub fn search_button(&self) -> &gtk::Button {
        &self.imp().search
    }

    /// What the _Search_ button reads: `Search` or `Stop`; for tests.
    pub fn search_text(&self) -> String {
        search_button::text(&self.imp().search)
    }

    /// _Search_ (manual mode only) starts a discovery for [`SEARCH_FOR`]; _Stop_ ends it.
    pub(super) fn search_clicked(&self) {
        let imp = self.imp();
        if imp.requested.borrow().is_some() {
            self.end_search();
            self.update_discovery();
            return;
        }
        let Some(path) = self.powered_adapter() else {
            return;
        };
        let weak = self.downgrade();
        let timer = glib::timeout_add_local_once(SEARCH_FOR, move || {
            if let Some(page) = weak.upgrade() {
                // The timer is spent, so its id is only dropped, not removed.
                page.imp().requested.replace(None);
                page.update_search_button();
                page.update_discovery();
            }
        });
        imp.requested.replace(Some((path, timer)));
        self.update_search_button();
        self.update_discovery();
    }

    /// Ends the search started with _Search_, if one runs.
    pub(super) fn end_search(&self) {
        if let Some((_, timer)) = self.imp().requested.take() {
            timer.remove();
        }
        self.update_search_button();
    }

    /// Shows _Stop_ while a search started with _Search_ runs, _Search_ otherwise.
    pub(super) fn update_search_button(&self) {
        let imp = self.imp();
        let running = imp.requested.borrow().is_some();
        search_button::set_running(
            &imp.search,
            running,
            tr(if running {
                msg!("Stop looking for devices")
            } else {
                msg!("Look for devices nearby for 30 seconds")
            }),
        );
    }

    /// The path of the selected adapter if its radio is on.
    pub(super) fn powered_adapter(&self) -> Option<String> {
        let imp = self.imp();
        imp.selected.borrow().clone().filter(|path| {
            imp.snapshot
                .borrow()
                .as_ref()
                .and_then(|snapshot| snapshot.adapter(path))
                .is_some_and(|adapter| adapter.powered)
        })
    }

    /// The adapter the page should look for devices on now: the selected one while the page is
    /// on screen and its radio is on, after _Search_ or always in automatic mode. Ends a search
    /// started with _Search_ that no longer applies.
    pub(super) fn wanted_discovery(&self) -> Option<String> {
        let imp = self.imp();
        let adapter = self.powered_adapter().filter(|_| self.is_mapped());
        let requested = imp
            .requested
            .borrow()
            .as_ref()
            .map(|(path, _)| path.clone());
        if requested.is_some() && requested != adapter {
            self.end_search();
        }
        let requested = imp.requested.borrow().is_some();
        adapter.filter(|_| requested || !imp.manual.get())
    }

    /// Starts or stops looking for devices as [`wanted_discovery`](Self::wanted_discovery) says.
    pub(super) fn update_discovery(&self) {
        let imp = self.imp();
        let wanted = self.wanted_discovery();
        if wanted == *imp.discovery.borrow() {
            return;
        }
        self.stop_discovery();
        let (Some(path), Some(bluez)) = (wanted, imp.bluez.borrow().clone()) else {
            return;
        };
        imp.discovery.replace(Some(path.clone()));
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            if let Err(error) = bluez.start_discovery(&path).await {
                if let Some(page) = weak.upgrade() {
                    page.show_error(&trf(
                        msg!("Couldn't look for devices: {error}"),
                        &[("error", &error.text().translated())],
                    ));
                }
            }
        });
    }

    /// Stops the discovery the page started, so the radio doesn't keep scanning once the page is
    /// gone or hidden. BlueZ also stops it when the connection closes.
    pub(super) fn stop_discovery(&self) {
        let imp = self.imp();
        let Some(path) = imp.discovery.take() else {
            return;
        };
        if let Some(bluez) = imp.bluez.borrow().clone() {
            glib::spawn_future_local(async move {
                // The radio may be off or BlueZ gone; then there's nothing to stop.
                let _ = bluez.stop_discovery(&path).await;
            });
        }
    }

    /// Starts blueman-manager.
    pub(super) fn open_blueman(&self) {
        match delegate::BLUEMAN.launch() {
            Ok(mut child) => {
                // Waiting reaps it once it ends, so it doesn't stay behind as a zombie.
                glib::spawn_future_local(async move {
                    let _ = child.status().await;
                });
            }
            Err(error) => self.show_error(&trf(
                msg!("Blueman can't be started: {error}"),
                &[("error", &error)],
            )),
        }
    }
}
