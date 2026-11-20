// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Wallpaper page's own parts, which [`PageView`](super::PageView) adds below the image row:
//!
//! - a notice for each line of the user's config that starts a wallpaper tool or script
//!   ([`wallpaper::starters`]), which may draw over the background set here;
//! - _Choose Image…_, a file dialog for any image;
//! - the pictures of the wallpaper folder (`app.wallpaper_folder`, else `~/Pictures/Wallpapers`
//!   or the system's backgrounds) as thumbnails, the one in use selected, and _Change Folder…_.
//!
//! Each part follows the output the image row is for. A picked image is stored with `~/` for the
//! home folder, so settings shared through dotfiles work for every user name.
//!
//! File names are external strings: they are only shown as plain text, and reach sway through
//! the `output` command builder.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use sway_things_core::registry::{Registry, Setting, Value};
use sway_things_core::wallpaper::{self, Starter};

use super::SettingRow;
use super::setting_row::tilde;
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The title of the file dialog for an image.
const CHOOSE_IMAGE: Msg = msg!("Choose an Image");
/// The title of the file dialog for the wallpaper folder.
const CHOOSE_FOLDER: Msg = msg!("Choose the Wallpaper Folder");
/// The name of the file filter of the image dialog.
const IMAGES: Msg = msg!("Images");
/// Shown in place of the thumbnails while the folder has no pictures.
const NO_PICTURES: Msg = msg!("No pictures in {folder}");
/// Shown in place of the thumbnails while the folder can't be read.
const UNREADABLE: Msg = msg!("{folder} can't be read: {error}");

/// The key of the image.
const IMAGE: &str = "wallpaper.image";
/// The key of the folder whose pictures are offered.
const FOLDER: &str = "app.wallpaper_folder";
/// The file name extensions of the images offered, which swaybg reads through gdk-pixbuf.
const EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "avif", "jxl", "bmp", "tif", "tiff",
];
/// At most this many pictures of a folder are shown.
const MAX_PICTURES: usize = 120;
/// The size of a thumbnail, in pixels.
const THUMBNAIL: (i32, i32) = (160, 100);

/// The widgets below `row`, if it is the image row.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    if row.setting().map(Setting::key) != Some(IMAGE) {
        return Vec::new();
    }
    // One widget, so the notice leaves no empty row while it is hidden.
    let parts = gtk::Box::new(gtk::Orientation::Vertical, 12);
    parts.append(&notice(model));
    parts.append(&picker(model, row));
    vec![parts.upcast()]
}

/// The notice about wallpaper tools the user's config starts. Hidden while there are none.
fn notice(model: &Model) -> gtk::Box {
    let notice = gtk::Box::new(gtk::Orientation::Vertical, 6);
    notice.add_css_class("st-wallpaper-notice");
    let update = glib::clone!(
        #[weak]
        notice,
        move |model: &Model| {
            while let Some(child) = notice.first_child() {
                notice.remove(&child);
            }
            let found = model
                .status()
                .map(|status| wallpaper::starters(status.config()))
                .unwrap_or_default();
            notice.set_visible(!found.is_empty());
            for external in found {
                notice.append(&starter_row(model, &external));
            }
        }
    );
    update(model);
    model.connect_changed(update);
    notice
}

