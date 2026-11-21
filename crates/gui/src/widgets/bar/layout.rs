// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The modules of one area of the bar (left, center or right) as chips, below its row.
//!
//! A chip is dragged within its area or onto another; with the keyboard, Alt+←/→ move the
//! focused chip within its area and Alt+↑/↓ to the area above or below, Delete removes it and
//! Enter opens its settings. Every change goes through the row of its list
//! ([`SettingRow::change`]), so refusals show there. _Add module…_ offers the catalog, a custom
//! module and the templates.

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use serde_json::{Map, Value as Json};
use sway_things_core::bar::catalog::{self, Works};
use sway_things_core::bar::templates::{self, Group, TEMPLATES};
use sway_things_core::bar::{Modules, kind_of, modules::members};
use sway_things_core::registry::{Value, bar as keys};

use super::{dialog, follow, list_of};
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;
use crate::widgets::{PageView, SettingRow};

// Messages used inside `glib::clone!` blocks, which xgettext can't look into.
/// How to move a chip, for its tooltip and accessible description.
const HOW_TO_MOVE: Msg = msg!(
    "Drag to move. Alt+Left and Alt+Right move it in its area, Alt+Up and Alt+Down to another; \
     Delete removes it, Enter opens its settings"
);
/// A copy of a template's script the user changed stays.
const SCRIPT_KEPT: Msg = msg!("Your copy of the script stays");
/// Where it is, and that the module runs it.
const SCRIPT_KEPT_DETAIL: Msg = msg!(
    "{file} isn't the script as Sway Things ships it, so it stays as it is and the new module \
     runs it. Move it away to get the template's script."
);
/// Why a template couldn't be added.
const TEMPLATE_FAILED: Msg = msg!("The template couldn't be added");

thread_local! {
    /// The chip that gets the focus when its area is shown next, after a move: the list and the
    /// place in it.
    static FOCUS: std::cell::RefCell<Option<(String, usize)>> = const { std::cell::RefCell::new(None) };
}

/// The lists of the three areas, top to bottom on the page.
const AREAS: [&str; 3] = [
    keys::MODULES_LEFT,
    keys::MODULES_CENTER,
    keys::MODULES_RIGHT,
];

/// The chips of one area, and _Add module…_.
#[derive(Debug, Clone)]
pub struct ModuleArea {
    root: gtk::Box,
}

impl ModuleArea {
    /// The area of the list `row` is for, following `model`.
    pub fn new(model: &Model, row: &SettingRow) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("st-bar-area");
        let chips = gtk::FlowBox::new();
        chips.set_selection_mode(gtk::SelectionMode::None);
        chips.set_homogeneous(false);
        chips.set_row_spacing(6);
        chips.set_column_spacing(6);
        chips.set_max_children_per_line(30);
        chips.set_activate_on_single_click(true);
        chips.add_css_class("st-bar-chips");
        let title = row
            .setting()
            .map(|setting| tr(setting.title()).to_owned())
            .unwrap_or_default();
        chips.update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("Modules: {area}"),
            &[("area", &title)],
        ))]);
        root.append(&chips);
        let add = gtk::MenuButton::new();
        add.set_label(tr(msg!("Add Module…")));
        add.set_halign(gtk::Align::Start);
        add.set_tooltip_text(Some(&trf(
            msg!("Add a module to the {area} area"),
            &[("area", &title)],
        )));
        root.append(&add);
        let area = Self { root };
        drop_onto(&chips, row);
        chips.connect_child_activated(glib::clone!(
            #[weak]
            model,
            #[weak]
            row,
            move |_, child| {
                let names = list_of(&row);
                if let Some(name) = usize::try_from(child.index())
                    .ok()
                    .and_then(|index| names.get(index))
                {
                    dialog::open(&child.clone().upcast(), &model, name, None);
                }
            }
        ));
        let popover = add_popover(model, row);
        add.set_popover(Some(&popover));
        let refresh = glib::clone!(
            #[weak]
            chips,
            #[weak]
            row,
            #[weak]
            model,
            #[weak]
            add,
            move || {
                let names = list_of(&row);
                let shown: Vec<String> = chip_names(&chips);
                let locked = row.locked().is_some();
                add.set_sensitive(!locked);
                chips.set_sensitive(!locked);
                if shown == names {
                    return;
                }
                let focused = chips
                    .focus_child()
                    .and_downcast::<gtk::FlowBoxChild>()
                    .map(|child| child.index());
                while let Some(child) = chips.first_child() {
                    chips.remove(&child);
                }
                let modules = model.bar_modules().and_then(Result::ok).unwrap_or_default();
                for (index, name) in names.iter().enumerate() {
                    chips.append(&chip(&model, &row, &modules, name, index, names.len()));
                }
                let key = row.setting().map(sway_things_core::registry::Setting::key);
                let moved = FOCUS.with(|focus| {
                    let mut focus = focus.borrow_mut();
                    match focus.as_ref() {
                        Some((moved, index)) if Some(moved.as_str()) == key => {
                            let index = *index;
                            *focus = None;
                            i32::try_from(index).ok()
                        }
                        _ => None,
                    }
                });
                if let Some(child) = moved
                    .or(focused)
                    .and_then(|index| chips.child_at_index(index))
                {
                    child.grab_focus();
                }
            }
        );
        follow(model, &area.root, refresh);
        area
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }
}

