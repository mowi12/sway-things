// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A horizontal line below a bar, above a scrolled list or page: hidden at the top, it grows from
//! the center as the content scrolls, and spans the full width from [`FULL_OFFSET`] on.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

/// The scroll offset in pixels from which the line spans the full width.
pub const FULL_OFFSET: f64 = 50.0;

/// The share of the full width the line takes at the scroll offset `value`, from 0 to 1.
pub fn fraction(value: f64) -> f64 {
    (value / FULL_OFFSET).clamp(0.0, 1.0)
}

mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug)]
    pub struct ScrollLine {
        pub line: gtk::Separator,
        /// The share of the width the line takes.
        pub fraction: Cell<f64>,
        /// The adjustment followed, and its handlers.
        pub followed: RefCell<Option<(gtk::Adjustment, Vec<glib::SignalHandlerId>)>>,
    }

    impl Default for ScrollLine {
        fn default() -> Self {
            Self {
                line: gtk::Separator::new(gtk::Orientation::Horizontal),
                fraction: Cell::new(0.0),
                followed: RefCell::new(None),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ScrollLine {
        const NAME: &'static str = "StScrollLine";
        type Type = super::ScrollLine;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-scroll-line");
            klass.set_accessible_role(gtk::AccessibleRole::Presentation);
        }
    }

    impl ObjectImpl for ScrollLine {
        fn constructed(&self) {
            self.parent_constructed();
            self.line.set_parent(&*self.obj());
        }

        fn dispose(&self) {
            self.obj().unfollow();
            self.line.unparent();
        }
    }

    impl WidgetImpl for ScrollLine {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Horizontal {
                // Any width: the line takes a share of what it gets.
                (0, 0, -1, -1)
            } else {
                // Always as tall as the line, so nothing below it moves when it shows.
                let (minimum, natural, _, _) = self.line.measure(orientation, for_size);
                (minimum, natural, -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let shown = super::shown_width(width, self.fraction.get());
            self.line.set_child_visible(shown > 0);
            let left = (width - shown) / 2;
            #[expect(
                clippy::cast_precision_loss,
                reason = "widget widths are far below 2^24, where f32 starts to round"
            )]
            let left = left as f32;
            let at = gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(left, 0.0));
            self.line.allocate(shown, height, baseline, Some(at));
        }
    }
}

glib::wrapper! {
    /// A horizontal line that grows from the center as the content below it scrolls.
    pub struct ScrollLine(ObjectSubclass<imp::ScrollLine>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for ScrollLine {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl ScrollLine {
    /// A line, hidden until it follows a scrolled window.
    pub fn new() -> Self {
        Self::default()
    }

    /// The share of the full width the line takes, from 0 to 1.
    pub fn fraction(&self) -> f64 {
        self.imp().fraction.get()
    }

    /// The width of the line itself.
    pub fn line_width(&self) -> i32 {
        if self.imp().line.is_child_visible() {
            self.imp().line.width()
        } else {
            0
        }
    }

    /// Follows the vertical adjustment of `scroller` instead of the one before; without one,
    /// the line hides.
    pub fn follow(&self, scroller: Option<&gtk::ScrolledWindow>) {
        self.unfollow();
        let Some(adjustment) = scroller.map(gtk::ScrolledWindow::vadjustment) else {
            self.set_fraction(0.0);
            return;
        };
        let update = glib::clone!(
            #[weak(rename_to = line)]
            self,
            move |adjustment: &gtk::Adjustment| line.set_fraction(fraction(adjustment.value()))
        );
        // `changed` covers new bounds or a new page size, as when the content resizes.
        let handlers = vec![
            adjustment.connect_value_changed(update.clone()),
            adjustment.connect_changed(update.clone()),
        ];
        update(&adjustment);
        self.imp().followed.replace(Some((adjustment, handlers)));
    }

    /// Stops following the adjustment.
    fn unfollow(&self) {
        if let Some((adjustment, handlers)) = self.imp().followed.take() {
            for handler in handlers {
                adjustment.disconnect(handler);
            }
        }
    }

    fn set_fraction(&self, fraction: f64) {
        let imp = self.imp();
        if (imp.fraction.get() - fraction).abs() > f64::EPSILON {
            imp.fraction.set(fraction);
            self.queue_allocate();
        }
    }
}

/// The width of the line out of `width` at `fraction`.
fn shown_width(width: i32, fraction: f64) -> i32 {
    let shown = (f64::from(width) * fraction.clamp(0.0, 1.0)).round();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the product stays within 0..=width, which fits an i32"
    )]
    let shown = shown as i32;
    shown.clamp(0, width.max(0))
}

/// The first scrolled window in `widget`, itself included, depth first.
pub fn scroller_in(widget: &gtk::Widget) -> Option<gtk::ScrolledWindow> {
    if let Some(scroller) = widget.downcast_ref::<gtk::ScrolledWindow>() {
        return Some(scroller.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(scroller) = scroller_in(&current) {
            return Some(scroller);
        }
        child = current.next_sibling();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{FULL_OFFSET, fraction, shown_width};

    #[test]
    fn grows_with_the_offset() {
        assert!(fraction(0.0).abs() < f64::EPSILON);
        assert!((fraction(FULL_OFFSET / 2.0) - 0.5).abs() < f64::EPSILON);
        assert!((fraction(FULL_OFFSET) - 1.0).abs() < f64::EPSILON);
        assert!((fraction(FULL_OFFSET * 4.0) - 1.0).abs() < f64::EPSILON);
        assert!(fraction(-10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn takes_a_share_of_the_width() {
        assert_eq!(shown_width(200, 0.0), 0);
        assert_eq!(shown_width(200, 0.5), 100);
        assert_eq!(shown_width(200, 1.0), 200);
        assert_eq!(shown_width(0, 1.0), 0);
    }
}
