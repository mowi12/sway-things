// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The items of the bar in three lanes, _Left_, _Center_ and _Right_, side by side as on the bar. Each lane is a list of rows: the item's icon and name, what it
//! shows, a warning where it can't work here, and a menu.
//!
//! Every move has three ways, so none needs a mouse: drag a row within or between lanes (a line
//! shows where it lands); with the keyboard, Alt+↑/↓ move the focused row within its lane and
//! Alt+←/→ to the lane beside it; or the row's menu (_Move Up_, _Move Down_, _Move to …_). The
//! focus follows the moved row, and a screen reader hears where it went. Delete or the menu's
//! _Remove_ take an item out, with a note that offers _Undo_ (Ctrl+Z works too). Enter or a
//! double click opens an item's settings; _Add Item…_ opens the picker for the lane.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bar::{Modules, catalog};
use sway_things_core::registry::{Value, bar as keys};

use super::preview::{item_missing, item_sample, item_title, spoken};
use super::toast::Toast;
use super::{Ctx, dialog, follow, picker};
use crate::i18n::{Msg, msg, msg_plural, tr, trf};

/// How to move a row, for its tooltip and accessible description.
const HOW_TO_MOVE: Msg = msg!(
    "Drag to move it. Alt+Up and Alt+Down move it in its lane, Alt+Left and Alt+Right to the \
     lane beside it; Delete removes it, Enter opens its settings"
);
/// A row's accessible name.
const ROW_NAME: Msg = msg!("{item}, {position} of {count} in {lane}");
/// Said after a move.
const MOVED: Msg = msg!("{item} moved to {lane}, position {position} of {count}");
/// Shown after a removal.
const REMOVED: Msg = msg!("Removed {item} from the bar.");
/// The menu's move to another lane.
const MOVE_TO: Msg = msg!("Move to {lane}");

/// The lanes, left to right, with their names.
pub(crate) const LANES: [(&str, Msg); 3] = [
    (keys::MODULES_LEFT, msg!("Left")),
    (keys::MODULES_CENTER, msg!("Center")),
    (keys::MODULES_RIGHT, msg!("Right")),
];

thread_local! {
    /// The row that gets the focus once its lane is shown again, after a move: the lane and the
    /// place in it.
    static FOCUS: RefCell<Option<(String, usize)>> = const { RefCell::new(None) };
}

/// The name of lane `key`.
pub(crate) fn lane_name(key: &str) -> &'static str {
    LANES
        .iter()
        .find(|(lane, _)| *lane == key)
        .map_or("", |(_, name)| tr(*name))
}

/// The three lanes and the note after a removal.
#[derive(Debug, Clone)]
pub struct Lanes {
    root: gtk::Box,
}

impl Lanes {
    /// The lanes, following the settings.
    pub fn new(ctx: &Rc<Ctx>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.add_css_class("st-bar-lanes");
        ctx.set_voice(&root);
        let note = gtk::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class(crate::style::DIM);
        root.append(&note);
        let flow = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        flow.add_css_class("st-bar-lane-columns");
        root.append(&flow);
        let weak = Rc::downgrade(ctx);
        let toast = Toast::new(move || {
            if let Some(ctx) = weak.upgrade() {
                ctx.undo();
            }
        });
        root.append(toast.widget());
        let lanes: Vec<Lane> = LANES
            .iter()
            .map(|(key, title)| Lane::new(ctx, key, tr(*title), &toast))
            .collect();
        let widths = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        for (index, lane) in lanes.iter().enumerate() {
            widths.add_widget(&lane.root);
            if index > 0 {
                flow.append(&gtk::Separator::new(gtk::Orientation::Vertical));
            }
            lane.root.set_hexpand(true);
            // Each lane gets the same width, whatever its items' names.
            lane.root.set_size_request(0, -1);
            flow.append(&lane.root);
        }
        let weak = Rc::downgrade(ctx);
        follow(ctx.model(), &root, move || {
            let Some(ctx) = weak.upgrade() else { return };
            let modules = ctx
                .model()
                .bar_modules()
                .and_then(Result::ok)
                .unwrap_or_default();
            let locked = ctx.locked(keys::MODULES_LEFT);
            for lane in &lanes {
                lane.refresh(&ctx, &modules);
            }
            flow.set_sensitive(locked.is_none() && ctx.writable());
            let swaybar =
                matches!(ctx.value(keys::BACKEND), Some(Value::Choice(id)) if id == "swaybar");
            match (&locked, swaybar) {
                (Some(_), true) => {
                    note.set_text(tr(msg!(
                        "swaybar doesn't show these items: it shows the workspaces, the binding \
                         mode and the text of a status command. They stay saved for Waybar."
                    )));
                    note.set_visible(true);
                }
                (Some(reason), false) => {
                    note.set_text(reason);
                    note.set_visible(true);
                }
                (None, _) => note.set_visible(false),
            }
        });
        Self { root }
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }
}

