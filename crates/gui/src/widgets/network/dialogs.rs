// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialogs of the Network page: a Wi-Fi password (for a new network, or when the secret agent
//! asks again), a hidden network, a connection's details, and forgetting a network.
//!
//! Network names are untrusted: they only ever go into label text, window titles and dialog
//! messages, which GTK shows as plain text.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::network::{Security, check_password, check_ssid};

use crate::i18n::{Msg, msg, tr, trf};
use crate::style;

/// The securities the hidden network dialog offers, with their labels.
const HIDDEN_SECURITY: [(Security, Msg); 3] = [
    // TRANSLATORS: The security of a Wi-Fi network: none.
    (Security::Open, msg!("None")),
    (Security::Personal, msg!("WPA & WPA2 Personal")),
    (Security::Wpa3Personal, msg!("WPA3 Personal")),
];

/// What a dialog calls once with its answer.
type Done<T> = Rc<RefCell<Option<Box<dyn FnOnce(Option<T>)>>>>;

/// An open dialog that answers with a `T`, or with nothing when it is closed any other way than
/// its accept button.
pub struct Dialog<T> {
    window: gtk::Window,
    done: Done<T>,
}

impl<T> Clone for Dialog<T> {
    fn clone(&self) -> Self {
        Self {
            window: self.window.clone(),
            done: Rc::clone(&self.done),
        }
    }
}

impl<T: 'static> Dialog<T> {
    /// A dialog window titled `title` over `parent`, which calls `done` once.
    fn new(
        parent: Option<&gtk::Window>,
        title: &str,
        done: impl FnOnce(Option<T>) + 'static,
    ) -> (Self, gtk::Box) {
        let window = gtk::Window::builder()
            .modal(true)
            .resizable(false)
            .destroy_with_parent(true)
            .default_width(380)
            .title(title)
            .build();
        window.set_transient_for(parent);
        window.add_css_class("st-dialog");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(24);
        content.set_margin_bottom(16);
        content.set_margin_start(20);
        content.set_margin_end(20);
        let heading = gtk::Label::new(Some(title));
        heading.set_wrap(true);
        heading.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        heading.set_justify(gtk::Justification::Center);
        heading.set_max_width_chars(40);
        heading.add_css_class("title-2");
        heading.set_accessible_role(gtk::AccessibleRole::Heading);
        content.append(&heading);
        window.set_child(Some(&content));
        let dialog = Self {
            window,
            done: Rc::new(RefCell::new(Some(Box::new(done)))),
        };
        let closing = Rc::clone(&dialog.done);
        dialog.window.connect_close_request(move |_| {
            if let Some(done) = closing.borrow_mut().take() {
                done(None);
            }
            glib::Propagation::Proceed
        });
        let escape = gtk::ShortcutController::new();
        let weak = dialog.clone();
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::CallbackAction::new(move |_, _| {
                weak.finish(None);
                glib::Propagation::Stop
            })),
        ));
        dialog.window.add_controller(escape);
        (dialog, content)
    }

    /// Cancel and, if `accept` is given, the button that accepts, at the end of `content`.
    /// Returns the accept button.
    fn buttons(&self, content: &gtk::Box, accept: Option<&str>) -> Option<gtk::Button> {
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_homogeneous(true);
        buttons.set_margin_top(8);
        let cancel = gtk::Button::with_label(tr(if accept.is_some() {
            msg!("Cancel")
        } else {
            msg!("Close")
        }));
        let dialog = self.clone();
        cancel.connect_clicked(move |_| dialog.finish(None));
        buttons.append(&cancel);
        let accept = accept.map(|label| {
            let button = gtk::Button::with_label(label);
            button.add_css_class("suggested-action");
            buttons.append(&button);
            self.window.set_default_widget(Some(&button));
            button
        });
        content.append(&buttons);
        if accept.is_none() {
            cancel.grab_focus();
        }
        accept
    }

    /// Closes the dialog without an answer, as when NetworkManager stops waiting.
    pub fn close(&self) {
        self.finish(None);
    }

    /// The dialog's window.
    pub fn window(&self) -> &gtk::Window {
        &self.window
    }

    /// Answers once and closes.
    fn finish(&self, answer: Option<T>) {
        if let Some(done) = self.done.borrow_mut().take() {
            done(answer);
        }
        self.window.destroy();
    }
}

