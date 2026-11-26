// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The layout of a row: its text, and its controls at the end.
//!
//! The controls sit beside the text while the text keeps a readable width next to them; the
//! controls give up the room they would like beyond their minimum first (a slider shrinks). On a
//! narrower row they move below the text, aligned to the end, so the text never gets squeezed
//! into a few letters per line.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

/// The space between the text and the controls beside it, in pixels.
const SPACING: i32 = 24;

/// The space between the text and the controls below it, in pixels.
const STACKED_SPACING: i32 = 8;

/// The width the text keeps beside the controls, in pixels, unless it needs less: below it, the
/// controls move below the text.
pub const TEXT_WIDTH: i32 = 160;

mod imp {
    use std::cell::Cell;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct RowLayout {
        /// How much wider than their minimum the controls would like to be, in pixels.
        pub(super) growth: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RowLayout {
        const NAME: &'static str = "StRowLayout";
        type Type = super::RowLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for RowLayout {}

    impl LayoutManagerImpl for RowLayout {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(
            &self,
            widget: &gtk::Widget,
            orientation: gtk::Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let Some(parts) = super::Parts::of(widget, self.growth.get()) else {
                return (0, 0, -1, -1);
            };
            if orientation == gtk::Orientation::Horizontal {
                let (text_minimum, text_natural) = parts.text_widths();
                let (controls_minimum, controls_natural) = parts.controls_widths();
                (
                    text_minimum.max(controls_minimum),
                    text_natural + super::SPACING + controls_natural,
                    -1,
                    -1,
                )
            } else {
                let width = if for_size < 0 {
                    parts.natural_width()
                } else {
                    for_size
                };
                let height = parts.place(width).height;
                (height, height, -1, -1)
            }
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, _baseline: i32) {
            let Some(parts) = super::Parts::of(widget, self.growth.get()) else {
                return;
            };
            let place = parts.place(width);
            let rtl = widget.direction() == gtk::TextDirection::Rtl;
            let at = |x: i32, w: i32, y: i32| {
                let x = if rtl { width - x - w } else { x };
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "pixel offsets are far below 2^24, where f32 starts to round"
                )]
                let point = gtk::graphene::Point::new(x as f32, y as f32);
                Some(gtk::gsk::Transform::new().translate(&point))
            };
            if place.stacked {
                parts
                    .text
                    .allocate(width, place.text_height, -1, at(0, width, 0));
                parts.controls.allocate(
                    place.controls_width,
                    height - place.text_height - super::STACKED_SPACING,
                    -1,
                    at(
                        width - place.controls_width,
                        place.controls_width,
                        place.text_height + super::STACKED_SPACING,
                    ),
                );
            } else {
                let text_width = width - super::SPACING - place.controls_width;
                parts
                    .text
                    .allocate(text_width, height, -1, at(0, text_width, 0));
                parts.controls.allocate(
                    place.controls_width,
                    height,
                    -1,
                    at(width - place.controls_width, place.controls_width, 0),
                );
            }
        }
    }
}

glib::wrapper! {
    /// Lays out the first child of a widget, its text, and the second, its controls: beside each
    /// other while the text keeps [`TEXT_WIDTH`], else the controls below the text, at the end.
    pub struct RowLayout(ObjectSubclass<imp::RowLayout>)
        @extends gtk::LayoutManager;
}

impl Default for RowLayout {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl RowLayout {
    /// Lets the controls grow `growth` pixels beyond their minimum width when there is room, e.g.
    /// for a slider that can shrink down to its minimum on a narrow row.
    pub fn set_growth(&self, growth: i32) {
        self.imp().growth.set(growth);
        self.layout_changed();
    }

    /// The narrowest `widget` can be with its controls beside its text, in pixels.
    pub fn beside_width(&self, widget: &impl IsA<gtk::Widget>) -> i32 {
        Parts::of(widget.upcast_ref(), self.imp().growth.get())
            .map_or(0, |parts| parts.beside_width())
    }
}

/// The text and the controls of a row.
struct Parts {
    text: gtk::Widget,
    controls: gtk::Widget,
    growth: i32,
}

/// Where the parts go in a row of a given width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Place {
    /// Whether the controls are below the text.
    stacked: bool,
    controls_width: i32,
    text_height: i32,
    /// The height of the row.
    height: i32,
}

impl Parts {
    fn of(widget: &gtk::Widget, growth: i32) -> Option<Self> {
        let text = widget.first_child()?;
        let controls = text.next_sibling()?;
        Some(Self {
            text,
            controls,
            growth,
        })
    }

    fn text_widths(&self) -> (i32, i32) {
        let (minimum, natural, _, _) = self.text.measure(gtk::Orientation::Horizontal, -1);
        (minimum, natural)
    }

    fn controls_widths(&self) -> (i32, i32) {
        let (minimum, natural, _, _) = self.controls.measure(gtk::Orientation::Horizontal, -1);
        (minimum, natural.max(minimum + self.growth))
    }

    /// The width the text keeps beside the controls: [`TEXT_WIDTH`], or less if it needs less,
    /// but never less than its minimum (a stack switcher can't wrap).
    fn text_beside(&self) -> i32 {
        let (text_minimum, text_natural) = self.text_widths();
        text_natural.min(TEXT_WIDTH).max(text_minimum)
    }

    /// The narrowest the row can be with the controls beside the text.
    fn beside_width(&self) -> i32 {
        let (controls_minimum, _) = self.controls_widths();
        self.text_beside() + SPACING + controls_minimum
    }

    fn natural_width(&self) -> i32 {
        let (_, text) = self.text_widths();
        let (_, controls) = self.controls_widths();
        text + SPACING + controls
    }

    fn height(widget: &gtk::Widget, width: i32) -> i32 {
        widget.measure(gtk::Orientation::Vertical, width).1
    }

    fn place(&self, width: i32) -> Place {
        let (controls_minimum, controls_natural) = self.controls_widths();
        if width >= self.beside_width() {
            let beside = width - SPACING - self.text_beside();
            let controls_width = controls_natural.min(beside).max(controls_minimum);
            let text_height = Self::height(&self.text, width - SPACING - controls_width);
            let height = text_height.max(Self::height(&self.controls, controls_width));
            Place {
                stacked: false,
                controls_width,
                text_height,
                height,
            }
        } else {
            let controls_width = controls_natural.min(width).max(controls_minimum);
            let text_height = Self::height(&self.text, width);
            let height =
                text_height + STACKED_SPACING + Self::height(&self.controls, controls_width);
            Place {
                stacked: true,
                controls_width,
                text_height,
                height,
            }
        }
    }
}
