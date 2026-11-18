// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Wallpaper page.
//!
//! - A notice for each line of the user's config that starts a wallpaper tool or script
//!   ([`wallpaper::starters`]), which may draw over the background set here.
//! - The preview: every display as a tile with its wallpaper, placed like the output layout.
//!   _Same wallpaper on all displays_ edits the values for every output; switched off, a click on
//!   a tile picks the display whose values are edited.
//! - What the edited display shows (a picture, a collection or a color), _Next Picture_ for a
//!   collection, the fit and the background color.
//! - Three tabs to pick from: the pictures of the wallpaper folder (`app.wallpaper_folder`, else
//!   `~/Pictures`) with its subfolders as chips; the collections, with the settings and pictures
//!   of the open one; and colors.
//!
//! A picked picture is stored with `~/` for the home folder, so settings shared through dotfiles
//! work for every user name. File and collection names are external strings: they are only shown
//! as plain text, and reach sway through the `output` command builder.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use sway_things_core::command::Color;
use sway_things_core::ipc::Client;
use sway_things_core::outputs::{Output, Rect};
use sway_things_core::registry::{Registry, Value};
use sway_things_core::state::State;
use sway_things_core::wallpaper::{
    self, CHANGE, COLLECTION, Change, Collection, EXTENSIONS, INTERVAL, Moment, ORDER, Order,
    PICTURES, Starter,
};

use super::setting_row::tilde;
use crate::i18n::{Msg, msg, msg_plural, ntrf, tr, trf};
use crate::model::Model;
use crate::style;

/// The key of the picture.
const IMAGE: &str = "wallpaper.image";
/// The key of the fit.
const FILL: &str = "wallpaper.fill";
/// The key of the background color.
const COLOR: &str = "wallpaper.color";
/// The key of the folder whose pictures are offered.
const FOLDER: &str = "app.wallpaper_folder";
/// The fits, by their choice ids, with their short names.
const FITS: [(&str, Msg); 5] = [
    ("fill", msg!("Fill")),
    ("fit", msg!("Fit")),
    ("stretch", msg!("Stretch")),
    ("center", msg!("Center")),
    ("tile", msg!("Tile")),
];
/// The colors offered on the Colors tab.
const COLORS: [Color; 12] = [
    Color::rgb(0x1d, 0x1d, 0x1d),
    Color::rgb(0x00, 0x00, 0x00),
    Color::rgb(0x28, 0x55, 0x77),
    Color::rgb(0x1b, 0x3a, 0x52),
    Color::rgb(0x1f, 0x33, 0x28),
    Color::rgb(0x3b, 0x6e, 0x4f),
    Color::rgb(0x5b, 0x2a, 0x86),
    Color::rgb(0x8a, 0x3b, 0x2a),
    Color::rgb(0x56, 0x61, 0x6c),
    Color::rgb(0xc3, 0xcf, 0xe2),
    Color::rgb(0xe8, 0xd6, 0xa8),
    Color::rgb(0xf5, 0xf5, 0xf4),
];
/// At most this many pictures of a folder are shown.
const MAX_PICTURES: usize = 240;
/// The size thumbnails are read at, in pixels.
const THUMBNAIL: (i32, i32) = (320, 200);
/// The size of a picture in the grid, in pixels.
const TILE: (i32, i32) = (128, 80);
/// The largest size of the preview of the displays, in pixels.
const PREVIEW: (f64, f64) = (420.0, 170.0);
/// The space for a display's name below its tile, in pixels.
const NAME_HEIGHT: i32 = 22;

/// The pictures of a folder, each with the name of its subfolder.
type Listing = Vec<(Option<String>, PathBuf)>;
/// The outputs with a collection, and when it changes.
type Timers = Vec<(Option<String>, Change)>;
/// The tiles of the preview: the output, its place and its size.
type Placed<'a> = Vec<(Option<&'a Output>, (f64, f64, i32, i32))>;

/// The Wallpaper page's body.
pub fn page(model: &Model) -> gtk::Box {
    let ui = Ui::new(model);
    let body = ui.body.clone();
    ui.refresh();
    let weak = Rc::downgrade(&ui);
    model.connect_changed(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.refresh();
            ui.follow_timers();
        }
    });
    let weak = Rc::downgrade(&ui);
    model.connect_failed(move |_, key, _, message| {
        let ours = [
            IMAGE, FILL, COLOR, COLLECTION, PICTURES, CHANGE, INTERVAL, ORDER,
        ];
        if let (Some(ui), true) = (weak.upgrade(), ours.contains(&key)) {
            ui.error.set_text(message);
            ui.error.set_visible(true);
        }
    });
    // The widgets live as long as the page; the shared parts as long as the widgets.
    body.connect_destroy(move |_| drop(Rc::clone(&ui)));
    body
}

/// What an output shows.
#[derive(Debug, Clone, PartialEq)]
enum Look {
    /// A picture, as stored.
    Picture(String),
    /// A collection, by name, with its current picture.
    Collection(String, Option<PathBuf>),
    /// The background color alone.
    Color,
}

/// The parts of the page and what it shows.
struct Ui {
    model: Model,
    body: gtk::Box,
    error: gtk::Label,
    notice: gtk::Box,
    same: gtk::Switch,
    preview: gtk::Fixed,
    title: gtk::Label,
    detail: gtk::Label,
    next: gtk::Button,
    fits: Vec<gtk::ToggleButton>,
    color: gtk::ColorDialogButton,
    folder_button: gtk::Button,
    chips: gtk::FlowBox,
    pictures: gtk::FlowBox,
    pictures_empty: gtk::Label,
    collections: gtk::FlowBox,
    collections_empty: gtk::Label,
    editor: Editor,
    colors: gtk::FlowBox,
    /// Whether the outputs are edited one by one; `None` until the user switches.
    separate: Cell<Option<bool>>,
    /// The output whose values are edited while they are edited one by one.
    selected: RefCell<Option<String>>,
    /// The collection whose settings are shown.
    open: RefCell<Option<String>>,
    /// The folder whose pictures are listed, and its pictures with their subfolders.
    listed: RefCell<Option<(PathBuf, Listing)>>,
    /// The subfolder the pictures are filtered by.
    chip: RefCell<Option<String>>,
    /// The pictures of the grid, in order.
    shown_pictures: RefCell<Vec<PathBuf>>,
    /// The collections of the grid, in order.
    shown_collections: RefCell<Vec<String>>,
    /// The collections that change by a timer, as last scheduled.
    timers: RefCell<Option<Timers>>,
    /// Set while the widgets are brought to the model, so their signals change nothing.
    syncing: Cell<bool>,
}

/// The settings and pictures of the open collection.
struct Editor {
    card: gtk::Box,
    name: gtk::Label,
    changes: Vec<gtk::ToggleButton>,
    interval: gtk::SpinButton,
    unit: gtk::DropDown,
    orders: Vec<gtk::ToggleButton>,
    count: gtk::Label,
    members: gtk::FlowBox,
    /// The entries of `members`, in order.
    entries: RefCell<Vec<String>>,
}

