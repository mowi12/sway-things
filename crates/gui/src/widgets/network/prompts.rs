// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The secret agent's questions as dialogs: a password that is wrong or changed, while the page
//! is on screen or a connection it started is under way.
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::network::SecretPrompt;
use sway_things_core::network::Security;

use super::NetworkPage;
use super::dialogs;
use super::dialogs::PasswordSpec;
use crate::i18n::msg;
use crate::i18n::tr;
use crate::i18n::trf;

impl NetworkPage {
    /// Shows the agent's `prompt` as a dialog.
    pub(super) fn prompt(&self, prompt: SecretPrompt) {
        self.close_prompt();
        let window = self.root().and_downcast::<gtk::Window>();
        let spec = if prompt.retry {
            PasswordSpec {
                title: trf(
                    msg!("Wrong password for “{network}”"),
                    &[("network", &prompt.network)],
                ),
                body: tr(msg!("The password didn't work, or it has changed.")),
                security: if prompt.wpa3 {
                    Security::Wpa3Personal
                } else {
                    Security::Personal
                },
                saved_note: false,
            }
        } else {
            PasswordSpec {
                title: trf(
                    msg!("Password for “{network}”"),
                    &[("network", &prompt.network)],
                ),
                body: tr(msg!("Enter the password of the Wi-Fi network.")),
                security: if prompt.wpa3 {
                    Security::Wpa3Personal
                } else {
                    Security::Personal
                },
                saved_note: false,
            }
        };
        let connection = prompt.connection.clone();
        let weak = self.downgrade();
        let mut prompt = Some(prompt);
        let dialog = dialogs::password(window.as_ref(), &spec, move |password| {
            // Dropping the prompt without an answer cancels it.
            if let (Some(prompt), Some(password)) = (prompt.take(), password) {
                prompt.answer(password);
            }
            if let Some(page) = weak.upgrade() {
                page.imp().prompt.replace(None);
            }
        });
        self.imp().prompt.replace(Some((connection, dialog)));
    }

    /// Closes the agent's dialog, if one is open.
    pub(super) fn close_prompt(&self) {
        let open = self.imp().prompt.borrow().clone();
        if let Some((_, dialog)) = open {
            dialog.close();
        }
    }

    /// Closes the agent's dialog if it is about the connection at `path`.
    pub(super) fn close_prompt_of(&self, path: &str) {
        let open = self
            .imp()
            .prompt
            .borrow()
            .as_ref()
            .is_some_and(|(connection, _)| connection == path);
        if open {
            self.close_prompt();
        }
    }

    /// Registers our agent while the page is on screen or a connection it started is on its way,
    /// and unregisters it otherwise.
    pub(super) fn update_agent(&self) {
        let imp = self.imp();
        let Some(nm) = imp.nm.borrow().clone() else {
            return;
        };
        let wanted = imp.snapshot.borrow().is_some() && (self.is_mapped() || imp.pending.get() > 0);
        if imp.agent.get() == wanted {
            return;
        }
        imp.agent.set(wanted);
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            if wanted {
                if nm.register_agent().await.is_err() {
                    // Tried again with the next snapshot.
                    if let Some(page) = weak.upgrade() {
                        page.imp().agent.set(false);
                    }
                }
            } else {
                // NetworkManager may be gone; then there's nothing to unregister.
                let _ = nm.unregister_agent().await;
            }
        });
    }
}
