// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Region & Time page: the time zone, automatic time and the clock, live from
//! systemd-timedated, and the automatic time zone from geoclue's location; the user's language
//! and formats, and the system's through systemd-localed ([`language`]).
//!
//! Of the Date & Time section, Sway Things only stores whether the time zone follows the
//! location (`region.automatic_timezone`); the rest is timedated's own. Every change
//! goes to timedated, whose polkit rule asks for an administrator's password through the user's
//! polkit agent. When polkit refuses, the section shows why and the `timedatectl` command that
//! does the same in a terminal. Zone names come from timedated and tzdata; they are only ever
//! shown as plain label text.

mod clock;
mod language;
mod time;
mod zones;

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::region::Bus;
use sway_things_core::registry::Registry;
use sway_things_core::registry::region::AUTOMATIC_TIMEZONE;

use self::language::LanguageSection;
use self::time::TimeSection;
use crate::i18n::{msg, tr};
use crate::model::Model;
use crate::style;
use crate::widgets::SettingRow;

mod imp {
    use std::cell::{OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    use super::language::LanguageSection;
    use super::time::TimeSection;

    #[derive(Default)]
    pub struct RegionTimePage {
        /// The only child: the sections, one below the other.
        pub content: gtk::Box,
        pub(super) time: OnceCell<TimeSection>,
        pub(super) language: OnceCell<LanguageSection>,
        /// The rows of the settings the page stores.
        pub rows: RefCell<Vec<crate::widgets::SettingRow>>,
        /// The automatic time zone as last followed; `None` before the first time.
        pub automatic: std::cell::Cell<Option<bool>>,
        /// Set when leaving the page stopped the automatic time zone's lookup: it starts again
        /// when the page is shown.
        pub resume: std::cell::Cell<bool>,
        pub tasks: RefCell<Vec<glib::JoinHandle<()>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RegionTimePage {
        const NAME: &'static str = "StRegionTimePage";
        type Type = super::RegionTimePage;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-region-time");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for RegionTimePage {
        fn constructed(&self) {
            self.parent_constructed();
            self.content.set_orientation(gtk::Orientation::Vertical);
            self.content.set_parent(&*self.obj());
        }

        fn dispose(&self) {
            for task in self.tasks.take() {
                task.abort();
            }
            if let Some(time) = self.time.get() {
                time.stop_ticking();
            }
            self.content.unparent();
        }
    }

    impl WidgetImpl for RegionTimePage {
        fn map(&self) {
            self.parent_map();
            if let Some(time) = self.time.get() {
                time.start_ticking();
                if self.resume.replace(false) {
                    time.detect();
                }
            }
        }

        fn unmap(&self) {
            self.parent_unmap();
            if let Some(time) = self.time.get() {
                time.stop_ticking();
                self.resume.set(time.cancel_detect());
            }
        }
    }
}

glib::wrapper! {
    /// The time zone, automatic time and the clock, live from timedated; the language and
    /// formats.
    pub struct RegionTimePage(ObjectSubclass<imp::RegionTimePage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl RegionTimePage {
    /// The page with timedated, localed (and geoclue) on `bus`, which it connects to right away and
    /// follows, and with the rows of the settings it stores from `model`, if there is one.
    pub fn new(bus: &Bus, model: Option<&Model>) -> Self {
        let page: Self = glib::Object::new();
        let time = TimeSection::new(&page.imp().content, true);
        time.follow(bus.clone());
        if let Some(row) = model.and_then(|model| {
            let setting = Registry::builtin().get(AUTOMATIC_TIMEZONE)?;
            SettingRow::new(model, setting, None)
        }) {
            time.insert_row(&row);
            page.imp().rows.borrow_mut().push(row);
        }
        let _ = page.imp().time.set(time);
        let language = LanguageSection::new(&page.imp().content, model, bus);
        page.imp().rows.borrow_mut().extend(language.rows());
        let _ = page.imp().language.set(language);
        page
    }

    /// The rows of the settings the page stores, for search and deep links.
    pub fn rows(&self) -> Vec<SettingRow> {
        self.imp().rows.borrow().clone()
    }

    /// Follows the automatic time zone: turned on (or on when the page opens), the zone where
    /// the computer is gets set; turned off, a lookup still running stops silently.
    pub fn set_automatic_timezone(&self, on: bool) {
        let was = self.imp().automatic.replace(Some(on));
        let Some(time) = self.imp().time.get() else {
            return;
        };
        if on && was != Some(true) {
            time.detect();
        } else if !on && was != Some(false) {
            time.cancel_detect();
            time.forget_detect();
            self.imp().resume.set(false);
        }
    }

    /// The Language & Formats section, for tests.
    pub fn language_section(&self) -> Option<&LanguageSection> {
        self.imp().language.get()
    }

    /// The Date & Time section, for tests.
    pub fn time_section(&self) -> Option<&TimeSection> {
        self.imp().time.get()
    }
}

/// The help shown under a section when a change failed: why, and the command that does the same
/// in a terminal, with _Copy_.
#[derive(Debug, Default)]
pub struct Help {
    /// Holds the text and the command line; hidden without a failure.
    pub root: gtk::Box,
    text: gtk::Label,
    line: gtk::Box,
    command: gtk::Label,
    copy: gtk::Button,
    /// The command _Copy_ copies.
    copied: std::rc::Rc<RefCell<String>>,
}

impl Help {
    /// The help, hidden.
    pub fn new() -> Self {
        let help = Self::default();
        help.root.set_orientation(gtk::Orientation::Vertical);
        help.root.set_spacing(6);
        help.root.set_margin_top(8);
        help.root.set_visible(false);
        help.text.set_xalign(0.0);
        help.text.set_wrap(true);
        help.text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        help.text.add_css_class("error");
        help.root.append(&help.text);
        help.line.set_spacing(8);
        help.command.set_xalign(0.0);
        help.command.set_hexpand(true);
        help.command.set_wrap(true);
        help.command.set_wrap_mode(gtk::pango::WrapMode::Char);
        help.command.set_selectable(true);
        help.command.add_css_class("monospace");
        help.copy.set_label(tr(msg!("Copy")));
        help.copy.set_valign(gtk::Align::Center);
        help.copy
            .update_property(&[gtk::accessible::Property::Label(tr(msg!(
                "Copy the command"
            )))]);
        let copied = std::rc::Rc::clone(&help.copied);
        help.copy.connect_clicked(move |button| {
            button.clipboard().set_text(&copied.borrow());
            button.set_label(tr(msg!("Copied")));
        });
        help.line.append(&help.command);
        help.line.append(&help.copy);
        help.root.append(&help.line);
        help
    }

    /// Shows `text`, and `command` with _Copy_ if there is one; `None` hides the help.
    pub fn show(&self, text: Option<&str>, command: Option<&str>) {
        self.root.set_visible(text.is_some());
        self.text.set_text(text.unwrap_or_default());
        self.line.set_visible(command.is_some());
        self.command.set_text(command.unwrap_or_default());
        command
            .unwrap_or_default()
            .clone_into(&mut self.copied.borrow_mut());
        self.copy.set_label(tr(msg!("Copy")));
    }

    /// The text shown, empty while hidden; for tests.
    pub fn text(&self) -> String {
        if self.root.is_visible() {
            self.text.text().to_string()
        } else {
            String::new()
        }
    }

    /// The command shown, empty without one; for tests.
    pub fn command(&self) -> String {
        if self.root.is_visible() && self.line.is_visible() {
            self.command.text().to_string()
        } else {
            String::new()
        }
    }
}

/// A dim label for the subtitle of a row.
fn subtitle() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class(style::DIM);
    label
}

/// A modal dialog over `parent` named `title`, and the box for its content. `resizable` for a
/// dialog with a list.
fn dialog(parent: Option<&gtk::Window>, title: &str, resizable: bool) -> (gtk::Window, gtk::Box) {
    let window = gtk::Window::builder()
        .modal(true)
        .resizable(resizable)
        .destroy_with_parent(true)
        .title(title)
        .build();
    window.set_transient_for(parent);
    window.add_css_class("st-dialog");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    window.set_child(Some(&content));
    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(glib::clone!(
            #[weak]
            window,
            #[upgrade_or]
            glib::Propagation::Stop,
            move |_, _| {
                window.close();
                glib::Propagation::Stop
            }
        ))),
    ));
    window.add_controller(escape);
    (window, content)
}

/// A _Cancel_ button that closes `window`.
fn cancel_button(window: &gtk::Window) -> gtk::Button {
    let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
    cancel.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));
    cancel
}
