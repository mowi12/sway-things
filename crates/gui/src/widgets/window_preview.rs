// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The live preview of the Windows page: a focused and an unfocused window side by side, drawn
//! with the current colors, border, gaps and title bar, and on SwayFX with rounded corners,
//! shadows and dimming. It follows a control while it moves: it reads [`Model::current`], which
//! includes the changes on their way.
//!
//! Where everything goes comes from [`sway_things_core::preview`]; this widget measures the
//! titles in the title bar font (at sway's 96 dpi) and paints the rectangles. The surface behind
//! the windows and the contents of the windows take the theme's colors, so the preview fits
//! light and dark; the decorations are sway's colors, as the user set them.

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};
use sway_things_core::command::Color;
use sway_things_core::preview::{Layout, Look, Rect, TITLES, TextSize, Window};

use crate::model::Model;

/// The height of the preview, in pixels.
pub const HEIGHT: i32 = 150;

/// The distance of the "Not to scale" note from the corner, in pixels.
const NOTE_MARGIN: i32 = 6;

mod imp {
    use std::cell::RefCell;

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{glib, graphene};
    use sway_things_core::preview::{Layout, Look};

    #[derive(Debug)]
    pub struct WindowPreview {
        pub(super) look: RefCell<Look>,
        pub(super) layout: RefCell<Option<Layout>>,
        /// The contents of the focused and the unfocused window, colored by the theme.
        pub(super) contents: [gtk::Box; 2],
        pub(super) note: gtk::Label,
    }

