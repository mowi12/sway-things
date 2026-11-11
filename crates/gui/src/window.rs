// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The main window: the sidebar and the content pane.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::registry::Page;

use crate::i18n::tr;
use crate::model::Model;
use crate::widgets::{PageView, Placeholder, Sidebar};

/// The width of the sidebar, in pixels.
const SIDEBAR_WIDTH: i32 = 240;

mod imp {
    use std::cell::OnceCell;

    use gtk::glib;
    use gtk::subclass::prelude::*;

    use crate::model::Model;
    use crate::widgets::Sidebar;

    #[derive(Debug, Default)]
    pub struct Window {
        pub sidebar: Sidebar,
        pub pages: gtk::Stack,
        pub model: OnceCell<Model>,
        pub banner: gtk::Revealer,
        pub banner_text: gtk::Label,
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

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("view");
        content.set_hexpand(true);
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
            self.show_page(*first);
        }
    }

    /// Opens `page` and marks it in the sidebar.
    pub fn show_page(&self, page: Page) {
        let imp = self.imp();
        if imp.pages.child_by_name(page.id()).is_none() {
            imp.pages.add_named(&self.build_page(page), Some(page.id()));
        }
        imp.pages.set_visible_child_name(page.id());
        imp.sidebar.select(page);
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
