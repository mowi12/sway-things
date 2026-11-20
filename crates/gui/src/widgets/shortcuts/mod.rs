// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The list of every binding on the Keyboard Shortcuts page, in the manner of GNOME Settings: a
//! search field that also matches keys, then a few groups (windows, workspaces, apps, media,
//! system, and one per mode) with one line per binding, what it does on the left and its keys as
//! keycaps on the right. A line that isn't in effect shows a warning sign, one that can't be
//! changed here a lock, each with the reason as its tooltip; a binding of the user's that Sway
//! Things changed or removes has a button that puts it back. Everything else (the command, the
//! options, the file and line, the clashes) is in the dialog a click on the line opens
//! ([`Editor`]), which changes or removes the binding through the items of `shortcuts.bindings`
//! ([`sway_things_core::bindings::edit`]), never in the user's file.
//!
//! [`below`] adds to each switch of the media keys what its keys clash with.
//!
//! A change builds the list again, keeping the [`Place`] on the page: the row that had the
//! focus keeps it (the same binding, or the row after one that is gone), and stays where it was
//! on screen.

mod editor;
mod osd;
mod parts;
mod row;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bindings::ours::{BINDINGS, Group};
use sway_things_core::bindings::{
    Category, Clash, ClashKind, DEFAULT_MODE, Flags, Keymap, Listing, Origin, Row, RowKind, Source,
};
use sway_things_core::registry::{Link, Page, Registry, Value};

use super::setting_row::tilde;
use super::{Place, SettingRow};
use crate::i18n::{msg, tr, trf};
use crate::model::Model;
use crate::style;

pub(crate) use editor::first_level;
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
    empty: gtk::Box,
    empty_icon: gtk::Image,
    empty_title: gtk::Label,
    empty_text: gtk::Label,
    keymap: Rc<Option<GdkKeymap>>,
    /// The rows shown, in order, each with the index of its row in the listing and what it
    /// stands for.
    shown: Rc<RefCell<Vec<Shown>>>,
}

/// A row shown in the list.
#[derive(Debug, Clone)]
struct Shown {
    /// The index of its row in the listing.
    index: usize,
    /// Its content, in its list item.
    content: gtk::Box,
    /// The binding it stands for.
    binding: BindingId,
}

/// What a row stands for across changes: the line of the user's config, or the line of Sway
/// Things' file in its mode. The index in the listing changes when a binding before it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BindingId {
    User(Arc<Path>, usize),
    Ours(String, String),
}

impl BindingId {
    fn of(row: &Row) -> Self {
        if row.source.origin == Origin::User {
            Self::User(row.source.file.clone(), row.source.line)
        } else {
            Self::Ours(row.mode.clone(), row.source.text.clone())
        }
    }
}

impl ShortcutList {
    /// The list, following `model` until it is gone.
    pub fn new(model: &Model) -> Self {
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some(tr(msg!("Search by action or keys"))));
        search.set_hexpand(true);
        search.add_css_class("st-shortcut-search");
        search.update_property(&[gtk::accessible::Property::Label(tr(msg!(
            "Search shortcuts"
        )))]);
        let add = gtk::Button::new();
        let add_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        add_content.append(&gtk::Image::from_icon_name("list-add-symbolic"));
        add_content.append(&gtk::Label::new(Some(tr(msg!("Add Shortcut…")))));
        add.set_child(Some(&add_content));
        add.update_property(&[gtk::accessible::Property::Label(tr(msg!("Add Shortcut…")))]);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        bar.add_css_class("st-shortcut-bar");
        bar.append(&search);
        bar.append(&add);
        let (empty, empty_icon, empty_title, empty_text) = empty_state();
        let lists = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("st-shortcuts");
        root.append(&bar);
        root.append(&empty);
        root.append(&lists);
        let list = Self {
            root,
            search,
            add,
            lists,
            empty,
            empty_icon,
            empty_title,
            empty_text,
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
            move |button| adding.open_editor(button, &model, None)
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
        self.shown
            .borrow()
            .iter()
            .map(|shown| (shown.index, shown.content.clone()))
            .collect()
    }

    /// Opens the dialog for the binding at `index`, or for a new one.
    fn open_editor(&self, from: &impl IsA<gtk::Widget>, model: &Model, index: Option<usize>) {
        Editor::open(from, model, index, self.keymap.clone());
    }

