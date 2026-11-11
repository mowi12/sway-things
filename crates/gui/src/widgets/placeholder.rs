// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The body of a page that isn't built yet.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use crate::style;

mod imp {
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug)]
    pub struct Placeholder {
        pub icon: gtk::Image,
        pub title: gtk::Label,
        pub text: gtk::Label,
    }

    impl Default for Placeholder {
        fn default() -> Self {
            Self {
                // Decorative: the title says it all.
                icon: gtk::Image::builder()
                    .accessible_role(gtk::AccessibleRole::Presentation)
                    .build(),
                title: gtk::Label::builder()
                    .accessible_role(gtk::AccessibleRole::Heading)
                    .build(),
                text: gtk::Label::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Placeholder {
        const NAME: &'static str = "StPlaceholder";
        type Type = super::Placeholder;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-placeholder");
            klass.set_layout_manager_type::<gtk::BoxLayout>();
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }
    }

    impl ObjectImpl for Placeholder {
        fn constructed(&self) {
            self.parent_constructed();
            let placeholder = self.obj();
            if let Some(layout) = placeholder
                .layout_manager()
                .and_downcast::<gtk::BoxLayout>()
            {
                layout.set_orientation(gtk::Orientation::Vertical);
                layout.set_spacing(6);
            }
            self.icon.set_parent(&*placeholder);
            self.title.set_parent(&*placeholder);
            self.text.set_parent(&*placeholder);
        }

        fn dispose(&self) {
            self.icon.unparent();
            self.title.unparent();
            self.text.unparent();
        }
    }

    impl WidgetImpl for Placeholder {}
}

glib::wrapper! {
    /// A centered icon, title and explanation in place of a page's content.
    pub struct Placeholder(ObjectSubclass<imp::Placeholder>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Placeholder {
    /// A placeholder with an icon, `title` and an explanation.
    pub fn new(icon_name: &str, title: &str, text: &str) -> Self {
        let placeholder: Self = glib::Object::new();
        placeholder.set_valign(gtk::Align::Start);
        let imp = placeholder.imp();
        imp.icon.set_icon_name(Some(icon_name));
        imp.icon.set_pixel_size(64);
        imp.icon.add_css_class(style::DIM);
        imp.title.set_text(title);
        imp.title.add_css_class("title-2");
        imp.text.set_text(text);
        imp.text.add_css_class(style::DIM);
        for label in [&imp.title, &imp.text] {
            label.set_justify(gtk::Justification::Center);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        }
        placeholder
    }

    /// The placeholder for a page that isn't built yet.
    pub fn unbuilt() -> Self {
        Self::new(
            "emblem-system-symbolic",
            "Not built yet",
            "This page is planned for a later version of Sway Things.",
        )
    }
}
