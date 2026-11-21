// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialogs of the Bluetooth page: the questions of the pairing agent, renaming a device and
//! forgetting one.
//!
//! Device names are untrusted: they only ever go into label text and dialog messages, which GTK
//! shows as plain text.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bluetooth::passkey_text;

use crate::i18n::{msg, tr, trf};
use crate::style;

/// The style class of the digits of a passkey already typed on the device.
const TYPED: &str = "st-passkey-typed";

/// A text field in a dialog.
pub struct Field {
    /// Its label.
    pub label: &'static str,
    /// What it starts with.
    pub text: String,
    /// Whether a text can be accepted.
    pub valid: fn(&str) -> bool,
}

/// What a dialog shows.
pub struct Spec {
    /// The question, in bold.
    pub title: String,
    /// What to do or check.
    pub body: String,
    /// A code to compare or type, large.
    pub code: Option<String>,
    /// A text field.
    pub field: Option<Field>,
    /// A dim line below.
    pub note: Option<String>,
    /// The label of the button that accepts; `None` for a dialog that only shows something.
    pub accept: Option<&'static str>,
}

/// An open dialog.
#[derive(Clone)]
pub struct Dialog {
    window: gtk::Window,
    typed: gtk::Label,
    untyped: gtk::Label,
    note: gtk::Label,
    /// Called once: with the field's text (empty without one) on accept, `None` otherwise.
    done: Done,
}

/// What a dialog calls with its answer, until it has.
type Done = Rc<RefCell<Option<Box<dyn FnOnce(Option<String>)>>>>;

impl Dialog {
    /// Opens a dialog with `spec` over `parent`. `done` gets the answer once; closing the dialog
    /// any way but the accept button counts as no.
    pub fn open(
        parent: Option<&gtk::Window>,
        spec: &Spec,
        done: impl FnOnce(Option<String>) + 'static,
    ) -> Self {
        let window = gtk::Window::builder()
            .modal(true)
            .resizable(false)
            .destroy_with_parent(true)
            .default_width(360)
            .title(&spec.title)
            .build();
        window.set_transient_for(parent);
        window.add_css_class("st-dialog");

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(24);
        content.set_margin_bottom(16);
        content.set_margin_start(20);
        content.set_margin_end(20);

        let title = centered(&spec.title);
        title.add_css_class("title-2");
        title.set_accessible_role(gtk::AccessibleRole::Heading);
        content.append(&title);
        content.append(&centered(&spec.body));

        let typed = gtk::Label::new(None);
        typed.add_css_class(TYPED);
        let untyped = gtk::Label::new(None);
        if let Some(code) = &spec.code {
            untyped.set_text(code);
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            line.set_halign(gtk::Align::Center);
            line.add_css_class("st-passkey");
            line.append(&typed);
            line.append(&untyped);
            content.append(&line);
        }

        let entry = spec.field.as_ref().map(|field| {
            let label = gtk::Label::new(Some(field.label));
            label.set_xalign(0.0);
            label.add_css_class(style::DIM);
            let entry = gtk::Entry::builder()
                .text(&field.text)
                .activates_default(true)
                .build();
            entry.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
            content.append(&label);
            content.append(&entry);
            (entry, field.valid)
        });

        let note = centered(spec.note.as_deref().unwrap_or_default());
        note.add_css_class(style::DIM);
        note.set_visible(spec.note.is_some());
        content.append(&note);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_homogeneous(true);
        buttons.set_margin_top(8);
        let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
        buttons.append(&cancel);
        content.append(&buttons);
        window.set_child(Some(&content));

        let dialog = Self {
            window: window.clone(),
            typed,
            untyped,
            note,
            done: Rc::new(RefCell::new(Some(Box::new(done)))),
        };

        if let Some(label) = spec.accept {
            let accept = gtk::Button::with_label(label);
            accept.add_css_class("suggested-action");
            buttons.append(&accept);
            window.set_default_widget(Some(&accept));
            if let Some((entry, valid)) = &entry {
                let valid = *valid;
                accept.set_sensitive(valid(&entry.text()));
                entry.connect_changed(glib::clone!(
                    #[weak]
                    accept,
                    move |entry| accept.set_sensitive(valid(&entry.text()))
                ));
            }
            let text = entry.as_ref().map(|(entry, _)| entry.clone());
            accept.connect_clicked(glib::clone!(
                #[strong]
                dialog,
                move |_| {
                    let answer = text.as_ref().map(|entry| entry.text().to_string());
                    dialog.finish(Some(answer.unwrap_or_default()));
                }
            ));
        }
        dialog.connect_closing(&cancel);

        window.present();
        if let Some((entry, _)) = &entry {
            entry.grab_focus();
        } else if spec.accept.is_none() {
            cancel.grab_focus();
        }
        dialog
    }

