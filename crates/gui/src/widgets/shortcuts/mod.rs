// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The list of every binding on the Keyboard Shortcuts page: the user's (with file and line) and
//! those of Sway Things, grouped by what they do, searchable by keys and command. A row says what
//! became of its binding and which others it clashes with; its menu changes or removes it
//! through the items of `shortcuts.bindings` ([`sway_things_core::bindings::edit`]), never in
//! the user's file, and puts the user's binding back. Bindings of other modes are listed by mode
//! and read only.
//!
//! [`below`] adds to each switch of the media keys what its keys clash with.

mod editor;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bindings::ours::{BINDINGS, Group};
use sway_things_core::bindings::{
    Category, Clash, ClashKind, DEFAULT_MODE, Flags, Keymap, Listing, Origin, Row, RowKind, Source,
    edit,
};
use sway_things_core::registry::Value;

use super::SettingRow;
use super::setting_row::tilde;
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;

pub use editor::{Captured, Editor, capture};

/// The keymap of the app's keyboard, from GDK: which keycode gives a keysym on its first level
/// in the first layout, as sway translates `--to-code` bindings.
#[derive(Debug, Default)]
pub struct GdkKeymap {
    /// The canonical keysym names and their keycode; `None` for a keysym on several keys.
    codes: HashMap<String, Option<u32>>,
    /// The keysym name of each keycode.
    names: HashMap<u32, String>,
}

impl GdkKeymap {
    /// The keymap of `display`.
    pub fn new(display: &gdk::Display) -> Self {
        let mut keymap = Self::default();
        for code in 8..=767 {
            let Some(entries) = display.map_keycode(code) else {
                continue;
            };
            let Some(name) = entries
                .iter()
                .find(|(key, _)| key.group() == 0 && key.level() == 0)
                .and_then(|(_, keyval)| keyval.name())
            else {
                continue;
            };
            let canonical = sway_things_core::bindings::keys::keysym_key(&name);
            keymap
                .codes
                .entry(canonical)
                .and_modify(|found| *found = None)
                .or_insert(Some(code));
            keymap.names.insert(code, name.to_string());
        }
        keymap
    }

    /// The keymap of the default display, if there is one.
    pub fn of_default() -> Option<Self> {
        gdk::Display::default().map(|display| Self::new(&display))
    }
}

impl Keymap for GdkKeymap {
    fn keycode(&self, name: &str) -> Option<u32> {
        self.codes.get(name).copied().flatten()
    }

    fn keysym(&self, code: u32) -> Option<String> {
        self.names.get(&code).cloned()
    }
}

/// The list of bindings, with its search field and the button that adds one.
#[derive(Debug, Clone)]
pub struct ShortcutList {
    root: gtk::Box,
    search: gtk::SearchEntry,
    add: gtk::Button,
    lists: gtk::Box,
    empty: gtk::Label,
    keymap: Rc<Option<GdkKeymap>>,
    /// The rows shown, by the index of their row in the listing, for tests.
    shown: Rc<RefCell<Vec<(usize, gtk::Box)>>>,
}

