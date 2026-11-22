// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pictures read off the main thread: thumbnails for the tiles, and a picture at its own size
//! for the fits _Center_ and _Tile_. The tiles of the grids frame their thumbnails alike.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use sway_things_core::command::Color;

use super::{THUMBNAIL, TILE};

thread_local! {
    /// The thumbnails read so far, by picture; `None` for one that can't be read.
    static THUMBNAILS: RefCell<HashMap<PathBuf, Option<gdk::Texture>>> = RefCell::new(HashMap::new());
}

thread_local! {
    /// The style sheet that gives the frames the background color of the edited display, with
    /// the color it holds.
    static FRAME_COLOR: RefCell<Option<(gtk::CssProvider, Color)>> = const { RefCell::new(None) };
}

/// A tile of the grids (the pictures, the collections, the members of one): a frame of 16:10
/// that `picture` fills, cut off where it doesn't fit, like swaybg's `fill`; a folder as its
/// icon on the background color of the edited display ([`set_frame_color`]), and without
/// either, the color alone. Returns the frame and the overlay in it, for marks on the picture.
pub(super) fn frame(picture: Option<&Path>) -> (gtk::AspectFrame, gtk::Overlay) {
    let frame = gtk::AspectFrame::new(0.5, 0.5, 16.0 / 10.0, false);
    frame.set_size_request(TILE.0, TILE.1);
    let inside = gtk::Overlay::new();
    inside.add_css_class("st-wallpaper-frame");
    inside.set_overflow(gtk::Overflow::Hidden);
    match picture {
        Some(path) if path.is_dir() => {
            let icon = gtk::Image::from_icon_name("folder-symbolic");
            icon.set_pixel_size(40);
            icon.add_css_class("st-wallpaper-folder");
            inside.set_child(Some(&icon));
        }
        Some(path) => {
            let image = gtk::Picture::new();
            image.set_content_fit(gtk::ContentFit::Cover);
            image.set_can_shrink(true);
            thumbnail_into(&image, path);
            inside.set_child(Some(&image));
        }
        None => {}
    }
    frame.set_child(Some(&inside));
    (frame, inside)
}

/// Gives the [frames](frame) without a picture the background color `color` of the edited
/// display.
pub(super) fn set_frame_color(color: Color) {
    FRAME_COLOR.with_borrow_mut(|loaded| {
        if loaded.as_ref().is_some_and(|(_, shown)| *shown == color) {
            return;
        }
        let provider = if let Some((provider, _)) = loaded.take() {
            provider
        } else {
            let provider = gtk::CssProvider::new();
            if let Some(display) = gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    // Above the app's own style sheet.
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
                );
            }
            provider
        };
        // Our own color as #rrggbb, never text from outside.
        provider.load_from_string(&format!(
            "st-page .st-wallpaper-frame {{ background-color: {color}; }}"
        ));
        *loaded = Some((provider, color));
    });
}

/// Shows the thumbnail of `path` in `image`: at once if it was read before, else once it is. A
/// picture that can't be read shows an icon instead, at its own size.
pub(super) fn thumbnail_into(image: &gtk::Picture, path: &Path) {
    let weak = image.downgrade();
    read(path, move |texture| {
        if let Some(image) = weak.upgrade() {
            show(&image, texture);
        }
    });
}

/// Calls `done` with whether `path` can be read as a picture: at once if it was read before,
/// else once it is. swaybg reads pictures with the same loaders (gdk-pixbuf), so a picture that
/// can't be read here likely can't be shown as a wallpaper either.
pub(super) fn when_readable(path: &Path, done: impl FnOnce(bool) + 'static) {
    read(path, move |texture| done(texture.is_some()));
}

/// Calls `done` with the thumbnail of `path`, `None` if it can't be read: from the cache, else
/// read off the main loop.
fn read(path: &Path, done: impl FnOnce(Option<&gdk::Texture>) + 'static) {
    let known = THUMBNAILS.with(|cache| cache.borrow().get(path).cloned());
    if let Some(texture) = known {
        done(texture.as_ref());
        return;
    }
    let path = path.to_path_buf();
    glib::spawn_future_local(async move {
        let read = {
            let path = path.clone();
            gio::spawn_blocking(move || pixels(&path)).await
        };
        let texture = read.ok().flatten().map(|pixels| texture(&pixels));
        THUMBNAILS.with(|cache| cache.borrow_mut().insert(path, texture.clone()));
        done(texture.as_ref());
    });
}