    /// Makes Cancel, Escape and closing the window answer no.
    fn connect_closing(&self, cancel: &gtk::Button) {
        let dialog = self;
        let window = &self.window;
        cancel.connect_clicked(glib::clone!(
            #[strong(rename_to = dialog)]
            self,
            move |_| dialog.finish(None)
        ));
        let done = Rc::clone(&dialog.done);
        window.connect_close_request(move |_| {
            if let Some(done) = done.borrow_mut().take() {
                done(None);
            }
            glib::Propagation::Proceed
        });
        let escape = gtk::ShortcutController::new();
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::CallbackAction::new(glib::clone!(
                #[strong(rename_to = dialog)]
                self,
                move |_, _| {
                    dialog.finish(None);
                    glib::Propagation::Stop
                }
            ))),
        ));
        window.add_controller(escape);
    }

    /// Shows that `entered` digits of the code are typed on the device.
    pub fn set_typed(&self, passkey: u32, entered: u16) {
        let (typed, untyped) = split_typed(passkey, entered);
        self.typed.set_text(&typed);
        self.untyped.set_text(&untyped);
        self.note.set_text(&trf(
            msg!("{count} of 6 digits typed"),
            &[("count", &entered.min(6))],
        ));
        self.note.set_visible(true);
    }

    /// Closes the dialog without an answer, as when BlueZ cancels.
    pub fn close(&self) {
        self.finish(None);
    }

    /// The dialog's window.
    pub fn window(&self) -> &gtk::Window {
        &self.window
    }

    /// Answers once and closes.
    fn finish(&self, answer: Option<String>) {
        if let Some(done) = self.done.borrow_mut().take() {
            done(answer);
        }
        self.window.destroy();
    }
}

/// A centered label that wraps, for plain text.
fn centered(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_justify(gtk::Justification::Center);
    label.set_max_width_chars(40);
    label
}

/// `passkey` as a device shows it, in two groups of three digits (`012 345`).
pub fn grouped(passkey: u32) -> String {
    let digits = passkey_text(passkey);
    let (first, second) = digits.split_at(digits.len().saturating_sub(3));
    format!("{first} {second}")
}

/// [`grouped`] split after the first `entered` digits.
fn split_typed(passkey: u32, entered: u16) -> (String, String) {
    let code = grouped(passkey);
    let mut digits = 0;
    let at = code
        .char_indices()
        .find(|(_, c)| {
            if c.is_ascii_digit() {
                digits += 1;
            }
            digits > usize::from(entered)
        })
        .map_or(code.len(), |(at, _)| at);
    let (typed, untyped) = code.split_at(at);
    (typed.to_owned(), untyped.to_owned())
}

/// Asks whether to forget the device `name`.
pub async fn confirm_forget(name: &str, window: Option<&gtk::Window>) -> bool {
    crate::widgets::live::confirm_forget(
        &trf(msg!("Forget “{device}”?"), &[("device", &name)]),
        tr(msg!(
            "The pairing is removed. To use the device again, pair it again."
        )),
        window,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passkeys_are_grouped_and_split_after_the_typed_digits() {
        assert_eq!(grouped(12_345), "012 345");
        assert_eq!(
            split_typed(481_520, 0),
            (String::new(), "481 520".to_owned())
        );
        assert_eq!(
            split_typed(481_520, 2),
            ("48".to_owned(), "1 520".to_owned())
        );
        assert_eq!(
            split_typed(481_520, 3),
            ("481 ".to_owned(), "520".to_owned())
        );
        assert_eq!(
            split_typed(481_520, 6),
            ("481 520".to_owned(), String::new())
        );
        assert_eq!(
            split_typed(481_520, 9),
            ("481 520".to_owned(), String::new())
        );
    }
}
