// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The header at the top of a page: title and one-line description.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use crate::style;

mod imp {
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug)]
    pub struct PageHeader {
        pub title: gtk::Label,
        pub description: gtk::Label,
    }

    impl Default for PageHeader {
        fn default() -> Self {
            Self {
                // The role is fixed at construction.
                title: gtk::Label::builder()
                    .accessible_role(gtk::AccessibleRole::Heading)
                    .build(),
                description: gtk::Label::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PageHeader {
        const NAME: &'static str = "StPageHeader";
        type Type = super::PageHeader;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-page-header");
            klass.set_layout_manager_type::<gtk::BoxLayout>();
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }
    }

    impl ObjectImpl for PageHeader {
        fn constructed(&self) {
            self.parent_constructed();
            let header = self.obj();
            if let Some(layout) = header.layout_manager().and_downcast::<gtk::BoxLayout>() {
                layout.set_orientation(gtk::Orientation::Vertical);
                layout.set_spacing(4);
            }
            self.title.set_parent(&*header);
            self.description.set_parent(&*header);
        }

        fn dispose(&self) {
            self.title.unparent();
            self.description.unparent();
        }
    }

    impl WidgetImpl for PageHeader {}
}

glib::wrapper! {
    /// The title of a page and its one-line description.
    pub struct PageHeader(ObjectSubclass<imp::PageHeader>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PageHeader {
    /// A header with `title` and `description`.
    pub fn new(title: &str, description: &str) -> Self {
        let header: Self = glib::Object::new();
        let imp = header.imp();
        imp.title.set_text(title);
        imp.title.add_css_class("title-1");
        imp.description.set_text(description);
        imp.description.add_css_class(style::DIM);
        for label in [&imp.title, &imp.description] {
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        }
        header
    }

    /// The title.
    pub fn title(&self) -> glib::GString {
        self.imp().title.text()
    }

    /// The description.
    pub fn description(&self) -> glib::GString {
        self.imp().description.text()
    }
}
