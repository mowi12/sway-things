// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Wallpaper page.
//!
//! - A notice for each line of the user's config that starts a wallpaper tool or script
//!   ([`wallpaper::starters`]), which may draw over the background set here.
//! - The preview: every display as a tile with its wallpaper, placed like the output layout.
//!   _Same wallpaper on all displays_ edits the values for every output; switched off, a click on
//!   a tile picks the display whose values are edited.
//! - What the edited display shows (a picture, a collection or a color) and where it comes from,
//!   _Next Picture_ for a collection, _Use Default_, the fit and the background color.
//! - Three tabs to pick from: the pictures of the wallpaper folder (`app.wallpaper_folder`, else
//!   `~/Pictures`) with its subfolders as chips, after the _Default_ tile; the collections, with
//!   the settings and pictures of the open one; and colors.
//!
//! The default of a display is what it shows once Sway Things stores no wallpaper for it
//! ([`Fallback`]): for one display, what all of them show if Sway Things stores that; else the
//! `bg` line of the user's config; else sway's, no wallpaper. _Use Default_ and the _Default_
//! tile remove the stored values of the edited display (or of all of them), so it comes back.
//!
//! A picked picture is stored with `~/` for the home folder, so settings shared through dotfiles
//! work for every user name. File and collection names are external strings: they are only shown
//! as plain text, and reach sway through the `output` command builder.
//!
//! This module holds what the parts share: the state of the page ([`Ui`]) and the small widget
//! helpers. The parts are `preview`, `pictures`, `collections`, `colors`, `notices`, and
//! `images`, which reads the pictures off the main thread.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use sway_things_core::command::Color;
use sway_things_core::outputs::Output;
use sway_things_core::registry::wallpaper::{COLOR, FILL, IMAGE};
use sway_things_core::registry::{Registry, Value};
use sway_things_core::wallpaper::{self, CHANGE, COLLECTION, Change, INTERVAL, ORDER, PICTURES};

use super::RowLayout;
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;
use crate::widgets::setting_row::tilde;

mod collections;
mod colors;
mod images;
mod notices;
mod pictures;
mod preview;

use self::preview::{own_outputs, timers};

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

/// What an output shows once Sway Things stores no wallpaper for it.
#[derive(Debug, Clone, PartialEq)]
enum Fallback {
    /// What every output shows, as Sway Things stores it: for an output of its own.
    AllDisplays,
    /// What the `bg` line of the user's config sets: the picture (`None` for the color alone,
    /// or a path the page can't follow, like one with a variable), the fit and the color, with
    /// the file and number of the line.
    Config {
        image: Option<String>,
        fill: Option<String>,
        color: Color,
        file: PathBuf,
        line: usize,
    },
    /// No `bg` line: sway starts no swaybg, and the screen stays black.
    Sway,
}

/// What an output shows, as the page names it with an icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Picture,
    Collection,
    Color,
}

impl Kind {
    /// The icon of the kind, before the name of what an output shows and on its tile.
    fn icon(self) -> &'static str {
        match self {
            Self::Picture => "image-x-generic-symbolic",
            Self::Collection => "view-grid-symbolic",
            Self::Color => "color-select-symbolic",
        }
    }
}

/// The parts of the page and what it shows.
struct Ui {
    model: Model,
    body: gtk::Box,
    error: gtk::Label,
    notices: gtk::Box,
    same: gtk::Switch,
    preview: gtk::Fixed,
    /// What the edited output shows: its kind as an icon, its name, the kind with where it is,
    /// and for a collection its pictures and when they change.
    kind: gtk::Image,
    title: gtk::Label,
    detail: gtk::Label,
    more: gtk::Label,
    /// Where what the edited output shows comes from, while it is the default.
    origin: gtk::Label,
    /// Why the edited output's picture can't be shown, if it can't.
    warning: gtk::Label,
    next: gtk::Button,
    use_default: gtk::Button,
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
    /// The picture of the edited output whose readability is checked.
    checked: RefCell<Option<PathBuf>>,
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
        let notices = gtk::Box::new(gtk::Orientation::Vertical, 6);

        let same = gtk::Switch::new();
        same.set_valign(gtk::Align::Center);
        let preview = gtk::Fixed::new();
        preview.set_halign(gtk::Align::Center);
        preview.set_valign(gtk::Align::End);
        let kind = gtk::Image::from_icon_name(Kind::Picture.icon());
        kind.set_valign(gtk::Align::Start);
        kind.set_accessible_role(gtk::AccessibleRole::Presentation);
        kind.add_css_class("st-wallpaper-kind");
        let title = label("st-wallpaper-title");
        let detail = label(style::DIM);
        let more = label(style::DIM);
        let origin = label(style::DIM);
        let warning = label("warning");
        warning.set_visible(false);
        let next = gtk::Button::with_label(tr(msg!("Next Picture")));
        next.set_valign(gtk::Align::Center);
        let use_default = gtk::Button::with_label(tr(msg!("Use Default")));
        use_default.set_valign(gtk::Align::Center);
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
            notices,
            same,
            preview,
            kind,
            title,
            detail,
            more,
            origin,
            warning,
            next,
            use_default,
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
            checked: RefCell::new(None),
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
        self.body.append(&self.notices);
        self.body.append(&self.current_card());
        self.editor_card();
        self.body.append(&self.tabs());
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

        // The tab's buttons at the end of the row of the tabs, below them when the window is
        // narrow, as the controls of a row.
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        bar.set_layout_manager(Some(RowLayout::default()));
        bar.append(&switcher);
        bar.append(&actions);
        tabs.append(&bar);
        tabs.append(&stack);
        tabs
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