    /// Shows the bindings of `model` that match the search, keeping the place on the page.
    fn update(&self, model: &Model) {
        let before: Vec<BindingId> = self
            .shown
            .borrow()
            .iter()
            .map(|shown| shown.binding.clone())
            .collect();
        let mut place = Place::of(&self.root);
        let focused = place
            .as_ref()
            .and_then(Place::focus)
            .and_then(|focus| self.row_of(focus));
        if let Some(place) = &mut place {
            // A button of a row has the focus: the row takes its place.
            if let Some(item) = focused.and_then(|at| self.item(at)) {
                place.keep(&item);
            }
            place.release(&self.lists);
        }
        self.build(model);
        if let Some(place) = place {
            match focused.and_then(|at| self.successor(&before, at)) {
                Some((item, true)) => place.restore(Some(item)),
                Some((item, false)) => place.restore_after_removal(Some(item)),
                None => place.restore(None),
            }
        }
    }

    /// The position in the list of the row that `widget` is in, if it is in one.
    fn row_of(&self, widget: &gtk::Widget) -> Option<usize> {
        let item = if widget.is::<gtk::ListBoxRow>() {
            widget.clone()
        } else {
            widget.ancestor(gtk::ListBoxRow::static_type())?
        };
        let content = item.downcast_ref::<gtk::ListBoxRow>()?.child()?;
        self.shown
            .borrow()
            .iter()
            .position(|shown| *shown.content.upcast_ref::<gtk::Widget>() == content)
    }

    /// The list item of the row at `at` of the rows shown.
    fn item(&self, at: usize) -> Option<gtk::Widget> {
        self.shown.borrow().get(at)?.content.parent()
    }

    /// The list item that takes the focus from the row at `at` of the rows shown `before`, and
    /// whether it stands for the same binding: the row of that binding; else, if as many rows
    /// are shown, the one in its place (its binding changed); else the first row after it that is
    /// still shown, or the last before it (it was removed).
    fn successor(&self, before: &[BindingId], at: usize) -> Option<(gtk::Widget, bool)> {
        let shown = self.shown.borrow();
        let find = |binding: &BindingId| shown.iter().find(|shown| shown.binding == *binding);
        let same = before.get(at).and_then(find).or_else(|| {
            (shown.len() == before.len())
                .then(|| shown.get(at))
                .flatten()
        });
        if let Some(same) = same {
            return Some((same.content.parent()?, true));
        }
        let next = before
            .iter()
            .skip(at + 1)
            .chain(before.iter().take(at).rev())
            .find_map(find)?;
        Some((next.content.parent()?, false))
    }

    /// Builds the lists of the bindings of `model` that match the search.
    fn build(&self, model: &Model) {
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
        // "super+shift+q" finds the keys as well as "super shift q".
        let words: Vec<String> = self
            .search
            .text()
            .split(|character: char| character.is_whitespace() || character == '+')
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect();
        let matching: Vec<(usize, &Row)> = listing
            .rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| row.matches(&words, &labels(row, &listing, keymap)))
            .collect();
        self.show_empty(listing.rows().is_empty(), matching.is_empty());
        let mut first = true;
        for (title, rows) in groups(&listing, &matching) {
            self.add_list(model, &listing, &title, &rows, writable, first);
            first = false;
        }
    }

    /// Shows the empty state when nothing matches: that nothing is bound at all, or that the
    /// search finds nothing.
    fn show_empty(&self, nothing: bool, none_matches: bool) {
        self.empty.set_visible(none_matches);
        if nothing {
            self.empty_icon
                .set_icon_name(Some("input-keyboard-symbolic"));
            self.empty_title.set_text(tr(msg!("No Shortcuts Yet")));
            self.empty_text.set_text(tr(msg!(
                "Neither your config nor Sway Things binds any keys yet."
            )));
        } else {
            self.empty_icon.set_icon_name(Some("edit-find-symbolic"));
            self.empty_title.set_text(tr(msg!("No Shortcuts Found")));
            self.empty_text.set_text(tr(msg!(
                "Try other words, or keys like \u{201c}super enter\u{201d}."
            )));
        }
    }

    /// Adds a heading `title` and a list of `rows`; `first` is the first list of the page.
    fn add_list(
        &self,
        model: &Model,
        listing: &Listing,
        title: &str,
        rows: &[(usize, &Row)],
        writable: bool,
        first: bool,
    ) {
        let heading = gtk::Label::new(Some(title));
        heading.set_xalign(0.0);
        heading.add_css_class("st-shortcut-heading");
        heading.add_css_class("heading");
        if first {
            heading.add_css_class("st-first");
        }
        heading.set_accessible_role(gtk::AccessibleRole::Heading);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("st-section");
        list.add_css_class("st-shortcut-list");
        list.update_property(&[gtk::accessible::Property::Label(title)]);
        let keymap = (*self.keymap).as_ref().map(|keymap| keymap as &dyn Keymap);
        for &(index, row) in rows {
            let built = row::build(model, listing, index, row, keymap, writable);
            self.shown.borrow_mut().push(Shown {
                index,
                content: built.content.clone(),
                binding: BindingId::of(row),
            });
            list.append(&built.item);
        }
        let shown = self.shown.clone();
        let opening = self.clone();
        list.connect_row_activated(glib::clone!(
            #[weak]
            model,
            move |_, item| {
                let child = item.child();
                let index = shown
                    .borrow()
                    .iter()
                    .find(|shown| child.as_ref() == Some(shown.content.upcast_ref()))
                    .map(|shown| shown.index);
                if let Some(index) = index {
                    opening.open_editor(item, &model, Some(index));
                }
            }
        ));
        self.lists.append(&heading);
        self.lists.append(&list);
    }
}