impl Ui {
    fn new(model: &Model) -> Rc<Self> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 18);
        body.add_css_class("st-wallpaper");
        let error = gtk::Label::new(None);
        error.set_xalign(0.0);
        error.set_wrap(true);
        error.add_css_class("error");
        error.set_visible(false);
        let notice = gtk::Box::new(gtk::Orientation::Vertical, 6);
        notice.add_css_class("st-wallpaper-notice");

        let same = gtk::Switch::new();
        same.set_valign(gtk::Align::Center);
        let preview = gtk::Fixed::new();
        preview.set_halign(gtk::Align::Center);
        preview.set_valign(gtk::Align::End);
        let title = label("st-wallpaper-title");
        let detail = label(style::DIM);
        let next = gtk::Button::with_label(tr(msg!("Next Picture")));
        next.set_valign(gtk::Align::Center);
        let fits: Vec<gtk::ToggleButton> = FITS
            .iter()
            .map(|(_, name)| gtk::ToggleButton::with_label(tr(*name)))
            .collect();
        for fit in fits.iter().skip(1) {
            fit.set_group(fits.first());
        }
        let dialog = gtk::ColorDialog::new();
        dialog.set_with_alpha(false);
        let color = gtk::ColorDialogButton::new(Some(dialog));
        color.set_valign(gtk::Align::Center);
        color.set_tooltip_text(Some(tr(msg!("Background color"))));
        color.update_property(&[gtk::accessible::Property::Label(tr(msg!(
            "Background color"
        )))]);

        let folder_button = gtk::Button::new();
        let chips = gtk::FlowBox::new();
        chips.set_selection_mode(gtk::SelectionMode::None);
        chips.set_column_spacing(6);
        chips.set_row_spacing(6);
        chips.set_max_children_per_line(12);
        chips.add_css_class("st-wallpaper-chips");
        let pictures = grid(tr(msg!("Pictures")), 6);
        let pictures_empty = label(style::DIM);
        let collections = grid(tr(msg!("Collections")), 4);
        // A selected collection is outlined, not filled.
        collections.add_css_class("st-wallpaper-outlined");
        let collections_empty = label(style::DIM);
        collections_empty.set_text(tr(msg!(
            "No collections yet. A collection is a set of pictures and folders whose pictures \
             take turns."
        )));
        let colors = grid(tr(msg!("Colors")), 12);

        let ui = Rc::new(Self {
            model: model.clone(),
            body,
            error,
            notice,
            same,
            preview,
            title,
            detail,
            next,
            fits,
            color,
            folder_button,
            chips,
            pictures,
            pictures_empty,
            collections,
            collections_empty,
            editor: Editor::new(),
            colors,
            separate: Cell::new(None),
            selected: RefCell::new(None),
            open: RefCell::new(None),
            listed: RefCell::new(None),
            chip: RefCell::new(None),
            shown_pictures: RefCell::new(Vec::new()),
            shown_collections: RefCell::new(Vec::new()),
            timers: RefCell::new(None),
            syncing: Cell::new(false),
        });
        ui.build();
        *ui.timers.borrow_mut() = model.state().map(|state| timers(&state));
        ui
    }

    /// Puts the parts together and connects their signals.
    fn build(self: &Rc<Self>) {
        self.body.append(&self.error);
        self.body.append(&self.notice);
        self.body.append(&self.current_card());
        self.editor_card();
        self.body.append(&self.tabs());
    }

    /// The card with the preview, what the edited display shows, the fit and the color.
    fn current_card(self: &Rc<Self>) -> gtk::Box {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 14);
        card.add_css_class("st-wallpaper-card");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let heading = gtk::Label::new(Some(tr(msg!("Displays"))));
        heading.add_css_class("st-section-heading");
        heading.set_xalign(0.0);
        heading.set_hexpand(true);
        let same_label = gtk::Label::new(Some(tr(msg!("Same wallpaper on all displays"))));
        same_label.set_wrap(true);
        same_label.set_xalign(1.0);
        self.same
            .update_property(&[gtk::accessible::Property::Label(tr(msg!(
                "Same wallpaper on all displays"
            )))]);
        header.append(&heading);
        header.append(&same_label);
        header.append(&self.same);
        card.append(&header);

        let stage = gtk::Box::new(gtk::Orientation::Vertical, 0);
        stage.add_css_class("st-wallpaper-stage");
        stage.append(&self.preview);
        card.append(&stage);

        let info = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&self.title);
        text.append(&self.detail);
        info.append(&text);
        info.append(&self.next);
        card.append(&info);

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let fits = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fits.add_css_class("linked");
        fits.update_property(&[gtk::accessible::Property::Label(tr(msg!("Fit")))]);
        for fit in &self.fits {
            fits.append(fit);
        }
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        controls.append(&fits);
        controls.append(&spacer);
        controls.append(&self.color);
        card.append(&controls);

        let weak = Rc::downgrade(self);
        self.same.connect_active_notify(move |switch| {
            if let Some(ui) = weak.upgrade() {
                ui.switch_same(switch.is_active());
            }
        });
        let weak = Rc::downgrade(self);
        self.next.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.next_picture();
            }
        });
        for (fit, (id, _)) in self.fits.iter().zip(FITS) {
            let weak = Rc::downgrade(self);
            fit.connect_toggled(move |button| {
                if let (Some(ui), true) = (weak.upgrade(), button.is_active()) {
                    if !ui.syncing.get() {
                        ui.set(FILL, Value::Choice(id.to_owned()));
                    }
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.color.connect_rgba_notify(move |button| {
            if let Some(ui) = weak.upgrade() {
                if !ui.syncing.get() {
                    ui.set(COLOR, Value::Color(color_of(&button.rgba())));
                }
            }
        });
        card
    }

    /// The tabs: pictures, collections and colors.
    fn tabs(self: &Rc<Self>) -> gtk::Box {
        let tabs = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let stack = gtk::Stack::new();
        stack.set_vhomogeneous(false);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.add_titled(&self.pictures_tab(), Some("pictures"), tr(msg!("Pictures")));
        stack.add_titled(
            &self.collections_tab(),
            Some("collections"),
            tr(msg!("Collections")),
        );
        stack.add_titled(&self.colors_tab(), Some("colors"), tr(msg!("Colors")));
        let switcher = gtk::StackSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_halign(gtk::Align::Start);

        let actions = gtk::Stack::new();
        actions.set_hhomogeneous(false);
        actions.set_halign(gtk::Align::End);
        actions.set_hexpand(true);
        actions.add_named(&self.picture_actions(), Some("pictures"));
        actions.add_named(&self.collection_actions(), Some("collections"));
        actions.add_named(
            &gtk::Box::new(gtk::Orientation::Horizontal, 0),
            Some("colors"),
        );
        stack
            .bind_property("visible-child-name", &actions, "visible-child-name")
            .sync_create()
            .build();

        // The buttons go below the tabs when the window is narrow.
        let bar = gtk::FlowBox::new();
        bar.set_selection_mode(gtk::SelectionMode::None);
        bar.set_homogeneous(false);
        bar.set_min_children_per_line(1);
        bar.set_max_children_per_line(2);
        bar.set_column_spacing(10);
        bar.set_row_spacing(8);
        bar.add_css_class("st-wallpaper-bar");
        bar.append(&switcher);
        bar.append(&actions);
        tabs.append(&bar);
        tabs.append(&stack);
        tabs
    }

    /// _Change Folder…_ with the folder's name, and _Choose Image…_.
    fn picture_actions(self: &Rc<Self>) -> gtk::Box {
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
    fn pictures_tab(self: &Rc<Self>) -> gtk::Box {
        let tab = gtk::Box::new(gtk::Orientation::Vertical, 10);
        tab.append(&self.chips);
        tab.append(&self.pictures);
        tab.append(&self.pictures_empty);
        let weak = Rc::downgrade(self);
        self.pictures.connect_child_activated(move |_, child| {
            let Some(ui) = weak.upgrade() else { return };
            let picture =
                index(child).and_then(|index| ui.shown_pictures.borrow().get(index).cloned());
            if let Some(picture) = picture {
                ui.pick_picture(&picture);
            }
        });
        tab
    }

    /// _New Collection…_: a name, then _Create_.
    fn collection_actions(self: &Rc<Self>) -> gtk::MenuButton {
        let button = gtk::MenuButton::new();
        button.set_label(tr(msg!("New Collection…")));
        let popover = gtk::Popover::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.set_margin_top(6);
        content.set_margin_bottom(6);
        content.set_margin_start(6);
        content.set_margin_end(6);
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some(tr(msg!("Name, e.g. Nature"))));
        entry.update_property(&[gtk::accessible::Property::Label(tr(msg!(
            "Name of the collection"
        )))]);
        let hint = label(style::DIM);
        let create = gtk::Button::with_label(tr(msg!("Create")));
        create.add_css_class("suggested-action");
        create.set_sensitive(false);
        content.append(&entry);
        content.append(&hint);
        content.append(&create);
        popover.set_child(Some(&content));
        button.set_popover(Some(&popover));

        let weak = Rc::downgrade(self);
        let check = move |entry: &gtk::Entry| -> Option<String> {
            let ui = weak.upgrade()?;
            let name = entry.text().trim().to_owned();
            let taken = ui
                .model
                .state()
                .is_some_and(|state| Collection::names(&state).contains(&name));
            match (name.is_empty(), wallpaper::is_collection_name(&name), taken) {
                (true, ..) => None,
                (false, false, _) => {
                    hint.set_text(tr(msg!("At most 64 characters, on one line")));
                    None
                }
                (false, true, true) => {
                    hint.set_text(tr(msg!("A collection with this name exists")));
                    None
                }
                (false, true, false) => {
                    hint.set_text("");
                    Some(name)
                }
            }
        };
        let check = Rc::new(check);
        entry.connect_changed({
            let check = Rc::clone(&check);
            let create = create.clone();
            move |entry| create.set_sensitive(check(entry).is_some())
        });
        let weak = Rc::downgrade(self);
        let make = {
            let entry = entry.clone();
            let popover = popover.clone();
            move || {
                let (Some(ui), Some(name)) = (weak.upgrade(), check(&entry)) else {
                    return;
                };
                popover.popdown();
                entry.set_text("");
                ui.model.set(PICTURES, Some(&name), Value::List(Vec::new()));
                ui.open.replace(Some(name));
            }
        };
        let make = Rc::new(make);
        entry.connect_activate({
            let make = Rc::clone(&make);
            move |_| make()
        });
        create.connect_clicked(move |_| make());
        button
    }

    /// The collections, and the settings and pictures of the open one.
    fn collections_tab(self: &Rc<Self>) -> gtk::Box {
        let tab = gtk::Box::new(gtk::Orientation::Vertical, 14);
        tab.append(&self.collections);
        tab.append(&self.collections_empty);
        tab.append(&self.editor.card);
        let weak = Rc::downgrade(self);
        self.collections.connect_child_activated(move |_, child| {
            let Some(ui) = weak.upgrade() else { return };
            let name =
                index(child).and_then(|index| ui.shown_collections.borrow().get(index).cloned());
            if let Some(name) = name {
                ui.pick_collection(&name);
            }
        });
        self.connect_editor();
        tab
    }

    /// The colors to pick, and _Other…_.
    fn colors_tab(self: &Rc<Self>) -> gtk::Box {
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

    /// Connects the controls of the collection editor.
    fn connect_editor(self: &Rc<Self>) {
        let editor = &self.editor;
        for (button, id) in editor.changes.iter().zip(["login", "daily", "interval"]) {
            let weak = Rc::downgrade(self);
            button.connect_toggled(move |button| {
                if let (Some(ui), true) = (weak.upgrade(), button.is_active()) {
                    ui.set_open(CHANGE, Value::Choice(id.to_owned()));
                }
            });
        }
        for (button, id) in editor.orders.iter().zip(["shuffle", "in_order"]) {
            let weak = Rc::downgrade(self);
            button.connect_toggled(move |button| {
                if let (Some(ui), true) = (weak.upgrade(), button.is_active()) {
                    ui.set_open(ORDER, Value::Choice(id.to_owned()));
                }
            });
        }
        let weak = Rc::downgrade(self);
        let interval = move || {
            if let Some(ui) = weak.upgrade() {
                let minutes = ui.editor.minutes();
                ui.set_open(INTERVAL, Value::Integer(minutes));
            }
        };
        let interval = Rc::new(interval);
        editor.interval.connect_value_changed({
            let interval = Rc::clone(&interval);
            move |_| interval()
        });
        editor.unit.connect_selected_notify(move |_| interval());
        let weak = Rc::downgrade(self);
        editor.members.connect_child_activated(move |_, child| {
            let Some(ui) = weak.upgrade() else { return };
            let entry =
                index(child).and_then(|index| ui.editor.entries.borrow().get(index).cloned());
            if let Some(entry) = entry {
                ui.remove_entry(&entry);
            }
        });
    }

    /// The output whose values are edited; `None` for every output.
    fn target(&self) -> Option<String> {
        if self.is_separate() {
            self.selected.borrow().clone()
        } else {
            None
        }
    }

    /// Whether the outputs are edited one by one: as the user switched, else whether an output
    /// has a wallpaper of its own.
    fn is_separate(&self) -> bool {
        self.separate.get().unwrap_or_else(|| {
            self.model
                .state()
                .is_some_and(|state| !own_outputs(&state).is_empty())
        })
    }

    /// Stores `value` for `key` of the edited output.
    fn set(&self, key: &'static str, value: Value) {
        self.error.set_visible(false);
        let target = self.target();
        self.model.set(key, target.as_deref(), value);
    }

    /// Stores `value` for `key` of the open collection.
    fn set_open(&self, key: &'static str, value: Value) {
        if self.syncing.get() {
            return;
        }
        let open = self.open.borrow().clone();
        if let Some(name) = open {
            self.error.set_visible(false);
            self.model.set(key, Some(&name), value);
        }
    }

    /// What `output` shows (`None`: every output).
    fn look(&self, output: Option<&str>) -> Look {
        let text = |key| match self.value(key, output) {
            Some(Value::Text(text)) => text,
            _ => String::new(),
        };
        let collection = text(COLLECTION);
        if !collection.is_empty() {
            // The link of the output's own collection, else of every output's.
            let own = self
                .model
                .state()
                .and_then(|state| wallpaper::shown_on(&state, output))
                .is_some();
            let link = wallpaper::link(if own { output } else { None });
            let current = link.as_deref().and_then(wallpaper::current);
            return Look::Collection(collection, current);
        }
        let image = text(IMAGE);
        if image.is_empty() {
            Look::Color
        } else {
            Look::Picture(image)
        }
    }

    /// The value of `key` in effect for `output`.
    fn value(&self, key: &str, output: Option<&str>) -> Option<Value> {
        let setting = Registry::builtin().get(key)?;
        Some(self.model.view(setting, output)?.value)
    }

    /// The color in effect for `output`.
    fn color_value(&self, output: Option<&str>) -> Color {
        match self.value(COLOR, output) {
            Some(Value::Color(color)) => color,
            _ => Color::rgb(0, 0, 0),
        }
    }

    /// Brings every part to the model.
    fn refresh(self: &Rc<Self>) {
        self.syncing.set(true);
        self.refresh_notice();
        let separate = self.is_separate();
        self.same.set_active(!separate);
        let outputs = self.outputs();
        if separate {
            let selected = self.selected.borrow().clone();
            if !selected
                .is_some_and(|picked| outputs.iter().any(|output| output.identifier() == picked))
            {
                self.selected
                    .replace(outputs.first().map(|output| output.identifier().to_owned()));
            }
        }
        let target = self.target();
        self.refresh_preview(&outputs, separate);
        self.refresh_current(target.as_deref());
        self.refresh_folder();
        self.select_picture(target.as_deref());
        self.refresh_collections(target.as_deref());
        self.syncing.set(false);
    }

    /// The outputs to show: the connected ones, else those with stored values.
    fn outputs(&self) -> Vec<Output> {
        let all: Vec<Output> = self
            .model
            .outputs()
            .map(|outputs| outputs.iter().cloned().collect())
            .unwrap_or_default();
        let connected: Vec<Output> = all
            .iter()
            .filter(|output| output.is_connected())
            .cloned()
            .collect();
        if connected.is_empty() { all } else { connected }
    }

    fn refresh_notice(&self) {
        while let Some(child) = self.notice.first_child() {
            self.notice.remove(&child);
        }
        let found = self
            .model
            .status()
            .map(|status| wallpaper::starters(status.config()))
            .unwrap_or_default();
        self.notice.set_visible(!found.is_empty());
        // One notice per program, naming every line that starts it.
        let mut programs: Vec<(String, Vec<wallpaper::External>)> = Vec::new();
        for external in found {
            let program = program_of(&external.starter);
            match programs.iter_mut().find(|(other, _)| *other == program) {
                Some((_, lines)) => lines.push(external),
                None => programs.push((program, vec![external])),
            }
        }
        for (program, lines) in &programs {
            self.notice
                .append(&starter_row(&self.model, program, lines));
        }
    }

    /// The tiles of the displays, placed like the output layout.
    fn refresh_preview(self: &Rc<Self>, outputs: &[Output], separate: bool) {
        while let Some(child) = self.preview.first_child() {
            self.preview.remove(&child);
        }
        let placed = place(outputs);
        let selected = self.selected.borrow().clone();
        for (output, (x, y, width, height)) in placed {
            let name = output.map(|output| output.identifier().to_owned());
            let shows = if separate { name.as_deref() } else { None };
            let tile = self.tile(output, shows, width, height);
            let editing = separate && name.is_some() && name == selected;
            if editing {
                tile.add_css_class("st-editing");
            }
            if separate {
                if let Some(name) = name {
                    let weak = Rc::downgrade(self);
                    tile.connect_clicked(move |_| {
                        if let Some(ui) = weak.upgrade() {
                            ui.selected.replace(Some(name.clone()));
                            ui.refresh();
                        }
                    });
                }
            } else {
                tile.set_can_target(false);
            }
            self.preview.put(&tile, x, y);
        }
    }

    /// The tile of `output` (`None`: every output) with the wallpaper of `shows`.
    fn tile(
        &self,
        output: Option<&Output>,
        shows: Option<&str>,
        width: i32,
        height: i32,
    ) -> gtk::Button {
        let picture = gtk::Picture::new();
        picture.set_size_request(width, height);
        picture.set_can_shrink(true);
        picture.add_css_class("st-wallpaper-screen");
        let color = self.color_value(shows);
        picture.set_paintable(Some(&solid(color)));
        // Stretched, so the one pixel stays inside the tile.
        picture.set_content_fit(gtk::ContentFit::Fill);
        // How swaybg puts the picture on the screen: scaled, or at its own size.
        let fill = match self.value(FILL, shows) {
            Some(Value::Choice(fill)) if fill == "fit" => Ok(gtk::ContentFit::Contain),
            Some(Value::Choice(fill)) if fill == "stretch" => Ok(gtk::ContentFit::Fill),
            Some(Value::Choice(fill)) if fill == "center" => Err(false),
            Some(Value::Choice(fill)) if fill == "tile" => Err(true),
            _ => Ok(gtk::ContentFit::Cover),
        };
        // Without a session, a common screen.
        let pixels = output
            .and_then(Output::pixels)
            .and_then(|(width, height)| {
                Some((i32::try_from(width).ok()?, i32::try_from(height).ok()?))
            })
            .unwrap_or((1920, 1080));
        let path = match self.look(shows) {
            Look::Picture(image) => Some(wallpaper::expand(&image, Some(&glib::home_dir()))),
            Look::Collection(_, current) => current,
            Look::Color => None,
        };
        let screen = gtk::Overlay::new();
        screen.set_child(Some(&picture));
        screen.set_overflow(gtk::Overflow::Hidden);
        screen.add_css_class("st-wallpaper-screen");
        match (path, fill) {
            (Some(path), Ok(fit)) => {
                let image = gtk::Picture::new();
                image.set_content_fit(fit);
                image.set_can_shrink(true);
                thumbnail_into(&image, &path);
                screen.add_overlay(&image);
            }
            (Some(path), Err(repeat)) => {
                let area = gtk::DrawingArea::new();
                own_size_into(&area, &path, pixels, repeat);
                screen.add_overlay(&area);
            }
            (None, _) => {}
        }
        if let Look::Collection(name, _) = self.look(shows) {
            let badge = gtk::Label::new(Some(&name));
            badge.add_css_class("st-wallpaper-badge");
            badge.set_halign(gtk::Align::Start);
            badge.set_valign(gtk::Align::Start);
            badge.set_ellipsize(gtk::pango::EllipsizeMode::End);
            screen.add_overlay(&badge);
        }
        let name = match output {
            Some(output) if output.label() != output.name() => {
                format!("{} · {}", output.label(), output.name())
            }
            Some(output) => output.name().to_owned(),
            None => tr(msg!("All displays")).to_owned(),
        };
        let caption = gtk::Label::new(Some(&name));
        caption.add_css_class("st-wallpaper-caption");
        caption.set_ellipsize(gtk::pango::EllipsizeMode::End);
        caption.set_max_width_chars(1);
        caption.set_hexpand(false);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
        content.append(&screen);
        content.append(&caption);
        let tile = gtk::Button::new();
        tile.set_child(Some(&content));
        tile.add_css_class("flat");
        tile.add_css_class("st-wallpaper-tile");
        tile.set_tooltip_text(Some(&name));
        tile.update_property(&[gtk::accessible::Property::Label(&name)]);
        tile
    }

    /// The name and details of what the edited output shows, the fit and the color.
    fn refresh_current(&self, target: Option<&str>) {
        let look = self.look(target);
        let color = self.color_value(target);
        let (title, detail) = match &look {
            Look::Picture(image) => {
                let path = Path::new(image);
                let name = path
                    .file_name()
                    .map_or_else(|| image.clone(), |name| name.to_string_lossy().into_owned());
                let folder = path
                    .parent()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_default();
                (name, folder)
            }
            Look::Collection(name, current) => {
                let picture = current
                    .as_ref()
                    .and_then(|current| current.file_name())
                    .map(|name| name.to_string_lossy().into_owned());
                let title = match picture {
                    Some(picture) => format!("{name} · {picture}"),
                    None => name.clone(),
                };
                let detail = self
                    .model
                    .state()
                    .map(|state| describe(&Collection::load(&state, name)))
                    .unwrap_or_default();
                (title, detail)
            }
            Look::Color => (tr(msg!("Solid color")).to_owned(), color.to_string()),
        };
        self.title.set_text(&title);
        self.detail.set_text(&detail);
        self.detail.set_visible(!detail.is_empty());
        self.next.set_visible(matches!(look, Look::Collection(..)));
        let fill = match self.value(FILL, target) {
            Some(Value::Choice(fill)) => fill,
            _ => "fill".to_owned(),
        };
        for (button, (id, _)) in self.fits.iter().zip(FITS) {
            button.set_active(id == fill);
            button.set_sensitive(!matches!(look, Look::Color));
        }
        self.color.set_rgba(&rgba(color));
    }

    /// The folder whose pictures are offered: the preference, else `~/Pictures`.
    fn folder(&self) -> PathBuf {
        let preference = Registry::builtin()
            .get(FOLDER)
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
    fn refresh_folder(self: &Rc<Self>) {
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
    fn fill_chips(self: &Rc<Self>) {
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
    fn fill_pictures(self: &Rc<Self>) {
        self.pictures.remove_all();
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
        let target = self.target();
        self.select_picture(target.as_deref());
    }

    /// Selects the picture `target` shows in the grid.
    fn select_picture(&self, target: Option<&str>) {
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
            .and_then(|index| i32::try_from(index).ok())
            .and_then(|index| self.pictures.child_at_index(index))
        {
            Some(child) => self.pictures.select_child(&child),
            None => self.pictures.unselect_all(),
        }
    }

    /// The collections, selected the one `target` shows, and the editor of the open one.
    fn refresh_collections(self: &Rc<Self>, target: Option<&str>) {
        let Some(state) = self.model.state() else {
            return;
        };
        let names = Collection::names(&state);
        let in_use = match self.look(target) {
            Look::Collection(name, _) => Some(name),
            Look::Picture(_) | Look::Color => None,
        };
        {
            let mut open = self.open.borrow_mut();
            if !open.as_ref().is_some_and(|open| names.contains(open)) {
                *open = in_use.clone().filter(|name| names.contains(name));
            }
        }
        if *self.shown_collections.borrow() != names || self.collections.first_child().is_none() {
            self.collections.remove_all();
            for name in &names {
                let collection = Collection::load(&state, name);
                self.collections.append(&collection_card(&collection));
            }
            self.shown_collections.replace(names.clone());
        } else {
            // The details below each name may have changed.
            for (index, name) in names.iter().enumerate() {
                let child = i32::try_from(index)
                    .ok()
                    .and_then(|index| self.collections.child_at_index(index));
                if let Some(child) = child {
                    child.set_child(Some(&collection_card(&Collection::load(&state, name))));
                }
            }
        }
        self.collections_empty.set_visible(names.is_empty());
        let index = in_use
            .and_then(|name| names.iter().position(|other| *other == name))
            .and_then(|index| i32::try_from(index).ok())
            .and_then(|index| self.collections.child_at_index(index));
        match index {
            Some(child) => self.collections.select_child(&child),
            None => self.collections.unselect_all(),
        }
        let open = self.open.borrow().clone();
        match open {
            Some(name) => {
                self.editor.show(&Collection::load(&state, &name));
                self.editor.card.set_visible(true);
            }
            None => self.editor.card.set_visible(false),
        }
    }

    /// Shows `picture` on the edited output.
    fn pick_picture(&self, picture: &Path) {
        self.set(IMAGE, Value::Text(tilde(picture)));
        self.leave_collection();
    }

    /// Shows the collection `name` on the edited output, and opens it.
    fn pick_collection(self: &Rc<Self>, name: &str) {
        self.open.replace(Some(name.to_owned()));
        self.set(COLLECTION, Value::Text(name.to_owned()));
        self.refresh();
    }

    /// Fills the edited output with `color` alone.
    fn pick_color(&self, color: Color) {
        self.set(COLOR, Value::Color(color));
        let target = self.target();
        if matches!(self.value(IMAGE, target.as_deref()), Some(Value::Text(image)) if !image.is_empty())
        {
            self.set(IMAGE, Value::Text(String::new()));
        }
        self.leave_collection();
    }

    /// Stops showing a collection on the edited output, if it does.
    fn leave_collection(&self) {
        let target = self.target();
        if matches!(self.look(target.as_deref()), Look::Collection(..)) {
            self.set(COLLECTION, Value::Text(String::new()));
        }
    }

    /// Switches between editing every output and one output at a time. Back to every output,
    /// the values of single outputs go, after asking.
    fn switch_same(self: &Rc<Self>, same: bool) {
        if self.syncing.get() {
            return;
        }
        if !same {
            self.separate.set(Some(true));
            self.refresh();
            return;
        }
        let own = self
            .model
            .state()
            .map(|state| own_outputs(&state))
            .unwrap_or_default();
        if own.is_empty() {
            self.separate.set(Some(false));
            self.refresh();
            return;
        }
        let dialog = gtk::AlertDialog::builder()
            .message(tr(msg!("Use the same wallpaper on all displays?")))
            .detail(tr(msg!(
                "The wallpapers set for single displays are removed, and every display shows the \
                 wallpaper set for all of them."
            )))
            .buttons([tr(msg!("Cancel")), tr(msg!("Use the Same Wallpaper"))])
            .cancel_button(0)
            .default_button(1)
            .modal(true)
            .build();
        let window = self.body.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(self);
        dialog.choose(window.as_ref(), gio::Cancellable::NONE, move |answer| {
            let Some(ui) = weak.upgrade() else { return };
            if answer == Ok(1) {
                for (key, output) in &own {
                    ui.model.reset(key, Some(output));
                }
                ui.separate.set(Some(false));
            }
            ui.refresh();
        });
    }

    /// Shows the next picture of the edited output's collection.
    fn next_picture(self: &Rc<Self>) {
        let Some(state) = self.model.state() else {
            return;
        };
        let target = self.target();
        // An output without a collection of its own shows the one of every output.
        let output = target.filter(|output| wallpaper::shown_on(&state, Some(output)).is_some());
        let weak = Rc::downgrade(self);
        self.next.set_sensitive(false);
        glib::spawn_future_local(async move {
            let mut client = Client::connect().await.ok();
            let result =
                wallpaper::next(client.as_mut(), &state, output.as_deref(), Moment::Now).await;
            let Some(ui) = weak.upgrade() else { return };
            ui.next.set_sensitive(true);
            if let Err(error) = result {
                ui.error.set_text(&error.text().translated());
                ui.error.set_visible(true);
            }
            ui.refresh();
        });
    }

    /// Starts the timers of the collections again when what they change or when changed.
    fn follow_timers(&self) {
        let Some(state) = self.model.state() else {
            return;
        };
        let now = timers(&state);
        if self.timers.borrow().as_ref() == Some(&now) {
            return;
        }
        self.timers.replace(Some(now));
        if !wallpaper::has_timers() || std::env::var_os("SWAYSOCK").is_none() {
            return;
        }
        let error = self.error.downgrade();
        glib::spawn_future_local(async move {
            if let Err(failure) = wallpaper::reschedule(&state, &wallpaper::program()).await {
                if let Some(error) = error.upgrade() {
                    error.set_text(&failure.text().translated());
                    error.set_visible(true);
                }
            }
        });
    }

    /// Asks for an image and shows it on the edited output.
    fn choose_image(self: &Rc<Self>, button: &gtk::Button) {
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
    fn choose_folder(self: &Rc<Self>, button: &gtk::Button) {
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
                ui.model.set(FOLDER, None, Value::Text(tilde(&path)));
            }
        });
    }

    /// Adds `paths` to the open collection.
    fn add_entries(&self, paths: Vec<PathBuf>) {
        let Some(name) = self.open.borrow().clone() else {
            return;
        };
        let Some(state) = self.model.state() else {
            return;
        };
        let mut entries = Collection::load(&state, &name).entries;
        for path in paths {
            let entry = tilde(&path);
            if !entries.contains(&entry) {
                entries.push(entry);
            }
        }
        self.model.set(PICTURES, Some(&name), Value::List(entries));
    }

    /// Removes `entry` from the open collection.
    fn remove_entry(&self, entry: &str) {
        let Some(name) = self.open.borrow().clone() else {
            return;
        };
        let Some(state) = self.model.state() else {
            return;
        };
        let mut entries = Collection::load(&state, &name).entries;
        entries.retain(|other| other != entry);
        self.model.set(PICTURES, Some(&name), Value::List(entries));
    }

    /// Asks for pictures to add to the open collection.
    fn add_pictures(self: &Rc<Self>, button: &gtk::Button) {
        let filter = picture_filter();
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(tr(msg!("Add Pictures")))
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .initial_folder(&gio::File::for_path(self.folder()))
            .build();
        let window = button.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(self);
        dialog.open_multiple(window.as_ref(), gio::Cancellable::NONE, move |result| {
            let (Some(ui), Ok(files)) = (weak.upgrade(), result) else {
                return;
            };
            let paths = (0..files.n_items())
                .filter_map(|index| files.item(index).and_downcast::<gio::File>())
                .filter_map(|file| file.path())
                .collect();
            ui.add_entries(paths);
        });
    }

    /// Asks for a folder to add to the open collection.
    fn add_folder(self: &Rc<Self>, button: &gtk::Button) {
        let dialog = gtk::FileDialog::builder()
            .title(tr(msg!("Add a Folder")))
            .modal(true)
            .initial_folder(&gio::File::for_path(self.folder()))
            .build();
        let window = button.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(self);
        dialog.select_folder(window.as_ref(), gio::Cancellable::NONE, move |result| {
            if let (Some(ui), Some(path)) =
                (weak.upgrade(), result.ok().and_then(|folder| folder.path()))
            {
                ui.add_entries(vec![path]);
            }
        });
    }

    /// Deletes the open collection after asking: its settings, and its use on every output.
    fn delete_open(self: &Rc<Self>) {
        let Some(name) = self.open.borrow().clone() else {
            return;
        };
        let dialog = gtk::AlertDialog::builder()
            .message(trf(
                msg!("Delete the collection {name}?"),
                &[("name", &name)],
            ))
            .detail(tr(msg!(
                "The pictures stay where they are. Displays that show the collection show their \
                 color instead."
            )))
            .buttons([tr(msg!("Cancel")), tr(msg!("Delete"))])
            .cancel_button(0)
            .default_button(0)
            .modal(true)
            .build();
        let window = self.body.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(self);
        dialog.choose(window.as_ref(), gio::Cancellable::NONE, move |answer| {
            let (Some(ui), Ok(1)) = (weak.upgrade(), answer) else {
                return;
            };
            if let Some(state) = ui.model.state() {
                for shown in wallpaper::shown(&state) {
                    if shown.collection.name == name {
                        ui.model.reset(COLLECTION, shown.output.as_deref());
                    }
                }
                for key in [PICTURES, CHANGE, INTERVAL, ORDER] {
                    if state.get(key, Some(&name)).is_some() {
                        ui.model.reset(key, Some(&name));
                    }
                }
            }
            ui.open.replace(None);
        });
    }
}

impl Editor {
    fn new() -> Self {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("st-wallpaper-editor");
        let name = label("st-wallpaper-title");
        let changes: Vec<gtk::ToggleButton> = [msg!("At login"), msg!("Daily"), msg!("Every…")]
            .into_iter()
            .map(|text| gtk::ToggleButton::with_label(tr(text)))
            .collect();
        for change in changes.iter().skip(1) {
            change.set_group(changes.first());
        }
        let interval = gtk::SpinButton::with_range(1.0, 999.0, 1.0);
        interval.update_property(&[gtk::accessible::Property::Label(tr(msg!("Interval")))]);
        let unit = gtk::DropDown::from_strings(&[
            tr(msg!("minutes")),
            tr(msg!("hours")),
            tr(msg!("days")),
        ]);
        unit.update_property(&[gtk::accessible::Property::Label(tr(msg!("Unit")))]);
        let orders: Vec<gtk::ToggleButton> = [msg!("Shuffle"), msg!("In order")]
            .into_iter()
            .map(|text| gtk::ToggleButton::with_label(tr(text)))
            .collect();
        for order in orders.iter().skip(1) {
            order.set_group(orders.first());
        }
        let count = label("st-wallpaper-subtitle");
        let members = grid(tr(msg!("Pictures of the collection")), 5);
        members.set_selection_mode(gtk::SelectionMode::None);
        Self {
            card,
            name,
            changes,
            interval,
            unit,
            orders,
            count,
            members,
            entries: RefCell::new(Vec::new()),
        }
    }

    /// The interval of the controls, in minutes.
    fn minutes(&self) -> i64 {
        let factor = match self.unit.selected() {
            1 => 60,
            2 => 24 * 60,
            _ => 1,
        };
        i64::from(self.interval.value_as_int()) * factor
    }

    /// Shows `collection`.
    fn show(&self, collection: &Collection) {
        self.name.set_text(&collection.name);
        let timers = wallpaper::has_timers();
        let active = match collection.change {
            Change::Login => 0,
            Change::Daily => 1,
            Change::Every(_) => 2,
        };
        for (index, button) in self.changes.iter().enumerate() {
            button.set_active(index == active);
            if index > 0 {
                button.set_sensitive(timers);
                if !timers {
                    button.set_tooltip_text(Some(tr(msg!(
                        "Needs systemd: without it, pictures change at login"
                    ))));
                }
            }
        }
        let minutes = match collection.change {
            Change::Every(period) => period.as_secs() / 60,
            Change::Login | Change::Daily => 30,
        };
        let (value, unit) = if minutes % (24 * 60) == 0 {
            (minutes / (24 * 60), 2)
        } else if minutes % 60 == 0 {
            (minutes / 60, 1)
        } else {
            (minutes, 0)
        };
        self.interval
            .set_value(f64::from(u32::try_from(value).unwrap_or(999)));
        self.unit.set_selected(unit);
        let every = matches!(collection.change, Change::Every(_));
        self.interval.set_sensitive(every && timers);
        self.unit.set_sensitive(every && timers);
        for (button, order) in self.orders.iter().zip([Order::Shuffle, Order::InOrder]) {
            button.set_active(collection.order == order);
        }
        let pictures = collection.pictures(Some(&glib::home_dir())).len();
        self.count.set_text(&ntrf(
            msg_plural!("{count} picture in {name}", "{count} pictures in {name}"),
            pictures,
            &[("count", &pictures), ("name", &collection.name)],
        ));
        if *self.entries.borrow() != collection.entries {
            self.members.remove_all();
            for entry in &collection.entries {
                self.members.append(&member_tile(entry));
            }
            self.entries.replace(collection.entries.clone());
        }
    }
}

impl Ui {
    /// Builds the editor card; its buttons need the page.
    fn editor_card(self: &Rc<Self>) {
        let editor = &self.editor;
        let card = &editor.card;
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        editor.name.set_hexpand(true);
        let delete = gtk::Button::with_label(tr(msg!("Delete Collection…")));
        delete.add_css_class("destructive-action");
        head.append(&editor.name);
        head.append(&delete);
        card.append(&row(&head));

        let change = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let change_text = text_block(
            tr(msg!("Change picture")),
            tr(msg!("Also when sway starts")),
        );
        let changes = linked(&editor.changes);
        changes.update_property(&[gtk::accessible::Property::Label(tr(msg!("Change picture")))]);
        let interval = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        interval.append(&editor.interval);
        interval.append(&editor.unit);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        controls.append(&changes);
        controls.append(&interval);
        change.append(&change_text);
        change.append(&controls);
        card.append(&row(&change));

        let order = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let order_text = text_block(tr(msg!("Order")), "");
        let orders = linked(&editor.orders);
        orders.update_property(&[gtk::accessible::Property::Label(tr(msg!("Order")))]);
        order.append(&order_text);
        order.append(&orders);
        card.append(&row(&order));

        let members = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        editor.count.set_hexpand(true);
        let add_pictures = gtk::Button::with_label(tr(msg!("Add Pictures…")));
        let add_folder = gtk::Button::with_label(tr(msg!("Add Folder…")));
        head.append(&editor.count);
        head.append(&add_pictures);
        head.append(&add_folder);
        let hint = label(style::DIM);
        hint.set_text(tr(msg!(
            "Click a picture or folder to remove it from the collection."
        )));
        members.append(&head);
        members.append(&editor.members);
        members.append(&hint);
        card.append(&row(&members));

        let weak = Rc::downgrade(self);
        delete.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.delete_open();
            }
        });
        let weak = Rc::downgrade(self);
        add_pictures.connect_clicked(move |button| {
            if let Some(ui) = weak.upgrade() {
                ui.add_pictures(button);
            }
        });
        let weak = Rc::downgrade(self);
        add_folder.connect_clicked(move |button| {
            if let Some(ui) = weak.upgrade() {
                ui.add_folder(button);
            }
        });
    }
}

/// A label that wraps, aligned to the start, with `class`.
fn label(class: &str) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class(class);
    label
}

