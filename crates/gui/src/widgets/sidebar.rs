// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sidebar: the search field, and every page grouped under headings, or the results of a
//! search.

use gtk::glib;
use gtk::glib::closure_local;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::registry::{Hit, Link, Page, Registry, Setting};

use crate::i18n::tr;

/// The name of the list of pages in the sidebar's stack.
const PAGES: &str = "pages";

/// The name of the search results in the sidebar's stack.
const RESULTS: &str = "results";

mod imp {
    use std::cell::{OnceCell, RefCell};
    use std::sync::OnceLock;

    use gtk::glib;
    use gtk::glib::subclass::Signal;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::registry::{Link, Page, SearchIndex};

    use crate::i18n::tr;

    #[derive(Debug, Default)]
    pub struct Sidebar {
        pub layout: gtk::Box,
        pub entry: gtk::SearchEntry,
        pub stack: gtk::Stack,
        pub scroller: gtk::ScrolledWindow,
        pub list: gtk::ListBox,
        pub results_scroller: gtk::ScrolledWindow,
        pub results: gtk::ListBox,
        /// The texts of the pages and settings, built on the first search.
        pub index: OnceCell<SearchIndex<'static>>,
        /// The query the results are for.
        pub query: RefCell<String>,
        /// What the rows of the results open, in their order.
        pub hits: RefCell<Vec<Link<'static>>>,
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
            self.scroller.set_child(Some(&self.list));

            self.build_search();

            self.stack.add_named(&self.scroller, Some(super::PAGES));
            self.stack
                .add_named(&self.results_scroller, Some(super::RESULTS));
            // The rows ellipsize, and neither list asks for the width of its texts: the sidebar is
            // as wide with any results, or none, as with the pages.
            self.stack.set_vexpand(true);
            self.layout.set_orientation(gtk::Orientation::Vertical);
            self.layout.append(&self.entry);
            self.layout.append(&self.stack);
            self.layout.set_parent(&*sidebar);
        }