/// The rows of one group of the list, by their index in the listing.
type GroupRows<'a> = Vec<(usize, &'a Row)>;

/// The groups of the list with the rows of `matching` in them, each with its title: the
/// categories of the default mode (with "other" under "system", so the list has few groups),
/// then one per mode. Empty groups are left out.
fn groups<'a>(listing: &Listing, matching: &[(usize, &'a Row)]) -> Vec<(String, GroupRows<'a>)> {
    let mut groups = Vec::new();
    for category in Category::ALL {
        if category == Category::Other {
            continue;
        }
        let rows: GroupRows<'a> = matching
            .iter()
            .filter(|(_, row)| {
                row.mode == DEFAULT_MODE
                    && (row.category() == category
                        || (category == Category::System && row.category() == Category::Other))
            })
            .copied()
            .collect();
        let title = if category == Category::System {
            tr(msg!("System and Other"))
        } else {
            tr(category.title())
        };
        groups.push((title.to_owned(), rows));
    }
    for mode in listing.modes() {
        let rows: GroupRows<'a> = matching
            .iter()
            .filter(|(_, row)| row.mode == mode)
            .copied()
            .collect();
        groups.push((
            trf(msg!("Mode \u{201c}{mode}\u{201d}"), &[("mode", &mode)]),
            rows,
        ));
    }
    groups.retain(|(_, rows)| !rows.is_empty());
    groups
}

