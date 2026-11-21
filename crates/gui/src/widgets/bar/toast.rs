// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A short note with _Undo_ below the part of the page it is about: an item was removed, a
//! preset was applied. It stays ten seconds, is announced to screen readers, and _Undo_ (or
//! Ctrl+Z anywhere on the page) takes the change back.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use crate::i18n::{msg, tr};

/// How long a note stays.
const SHOWN: Duration = Duration::from_secs(10);

/// The note and its timer.
#[derive(Debug, Clone)]
pub struct Toast {
    revealer: gtk::Revealer,
    label: gtk::Label,
    timer: Rc<RefCell<Option<glib::SourceId>>>,
}

impl Toast {
    /// A hidden note whose _Undo_ calls `undo`.
    pub fn new(undo: impl Fn() + 'static) -> Self {
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        let undo_button = gtk::Button::with_label(tr(msg!("Undo")));
        undo_button.set_valign(gtk::Align::Center);
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        close.set_valign(gtk::Align::Center);
        close.set_tooltip_text(Some(tr(msg!("Dismiss"))));
        close.update_property(&[gtk::accessible::Property::Label(tr(msg!("Dismiss")))]);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        content.add_css_class("st-bar-toast");
        content.append(&label);
        content.append(&undo_button);
        content.append(&close);
        let revealer = gtk::Revealer::new();
        revealer.set_child(Some(&content));
        revealer.set_reveal_child(false);
        let toast = Self {
            revealer,
            label,
            timer: Rc::new(RefCell::new(None)),
        };
        let hide = toast.clone();
        undo_button.connect_clicked(move |_| {
            hide.hide();
            undo();
        });
        let hide = toast.clone();
        close.connect_clicked(move |_| hide.hide());
        toast
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    /// Shows `text` for a while and announces it.
    pub fn show(&self, text: &str) {
        self.label.set_text(text);
        self.revealer.set_reveal_child(true);
        self.revealer
            .announce(text, gtk::AccessibleAnnouncementPriority::Medium);
        let weak = self.revealer.downgrade();
        let timer = self.timer.clone();
        let id = glib::timeout_add_local_once(SHOWN, move || {
            timer.take();
            if let Some(revealer) = weak.upgrade() {
                revealer.set_reveal_child(false);
            }
        });
        if let Some(old) = self.timer.replace(Some(id)) {
            old.remove();
        }
    }

    /// Hides it at once.
    pub fn hide(&self) {
        if let Some(old) = self.timer.take() {
            old.remove();
        }
        self.revealer.set_reveal_child(false);
    }
}
