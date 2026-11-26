// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The items of the bar in three lanes, _Left_, _Center_ and _Right_, side by side as on the
//! bar. Each lane is a list of rows: the item's icon and name, what it shows, a warning where it
//! can't work here, and a menu. A group (several items in one pill, waybar's `group/<name>`) is a
//! card in its lane: a header with its name, which can be edited in place, a drawer's symbol and
//! the number of its items, and its members as rows inside it; in a drawer the first member is
//! marked _Always shown_ and the others _Shown on hover_.
//!
//! Every move has three ways, so none needs a mouse: drag a row within or between lanes, into a
//! group or out of it (a line shows where it lands); with the keyboard, Alt+↑/↓ move the focused
//! row one step, into a group it passes and out at its ends, and Alt+←/→ to the lane beside it;
//! or the row's menu. Dropping an item onto the middle of another makes the two a group, which
//! gets a name from its items, ready to be changed; the menu's _Group With…_ does the same. A
//! group's menu renames it, makes it a drawer or a plain pill, sets where the drawer opens, and
//! ungroups it. The focus follows the moved row, and a screen reader hears where it went
//! ("in group Audio, 2 of 3"). Delete or the menu's _Remove_ take an item out, with a note that
//! offers _Undo_ (Ctrl+Z works too). Enter or a double click opens an item's settings; _Add
//! Item…_ opens the picker for the lane. Pointing at a group marks its pill in the preview and
//! opens its drawer there.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bar::groups::{self, Drawer, Place};
use sway_things_core::bar::{Modules, catalog};
use sway_things_core::registry::{Value, bar as keys};

use super::preview::{group_title, item_missing, item_sample, item_title, spoken};
use super::toast::Toast;
use super::{Ctx, dialog, follow, picker};
use crate::i18n::{Msg, msg, msg_plural, ntrf, tr, trf};

/// How to move a row, for its tooltip and accessible description.
const HOW_TO_MOVE: Msg = msg!(
    "Drag to move it, or onto another item to make one pill of both. Alt+Up and Alt+Down move \
     it, also into and out of groups, Alt+Left and Alt+Right to the lane beside it; Delete \
     removes it, Enter opens its settings"
);
/// A row's accessible name.
const ROW_NAME: Msg = msg!("{item}, {position} of {count} in {lane}");
/// A member row's accessible name.
const MEMBER_NAME: Msg = msg!("{item}, in group {group}, {position} of {count}");
/// A group's accessible name.
const GROUP_NAME: Msg = msg!("Group {group} with {count} items, {position} of {total} in {lane}");
/// Said after a move.
const MOVED: Msg = msg!("{item} moved to {lane}, position {position} of {count}");
/// Said after a move into a group.
const MOVED_INTO: Msg = msg!("{item} moved into group {group}, {position} of {count}");
/// Shown after a removal.
const REMOVED: Msg = msg!("Removed {item} from the bar.");
/// Said when a group is made.
const GROUPED: Msg = msg!("{first} and {second} are now group {group}, one pill on the bar");
/// The menu's move to another lane.
const MOVE_TO: Msg = msg!("Move to {lane}");
/// The badge of a drawer's first member.
const ALWAYS: Msg = msg!("Always shown");
/// The badge of a drawer's other members.
const ON_HOVER: Msg = msg!("Shown on hover");
/// What a drop onto the middle of an item does.
const DROP_TO_GROUP: Msg = msg!("Drop to group");
/// The name of the hint "Drop to group" in a row, to find it again.
const HINT: &str = "st-drop-hint";

/// The lanes, left to right, with their names.
pub(crate) const LANES: [(&str, Msg); 3] = [
    (keys::MODULES_LEFT, msg!("Left")),
    (keys::MODULES_CENTER, msg!("Center")),
    (keys::MODULES_RIGHT, msg!("Right")),
];

thread_local! {
    /// The row that gets the focus once its lane is shown again, after a move.
    static FOCUS: RefCell<Option<Place>> = const { RefCell::new(None) };
    /// The group whose name is edited once it is shown, after it was made.
    static RENAME: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Whether the tip about groups was put away in this session.
    static TIP_DISMISSED: Cell<bool> = const { Cell::new(false) };
}

/// The name of lane `key`.
pub(crate) fn lane_name(key: &str) -> &'static str {
    LANES
        .iter()
        .find(|(lane, _)| *lane == key)
        .map_or("", |(_, name)| tr(*name))
}

/// The index of lane `key`.
fn lane_index(key: &str) -> usize {
    LANES.iter().position(|(lane, _)| *lane == key).unwrap_or(0)
}

/// The three lists of the lanes.
fn lists(ctx: &Ctx) -> Vec<Vec<String>> {
    LANES.iter().map(|(key, _)| ctx.list(key)).collect()
}

/// Stores a change of the lanes and the groups as one step of Ctrl+Z: `lists` and `modules`
/// as they are after it, `before` the definitions before it. The definitions go first, so a
/// list never names a group they don't define yet.
fn commit(ctx: &Ctx, before: Modules, lists: &[Vec<String>], modules: Modules) {
    let keys: Vec<&'static str> = LANES.iter().map(|(key, _)| *key).collect();
    let old = self::lists(ctx);
    let changed_modules = modules != before;
    ctx.remember_with(ctx.snapshot(&keys), changed_modules.then_some(before));
    if changed_modules {
        ctx.model().set_bar_modules(modules);
    }
    // Lists that gain items first, then those that lose them: an item is never missing.
    let mut order: Vec<usize> = (0..LANES.len()).collect();
    order.sort_by_key(|lane| old[*lane].len() >= lists[*lane].len());
    for lane in order {
        if old[lane] != lists[lane] {
            ctx.model()
                .set(LANES[lane].0, None, Value::List(lists[lane].clone()));
        }
    }
}

/// The three lanes, the tip about groups and the note after a removal.
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
        let tip = tip();
        root.append(&tip);
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
        let lanes: Vec<Rc<Lane>> = LANES
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
            let modules = ctx.modules();
            let locked = ctx.locked(keys::MODULES_LEFT);
            for lane in &lanes {
                lane.refresh(&ctx, &modules);
            }
            let grouped = lists(&ctx)
                .iter()
                .flatten()
                .any(|name| groups::is_group(name));
            tip.set_visible(!grouped && !TIP_DISMISSED.with(Cell::get) && locked.is_none());
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

