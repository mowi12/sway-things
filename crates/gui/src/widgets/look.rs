// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Apps Look & Feel page's own parts, which [`PageView`](super::PageView) adds to the rows
//! built from the registry:
//!
//! - the GTK theme as a drop-down of the installed themes;
//! - the icon theme as a drop-down of the installed icon themes, each with a few of its icons;
//! - a note at the top of the page when GSettings has no `org.gnome.desktop.interface`, since
//!   nothing on the page can reach apps then, and one when no Settings portal reports the style,
//!   which is how GTK 4, libadwaita and the browsers learn it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use sway_things_core::gsettings::{Gio, INTERFACE};
use sway_things_core::look::{self, Theme};
use sway_things_core::registry::Setting;
use sway_things_core::sway_config::Disk;

use super::SettingRow;
use crate::i18n::{msg, tr};
use crate::{style, theme};

/// The key of the GTK theme.
const GTK_THEME: &str = "look.gtk_theme";
/// The key of the icon theme.
const ICON_THEME: &str = "look.icon_theme";

/// The icons an icon theme is shown with: a folder, a document, settings and the trash, which
/// every complete theme has.
const SAMPLE_ICONS: [&str; 4] = [
    "folder",
    "text-x-generic",
    "preferences-system",
    "user-trash",
];

/// The size the sample icons are drawn at.
const SAMPLE_SIZE: i32 = 20;

thread_local! {
    /// The installed GTK themes, read once.
    static GTK_THEMES: Vec<Theme> = look::gtk_themes(&Disk);
    /// The installed icon themes, read once.
    static ICON_THEMES: Vec<Theme> = look::icon_themes(&Disk);
}

/// The choices of a setting the page offers as a drop-down, each a value and its label: the
/// installed GTK themes and icon themes. `None` for other settings.
pub fn choices(setting: &Setting) -> Option<Vec<(String, String)>> {
    let themes = |themes: &Vec<Theme>| {
        themes
            .iter()
            .map(|theme| (theme.name.clone(), theme.title.clone()))
            .collect()
    };
    match setting.key() {
        GTK_THEME => Some(GTK_THEMES.with(themes)),
        ICON_THEME => Some(ICON_THEMES.with(themes)),
        _ => None,
    }
}

/// How a summary shows the text `value` of `setting`: a theme by its title. `None` for other
/// settings.
pub fn label(setting: &Setting, value: &str) -> Option<String> {
    let title = |themes: &Vec<Theme>| {
        themes
            .iter()
            .find(|theme| theme.name == value)
            .map_or_else(|| value.to_owned(), |theme| theme.title.clone())
    };
    match setting.key() {
        GTK_THEME => Some(GTK_THEMES.with(title)),
        ICON_THEME => Some(ICON_THEMES.with(title)),
        _ => None,
    }
}

/// Draws the items of the icon theme drop-down with a few icons of each theme.
pub fn follow(row: &SettingRow) {
    if row.setting().map(Setting::key) == Some(ICON_THEME) {
        draw_samples(row);
    }
}

/// The note above the page's rows: that GSettings lacks the schema or keeps nothing, or else
/// that no Settings portal reports the style (hidden unless so).
pub fn note() -> gtk::Widget {
    let has_schema = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(INTERFACE, true))
        .is_some();
    if !has_schema {
        note_label(tr(msg!(
            "Apps can't read these settings: GSettings has no org.gnome.desktop.interface. \
             Install the package gsettings-desktop-schemas, then open Sway Things again."
        )))
        .upcast()
    } else if !Gio::keeps_values() {
        note_label(tr(msg!(
            "Apps can't read these settings: nothing keeps them, since dconf isn't installed. \
             Install dconf, then log in again."
        )))
        .upcast()
    } else {
        portal_note()
    }
}

/// A note above the page's rows.
fn note_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.add_css_class(style::DIM);
    label.set_margin_bottom(12);
    label
}

/// A note that the style can't reach apps, shown once it is known that no Settings portal
/// reports it. Hidden until then, and for good if one does.
fn portal_note() -> gtk::Widget {
    let label = note_label(tr(msg!(
        "Apps can't learn the style: no desktop portal tells them. Install \
         xdg-desktop-portal-gtk, then log in again."
    )));
    label.set_visible(false);
    glib::spawn_future_local(glib::clone!(
        #[weak]
        label,
        async move {
            label.set_visible(!theme::portal_has_color_scheme().await);
        }
    ));
    label.upcast()
}

fn draw_samples(row: &SettingRow) {
    let Some(dropdown) = row.dropdown() else {
        return;
    };
    // An icon theme object per title, made when its item is first drawn.
    let themes: Rc<RefCell<BTreeMap<String, gtk::IconTheme>>> = Rc::default();
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        content.append(&label);
        let icons = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        for _ in SAMPLE_ICONS {
            let image = gtk::Image::new();
            image.set_pixel_size(SAMPLE_SIZE);
            image.set_accessible_role(gtk::AccessibleRole::Presentation);
            icons.append(&image);
        }
        content.append(&icons);
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
        if let Some(label) = content.first_child().and_downcast::<gtk::Label>() {
            label.set_text(&title);
        }
        let icons = content.last_child();
        let name = ICON_THEMES.with(|known| {
            known
                .iter()
                .find(|theme| theme.title == title)
                .map(|theme| theme.name.clone())
        });
        // A theme that isn't installed shows no icons rather than another theme's.
        if let Some(icons) = &icons {
            icons.set_visible(name.is_some());
        }
        let Some(name) = name else {
            return;
        };
        let theme = themes
            .borrow_mut()
            .entry(name.clone())
            .or_insert_with(|| {
                let theme = gtk::IconTheme::new();
                theme.set_theme_name(Some(&name));
                theme
            })
            .clone();
        let mut child = icons.and_then(|icons| icons.first_child());
        for icon in SAMPLE_ICONS {
            let Some(image) = child.and_downcast_ref::<gtk::Image>().cloned() else {
                break;
            };
            let paintable = theme.lookup_icon(
                icon,
                &[],
                SAMPLE_SIZE,
                image.scale_factor(),
                gtk::TextDirection::None,
                gtk::IconLookupFlags::empty(),
            );
            image.set_paintable(Some(&paintable));
            child = image.next_sibling();
        }
    });
    // The button keeps showing the title alone; the list shows the icons.
    dropdown.set_list_factory(Some(&factory));
}