/// Chips dropped onto `chips`: put before the chip under the pointer, or at the end.
fn drop_onto(chips: &gtk::FlowBox, row: &SettingRow) {
    {
        let target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
        target.connect_drop(glib::clone!(
            #[weak]
            chips,
            #[weak]
            row,
            #[upgrade_or]
            false,
            move |_, value, x, y| {
                let Ok(payload) = value.get::<String>() else {
                    return false;
                };
                let Some((from_key, index)) = payload.split_once('\n') else {
                    return false;
                };
                let Ok(index) = index.parse::<usize>() else {
                    return false;
                };
                #[expect(clippy::cast_possible_truncation, reason = "pixels of a widget")]
                let to = chips
                    .child_at_pos(x as i32, y as i32)
                    .and_then(|child| usize::try_from(child.index()).ok())
                    .unwrap_or(usize::MAX);
                let Some(key) = row.setting().map(sway_things_core::registry::Setting::key) else {
                    return false;
                };
                move_module(&row, from_key, index, key, to);
                true
            }
        ));
        chips.add_controller(target);
    }
}

/// The names the chips of `chips` stand for, in order.
fn chip_names(chips: &gtk::FlowBox) -> Vec<String> {
    let mut names = Vec::new();
    let mut child = chips.first_child();
    while let Some(widget) = child {
        names.push(widget.widget_name().to_string());
        child = widget.next_sibling();
    }
    names
}

/// What a chip says for module `name`: the catalog's title, with the instance (`bat2`), or the
/// name after `custom/` or `group/`.
fn chip_label(modules: &Modules, name: &str) -> String {
    let kind = kind_of(name);
    let instance = name.split_once('#').map(|(_, instance)| instance);
    let base = if let Some(custom) = kind.strip_prefix("custom/") {
        custom.to_owned()
    } else if let Some(group) = kind.strip_prefix("group/") {
        let count = modules
            .definition(name)
            .map_or(0, |definition| members(&definition).len());
        trf(
            msg!("{group} (group of {count})"),
            &[("group", &group), ("count", &count)],
        )
    } else {
        catalog::find(name).map_or_else(|| kind.to_owned(), |module| tr(module.title).to_owned())
    };
    match instance {
        Some(instance) => format!("{base} ({instance})"),
        None => base,
    }
}

