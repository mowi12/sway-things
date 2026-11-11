// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A container that limits the width of its child and centers it.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct Clamp {
        pub child: RefCell<Option<gtk::Widget>>,
        pub maximum_width: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Clamp {
        const NAME: &'static str = "StClamp";
        type Type = super::Clamp;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-clamp");
        }
    }

    impl ObjectImpl for Clamp {
        fn dispose(&self) {
            if let Some(child) = self.child.take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Clamp {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.child.borrow().clone() else {
                return (0, 0, -1, -1);
            };
            if orientation == gtk::Orientation::Horizontal {
                let (minimum, natural, _, _) = child.measure(orientation, for_size);
                (
                    minimum,
                    natural.min(self.maximum_width.get()).max(minimum),
                    -1,
                    -1,
                )
            } else {
                let width = if for_size < 0 {
                    for_size
                } else {
                    self.child_width(&child, for_size)
                };
                child.measure(orientation, width)
            }
        }

        #[expect(
            clippy::cast_precision_loss,
            reason = "pixel offsets are far below 2^24, where f32 starts to round"
        )]
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.child.borrow().as_ref() {
                let child_width = self.child_width(child, width);
                let x = (width - child_width) / 2;
                let transform =
                    gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(x as f32, 0.0));
                child.allocate(child_width, height, baseline, Some(transform));
            }
        }
    }

    impl Clamp {
        /// The width the child gets out of `available`: at most the maximum, at least its minimum.
        fn child_width(&self, child: &gtk::Widget, available: i32) -> i32 {
            let (minimum, _, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
            available.min(self.maximum_width.get()).max(minimum)
        }
    }
}

glib::wrapper! {
    /// Gives its child the available width up to a maximum and centers it, so long lines stay
    /// readable on a wide window.
    pub struct Clamp(ObjectSubclass<imp::Clamp>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Clamp {
    /// A clamp around `child` that is at most `maximum_width` pixels wide.
    pub fn new(child: &impl IsA<gtk::Widget>, maximum_width: i32) -> Self {
        let clamp: Self = glib::Object::new();
        clamp.imp().maximum_width.set(maximum_width);
        child.set_parent(&clamp);
        clamp.imp().child.replace(Some(child.clone().upcast()));
        clamp
    }
}