/// The tip that items combine into one pill, until there is a group or it is put away.
fn tip() -> gtk::Box {
    let tip = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    tip.add_css_class("st-bar-tip");
    let icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    tip.append(&icon);
    let label = gtk::Label::new(Some(tr(msg!(
        "Tip: drag an item onto another to combine them into one pill. A group can also show \
         its other items only on hover."
    ))));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_hexpand(true);
    tip.append(&label);
    let close = gtk::Button::from_icon_name("window-close-symbolic");
    close.add_css_class("flat");
    close.set_valign(gtk::Align::Center);
    close.set_tooltip_text(Some(tr(msg!("Dismiss"))));
    close.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Dismiss the tip"
    )))]);
    let shown = tip.downgrade();
    close.connect_clicked(move |_| {
        TIP_DISMISSED.with(|dismissed| dismissed.set(true));
        if let Some(tip) = shown.upgrade() {
            tip.set_visible(false);
        }
    });
    tip.append(&close);
    tip
}

/// What a row of a lane stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// An item in the lane.
    Item,
    /// A group's card in the lane.
    Group,
    /// A member of a group.
    Member,
}

/// One lane: its heading, its rows and _Add Item…_.
struct Lane {
    key: &'static str,
    index: usize,
    root: gtk::Box,
    list: gtk::ListBox,
    count: gtk::Label,
    toast: Toast,
    /// What the rows show, to rebuild them only when it changed.
    shown: RefCell<Vec<String>>,
    /// The rows with what they stand for, for drops and the focus.
    rows: RefCell<Vec<(gtk::ListBoxRow, Place, Kind)>>,
    /// The names of the groups' cards, to edit after a group was made.
    names: RefCell<Vec<(String, gtk::EditableLabel)>>,
}

impl Lane {
    fn new(ctx: &Rc<Ctx>, key: &'static str, title: &str, toast: &Toast) -> Rc<Self> {
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
        let lane = Rc::new(Self {
            key,
            index: lane_index(key),
            root,
            list,
            count,
            toast: toast.clone(),
            shown: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
            names: RefCell::new(Vec::new()),
        });
        drop_onto(ctx, &lane);
        lane
    }

    /// Shows the items of the lane now, rebuilt only if they changed; the focus stays on its row
    /// or follows a moved one.
    fn refresh(&self, ctx: &Rc<Ctx>, modules: &Modules) {
        let names = ctx.list(self.key);
        self.count.set_text(&ntrf(
            msg_plural!("{count} item", "{count} items"),
            names.len(),
            &[("count", &names.len())],
        ));
        // Each row's name and what it shows, and each group's members and drawer: a change
        // shows at once.
        let signature: Vec<String> = names
            .iter()
            .map(|name| {
                let mut line = format!(
                    "{name}\u{1f}{}\u{1f}{}",
                    item_title(modules, name),
                    item_sample(modules, name).unwrap_or_default()
                );
                if groups::is_group(name) {
                    let definition = modules.definition(name).unwrap_or_default();
                    let _ = write!(line, "\u{1f}{:?}", groups::drawer(&definition));
                    for member in groups::members_of(modules, name) {
                        let _ = write!(
                            line,
                            "\u{1f}{member}\u{1f}{}\u{1f}{}",
                            item_title(modules, &member),
                            item_sample(modules, &member).unwrap_or_default()
                        );
                    }
                }
                line
            })
            .collect();
        let unchanged = *self.shown.borrow() == signature;
        let wanted = FOCUS.with(|focus| match *focus.borrow() {
            Some(place @ (Place::Lane(lane, _) | Place::Member(lane, _, _)))
                if lane == self.index =>
            {
                Some(place)
            }
            _ => None,
        });
        let rename = RENAME.with(|rename| rename.borrow().clone());
        let renaming = rename.as_ref().is_some_and(|name| names.contains(name));
        if unchanged && wanted.is_none() && !renaming {
            return;
        }
        self.shown.replace(signature);
        let focused = self
            .rows
            .borrow()
            .iter()
            .find(|(row, _, _)| row.is_focus())
            .map(|(_, place, _)| *place);
        self.list.remove_all();
        self.rows.borrow_mut().clear();
        self.names.borrow_mut().clear();
        for (index, name) in names.iter().enumerate() {
            let place = Place::Lane(self.index, index);
            if groups::is_group(name) {
                let row = self.group_card(ctx, index, names.len(), name, modules);
                self.list.append(&row);
            } else {
                let row = self.item_row(ctx, place, names.len(), name, modules, None);
                self.list.append(&row);
                self.rows.borrow_mut().push((row, place, Kind::Item));
            }
        }
        // The lists and the definitions are stored one after the other: a moved row may show
        // only once both are, so the focus waits for it.
        if let Some(target) = wanted.or(focused) {
            let found = self
                .rows
                .borrow()
                .iter()
                .find(|(_, place, _)| *place == target)
                .map(|(row, _, _)| row.clone());
            if let Some(row) = found {
                row.grab_focus();
                if wanted.is_some() {
                    FOCUS.with(|focus| focus.replace(None));
                }
            }
        }
        if let Some(name) = rename.filter(|_| renaming) {
            RENAME.with(|rename| rename.replace(None));
            let editable = self
                .names
                .borrow()
                .iter()
                .find(|(group, _)| *group == name)
                .map(|(_, editable)| editable.clone());
            if let Some(editable) = editable {
                editable.grab_focus();
                editable.start_editing();
            }
        }
    }