/// The chip of module `name` at `index` of `count` in the list of `row`.
fn chip(
    model: &Model,
    row: &SettingRow,
    modules: &Modules,
    name: &str,
    index: usize,
    count: usize,
) -> gtk::FlowBoxChild {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    content.add_css_class("st-bar-chip");
    content.set_halign(gtk::Align::Start);
    let label_text = chip_label(modules, name);
    let label = gtk::Label::new(Some(&label_text));
    content.append(&label);
    let module = catalog::find(name);
    let mut tooltip = name.to_owned();
    if let Some(module) = module {
        tooltip.push_str(" — ");
        tooltip.push_str(tr(module.description));
        if let Works::No(need) = module.need.check() {
            let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
            icon.set_tooltip_text(Some(tr(need.missing())));
            icon.update_property(&[gtk::accessible::Property::Label(tr(need.missing()))]);
            content.append(&icon);
        }
    }
    let remove = gtk::Button::from_icon_name("window-close-symbolic");
    remove.add_css_class("flat");
    remove.add_css_class("circular");
    remove.set_focus_on_click(false);
    remove.set_tooltip_text(Some(tr(msg!("Remove from the bar"))));
    remove.update_property(&[gtk::accessible::Property::Label(&trf(
        msg!("Remove {module}"),
        &[("module", &label_text)],
    ))]);
    remove.connect_clicked(glib::clone!(
        #[weak]
        row,
        move |_| remove_module(&row, index)
    ));
    content.append(&remove);
    let child = gtk::FlowBoxChild::new();
    child.set_widget_name(name);
    child.set_child(Some(&content));
    child.set_tooltip_text(Some(&format!("{tooltip}\n{}", tr(HOW_TO_MOVE))));
    child.update_property(&[
        gtk::accessible::Property::Label(&trf(
            msg!("{module}, {position} of {count}"),
            &[
                ("module", &label_text),
                ("position", &(index + 1)),
                ("count", &count),
            ],
        )),
        gtk::accessible::Property::Description(tr(HOW_TO_MOVE)),
    ]);
    let Some(key) = row.setting().map(sway_things_core::registry::Setting::key) else {
        return child;
    };
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    let payload = format!("{key}\n{index}");
    source.set_content(Some(&gdk::ContentProvider::for_value(&payload.to_value())));
    source.connect_drag_begin(glib::clone!(
        #[weak]
        content,
        move |source, _| {
            let icon = gtk::WidgetPaintable::new(Some(&content));
            source.set_icon(Some(&icon), 0, 0);
        }
    ));
    child.add_controller(source);
    add_keys(model, row, &child, key, index, name);
    child
}

/// The keys of a chip: Alt and the arrows move it, Delete removes it, Enter opens it.
fn add_keys(
    model: &Model,
    row: &SettingRow,
    child: &gtk::FlowBoxChild,
    key: &'static str,
    index: usize,
    name: &str,
) {
    let keys = gtk::EventControllerKey::new();
    let name = name.to_owned();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        row,
        #[weak]
        model,
        #[weak]
        child,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, pressed, _, modifiers| {
            let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);
            match pressed {
                gdk::Key::Left | gdk::Key::KP_Left if alt => {
                    if let Some(to) = index.checked_sub(1) {
                        move_module(&row, key, index, key, to);
                    }
                    glib::Propagation::Stop
                }
                gdk::Key::Right | gdk::Key::KP_Right if alt => {
                    move_module(&row, key, index, key, index + 2);
                    glib::Propagation::Stop
                }
                gdk::Key::Up | gdk::Key::KP_Up if alt => {
                    if let Some(to) = neighbor(key, -1) {
                        move_module(&row, key, index, to, usize::MAX);
                    }
                    glib::Propagation::Stop
                }
                gdk::Key::Down | gdk::Key::KP_Down if alt => {
                    if let Some(to) = neighbor(key, 1) {
                        move_module(&row, key, index, to, usize::MAX);
                    }
                    glib::Propagation::Stop
                }
                gdk::Key::Delete | gdk::Key::KP_Delete | gdk::Key::BackSpace => {
                    remove_module(&row, index);
                    glib::Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space => {
                    dialog::open(&child.clone().upcast(), &model, &name, None);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        }
    ));
    child.add_controller(keys);
}

/// The list above (`-1`) or below (`1`) `key`.
fn neighbor(key: &str, step: isize) -> Option<&'static str> {
    let at = AREAS.iter().position(|area| *area == key)?;
    AREAS.get(at.checked_add_signed(step)?).copied()
}

