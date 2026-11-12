// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The main window: the sidebar and the content pane.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::registry::Page;

use crate::i18n::tr;
use crate::model::Model;
use crate::widgets::{PageView, Placeholder, Sidebar, Welcome};

/// The width of the sidebar, in pixels.
const SIDEBAR_WIDTH: i32 = 240;

/// The name of the welcome screen in the content pane.
const WELCOME: &str = "welcome";

mod imp {
    use std::cell::{Cell, OnceCell};

    use gtk::glib;
    use gtk::subclass::prelude::*;
    use sway_things_core::registry::Page;

    use crate::model::Model;
    use crate::widgets::{Sidebar, Welcome};

    #[derive(Debug, Default)]
    pub struct Window {
        pub sidebar: Sidebar,
        pub pages: gtk::Stack,
        pub model: OnceCell<Model>,
        pub banner: gtk::Revealer,
        pub banner_text: gtk::Label,
        /// The welcome screen while the include line is missing.
        pub welcome: Welcome,
        /// Whether the user chose to look around instead of the welcome screen.
        pub welcome_dismissed: Cell<bool>,
        /// The banner with the include status: read-only, or a line that isn't last.
        pub status: gtk::Revealer,
        pub status_text: gtk::Label,
        pub status_button: gtk::Button,
        /// Whether the user dismissed the note that the line isn't last.
        pub note_dismissed: Cell<bool>,
        /// The page open last, to go back to from the welcome screen.
        pub last_page: Cell<Option<Page>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "StWindow";
        type Type = super::Window;
        type ParentType = gtk::ApplicationWindow;
    }

    impl ObjectImpl for Window {}

    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
}