        fn dispose(&self) {
            self.layout.unparent();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    // A page was opened from the sidebar; the argument is its id.
                    Signal::builder("page-activated")
                        .param_types([String::static_type()])
                        .build(),
                    // A search result was opened; the argument is its link: a page id or the key
                    // of a setting.
                    Signal::builder("link-activated")
                        .param_types([String::static_type()])
                        .build(),
                ]
            })
        }
    }

    impl WidgetImpl for Sidebar {}

    impl Sidebar {
        /// The search field and the list of results that replaces the pages while searching.
        fn build_search(&self) {
            let sidebar = self.obj();
            self.entry.set_placeholder_text(Some("Search settings…"));
            self.entry
                .update_property(&[gtk::accessible::Property::Label("Search settings")]);
            self.entry.connect_search_changed(glib::clone!(
                #[weak]
                sidebar,
                move |entry| sidebar.show_results(&entry.text())
            ));
            // Esc clears the search.
            self.entry.connect_stop_search(|entry| entry.set_text(""));
            // Enter opens the best result.
            self.entry.connect_activate(glib::clone!(
                #[weak]
                sidebar,
                move |entry| {
                    sidebar.show_results(&entry.text());
                    sidebar.open_result(0);
                }
            ));
            // Down goes from the field to the results, or to the pages.
            let keys = gtk::EventControllerKey::new();
            keys.connect_key_pressed(glib::clone!(
                #[weak]
                sidebar,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, key, _, _| {
                    if key == gtk::gdk::Key::Down && sidebar.focus_list() {
                        glib::Propagation::Stop
                    } else {
                        glib::Propagation::Proceed
                    }
                }
            ));
            self.entry.add_controller(keys);

            self.results.add_css_class("navigation-sidebar");
            self.results.set_selection_mode(gtk::SelectionMode::Single);
            self.results.set_activate_on_single_click(true);
            self.results
                .update_property(&[gtk::accessible::Property::Label("Search results")]);
            let nothing = gtk::Label::builder()
                .label("No settings found")
                .wrap(true)
                .justify(gtk::Justification::Center)
                .css_classes(["st-no-results", crate::style::DIM])
                .build();
            self.results.set_placeholder(Some(&nothing));
            self.results.connect_row_activated(glib::clone!(
                #[weak]
                sidebar,
                move |_, row| sidebar.open_result(row.index())
            ));
            self.up_to_entry(&self.list);
            self.up_to_entry(&self.results);
            self.results_scroller
                .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
            self.results_scroller.set_child(Some(&self.results));
        }

        /// Up from the first row of `list` goes back to the search field.
        fn up_to_entry(&self, list: &gtk::ListBox) {
            let sidebar = self.obj();
            list.connect_keynav_failed(glib::clone!(
                #[weak]
                sidebar,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, direction| {
                    if direction == gtk::DirectionType::Up && sidebar.focus_search() {
                        glib::Propagation::Stop
                    } else {
                        glib::Propagation::Proceed
                    }
                }
            ));
        }

        /// The search index, built the first time it is needed.
        pub fn index(&self) -> &SearchIndex<'static> {
            self.index.get_or_init(|| {
                SearchIndex::new(sway_things_core::registry::Registry::builtin(), tr)
            })
        }
    }

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
    /// The search field above every page under its group heading. Arrow keys move through the
    /// pages, Enter or a click opens one. While the field has text, the results of the search
    /// replace the pages.
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

    /// Marks no page as open, while something else fills the content pane.
    pub fn unselect(&self) {
        self.imp().list.unselect_all();
    }

    /// The page marked as open.
    pub fn selected(&self) -> Option<Page> {
        let row = self.imp().list.selected_row()?;
        imp::page_at(row.index())
    }

    /// Moves the keyboard focus to the open page's row, or the first row. While searching, to
    /// the result opened last, or the first result, or the search field if nothing was found.
    pub fn focus_selected(&self) -> bool {
        self.focus_list() || self.focus_search()
    }

    /// Moves the keyboard focus to the open page's row, or the first row, or while searching, to
    /// the result opened last or the first result. Returns whether there was a row to focus.
    fn focus_list(&self) -> bool {
        let imp = self.imp();
        if self.is_searching() {
            let results = &imp.results;
            return results
                .selected_row()
                .or_else(|| results.row_at_index(0))
                .is_some_and(|row| row.grab_focus());
        }
        let list = &imp.list;
        list.selected_row()
            .or_else(|| list.row_at_index(0))
            .is_some_and(|row| row.grab_focus())
    }

    /// The list of pages, for keyboard tests.
    pub fn list(&self) -> &gtk::ListBox {
        &self.imp().list
    }

    /// The search field. Typing while the focus is elsewhere goes to it once it captures the
    /// keys of the window ([`gtk::SearchEntry::set_key_capture_widget`]).
    pub fn search_entry(&self) -> &gtk::SearchEntry {
        &self.imp().entry
    }

    /// The list of search results.
    pub fn results(&self) -> &gtk::ListBox {
        &self.imp().results
    }

    /// Whether the results of a search replace the pages.
    pub fn is_searching(&self) -> bool {
        self.imp().stack.visible_child_name().as_deref() == Some(RESULTS)
    }

    /// Moves the keyboard focus to the search field.
    pub fn focus_search(&self) -> bool {
        self.imp().entry.grab_focus()
    }

    /// Searches for `query` at once, as if it had been typed into the search field.
    pub fn search(&self, query: &str) {
        self.imp().entry.set_text(query);
        self.show_results(query);
    }

    /// Ends the search: clears the field, shows the pages again and gives the field the focus.
    /// Returns whether there was a search to end.
    pub fn stop_search(&self) -> bool {
        if !self.is_searching() {
            return false;
        }
        self.search("");
        self.focus_search();
        true
    }

    /// Shows the results for `query`, or the pages if it has no words.
    fn show_results(&self, query: &str) {
        let imp = self.imp();
        if *imp.query.borrow() == query {
            return;
        }
        imp.query.replace(query.to_owned());
        if query.trim().is_empty() {
            imp.stack.set_visible_child_name(PAGES);
            return;
        }
        let hits: Vec<Link<'static>> = imp.index().search(query).iter().map(Hit::link).collect();
        // Row by row: `remove_all` would remove the placeholder too.
        while let Some(row) = imp.results.row_at_index(0) {
            imp.results.remove(&row);
        }
        for link in &hits {
            imp.results.append(&result_row(link));
        }
        let count = hits.len();
        imp.hits.replace(hits);
        imp.stack.set_visible_child_name(RESULTS);
        imp.results_scroller.vadjustment().set_value(0.0);
        let announcement = match count {
            0 => "No settings found".to_owned(),
            1 => "1 result".to_owned(),
            n => format!("{n} results"),
        };
        imp.results
            .announce(&announcement, gtk::AccessibleAnnouncementPriority::Medium);
    }

    /// Opens the result at `index`, marks it, and emits `link-activated`.
    fn open_result(&self, index: i32) {
        let imp = self.imp();
        let link = usize::try_from(index)
            .ok()
            .and_then(|index| imp.hits.borrow().get(index).copied());
        let Some(link) = link else { return };
        imp.results
            .select_row(imp.results.row_at_index(index).as_ref());
        let text = link.setting().map_or(link.page().id(), Setting::key);
        self.emit_by_name::<()>("link-activated", &[&text]);
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

    /// Calls `callback` with the page or setting of the search result the user opened.
    pub fn connect_link_activated<F: Fn(&Self, Link<'static>) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "link-activated",
            false,
            closure_local!(move |sidebar: &Self, text: String| {
                if let Ok(link) = Link::parse(&text, Registry::builtin()) {
                    callback(sidebar, link);
                }
            }),
        )
    }
}

/// The row of a search result: the title of the setting or page, and under it where it is
/// ("Mouse & Touchpad › Touchpad" for a setting, the sidebar group for a page).
fn result_row(link: &Link) -> gtk::ListBoxRow {
    let page = link.page();
    let (title, place) = match link.setting() {
        Some(setting) => {
            let (page_title, section) = (tr(page.title()), tr(setting.section()));
            let place = if section.is_empty() || section == page_title {
                page_title.to_owned()
            } else {
                format!("{page_title} › {section}")
            };
            (tr(setting.title()), place)
        }
        None => (tr(page.title()), tr(page.group().title()).to_owned()),
    };
    let title = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let place = gtk::Label::builder()
        .label(place)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["st-result-place", crate::style::DIM])
        .build();
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.append(&title);
    labels.append(&place);
    gtk::ListBoxRow::builder().child(&labels).build()
}