/// One lane: its heading, its rows and _Add Item…_.
struct Lane {
    key: &'static str,
    root: gtk::Box,
    list: gtk::ListBox,
    count: gtk::Label,
    toast: Toast,
    /// What the rows show, to rebuild them only when it changed.
    shown: RefCell<Vec<String>>,
}

impl Lane {
    fn new(ctx: &Rc<Ctx>, key: &'static str, title: &str, toast: &Toast) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("st-bar-lane");
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        heading.add_css_class("st-bar-lane-heading");
        let name = gtk::Label::new(Some(title));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.add_css_class("heading");
        name.set_accessible_role(gtk::AccessibleRole::Heading);
        let count = gtk::Label::new(None);
        count.add_css_class(crate::style::DIM);
        count.add_css_class("st-row-detail");
        heading.append(&name);
        heading.append(&count);
        root.append(&heading);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_activate_on_single_click(false);
        list.add_css_class("st-bar-lane-list");
        list.update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("Items on the {lane}"),
            &[("lane", &title)],
        ))]);
        let empty = gtk::Label::new(Some(tr(msg!("Empty. Drag items here or add one."))));
        empty.set_wrap(true);
        empty.add_css_class(crate::style::DIM);
        empty.add_css_class("st-bar-lane-empty");
        list.set_placeholder(Some(&empty));
        root.append(&list);
        let add = gtk::Button::new();
        let add_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        add_content.append(&gtk::Image::from_icon_name("list-add-symbolic"));
        add_content.append(&gtk::Label::new(Some(tr(msg!("Add Item…")))));
        add.set_child(Some(&add_content));
        add.add_css_class("flat");
        add.add_css_class("st-bar-lane-add");
        add.update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("Add an item to the {lane}"),
            &[("lane", &title)],
        ))]);
        let weak = Rc::downgrade(ctx);
        add.connect_clicked(move |button| {
            if let Some(ctx) = weak.upgrade() {
                picker::open(&ctx, button.upcast_ref(), key);
            }
        });
        root.append(&add);
        let weak = Rc::downgrade(ctx);
        list.connect_row_activated(move |_, row| {
            if let Some(ctx) = weak.upgrade() {
                dialog::open(&ctx, row.upcast_ref(), &row.widget_name());
            }
        });
        drop_onto(ctx, &root, &list, key);
        Self {
            key,
            root,
            list,
            count: count.clone(),
            toast: toast.clone(),
            shown: RefCell::new(Vec::new()),
        }
    }

    /// Shows the items of the lane now, rebuilt only if they changed; the focus stays on its row
    /// or follows a moved one.
    fn refresh(&self, ctx: &Rc<Ctx>, modules: &Modules) {
        let names = ctx.list(self.key);
        self.count.set_text(&crate::i18n::ntrf(
            msg_plural!("{count} item", "{count} items"),
            names.len(),
            &[("count", &names.len())],
        ));
        // Each row's name and what it shows: a changed format shows at once.
        let signature: Vec<String> = names
            .iter()
            .map(|name| {
                format!(
                    "{name}\u{1f}{}\u{1f}{}",
                    item_title(modules, name),
                    item_sample(modules, name).unwrap_or_default()
                )
            })
            .collect();
        let unchanged = *self.shown.borrow() == signature;
        let wanted = FOCUS.with(|focus| match focus.borrow().as_ref() {
            Some((lane, index)) if lane == self.key => Some(*index),
            _ => None,
        });
        if unchanged && wanted.is_none() {
            return;
        }
        self.shown.replace(signature);
        let focused = self
            .list
            .focus_child()
            .and_downcast::<gtk::ListBoxRow>()
            .map(|row| row.index());
        self.list.remove_all();
        for (index, name) in names.iter().enumerate() {
            self.list.append(&row(
                ctx,
                self.key,
                index,
                names.len(),
                name,
                modules,
                &self.toast,
            ));
        }
        let index = wanted
            .and_then(|index| {
                FOCUS.with(|focus| focus.replace(None));
                i32::try_from(index).ok()
            })
            .or(focused);
        if let Some(row) = index.and_then(|index| self.list.row_at_index(index)) {
            row.grab_focus();
        }
    }
}