glib::wrapper! {
    /// The main window of Sway Things.
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gtk::gio::ActionGroup, gtk::gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    /// The main window of `app` for the settings of `model`, showing the first page.
    pub fn new(app: &gtk::Application, model: &Model) -> Self {
        let window: Self = glib::Object::builder().property("application", app).build();
        window.build(model);
        window
    }

    /// A main window for the settings of `model` that belongs to no application, e.g. in tests.
    pub fn with_model(model: &Model) -> Self {
        let window: Self = glib::Object::new();
        window.build(model);
        window
    }

    fn build(&self, model: &Model) {
        self.set_title(Some("Sway Things"));
        self.set_default_size(980, 700);

        let imp = self.imp();
        imp.sidebar.set_size_request(SIDEBAR_WIDTH, -1);
        imp.sidebar.connect_page_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, page| window.show_page(page)
        ));

        let _ = imp.model.set(model.clone());
        imp.pages.set_hexpand(true);
        imp.pages.set_vexpand(true);

        // Problems that aren't one setting's: a generated file that couldn't be written, …
        imp.banner_text.set_xalign(0.0);
        imp.banner_text.set_hexpand(true);
        imp.banner_text.set_wrap(true);
        imp.banner_text
            .set_wrap_mode(gtk::pango::WrapMode::WordChar);
        let dismiss = gtk::Button::with_label("Dismiss");
        dismiss.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.imp().banner.set_reveal_child(false)
        ));
        let banner = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        banner.add_css_class("st-banner");
        banner.append(&imp.banner_text);
        banner.append(&dismiss);
        imp.banner.set_child(Some(&banner));
        model.connect_problem(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, message| window.show_problem(message)
        ));

        // The include status: why everything is read-only, or that some of the user's lines win.
        imp.status_text.set_xalign(0.0);
        imp.status_text.set_hexpand(true);
        imp.status_text.set_wrap(true);
        imp.status_text
            .set_wrap_mode(gtk::pango::WrapMode::WordChar);
        imp.status_button.set_valign(gtk::Align::Center);
        imp.status_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.status_action()
        ));
        let status = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        status.add_css_class("st-banner");
        status.append(&imp.status_text);
        status.append(&imp.status_button);
        imp.status.set_child(Some(&status));

        imp.welcome.connect_look_around(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.look_around()
        ));
        imp.pages.add_named(&imp.welcome, Some(WELCOME));
        model.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.follow_status()
        ));

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("view");
        content.set_hexpand(true);
        content.append(&imp.status);
        content.append(&imp.banner);
        content.append(&imp.pages);

        let panes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        panes.append(&imp.sidebar);
        panes.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        panes.append(&content);
        self.set_child(Some(&panes));

        // Esc in the content pane goes back to the sidebar.
        let escape = gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::CallbackAction::new(|widget, _| {
                widget
                    .downcast_ref::<Self>()
                    .is_some_and(Self::back_to_sidebar)
                    .into()
            })),
        );
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.add_shortcut(escape);
        self.add_controller(shortcuts);

        if let Some(first) = Page::ALL.first() {
            self.open_page(*first);
        }
        self.follow_status();
    }

    /// Opens `page` and marks it in the sidebar. Opening a page from the welcome screen counts as
    /// looking around.
    pub fn show_page(&self, page: Page) {
        if self.is_welcome_shown() {
            self.imp().welcome_dismissed.set(true);
        }
        self.open_page(page);
        self.follow_status();
    }

    /// Opens `page` and marks it in the sidebar.
    fn open_page(&self, page: Page) {
        let imp = self.imp();
        if imp.pages.child_by_name(page.id()).is_none() {
            imp.pages.add_named(&self.build_page(page), Some(page.id()));
        }
        imp.pages.set_visible_child_name(page.id());
        imp.sidebar.select(page);
        imp.last_page.set(Some(page));
    }

    /// Whether the welcome screen fills the content pane.
    pub fn is_welcome_shown(&self) -> bool {
        self.imp().pages.visible_child_name().as_deref() == Some(WELCOME)
    }

    /// The welcome screen.
    pub fn welcome(&self) -> &Welcome {
        &self.imp().welcome
    }

    /// The text of the banner with the include status, if it is shown.
    pub fn status_banner(&self) -> Option<String> {
        let imp = self.imp();
        imp.status
            .reveals_child()
            .then(|| imp.status_text.text().to_string())
    }

    /// The button of the banner with the include status.
    pub fn status_button(&self) -> &gtk::Button {
        &self.imp().status_button
    }

    /// Leaves the welcome screen for the page that was open, with the banner above it.
    pub fn look_around(&self) {
        let imp = self.imp();
        imp.welcome_dismissed.set(true);
        let page = imp.last_page.get().or_else(|| Page::ALL.first().copied());
        if let Some(page) = page {
            self.show_page(page);
        }
    }

    /// What the button of the status banner does: shows the instructions again while read-only,
    /// and dismisses the note that the line isn't last otherwise.
    fn status_action(&self) {
        let imp = self.imp();
        let read_only = self.read_only_status().is_some();
        if read_only {
            imp.welcome_dismissed.set(false);
        } else {
            imp.note_dismissed.set(true);
        }
        self.follow_status();
    }

    /// The include status, if Sway Things is read-only.
    fn read_only_status(&self) -> Option<sway_things_core::include::IncludeStatus> {
        self.imp()
            .model
            .get()
            .and_then(Model::status)
            .filter(|status| !status.is_active())
    }

    /// Shows the welcome screen or the banner as the include status asks: the welcome screen
    /// while read-only (unless the user chose to look around), a banner with a way back to it,
    /// and once the line is there, a note if some of the user's lines come after it.
    fn follow_status(&self) {
        let imp = self.imp();
        let Some(status) = imp.model.get().and_then(Model::status) else {
            return;
        };
        if status.is_active() {
            if self.is_welcome_shown() {
                // The line is there: on to the page that was open.
                imp.welcome_dismissed.set(true);
                if let Some(page) = imp.last_page.get().or_else(|| Page::ALL.first().copied()) {
                    imp.pages.set_visible_child_name(page.id());
                    imp.sidebar.select(page);
                }
            }
            let note = !status.is_last() && !imp.note_dismissed.get();
            if note {
                imp.status_text.set_text(&status.explanation());
                imp.status_button.set_label("Dismiss");
            }
            imp.status.set_reveal_child(note);
            return;
        }
        imp.welcome.update(&status);
        if imp.welcome_dismissed.get() {
            imp.status_text
                .set_text("Not enabled yet: add one line to your sway config to change settings.");
            imp.status_button.set_label("Show instructions");
            imp.status.set_reveal_child(true);
        } else {
            imp.status.set_reveal_child(false);
            if !self.is_welcome_shown() {
                imp.pages.set_visible_child_name(WELCOME);
                // The keyboard starts on the welcome screen, which also keeps the sidebar from
                // marking the row it would focus.
                GtkWindowExt::set_focus(self, Some(&imp.welcome.first_button()));
                imp.sidebar.unselect();
            }
        }
    }

    fn build_page(&self, page: Page) -> PageView {
        let Some(model) = self.imp().model.get() else {
            return PageView::unbuilt(page);
        };
        if let Some(error) = model.error() {
            let placeholder =
                Placeholder::new("dialog-error-symbolic", "Settings can't be loaded", &error);
            return PageView::new(tr(page.title()), tr(page.description()), &placeholder);
        }
        PageView::for_settings(model, page, None).unwrap_or_else(|| PageView::unbuilt(page))
    }

    /// Shows `message` in the banner above the page.
    pub fn show_problem(&self, message: &str) {
        let imp = self.imp();
        imp.banner_text.set_text(message);
        imp.banner.set_reveal_child(true);
    }

    /// The text of the banner above the page, if it is shown.
    pub fn problem(&self) -> Option<String> {
        let imp = self.imp();
        imp.banner
            .reveals_child()
            .then(|| imp.banner_text.text().to_string())
    }

    /// The settings of the window.
    pub fn model(&self) -> Option<&Model> {
        self.imp().model.get()
    }

    /// The open page.
    pub fn page(&self) -> Option<Page> {
        self.imp()
            .pages
            .visible_child_name()
            .and_then(|id| Page::from_id(&id))
    }

    /// The sidebar.
    pub fn sidebar(&self) -> &Sidebar {
        &self.imp().sidebar
    }

    /// The open page's widget in the content pane.
    pub fn content(&self) -> Option<gtk::Widget> {
        self.imp().pages.visible_child()
    }

    /// What Esc does: moves the focus from the content pane back to the open page in the sidebar.
    /// Returns whether the focus was in the content pane.
    pub fn back_to_sidebar(&self) -> bool {
        let imp = self.imp();
        let in_content = gtk::prelude::GtkWindowExt::focus(self)
            .is_some_and(|focus| focus.is_ancestor(&imp.pages));
        in_content && imp.sidebar.focus_selected()
    }
}
