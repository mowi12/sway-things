// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pairing agent's questions as dialogs, and cancelling a pairing.
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::bluetooth::{Answer, Prompt, Request, parse_passkey};

use super::dialogs::Dialog;
use super::{BluetoothPage, prompt_spec};

impl BluetoothPage {
    /// Shows the agent's `prompt` as a dialog, or updates the open one.
    pub(super) fn prompt(&self, prompt: Prompt) {
        let imp = self.imp();
        let name = self
            .device(&prompt.device)
            .map_or_else(|| prompt.device.clone(), |device| device.name);
        if let Request::DisplayPasskey { passkey, entered } = prompt.request {
            if let Some((path, dialog)) = imp.dialog.borrow().as_ref() {
                if *path == prompt.device {
                    dialog.set_typed(passkey, entered);
                    return;
                }
            }
        }
        self.close_dialog();
        let window = self.root().and_downcast::<gtk::Window>();
        let (spec, passkey) = prompt_spec(&prompt.request, &name);
        let device = prompt.device.clone();
        let weak = self.downgrade();
        let request = prompt.request.clone();
        let mut prompt = Some(prompt);
        let dialog = Dialog::open(window.as_ref(), &spec, move |answer| {
            let Some(prompt) = prompt.take() else {
                return;
            };
            if !prompt.request.needs_answer() {
                // Only shown: closing it cancels the pairing.
                if answer.is_none() {
                    if let Some(page) = weak.upgrade() {
                        page.cancel_pairing(&prompt.device);
                    }
                }
                return;
            }
            prompt.answer(match (answer, &request) {
                (None, _) => Answer::Reject,
                (Some(pin), Request::PinCode) => Answer::PinCode(pin.trim().to_owned()),
                (Some(text), Request::Passkey) => {
                    parse_passkey(&text).map_or(Answer::Reject, Answer::Passkey)
                }
                (Some(_), _) => Answer::Accept,
            });
            if let Some(page) = weak.upgrade() {
                page.imp().dialog.replace(None);
            }
        });
        if let Some((passkey, entered)) = passkey {
            dialog.set_typed(passkey, entered);
        }
        imp.dialog.replace(Some((device, dialog)));
    }

    /// Stops the pairing of the device at `path`.
    pub(super) fn cancel_pairing(&self, path: &str) {
        let Some(bluez) = self.imp().bluez.borrow().clone() else {
            return;
        };
        let path = path.to_owned();
        glib::spawn_future_local(async move {
            // The pairing may have ended already; its result is reported by the pairing itself.
            let _ = bluez.cancel_pairing(&path).await;
        });
    }

    /// Closes the agent's dialog, if one is open.
    pub(super) fn close_dialog(&self) {
        if let Some((_, dialog)) = self.imp().dialog.take() {
            dialog.close();
        }
    }

    /// Closes the agent's dialog if it is about the device at `path`.
    pub(super) fn close_dialog_of(&self, path: &str) {
        let open = self
            .imp()
            .dialog
            .borrow()
            .as_ref()
            .is_some_and(|(device, _)| device == path);
        if open {
            self.close_dialog();
        }
    }
}