impl ShortcutList {
    /// The list, following `model` until it is gone.
    pub fn new(model: &Model) -> Self {
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some(tr(msg!("Search keys and commands"))));
        search.set_hexpand(true);
        search.update_property(&[gtk::accessible::Property::Label(tr(msg!(
            "Search shortcuts"
        )))]);
        let add = gtk::Button::with_label(tr(msg!("Add Shortcut…")));
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        bar.add_css_class("st-shortcut-bar");
        bar.append(&search);
        bar.append(&add);
        let note = gtk::Label::new(Some(tr(msg!(
            "Changes go to Sway Things' own file: a binding of your config is removed there with \
             unbindsym and bound again, your file stays as it is."
        ))));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class(style::DIM);
        note.add_css_class("st-section-hint");
        let empty = gtk::Label::new(None);
        empty.set_xalign(0.0);
        empty.set_wrap(true);
        empty.add_css_class(style::DIM);
        empty.add_css_class("st-section-hint");
        let lists = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("st-shortcuts");
        root.append(&bar);
        root.append(&note);
        root.append(&empty);
        root.append(&lists);
        let list = Self {
            root,
            search,
            add,
            lists,
            empty,
            keymap: Rc::new(GdkKeymap::of_default()),
            shown: Rc::default(),
        };
        list.update(model);
        let weak = list.root.downgrade();
        let shown = list.clone();
        let handler = RefCell::new(Some(model.connect_changed(move |model| {
            if weak.upgrade().is_some() {
                shown.update(model);
            }
        })));
        list.root.connect_destroy(glib::clone!(
            #[weak]
            model,
            move |_| {
                if let Some(handler) = handler.take() {
                    model.disconnect(handler);
                }
            }
        ));
        let searched = list.clone();
        list.search.connect_search_changed(glib::clone!(
            #[weak]
            model,
            move |_| searched.update(&model)
        ));
        let adding = list.clone();
        list.add.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |button| {
                Editor::open(button, &model, None, adding.keymap.clone());
            }
        ));
        list
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// The search field.
    pub fn search(&self) -> &gtk::SearchEntry {
        &self.search
    }

    /// The button that adds a binding.
    pub fn add_button(&self) -> &gtk::Button {
        &self.add
    }

    /// The rows shown, each with the index of its binding in [`Model::bindings`].
    pub fn shown_rows(&self) -> Vec<(usize, gtk::Box)> {
        self.shown.borrow().clone()
    }

    /// Shows the bindings of `model` that match the search.
    fn update(&self, model: &Model) {
        while let Some(child) = self.lists.first_child() {
            self.lists.remove(&child);
        }
        self.shown.borrow_mut().clear();
        let writable = model.status().is_some_and(|status| status.is_active());
        self.add.set_sensitive(writable);
        let keymap = (*self.keymap).as_ref().map(|keymap| keymap as &dyn Keymap);
        let Some(listing) = model.bindings(keymap) else {
            return;
        };
        let words: Vec<String> = self
            .search
            .text()
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let matching: Vec<(usize, &Row)> = listing
            .rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| row.matches(&words, &labels(row, &listing, keymap)))
            .collect();
        self.empty.set_visible(matching.is_empty());
        self.empty.set_text(if listing.rows().is_empty() {
            tr(msg!(
                "Neither your config nor Sway Things binds any keys yet."
            ))
        } else {
            tr(msg!("No shortcut matches the search."))
        });
        for category in Category::ALL {
            let rows: Vec<(usize, &Row)> = matching
                .iter()
                .filter(|(_, row)| row.mode == DEFAULT_MODE && row.category() == category)
                .copied()
                .collect();
            self.add_list(model, &listing, tr(category.title()), &rows, writable);
        }
        for mode in listing.modes() {
            let rows: Vec<(usize, &Row)> = matching
                .iter()
                .filter(|(_, row)| row.mode == mode)
                .copied()
                .collect();
            let title = trf(msg!("Mode {mode}"), &[("mode", &mode)]);
            self.add_list(model, &listing, &title, &rows, writable);
        }
    }

    /// Adds a heading `title` and a list of `rows`, if there are any.
    fn add_list(
        &self,
        model: &Model,
        listing: &Listing,
        title: &str,
        rows: &[(usize, &Row)],
        writable: bool,
    ) {
        if rows.is_empty() {
            return;
        }
        let heading = gtk::Label::new(Some(title));
        heading.set_xalign(0.0);
        heading.add_css_class("st-shortcut-heading");
        heading.add_css_class("heading");
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("st-section");
        list.update_property(&[gtk::accessible::Property::Label(title)]);
        let keymap = (*self.keymap).as_ref().map(|keymap| keymap as &dyn Keymap);
        for &(index, row) in rows {
            let content = self.row(model, listing, index, row, keymap, writable);
            self.shown.borrow_mut().push((index, content.clone()));
            let item = gtk::ListBoxRow::new();
            item.set_activatable(false);
            item.set_focusable(false);
            item.set_child(Some(&content));
            list.append(&item);
        }
        self.lists.append(&heading);
        self.lists.append(&list);
    }

    /// The widget of the binding `row` at `index` of `listing`.
    fn row(
        &self,
        model: &Model,
        listing: &Listing,
        index: usize,
        row: &Row,
        keymap: Option<&dyn Keymap>,
        writable: bool,
    ) -> gtk::Box {
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 3);
        texts.set_hexpand(true);
        texts.append(&keycaps(&labels(row, listing, keymap)));
        let command = row.line.command.clone().unwrap_or_default();
        let title = row
            .description()
            .map_or_else(|| command.clone(), |text| text.translated());
        let title = label(&title, &["st-row-title"]);
        texts.append(&title);
        if row.description().is_some() {
            texts.append(&label(&command, &[style::DIM, "monospace"]));
        }
        for line in status_lines(row, listing, keymap) {
            texts.append(&label(&line, &[style::DIM]));
        }
        for clash in &row.clashes {
            texts.append(&clash_line(clash));
        }
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        content.add_css_class("st-shortcut-row");
        content.append(&texts);
        if let Some(menu) = self.menu(model, index, row, writable) {
            content.append(&menu);
        }
        let mut accessible = labels(row, listing, keymap).join(" + ");
        accessible.push_str(": ");
        accessible.push_str(&title.text());
        content.update_property(&[gtk::accessible::Property::Label(&accessible)]);
        content
    }

    /// The ⋮ menu of a row: change, remove, back to the user's binding, open the file.
    fn menu(
        &self,
        model: &Model,
        index: usize,
        row: &Row,
        writable: bool,
    ) -> Option<gtk::MenuButton> {
        let actions = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&actions));
        let changes = writable && row.editable();
        if changes {
            let keymap = self.keymap.clone();
            actions.append(&action(
                &popover,
                msg!("Change…"),
                move |button, model| {
                    Editor::open(button, model, Some(index), keymap.clone());
                },
                model,
            ));
            actions.append(&action(
                &popover,
                msg!("Remove"),
                move |_, model| {
                    store(model, |listing, items| edit::remove(listing, items, index));
                },
                model,
            ));
        }
        if writable && edit::can_restore(row) {
            actions.append(&action(
                &popover,
                msg!("Back to Your Config's Binding"),
                move |_, model| store(model, |listing, items| edit::restore(listing, items, index)),
                model,
            ));
        }
        if row.source.origin == Origin::User && !row.source.file.as_os_str().is_empty() {
            let file = row.source.file.to_path_buf();
            let line = row.source.line;
            actions.append(&action(
                &popover,
                msg!("Open in Editor"),
                move |button, model| {
                    let reporter = model.clone();
                    crate::editor::open(button, Some(model), &file, Some(line), move |error| {
                        if let Some(error) = error {
                            reporter.emit_problem(error);
                        }
                    });
                },
                model,
            ));
        }
        actions.first_child()?;
        let button = gtk::MenuButton::new();
        button.set_icon_name("view-more-symbolic");
        button.set_valign(gtk::Align::Center);
        button.add_css_class("flat");
        button.set_popover(Some(&popover));
        button.set_tooltip_text(Some(tr(msg!("Change, remove or restore"))));
        button.update_property(&[gtk::accessible::Property::Label(tr(msg!(
            "Shortcut actions"
        )))]);
        Some(button)
    }
}

