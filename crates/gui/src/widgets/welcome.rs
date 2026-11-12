// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The welcome screen while Sway Things is read-only: why, the one line to add with a copy
//! button, a button that opens the config in the default text editor, and that it waits for the
//! line. "Look around first" lets the user browse the pages read-only.
//!
//! The texts come from the core's [`IncludeStatus`], so the app and the command line say the
//! same: the line is missing, it covers only some of the files, or it may be in a part of the
//! config the parser can't follow.

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};
use sway_things_core::include::{IncludeStatus, Inclusion};

use super::Clamp;
use crate::style;

/// The widest the welcome screen gets, in pixels.
const WIDTH: i32 = 560;

mod imp {
    use std::cell::{OnceCell, RefCell};
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use gtk::glib;
    use gtk::glib::subclass::Signal;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct Welcome {
        pub(super) scroller: gtk::ScrolledWindow,
        pub(super) title: OnceCell<gtk::Label>,
        pub(super) text: gtk::Label,
        pub(super) line: gtk::Label,
        pub(super) copy: gtk::Button,
        pub(super) open: gtk::Button,
        pub(super) error: gtk::Label,
        pub(super) look_around: gtk::Button,
        /// Where the spinner is: it is replaced each time the screen is shown.
        pub(super) spinner_slot: gtk::Box,
        pub(super) spinner: RefCell<Option<gtk::Spinner>>,
        pub(super) config: RefCell<Option<PathBuf>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Welcome {
        const NAME: &'static str = "StWelcome";
        type Type = super::Welcome;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-welcome");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for Welcome {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().build();
        }

        fn dispose(&self) {
            self.scroller.unparent();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![Signal::builder("look-around").build()])
        }
    }

    impl WidgetImpl for Welcome {}
}