/// The row of item `name` at `index` of `count` in lane `key`.
fn row(
    ctx: &Rc<Ctx>,
    key: &'static str,
    index: usize,
    count: usize,
    name: &str,
    modules: &Modules,
    toast: &Toast,
) -> gtk::ListBoxRow {
    let title = item_title(modules, name);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.add_css_class("st-bar-item-row");
    let icon = gtk::Image::from_icon_name(catalog::icon(name));
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    content.append(&icon);
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 0);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    let label = gtk::Label::new(Some(&title));
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    texts.append(&label);
    let missing = item_missing(name);
    let detail = match (
        &missing,
        item_sample(modules, name).map(|sample| spoken(&sample)),
    ) {
        (Some(reason), _) => Some(reason.clone()),
        (None, Some(sample)) if !sample.is_empty() && sample != title => Some(sample),
        _ => None,
    };
    if let Some(detail) = &detail {
        let sample = gtk::Label::new(Some(detail));
        sample.set_xalign(0.0);
        sample.set_ellipsize(gtk::pango::EllipsizeMode::End);
        sample.add_css_class(crate::style::DIM);
        sample.add_css_class("st-row-detail");
        texts.append(&sample);
    }
    content.append(&texts);
    if missing.is_some() {
        let warning = gtk::Image::from_icon_name("dialog-warning-symbolic");
        warning.set_tooltip_text(missing.as_deref());
        warning.update_property(&[gtk::accessible::Property::Label(
            missing.as_deref().unwrap_or_default(),
        )]);
        content.append(&warning);
    }
    let menu = gtk::MenuButton::new();
    menu.set_icon_name("view-more-symbolic");
    menu.set_has_frame(false);
    menu.set_valign(gtk::Align::Center);
    menu.set_tooltip_text(Some(tr(msg!("Item menu"))));
    menu.update_property(&[gtk::accessible::Property::Label(&trf(
        msg!("Menu of {item}"),
        &[("item", &title)],
    ))]);
    content.append(&menu);
    let row = gtk::ListBoxRow::new();
    // The pointer says a row can be picked up.
    row.set_cursor_from_name(Some("grab"));
    row.set_widget_name(name);
    row.set_child(Some(&content));
    row.set_tooltip_text(Some(tr(HOW_TO_MOVE)));
    let lane = lane_name(key);
    row.update_property(&[
        gtk::accessible::Property::Label(&trf(
            ROW_NAME,
            &[
                ("item", &title),
                ("position", &(index + 1)),
                ("count", &count),
                ("lane", &lane),
            ],
        )),
        gtk::accessible::Property::Description(tr(HOW_TO_MOVE)),
    ]);
    menu.set_popover(Some(&item_menu(ctx, key, index, count, name, toast)));
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    source.set_content(Some(&gdk::ContentProvider::for_value(
        &format!("{key}\n{index}").to_value(),
    )));
    let weak_content = content.downgrade();
    source.connect_drag_begin(move |source, _| {
        if let Some(content) = weak_content.upgrade() {
            let icon = gtk::WidgetPaintable::new(Some(&content));
            source.set_icon(Some(&icon), 0, 0);
        }
    });
    row.add_controller(source);
    add_keys(ctx, &row, key, index, toast);
    row
}

