// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sending a value: at once, or for the settings of displays deferred until the slider is let go,
//! so a drag sends one change.
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::registry::{Scope, Value};

use super::SettingRow;
use super::controls::COMMIT_DELAY;

impl SettingRow {
    /// Sends `value` as the new value of the setting.
    pub(super) fn send(&self, value: Value) {
        let imp = self.imp();
        if imp.updating.get() {
            return;
        }
        let (Some(setting), Some(model)) = (self.setting(), imp.model.get()) else {
            return;
        };
        self.show_error(None);
        model.set(setting.key(), imp.device.borrow().as_deref(), value);
    }

    /// Whether the changes of the row's number control wait until the pointer is let go and
    /// the control stayed still for a moment: for a display's scale. A display that changes its
    /// scale changes the size of every window on it, the app's too, under the pointer: a change
    /// sent while a button is down (a slider dragged, a step button held) would move what the
    /// pointer is on, and the button could be let go elsewhere, leaving the step button
    /// repeating.
    pub(super) fn defers(&self) -> bool {
        self.setting()
            .is_some_and(|setting| setting.scope() == Scope::PerOutput)
    }

    /// Sends `value` from the number control: at once, or for a control that defers
    /// ([`SettingRow::defers`]) once the pointer is let go and the control stayed still.
    pub(super) fn send_number(&self, value: Value) {
        if self.imp().updating.get() {
            return;
        }
        if !self.defers() {
            self.send(value);
            return;
        }
        self.imp().unsent.replace(Some(value));
        self.plan_commit();
    }

    /// Sends the value that waits after [`COMMIT_DELAY`], unless a button is down then.
    pub(super) fn plan_commit(&self) {
        let imp = self.imp();
        if let Some(planned) = imp.commit.take() {
            planned.remove();
        }
        let planned = glib::timeout_add_local_once(
            COMMIT_DELAY,
            glib::clone!(
                #[weak(rename_to = row)]
                self,
                move || {
                    let imp = row.imp();
                    // It fired: nothing to remove any more.
                    imp.commit.replace(None);
                    // Sent when the button is let go; if that never reached the control (the
                    // window lost the pointer meanwhile), once no button is down any more.
                    if imp.held.get() {
                        if imp.touched.get() || row.button_down() {
                            row.plan_commit();
                            return;
                        }
                        imp.held.set(false);
                    }
                    if let Some(value) = imp.unsent.take() {
                        row.send(value);
                    }
                }
            ),
        );
        imp.commit.replace(Some(planned));
    }

    /// Sends the value of the number control that waits now, for the display shown, unless that
    /// went (which is why another one is shown, or the row goes).
    pub(super) fn flush_unsent(&self) {
        let imp = self.imp();
        if let Some(planned) = imp.commit.take() {
            planned.remove();
        }
        imp.held.set(false);
        imp.touched.set(false);
        let Some(value) = imp.unsent.take() else {
            return;
        };
        let shown = imp.device.borrow().clone();
        let still_there = shown.as_deref().is_none_or(|shown| {
            imp.model.get().is_some_and(|model| {
                model.outputs().is_some_and(|outputs| {
                    outputs
                        .get(shown)
                        .is_some_and(sway_things_core::outputs::Output::is_connected)
                })
            })
        });
        if still_there {
            self.send(value);
        }
    }

    /// Whether a pointer button is down on the pointer of the row's seat, as the window's
    /// surface last heard of it.
    pub(super) fn button_down(&self) -> bool {
        let buttons = gtk::gdk::ModifierType::BUTTON1_MASK
            | gtk::gdk::ModifierType::BUTTON2_MASK
            | gtk::gdk::ModifierType::BUTTON3_MASK;
        let Some(pointer) = self
            .display()
            .default_seat()
            .and_then(|seat| seat.pointer())
        else {
            return false;
        };
        self.native()
            .and_then(|native| native.surface())
            .and_then(|surface| surface.device_position(&pointer))
            .is_some_and(|(_, _, state)| state.intersects(buttons))
    }

    /// Follows whether a pointer button is down on `widget`, a part of a number control that
    /// defers: the value waits while it is, and is sent a moment after it is let go.
    pub(super) fn follow_presses(&self, widget: &gtk::Widget) {
        let presses = gtk::EventControllerLegacy::new();
        presses.set_propagation_phase(gtk::PropagationPhase::Capture);
        presses.connect_event(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                match event.event_type() {
                    gtk::gdk::EventType::ButtonPress => row.imp().held.set(true),
                    gtk::gdk::EventType::TouchBegin => {
                        row.imp().held.set(true);
                        row.imp().touched.set(true);
                    }
                    gtk::gdk::EventType::ButtonRelease
                    | gtk::gdk::EventType::TouchEnd
                    | gtk::gdk::EventType::TouchCancel => {
                        row.imp().held.set(false);
                        row.imp().touched.set(false);
                        if row.imp().unsent.borrow().is_some() {
                            row.plan_commit();
                        }
                    }
                    _ => {}
                }
                glib::Propagation::Proceed
            }
        ));
        widget.add_controller(presses);
    }

    /// Whether the number control shows a value that isn't sent yet.
    pub(super) fn holds_unsent(&self) -> bool {
        let imp = self.imp();
        imp.held.get() || imp.unsent.borrow().is_some()
    }
}