/// The box shown in place of the list when nothing matches, with its icon, title and text.
fn empty_state() -> (gtk::Box, gtk::Image, gtk::Label, gtk::Label) {
    let icon = gtk::Image::new();
    icon.set_pixel_size(48);
    icon.add_css_class(style::DIM);
    let title = gtk::Label::new(None);
    title.add_css_class("title-3");
    title.set_wrap(true);
    title.set_justify(gtk::Justification::Center);
    let text = gtk::Label::new(None);
    text.add_css_class(style::DIM);
    text.set_wrap(true);
    text.set_justify(gtk::Justification::Center);
    let empty = gtk::Box::new(gtk::Orientation::Vertical, 8);
    empty.add_css_class("st-shortcut-empty");
    empty.set_halign(gtk::Align::Center);
    empty.append(&icon);
    empty.append(&title);
    empty.append(&text);
    empty.set_visible(false);
    (empty, icon, title, text)
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

/// What a binding does, as its row says it: in words for a common command, else the command.
/// The second value says whether it is the command.
fn title_of(row: &Row) -> (String, bool) {
    row.description().map_or_else(
        || (row.line.command.clone().unwrap_or_default(), true),
        |text| (text.translated(), false),
    )
}

/// The keys as keycaps: a framed label per key, with a "+" between them.
pub(crate) fn keycaps(labels: &[String]) -> gtk::Box {
    let keys = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    keys.add_css_class("st-keycaps");
    keys.set_valign(gtk::Align::Center);
    for (index, text) in labels.iter().enumerate() {
        if index > 0 {
            let plus = gtk::Label::new(Some("+"));
            plus.add_css_class("st-keycap-plus");
            keys.append(&plus);
        }
        let key = keycap(text);
        keys.append(&key);
    }
    keys
}

/// A key as a keycap. A lone symbol is a little larger, so it reads, and `-` is the wider minus
/// sign.
fn keycap(text: &str) -> gtk::Label {
    let key = gtk::Label::new(Some(if text == "-" { "\u{2212}" } else { text }));
    key.add_css_class("st-keycap");
    if text.chars().count() == 1 && text.chars().all(|c| c.is_ascii_punctuation()) {
        key.add_css_class("st-keycap-symbol");
    }
    key
}

/// A label, left-aligned and wrapping, with `classes`.
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
pub(crate) fn place(source: &Source) -> String {
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

/// The lines that say where a binding is and what became of it: its options in words, if any,
/// then where it comes from.
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

/// Why the binding of `row` can't be changed on this page, if it can't: shown as the tooltip of
/// the lock on its row and at the top of its dialog. A binding Sway Things removes can't be
/// changed either, but it can be put back, which its row offers instead.
fn locked_reason(row: &Row) -> Option<String> {
    if row.editable() {
        return None;
    }
    Some(match &row.kind {
        RowKind::Hand if row.source.after_ours => tr(msg!(
            "Read after Sway Things' file, so it can only be changed in your config."
        ))
        .to_owned(),
        RowKind::Hand if row.mode != DEFAULT_MODE => tr(msg!(
            "Bindings of modes can only be changed in your config for now."
        ))
        .to_owned(),
        RowKind::Hand => tr(msg!(
            "Its keys need quotes, so it can only be changed in your config."
        ))
        .to_owned(),
        RowKind::Added { .. } | RowKind::Changed { .. } | RowKind::Removed { .. } => return None,
        RowKind::Group { key } => Group::of(key).map_or_else(String::new, |group| {
            trf(
                msg!("Part of the {group}, switched on below."),
                &[("group", &tr(group.title).to_lowercase())],
            )
        }),
        RowKind::Feature { key } => feature(key).map_or_else(String::new, |(title, page)| {
            trf(
                msg!("{title}, set on the {page} page"),
                &[("title", &title), ("page", &page)],
            )
        }),
    })
}

/// The setting that owns the binding of `row`, if another setting than the list of shortcuts
/// does: its dialog links to it.
fn owner(row: &Row) -> Option<&'static str> {
    match &row.kind {
        RowKind::Hand
        | RowKind::Added { .. }
        | RowKind::Changed { .. }
        | RowKind::Removed { .. } => None,
        RowKind::Group { key } | RowKind::Feature { key } => Some(key),
    }
}

/// The title of the button that opens the setting `key`: on this page its row, else its page.
fn owner_button_title(key: &str) -> Option<String> {
    let setting = Registry::builtin().get(key)?;
    Some(if setting.page() == Page::KeyboardShortcuts {
        tr(msg!("Show Switch")).to_owned()
    } else {
        trf(
            msg!("Open {page}"),
            &[("page", &tr(setting.page().title()))],
        )
    })
}

/// Opens the setting `key` in `main`, the app's window.
fn open_setting(main: &gtk::Window, key: &str) {
    let main = main.downcast_ref::<crate::window::Window>();
    if let (Some(main), Ok(link)) = (main, Link::parse(key, Registry::builtin())) {
        main.open_link(&link);
    }
}

/// What a clash means for a binding, in words.
fn clash_text(clash: &Clash) -> String {
    let place = place(&clash.source);
    let command = clash.command.as_deref().unwrap_or_default();
    let args: &[(&str, &dyn std::fmt::Display)] = &[("place", &place), ("command", &command)];
    match clash.kind {
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
    }
}

/// The line that says how a binding clashes with another, with a warning sign.
pub(crate) fn clash_line(clash: &Clash) -> gtk::Box {
    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    icon.set_valign(gtk::Align::Start);
    icon.add_css_class("st-shortcut-warning");
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.add_css_class("st-shortcut-clash");
    line.append(&icon);
    line.append(&label(&clash_text(clash), &[]));
    line
}

/// What goes below the row of a switch of media keys: what its keys clash with; below the
/// switch of its on-screen display, whether SwayOSD's server runs; nothing for other rows.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    if let Some(group) = row
        .setting()
        .and_then(|setting| Group::of_osd(setting.key()))
    {
        return vec![osd::note(model, group)];
    }
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
/// its list, a switch of media keys only while its program is installed or it is stored (so
/// it can be switched off again), and the switch of a group's on-screen display below it
/// likewise while `swayosd-client` is installed or it is stored.
pub fn has_row(model: &Model, setting: &sway_things_core::registry::Setting) -> bool {
    if setting.key() == BINDINGS {
        return false;
    }
    if let Some(group) = Group::of_osd(setting.key()) {
        let group_shown = Registry::builtin()
            .get(group.key)
            .is_some_and(|switch| has_row(model, switch));
        return group_shown && osd::has_row(model, setting);
    }
    Group::of(setting.key())
        .is_none_or(|group| group.installed() || model.stored_in(setting, None).is_some())
}
