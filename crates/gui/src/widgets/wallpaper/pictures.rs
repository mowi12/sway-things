// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Pictures tab: the _Default_ tile, then the pictures of the wallpaper folder, with its
//! subfolders as chips, and _Choose Image…_.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use sway_things_core::registry::app::WALLPAPER_FOLDER;
use sway_things_core::registry::wallpaper::IMAGE;
use sway_things_core::registry::{Registry, Value};
use sway_things_core::wallpaper::{self, EXTENSIONS};

use super::colors::solid;
use super::images::frame;
use super::{Listing, Look, MAX_PICTURES, Ui, index};
use crate::i18n::{msg, tr, trf};
use crate::style;
use crate::widgets::setting_row::tilde;

impl Ui {
    /// _Change Folder…_ with the folder's name, and _Choose Image…_.
    pub(super) fn picture_actions(self: &Rc<Self>) -> gtk::Box {
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        self.folder_button
            .set_tooltip_text(Some(tr(msg!("Change the folder whose pictures are shown"))));
        let choose = gtk::Button::with_label(tr(msg!("Choose Image…")));
        actions.append(&self.folder_button);
        actions.append(&choose);
        let weak = Rc::downgrade(self);
        self.folder_button.connect_clicked(move |button| {
            if let Some(ui) = weak.upgrade() {
                ui.choose_folder(button);
            }
        });
        let weak = Rc::downgrade(self);
        choose.connect_clicked(move |button| {
            if let Some(ui) = weak.upgrade() {
                ui.choose_image(button);
            }
        });
        actions
    }

