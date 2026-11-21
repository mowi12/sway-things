// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the model follows besides the app: the devices, the outputs and reloads of sway, the user's
//! sway config and the settings files, each on the main loop.
use std::path::PathBuf;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::devices::{DeviceWatch, Devices};
use sway_things_core::include::IncludeWatch;
use sway_things_core::ipc::{Client, Reloads};
use sway_things_core::outputs::{OutputWatch, Outputs};

use super::{Model, NO_DEVICES, NO_DISPLAYS, NO_RELOADS, Op};
use crate::i18n::trf;

/// How long the displays stand unchanged before the app looks whether a picture went
/// ([`Model::heal_mirrors_later`]): kanshi has set them up and run its hooks by then.
const HEAL_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

impl Model {
    pub(super) fn watch_config(&self, mut watch: IncludeWatch) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                while let Some(status) = watch.changed().await {
                    model.set_status(status.clone());
                }
            }
        ));
    }

    pub(super) fn watch_state(&self, watcher: sway_things_core::watch::Watcher) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                while watcher.changed().await.is_some() {
                    model.queue(Op::Reload);
                }
            }
        ));
    }
}

impl Model {
    /// Follows the input devices of the session on `socket`: reads them, and again on each
    /// `input` event, until sway closes the connection.
    pub(super) fn watch_devices(&self, socket: PathBuf) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                let started = match Client::connect_to(socket).await {
                    Ok(client) => DeviceWatch::start(&client).await,
                    Err(error) => Err(error),
                };
                let mut watch = match started {
                    Ok(watch) => watch,
                    Err(error) => {
                        model.emit_problem(&trf(
                            NO_DEVICES,
                            &[("error", &error.text().translated())],
                        ));
                        return;
                    }
                };
                model.take_devices(watch.devices());
                // An error leaves the list as it was; the events after it still come.
                while watch.changed().await.is_some() {
                    model.take_devices(watch.devices());
                }
            }
        ));
    }

    /// Follows the outputs of the session on `socket`: reads them, and again on each `output`
    /// event, until sway closes the connection.
    pub(super) fn watch_outputs(&self, socket: PathBuf) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                let started = match Client::connect_to(socket).await {
                    Ok(client) => OutputWatch::start(&client).await,
                    Err(error) => Err(error),
                };
                let mut watch = match started {
                    Ok(watch) => watch,
                    Err(error) => {
                        model.emit_problem(&trf(
                            NO_DISPLAYS,
                            &[("error", &error.text().translated())],
                        ));
                        return;
                    }
                };
                model.take_outputs(watch.outputs());
                // An error leaves the list as it was; the events after it still come.
                while watch.changed().await.is_some() {
                    model.take_outputs(watch.outputs());
                }
                // sway is gone: what it reported is no longer so.
                model.forget_outputs();
            }
        ));
    }

    /// Forgets the connected outputs once sway is gone; those with stored values stay listed.
    pub(super) fn forget_outputs(&self) {
        if let Some(core) = self.imp().core.borrow_mut().as_mut() {
            let mut outputs = Outputs::default();
            outputs.set_stored(&core.state);
            core.outputs = outputs;
            core.outputs_known = false;
        }
        self.emit_by_name::<()>("changed", &[]);
    }

    /// Takes the connected outputs of `outputs`, and lists those with stored values too.
    pub(super) fn take_outputs(&self, outputs: &Outputs) {
        if let Some(core) = self.imp().core.borrow_mut().as_mut() {
            let mut outputs = outputs.clone();
            outputs.set_stored(&core.state);
            core.outputs = outputs;
            core.outputs_known = true;
        }
        // While kanshi sets up the displays, the stored values follow what it set up.
        self.queue(Op::FollowCombination);
        self.heal_mirrors_later();
        self.emit_by_name::<()>("changed", &[]);
    }

    /// Once the displays stand (no change for [`HEAL_AFTER`]: kanshi sets them up and runs its
    /// hooks meanwhile), starts a remembered picture of the combination again that doesn't run,
    /// as when its window was closed. When the app opens, too.
    fn heal_mirrors_later(&self) {
        let seen = self.imp().outputs_seen.get().wrapping_add(1);
        self.imp().outputs_seen.set(seen);
        if !self.imp().looks_for_kanshi.get() {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                glib::timeout_future(HEAL_AFTER).await;
                if model.imp().outputs_seen.get() == seen {
                    model.queue(Op::HealMirrors);
                }
            }
        ));
    }

    /// Follows the reloads of sway's config in the session on `socket`, whoever asks for them, so
    /// the changes that waited for one count as in effect.
    pub(super) fn watch_reloads(&self, socket: PathBuf) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = model)]
            self,
            async move {
                let started = match Client::connect_to(socket).await {
                    Ok(client) => Reloads::start(&client).await,
                    Err(error) => Err(error),
                };
                let mut reloads = match started {
                    Ok(reloads) => reloads,
                    Err(error) => {
                        model.emit_problem(&trf(
                            NO_RELOADS,
                            &[("error", &error.text().translated())],
                        ));
                        return;
                    }
                };
                // An event that can't be read is skipped; the events after it still come.
                while let Some(reloaded) = reloads.next().await {
                    if reloaded.is_ok() {
                        model.queue(Op::SwayReloaded);
                    }
                }
            }
        ));
    }

    /// Takes the connected devices of `devices`, and lists those with stored values too.
    pub(super) fn take_devices(&self, devices: &Devices) {
        if let Some(core) = self.imp().core.borrow_mut().as_mut() {
            let mut devices = devices.clone();
            devices.set_stored(&core.state);
            core.devices = devices;
            core.devices_known = true;
        }
        self.emit_by_name::<()>("changed", &[]);
    }
}