/// A flat button of a row's menu, `title`, that closes `popover` and runs `run`.
fn action(
    popover: &gtk::Popover,
    title: Msg,
    run: impl Fn(&gtk::Button, &Model) + 'static,
    model: &Model,
) -> gtk::Button {
    let button = gtk::Button::with_label(tr(title));
    button.add_css_class("flat");
    if let Some(label) = button.child().and_downcast::<gtk::Label>() {
        label.set_xalign(0.0);
    }
    button.connect_clicked(glib::clone!(
        #[weak]
        model,
        #[weak]
        popover,
        move |button| {
            popover.popdown();
            run(button, &model);
        }
    ));
    button
}

/// Stores the items `change` makes of the items stored now, for the listing of `model`.
pub(crate) fn store(model: &Model, change: impl FnOnce(&Listing, &[String]) -> Vec<String>) {
    let keymap = GdkKeymap::of_default();
    let Some(listing) = model.bindings(keymap.as_ref().map(|keymap| keymap as &dyn Keymap)) else {
        return;
    };
    let items = model.binding_items();
    let new = change(&listing, &items);
    if new != items {
        model.set(BINDINGS, None, Value::List(new));
    }
}

/// The labels of the keys of `row`, translated.
fn labels(row: &Row, listing: &Listing, keymap: Option<&dyn Keymap>) -> Vec<String> {
    row.line
        .labels(listing.modifier(), keymap)
        .iter()
        .map(sway_things_core::i18n::Text::translated)
        .collect()
}