    /// The row of item `name` at `place`, among `count` in its lane or group (`group`, its name
    /// and whether it is a drawer).
    #[expect(
        clippy::too_many_lines,
        reason = "the parts of one row, read best in order"
    )]
    fn item_row(
        &self,
        ctx: &Rc<Ctx>,
        place: Place,
        count: usize,
        name: &str,
        modules: &Modules,
        group: Option<(&str, bool)>,
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
        // In a drawer: which members always show and which only on hover.
        let member = match place {
            Place::Member(_, _, member) => Some(member),
            Place::Lane(..) => None,
        };
        let badge = match (group, member) {
            (Some((_, true)), Some(0)) => Some(tr(ALWAYS)),
            (Some((_, true)), Some(_)) => Some(tr(ON_HOVER)),
            _ => None,
        };
        if let Some(badge) = badge {
            let label = gtk::Label::new(Some(badge));
            label.set_xalign(0.0);
            label.add_css_class("st-bar-member-badge");
            if member == Some(0) {
                label.add_css_class("st-bar-always");
            }
            texts.append(&label);
        }
        // Shown below the name while an item is dragged over the middle of this one.
        let hint = gtk::Label::new(Some(tr(DROP_TO_GROUP)));
        hint.add_css_class("st-bar-drop-hint");
        hint.set_widget_name(HINT);
        hint.set_visible(false);
        hint.set_halign(gtk::Align::Start);
        texts.append(&hint);
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
        let accessible = match (group, place) {
            (Some((group, _)), Place::Member(_, _, member)) => trf(
                MEMBER_NAME,
                &[
                    ("item", &title),
                    ("group", &group_title(group)),
                    ("position", &(member + 1)),
                    ("count", &count),
                ],
            ),
            (_, Place::Lane(_, index) | Place::Member(_, index, _)) => trf(
                ROW_NAME,
                &[
                    ("item", &title),
                    ("position", &(index + 1)),
                    ("count", &count),
                    ("lane", &lane_name(self.key)),
                ],
            ),
        };
        row.update_property(&[
            gtk::accessible::Property::Label(&accessible),
            gtk::accessible::Property::Description(tr(HOW_TO_MOVE)),
        ]);
        let entries = match group {
            Some((_, drawer)) => member_menu(place, count, name, drawer, &self.toast),
            None => item_menu(place, count, name, &self.toast),
        };
        menu.set_popover(Some(&popover(ctx, entries)));
        drag_source(&row, &content, place);
        add_keys(ctx, &row, place, &self.toast);
        row
    }

    /// The card of group `name` at `index` of `count` in the lane: its header (name, drawer,
    /// number of items, menu) and its members as rows.
    #[expect(
        clippy::too_many_lines,
        reason = "the parts of one card, read best in order"
    )]
    fn group_card(
        &self,
        ctx: &Rc<Ctx>,
        index: usize,
        count: usize,
        name: &str,
        modules: &Modules,
    ) -> gtk::ListBoxRow {
        let place = Place::Lane(self.index, index);
        let definition = modules.definition(name).unwrap_or_default();
        let drawer = groups::drawer(&definition);
        let members = groups::members_of(modules, name);
        let title = group_title(name);
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("st-bar-group-card");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.add_css_class("st-bar-group-header");
        let about = tr(if drawer.is_some() {
            msg!("A drawer: the first item always shows, the others on hover")
        } else {
            msg!("One pill: all its items always show")
        });
        let icon = gtk::Image::from_icon_name(if drawer.is_some() {
            "sidebar-show-symbolic"
        } else {
            "view-dual-symbolic"
        });
        icon.set_tooltip_text(Some(about));
        icon.update_property(&[gtk::accessible::Property::Label(about)]);
        header.append(&icon);
        let editable = gtk::EditableLabel::new(&title);
        editable.set_hexpand(true);
        editable.add_css_class("st-bar-group-name");
        editable.set_tooltip_text(Some(tr(msg!("The group's name; click to change it"))));
        editable.update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("Name of group {group}"),
            &[("group", &title)],
        ))]);
        {
            let weak = Rc::downgrade(ctx);
            let group = name.to_owned();
            let shown = title.clone();
            editable.connect_editing_notify(move |editable| {
                if editable.is_editing() {
                    return;
                }
                let text = editable.text().trim().to_owned();
                if text.is_empty() || text == shown {
                    editable.set_text(&shown);
                    return;
                }
                if let Some(ctx) = weak.upgrade() {
                    rename(&ctx, &group, &text);
                }
            });
        }
        header.append(&editable);
        let items = gtk::Label::new(Some(&ntrf(
            msg_plural!("{count} item", "{count} items"),
            members.len(),
            &[("count", &members.len())],
        )));
        items.add_css_class(crate::style::DIM);
        items.add_css_class("st-row-detail");
        header.append(&items);
        let menu = gtk::MenuButton::new();
        menu.set_icon_name("view-more-symbolic");
        menu.set_has_frame(false);
        menu.set_valign(gtk::Align::Center);
        menu.set_tooltip_text(Some(tr(msg!("Group menu"))));
        menu.update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("Menu of group {group}"),
            &[("group", &title)],
        ))]);
        menu.set_popover(Some(&popover(
            ctx,
            group_menu(
                place,
                count,
                name,
                drawer,
                members.len(),
                &self.toast,
                &editable,
            ),
        )));
        header.append(&menu);
        card.append(&header);
        let inner = gtk::ListBox::new();
        inner.set_selection_mode(gtk::SelectionMode::None);
        inner.set_activate_on_single_click(false);
        inner.add_css_class("st-bar-group-list");
        inner.update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("Items of group {group}"),
            &[("group", &title)],
        ))]);
        let empty = gtk::Label::new(Some(tr(msg!("No items. Drag items here."))));
        empty.add_css_class(crate::style::DIM);
        empty.add_css_class("st-bar-lane-empty");
        inner.set_placeholder(Some(&empty));
        for (member, member_name) in members.iter().enumerate() {
            let member_place = Place::Member(self.index, index, member);
            let row = self.item_row(
                ctx,
                member_place,
                members.len(),
                member_name,
                modules,
                Some((name, drawer.is_some())),
            );
            row.add_css_class("st-bar-member");
            inner.append(&row);
            self.rows
                .borrow_mut()
                .push((row, member_place, Kind::Member));
        }
        {
            let weak = Rc::downgrade(ctx);
            inner.connect_row_activated(move |_, row| {
                if let Some(ctx) = weak.upgrade() {
                    dialog::open(&ctx, row.upcast_ref(), &row.widget_name());
                }
            });
        }
        card.append(&inner);
        let row = gtk::ListBoxRow::new();
        row.add_css_class("st-bar-group-row");
        row.set_widget_name(name);
        row.set_child(Some(&card));
        row.set_cursor_from_name(Some("grab"));
        row.update_property(&[
            gtk::accessible::Property::Label(&trf(
                GROUP_NAME,
                &[
                    ("group", &title),
                    ("count", &members.len()),
                    ("position", &(index + 1)),
                    ("total", &count),
                    ("lane", &lane_name(self.key)),
                ],
            )),
            gtk::accessible::Property::Description(tr(HOW_TO_MOVE)),
        ]);
        // Pointing at the group, or the focus in it, marks its pill in the preview and opens
        // its drawer there.
        let mark = |on: bool| {
            let weak = Rc::downgrade(ctx);
            let name = name.to_owned();
            move || {
                if let Some(ctx) = weak.upgrade() {
                    ctx.mark(on.then_some(name.as_str()));
                }
            }
        };
        let motion = gtk::EventControllerMotion::new();
        let enter = mark(true);
        motion.connect_enter(move |_, _, _| enter());
        let leave = mark(false);
        motion.connect_leave(move |_| leave());
        row.add_controller(motion);
        let focus = gtk::EventControllerFocus::new();
        let enter = mark(true);
        focus.connect_enter(move |_| enter());
        let leave = mark(false);
        focus.connect_leave(move |_| leave());
        row.add_controller(focus);
        drag_source(&row, &header, place);
        add_keys(ctx, &row, place, &self.toast);
        self.rows
            .borrow_mut()
            .push((row.clone(), place, Kind::Group));
        self.names.borrow_mut().push((name.to_owned(), editable));
        row
    }
}

