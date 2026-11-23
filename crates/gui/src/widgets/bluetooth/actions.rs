// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the page does: connect, pair, disconnect, rename and forget devices, rename the adapter
//! and make it visible.
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::bluetooth::{Bluez, Device};

use super::dialogs::Dialog;
use super::{Action, BluetoothPage, dialogs, refresh, rename_spec, set_running};
use crate::i18n::{msg, tr, trf};

impl BluetoothPage {
    /// The device at `path`, as last read.
    pub(super) fn device(&self, path: &str) -> Option<Device> {
        self.imp()
            .snapshot
            .borrow()
            .as_ref()
            .and_then(|snapshot| snapshot.device(path).cloned())
    }

    /// What the row's button does: connect (pairing first if needed) or disconnect, or cancel
    /// the connection or pairing that runs.
    pub(super) fn main_action(&self, path: &str) {
        let running = self.imp().running.borrow().get(path).copied();
        if running.is_some_and(Action::cancelable) {
            self.cancel(path);
            return;
        }
        let Some(device) = self.device(path) else {
            return;
        };
        let action = if device.connected {
            Action::Disconnect
        } else if device.paired {
            Action::Connect
        } else {
            Action::Pair
        };
        self.run(action, path);
    }

    /// Runs `action` on the device at `path`; a failure shows under the controls. Pairing goes on
    /// to connect; forgetting asks first, renaming asks for the name.
    pub(super) fn run(&self, action: Action, path: &str) {
        let imp = self.imp();
        let (Some(bluez), Some(device)) = (imp.bluez.borrow().clone(), self.device(path)) else {
            return;
        };
        if imp.running.borrow().contains_key(path) {
            return;
        }
        if action == Action::Rename {
            self.rename(&bluez, &device);
            return;
        }
        let weak = self.downgrade();
        let window = self.root().and_downcast::<gtk::Window>();
        glib::spawn_future_local(async move {
            if action == Action::Forget
                && !dialogs::confirm_forget(&device.name, window.as_ref()).await
            {
                return;
            }
            let mut action = action;
            let result = loop {
                set_running(&weak, &device.path, Some(action));
                let result = match action {
                    Action::Pair => bluez.pair(&device.path).await,
                    Action::Connect => bluez.connect_device(&device.path).await,
                    Action::Disconnect => bluez.disconnect_device(&device.path).await,
                    Action::Forget => bluez.remove(&device.adapter, &device.path).await,
                    Action::Rename => Ok(()),
                };
                if action == Action::Pair {
                    if let Some(page) = weak.upgrade() {
                        page.close_dialog_of(&device.path);
                    }
                }
                // A canceled call fails, or ends before the cancel arrives: its result is dropped,
                // and a pairing doesn't go on to connect.
                let canceled = weak
                    .upgrade()
                    .is_some_and(|page| page.imp().canceled.borrow_mut().remove(&device.path));
                if canceled {
                    break Ok(());
                }
                match result {
                    // A device pairs to be used: connect it right away.
                    Ok(()) if action == Action::Pair => action = Action::Connect,
                    result => break result,
                }
            };
            set_running(&weak, &device.path, None);
            if let Some(page) = weak.upgrade() {
                page.show_error(&match result {
                    Ok(()) => String::new(),
                    Err(error) => action.failure(&device.name, &error),
                });
            }
            refresh(&weak, &bluez).await;
        });
    }

    /// Cancels the connection or pairing that runs on the device at `path`: BlueZ ends a pending
    /// `Connect` on `Disconnect` and a pending `Pair` on `CancelPairing`. The running call then
    /// returns, and [`run`](Self::run) drops its result.
    pub(super) fn cancel(&self, path: &str) {
        let imp = self.imp();
        let Some(bluez) = imp.bluez.borrow().clone() else {
            return;
        };
        let Some(action) = imp.running.borrow().get(path).copied() else {
            return;
        };
        if !self.mark_canceled(path, action) {
            return;
        }
        self.render();
        let path = path.to_owned();
        glib::spawn_future_local(async move {
            cancel_call(&bluez, action, &path).await;
        });
    }

