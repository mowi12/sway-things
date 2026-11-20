// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Looking for Wi-Fi networks: the scans while the page is on screen, or only on request with the
//! _Search_ button.
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use super::super::search_button;
use super::NetworkPage;
use super::SCAN_EVERY;
use super::SCAN_POLL;
use super::SCAN_TIMEOUT;
use crate::i18n::msg;
use crate::i18n::tr;

impl NetworkPage {
    /// Whether the page asks for scans only after _Search_ (`true`) or also every
    /// [`SCAN_EVERY`] while it is on screen and Wi-Fi is on (`false`, the default).
    pub fn set_manual_search(&self, manual: bool) {
        let imp = self.imp();
        imp.manual.set(manual);
        imp.search.set_visible(manual);
        if !manual {
            self.end_search();
        }
        self.render();
        self.update_scan();
    }

    /// The _Search_ button next to "Other networks"; for tests.
    pub fn search_button(&self) -> &gtk::Button {
        &self.imp().search
    }

    /// What the _Search_ button reads: `Search` or `Stop`; for tests.
    pub fn search_text(&self) -> String {
        search_button::text(&self.imp().search)
    }

    /// _Search_ (manual mode only) asks for one scan and waits for the new list; _Stop_ stops waiting.
    pub(super) fn search_clicked(&self) {
        let imp = self.imp();
        if imp.requested.borrow().is_some() {
            self.end_search();
            return;
        }
        let (Some(nm), Some(device), true) = (
            imp.nm.borrow().clone(),
            imp.selected.borrow().clone(),
            self.wifi_on(),
        ) else {
            return;
        };
        let weak = self.downgrade();
        let task = glib::spawn_future_local(async move {
            let before = nm.last_scan(&device).await.ok();
            // A failed scan only means an older list, as for the scans in automatic mode.
            if nm.request_scan(&device).await.is_ok() {
                let mut waited = Duration::ZERO;
                while waited < SCAN_TIMEOUT {
                    glib::timeout_future(SCAN_POLL).await;
                    waited += SCAN_POLL;
                    if nm.last_scan(&device).await.ok() != before {
                        break;
                    }
                }
            }
            if let Some(page) = weak.upgrade() {
                page.imp().requested.replace(None);
                page.update_search_button();
                page.render();
            }
        });
        imp.requested.replace(Some(task));
        self.update_search_button();
        self.render();
    }

    /// Stops waiting for the scan asked for with _Search_, if one runs.
    pub(super) fn end_search(&self) {
        let Some(task) = self.imp().requested.take() else {
            return;
        };
        task.abort();
        self.update_search_button();
        self.render();
    }

    /// Shows _Stop_ while a scan asked for with _Search_ runs, _Search_ otherwise.
    pub(super) fn update_search_button(&self) {
        let imp = self.imp();
        let running = imp.requested.borrow().is_some();
        search_button::set_running(
            &imp.search,
            running,
            tr(if running {
                msg!("Stop waiting for the scan")
            } else {
                msg!("Look for networks nearby now")
            }),
        );
    }

    /// Whether Wi-Fi is on and the page has a Wi-Fi device.
    pub(super) fn wifi_on(&self) -> bool {
        let imp = self.imp();
        imp.selected.borrow().is_some()
            && imp
                .snapshot
                .borrow()
                .as_ref()
                .is_some_and(|s| s.wifi_enabled && s.networking_enabled)
    }

    /// Asks for scans while the page is on screen and Wi-Fi is on, in automatic mode, and stops
    /// otherwise. Ends a search asked for with _Search_ once the page is off screen or Wi-Fi off.
    pub(super) fn update_scan(&self) {
        let imp = self.imp();
        let shown = self.is_mapped() && self.wifi_on();
        if !shown {
            self.end_search();
        }
        let wanted = shown && !imp.manual.get();
        if wanted == imp.scan.borrow().is_some() {
            return;
        }
        if let Some(scan) = imp.scan.take() {
            scan.remove();
            self.render();
            return;
        }
        self.request_scan();
        let weak = self.downgrade();
        let source = glib::timeout_add_seconds_local(SCAN_EVERY, move || {
            if let Some(page) = weak.upgrade() {
                page.request_scan();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        imp.scan.replace(Some(source));
        self.render();
    }

    /// Asks the selected Wi-Fi device for a scan.
    pub(super) fn request_scan(&self) {
        let imp = self.imp();
        let (Some(nm), Some(device)) = (imp.nm.borrow().clone(), imp.selected.borrow().clone())
        else {
            return;
        };
        glib::spawn_future_local(async move {
            // A failed scan only means an older list; NetworkManager scans by itself too.
            let _ = nm.request_scan(&device).await;
        });
    }

    /// Puts a new, turning spinner next to "Searching".
    pub(super) fn new_spinner(&self) {
        let imp = self.imp();
        if let Some(old) = imp.spinner.take() {
            imp.spinner_slot.remove(&old);
        }
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        imp.spinner_slot.append(&spinner);
        imp.spinner.replace(Some(spinner));
    }
}