/// One line that starts a wallpaper tool, with a button that opens it in the editor.
fn starter_row(model: &Model, external: &wallpaper::External) -> gtk::Box {
    let program = match &external.starter {
        Starter::Tool(tool) => (*tool).to_owned(),
        Starter::Script(script) => script.clone(),
    };
    let place = format!(
        "{}:{}",
        tilde(&external.directive.file),
        external.directive.line
    );
    let text = gtk::Label::new(Some(&trf(
        // TRANSLATORS: {program} is a wallpaper tool such as swww-daemon, {place} a file and line
        // such as ~/.config/sway/config:12.
        msg!(
            "Your config starts {program} ({place}). It sets a wallpaper of its own, which may \
             cover the one set here. To use this page, remove that line."
        ),
        &[("program", &program), ("place", &place)],
    )));
    text.set_xalign(0.0);
    text.set_wrap(true);
    text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    text.set_hexpand(true);
    let open = gtk::Button::with_label(tr(msg!("Open config in editor")));
    open.set_valign(gtk::Align::Center);
    let (file, line) = (
        external.directive.file.to_path_buf(),
        external.directive.line,
    );
    open.connect_clicked(glib::clone!(
        #[weak]
        model,
        #[weak]
        text,
        move |button| {
            crate::editor::open(
                button,
                Some(&model),
                &file,
                Some(line),
                glib::clone!(
                    #[weak]
                    text,
                    move |message| {
                        if let Some(message) = message {
                            text.set_text(message);
                        }
                    }
                ),
            );
        }
    ));
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    icon.set_valign(gtk::Align::Start);
    row.append(&icon);
    row.append(&text);
    row.append(&open);
    row
}

/// What the picker shows and where it reads from.
#[derive(Default)]
struct Shown {
    /// The folder whose pictures are listed, once they are.
    folder: Option<PathBuf>,
    /// The picture of each thumbnail, in order.
    pictures: Vec<PathBuf>,
}

/// _Choose Image…_, _Change Folder…_ and the thumbnails of the folder.
fn picker(model: &Model, row: &SettingRow) -> gtk::Box {
    let picker = gtk::Box::new(gtk::Orientation::Vertical, 8);
    picker.add_css_class("st-wallpaper-picker");

    let row = row.downgrade();
    let folder_label = gtk::Label::new(None);
    picker.append(&buttons(model, &row, &folder_label));

    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::Single);
    grid.set_homogeneous(true);
    grid.set_max_children_per_line(6);
    grid.set_min_children_per_line(2);
    grid.set_row_spacing(8);
    grid.set_column_spacing(8);
    grid.set_activate_on_single_click(true);
    grid.update_property(&[gtk::accessible::Property::Label(tr(msg!("Pictures")))]);
    picker.append(&grid);
    let empty = gtk::Label::new(None);
    empty.set_xalign(0.0);
    empty.set_wrap(true);
    empty.add_css_class(style::DIM);
    picker.append(&empty);

    let shown = Rc::new(RefCell::new(Shown::default()));
    grid.connect_child_activated(glib::clone!(
        #[strong]
        shown,
        #[strong]
        row,
        move |_, child| {
            let index = usize::try_from(child.index()).ok();
            let picture = index.and_then(|index| shown.borrow().pictures.get(index).cloned());
            if let (Some(picture), Some(row)) = (picture, row.upgrade()) {
                row.change(Value::Text(stored_path(&picture)));
            }
        }
    ));

    // The image in use is selected, for the output the row is for.
    let select = glib::clone!(
        #[weak]
        grid,
        #[strong]
        shown,
        #[strong]
        row,
        move |model: &Model| {
            let current = row.upgrade().and_then(|row| image_of(model, &row));
            let index = current.and_then(|current| {
                shown
                    .borrow()
                    .pictures
                    .iter()
                    .position(|picture| *picture == current)
            });
            match index
                .and_then(|index| i32::try_from(index).ok())
                .and_then(|index| grid.child_at_index(index))
            {
                Some(child) => grid.select_child(&child),
                None => grid.unselect_all(),
            }
        }
    );

    let update = glib::clone!(
        #[weak]
        grid,
        #[weak]
        empty,
        #[weak]
        folder_label,
        #[strong]
        shown,
        #[strong]
        select,
        move |model: &Model| {
            let folder = folder_of(model);
            if shown.borrow().folder.as_ref() != Some(&folder) {
                folder_label.set_text(&tilde(&folder));
                shown.replace(Shown {
                    folder: Some(folder.clone()),
                    pictures: Vec::new(),
                });
                load(&folder, &grid, &empty, &shown, &select, model);
            }
            select(model);
        }
    );
    update(model);
    model.connect_changed(update);
    if let Some(row) = row.upgrade() {
        row.connect_device_changed(glib::clone!(
            #[weak]
            model,
            move |_| select(&model)
        ));
    }
    picker
}