/// The row of list `key` on the page `row` is on.
fn row_of(row: &SettingRow, key: &str) -> Option<SettingRow> {
    if row.setting().is_some_and(|setting| setting.key() == key) {
        return Some(row.clone());
    }
    row.ancestor(PageView::static_type())
        .and_downcast::<PageView>()?
        .row(key)
}

/// Moves the module at `from` of list `from_key` to the place before `to` of list `to_key` (the
/// end for `usize::MAX`), from the page `row` is on.
fn move_module(row: &SettingRow, from_key: &str, from: usize, to_key: &str, to: usize) {
    let (Some(source), Some(target)) = (row_of(row, from_key), row_of(row, to_key)) else {
        return;
    };
    let mut from_list = list_of(&source);
    if from >= from_list.len() {
        return;
    }
    if from_key == to_key {
        let name = from_list.remove(from);
        // The place before `to`, counted in the list without the module moved.
        let to = if to > from { to - 1 } else { to }.min(from_list.len());
        from_list.insert(to, name);
        FOCUS.with(|focus| focus.replace(Some((to_key.to_owned(), to))));
        source.change(Value::List(from_list));
        return;
    }
    let name = from_list.remove(from);
    let mut to_list = list_of(&target);
    let to = to.min(to_list.len());
    to_list.insert(to, name);
    // The focus follows the chip to its new place once it is there.
    FOCUS.with(|focus| focus.replace(Some((to_key.to_owned(), to))));
    // Added first, then removed: the module is never missing from the bar meanwhile.
    target.change(Value::List(to_list));
    source.change(Value::List(from_list));
}

/// Removes the module at `index` of the list of `row`.
fn remove_module(row: &SettingRow, index: usize) {
    let mut list = list_of(row);
    if index < list.len() {
        list.remove(index);
        row.change(Value::List(list));
    }
}