    /// The wallpaper settings Sway Things stores for exactly `output` (`None`: every output).
    fn own_keys(&self, output: Option<&str>) -> Vec<&'static str> {
        self.model
            .state()
            .map(|state| {
                state
                    .entries()
                    .into_iter()
                    .filter(|entry| {
                        entry.setting.key().starts_with("wallpaper.") && entry.device == output
                    })
                    .map(|entry| entry.setting.key())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// What `output` (`None`: every output) shows once Sway Things stores no wallpaper for it.
    fn fallback(&self, output: Option<&str>) -> Fallback {
        if output.is_some() && !self.own_keys(None).is_empty() {
            return Fallback::AllDisplays;
        }
        let registry = Registry::builtin();
        let line = registry
            .get(IMAGE)
            .and_then(|image| self.model.report(image, output))
            .and_then(|report| report.user_value().cloned());
        let Some(line) = line else {
            return Fallback::Sway;
        };
        let image = match &line.value {
            Ok(Value::Text(image)) if !image.is_empty() => Some(image.clone()),
            _ => None,
        };
        let fill = match registry
            .get(FILL)
            .and_then(|fill| fill.read(&line.directive))
        {
            Some(Ok(Value::Choice(fill))) => Some(fill),
            _ => None,
        };
        let color = match registry
            .get(COLOR)
            .and_then(|color| color.read(&line.directive))
        {
            Some(Ok(Value::Color(color))) => color,
            _ => Color::rgb(0, 0, 0),
        };
        Fallback::Config {
            image,
            fill,
            color,
            file: line.file().to_path_buf(),
            line: line.line(),
        }
    }

    /// The picture and color `output` shows once Sway Things stores no wallpaper for it.
    fn fallback_look(&self, output: Option<&str>) -> (Option<PathBuf>, Color) {
        match self.fallback(output) {
            Fallback::AllDisplays => (self.picture_of(None), self.color_value(None)),
            Fallback::Config { image, color, .. } => (
                image.map(|image| wallpaper::expand(&image, Some(&gtk::glib::home_dir()))),
                color,
            ),
            Fallback::Sway => (None, Color::rgb(0, 0, 0)),
        }
    }

    /// Where the default of `output` comes from, for a line below its name.
    fn fallback_origin(&self, output: Option<&str>) -> String {
        match self.fallback(output) {
            Fallback::AllDisplays => tr(msg!("Default · the wallpaper of all displays")).to_owned(),
            Fallback::Config { file, line, .. } => trf(
                // TRANSLATORS: Where the wallpaper comes from: a line of the user's sway config,
                // {place} like ~/.config/sway/config:12.
                msg!("Default · from your config, {place}"),
                &[("place", &format!("{}:{line}", tilde(&file)))],
            ),
            Fallback::Sway => tr(msg!(
                "Default · sway draws no wallpaper, the screen stays black"
            ))
            .to_owned(),
        }
    }

    /// Before the fit or the color of the edited output changes while it shows the picture of
    /// the user's `bg` line: stores that line's picture, fit and color, so the line Sway Things
    /// generates draws the same picture with the change instead of replacing it with a color.
    /// _Use Default_ goes back to the user's line.
    fn take_over(&self) {
        let target = self.target();
        if !self.own_keys(target.as_deref()).is_empty() {
            return;
        }
        let Fallback::Config {
            image: Some(image),
            fill,
            color,
            ..
        } = self.fallback(target.as_deref())
        else {
            return;
        };
        // The picture first, so what shows meanwhile is the user's.
        self.model.set(IMAGE, target.as_deref(), Value::Text(image));
        if let Some(fill) = fill {
            self.model.set(FILL, target.as_deref(), Value::Choice(fill));
        }
        self.model
            .set(COLOR, target.as_deref(), Value::Color(color));
    }

    /// Removes what Sway Things stores for the wallpaper of the edited output, so its default
    /// shows again, and sends the default to the session: for an output that falls back to all
    /// of them, their wallpaper; else the `bg` line of the user's config (one line sets the
    /// picture, fit, color and collection together), or without one sway's default.
    fn use_default(&self) {
        self.error.set_visible(false);
        let target = self.target();
        let own = self.own_keys(target.as_deref());
        if own.is_empty() {
            return;
        }
        if self.fallback(target.as_deref()) == Fallback::AllDisplays {
            for key in own {
                self.model.reset(key, target.as_deref());
            }
            return;
        }
        let report = Registry::builtin()
            .get(IMAGE)
            .and_then(|image| self.model.report(image, target.as_deref()));
        match report {
            Some(report) => self.model.let_config_win(report),
            None => {
                for key in own {
                    self.model.reset(key, target.as_deref());
                }
            }
        }
    }

    /// The picture `output` shows now, if any: its own, or the current one of its collection.
    fn picture_of(&self, output: Option<&str>) -> Option<PathBuf> {
        match self.look(output) {
            Look::Picture(image) => Some(wallpaper::expand(&image, Some(&gtk::glib::home_dir()))),
            Look::Collection(_, current) => current,
            Look::Color => None,
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
            let current = self.model.state().and_then(|state| {
                let own = wallpaper::shown_on(&state, output).is_some();
                wallpaper::current(
                    &state
                        .paths()
                        .wallpaper_link(if own { output } else { None }),
                )
            });
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
        self.refresh_default(target.as_deref());
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