/// The keys as keycaps: a framed label per key.
fn keycaps(labels: &[String]) -> gtk::Box {
    let keys = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    keys.add_css_class("st-keycaps");
    for (index, text) in labels.iter().enumerate() {
        if index > 0 {
            keys.append(&gtk::Label::new(Some("+")));
        }
        let key = gtk::Label::new(Some(text));
        key.add_css_class("st-keycap");
        keys.append(&key);
    }
    keys
}

/// A label of a row, left-aligned and wrapping, with `classes`.
fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    for class in classes {
        label.add_css_class(class);
    }
    label
}

/// Where a binding is, to show: the user's file and line, or what of Sway Things it is.
fn place(source: &Source) -> String {
    match source.origin {
        Origin::User => format!("{}:{}", tilde(&source.file), source.line),
        Origin::Group(key) => Group::of(key).map_or_else(String::new, |group| {
            trf(
                msg!("the {group} of Sway Things"),
                &[("group", &tr(group.title).to_lowercase())],
            )
        }),
        Origin::Item(_) | Origin::Placeholder(_) => {
            tr(msg!("a shortcut of Sway Things")).to_owned()
        }
        Origin::Feature(key) => feature(key).map_or_else(String::new, |(title, page)| {
            trf(
                msg!("the key \u{201c}{title}\u{201d} of the {page} page"),
                &[("title", &title), ("page", &page)],
            )
        }),
    }
}

/// The title of the setting `key` of another page that binds a key, and the title of its page.
fn feature(key: &str) -> Option<(String, String)> {
    sway_things_core::registry::Registry::builtin()
        .get(key)
        .map(|setting| {
            let section = tr(setting.section());
            let title = tr(setting.title());
            (
                format!("{section} \u{203a} {title}"),
                tr(setting.page().title()).to_owned(),
            )
        })
}

/// The options of a binding in words.
fn options(flags: Flags, code: bool) -> Vec<&'static str> {
    [
        (flags.locked, msg!("also on the lock screen")),
        (flags.to_code, msg!("the same key in every layout")),
        (code, msg!("by keycode")),
        (flags.release, msg!("on release")),
        (flags.no_repeat, msg!("no repeat")),
        (
            flags.inhibited,
            msg!("also while an app inhibits shortcuts"),
        ),
        (
            flags.whole_window || flags.border || flags.exclude_titlebar,
            msg!("over a window"),
        ),
    ]
    .into_iter()
    .filter(|(set, _)| *set)
    .map(|(_, text)| tr(text))
    .collect()
}