/// A title with a dim line below it; the line is left out when empty.
fn text_block(title: &str, subtitle: &str) -> gtk::Box {
    let block = gtk::Box::new(gtk::Orientation::Vertical, 2);
    block.set_hexpand(true);
    block.set_valign(gtk::Align::Center);
    let heading = label("st-wallpaper-subtitle");
    heading.set_text(title);
    block.append(&heading);
    if !subtitle.is_empty() {
        let line = label(style::DIM);
        line.set_text(subtitle);
        block.append(&line);
    }
    block
}

/// A row of the editor card around `content`.
fn row(content: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 0);
    row.add_css_class("st-wallpaper-row");
    row.append(content);
    row
}

/// `buttons` side by side as one control.
fn linked(buttons: &[gtk::ToggleButton]) -> gtk::Box {
    let linked = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    linked.add_css_class("linked");
    linked.set_valign(gtk::Align::Center);
    for button in buttons {
        linked.append(button);
    }
    linked
}

/// A grid of tiles, named `name` for screen readers.
fn grid(name: &str, per_line: u32) -> gtk::FlowBox {
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::Single);
    grid.set_homogeneous(true);
    grid.set_max_children_per_line(per_line);
    grid.set_min_children_per_line(2);
    grid.set_row_spacing(10);
    grid.set_column_spacing(10);
    grid.set_activate_on_single_click(true);
    grid.set_valign(gtk::Align::Start);
    grid.add_css_class("st-wallpaper-grid");
    grid.update_property(&[gtk::accessible::Property::Label(name)]);
    grid
}