/// A name for another module of `kind` that no list has yet: `kind`, else `kind#2`, `kind#3`, …
fn free_name(row: &SettingRow, kind: &str) -> String {
    let taken: Vec<String> = AREAS
        .iter()
        .filter_map(|key| row_of(row, key))
        .flat_map(|row| list_of(&row))
        .collect();
    if !taken.iter().any(|name| name == kind) {
        return kind.to_owned();
    }
    (2..=taken.len() + 2)
        .map(|n| format!("{kind}#{n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_else(|| kind.to_owned())
}

/// Adds module `name` with `definition` (stored when given) to the end of the list of `row`.
fn add_module(model: &Model, row: &SettingRow, name: &str, definition: Option<Map<String, Json>>) {
    if let Some(definition) = definition {
        let mut modules = model.bar_modules().and_then(Result::ok).unwrap_or_default();
        modules.set(name, definition);
        model.set_bar_modules(modules);
    }
    let mut list = list_of(row);
    list.push(name.to_owned());
    row.change(Value::List(list));
}

/// The popover of _Add module…_: the catalog, a custom module, and the templates.
fn add_popover(model: &Model, row: &SettingRow) -> gtk::Popover {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(6);
    content.set_margin_end(6);
    let popover = gtk::Popover::new();
    heading(&content, tr(msg!("Modules")));
    for module in catalog::MODULES
        .iter()
        .filter(|module| !matches!(module.name, "custom" | "group"))
    {
        let missing = match module.need.check() {
            Works::Yes => None,
            Works::No(need) => Some(tr(need.missing())),
        };
        let item = entry(tr(module.title), tr(module.description), missing);
        let kind = module.name;
        item.connect_clicked(glib::clone!(
            #[weak]
            model,
            #[weak]
            row,
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                let name = free_name(&row, kind);
                // Another instance starts from the definition of the module it copies.
                let definition = (name != kind)
                    .then(|| model.bar_modules().and_then(Result::ok)?.definition(kind))
                    .flatten();
                add_module(&model, &row, &name, definition);
            }
        ));
        content.append(&item);
    }
    let custom = entry(
        tr(msg!("Custom module…")),
        tr(msg!("The output of a command or script of yours")),
        None,
    );
    custom.connect_clicked(glib::clone!(
        #[weak]
        model,
        #[weak]
        row,
        #[weak]
        popover,
        move |button| {
            popover.popdown();
            let key = row.setting().map_or(
                keys::MODULES_RIGHT,
                sway_things_core::registry::Setting::key,
            );
            dialog::open(&button.clone().upcast(), &model, "custom/", Some(key));
        }
    ));
    content.append(&custom);
    templates_part(model, row, &popover, &content);
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_hscrollbar_policy(gtk::PolicyType::Never);
    scrolled.set_propagate_natural_height(true);
    scrolled.set_max_content_height(480);
    scrolled.set_min_content_width(320);
    scrolled.set_child(Some(&content));
    popover.set_child(Some(&scrolled));
    popover
}

/// The templates in the popover.
fn templates_part(model: &Model, row: &SettingRow, popover: &gtk::Popover, content: &gtk::Box) {
    for (group, title, note) in [(Group::General, msg!("From a template"), None)] {
        heading(content, tr(title));
        if let Some(note) = note {
            let label = gtk::Label::new(Some(tr(note)));
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_max_width_chars(44);
            label.add_css_class(style::DIM);
            content.append(&label);
        }
        for template in TEMPLATES.iter().filter(|template| template.group == group) {
            let missing = template.missing();
            let missing = (!missing.is_empty()).then(|| {
                trf(
                    msg!("Needs {programs}"),
                    &[("programs", &missing.join(", "))],
                )
            });
            let item = entry(
                tr(template.title),
                tr(template.description),
                missing.as_deref(),
            );
            let id = template.id;
            item.connect_clicked(glib::clone!(
                #[weak]
                model,
                #[weak]
                row,
                #[weak]
                popover,
                move |button| {
                    popover.popdown();
                    add_template(button, &model, &row, id);
                }
            ));
            content.append(&item);
        }
    }
}

/// Copies template `id`'s script (if it has one) and adds its module.
fn add_template(button: &gtk::Button, model: &Model, row: &SettingRow, id: &str) {
    let (Some(template), Some(files), Some(status)) =
        (templates::find(id), model.waybar_files(), model.status())
    else {
        return;
    };
    let installed = status
        .write_access()
        .map_err(|read_only| read_only.text().translated())
        .and_then(|access| {
            template
                .install(&files, &access)
                .map_err(|error| error.text().translated())
        });
    let window = button.root().and_downcast::<gtk::Window>();
    match installed {
        Err(error) => {
            gtk::AlertDialog::builder()
                .message(tr(TEMPLATE_FAILED))
                .detail(error)
                .modal(true)
                .build()
                .show(window.as_ref());
            return;
        }
        Ok(templates::Installed::Kept(path)) => {
            gtk::AlertDialog::builder()
                .message(tr(SCRIPT_KEPT))
                .detail(trf(
                    SCRIPT_KEPT_DETAIL,
                    &[("file", &sway_things_core::bar::tilde(&path))],
                ))
                .modal(true)
                .build()
                .show(window.as_ref());
        }
        Ok(_) => {}
    }
    let name = free_name(row, &template.module_name());
    add_module(model, row, &name, Some(template.definition(&files)));
}

/// A heading in the popover.
fn heading(content: &gtk::Box, text: &str) {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_margin_top(8);
    label.add_css_class("heading");
    label.set_accessible_role(gtk::AccessibleRole::Heading);
    content.append(&label);
}

/// A button in the popover: a title, what it does, and what is missing for it, if anything.
fn entry(title: &str, description: &str, missing: Option<&str>) -> gtk::Button {
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    texts.append(&title_label);
    let detail = match missing {
        Some(missing) => format!("{description}. {missing}"),
        None => description.to_owned(),
    };
    let detail_label = gtk::Label::new(Some(&detail));
    detail_label.set_xalign(0.0);
    detail_label.set_wrap(true);
    detail_label.set_max_width_chars(44);
    detail_label.add_css_class(style::DIM);
    texts.append(&detail_label);
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.set_child(Some(&texts));
    button.update_property(&[
        gtk::accessible::Property::Label(title),
        gtk::accessible::Property::Description(&detail),
    ]);
    button
}