/// The lines that say where a binding is and what became of it.
fn status_lines(row: &Row, listing: &Listing, keymap: Option<&dyn Keymap>) -> Vec<String> {
    let mut lines = Vec::new();
    let options = options(row.line.flags, row.line.code);
    if !options.is_empty() {
        lines.push(options.join(" · "));
    }
    let place = place(&row.source);
    lines.push(match &row.kind {
        RowKind::Hand if row.source.after_ours => trf(
            msg!("{place}, read after Sway Things' file: change it there"),
            &[("place", &place)],
        ),
        RowKind::Hand if row.mode != DEFAULT_MODE => trf(
            msg!("{place}; bindings of modes are changed in your config for now"),
            &[("place", &place)],
        ),
        RowKind::Hand => place,
        RowKind::Added { .. } => tr(msg!("Added here")).to_owned(),
        RowKind::Changed { original, .. } => {
            let was = original
                .labels(listing.modifier(), keymap)
                .iter()
                .map(sway_things_core::i18n::Text::translated)
                .collect::<Vec<_>>()
                .join(" + ");
            trf(
                msg!("Changed here; your config binds {was} to {command} at {place}"),
                &[
                    ("was", &was),
                    ("command", &original.command.as_deref().unwrap_or_default()),
                    ("place", &place),
                ],
            )
        }
        RowKind::Removed { .. } if row.source.origin == Origin::User => trf(
            msg!("Removed here; your config binds it at {place}"),
            &[("place", &place)],
        ),
        RowKind::Removed { .. } => tr(msg!(
            "Removed here, but your config no longer binds these keys"
        ))
        .to_owned(),
        RowKind::Group { key } => Group::of(key).map_or_else(String::new, |group| {
            trf(
                msg!("{group}, switched on below"),
                &[("group", &tr(group.title))],
            )
        }),
        RowKind::Feature { key } => feature(key).map_or_else(String::new, |(title, page)| {
            trf(
                msg!("{title}, set on the {page} page"),
                &[("title", &title), ("page", &page)],
            )
        }),
    });
    lines
}

/// The line that says how a binding clashes with another, with a warning sign.
fn clash_line(clash: &Clash) -> gtk::Box {
    let place = place(&clash.source);
    let command = clash.command.as_deref().unwrap_or_default();
    let args: &[(&str, &dyn std::fmt::Display)] = &[("place", &place), ("command", &command)];
    let text = match clash.kind {
        ClashKind::Replaces => trf(msg!("Takes the place of {command} at {place}"), args),
        ClashKind::ReplacedBy => trf(
            msg!("Not in effect: {place} binds the same keys later, to {command}"),
            args,
        ),
        ClashKind::RemovedBy => trf(msg!("Not in effect: {place} unbinds it"), args),
        ClashKind::Shadowed => trf(
            msg!("Not in effect: the keycode binding at {place} is on the same key, to {command}"),
            args,
        ),
        ClashKind::Shadows => trf(
            msg!("Wins over the binding of the same key at {place}, to {command}"),
            args,
        ),
        ClashKind::LockScreenOnly => trf(
            msg!("Only on the lock screen: {place} binds the same keys otherwise, to {command}"),
            args,
        ),
    };
    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    icon.set_valign(gtk::Align::Start);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.add_css_class("st-shortcut-clash");
    line.append(&icon);
    line.append(&label(&text, &[style::DIM]));
    line
}

/// What goes below the row of a switch of media keys: what its keys clash with, and nothing
/// for other rows.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let Some(group) = row.setting().and_then(|setting| Group::of(setting.key())) else {
        return Vec::new();
    };
    let note = gtk::Box::new(gtk::Orientation::Vertical, 3);
    note.add_css_class("st-shortcut-group-clashes");
    let follow = glib::clone!(
        #[weak]
        note,
        move |model: &Model| {
            while let Some(child) = note.first_child() {
                note.remove(&child);
            }
            let keymap = GdkKeymap::of_default();
            let keymap = keymap.as_ref().map(|keymap| keymap as &dyn Keymap);
            let Some(listing) = model.bindings(keymap) else {
                return;
            };
            for row in listing.rows() {
                if row.kind != (RowKind::Group { key: group.key }) {
                    continue;
                }
                for clash in &row.clashes {
                    note.append(&clash_line(clash));
                }
            }
            note.set_visible(note.first_child().is_some());
        }
    );
    follow(model);
    model.connect_changed(follow);
    vec![note.upcast()]
}

/// Whether the row of `key` belongs on the page: not the list of items, which the page shows as
/// its list, and a switch of media keys only while its program is installed or it is stored (so
/// it can be switched off again).
pub fn has_row(model: &Model, setting: &sway_things_core::registry::Setting) -> bool {
    if setting.key() == BINDINGS {
        return false;
    }
    Group::of(setting.key())
        .is_none_or(|group| group.installed() || model.stored_in(setting, None).is_some())
}
