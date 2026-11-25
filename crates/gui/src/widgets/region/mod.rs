// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Region & Time page: the time zone, automatic time and the clock, live from
//! systemd-timedated.
//!
//! Nothing of the Date & Time section is stored by Sway Things: timedated owns it. Every change
//! goes to timedated, whose polkit rule asks for an administrator's password through the user's
//! polkit agent. When polkit refuses, the section shows why and the `timedatectl` command that
//! does the same in a terminal. Zone names come from timedated and tzdata; they are only ever
//! shown as plain label text.

mod clock;
mod time;
mod zones;

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::region::Bus;

use self::time::TimeSection;
use crate::i18n::{msg, tr};
use crate::style;

mod imp {
    use std::cell::{OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    use super::time::TimeSection;

    #[derive(Default)]
    pub struct RegionTimePage {
        /// The only child: the sections, one below the other.
        pub content: gtk::Box,
        pub(super) time: OnceCell<TimeSection>,
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
            }
        }

        fn unmap(&self) {
            self.parent_unmap();
            if let Some(time) = self.time.get() {
                time.stop_ticking();
            }
        }
    }
}

glib::wrapper! {
    /// The time zone, automatic time and the clock, live from timedated.
    pub struct RegionTimePage(ObjectSubclass<imp::RegionTimePage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl RegionTimePage {
    /// The page with timedated on `bus`, which it connects to right away and follows.
    pub fn new(bus: &Bus) -> Self {
        let page: Self = glib::Object::new();
        let time = TimeSection::new(&page.imp().content, true);
        time.follow(bus.clone());
        let _ = page.imp().time.set(time);
        page
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