/// The hint "Drop to group" of `row`, if it has one: in the texts of its content.
fn hint_of(row: &gtk::ListBoxRow) -> Option<gtk::Widget> {
    let mut child = row.child()?.first_child();
    while let Some(current) = child {
        let mut inner = current.first_child();
        while let Some(widget) = inner {
            if widget.widget_name() == HINT {
                return Some(widget);
            }
            inner = widget.next_sibling();
        }
        child = current.next_sibling();
    }
    None
}

/// The payload of a dragged row: where it is.
fn payload(place: Place) -> String {
    match place {
        Place::Lane(lane, index) => format!("{lane}\n{index}"),
        Place::Member(lane, group, member) => format!("{lane}\n{group}\n{member}"),
    }
}

/// The place a payload names.
fn place_of(payload: &str) -> Option<Place> {
    let numbers: Vec<usize> = payload
        .split('\n')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    match numbers[..] {
        [lane, index] if lane < LANES.len() => Some(Place::Lane(lane, index)),
        [lane, group, member] if lane < LANES.len() => Some(Place::Member(lane, group, member)),
        _ => None,
    }
}

/// Lets `row` be dragged by `content`, as the item at `place`.
fn drag_source(row: &gtk::ListBoxRow, content: &impl IsA<gtk::Widget>, place: Place) {
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    source.set_content(Some(&gdk::ContentProvider::for_value(
        &payload(place).to_value(),
    )));
    let weak_content = content.as_ref().downgrade();
    source.connect_drag_begin(move |source, _| {
        if let Some(content) = weak_content.upgrade() {
            let icon = gtk::WidgetPaintable::new(Some(&content));
            source.set_icon(Some(&icon), 0, 0);
        }
    });
    row.add_controller(source);
}

/// What an entry of a menu does, with the widget the menu belongs to.
type Action = Box<dyn Fn(&Rc<Ctx>, &gtk::Widget)>;

/// An entry of a menu: its text, whether it is on (a check mark) where it can be, whether it
/// can be picked, and what it does.
struct Entry {
    label: String,
    checked: Option<bool>,
    sensitive: bool,
    action: Action,
}

impl Entry {
    #[expect(
        clippy::unnecessary_wraps,
        reason = "an entry of a menu's list, in which `None` is a separator"
    )]
    fn new(label: &str, sensitive: bool, action: Action) -> Option<Self> {
        Some(Self {
            label: label.to_owned(),
            checked: None,
            sensitive,
            action,
        })
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "an entry of a menu's list, in which `None` is a separator"
    )]
    fn check(label: &str, checked: bool, action: Action) -> Option<Self> {
        Some(Self {
            label: label.to_owned(),
            checked: Some(checked),
            sensitive: true,
            action,
        })
    }
}