/// What an entry of an item's menu does, with the widget the menu belongs to.
type Action = Box<dyn Fn(&Rc<Ctx>, &gtk::Widget)>;

/// The menu of an item: its settings, the moves, and _Remove_.
fn item_menu(
    ctx: &Rc<Ctx>,
    key: &'static str,
    index: usize,
    count: usize,
    name: &str,
    toast: &Toast,
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("st-bar-menu");
    let entry = |label: &str, action: Action, sensitive: bool| {
        let button = gtk::Button::with_label(label);
        button.add_css_class("flat");
        if let Some(child) = button.child().and_downcast::<gtk::Label>() {
            child.set_xalign(0.0);
        }
        button.set_sensitive(sensitive);
        let weak = Rc::downgrade(ctx);
        let popover = popover.downgrade();
        button.connect_clicked(move |button| {
            let anchor = popover
                .upgrade()
                .and_then(|popover| {
                    popover.popdown();
                    popover.parent()
                })
                .unwrap_or_else(|| button.clone().upcast());
            if let Some(ctx) = weak.upgrade() {
                action(&ctx, &anchor);
            }
        });
        content.append(&button);
    };
    let item = name.to_owned();
    entry(
        tr(msg!("Settings…")),
        Box::new(move |ctx, anchor| dialog::open(ctx, anchor, &item)),
        true,
    );
    entry(
        tr(msg!("Move Up")),
        Box::new(move |ctx, _| move_item(ctx, key, index, key, index.saturating_sub(1))),
        index > 0,
    );
    entry(
        tr(msg!("Move Down")),
        Box::new(move |ctx, _| move_item(ctx, key, index, key, index + 2)),
        index + 1 < count,
    );
    for (lane, title) in LANES {
        if lane == key {
            continue;
        }
        entry(
            &trf(MOVE_TO, &[("lane", &tr(title))]),
            Box::new(move |ctx, _| move_item(ctx, key, index, lane, usize::MAX)),
            true,
        );
    }
    let toast = toast.clone();
    entry(
        tr(msg!("Remove")),
        Box::new(move |ctx, _| remove_item(ctx, key, index, &toast)),
        true,
    );
    popover.set_child(Some(&content));
    popover
}

