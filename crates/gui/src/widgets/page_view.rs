// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A page in the content pane: header and body, scrollable, at a readable width.

use std::collections::BTreeSet;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::registry::{Msg, Page, Registry};

use super::setting_row::tilde;
use super::{Clamp, PageHeader, Placeholder, SettingRow};
use crate::i18n::tr;
use crate::model::Model;
use crate::style;

/// The widest a page's content gets, in pixels.
pub const MAXIMUM_WIDTH: i32 = 760;

mod imp {
    use std::cell::OnceCell;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct PageView {
        pub scroller: gtk::ScrolledWindow,
        pub content: gtk::Box,
        pub header: OnceCell<super::PageHeader>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PageView {
        const NAME: &'static str = "StPageView";
        type Type = super::PageView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-page");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for PageView {
        fn constructed(&self) {
            self.parent_constructed();
            self.content.set_orientation(gtk::Orientation::Vertical);
            self.content.add_css_class("st-page-content");
            let clamp = super::Clamp::new(&self.content, super::MAXIMUM_WIDTH);
            self.scroller
                .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
            self.scroller.set_child(Some(&clamp));
            self.scroller.set_parent(&*self.obj());
        }

        fn dispose(&self) {
            self.scroller.unparent();
        }
    }

    impl WidgetImpl for PageView {}
}

glib::wrapper! {
    /// A page of the content pane: its header and its body, scrolled vertically and at most
    /// [`MAXIMUM_WIDTH`] wide.
    pub struct PageView(ObjectSubclass<imp::PageView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PageView {
    /// A page with a header from `title` and `description`, and `body` below it.
    pub fn new(title: &str, description: &str, body: &impl IsA<gtk::Widget>) -> Self {
        let view: Self = glib::Object::new();
        let imp = view.imp();
        let header = PageHeader::new(title, description);
        imp.content.append(&header);
        imp.content.append(body);
        imp.header.get_or_init(|| header);
        view.update_property(&[gtk::accessible::Property::Label(title)]);
        view
    }

    /// The page view of `page` while it isn't built yet: its header and a placeholder.
    pub fn unbuilt(page: Page) -> Self {
        Self::new(
            tr(page.title()),
            tr(page.description()),
            &Placeholder::unbuilt(),
        )
    }

    /// The page view of `page` built from its settings in the registry: a section for each of
    /// their sections, with a row per setting, and the generated files the page writes to below.
    /// `device` selects one input device for the per-device settings. `None` if the page has no
    /// settings with a row.
    pub fn for_settings(model: &Model, page: Page, device: Option<&str>) -> Option<Self> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let mut section: Option<(Msg, gtk::ListBox)> = None;
        let mut files = BTreeSet::new();
        for setting in Registry::builtin()
            .iter()
            .filter(|setting| setting.page() == page)
        {
            let Some(row) = SettingRow::new(model, setting, device) else {
                continue;
            };
            if let Some(mapping) = setting.sway() {
                files.insert(mapping.file());
            }
            let list = match &section {
                Some((title, list)) if *title == setting.section() => list.clone(),
                _ => {
                    let list = section_list(&body, setting.section(), section.is_none());
                    section = Some((setting.section(), list.clone()));
                    list
                }
            };
            let item = gtk::ListBoxRow::new();
            item.set_activatable(false);
            item.set_focusable(false);
            item.set_child(Some(&row));
            list.append(&item);
        }
        section.as_ref()?;
        if let (Some(locations), false) = (model.locations(), files.is_empty()) {
            let paths: Vec<String> = files
                .into_iter()
                .map(|file| tilde(&locations.path(file)))
                .collect();
            let footer = gtk::Label::new(Some(&paths.join("\n")));
            footer.set_xalign(0.0);
            footer.set_wrap(true);
            footer.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            footer.set_selectable(true);
            // Selectable with the mouse, but not a stop for the keyboard, which would select it all.
            footer.set_focusable(false);
            footer.add_css_class("monospace");
            footer.add_css_class("st-page-footer");
            footer.add_css_class(style::DIM);
            footer.update_property(&[gtk::accessible::Property::Label(&format!(
                "Written to {}",
                paths.join(", ")
            ))]);
            body.append(&footer);
        }
        Some(Self::new(tr(page.title()), tr(page.description()), &body))
    }

    /// The header of the page.
    pub fn header(&self) -> Option<&PageHeader> {
        self.imp().header.get()
    }
}

/// Adds the heading of a section titled `title` to `body`, and the list for its rows.
fn section_list(body: &gtk::Box, title: Msg, first: bool) -> gtk::ListBox {
    let heading = gtk::Label::builder()
        .label(tr(title).to_uppercase())
        .xalign(0.0)
        .accessible_role(gtk::AccessibleRole::Heading)
        .build();
    heading.update_property(&[gtk::accessible::Property::Label(tr(title))]);
    heading.add_css_class("st-section-heading");
    heading.add_css_class(style::DIM);
    if first {
        heading.add_css_class("st-first");
    }
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("st-section");
    list.update_property(&[gtk::accessible::Property::Label(tr(title))]);
    body.append(&heading);
    body.append(&list);
    list
}
