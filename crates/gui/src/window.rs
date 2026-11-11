// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The main window: the sidebar and the content pane.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::registry::Page;

use crate::widgets::{PageView, Sidebar};

/// The width of the sidebar, in pixels.
const SIDEBAR_WIDTH: i32 = 240;

mod imp {
    use gtk::glib;
    use gtk::subclass::prelude::*;

    use crate::widgets::Sidebar;

    #[derive(Debug, Default)]
    pub struct Window {
        pub sidebar: Sidebar,
        pub pages: gtk::Stack,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "StWindow";
        type Type = super::Window;
        type ParentType = gtk::ApplicationWindow;
    }

    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().build();
        }
    }

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
    /// The main window of `app`, showing the first page.
    pub fn new(app: &gtk::Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    fn build(&self) {
        self.set_title(Some("Sway Things"));
        self.set_default_size(980, 700);

        let imp = self.imp();
        imp.sidebar.set_size_request(SIDEBAR_WIDTH, -1);
        imp.sidebar.connect_page_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, page| window.show_page(page)
        ));

        imp.pages.set_hexpand(true);
        imp.pages.add_css_class("view");

        let panes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        panes.append(&imp.sidebar);
        panes.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        panes.append(&imp.pages);
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
            imp.pages
                .add_named(&PageView::unbuilt(page), Some(page.id()));
        }
        imp.pages.set_visible_child_name(page.id());
        imp.sidebar.select(page);
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