/// What the password dialog shows.
pub struct PasswordSpec {
    /// The question.
    pub title: String,
    /// What to do, below it.
    pub body: &'static str,
    /// The network's security, which says which passwords it takes.
    pub security: Security,
    /// Whether to say that the password is saved for all users.
    pub saved_note: bool,
}

/// Asks for a Wi-Fi password; `done` gets it, or `None`.
pub fn password(
    parent: Option<&gtk::Window>,
    spec: &PasswordSpec,
    done: impl FnOnce(Option<String>) + 'static,
) -> Dialog<String> {
    let (dialog, content) = Dialog::new(parent, &spec.title, done);
    let body = gtk::Label::new(Some(spec.body));
    body.set_wrap(true);
    body.set_justify(gtk::Justification::Center);
    body.set_max_width_chars(40);
    content.append(&body);
    let (entry, hint) = password_field(&content, spec.security);
    if spec.saved_note {
        let note = gtk::Label::new(Some(tr(msg!("Saved for all users of this computer."))));
        note.set_xalign(0.0);
        note.add_css_class(style::DIM);
        content.append(&note);
    }
    let security = spec.security;
    if let Some(accept) = dialog.buttons(&content, Some(tr(msg!("Connect")))) {
        let check = glib::clone!(
            #[weak]
            accept,
            #[weak]
            entry,
            #[weak]
            hint,
            move || {
                let valid = check_password(security, &entry.text()).is_ok();
                accept.set_sensitive(valid);
                show_validity(&hint, valid || entry.text().is_empty());
            }
        );
        check();
        entry.connect_changed(move |_| check());
        let answer = dialog.clone();
        accept.connect_clicked(glib::clone!(
            #[weak]
            entry,
            move |_| answer.finish(Some(entry.text().to_string()))
        ));
    }
    dialog.window.present();
    entry.grab_focus();
    dialog
}

/// The hidden network a user typed in: its SSID, security and password.
pub type Hidden = (String, Security, Option<String>);

/// Asks for a hidden network; `done` gets it, or `None`.
pub fn hidden(
    parent: Option<&gtk::Window>,
    done: impl FnOnce(Option<Hidden>) + 'static,
) -> Dialog<Hidden> {
    let (dialog, content) = Dialog::new(parent, tr(msg!("Connect to Hidden Network")), done);
    let ssid = gtk::Entry::builder().activates_default(true).build();
    labeled(&content, tr(msg!("Network name")), &ssid);
    let labels: Vec<&str> = HIDDEN_SECURITY
        .iter()
        .map(|(_, label)| tr(*label))
        .collect();
    let security = gtk::DropDown::from_strings(&labels);
    security.set_selected(1);
    labeled(&content, tr(msg!("Security")), &security);
    let password_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&password_box);
    let (entry, hint) = password_field(&password_box, Security::Personal);
    let chosen = |dropdown: &gtk::DropDown| {
        usize::try_from(dropdown.selected())
            .ok()
            .and_then(|i| HIDDEN_SECURITY.get(i))
            .map_or(Security::Personal, |(security, _)| *security)
    };
    if let Some(accept) = dialog.buttons(&content, Some(tr(msg!("Connect")))) {
        let check = glib::clone!(
            #[weak]
            accept,
            #[weak]
            ssid,
            #[weak]
            security,
            #[weak]
            entry,
            #[weak]
            hint,
            #[weak]
            password_box,
            move || {
                let kind = chosen(&security);
                password_box.set_visible(kind.needs_password());
                hint.set_text(password_hint(kind));
                let password_ok =
                    !kind.needs_password() || check_password(kind, &entry.text()).is_ok();
                show_validity(&hint, password_ok || entry.text().is_empty());
                accept.set_sensitive(check_ssid(&ssid.text()).is_ok() && password_ok);
            }
        );
        check();
        let on_ssid = check.clone();
        ssid.connect_changed(move |_| on_ssid());
        let on_password = check.clone();
        entry.connect_changed(move |_| on_password());
        security.connect_selected_notify(move |_| check());
        let answer = dialog.clone();
        accept.connect_clicked(glib::clone!(
            #[weak]
            ssid,
            #[weak]
            security,
            #[weak]
            entry,
            move |_| {
                let kind = chosen(&security);
                let password = kind.needs_password().then(|| entry.text().to_string());
                answer.finish(Some((ssid.text().to_string(), kind, password)));
            }
        ));
    }
    dialog.window.present();
    ssid.grab_focus();
    dialog
}

