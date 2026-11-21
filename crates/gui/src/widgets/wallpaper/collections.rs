// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Collections tab: the collections as cards, and the editor of the open one with its
//! pictures and folders, when it changes and in which order.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use sway_things_core::registry::Value;
use sway_things_core::wallpaper::{
    self, CHANGE, COLLECTION, Change, Collection, INTERVAL, ORDER, Order, PICTURES,
};

use super::images::frame;
use super::pictures::picture_filter;
use super::{Editor, Look, TILE, Ui, grid, index, label, linked, row, text_block};
use crate::i18n::{msg, msg_plural, ntrf, tr, trf};
use crate::style;
use crate::widgets::setting_row::tilde;

impl Ui {
    /// _New Collection…_: a name, then _Create_.
    pub(super) fn collection_actions(self: &Rc<Self>) -> gtk::MenuButton {
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
    pub(super) fn collections_tab(self: &Rc<Self>) -> gtk::Box {
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

    /// Connects the controls of the collection editor.
    pub(super) fn connect_editor(self: &Rc<Self>) {
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

    /// The collections, selected the one `target` shows, and the editor of the open one.
    pub(super) fn refresh_collections(self: &Rc<Self>, target: Option<&str>) {
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

    /// Shows the collection `name` on the edited output, and opens it.
    pub(super) fn pick_collection(self: &Rc<Self>, name: &str) {
        self.open.replace(Some(name.to_owned()));
        self.set(COLLECTION, Value::Text(name.to_owned()));
        self.refresh();
    }

    /// Stops showing a collection on the edited output, if it does.
    pub(super) fn leave_collection(&self) {
        let target = self.target();
        if matches!(self.look(target.as_deref()), Look::Collection(..)) {
            self.set(COLLECTION, Value::Text(String::new()));
        }
    }

    /// Adds `paths` to the open collection.
    pub(super) fn add_entries(&self, paths: Vec<PathBuf>) {
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
    pub(super) fn remove_entry(&self, entry: &str) {
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
    pub(super) fn add_pictures(self: &Rc<Self>, button: &gtk::Button) {
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
    pub(super) fn add_folder(self: &Rc<Self>, button: &gtk::Button) {
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
    pub(super) fn delete_open(self: &Rc<Self>) {
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

    /// Builds the editor card; its buttons need the page.
    pub(super) fn editor_card(self: &Rc<Self>) {
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

impl Editor {
    pub(super) fn new() -> Self {
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
    pub(super) fn minutes(&self) -> i64 {
        let factor = match self.unit.selected() {
            1 => 60,
            2 => 24 * 60,
            _ => 1,
        };
        i64::from(self.interval.value_as_int()) * factor
    }

    /// Shows `collection`.
    pub(super) fn show(&self, collection: &Collection) {
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

/// "12 pictures · every 30 minutes · shuffled", for a collection.
pub(super) fn describe(collection: &Collection) -> String {
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
pub(super) fn when(change: Change) -> String {
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
pub(super) fn collection_card(collection: &Collection) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    // A stack of its first two pictures, the first in front.
    let pictures = collection.pictures(Some(&glib::home_dir()));
    // The stack has a size of its own: an overlay takes the size of its child, and a picture
    // would make it as wide as the picture once it is read.
    let cover = gtk::Overlay::new();
    let base = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    base.set_size_request(TILE.0 + 10, TILE.1 + 8);
    cover.set_child(Some(&base));
    cover.set_halign(gtk::Align::Start);
    let (back, _) = frame(pictures.get(1).map(PathBuf::as_path));
    back.set_margin_start(10);
    back.set_margin_top(8);
    back.add_css_class("st-wallpaper-stack");
    cover.add_overlay(&back);
    let (front, _) = frame(pictures.first().map(PathBuf::as_path));
    front.set_margin_end(10);
    front.set_margin_bottom(8);
    cover.add_overlay(&front);
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

/// A picture or folder of a collection in the editor, with a remove mark.
pub(super) fn member_tile(entry: &str) -> gtk::Box {
    let path = wallpaper::expand(entry, Some(&glib::home_dir()));
    let name = path.file_name().map_or_else(
        || entry.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let (frame, inside) = frame(Some(&path));
    // The mark on the corner of the picture.
    let remove = gtk::Image::from_icon_name("window-close-symbolic");
    remove.add_css_class("st-wallpaper-remove");
    remove.set_halign(gtk::Align::End);
    remove.set_valign(gtk::Align::Start);
    inside.add_overlay(&remove);
    tile.append(&frame);
    let caption = gtk::Label::new(Some(&name));
    caption.add_css_class(style::DIM);
    caption.add_css_class("st-wallpaper-caption");
    caption.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    caption.set_max_width_chars(1);
    tile.append(&caption);
    let label = trf(
        msg!("Remove {name} from the collection"),
        &[("name", &name)],
    );
    tile.set_tooltip_text(Some(&label));
    tile.update_property(&[gtk::accessible::Property::Label(&label)]);
    tile
}
