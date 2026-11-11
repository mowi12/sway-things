// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sidebar: every page, grouped under headings.

use gtk::glib;
use gtk::glib::closure_local;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::registry::Page;

mod imp {
    use std::sync::OnceLock;

    use gtk::glib;
    use gtk::glib::subclass::Signal;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::registry::Page;

    use crate::i18n::tr;

    #[derive(Debug, Default)]
    pub struct Sidebar {
        pub scroller: gtk::ScrolledWindow,
        pub list: gtk::ListBox,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Sidebar {
        const NAME: &'static str = "StSidebar";
        type Type = super::Sidebar;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-sidebar");
            klass.set_layout_manager_type::<gtk::BinLayout>();
            klass.set_accessible_role(gtk::AccessibleRole::Navigation);
        }
    }

    impl ObjectImpl for Sidebar {
        fn constructed(&self) {
            self.parent_constructed();
            let sidebar = self.obj();
            sidebar.update_property(&[gtk::accessible::Property::Label("Settings pages")]);

            self.list.add_css_class("navigation-sidebar");
            // Arrow keys move the focus, Enter or a click opens the page.
            self.list.set_selection_mode(gtk::SelectionMode::Single);
            self.list.set_activate_on_single_click(true);
            self.list
                .update_property(&[gtk::accessible::Property::Label("Settings pages")]);
            for page in Page::ALL {
                self.list.append(&row(page));
            }
            self.list.set_header_func(|row, _| {
                row.set_header(group_heading(row.index()).as_ref());
            });
            self.list.connect_row_activated(glib::clone!(
                #[weak]
                sidebar,
                move |list, row| {
                    list.select_row(Some(row));
                    if let Some(page) = page_at(row.index()) {
                        sidebar.emit_by_name::<()>("page-activated", &[&page.id()]);
                    }
                }
            ));

            self.scroller
                .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
            self.scroller.set_propagate_natural_width(true);
            self.scroller.set_child(Some(&self.list));
            self.scroller.set_parent(&*sidebar);
        }

        fn dispose(&self) {
            self.scroller.unparent();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    // A page was opened from the sidebar; the argument is its id.
                    Signal::builder("page-activated")
                        .param_types([String::static_type()])
                        .build(),
                ]
            })
        }
    }

    impl WidgetImpl for Sidebar {}

    /// The page of the row at `index`: the rows follow [`Page::ALL`].
    pub fn page_at(index: i32) -> Option<Page> {
        usize::try_from(index)
            .ok()
            .and_then(|index| Page::ALL.get(index).copied())
    }

    fn row(page: Page) -> gtk::ListBoxRow {
        let label = gtk::Label::builder()
            .label(tr(page.title()))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        gtk::ListBoxRow::builder().child(&label).build()
    }

    /// The group heading above the row at `index`, if it is the first page of its group.
    fn group_heading(index: i32) -> Option<gtk::Widget> {
        let page = page_at(index)?;
        let previous = index.checked_sub(1).and_then(page_at);
        if previous.is_some_and(|previous| previous.group() == page.group()) {
            return None;
        }
        let title = tr(page.group().title());
        let heading = gtk::Label::builder()
            .label(title.to_uppercase())
            .xalign(0.0)
            .accessible_role(gtk::AccessibleRole::Heading)
            .css_classes(["st-group-heading", crate::style::DIM])
            .build();
        // Screen readers get the heading in normal case, not spelled out letter by letter.
        heading.update_property(&[gtk::accessible::Property::Label(title)]);
        if previous.is_none() {
            heading.add_css_class("st-first");
        }
        Some(heading.upcast())
    }
}

glib::wrapper! {
    /// Lists every page under its group heading. Arrow keys move through the pages, Enter or a
    /// click opens one.
    pub struct Sidebar(ObjectSubclass<imp::Sidebar>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Sidebar {
    fn default() -> Self {
        Self::new()
    }
}

impl Sidebar {
    /// A sidebar with every page.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Marks `page` as the open one, without emitting `page-activated`.
    pub fn select(&self, page: Page) {
        let list = &self.imp().list;
        let row = Page::ALL
            .iter()
            .position(|p| *p == page)
            .and_then(|index| i32::try_from(index).ok())
            .and_then(|index| list.row_at_index(index));
        list.select_row(row.as_ref());
    }

    /// The page marked as open.
    pub fn selected(&self) -> Option<Page> {
        let row = self.imp().list.selected_row()?;
        imp::page_at(row.index())
    }

    /// Moves the keyboard focus to the open page's row, or the first row.
    pub fn focus_selected(&self) -> bool {
        let list = &self.imp().list;
        list.selected_row()
            .or_else(|| list.row_at_index(0))
            .is_some_and(|row| row.grab_focus())
    }

    /// The list of pages, for keyboard tests.
    pub fn list(&self) -> &gtk::ListBox {
        &self.imp().list
    }

    /// Calls `callback` with the page the user opened.
    pub fn connect_page_activated<F: Fn(&Self, Page) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "page-activated",
            false,
            closure_local!(move |sidebar: &Self, id: String| {
                if let Some(page) = Page::from_id(&id) {
                    callback(sidebar, page);
                }
            }),
        )
    }
}
