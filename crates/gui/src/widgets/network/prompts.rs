// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The secret agent's questions as dialogs: a password that is wrong or changed, the fields of a
//! VPN's plugin, or the steps of an OpenConnect sign-in, while the page is on screen or a
//! connection it started is under way.
use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::network::{SecretPrompt, Security, SignIn, SignInStep, VpnPrompt};

use super::dialogs::PasswordSpec;
use super::{NetworkPage, dialogs};
use crate::i18n::{msg, tr, trf};

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
        self.imp()
            .prompt
            .replace(Some((connection, dialog.opened())));
    }

    /// Shows the agent's VPN `prompt` as a dialog with the plugin's fields.
    pub(super) fn vpn_prompt(&self, prompt: VpnPrompt) {
        self.close_prompt();
        let window = self.root().and_downcast::<gtk::Window>();
        let title = if prompt.retry {
            trf(
                msg!("Sign in to VPN “{vpn}” again"),
                &[("vpn", &prompt.name)],
            )
        } else {
            trf(msg!("Sign in to VPN “{vpn}”"), &[("vpn", &prompt.name)])
        };
        let connection = prompt.connection.clone();
        let description = prompt.description.clone();
        let fields = prompt.fields.clone();
        let weak = self.downgrade();
        let mut prompt = Some(prompt);
        let dialog = dialogs::vpn(
            window.as_ref(),
            &title,
            &description,
            &fields,
            move |values| {
                // Dropping the prompt without an answer cancels it.
                if let (Some(prompt), Some(values)) = (prompt.take(), values) {
                    prompt.answer(values);
                }
                if let Some(page) = weak.upgrade() {
                    page.imp().prompt.replace(None);
                }
            },
        );
        self.imp()
            .prompt
            .replace(Some((connection, dialog.opened())));
    }

    /// Shows the steps of the agent's OpenConnect `sign_in` in one dialog until it ends.
    pub(super) fn sign_in(&self, sign_in: SignIn) {
        self.close_prompt();
        let window = self.root().and_downcast::<gtk::Window>();
        let title = trf(msg!("Sign in to VPN “{vpn}”"), &[("vpn", &sign_in.name)]);
        let connection = sign_in.connection.clone();
        // Dropping the sign-in cancels it: the dialog holds it until it closes.
        let sign_in = Rc::new(RefCell::new(Some(sign_in)));
        let answering = Rc::clone(&sign_in);
        let closing = Rc::clone(&sign_in);
        let weak = self.downgrade();
        let dialog = dialogs::SignInDialog::new(
            window.as_ref(),
            &title,
            move |value| {
                if let Some(sign_in) = answering.borrow().as_ref() {
                    sign_in.answer(value);
                }
            },
            move |_| {
                closing.replace(None);
                if let Some(page) = weak.upgrade() {
                    page.imp().prompt.replace(None);
                }
            },
        );
        self.imp()
            .prompt
            .replace(Some((connection, dialog.opened())));
        let Some(steps) = sign_in.borrow().as_ref().map(SignIn::steps) else {
            return;
        };
        let task = glib::spawn_future_local(async move {
            loop {
                match steps.recv().await {
                    Ok(SignInStep::Ask { message, field }) => dialog.ask(&message, &field),
                    Ok(SignInStep::Browser) => dialog.browser(),
                    // Signed in, failed or canceled: the switch says how it went.
                    Err(_) => {
                        dialog.finish();
                        return;
                    }
                }
            }
        });
        self.imp().tasks.borrow_mut().push(task);
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