/// The position of `child` in its grid.
fn index(child: &gtk::FlowBoxChild) -> Option<usize> {
    usize::try_from(child.index()).ok()
}

/// Where the tiles of `outputs` go in the preview: their layout, scaled down, with room for the
/// names below. Outputs without a place sit side by side; without outputs, one tile stands for
/// all of them.
fn place(outputs: &[Output]) -> Placed<'_> {
    let rects: Vec<(&Output, Rect)> = outputs
        .iter()
        .filter_map(|output| Some((output, output.rect()?)))
        .collect();
    if rects.is_empty() {
        let count = outputs.len().max(1);
        let width = (PREVIEW.0 / float(count) - 16.0).min(220.0);
        let height = width * 10.0 / 16.0;
        if outputs.is_empty() {
            return vec![(None, (0.0, 0.0, px(width), px(height)))];
        }
        return outputs
            .iter()
            .enumerate()
            .map(|(index, output)| {
                (
                    Some(output),
                    ((width + 16.0) * float(index), 0.0, px(width), px(height)),
                )
            })
            .collect();
    }
    let left = rects.iter().map(|(_, rect)| rect.x).min().unwrap_or(0);
    let top = rects.iter().map(|(_, rect)| rect.y).min().unwrap_or(0);
    let right = rects
        .iter()
        .map(|(_, rect)| rect.x + rect.width)
        .max()
        .unwrap_or(1);
    let bottom = rects
        .iter()
        .map(|(_, rect)| rect.y + rect.height)
        .max()
        .unwrap_or(1);
    let scale = (PREVIEW.0 / f64::from((right - left).max(1)))
        .min(PREVIEW.1 / f64::from((bottom - top).max(1)));
    // A gap between outputs that touch, so the tiles stand apart.
    let gap = 12.0;
    let columns: Vec<i32> = {
        let mut columns: Vec<i32> = rects.iter().map(|(_, rect)| rect.x).collect();
        columns.sort_unstable();
        columns.dedup();
        columns
    };
    let rows: Vec<i32> = {
        let mut rows: Vec<i32> = rects.iter().map(|(_, rect)| rect.y).collect();
        rows.sort_unstable();
        rows.dedup();
        rows
    };
    rects
        .into_iter()
        .map(|(output, rect)| {
            let column = float(columns.iter().position(|x| *x == rect.x).unwrap_or(0));
            let row = float(rows.iter().position(|y| *y == rect.y).unwrap_or(0));
            let x = f64::from(rect.x - left) * scale + column * gap;
            let y = f64::from(rect.y - top) * scale + row * (gap + f64::from(NAME_HEIGHT));
            let width = px(f64::from(rect.width) * scale);
            let height = px(f64::from(rect.height) * scale);
            (Some(output), (x, y, width.max(24), height.max(16)))
        })
        .collect()
}

