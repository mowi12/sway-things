// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The _Portals_ row of the Environment page, in place of the row of
//! `environment.portal_config`: what the check of the XDG desktop portals found, in a line, and
//! _Details_, a dialog with its four steps and the fixes Sway Things can make in its own files:
//!
//! 1. **Installed:** xdg-desktop-portal, -wlr and -gtk; a missing one with the command that
//!    installs it, to copy.
//! 2. **Configured:** the backend of each interface and the file that decides; _Create
//!    Configuration_ (`environment.portal_config`) where the user has none of their own, else
//!    _Open in Editor_ and the lines to copy.
//! 3. **Session variables:** `WAYLAND_DISPLAY` and `XDG_CURRENT_DESKTOP` in the user manager;
//!    _Switch On_ for `environment.pass_session_variables`.
//! 4. **Running:** a portal on the session bus.
//!
//! The check runs when the row is made, after each change of the settings, and on _Check
//! Again_.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::environment::portals::{self, Check, Component};
use sway_things_core::packages::Manager;
use sway_things_core::registry::Value;
use sway_things_core::registry::environment::{PASS_SESSION_VARIABLES, PORTAL_CONFIG};

use super::variables::editor_button;
use super::{DIRS, copy_button, dialog_window, tilde};
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;
use crate::widgets::list;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The title of the row and the dialog.
const PORTALS: Msg = msg!("Portals");
/// The row's line while the check runs.
const CHECKING: Msg = msg!("Checking…");
/// The button that checks again.
const CHECK_AGAIN: Msg = msg!("Check Again");
/// The row's line when everything is in order.
const READY: Msg = msg!("Ready for screen sharing, screenshots and file pickers");
/// The row, following `model` until it is gone.
pub fn row(model: &Model) -> gtk::Widget {
    let title = gtk::Label::new(Some(tr(PORTALS)));
    title.add_css_class("st-row-title");
    let status = gtk::Label::new(Some(tr(CHECKING)));
    status.add_css_class(style::DIM);
    status.set_wrap(true);
    status.set_ellipsize(gtk::pango::EllipsizeMode::None);
    let details = gtk::Button::with_mnemonic(tr(msg!("_Details")));
    details.set_valign(gtk::Align::Center);
    details.add_css_class("st-portals-details");
    let content = list::row(&title, &status, &[details.upcast_ref()]);
    content.add_css_class("st-portals-row");
    let section = list::section_list(tr(PORTALS));
    section.add_css_class("st-environment-list");
    section.append(&list::list_row(&content));

    let found: Rc<RefCell<Option<Check>>> = Rc::new(RefCell::new(None));
    let dialog: Rc<RefCell<Option<gtk::Box>>> = Rc::new(RefCell::new(None));
    let running = Rc::new(Cell::new(false));
    let again = Rc::new(Cell::new(false));
    let checker = Checker {
        status: status.downgrade(),
        found: found.clone(),
        dialog: dialog.clone(),
        running,
        again,
        model: model.downgrade(),
    };
    checker.run();
    let follower = checker.clone();
    let handler = RefCell::new(Some(model.connect_changed(move |_| follower.run())));
    section.connect_destroy(glib::clone!(
        #[weak]
        model,
        move |_| {
            if let Some(handler) = handler.take() {
                model.disconnect(handler);
            }
        }
    ));
    details.connect_clicked(glib::clone!(
        #[weak]
        model,
        move |button| {
            let (window, content) = dialog_window(button.upcast_ref(), tr(PORTALS));
            let body = gtk::Box::new(gtk::Orientation::Vertical, 14);
            content.append(&body);
            let again = gtk::Button::with_label(tr(CHECK_AGAIN));
            again.set_halign(gtk::Align::End);
            let rerun = checker.clone();
            again.connect_clicked(move |_| rerun.run());
            content.append(&again);
            if let Some(check) = found.borrow().as_ref() {
                fill(&body, &model, check);
            }
            *dialog.borrow_mut() = Some(body);
            let forget = dialog.clone();
            window.connect_close_request(move |_| {
                forget.borrow_mut().take();
                glib::Propagation::Proceed
            });
            window.present();
        }
    ));
    section.upcast()
}