/// Shows `rows` (a label and a value each) about the connection `title`. The values can be
/// selected, to copy them.
pub fn details(parent: Option<&gtk::Window>, title: &str, rows: &[(&str, String)]) -> Dialog<()> {
    let (dialog, content) = Dialog::new(parent, title, |_| {});
    let grid = gtk::Grid::builder()
        .column_spacing(16)
        .row_spacing(6)
        .margin_top(4)
        .build();
    for (row, (label, value)) in (0..).zip(rows) {
        let name = gtk::Label::new(Some(label));
        name.set_xalign(0.0);
        name.set_yalign(0.0);
        name.add_css_class(style::DIM);
        let value = gtk::Label::new(Some(value));
        value.set_xalign(0.0);
        value.set_selectable(true);
        value.set_wrap(true);
        value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        value.set_hexpand(true);
        value.update_relation(&[gtk::accessible::Relation::LabelledBy(&[name.upcast_ref()])]);
        grid.attach(&name, 0, row, 1, 1);
        grid.attach(&value, 1, row, 1, 1);
    }
    content.append(&grid);
    dialog.buttons(&content, None);
    dialog.window.present();
    dialog
}

/// Asks whether to forget the network `name`.
pub async fn confirm_forget(name: &str, window: Option<&gtk::Window>) -> bool {
    crate::widgets::live::confirm_forget(
        &trf(msg!("Forget “{network}”?"), &[("network", &name)]),
        tr(msg!(
            "Its password is removed from this computer, and it won't connect by itself any more."
        )),
        window,
    )
    .await
}

/// A labeled password field for `security` in `content`, with the hint below; both returned.
fn password_field(content: &gtk::Box, security: Security) -> (gtk::PasswordEntry, gtk::Label) {
    let entry = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .activates_default(true)
        .build();
    labeled(content, tr(msg!("Password")), &entry);
    let hint = gtk::Label::new(Some(password_hint(security)));
    hint.set_xalign(0.0);
    hint.add_css_class(style::DIM);
    content.append(&hint);
    (entry, hint)
}

/// What a password for `security` must be.
fn password_hint(security: Security) -> &'static str {
    match security {
        Security::Personal => tr(msg!("8 to 63 characters")),
        _ => tr(msg!("Not empty")),
    }
}

/// Shows `hint` as an error while the password isn't `valid`.
fn show_validity(hint: &gtk::Label, valid: bool) {
    if valid {
        hint.remove_css_class("error");
        hint.add_css_class(style::DIM);
    } else {
        hint.remove_css_class(style::DIM);
        hint.add_css_class("error");
    }
}

/// Puts `label` above `field` in `content`, labeling it for screen readers.
fn labeled(content: &gtk::Box, label: &str, field: &impl IsA<gtk::Widget>) {
    let label = gtk::Label::new(Some(label));
    label.set_xalign(0.0);
    label.add_css_class(style::DIM);
    field
        .upcast_ref::<gtk::Widget>()
        .update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
    content.append(&label);
    content.append(field);
}