    /// The subfolder chips and the pictures of the folder.
    pub(super) fn pictures_tab(self: &Rc<Self>) -> gtk::Box {
        let tab = gtk::Box::new(gtk::Orientation::Vertical, 10);
        tab.append(&self.chips);
        tab.append(&self.pictures);
        tab.append(&self.pictures_empty);
        let weak = Rc::downgrade(self);
        self.pictures.connect_child_activated(move |_, child| {
            let Some(ui) = weak.upgrade() else { return };
            if is_default(child) {
                ui.use_default();
                return;
            }
            // The pictures come after the default.
            let picture = index(child)
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| ui.shown_pictures.borrow().get(index).cloned());
            if let Some(picture) = picture {
                ui.pick_picture(&picture);
            }
        });
        tab
    }

    /// The folder whose pictures are offered: the preference, else `~/Pictures`.
    pub(super) fn folder(&self) -> PathBuf {
        let preference = Registry::builtin()
            .get(WALLPAPER_FOLDER)
            .and_then(|setting| self.model.view(setting, None))
            .and_then(|view| match view.value {
                Value::Text(folder) if !folder.trim().is_empty() => {
                    Some(wallpaper::expand(folder.trim(), Some(&glib::home_dir())))
                }
                _ => None,
            });
        preference.unwrap_or_else(|| {
            glib::user_special_dir(glib::UserDirectory::Pictures)
                .unwrap_or_else(|| glib::home_dir().join("Pictures"))
        })
    }

    /// Lists the folder's pictures again if the folder changed.
    pub(super) fn refresh_folder(self: &Rc<Self>) {
        let folder = self.folder();
        let name = folder.file_name().map_or_else(
            || folder.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        self.folder_button.set_label(&format!("{name} ▾"));
        let same = self
            .listed
            .borrow()
            .as_ref()
            .is_some_and(|(listed, _)| *listed == folder);
        if same {
            return;
        }
        self.listed.replace(Some((folder.clone(), Vec::new())));
        self.chip.replace(None);
        self.pictures.remove_all();
        self.chips.remove_all();
        self.pictures_empty.set_visible(false);
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let listed = {
                let folder = folder.clone();
                gio::spawn_blocking(move || list_folder(&folder)).await
            };
            let Some(ui) = weak.upgrade() else { return };
            if ui.listed.borrow().as_ref().map(|(listed, _)| listed) != Some(&folder) {
                return;
            }
            match listed {
                Ok(Ok(pictures)) => {
                    ui.listed.replace(Some((folder, pictures)));
                    ui.fill_chips();
                    ui.fill_pictures();
                }
                Ok(Err(error)) => {
                    ui.pictures_empty.set_text(&trf(
                        msg!("{folder} can't be read: {error}"),
                        &[("folder", &tilde(&folder)), ("error", &error)],
                    ));
                    ui.pictures_empty.set_visible(true);
                }
                Err(_) => {}
            }
        });
    }

    /// _All_ and a chip for each subfolder with pictures.
    pub(super) fn fill_chips(self: &Rc<Self>) {
        self.chips.remove_all();
        let subfolders: Vec<String> = {
            let listed = self.listed.borrow();
            let mut names: Vec<String> = listed
                .iter()
                .flat_map(|(_, pictures)| pictures.iter().filter_map(|(sub, _)| sub.clone()))
                .collect();
            names.dedup();
            names
        };
        self.chips.set_visible(!subfolders.is_empty());
        let all = gtk::ToggleButton::with_label(tr(msg!("All")));
        all.add_css_class("st-wallpaper-chip");
        all.set_active(true);
        self.chips.append(&all);
        let weak = Rc::downgrade(self);
        all.connect_toggled(move |button| {
            if let (Some(ui), true) = (weak.upgrade(), button.is_active()) {
                ui.chip.replace(None);
                ui.fill_pictures();
            }
        });
        for name in subfolders {
            let chip = gtk::ToggleButton::with_label(&name);
            chip.add_css_class("st-wallpaper-chip");
            chip.set_group(Some(&all));
            self.chips.append(&chip);
            let weak = Rc::downgrade(self);
            chip.connect_toggled(move |button| {
                if let (Some(ui), true) = (weak.upgrade(), button.is_active()) {
                    ui.chip.replace(Some(name.clone()));
                    ui.fill_pictures();
                }
            });
        }
    }

    /// The pictures of the folder, or of the chosen subfolder.
    pub(super) fn fill_pictures(self: &Rc<Self>) {
        self.pictures.remove_all();
        let target = self.target();
        self.pictures.append(&self.default_tile(target.as_deref()));
        let chip = self.chip.borrow().clone();
        let pictures: Vec<PathBuf> = self
            .listed
            .borrow()
            .iter()
            .flat_map(|(_, pictures)| pictures.iter())
            .filter(|(sub, _)| chip.is_none() || *sub == chip)
            .map(|(_, picture)| picture.clone())
            .collect();
        for picture in &pictures {
            self.pictures.append(&picture_tile(picture));
        }
        if pictures.is_empty() {
            let folder = self.folder();
            self.pictures_empty.set_text(&trf(
                msg!("No pictures in {folder}"),
                &[("folder", &tilde(&folder))],
            ));
        }
        self.pictures_empty.set_visible(pictures.is_empty());
        self.shown_pictures.replace(pictures);
        self.select_picture(target.as_deref());
    }

    /// Puts the _Default_ tile for `target` first in the grid, in place of the one there.
    pub(super) fn refresh_default(&self, target: Option<&str>) {
        if let Some(first) = self.pictures.child_at_index(0).filter(is_default) {
            self.pictures.remove(&first);
        }
        self.pictures.insert(&self.default_tile(target), 0);
    }

    /// The tile of what `target` shows once Sway Things stores no wallpaper for it.
    fn default_tile(&self, target: Option<&str>) -> gtk::Box {
        let (picture, color) = self.fallback_look(target);
        let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
        tile.set_widget_name(DEFAULT);
        let (frame, inside) = frame(picture.as_deref());
        if picture.is_none() {
            let solid = gtk::Picture::for_paintable(&solid(color));
            solid.set_content_fit(gtk::ContentFit::Fill);
            inside.set_child(Some(&solid));
        }
        let origin = self.fallback_origin(target);
        frame.update_property(&[gtk::accessible::Property::Label(&origin)]);
        let caption = gtk::Label::new(Some(tr(msg!("Default"))));
        caption.add_css_class(style::DIM);
        caption.add_css_class("st-wallpaper-caption");
        caption.set_xalign(0.0);
        tile.append(&frame);
        tile.append(&caption);
        tile.set_tooltip_text(Some(&origin));
        tile
    }

    /// Selects the picture `target` shows in the grid: the default while Sway Things stores no
    /// wallpaper for it.
    pub(super) fn select_picture(&self, target: Option<&str>) {
        if self.own_keys(target).is_empty() {
            match self.pictures.child_at_index(0).filter(is_default) {
                Some(child) => self.pictures.select_child(&child),
                None => self.pictures.unselect_all(),
            }
            return;
        }
        let current = match self.look(target) {
            Look::Picture(image) => Some(wallpaper::expand(&image, Some(&glib::home_dir()))),
            Look::Collection(..) | Look::Color => None,
        };
        let index = current.and_then(|current| {
            self.shown_pictures
                .borrow()
                .iter()
                .position(|picture| *picture == current)
        });
        match index
            .and_then(|index| i32::try_from(index + 1).ok())
            .and_then(|index| self.pictures.child_at_index(index))
        {
            Some(child) => self.pictures.select_child(&child),
            None => self.pictures.unselect_all(),
        }
    }

    /// Shows `picture` on the edited output.
    pub(super) fn pick_picture(&self, picture: &Path) {
        self.set(IMAGE, Value::Text(tilde(picture)));
        self.leave_collection();
    }

    /// Asks for an image and shows it on the edited output.
    pub(super) fn choose_image(self: &Rc<Self>, button: &gtk::Button) {
        let filter = picture_filter();
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(tr(msg!("Choose an Image")))
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .initial_folder(&gio::File::for_path(self.folder()))
            .build();
        let window = button.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(self);
        dialog.open(window.as_ref(), gio::Cancellable::NONE, move |result| {
            let (Some(ui), Some(path)) = (weak.upgrade(), result.ok().and_then(|file| file.path()))
            else {
                return;
            };
            ui.pick_picture(&path);
        });
    }

    /// Asks for the folder whose pictures are offered and stores it.
    pub(super) fn choose_folder(self: &Rc<Self>, button: &gtk::Button) {
        let dialog = gtk::FileDialog::builder()
            .title(tr(msg!("Choose the Wallpaper Folder")))
            .modal(true)
            .initial_folder(&gio::File::for_path(self.folder()))
            .build();
        let window = button.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(self);
        dialog.select_folder(window.as_ref(), gio::Cancellable::NONE, move |result| {
            if let (Some(ui), Some(path)) =
                (weak.upgrade(), result.ok().and_then(|folder| folder.path()))
            {
                ui.model
                    .set(WALLPAPER_FOLDER, None, Value::Text(tilde(&path)));
            }
        });
    }
}