/// Runs the check and shows what it found in the row and an open dialog.
#[derive(Clone)]
struct Checker {
    status: glib::WeakRef<gtk::Label>,
    found: Rc<RefCell<Option<Check>>>,
    dialog: Rc<RefCell<Option<gtk::Box>>>,
    running: Rc<Cell<bool>>,
    again: Rc<Cell<bool>>,
    model: glib::WeakRef<Model>,
}

impl Checker {
    /// Checks, or once more after the check that runs.
    fn run(&self) {
        if self.running.replace(true) {
            self.again.set(true);
            return;
        }
        let Some(dirs) = DIRS.with(Clone::clone) else {
            self.running.set(false);
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let check = portals::check(&dirs).await;
            this.running.set(false);
            let (Some(status), Some(model)) = (this.status.upgrade(), this.model.upgrade()) else {
                return;
            };
            status.set_text(&summary(&check));
            if let Some(body) = this.dialog.borrow().as_ref() {
                fill(body, &model, &check);
            }
            *this.found.borrow_mut() = Some(check);
            if this.again.replace(false) {
                this.run();
            }
        });
    }
}

/// The row's line: ready, or what is wrong.
fn summary(check: &Check) -> String {
    let mut wrong = Vec::new();
    if !check.missing.is_empty() {
        let packages: Vec<&str> = check
            .missing
            .iter()
            .map(|component| component.package())
            .collect();
        wrong.push(trf(
            msg!("Not installed: {packages}"),
            &[("packages", &packages.join(", "))],
        ));
    } else if check.config_wrong() {
        wrong.push(
            tr(msg!(
                "The configuration doesn't pick wlr for screen sharing"
            ))
            .to_owned(),
        );
    }
    if check.variables_missing() {
        wrong.push(tr(msg!("The session's variables are missing in systemd")).to_owned());
    }
    if check.running == Some(false) {
        wrong.push(tr(msg!("Not running yet")).to_owned());
    }
    if wrong.is_empty() {
        tr(READY).to_owned()
    } else {
        wrong.join(" · ")
    }
}

/// Fills `body` with the four steps of `check`.
fn fill(body: &gtk::Box, model: &Model, check: &Check) {
    while let Some(child) = body.first_child() {
        body.remove(&child);
    }
    let writable = model.status().is_some_and(|status| status.is_active());
    body.append(&installed(check));
    body.append(&configured(model, check, writable));
    body.append(&session_variables(model, check, writable));
    body.append(&running(check));
}

/// A step: a heading with a sign for whether it passed, then `parts`.
fn step(title: &str, ok: Option<bool>, parts: &[gtk::Widget]) -> gtk::Widget {
    let sign = gtk::Image::from_icon_name(match ok {
        Some(true) => "object-select-symbolic",
        Some(false) => "dialog-warning-symbolic",
        None => "dialog-question-symbolic",
    });
    if ok == Some(false) {
        sign.add_css_class("st-environment-warning");
    }
    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");
    heading.set_accessible_role(gtk::AccessibleRole::Heading);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    top.append(&sign);
    top.append(&heading);
    let step = gtk::Box::new(gtk::Orientation::Vertical, 6);
    step.add_css_class("st-portals-step");
    step.append(&top);
    for part in parts {
        step.append(part);
    }
    step.upcast()
}

/// A line of text, dim.
fn text(said: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(said));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class(style::DIM);
    label.upcast()
}

/// Lines to copy, in monospace, with a button that copies them.
fn to_copy(lines: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(lines));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.add_css_class("monospace");
    let copy = copy_button(&label);
    copy.set_valign(gtk::Align::Center);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.add_css_class("st-portals-copy");
    line.append(&label);
    line.append(&copy);
    line.upcast()
}

/// A button that stores `value` for `key`.
fn set_button(
    model: &Model,
    label: &str,
    key: &'static str,
    value: bool,
    writable: bool,
) -> gtk::Widget {
    let button = gtk::Button::with_label(label);
    button.set_halign(gtk::Align::Start);
    button.set_sensitive(writable);
    button.connect_clicked(glib::clone!(
        #[weak]
        model,
        move |_| model.set(key, None, Value::Bool(value))
    ));
    button.upcast()
}

/// Step 1: what is installed.
fn installed(check: &Check) -> gtk::Widget {
    let manager = Manager::detect();
    let mut parts = Vec::new();
    for component in Component::ALL {
        if check.missing.contains(&component) {
            parts.push(text(&trf(
                msg!("{package} isn't installed"),
                &[("package", &component.package())],
            )));
            if let Some(manager) = manager {
                parts.push(to_copy(&manager.install_command(&[component.package()])));
            }
        } else {
            parts.push(text(&trf(
                msg!("{package} is installed"),
                &[("package", &component.package())],
            )));
        }
    }
    step(
        tr(msg!("Installed")),
        Some(check.missing.is_empty()),
        &parts,
    )
}