/// The folder shown in `folder_label`, _Change Folder…_ and _Choose Image…_.
fn buttons(model: &Model, row: &glib::WeakRef<SettingRow>, folder_label: &gtk::Label) -> gtk::Box {
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let choose = gtk::Button::with_label(tr(msg!("Choose Image…")));
    let change = gtk::Button::with_label(tr(msg!("Change Folder…")));
    folder_label.set_xalign(0.0);
    folder_label.set_hexpand(true);
    folder_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    folder_label.add_css_class(style::DIM);
    buttons.append(folder_label);
    buttons.append(&change);
    buttons.append(&choose);
    choose.connect_clicked(glib::clone!(
        #[weak]
        model,
        #[strong]
        row,
        move |button| choose_image(button, &model, &row)
    ));
    change.connect_clicked(glib::clone!(
        #[weak]
        model,
        move |button| choose_folder(button, &model)
    ));
    buttons
}

/// The image in effect for the output `row` is for, as a path to compare with the pictures.
fn image_of(model: &Model, row: &SettingRow) -> Option<PathBuf> {
    let setting = row.setting()?;
    match model.view(setting, row.device().as_deref())?.value {
        Value::Text(path) if !path.is_empty() => Some(expand(&path)),
        _ => None,
    }
}

/// The folder whose pictures are offered: the preference, else `~/Pictures/Wallpapers` if it
/// exists, else the system's backgrounds.
fn folder_of(model: &Model) -> PathBuf {
    let preference = Registry::builtin()
        .get(FOLDER)
        .and_then(|setting| model.view(setting, None))
        .and_then(|view| match view.value {
            Value::Text(folder) if !folder.trim().is_empty() => Some(expand(folder.trim())),
            _ => None,
        });
    preference.unwrap_or_else(|| {
        let pictures = glib::user_special_dir(glib::UserDirectory::Pictures)
            .unwrap_or_else(|| glib::home_dir().join("Pictures"))
            .join("Wallpapers");
        if pictures.is_dir() {
            pictures
        } else {
            PathBuf::from("/usr/share/backgrounds")
        }
    })
}

/// `path` with a leading `~/` replaced by the home folder.
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => glib::home_dir().join(rest),
        None => PathBuf::from(path),
    }
}

/// `path` as it is stored: with `~/` for the home folder.
fn stored_path(path: &Path) -> String {
    tilde(path)
}

/// Lists the pictures of `folder` and loads their thumbnails, off the main loop. A folder that
/// changed in the meantime wins: what was loaded for another folder is dropped.
fn load(
    folder: &Path,
    grid: &gtk::FlowBox,
    empty: &gtk::Label,
    shown: &Rc<RefCell<Shown>>,
    select: &(impl Fn(&Model) + Clone + 'static),
    model: &Model,
) {
    grid.remove_all();
    empty.set_visible(false);
    let folder = folder.to_path_buf();
    glib::spawn_future_local(glib::clone!(
        #[weak]
        grid,
        #[weak]
        empty,
        #[weak]
        model,
        #[strong]
        shown,
        #[strong]
        select,
        async move {
            let listed = {
                let folder = folder.clone();
                gio::spawn_blocking(move || pictures(&folder)).await
            };
            let pictures = match listed {
                Ok(Ok(pictures)) => pictures,
                Ok(Err(error)) => {
                    empty.set_text(&trf(
                        UNREADABLE,
                        &[("folder", &tilde(&folder)), ("error", &error)],
                    ));
                    empty.set_visible(true);
                    return;
                }
                Err(_) => return,
            };
            if shown.borrow().folder.as_ref() != Some(&folder) {
                return;
            }
            if pictures.is_empty() {
                empty.set_text(&trf(NO_PICTURES, &[("folder", &tilde(&folder))]));
                empty.set_visible(true);
            }
            for picture in &pictures {
                grid.append(&tile(picture));
            }
            shown.borrow_mut().pictures.clone_from(&pictures);
            select(&model);
            for (index, picture) in pictures.into_iter().enumerate() {
                let thumbnail = gio::spawn_blocking(move || thumbnail(&picture)).await;
                if shown.borrow().folder.as_ref() != Some(&folder) {
                    return;
                }
                let child = i32::try_from(index)
                    .ok()
                    .and_then(|index| grid.child_at_index(index));
                let image = child
                    .and_then(|child| child.child())
                    .and_downcast::<gtk::Picture>();
                if let (Ok(Some(texture)), Some(image)) = (thumbnail.map(texture), image) {
                    image.set_paintable(Some(&texture));
                }
            }
        }
    ));
}

/// The pictures of `folder` with a known extension, by name, at most [`MAX_PICTURES`].
fn pictures(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut pictures: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
                })
                && path.is_file()
        })
        .collect();
    pictures.sort();
    pictures.truncate(MAX_PICTURES);
    Ok(pictures)
}