/// A menu of `entries`; `None` separates them.
fn popover(ctx: &Rc<Ctx>, entries: Vec<Option<Entry>>) -> gtk::Popover {
    let popover = gtk::Popover::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("st-bar-menu");
    for entry in entries {
        let Some(entry) = entry else {
            content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            continue;
        };
        let button = gtk::Button::new();
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        if let Some(checked) = entry.checked {
            let mark = gtk::Image::from_icon_name("object-select-symbolic");
            mark.set_opacity(if checked { 1.0 } else { 0.0 });
            mark.set_accessible_role(gtk::AccessibleRole::Presentation);
            line.append(&mark);
            button.set_accessible_role(gtk::AccessibleRole::Checkbox);
            button.update_state(&[gtk::accessible::State::Checked(if checked {
                gtk::AccessibleTristate::True
            } else {
                gtk::AccessibleTristate::False
            })]);
        }
        let label = gtk::Label::new(Some(&entry.label));
        label.set_xalign(0.0);
        line.append(&label);
        button.set_child(Some(&line));
        button.update_property(&[gtk::accessible::Property::Label(&entry.label)]);
        button.add_css_class("flat");
        button.set_sensitive(entry.sensitive);
        let weak = Rc::downgrade(ctx);
        let weak_popover = popover.downgrade();
        let action = entry.action;
        button.connect_clicked(move |button| {
            let anchor = weak_popover
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
    }
    popover.set_child(Some(&content));
    popover
}

/// Opens a menu of `entries` at `anchor`, gone again once closed.
fn popup(ctx: &Rc<Ctx>, anchor: &gtk::Widget, entries: Vec<Option<Entry>>) {
    let menu = popover(ctx, entries);
    menu.set_parent(anchor);
    menu.connect_closed(|menu| {
        let menu = menu.clone();
        glib::idle_add_local_once(move || menu.unparent());
    });
    menu.popup();
}

/// The moves of the item at `place` (one of `count` in its lane) to the other lanes.
fn lane_moves(place: Place, entries: &mut Vec<Option<Entry>>) {
    let Place::Lane(lane, _) = place else {
        return;
    };
    for (other, (_, title)) in LANES.iter().enumerate() {
        if other == lane {
            continue;
        }
        entries.push(Entry::new(
            &trf(MOVE_TO, &[("lane", &tr(*title))]),
            true,
            Box::new(move |ctx, _| move_to(ctx, place, Place::Lane(other, usize::MAX))),
        ));
    }
}

/// The menu of the item at `place` of a lane (one of `count`).
fn item_menu(place: Place, count: usize, name: &str, toast: &Toast) -> Vec<Option<Entry>> {
    let Place::Lane(_, index) = place else {
        return Vec::new();
    };
    let item = name.to_owned();
    let mut entries = vec![
        Entry::new(
            tr(msg!("Settings…")),
            true,
            Box::new(move |ctx, anchor| dialog::open(ctx, anchor, &item)),
        ),
        Entry::new(
            tr(msg!("Group With…")),
            count > 1,
            Box::new(move |ctx, anchor| group_with(ctx, anchor, place)),
        ),
        None,
        Entry::new(
            tr(msg!("Move Up")),
            index > 0,
            Box::new(move |ctx, _| step(ctx, place, -1)),
        ),
        Entry::new(
            tr(msg!("Move Down")),
            index + 1 < count,
            Box::new(move |ctx, _| step(ctx, place, 1)),
        ),
    ];
    lane_moves(place, &mut entries);
    let toast = toast.clone();
    entries.push(None);
    entries.push(Entry::new(
        tr(msg!("Remove")),
        true,
        Box::new(move |ctx, _| remove(ctx, place, &toast)),
    ));
    entries
}

/// The menu of the member at `place` of a group (one of `count`), a drawer or not.
fn member_menu(
    place: Place,
    count: usize,
    name: &str,
    drawer: bool,
    toast: &Toast,
) -> Vec<Option<Entry>> {
    let Place::Member(lane, group, member) = place else {
        return Vec::new();
    };
    let item = name.to_owned();
    let mut entries = vec![
        Entry::new(
            tr(msg!("Settings…")),
            true,
            Box::new(move |ctx, anchor| dialog::open(ctx, anchor, &item)),
        ),
        None,
    ];
    if drawer {
        entries.push(Entry::new(
            tr(msg!("Make Always Shown")),
            member > 0,
            Box::new(move |ctx, _| move_to(ctx, place, Place::Member(lane, group, 0))),
        ));
    }
    entries.push(Entry::new(
        tr(msg!("Move Up")),
        member > 0,
        Box::new(move |ctx, _| step(ctx, place, -1)),
    ));
    entries.push(Entry::new(
        tr(msg!("Move Down")),
        member + 1 < count,
        Box::new(move |ctx, _| step(ctx, place, 1)),
    ));
    entries.push(Entry::new(
        tr(msg!("Move Out of the Group")),
        true,
        Box::new(move |ctx, _| move_to(ctx, place, Place::Lane(lane, group + 1))),
    ));
    let toast = toast.clone();
    entries.push(None);
    entries.push(Entry::new(
        tr(msg!("Remove")),
        true,
        Box::new(move |ctx, _| remove(ctx, place, &toast)),
    ));
    entries
}

/// The menu of the group `name` at `place` (one of `count` in its lane, with `members` items):
/// rename, drawer or not, where it opens, ungroup.
fn group_menu(
    place: Place,
    count: usize,
    name: &str,
    drawer: Option<Drawer>,
    members: usize,
    toast: &Toast,
    editable: &gtk::EditableLabel,
) -> Vec<Option<Entry>> {
    let Place::Lane(_, index) = place else {
        return Vec::new();
    };
    let weak_editable = editable.downgrade();
    let group = name.to_owned();
    let mut entries = vec![Entry::new(
        tr(msg!("Rename")),
        true,
        Box::new(move |_, _| {
            if let Some(editable) = weak_editable.upgrade() {
                editable.grab_focus();
                editable.start_editing();
            }
        }),
    )];
    let toggled = group.clone();
    entries.push(Entry::check(
        tr(msg!("Show the Others Only on Hover")),
        drawer.is_some(),
        Box::new(move |ctx, _| {
            let wanted = match drawer {
                Some(_) => None,
                None => Some(Drawer::default()),
            };
            set_drawer(ctx, &toggled, wanted);
        }),
    ));
    if let Some(drawer) = drawer {
        for (to_left, label) in [
            (true, msg!("Open Toward the Left")),
            (false, msg!("Open Toward the Right")),
        ] {
            let group = group.clone();
            entries.push(Entry::check(
                tr(label),
                drawer.to_left == to_left,
                Box::new(move |ctx, _| {
                    set_drawer(ctx, &group, Some(Drawer { to_left, ..drawer }));
                }),
            ));
        }
        let group = group.clone();
        entries.push(Entry::check(
            tr(msg!("Open on Click Instead")),
            drawer.on_click,
            Box::new(move |ctx, _| {
                set_drawer(
                    ctx,
                    &group,
                    Some(Drawer {
                        on_click: !drawer.on_click,
                        ..drawer
                    }),
                );
            }),
        ));
    }
    entries.push(None);
    entries.push(Entry::new(
        tr(msg!("Settings…")),
        true,
        Box::new(move |ctx, anchor| dialog::open(ctx, anchor, &group)),
    ));
    entries.push(Entry::new(
        tr(msg!("Move Up")),
        index > 0,
        Box::new(move |ctx, _| step(ctx, place, -1)),
    ));
    entries.push(Entry::new(
        tr(msg!("Move Down")),
        index + 1 < count,
        Box::new(move |ctx, _| step(ctx, place, 1)),
    ));
    lane_moves(place, &mut entries);
    entries.push(None);
    entries.push(Entry::new(
        tr(msg!("Ungroup")),
        true,
        Box::new(move |ctx, _| ungroup(ctx, place)),
    ));
    let toast = toast.clone();
    entries.push(Entry::new(
        &ntrf(
            msg_plural!(
                "Remove the Group and Its Item",
                "Remove the Group and Its {count} Items"
            ),
            members,
            &[("count", &members)],
        ),
        true,
        Box::new(move |ctx, _| remove(ctx, place, &toast)),
    ));
    entries
}

/// The keys of a row: Alt and the arrows move it, Delete removes it.
fn add_keys(ctx: &Rc<Ctx>, row: &gtk::ListBoxRow, place: Place, toast: &Toast) {
    let keys = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(ctx);
    let toast = toast.clone();
    keys.connect_key_pressed(move |controller, pressed, _, modifiers| {
        let Some(ctx) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        // A group's name being edited keeps its keys.
        let editing = controller
            .widget()
            .and_then(|row| row.root())
            .and_then(|root| root.focus())
            .is_some_and(|focus| focus.ancestor(gtk::EditableLabel::static_type()).is_some());
        if editing {
            return glib::Propagation::Proceed;
        }
        let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);
        let lane = match place {
            Place::Lane(lane, _) | Place::Member(lane, _, _) => lane,
        };
        match pressed {
            gdk::Key::Up | gdk::Key::KP_Up if alt => {
                step(&ctx, place, -1);
                glib::Propagation::Stop
            }
            gdk::Key::Down | gdk::Key::KP_Down if alt => {
                step(&ctx, place, 1);
                glib::Propagation::Stop
            }
            gdk::Key::Left | gdk::Key::KP_Left if alt => {
                if let Some(to) = lane.checked_sub(1) {
                    move_to(&ctx, place, Place::Lane(to, usize::MAX));
                }
                glib::Propagation::Stop
            }
            gdk::Key::Right | gdk::Key::KP_Right if alt => {
                if lane + 1 < LANES.len() {
                    move_to(&ctx, place, Place::Lane(lane + 1, usize::MAX));
                }
                glib::Propagation::Stop
            }
            gdk::Key::Delete | gdk::Key::KP_Delete | gdk::Key::BackSpace => {
                remove(&ctx, place, &toast);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    row.add_controller(keys);
}

/// Where one step up (`-1`) or down (`1`) takes the item at `place`, as the rows show it: into
/// a group it passes (a group itself passes groups), out of a group at its ends.
pub(crate) fn step_target(
    lists: &[Vec<String>],
    modules: &Modules,
    place: Place,
    direction: isize,
) -> Option<Place> {
    let name = groups::at(lists, modules, place)?;
    let moving_group = groups::is_group(&name);
    match place {
        Place::Lane(lane, index) => {
            let list = lists.get(lane)?;
            if direction < 0 {
                let previous = list.get(index.checked_sub(1)?)?;
                if groups::is_group(previous) && !moving_group {
                    let count = groups::members_of(modules, previous).len();
                    Some(Place::Member(lane, index - 1, count))
                } else {
                    Some(Place::Lane(lane, index - 1))
                }
            } else {
                let next = list.get(index + 1)?;
                if groups::is_group(next) && !moving_group {
                    Some(Place::Member(lane, index + 1, 0))
                } else {
                    Some(Place::Lane(lane, index + 2))
                }
            }
        }
        Place::Member(lane, group, member) => {
            let group_name = lists.get(lane)?.get(group)?;
            let count = groups::members_of(modules, group_name).len();
            if direction < 0 {
                Some(match member.checked_sub(1) {
                    Some(up) => Place::Member(lane, group, up),
                    None => Place::Lane(lane, group),
                })
            } else if member + 1 < count {
                Some(Place::Member(lane, group, member + 2))
            } else {
                Some(Place::Lane(lane, group + 1))
            }
        }
    }
}

/// Moves the item at `place` one step up (`-1`) or down (`1`).
fn step(ctx: &Rc<Ctx>, place: Place, direction: isize) {
    let lists = lists(ctx);
    let modules = ctx.modules();
    if let Some(to) = step_target(&lists, &modules, place, direction) {
        move_to(ctx, place, to);
    }
}

/// Moves the item at `from` to the place before `to` (counted before the move), as one step of
/// Ctrl+Z, and says where it went.
pub(crate) fn move_to(ctx: &Rc<Ctx>, from: Place, to: Place) {
    let mut lists = lists(ctx);
    let before = ctx.modules();
    let mut modules = before.clone();
    let Some(name) = groups::at(&lists, &modules, from) else {
        return;
    };
    // A group goes into no group.
    if groups::is_group(&name) && matches!(to, Place::Member(..)) {
        return;
    }
    let Some(landed) = groups::move_item(&mut lists, &mut modules, from, to) else {
        return;
    };
    FOCUS.with(|focus| focus.replace(Some(landed)));
    let title = item_title(&modules, &name);
    let said = match landed {
        Place::Lane(lane, index) => trf(
            MOVED,
            &[
                ("item", &title),
                ("lane", &tr(LANES[lane].1)),
                ("position", &(index + 1)),
                ("count", &lists[lane].len()),
            ],
        ),
        Place::Member(lane, group, member) => {
            let group = lists[lane].get(group).cloned().unwrap_or_default();
            trf(
                MOVED_INTO,
                &[
                    ("item", &title),
                    ("group", &group_title(&group)),
                    ("position", &(member + 1)),
                    ("count", &groups::members_of(&modules, &group).len()),
                ],
            )
        }
    };
    commit(ctx, before, &lists, modules);
    ctx.announce(&said);
}

/// A name for a group of `members`: the category they share (Network & Sound), else the first
/// one's name.
fn default_title(modules: &Modules, members: &[String]) -> String {
    let categories: Vec<catalog::Category> = members
        .iter()
        .map(|member| catalog::category(member))
        .collect();
    match categories.first() {
        Some(first)
            if categories.iter().all(|category| category == first)
                && *first != catalog::Category::Custom =>
        {
            tr(first.title()).to_owned()
        }
        _ => members
            .first()
            .map(|member| item_title(modules, member))
            .unwrap_or_default(),
    }
}

/// Makes the item at `from` and the item at `onto` (both in lanes) one group, at the place of
/// `onto`, with a name from them that is edited at once.
pub(crate) fn combine(ctx: &Rc<Ctx>, from: Place, onto: Place) {
    let (Place::Lane(lane, target_index), Place::Lane(..)) = (onto, from) else {
        return;
    };
    let mut lists = lists(ctx);
    let before = ctx.modules();
    let mut modules = before.clone();
    let (Some(target), Some(moved)) = (
        groups::at(&lists, &modules, onto),
        groups::at(&lists, &modules, from),
    ) else {
        return;
    };
    if target == moved || groups::is_group(&target) || groups::is_group(&moved) {
        return;
    }
    // The dragged item next to the one it was dropped on (unless it is there), then both as
    // one group.
    if from != Place::Lane(lane, target_index + 1)
        && groups::move_item(
            &mut lists,
            &mut modules,
            from,
            Place::Lane(lane, target_index + 1),
        )
        .is_none()
    {
        return;
    }
    let Some(at) = lists[lane].iter().position(|item| *item == target) else {
        return;
    };
    let members = vec![target.clone(), moved.clone()];
    let title = default_title(&modules, &members);
    let name = groups::free_name(&lists, &modules, &title);
    if !groups::group(&mut lists, &mut modules, lane, &[at, at + 1], &name, None) {
        return;
    }
    FOCUS.with(|focus| focus.replace(Some(Place::Lane(lane, at))));
    RENAME.with(|rename| rename.replace(Some(name.clone())));
    let said = trf(
        GROUPED,
        &[
            ("first", &item_title(&modules, &target)),
            ("second", &item_title(&modules, &moved)),
            ("group", &group_title(&name)),
        ],
    );
    commit(ctx, before, &lists, modules);
    ctx.announce(&said);
}

/// Offers the items and groups of the lane of `place` to group the item there with.
fn group_with(ctx: &Rc<Ctx>, anchor: &gtk::Widget, place: Place) {
    let Place::Lane(lane, index) = place else {
        return;
    };
    let lists = lists(ctx);
    let modules = ctx.modules();
    let moving_group = lists[lane]
        .get(index)
        .is_some_and(|name| groups::is_group(name));
    if moving_group {
        return;
    }
    let mut entries = Vec::new();
    for (other, name) in lists[lane].iter().enumerate() {
        if other == index {
            continue;
        }
        if groups::is_group(name) {
            let count = groups::members_of(&modules, name).len();
            entries.push(Entry::new(
                &trf(msg!("Add to {group}"), &[("group", &group_title(name))]),
                true,
                Box::new(move |ctx, _| move_to(ctx, place, Place::Member(lane, other, count))),
            ));
        } else {
            entries.push(Entry::new(
                &trf(
                    msg!("Group With {item}"),
                    &[("item", &item_title(&modules, name))],
                ),
                true,
                Box::new(move |ctx, _| combine(ctx, place, Place::Lane(lane, other))),
            ));
        }
    }
    if !entries.is_empty() {
        popup(ctx, anchor, entries);
    }
}

/// Gives group `name` a new name made from `title`.
fn rename(ctx: &Rc<Ctx>, name: &str, title: &str) {
    let mut lists = lists(ctx);
    let before = ctx.modules();
    let mut modules = before.clone();
    let Some(definition) = modules.definition(name) else {
        return;
    };
    let others: Vec<Vec<String>> = lists
        .iter()
        .map(|list| list.iter().filter(|item| *item != name).cloned().collect())
        .collect();
    let mut without = modules.clone();
    without.reset(name);
    let new = groups::free_name(&others, &without, title);
    if new == name {
        return;
    }
    for list in &mut lists {
        for item in list.iter_mut() {
            if item == name {
                new.clone_into(item);
            }
        }
    }
    modules.reset(name);
    modules.set(&new, definition);
    let place = lists.iter().enumerate().find_map(|(lane, list)| {
        list.iter()
            .position(|item| *item == new)
            .map(|index| Place::Lane(lane, index))
    });
    if let Some(place) = place {
        FOCUS.with(|focus| focus.replace(Some(place)));
    }
    commit(ctx, before, &lists, modules);
    ctx.announce(&trf(
        msg!("Group renamed {group}"),
        &[("group", &group_title(&new))],
    ));
}

/// Makes group `name` a drawer like `drawer`, or a plain pill.
fn set_drawer(ctx: &Rc<Ctx>, name: &str, drawer: Option<Drawer>) {
    let lists = lists(ctx);
    let before = ctx.modules();
    let mut modules = before.clone();
    let Some(mut definition) = modules.definition(name) else {
        return;
    };
    groups::set_drawer(&mut definition, drawer);
    modules.set(name, definition);
    commit(ctx, before, &lists, modules);
}

/// Puts the members of the group at `place` back into its lane.
fn ungroup(ctx: &Rc<Ctx>, place: Place) {
    let Place::Lane(lane, index) = place else {
        return;
    };
    let mut lists = lists(ctx);
    let before = ctx.modules();
    let mut modules = before.clone();
    let name = lists[lane].get(index).cloned().unwrap_or_default();
    if !groups::ungroup(&mut lists, &mut modules, lane, index) {
        return;
    }
    FOCUS.with(|focus| focus.replace(Some(place)));
    commit(ctx, before, &lists, modules);
    ctx.announce(&trf(
        msg!("Group {group} ungrouped; its items are in the lane again"),
        &[("group", &group_title(&name))],
    ));
}

/// Removes the item at `place` (a group with its members), as one step of Ctrl+Z, with a note
/// to undo it.
pub(crate) fn remove(ctx: &Rc<Ctx>, place: Place, toast: &Toast) {
    let mut lists = lists(ctx);
    let before = ctx.modules();
    let mut modules = before.clone();
    let Some(name) = groups::at(&lists, &modules, place) else {
        return;
    };
    let title = if groups::is_group(&name) {
        group_title(&name)
    } else {
        item_title(&modules, &name)
    };
    match place {
        Place::Lane(lane, index) => {
            lists[lane].remove(index);
            if groups::is_group(&name) {
                modules.reset(&name);
            }
            if let Some(last) = lists[lane].len().checked_sub(1) {
                FOCUS.with(|focus| focus.replace(Some(Place::Lane(lane, index.min(last)))));
            }
        }
        Place::Member(lane, group, member) => {
            let group_name = lists[lane][group].clone();
            let mut members = groups::members_of(&modules, &group_name);
            members.remove(member);
            if let Some(last) = members.len().checked_sub(1) {
                groups::set_members(&mut modules, &group_name, &members);
                FOCUS.with(|focus| {
                    focus.replace(Some(Place::Member(lane, group, member.min(last))));
                });
            } else {
                lists[lane].remove(group);
                modules.reset(&group_name);
            }
        }
    }
    commit(ctx, before, &lists, modules);
    toast.show(&trf(REMOVED, &[("item", &title)]));
}

/// Where a drop at a point lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drop {
    /// Before this place.
    At(Place),
    /// Onto the item at this place: both become a group.
    Combine(Place),
}

/// Where a drop at `x`, `y` of `lane` lands for the item at `dragged`, with the row to mark and
/// its class: before or after the row under the pointer, into a group or onto the middle of an
/// item; else at the end of the lane.
fn landing(
    lane: &Lane,
    x: f64,
    y: f64,
    dragged: Option<Place>,
) -> (Drop, Option<(gtk::Widget, &'static str)>) {
    let rows = lane.rows.borrow();
    let end = (
        Drop::At(Place::Lane(lane.index, usize::MAX)),
        None::<(gtk::Widget, &'static str)>,
    );
    // The innermost row under the pointer: a member before its group's card.
    #[expect(clippy::cast_possible_truncation, reason = "pixels of a widget")]
    let point = gtk::graphene::Point::new(x as f32, y as f32);
    let under = |wanted: &dyn Fn(Kind) -> bool| {
        rows.iter()
            .filter(|(row, _, kind)| wanted(*kind) && row.is_visible())
            .find(|(row, _, _)| {
                row.compute_bounds(&lane.root)
                    .is_some_and(|bounds| bounds.contains_point(&point))
            })
            .cloned()
    };
    let found = under(&|kind| kind != Kind::Group).or_else(|| under(&|kind| kind == Kind::Group));
    let Some((row, at, kind)) = found else {
        return end;
    };
    let Some(point) = lane.root.compute_point(&row, &point) else {
        return end;
    };
    let fraction = f64::from(point.y()) / f64::from(row.height().max(1));
    let dragging_group = dragged.is_some_and(|dragged| {
        rows.iter()
            .any(|(_, place, kind)| *place == dragged && *kind == Kind::Group)
    });
    let widget: gtk::Widget = row.upcast();
    let before = |place| (Drop::At(place), Some((widget.clone(), "st-drop-before")));
    let after = |place| (Drop::At(place), Some((widget.clone(), "st-drop-after")));
    match (kind, at) {
        (Kind::Member, Place::Member(lane_index, group, member)) => {
            if dragging_group {
                (Drop::At(Place::Lane(lane_index, group + 1)), None)
            } else if fraction < 0.5 {
                before(at)
            } else {
                after(Place::Member(lane_index, group, member + 1))
            }
        }
        (Kind::Group, Place::Lane(lane_index, index)) => {
            if fraction < 0.2 || (dragging_group && fraction < 0.5) {
                before(at)
            } else if dragging_group {
                after(Place::Lane(lane_index, index + 1))
            } else {
                // Into the group, at its end.
                let members = rows
                    .iter()
                    .filter(|(_, place, kind)| {
                        *kind == Kind::Member
                            && matches!(place, Place::Member(l, g, _) if *l == lane_index && *g == index)
                    })
                    .count();
                (
                    Drop::At(Place::Member(lane_index, index, members)),
                    Some((widget.clone(), "st-drop-into")),
                )
            }
        }
        (_, Place::Lane(lane_index, index)) => {
            let plain = dragged == Some(at) || dragging_group;
            if fraction < 0.3 || (plain && fraction < 0.5) {
                before(at)
            } else if fraction > 0.7 || plain {
                after(Place::Lane(lane_index, index + 1))
            } else {
                (Drop::Combine(at), Some((widget.clone(), "st-drop-combine")))
            }
        }
        _ => end,
    }
}

/// Rows dropped onto lane `lane` land where [`landing`] says; a line, the group's frame or the
/// hint "Drop to group" shows where while they are dragged.
fn drop_onto(ctx: &Rc<Ctx>, lane: &Rc<Lane>) {
    let target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
    target.set_preload(true);
    let marked: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(Vec::new()));
    let unmark = {
        let marked = marked.clone();
        move || {
            for widget in marked.take() {
                for class in [
                    "st-drop-before",
                    "st-drop-after",
                    "st-drop-combine",
                    "st-drop-into",
                ] {
                    widget.remove_css_class(class);
                }
                if let Some(hint) = widget.downcast_ref::<gtk::ListBoxRow>().and_then(hint_of) {
                    hint.set_visible(false);
                }
            }
        }
    };
    let dragged = |target: &gtk::DropTarget| -> Option<Place> {
        target
            .value()
            .and_then(|value| value.get::<String>().ok())
            .and_then(|payload| place_of(&payload))
    };
    let motion_lane = Rc::downgrade(lane);
    let motion_unmark = unmark.clone();
    target.connect_motion(move |target, x, y| {
        motion_unmark();
        let Some(lane) = motion_lane.upgrade() else {
            return gdk::DragAction::empty();
        };
        if let (_, Some((widget, class))) = landing(&lane, x, y, dragged(target)) {
            widget.add_css_class(class);
            if class == "st-drop-combine" {
                if let Some(hint) = widget.downcast_ref::<gtk::ListBoxRow>().and_then(hint_of) {
                    hint.set_visible(true);
                }
            }
            marked.borrow_mut().push(widget);
        }
        gdk::DragAction::MOVE
    });
    let leave_unmark = unmark.clone();
    target.connect_leave(move |_| leave_unmark());
    let weak = Rc::downgrade(ctx);
    let drop_lane = Rc::downgrade(lane);
    target.connect_drop(move |_, value, x, y| {
        unmark();
        let (Some(ctx), Some(lane)) = (weak.upgrade(), drop_lane.upgrade()) else {
            return false;
        };
        let Some(from) = value
            .get::<String>()
            .ok()
            .and_then(|payload| place_of(&payload))
        else {
            return false;
        };
        let (drop, _) = landing(&lane, x, y, Some(from));
        match drop {
            Drop::Combine(onto) => combine(&ctx, from, onto),
            Drop::At(to) => move_to(&ctx, from, to),
        }
        true
    });
    lane.root.add_controller(target);
}

/// Adds item `name` at the end of lane `key`, as one step of Ctrl+Z; the focus goes to it.
pub(crate) fn add_item(ctx: &Rc<Ctx>, key: &'static str, name: &str) {
    let mut list = ctx.list(key);
    ctx.remember(ctx.snapshot(&[key]));
    list.push(name.to_owned());
    FOCUS.with(|focus| focus.replace(Some(Place::Lane(lane_index(key), list.len() - 1))));
    ctx.model().set(key, None, Value::List(list));
}

/// A name for another item of `kind` that no lane or group has yet: `kind`, else `kind#2`,
/// `kind#3`, …
pub(crate) fn free_name(ctx: &Ctx, kind: &str) -> String {
    let taken = ctx.modules().shown(&lists(ctx));
    if !taken.iter().any(|name| name == kind) {
        return kind.to_owned();
    }
    (2..=taken.len() + 2)
        .map(|n| format!("{kind}#{n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_else(|| kind.to_owned())
}

/// Whether a lane or a group has item `name`.
pub(crate) fn taken(ctx: &Ctx, name: &str) -> bool {
    ctx.modules()
        .shown(&lists(ctx))
        .iter()
        .any(|item| item == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_step_goes_into_and_out_of_groups() {
        let mut modules = Modules::default();
        modules.set(
            "group/g",
            json!({"modules": ["b", "c"]}).as_object().unwrap().clone(),
        );
        let lists = vec![vec!["a".to_owned(), "group/g".to_owned(), "d".to_owned()]];
        // a down: into the group, first
        assert_eq!(
            step_target(&lists, &modules, Place::Lane(0, 0), 1),
            Some(Place::Member(0, 1, 0))
        );
        // d up: into the group, last
        assert_eq!(
            step_target(&lists, &modules, Place::Lane(0, 2), -1),
            Some(Place::Member(0, 1, 2))
        );
        // b up: out, before the group; c down: out, after it
        assert_eq!(
            step_target(&lists, &modules, Place::Member(0, 1, 0), -1),
            Some(Place::Lane(0, 1))
        );
        assert_eq!(
            step_target(&lists, &modules, Place::Member(0, 1, 1), 1),
            Some(Place::Lane(0, 2))
        );
        // the group passes items, never goes into a group
        assert_eq!(
            step_target(&lists, &modules, Place::Lane(0, 1), -1),
            Some(Place::Lane(0, 0))
        );
        assert_eq!(step_target(&lists, &modules, Place::Lane(0, 0), -1), None);
        assert_eq!(
            place_of(&payload(Place::Member(2, 3, 4))),
            Some(Place::Member(2, 3, 4))
        );
        assert_eq!(place_of("9\n1"), None);
    }
}