/// `value` rounded to whole pixels.
#[allow(
    clippy::cast_possible_truncation,
    reason = "sizes in the preview, far below the limits of i32"
)]
fn px(value: f64) -> i32 {
    value.round() as i32
}

/// `count` as a number to compute sizes with.
fn float(count: usize) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

/// The outputs with a wallpaper value of their own: each key and output.
fn own_outputs(state: &State) -> Vec<(&'static str, String)> {
    state
        .entries()
        .into_iter()
        .filter(|entry| entry.setting.key().starts_with("wallpaper."))
        .filter_map(|entry| Some((entry.setting.key(), entry.device?.to_owned())))
        .collect()
}

/// The outputs whose collection changes by a timer, with when.
fn timers(state: &State) -> Timers {
    wallpaper::shown(state)
        .into_iter()
        .map(|shown| (shown.output, shown.collection.change))
        .collect()
}

/// "12 pictures · every 30 minutes · shuffled", for a collection.
fn describe(collection: &Collection) -> String {
    let count = collection.pictures(Some(&glib::home_dir())).len();
    let pictures = ntrf(
        msg_plural!("{count} picture", "{count} pictures"),
        count,
        &[("count", &count)],
    );
    let order = match collection.order {
        Order::Shuffle => tr(msg!("shuffled")),
        Order::InOrder => tr(msg!("in order")),
    };
    format!("{pictures} · {} · {order}", when(collection.change))
}