/// A thumbnail tile for `picture`, empty until its thumbnail is loaded.
fn tile(picture: &Path) -> gtk::Picture {
    let image = gtk::Picture::new();
    image.set_size_request(THUMBNAIL.0, THUMBNAIL.1);
    image.set_content_fit(gtk::ContentFit::Cover);
    image.set_can_shrink(true);
    image.add_css_class("st-wallpaper-thumbnail");
    let name = picture
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    image.set_tooltip_text(Some(&name));
    image.set_alternative_text(Some(&name));
    image
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
fn thumbnail(picture: &Path) -> Option<Pixels> {
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(
        picture,
        THUMBNAIL.0 * 2,
        THUMBNAIL.1 * 2,
        true,
    )
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
fn texture(pixels: Option<Pixels>) -> Option<gdk::MemoryTexture> {
    let pixels = pixels?;
    let format = if pixels.alpha {
        gdk::MemoryFormat::R8g8b8a8
    } else {
        gdk::MemoryFormat::R8g8b8
    };
    Some(gdk::MemoryTexture::new(
        pixels.width,
        pixels.height,
        format,
        &pixels.bytes,
        pixels.stride,
    ))
}

/// Asks for an image and sets it for the output `row` is for.
fn choose_image(button: &gtk::Button, model: &Model, row: &glib::WeakRef<SettingRow>) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(tr(IMAGES)));
    for extension in EXTENSIONS {
        filter.add_suffix(extension);
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title(tr(CHOOSE_IMAGE))
        .modal(true)
        .filters(&filters)
        .default_filter(&filter)
        .initial_folder(&gio::File::for_path(folder_of(model)))
        .build();
    let window = button.root().and_downcast::<gtk::Window>();
    let row = row.clone();
    dialog.open(window.as_ref(), gio::Cancellable::NONE, move |result| {
        let (Ok(file), Some(row)) = (result, row.upgrade()) else {
            return;
        };
        if let Some(path) = file.path() {
            row.change(Value::Text(stored_path(&path)));
        }
    });
}

/// Asks for the folder whose pictures are offered and stores it.
fn choose_folder(button: &gtk::Button, model: &Model) {
    let dialog = gtk::FileDialog::builder()
        .title(tr(CHOOSE_FOLDER))
        .modal(true)
        .initial_folder(&gio::File::for_path(folder_of(model)))
        .build();
    let window = button.root().and_downcast::<gtk::Window>();
    dialog.select_folder(
        window.as_ref(),
        gio::Cancellable::NONE,
        glib::clone!(
            #[weak]
            model,
            move |result| {
                if let Some(path) = result.ok().and_then(|folder| folder.path()) {
                    model.set(FOLDER, None, Value::Text(stored_path(&path)));
                }
            }
        ),
    );
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::pictures;

    #[test]
    fn only_pictures_are_offered_by_name() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.JPG", "a.png", "notes.txt", "c.webp", ".hidden"] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        std::fs::create_dir(dir.path().join("d.png")).unwrap();
        let found = pictures(dir.path()).unwrap();
        let names: Vec<_> = found
            .iter()
            .map(|path| path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["a.png", "b.JPG", "c.webp"]);
        assert!(pictures(Path::new("/nonexistent/folder")).is_err());
    }
}
