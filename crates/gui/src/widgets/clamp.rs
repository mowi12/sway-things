// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A container that limits the width of its child and centers it, with side margins that shrink
//! on a narrow window.

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
        /// The smallest and the largest margin on each side, in pixels.
        pub margins: Cell<(i32, i32)>,
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
                let (smallest, largest) = self.margins.get();
                (
                    minimum + 2 * smallest,
                    natural.min(self.maximum_width.get()).max(minimum) + 2 * largest,
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
                let x = ((width - child_width) / 2).max(0);
                let transform =
                    gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(x as f32, 0.0));
                child.allocate(child_width, height, baseline, Some(transform));
            }
        }
    }

    impl Clamp {
        /// The width the child gets out of `available`, without the margins: at most the
        /// maximum, at least its minimum.
        fn child_width(&self, child: &gtk::Widget, available: i32) -> i32 {
            let (minimum, _, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
            (available - 2 * self.margin(available))
                .min(self.maximum_width.get())
                .max(minimum)
        }

        /// The margin on each side in `available`: a share of it, between the smallest and the
        /// largest margin.
        pub(super) fn margin(&self, available: i32) -> i32 {
            let (smallest, largest) = self.margins.get();
            (available / super::MARGIN_SHARE).clamp(smallest, largest.max(smallest))
        }
    }
}

/// The margins are a 1/17 of the width, about 6 %: the largest margin on a window as wide as a
/// third of a common laptop screen, less on a narrower one.
const MARGIN_SHARE: i32 = 17;

glib::wrapper! {
    /// Gives its child the available width up to a maximum and centers it, so long lines stay
    /// readable on a wide window.
    pub struct Clamp(ObjectSubclass<imp::Clamp>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Clamp {
    /// Whether the child fits in `width` with the margins it gets there, without scrolling
    /// sideways.
    pub fn fits(&self, width: i32) -> bool {
        let imp = self.imp();
        imp.child.borrow().as_ref().is_none_or(|child| {
            let (minimum, _, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
            minimum + 2 * imp.margin(width) <= width
        })
    }

    /// The narrowest the clamp can be to give its child `child_width` pixels, with the margins it
    /// gets there.
    pub fn width_for(&self, child_width: i32) -> i32 {
        let imp = self.imp();
        let (smallest, largest) = imp.margins.get();
        (child_width + 2 * smallest..=child_width + 2 * largest.max(smallest))
            .find(|&width| width - 2 * imp.margin(width) >= child_width)
            .unwrap_or(child_width + 2 * largest.max(smallest))
    }

    /// A clamp around `child` that is at most `maximum_width` pixels wide.
    pub fn new(child: &impl IsA<gtk::Widget>, maximum_width: i32) -> Self {
        Self::with_margins(child, maximum_width, (0, 0))
    }

    /// A clamp around `child` that is at most `maximum_width` pixels wide, with a margin on each
    /// side from `margins.0` on a narrow window to `margins.1` on a wide one.
    pub fn with_margins(
        child: &impl IsA<gtk::Widget>,
        maximum_width: i32,
        margins: (i32, i32),
    ) -> Self {
        let clamp: Self = glib::Object::new();
        clamp.imp().maximum_width.set(maximum_width);
        clamp.imp().margins.set(margins);
        child.set_parent(&clamp);
        clamp.imp().child.replace(Some(child.clone().upcast()));
        clamp
    }
}