/// When a collection changes, short.
fn when(change: Change) -> String {
    match change {
        Change::Login => tr(msg!("at login")).to_owned(),
        Change::Daily => tr(msg!("daily")).to_owned(),
        Change::Every(period) => {
            let minutes = usize::try_from(period.as_secs() / 60).unwrap_or(1);
            if minutes % (24 * 60) == 0 {
                let days = minutes / (24 * 60);
                ntrf(
                    msg_plural!("every {count} day", "every {count} days"),
                    days,
                    &[("count", &days)],
                )
            } else if minutes % 60 == 0 {
                let hours = minutes / 60;
                ntrf(
                    msg_plural!("every {count} hour", "every {count} hours"),
                    hours,
                    &[("count", &hours)],
                )
            } else {
                ntrf(
                    msg_plural!("every {count} minute", "every {count} minutes"),
                    minutes,
                    &[("count", &minutes)],
                )
            }
        }
    }
}

/// The card of a collection in the grid: its first picture, name and details.
fn collection_card(collection: &Collection) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let cover = gtk::Overlay::new();
    let back = gtk::Picture::for_paintable(&solid(Color::rgb(0x80, 0x80, 0x80)));
    back.set_size_request(TILE.0 + 24, TILE.1 + 16);
    back.set_content_fit(gtk::ContentFit::Fill);
    back.add_css_class("st-wallpaper-stack");
    cover.set_child(Some(&back));
    let pictures = collection.pictures(Some(&glib::home_dir()));
    if let Some(first) = pictures.first() {
        let front = gtk::Picture::new();
        front.set_content_fit(gtk::ContentFit::Cover);
        front.set_can_shrink(true);
        front.set_margin_end(10);
        front.set_margin_bottom(8);
        front.add_css_class("st-wallpaper-thumbnail");
        thumbnail_into(&front, first);
        cover.add_overlay(&front);
    }
    let name = gtk::Label::new(Some(&collection.name));
    name.add_css_class("st-wallpaper-subtitle");
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let count = pictures.len();
    let details = gtk::Label::new(Some(&format!(
        "{} · {}",
        ntrf(
            msg_plural!("{count} picture", "{count} pictures"),
            count,
            &[("count", &count)]
        ),
        when(collection.change)
    )));
    details.add_css_class(style::DIM);
    details.add_css_class("st-wallpaper-caption");
    details.set_xalign(0.0);
    details.set_ellipsize(gtk::pango::EllipsizeMode::End);
    card.append(&cover);
    card.append(&name);
    card.append(&details);
    card.update_property(&[gtk::accessible::Property::Label(&collection.name)]);
    card
}