glib::wrapper! {
    /// The welcome screen while Sway Things is read-only.
    pub struct Welcome(ObjectSubclass<imp::Welcome>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Welcome {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl Welcome {
    fn build(&self) {
        let imp = self.imp();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
        content.add_css_class("st-welcome-content");

        let icon = gtk::Image::builder()
            .icon_name("preferences-system-symbolic")
            .pixel_size(48)
            .halign(gtk::Align::Start)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();
        content.append(&icon);

        let title = imp.title.get_or_init(|| {
            gtk::Label::builder()
                .accessible_role(gtk::AccessibleRole::Heading)
                .css_classes(["title-1"])
                .build()
        });
        for label in [title, &imp.text, &imp.error] {
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        }
        content.append(title);
        content.append(&imp.text);

        imp.line.add_css_class("monospace");
        imp.line.add_css_class("st-include-line");
        imp.line.set_selectable(true);
        // The copy button is the keyboard's way to it.
        imp.line.set_focusable(false);
        imp.line.set_xalign(0.0);
        imp.line.set_hexpand(true);
        imp.line.set_wrap(true);
        imp.line.set_wrap_mode(gtk::pango::WrapMode::Char);
        imp.copy.set_label("Copy");
        imp.copy.set_valign(gtk::Align::Center);
        imp.copy
            .update_property(&[gtk::accessible::Property::Label("Copy the line")]);
        imp.copy.connect_clicked(glib::clone!(
            #[weak(rename_to = welcome)]
            self,
            move |_| welcome.copy_line()
        ));
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        line.append(&imp.line);
        line.append(&imp.copy);
        content.append(&line);

        imp.open.set_label("Open config in editor");
        imp.open.add_css_class("suggested-action");
        imp.open.connect_clicked(glib::clone!(
            #[weak(rename_to = welcome)]
            self,
            move |_| welcome.open_config()
        ));
        // A spinner's animation stops when the screen leaves the content pane, and setting it
        // spinning again doesn't restart it: each time the screen is shown, it gets a new one.
        self.connect_map(Self::new_spinner);
        let waiting = gtk::Label::new(Some("Waiting for the line…"));
        waiting.add_css_class(style::DIM);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        actions.append(&imp.open);
        actions.append(&imp.spinner_slot);
        actions.append(&waiting);
        content.append(&actions);

        imp.error.add_css_class("error");
        imp.error.set_visible(false);
        content.append(&imp.error);

        let note = gtk::Label::new(Some(
            "This screen closes by itself once the line is in your config. Until then you can \
             look around: every page shows your current values, read-only.",
        ));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class(style::DIM);
        content.append(&note);

        imp.look_around.set_label("Look around first");
        imp.look_around.set_halign(gtk::Align::Start);
        imp.look_around.connect_clicked(glib::clone!(
            #[weak(rename_to = welcome)]
            self,
            move |_| welcome.emit_by_name::<()>("look-around", &[])
        ));
        content.append(&imp.look_around);

        let clamp = Clamp::new(&content, WIDTH);
        imp.scroller
            .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        imp.scroller.set_child(Some(&clamp));
        imp.scroller.set_parent(self);
        self.update_property(&[gtk::accessible::Property::Label("Welcome to Sway Things")]);
    }

    /// Shows what `status` says: why Sway Things is read-only, and the line to add.
    pub fn update(&self, status: &IncludeStatus) {
        let imp = self.imp();
        let title = match status.inclusion() {
            Inclusion::Missing => "Welcome to Sway Things",
            _ => "Sway Things isn't enabled yet",
        };
        if let Some(label) = imp.title.get() {
            label.set_text(title);
        }
        imp.text.set_text(&status.explanation());
        imp.line.set_text(status.instructions().line());
        let config = status.instructions().config().to_owned();
        // A config that doesn't exist yet has to be created first, as the text says.
        imp.open.set_visible(config.is_file());
        imp.config.replace(Some(config));
    }

    /// The button the keyboard starts on: "Open config in editor", or "Copy" without a config to
    /// open.
    pub fn first_button(&self) -> gtk::Button {
        let imp = self.imp();
        if imp.open.is_visible() {
            imp.open.clone()
        } else {
            imp.copy.clone()
        }
    }

    /// Puts a new spinner next to "Waiting for the line…", which starts turning.
    fn new_spinner(&self) {
        let imp = self.imp();
        if let Some(old) = imp.spinner.take() {
            imp.spinner_slot.remove(&old);
        }
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        imp.spinner_slot.append(&spinner);
        imp.spinner.replace(Some(spinner));
    }

    /// Whether the spinner next to "Waiting for the line…" spins.
    pub fn is_waiting(&self) -> bool {
        self.is_mapped()
            && self
                .imp()
                .spinner
                .borrow()
                .as_ref()
                .is_some_and(gtk::Spinner::is_spinning)
    }

    /// The spinner next to "Waiting for the line…".
    pub fn spinner(&self) -> Option<gtk::Spinner> {
        self.imp().spinner.borrow().clone()
    }

    /// The line the screen asks for.
    pub fn line(&self) -> String {
        self.imp().line.text().to_string()
    }

    /// The button that opens the config, the one that copies the line, and "Look around first".
    pub fn buttons(&self) -> [gtk::Button; 3] {
        let imp = self.imp();
        [imp.open.clone(), imp.copy.clone(), imp.look_around.clone()]
    }

    /// Calls `callback` when the user wants to look around first.
    pub fn connect_look_around<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "look-around",
            false,
            glib::closure_local!(move |welcome: &Self| callback(welcome)),
        )
    }

    fn copy_line(&self) {
        let imp = self.imp();
        self.clipboard().set_text(&imp.line.text());
        imp.copy.set_label("Copied");
        glib::timeout_add_seconds_local_once(
            2,
            glib::clone!(
                #[weak(rename_to = welcome)]
                self,
                move || welcome.imp().copy.set_label("Copy")
            ),
        );
    }

    /// Opens the config with the desktop's default application for text files.
    fn open_config(&self) {
        let imp = self.imp();
        let Some(config) = imp.config.borrow().clone() else {
            return;
        };
        imp.error.set_visible(false);
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&config)));
        let window = self.root().and_downcast::<gtk::Window>();
        launcher.launch(
            window.as_ref(),
            gio::Cancellable::NONE,
            glib::clone!(
                #[weak(rename_to = welcome)]
                self,
                move |result| {
                    if let Err(error) = result {
                        let error_label = &welcome.imp().error;
                        error_label.set_text(&format!("The config can't be opened: {error}"));
                        error_label.set_visible(true);
                    }
                }
            ),
        );
    }
}
