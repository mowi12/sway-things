// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Cursor & Tablet page's own parts, which [`PageView`](super::PageView) adds to the rows
//! built from the registry:
//!
//! - the cursor theme as a drop-down of the installed themes, each with its arrow;
//! - the cursor size as a drop-down of the sizes the picked theme ships, following the theme;
//! - the screen of a drawing tablet or a touchscreen as a drop-down of the automatic mapping, the
//!   whole desktop and the connected outputs, following the outputs.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};
use sway_things_core::cursor::{self, CursorTheme, DEFAULT_SIZE, DEFAULT_THEME};
use sway_things_core::registry::{MAP_AUTOMATIC, MAP_DESKTOP, Registry, Setting, Value};
use sway_things_core::sway_config::Disk;

use super::SettingRow;
use crate::i18n::{msg, tr, trf};
use crate::model::Model;

/// The key of the cursor theme.
const THEME: &str = "cursor.theme";
/// The key of the cursor size.
const SIZE: &str = "cursor.size";
/// The keys of the settings that map a device to a screen.
const MAPS: [&str; 2] = [
    "input.tablet_tool.map_to_output",
    "input.touch.map_to_output",
];

/// The size the arrows of the theme list are drawn at.
const PREVIEW_SIZE: i32 = 24;

thread_local! {
    /// The installed cursor themes, read once.
    static THEMES: Vec<CursorTheme> = cursor::themes(&Disk);
}

/// The choices of a setting the page offers as a drop-down, each a value and its label: the
/// themes, the sizes of the theme in effect, and the screens. `None` for other settings.
pub fn choices(model: &Model, setting: &Setting) -> Option<Vec<(String, String)>> {
    match setting.key() {
        THEME => Some(theme_choices()),
        SIZE => Some(size_choices(model)),
        key if MAPS.contains(&key) => Some(screen_choices(model)),
        _ => None,
    }
}

/// How a summary shows the text `value` of `setting`: a cursor theme by the label of its
/// choice ("Default", or the theme's title). `None` for other settings.
pub fn label(setting: &Setting, value: &str) -> Option<String> {
    (setting.key() == THEME).then(|| {
        theme_choices()
            .into_iter()
            .find(|(name, _)| name == value)
            .map_or_else(|| value.to_owned(), |(_, label)| label)
    })
}

/// Default first, then the installed themes by title. A theme called `default` is what Default
/// loads anyway, so it isn't listed twice.
fn theme_choices() -> Vec<(String, String)> {
    let mut choices = vec![(DEFAULT_THEME.to_owned(), tr(msg!("Default")).to_owned())];
    THEMES.with(|themes| {
        choices.extend(
            themes
                .iter()
                .filter(|theme| theme.name != DEFAULT_THEME)
                .map(|theme| (theme.name.clone(), theme.title.clone())),
        );
    });
    choices
}

/// The theme in effect.
fn theme_in_effect(model: &Model) -> String {
    Registry::builtin()
        .get(THEME)
        .and_then(|setting| model.view(setting, None))
        .and_then(|view| match view.value {
            Value::Text(theme) => Some(theme),
            _ => None,
        })
        .unwrap_or_else(|| DEFAULT_THEME.to_owned())
}

/// The sizes the theme in effect ships, with sway's default among them. Without a readable theme
/// the common sizes.
fn size_choices(model: &Model) -> Vec<(String, String)> {
    let theme = theme_in_effect(model);
    let mut sizes = THEMES.with(|themes| {
        themes
            .iter()
            .find(|known| known.name == theme)
            .map(|known| known.sizes.clone())
            .unwrap_or_default()
    });
    if sizes.is_empty() {
        sizes = vec![24, 32, 48, 64, 96];
    }
    if !sizes.contains(&DEFAULT_SIZE) {
        sizes.push(DEFAULT_SIZE);
        sizes.sort_unstable();
    }
    sizes
        .into_iter()
        .map(|size| {
            let label = if size == DEFAULT_SIZE {
                trf(msg!("{size} px (default)"), &[("size", &size)])
            } else {
                trf(msg!("{size} px"), &[("size", &size)])
            };
            (size.to_string(), label)
        })
        .collect()
}

/// Automatic, the whole desktop, then the connected outputs.
fn screen_choices(model: &Model) -> Vec<(String, String)> {
    let mut choices = vec![
        (MAP_AUTOMATIC.to_owned(), tr(msg!("Automatic")).to_owned()),
        (MAP_DESKTOP.to_owned(), tr(msg!("All screens")).to_owned()),
    ];
    choices.extend(model.outputs());
    choices
}

/// Makes the page's rows follow what their choices depend on, and draws the arrow of each theme.
pub fn follow(model: &Model, row: &SettingRow) {
    let Some(setting) = row.setting() else {
        return;
    };
    match setting.key() {
        THEME => draw_previews(row),
        SIZE => {
            let row = row.downgrade();
            model.connect_changed(move |model| {
                if let Some(row) = row.upgrade() {
                    row.set_choices(&size_choices(model));
                }
            });
        }
        key if MAPS.contains(&key) => {
            let row = row.downgrade();
            model.connect_changed(move |model| {
                if let Some(row) = row.upgrade() {
                    row.set_choices(&screen_choices(model));
                }
            });
        }
        _ => {}
    }
}

/// Draws each item of the theme drop-down with the theme's arrow before its title.
fn draw_previews(row: &SettingRow) {
    let Some(dropdown) = row.dropdown() else {
        return;
    };
    // The arrow of each theme title, loaded when first drawn.
    let textures: Rc<RefCell<BTreeMap<String, Option<gdk::Texture>>>> = Rc::default();
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let image = gtk::Image::new();
        image.set_pixel_size(PREVIEW_SIZE);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        content.append(&image);
        content.append(&label);
        item.set_child(Some(&content));
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(title) = item
            .item()
            .and_downcast::<gtk::StringObject>()
            .map(|object| object.string().to_string())
        else {
            return;
        };
        let Some(content) = item.child() else {
            return;
        };
        let image = content.first_child().and_downcast::<gtk::Image>();
        let label = content.last_child().and_downcast::<gtk::Label>();
        if let Some(label) = label {
            label.set_text(&title);
        }
        let texture = textures
            .borrow_mut()
            .entry(title.clone())
            .or_insert_with(|| preview(&title))
            .clone();
        if let Some(image) = image {
            image.set_paintable(texture.as_ref());
            image.set_visible(texture.is_some());
        }
    });
    dropdown.set_factory(Some(&factory));
}

/// The arrow of the theme titled `title`, at the size the list draws it, or `None` if it can't be
/// read. Default shows the arrow of the theme called `default`, if there is one.
fn preview(title: &str) -> Option<gdk::Texture> {
    THEMES.with(|themes| {
        let theme = themes
            .iter()
            .find(|theme| theme.title == title)
            .or_else(|| {
                (title == tr(msg!("Default")))
                    .then(|| themes.iter().find(|theme| theme.name == DEFAULT_THEME))
                    .flatten()
            })?;
        let size = u32::try_from(PREVIEW_SIZE).ok()?;
        let image = theme.preview(&Disk, size).ok()?;
        let width = i32::try_from(image.width).ok()?;
        let height = i32::try_from(image.height).ok()?;
        let stride = usize::try_from(image.width).ok()?.checked_mul(4)?;
        let bytes = glib::Bytes::from_owned(image.pixels);
        Some(
            gdk::MemoryTexture::new(
                width,
                height,
                gdk::MemoryFormat::B8g8r8a8Premultiplied,
                &bytes,
                stride,
            )
            .upcast(),
        )
    })
}
