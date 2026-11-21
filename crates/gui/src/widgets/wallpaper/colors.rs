// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Colors tab, and the conversions between the colors of sway and of GTK.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use sway_things_core::command::Color;
use sway_things_core::registry::Value;
use sway_things_core::registry::wallpaper::{COLOR, IMAGE};

use super::{COLORS, Ui, index};
use crate::i18n::{msg, tr};

impl Ui {
    /// The colors to pick, and _Other…_.
    pub(super) fn colors_tab(self: &Rc<Self>) -> gtk::Box {
        let tab = gtk::Box::new(gtk::Orientation::Vertical, 10);
        for color in COLORS {
            let swatch = gtk::Picture::for_paintable(&solid(color));
            swatch.set_size_request(56, 36);
            swatch.set_content_fit(gtk::ContentFit::Fill);
            swatch.add_css_class("st-wallpaper-swatch");
            swatch.set_tooltip_text(Some(&color.to_string()));
            swatch.set_alternative_text(Some(&color.to_string()));
            self.colors.append(&swatch);
        }
        let other = gtk::Button::with_label(tr(msg!("Other Color…")));
        other.set_halign(gtk::Align::Start);
        tab.append(&self.colors);
        tab.append(&other);
        let weak = Rc::downgrade(self);
        self.colors.connect_child_activated(move |_, child| {
            let color = index(child).and_then(|index| COLORS.get(index).copied());
            if let (Some(ui), Some(color)) = (weak.upgrade(), color) {
                ui.pick_color(color);
            }
        });
        let weak = Rc::downgrade(self);
        other.connect_clicked(move |button| {
            let Some(ui) = weak.upgrade() else { return };
            let dialog = gtk::ColorDialog::new();
            dialog.set_with_alpha(false);
            let window = button.root().and_downcast::<gtk::Window>();
            let initial = ui.color.rgba();
            let weak = Rc::downgrade(&ui);
            dialog.choose_rgba(
                window.as_ref(),
                Some(&initial),
                gio::Cancellable::NONE,
                move |rgba| {
                    if let (Some(ui), Ok(rgba)) = (weak.upgrade(), rgba) {
                        ui.pick_color(color_of(&rgba));
                    }
                },
            );
        });
        tab
    }

    /// Fills the edited output with `color` alone.
    pub(super) fn pick_color(&self, color: Color) {
        self.set(COLOR, Value::Color(color));
        let target = self.target();
        if matches!(self.value(IMAGE, target.as_deref()), Some(Value::Text(image)) if !image.is_empty())
        {
            self.set(IMAGE, Value::Text(String::new()));
        }
        self.leave_collection();
    }
}

/// A texture of one pixel in `color`, which a picture stretches over its size.
pub(super) fn solid(color: Color) -> gdk::Texture {
    let bytes = glib::Bytes::from_owned(color.channels());
    gdk::MemoryTexture::new(1, 1, gdk::MemoryFormat::R8g8b8, &bytes, 3).upcast()
}

/// `color` for GTK.
pub(super) fn rgba(color: Color) -> gdk::RGBA {
    let [red, green, blue] = color.channels();
    gdk::RGBA::new(
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        1.0,
    )
}

/// The color of `rgba`, without its transparency.
pub(super) fn color_of(rgba: &gdk::RGBA) -> Color {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0 ..= 255 first"
    )]
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::rgb(
        channel(rgba.red()),
        channel(rgba.green()),
        channel(rgba.blue()),
    )
}