/// The widget name of the _Default_ tile.
const DEFAULT: &str = "st-wallpaper-default";

/// Whether `child` of the grid is the _Default_ tile.
fn is_default(child: &gtk::FlowBoxChild) -> bool {
    child
        .child()
        .is_some_and(|tile| tile.widget_name() == DEFAULT)
}

/// A picture of the folder in the grid, with its name below.
pub(super) fn picture_tile(picture: &Path) -> gtk::Box {
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let name = picture
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (frame, _) = frame(Some(picture));
    frame.update_property(&[gtk::accessible::Property::Label(&name)]);
    let caption = gtk::Label::new(Some(&name));
    caption.add_css_class(style::DIM);
    caption.add_css_class("st-wallpaper-caption");
    caption.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    caption.set_max_width_chars(1);
    caption.set_xalign(0.0);
    tile.append(&frame);
    tile.append(&caption);
    tile.set_tooltip_text(Some(&name));
    tile
}

/// The filter of the picture dialogs.
pub(super) fn picture_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(tr(msg!("Images"))));
    for extension in EXTENSIONS {
        filter.add_suffix(extension);
    }
    filter
}

/// The pictures of `folder` and of its subfolders, one level down, each with the name of its
/// subfolder; at most [`MAX_PICTURES`].
pub(super) fn list_folder(folder: &Path) -> Result<Listing, String> {
    let mut pictures: Listing = wallpaper::pictures_in(folder)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|picture| (None, picture))
        .collect();
    let mut subfolders: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .collect();
    subfolders.sort();
    for subfolder in subfolders {
        let name = subfolder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let inside = wallpaper::pictures_in(&subfolder).unwrap_or_default();
        pictures.extend(inside.into_iter().map(|picture| (name.clone(), picture)));
        if pictures.len() >= MAX_PICTURES {
            break;
        }
    }
    pictures.truncate(MAX_PICTURES);
    Ok(pictures)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::list_folder;

    #[test]
    fn subfolders_become_chips() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.png"), b"").unwrap();
        std::fs::create_dir(dir.path().join("Holidays")).unwrap();
        std::fs::write(dir.path().join("Holidays/a.jpg"), b"").unwrap();
        std::fs::create_dir(dir.path().join(".cache")).unwrap();
        std::fs::write(dir.path().join(".cache/c.png"), b"").unwrap();
        let listed = list_folder(dir.path()).unwrap();
        let names: Vec<(Option<&str>, &str)> = listed
            .iter()
            .map(|(sub, path)| (sub.as_deref(), path.file_name().unwrap().to_str().unwrap()))
            .collect();
        assert_eq!(names, [(None, "b.png"), (Some("Holidays"), "a.jpg")]);
        assert!(list_folder(Path::new("/nonexistent/folder")).is_err());
    }
}