/// Shows `texture` in `image`, or without one the icon of a picture that can't be read.
fn show(image: &gtk::Picture, texture: Option<&gdk::Texture>) {
    if let Some(texture) = texture {
        image.set_paintable(Some(texture));
        return;
    }
    let icon = gtk::IconTheme::for_display(&image.display()).lookup_icon(
        "image-missing-symbolic",
        &[],
        32,
        image.scale_factor(),
        gtk::TextDirection::None,
        gtk::IconLookupFlags::empty(),
    );
    image.set_content_fit(gtk::ContentFit::ScaleDown);
    image.add_css_class("st-wallpaper-unreadable");
    image.set_paintable(Some(&icon));
}

/// Draws `path` in `area` at its own size on a screen of `pixels`, as swaybg does for `center`
/// (in the middle) and `tile` (`repeat`, from the top left), scaled down with the screen.
pub(super) fn own_size_into(
    area: &gtk::DrawingArea,
    path: &Path,
    pixels: (i32, i32),
    repeat: bool,
) {
    let weak = area.downgrade();
    let path = path.to_path_buf();
    glib::spawn_future_local(async move {
        let read = gio::spawn_blocking(move || {
            let (_, width, _) = gtk::gdk_pixbuf::Pixbuf::file_info(&path)?;
            Some((self::pixels(&path)?, width))
        })
        .await;
        let (Some(area), Ok(Some((thumbnail, width)))) = (weak.upgrade(), read) else {
            return;
        };
        let Ok(stride) = i32::try_from(thumbnail.stride) else {
            return;
        };
        let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_bytes(
            &thumbnail.bytes,
            gtk::gdk_pixbuf::Colorspace::Rgb,
            thumbnail.alpha,
            8,
            thumbnail.width,
            thumbnail.height,
            stride,
        );
        area.set_draw_func(move |_, cairo, area_width, area_height| {
            // Area pixels per screen pixel, and thumbnail pixels per picture pixel.
            let screen = f64::from(area_width) / f64::from(pixels.0.max(1));
            let shown = f64::from(width) * screen;
            let scale = shown / f64::from(pixbuf.width().max(1));
            let shown_height = f64::from(pixbuf.height()) * scale;
            if repeat {
                cairo.scale(scale, scale);
            } else {
                cairo.translate(
                    (f64::from(area_width) - shown) / 2.0,
                    (f64::from(area_height) - shown_height) / 2.0,
                );
                cairo.scale(scale, scale);
            }
            cairo.set_source_pixbuf(&pixbuf, 0.0, 0.0);
            if repeat {
                cairo.source().set_extend(gtk::cairo::Extend::Repeat);
            }
            let _ = cairo.paint();
        });
    });
}

/// The pixels of a scaled-down picture: width, height, stride, alpha and the bytes. A pixbuf
/// can't leave the thread it was made in, its bytes can.
struct Pixels {
    width: i32,
    height: i32,
    stride: usize,
    alpha: bool,
    bytes: glib::Bytes,
}

/// Reads `picture` scaled down to the thumbnail size; `None` if it can't be read.
fn pixels(picture: &Path) -> Option<Pixels> {
    let pixbuf =
        gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(picture, THUMBNAIL.0, THUMBNAIL.1, true)
            .ok()?;
    Some(Pixels {
        width: pixbuf.width(),
        height: pixbuf.height(),
        stride: usize::try_from(pixbuf.rowstride()).ok()?,
        alpha: pixbuf.has_alpha(),
        bytes: pixbuf.read_pixel_bytes(),
    })
}

/// The texture of `pixels`, on the main loop.
fn texture(pixels: &Pixels) -> gdk::Texture {
    let format = if pixels.alpha {
        gdk::MemoryFormat::R8g8b8a8
    } else {
        gdk::MemoryFormat::R8g8b8
    };
    gdk::MemoryTexture::new(
        pixels.width,
        pixels.height,
        format,
        &pixels.bytes,
        pixels.stride,
    )
    .upcast()
}