/// Step 2: which backend serves each interface, and the file that decides.
fn configured(model: &Model, check: &Check, writable: bool) -> gtk::Widget {
    let mut parts = Vec::new();
    for (interface, backend) in &check.served {
        let backend = backend
            .clone()
            .unwrap_or_else(|| tr(msg!("none")).to_owned());
        parts.push(text(&format!("{interface}: {backend}")));
    }
    parts.push(text(&match &check.config {
        Some(path) => trf(msg!("Decided by {file}"), &[("file", &tilde(path))]),
        None => tr(msg!(
            "No configuration: the backends meant for the desktop serve, else gtk"
        ))
        .to_owned(),
    }));
    let ours = DIRS.with(|dirs| dirs.as_ref().map(portals::path));
    let ours_decides = ours.is_some() && check.config == ours;
    if let Some(users) = &check.users_config {
        parts.push(text(&trf(
            msg!(
                "{file} is your own: Sway Things doesn't change it. For screen sharing, put these \
                 lines into it:"
            ),
            &[("file", &tilde(users))],
        )));
        parts.push(to_copy(portals::LINES));
        parts.push(editor_button(model, users.clone(), 1).upcast());
    } else if ours_decides && model.state().is_some_and(|state| portals::enabled(&state)) {
        parts.push(text(tr(msg!(
            "Sway Things' configuration: wlr for screen sharing and screenshots, gtk for the rest"
        ))));
        parts.push(set_button(
            model,
            tr(msg!("Remove Configuration")),
            PORTAL_CONFIG,
            false,
            writable,
        ));
    } else if check.config_wrong() && check.can_configure {
        parts.push(text(tr(msg!(
            "Sway Things can write its own configuration: wlr for screen sharing and screenshots, \
             gtk for the rest. Saving writes it and restarts the portals."
        ))));
        parts.push(set_button(
            model,
            tr(msg!("Create Configuration")),
            PORTAL_CONFIG,
            true,
            writable,
        ));
    }
    step(tr(msg!("Configured")), Some(!check.config_wrong()), &parts)
}

/// Step 3: the session's variables in the user manager.
fn session_variables(model: &Model, check: &Check, writable: bool) -> gtk::Widget {
    let mut parts = Vec::new();
    let on = model.state().is_some_and(|state| {
        matches!(
            state.get(PASS_SESSION_VARIABLES, None),
            Some(Value::Bool(true))
        )
    });
    match check.session_variables {
        None => parts.push(text(tr(msg!("The systemd user manager didn't answer.")))),
        Some((wayland, desktop)) => {
            for (name, set) in [
                ("WAYLAND_DISPLAY", wayland),
                ("XDG_CURRENT_DESKTOP", desktop),
            ] {
                parts.push(text(&if set {
                    trf(msg!("{name} is set"), &[("name", &name)])
                } else {
                    trf(msg!("{name} is missing"), &[("name", &name)])
                }));
            }
            if check.variables_missing() && !on {
                parts.push(text(tr(msg!(
                    "Pass the session's variables to systemd and D-Bus at each login, and now:"
                ))));
                parts.push(set_button(
                    model,
                    tr(msg!("Switch On")),
                    PASS_SESSION_VARIABLES,
                    true,
                    writable,
                ));
            }
        }
    }
    if on {
        parts.push(text(tr(msg!("Sway Things passes them at each login."))));
    }
    let ok = check.session_variables.map(|_| !check.variables_missing());
    step(tr(msg!("Session variables")), ok, &parts)
}

/// Step 4: a portal on the session bus.
fn running(check: &Check) -> gtk::Widget {
    let said = match check.running {
        Some(true) => tr(msg!("A portal answers on the session bus.")),
        Some(false) => tr(msg!(
            "No portal answers yet. It starts when an app asks for it; if screen sharing still \
             fails, journalctl --user -u xdg-desktop-portal says why."
        )),
        None => tr(msg!("The session bus couldn't be asked.")),
    };
    step(tr(msg!("Running")), check.running, &[text(said)])
}