/// A picture of the folder in the grid, with its name below.
fn picture_tile(picture: &Path) -> gtk::Box {
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let image = gtk::Picture::new();
    image.set_size_request(TILE.0, TILE.1);
    image.set_content_fit(gtk::ContentFit::Cover);
    image.set_can_shrink(true);
    image.add_css_class("st-wallpaper-thumbnail");
    let name = picture
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    image.set_alternative_text(Some(&name));
    thumbnail_into(&image, picture);
    let caption = gtk::Label::new(Some(&name));
    caption.add_css_class(style::DIM);
    caption.add_css_class("st-wallpaper-caption");
    caption.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    caption.set_max_width_chars(1);
    caption.set_xalign(0.0);
    tile.append(&image);
    tile.append(&caption);
    tile.set_tooltip_text(Some(&name));
    tile
}

/// A picture or folder of a collection in the editor, with a remove mark.
fn member_tile(entry: &str) -> gtk::Overlay {
    let path = wallpaper::expand(entry, Some(&glib::home_dir()));
    let name = path.file_name().map_or_else(
        || entry.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let tile = gtk::Overlay::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    // 16:10, as wide as the grid makes it.
    let frame = gtk::AspectFrame::new(0.5, 0.5, 16.0 / 10.0, false);
    frame.set_size_request(TILE.0, TILE.1);
    if path.is_dir() {
        let icon = gtk::Image::from_icon_name("folder-symbolic");
        icon.set_pixel_size(40);
        icon.add_css_class("st-wallpaper-folder");
        frame.set_child(Some(&icon));
    } else {
        let image = gtk::Picture::new();
        image.set_content_fit(gtk::ContentFit::Cover);
        image.set_can_shrink(true);
        image.add_css_class("st-wallpaper-thumbnail");
        thumbnail_into(&image, &path);
        frame.set_child(Some(&image));
    }
    content.append(&frame);
    let caption = gtk::Label::new(Some(&name));
    caption.add_css_class(style::DIM);
    caption.add_css_class("st-wallpaper-caption");
    caption.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    caption.set_max_width_chars(1);
    content.append(&caption);
    tile.set_child(Some(&content));
    let remove = gtk::Image::from_icon_name("window-close-symbolic");
    remove.add_css_class("st-wallpaper-remove");
    remove.set_halign(gtk::Align::End);
    remove.set_valign(gtk::Align::Start);
    tile.add_overlay(&remove);
    let label = trf(
        msg!("Remove {name} from the collection"),
        &[("name", &name)],
    );
    tile.set_tooltip_text(Some(&label));
    tile.update_property(&[gtk::accessible::Property::Label(&label)]);
    tile
}

/// The filter of the picture dialogs.
fn picture_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(tr(msg!("Images"))));
    for extension in EXTENSIONS {
        filter.add_suffix(extension);
    }
    filter
}