/// The keys of a row: Alt and the arrows move it, Delete removes it.
fn add_keys(ctx: &Rc<Ctx>, row: &gtk::ListBoxRow, key: &'static str, index: usize, toast: &Toast) {
    let keys = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(ctx);
    let toast = toast.clone();
    keys.connect_key_pressed(move |_, pressed, _, modifiers| {
        let Some(ctx) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);
        match pressed {
            gdk::Key::Up | gdk::Key::KP_Up if alt => {
                if let Some(to) = index.checked_sub(1) {
                    move_item(&ctx, key, index, key, to);
                }
                glib::Propagation::Stop
            }
            gdk::Key::Down | gdk::Key::KP_Down if alt => {
                if index + 1 < ctx.list(key).len() {
                    move_item(&ctx, key, index, key, index + 2);
                }
                glib::Propagation::Stop
            }
            gdk::Key::Left | gdk::Key::KP_Left if alt => {
                if let Some(to) = neighbor(key, -1) {
                    move_item(&ctx, key, index, to, usize::MAX);
                }
                glib::Propagation::Stop
            }
            gdk::Key::Right | gdk::Key::KP_Right if alt => {
                if let Some(to) = neighbor(key, 1) {
                    move_item(&ctx, key, index, to, usize::MAX);
                }
                glib::Propagation::Stop
            }
            gdk::Key::Delete | gdk::Key::KP_Delete | gdk::Key::BackSpace => {
                remove_item(&ctx, key, index, &toast);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    row.add_controller(keys);
}

/// The lane left (`-1`) or right (`1`) of `key`.
fn neighbor(key: &str, step: isize) -> Option<&'static str> {
    let at = LANES.iter().position(|(lane, _)| *lane == key)?;
    LANES
        .get(at.checked_add_signed(step)?)
        .map(|(lane, _)| *lane)
}

/// Rows dropped onto lane `key` (its `root`, which holds `list`): put before or after the row
/// under the pointer, whichever half it is over, or at the end. A line shows where.
fn drop_onto(ctx: &Rc<Ctx>, root: &gtk::Box, list: &gtk::ListBox, key: &'static str) {
    let target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
    let marked: Rc<RefCell<Option<gtk::ListBoxRow>>> = Rc::new(RefCell::new(None));
    let unmark = {
        let marked = marked.clone();
        move || {
            if let Some(row) = marked.take() {
                row.remove_css_class("st-drop-before");
                row.remove_css_class("st-drop-after");
            }
        }
    };
    // The place in `list` the pointer at `y` of `root` is over: the row and whether after it.
    let place = {
        let root = root.downgrade();
        let list = list.downgrade();
        move |x: f64, y: f64| -> Option<(gtk::ListBoxRow, bool)> {
            let (root, list) = (root.upgrade()?, list.upgrade()?);
            #[expect(clippy::cast_possible_truncation, reason = "pixels of a widget")]
            let point =
                root.compute_point(&list, &gtk::graphene::Point::new(x as f32, y as f32))?;
            #[expect(clippy::cast_possible_truncation, reason = "pixels of a widget")]
            let row = list.row_at_y(point.y() as i32).or_else(|| {
                // Below the last row: after it.
                let count = list.observe_children().n_items();
                let final_index = i32::try_from(count).ok()?.checked_sub(1)?;
                (0..=final_index)
                    .rev()
                    .find_map(|index| list.row_at_index(index))
            })?;
            let bounds = row.compute_bounds(&list)?;
            Some((row, point.y() > bounds.y() + bounds.height() / 2.0))
        }
    };
    let motion_place = place.clone();
    let motion_unmark = unmark.clone();
    let motion_marked = marked.clone();
    target.connect_motion(move |_, x, y| {
        motion_unmark();
        if let Some((row, after)) = motion_place(x, y) {
            row.add_css_class(if after {
                "st-drop-after"
            } else {
                "st-drop-before"
            });
            motion_marked.replace(Some(row));
        }
        gdk::DragAction::MOVE
    });
    let leave_unmark = unmark.clone();
    target.connect_leave(move |_| leave_unmark());
    let weak = Rc::downgrade(ctx);
    target.connect_drop(move |_, value, x, y| {
        unmark();
        let Some(ctx) = weak.upgrade() else {
            return false;
        };
        let Ok(payload) = value.get::<String>() else {
            return false;
        };
        let Some((from_key, from)) = payload.split_once('\n') else {
            return false;
        };
        let (Ok(from), Some(from_key)) = (
            from.parse::<usize>(),
            LANES
                .iter()
                .map(|(lane, _)| *lane)
                .find(|lane| *lane == from_key),
        ) else {
            return false;
        };
        let to = place(x, y).map_or(usize::MAX, |(row, after)| {
            usize::try_from(row.index()).map_or(usize::MAX, |index| index + usize::from(after))
        });
        move_item(&ctx, from_key, from, key, to);
        true
    });
    root.add_controller(target);
}

/// Moves the item at `from` of lane `from_key` to the place before `to` of lane `to_key` (the
/// end for `usize::MAX`), as one step of Ctrl+Z, and says where it went.
pub(crate) fn move_item(
    ctx: &Rc<Ctx>,
    from_key: &'static str,
    from: usize,
    to_key: &'static str,
    to: usize,
) {
    let mut from_list = ctx.list(from_key);
    if from >= from_list.len() {
        return;
    }
    let step = ctx.snapshot(&[from_key, to_key]);
    let name = from_list.remove(from);
    let (to, count) = if from_key == to_key {
        // The place before `to`, counted in the list without the item moved.
        let to = if to > from { to - 1 } else { to }.min(from_list.len());
        if to == from {
            return;
        }
        from_list.insert(to, name.clone());
        let count = from_list.len();
        ctx.remember(step);
        FOCUS.with(|focus| focus.replace(Some((to_key.to_owned(), to))));
        ctx.model().set(from_key, None, Value::List(from_list));
        (to, count)
    } else {
        let mut to_list = ctx.list(to_key);
        let to = to.min(to_list.len());
        to_list.insert(to, name.clone());
        let count = to_list.len();
        ctx.remember(step);
        FOCUS.with(|focus| focus.replace(Some((to_key.to_owned(), to))));
        // Added first, then removed: the item is never missing from the bar meanwhile.
        ctx.model().set(to_key, None, Value::List(to_list));
        ctx.model().set(from_key, None, Value::List(from_list));
        (to, count)
    };
    let modules = ctx
        .model()
        .bar_modules()
        .and_then(Result::ok)
        .unwrap_or_default();
    ctx.announce(&trf(
        MOVED,
        &[
            ("item", &item_title(&modules, &name)),
            ("lane", &lane_name(to_key)),
            ("position", &(to + 1)),
            ("count", &count),
        ],
    ));
}

/// Removes the item at `index` of lane `key`, as one step of Ctrl+Z, with a note to undo it.
pub(crate) fn remove_item(ctx: &Rc<Ctx>, key: &'static str, index: usize, toast: &Toast) {
    let mut list = ctx.list(key);
    if index >= list.len() {
        return;
    }
    ctx.remember(ctx.snapshot(&[key]));
    let name = list.remove(index);
    // The focus goes to the row that takes its place, or the one before.
    if !list.is_empty() {
        FOCUS.with(|focus| focus.replace(Some((key.to_owned(), index.min(list.len() - 1)))));
    }
    ctx.model().set(key, None, Value::List(list));
    let modules = ctx
        .model()
        .bar_modules()
        .and_then(Result::ok)
        .unwrap_or_default();
    toast.show(&trf(REMOVED, &[("item", &item_title(&modules, &name))]));
}

/// Adds item `name` at the end of lane `key`, as one step of Ctrl+Z; the focus goes to it.
pub(crate) fn add_item(ctx: &Rc<Ctx>, key: &'static str, name: &str) {
    let mut list = ctx.list(key);
    ctx.remember(ctx.snapshot(&[key]));
    list.push(name.to_owned());
    FOCUS.with(|focus| focus.replace(Some((key.to_owned(), list.len() - 1))));
    ctx.model().set(key, None, Value::List(list));
}

/// A name for another item of `kind` that no lane has yet: `kind`, else `kind#2`, `kind#3`, …
pub(crate) fn free_name(ctx: &Ctx, kind: &str) -> String {
    let taken: Vec<String> = LANES.iter().flat_map(|(lane, _)| ctx.list(lane)).collect();
    if !taken.iter().any(|name| name == kind) {
        return kind.to_owned();
    }
    (2..=taken.len() + 2)
        .map(|n| format!("{kind}#{n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_else(|| kind.to_owned())
}

/// Whether a lane has item `name`.
pub(crate) fn taken(ctx: &Ctx, name: &str) -> bool {
    LANES
        .iter()
        .any(|(lane, _)| ctx.list(lane).iter().any(|item| item == name))
}