    /// Marks `action` on the device at `path` as canceled and closes its pairing dialog. False if
    /// it can't be canceled or is already.
    pub(super) fn mark_canceled(&self, path: &str, action: Action) -> bool {
        if !action.cancelable() || !self.imp().canceled.borrow_mut().insert(path.to_owned()) {
            return false;
        }
        self.close_dialog_of(path);
        true
    }

    /// Asks for a new name for the selected adapter and sets it.
    pub(super) fn rename_adapter(&self) {
        let imp = self.imp();
        let adapter = imp.selected.borrow().clone().and_then(|path| {
            let snapshot = imp.snapshot.borrow();
            Some((
                path.clone(),
                snapshot.as_ref()?.adapter(&path)?.name.clone(),
            ))
        });
        let (Some(bluez), Some((path, old))) = (imp.bluez.borrow().clone(), adapter) else {
            return;
        };
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        Dialog::open(
            window.as_ref(),
            &rename_spec(
                &old,
                tr(msg!("Leave it empty to use the adapter's own name.")),
            ),
            move |name| {
                let Some(name) = name else {
                    return;
                };
                glib::spawn_future_local(async move {
                    let result = bluez.set_adapter_alias(&path, &name).await;
                    if let Some(page) = weak.upgrade() {
                        page.show_error(&match result {
                            Ok(()) => String::new(),
                            Err(error) => Action::Rename.failure(&old, &error),
                        });
                    }
                    refresh(&weak, &bluez).await;
                });
            },
        );
    }

    /// The handler of the "Visible to other devices" switch.
    pub(super) fn visible_changed(&self, on: bool) -> glib::Propagation {
        let imp = self.imp();
        let adapter = imp.selected.borrow().clone().and_then(|path| {
            let snapshot = imp.snapshot.borrow();
            let discoverable = snapshot.as_ref()?.adapter(&path)?.discoverable;
            Some((path, discoverable))
        });
        let (Some(bluez), Some((path, discoverable))) = (imp.bluez.borrow().clone(), adapter)
        else {
            return glib::Propagation::Proceed;
        };
        if discoverable == on {
            return glib::Propagation::Proceed;
        }
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let result = bluez.set_discoverable(&path, on).await;
            if let Some(page) = weak.upgrade() {
                page.show_error(&match result {
                    Ok(()) => String::new(),
                    Err(error) => trf(
                        if on {
                            msg!("Couldn't make the computer visible: {error}")
                        } else {
                            msg!("Couldn't hide the computer: {error}")
                        },
                        &[("error", &error.text().translated())],
                    ),
                });
            }
            // A refused change puts the switch back.
            refresh(&weak, &bluez).await;
        });
        // The switch shows the new position; its state follows once BlueZ reports it.
        glib::Propagation::Stop
    }

    /// Asks for a new name for `device` and sets it.
    pub(super) fn rename(&self, bluez: &Bluez, device: &Device) {
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let bluez = bluez.clone();
        let path = device.path.clone();
        let old = device.name.clone();
        Dialog::open(
            window.as_ref(),
            &rename_spec(
                &device.name,
                tr(msg!("Leave it empty to use the name the device announces.")),
            ),
            move |name| {
                let Some(name) = name else {
                    return;
                };
                glib::spawn_future_local(async move {
                    set_running(&weak, &path, Some(Action::Rename));
                    let result = bluez.set_alias(&path, &name).await;
                    set_running(&weak, &path, None);
                    if let Some(page) = weak.upgrade() {
                        page.show_error(&match result {
                            Ok(()) => String::new(),
                            Err(error) => Action::Rename.failure(&old, &error),
                        });
                    }
                    refresh(&weak, &bluez).await;
                });
            },
        );
    }
}

/// The call that cancels `action` on the device at `path`. Its error is dropped: the call may come
/// after the action ended, and the canceled call reports the outcome.
pub(super) async fn cancel_call(bluez: &Bluez, action: Action, path: &str) {
    let _ = match action {
        Action::Pair => bluez.cancel_pairing(path).await,
        _ => bluez.disconnect_device(path).await,
    };
}