/// The pictures of `folder` and of its subfolders, one level down, each with the name of its
/// subfolder; at most [`MAX_PICTURES`].
fn list_folder(folder: &Path) -> Result<Listing, String> {
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

thread_local! {
    /// The thumbnails read so far, by picture; `None` for one that can't be read.
    static THUMBNAILS: RefCell<HashMap<PathBuf, Option<gdk::Texture>>> = RefCell::new(HashMap::new());
}

/// Shows the thumbnail of `path` in `image`: at once if it was read before, else once it is.
fn thumbnail_into(image: &gtk::Picture, path: &Path) {
    let known = THUMBNAILS.with(|cache| cache.borrow().get(path).cloned());
    if let Some(texture) = known {
        image.set_paintable(texture.as_ref());
        return;
    }
    let weak = image.downgrade();
    let path = path.to_path_buf();
    glib::spawn_future_local(async move {
        let read = {
            let path = path.clone();
            gio::spawn_blocking(move || pixels(&path)).await
        };
        let texture = read.ok().flatten().map(|pixels| texture(&pixels));
        THUMBNAILS.with(|cache| cache.borrow_mut().insert(path, texture.clone()));
        if let Some(image) = weak.upgrade() {
            image.set_paintable(texture.as_ref());
        }
    });
}

/// Draws `path` in `area` at its own size on a screen of `pixels`, as swaybg does for `center`
/// (in the middle) and `tile` (`repeat`, from the top left), scaled down with the screen.
fn own_size_into(area: &gtk::DrawingArea, path: &Path, pixels: (i32, i32), repeat: bool) {
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

/// A texture of one pixel in `color`, which a picture stretches over its size.
fn solid(color: Color) -> gdk::Texture {
    let bytes = glib::Bytes::from_owned(color.channels());
    gdk::MemoryTexture::new(1, 1, gdk::MemoryFormat::R8g8b8, &bytes, 3).upcast()
}

/// `color` for GTK.
fn rgba(color: Color) -> gdk::RGBA {
    let [red, green, blue] = color.channels();
    gdk::RGBA::new(
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        1.0,
    )
}

/// The color of `rgba`, without its transparency.
fn color_of(rgba: &gdk::RGBA) -> Color {
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

/// The program a line starts, as the notice names it.
fn program_of(starter: &Starter) -> String {
    match starter {
        Starter::Tool(tool) => (*tool).to_owned(),
        Starter::Script(script) => script.clone(),
    }
}

/// The notice about the lines of the user's config that start the wallpaper tool `program`,
/// with a button that opens the first of them in the editor.
fn starter_row(model: &Model, program: &str, lines: &[wallpaper::External]) -> gtk::Box {
    // `~/.config/sway/config:14, 15`: the file once for lines in the same file.
    let mut place = String::new();
    let mut file = None;
    for external in lines {
        let this = &external.directive.file;
        if file == Some(this) {
            let _ = write!(place, ", {}", external.directive.line);
        } else {
            if !place.is_empty() {
                place.push_str(", ");
            }
            let _ = write!(place, "{}:{}", tilde(this), external.directive.line);
            file = Some(this);
        }
    }
    let text = gtk::Label::new(Some(&trf(
        // TRANSLATORS: {program} is a wallpaper tool such as swww-daemon, {place} a file and line
        // such as ~/.config/sway/config:12.
        msg!(
            "Your config starts {program} ({place}). It sets a wallpaper of its own, which may \
             cover the one set here. To use this page, remove that line."
        ),
        &[("program", &program), ("place", &place)],
    )));
    let Some(external) = lines.first() else {
        return gtk::Box::new(gtk::Orientation::Horizontal, 12);
    };
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
    let weak_model = model.downgrade();
    let weak_text = text.downgrade();
    open.connect_clicked(move |button| {
        let Some(model) = weak_model.upgrade() else {
            return;
        };
        let weak_text = weak_text.clone();
        crate::editor::open(button, Some(&model), &file, Some(line), move |message| {
            if let (Some(message), Some(text)) = (message, weak_text.upgrade()) {
                text.set_text(message);
            }
        });
    });
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    icon.set_valign(gtk::Align::Start);
    row.append(&icon);
    row.append(&text);
    row.append(&open);
    row
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