    impl Default for WindowPreview {
        fn default() -> Self {
            Self {
                look: RefCell::new(Look::defaults(false)),
                layout: RefCell::new(None),
                contents: [gtk::Box::default(), gtk::Box::default()],
                note: gtk::Label::new(Some("Not to scale")),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WindowPreview {
        const NAME: &'static str = "StWindowPreview";
        type Type = super::WindowPreview;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-window-preview");
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for WindowPreview {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_overflow(gtk::Overflow::Hidden);
            obj.set_hexpand(true);
            for content in &self.contents {
                content.add_css_class("st-preview-content");
                content.set_parent(&*obj);
            }
            self.note.add_css_class("st-preview-note");
            self.note.set_visible(false);
            self.note.set_parent(&*obj);
        }

        fn dispose(&self) {
            for content in &self.contents {
                content.unparent();
            }
            self.note.unparent();
        }
    }

    impl WidgetImpl for WindowPreview {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Vertical => (super::HEIGHT, super::HEIGHT, -1, -1),
                _ => (120, 480, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let obj = self.obj();
            let layout = obj.place(width, height);
            for (content, window) in self.contents.iter().zip(&layout.windows) {
                let (x, y, width, height) = super::pixels(&window.content);
                content.set_child_visible(width > 0 && height > 0);
                content.allocate(width, height, -1, Some(super::at(x, y)));
            }
            self.note.set_visible(layout.not_to_scale);
            let (_, natural, _, _) = self.note.measure(gtk::Orientation::Horizontal, -1);
            let (_, note_height, _, _) = self.note.measure(gtk::Orientation::Vertical, natural);
            self.note.allocate(
                natural,
                note_height,
                -1,
                Some(super::at(
                    width - natural - super::NOTE_MARGIN,
                    height - note_height - super::NOTE_MARGIN,
                )),
            );
            self.layout.replace(Some(layout));
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            #[expect(
                clippy::cast_precision_loss,
                reason = "widget sizes are far below 2^24, where f32 starts to round"
            )]
            let bounds = graphene::Rect::new(0.0, 0.0, obj.width() as f32, obj.height() as f32);
            snapshot.push_clip(&bounds);
            if let Some(layout) = self.layout.borrow().as_ref() {
                obj.paint(snapshot, layout);
            }
            obj.snapshot_child(&self.note, snapshot);
            snapshot.pop();
        }
    }
}

glib::wrapper! {
    /// A focused and an unfocused window as sway draws them with the current settings.
    pub struct WindowPreview(ObjectSubclass<imp::WindowPreview>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl WindowPreview {
    /// A preview that follows the settings of `model`.
    pub fn new(model: &Model) -> Self {
        let preview: Self = glib::Object::new();
        preview.update_property(&[gtk::accessible::Property::Label(
            "Preview of a focused and an unfocused window",
        )]);
        let update = glib::clone!(
            #[weak]
            preview,
            move |model: &Model| {
                preview.set_look(Look::read(
                    |setting| model.current(setting),
                    model.is_swayfx(),
                ));
            }
        );
        update(model);
        model.connect_changed(update.clone());
        model.connect_queued(update);
        preview
    }

    /// What the preview shows now.
    pub fn look(&self) -> Look {
        self.imp().look.borrow().clone()
    }

    /// Where the windows are now, once the preview has its size.
    pub fn layout(&self) -> Option<Layout> {
        self.imp().layout.borrow().clone()
    }

    /// Whether the "Not to scale" note shows.
    pub fn is_not_to_scale(&self) -> bool {
        self.imp().note.is_visible()
    }

    fn set_look(&self, look: Look) {
        if *self.imp().look.borrow() == look {
            return;
        }
        self.imp().look.replace(look);
        self.queue_allocate();
        self.queue_draw();
    }

    /// The title bar font: sway's font at its 96 dpi, as GTK's resolution may differ.
    fn font(&self) -> pango::FontDescription {
        let mut font = pango::FontDescription::from_string(&self.imp().look.borrow().font);
        if !font.is_size_absolute() && font.size() > 0 {
            font.set_absolute_size(f64::from(font.size()) * 96.0 / 72.0);
        }
        font
    }

    /// The title `text` laid out in the title bar font.
    fn title_layout(&self, text: &str, font: &pango::FontDescription) -> pango::Layout {
        let layout = self.create_pango_layout(Some(text));
        layout.set_font_description(Some(font));
        layout
    }

    /// Places the windows in a preview of `width` × `height`.
    fn place(&self, width: i32, height: i32) -> Layout {
        let font = self.font();
        let metrics = self.pango_context().metrics(Some(&font), None);
        let scale = f64::from(pango::SCALE);
        let text_height = (f64::from(metrics.ascent() + metrics.descent()) / scale).ceil();
        let widths = TITLES.map(|title| f64::from(self.title_layout(title, &font).pixel_size().0));
        sway_things_core::preview::layout(
            &self.imp().look.borrow(),
            f64::from(width),
            f64::from(height),
            &TextSize {
                height: text_height,
                widths,
            },
        )
    }

    /// Paints the windows of `layout`: the shadows first, below both windows.
    fn paint(&self, snapshot: &gtk::Snapshot, layout: &Layout) {
        for window in &layout.windows {
            if let Some(shadow) = window.shadow {
                snapshot.append_outset_shadow(
                    &rounded(&window.area, window.corner_radius),
                    &rgba(shadow.color),
                    to_f32(shadow.dx),
                    to_f32(shadow.dy),
                    0.0,
                    to_f32(shadow.blur_radius),
                );
            }
        }
        let font = self.font();
        for (window, content) in layout.windows.iter().zip(&self.imp().contents) {
            self.paint_window(snapshot, window, content, &font);
        }
    }

    fn paint_window(
        &self,
        snapshot: &gtk::Snapshot,
        window: &Window,
        content: &gtk::Box,
        font: &pango::FontDescription,
    ) {
        if window.area.is_empty() {
            return;
        }
        snapshot.push_rounded_clip(&rounded(&window.area, window.corner_radius));
        if let Some(title) = &window.title_bar {
            snapshot.append_color(&rgba(window.colors.background), &rect(&title.area));
            for edge in &title.edges {
                snapshot.append_color(&rgba(window.colors.border), &rect(edge));
            }
            snapshot.push_clip(&rect(&title.text_area));
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                to_f32(title.text_x),
                to_f32(title.text_y),
            ));
            snapshot.append_layout(
                &self.title_layout(window.title, font),
                &rgba(window.colors.text),
            );
            snapshot.restore();
            snapshot.pop();
        }
        for (piece, color) in &window.borders {
            snapshot.append_color(&rgba(*color), &rect(piece));
        }
        self.snapshot_child(content, snapshot);
        paint_lines(snapshot, &window.content, content.color());
        if let Some((color, amount)) = window.dim {
            let mut color = rgba(color);
            color.set_alpha(color.alpha() * to_f32(amount));
            snapshot.append_color(&color, &rect(&window.content));
        }
        snapshot.pop();
    }
}

/// A few faint lines of "text" in the content of a window, in the theme's text color.
fn paint_lines(snapshot: &gtk::Snapshot, content: &Rect, ink: gdk::RGBA) {
    let mut ink = ink;
    ink.set_alpha(ink.alpha() * 0.2);
    let lengths = [0.55, 0.4, 0.65, 0.3];
    for (index, length) in (0_u8..).zip(lengths) {
        let y = content.y + 12.0 + f64::from(index) * 14.0;
        let width = (content.width - 24.0) * length;
        if y + 6.0 > content.y + content.height - 6.0 || width < 8.0 {
            break;
        }
        let line = Rect {
            x: content.x + 12.0,
            y,
            width,
            height: 6.0,
        };
        snapshot.push_rounded_clip(&rounded(&line, 3.0));
        snapshot.append_color(&ink, &rect(&line));
        snapshot.pop();
    }
}

/// `value` as a graphics coordinate; the preview's values are small.
#[expect(
    clippy::cast_possible_truncation,
    reason = "preview coordinates are small, f32 holds them"
)]
fn to_f32(value: f64) -> f32 {
    value as f32
}

fn rect(rect: &Rect) -> graphene::Rect {
    graphene::Rect::new(
        to_f32(rect.x),
        to_f32(rect.y),
        to_f32(rect.width),
        to_f32(rect.height),
    )
}

fn rounded(area: &Rect, radius: f64) -> gsk::RoundedRect {
    gsk::RoundedRect::from_rect(rect(area), to_f32(radius))
}

/// `rect` in whole pixels: x, y, width, height.
#[expect(
    clippy::cast_possible_truncation,
    reason = "preview coordinates are small and rounded first"
)]
fn pixels(rect: &Rect) -> (i32, i32, i32, i32) {
    (
        rect.x.round() as i32,
        rect.y.round() as i32,
        rect.width.round() as i32,
        rect.height.round() as i32,
    )
}

/// The transform to `x`, `y`.
#[expect(
    clippy::cast_precision_loss,
    reason = "pixel offsets are far below 2^24, where f32 starts to round"
)]
fn at(x: i32, y: i32) -> gsk::Transform {
    gsk::Transform::new().translate(&graphene::Point::new(x as f32, y as f32))
}

/// A sway color as GDK's.
fn rgba(color: Color) -> gdk::RGBA {
    let [red, green, blue] = color.channels().map(|channel| f32::from(channel) / 255.0);
    gdk::RGBA::new(red, green, blue, f32::from(color.alpha()) / 255.0)
}
