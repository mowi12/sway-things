// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings of one module: a form built from the module's options in the catalog, with a
//! preview under each format, and the options the catalog doesn't know as JSON, kept as they
//! are. For a new custom module the form starts with its name.
//!
//! A field left empty leaves the option out, so waybar's default applies. A value whose type
//! the field can't show (an `interval` of `"once"`) stays in the JSON. Commands are the user's
//! data: they are stored and shown as written, and waybar runs them, never Sway Things.

use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use serde_json::{Map, Number, Value as Json};
use sway_things_core::bar::catalog::{self, Kind, Opt};
use sway_things_core::bar::is_module_name;
use sway_things_core::registry::{Value, bar as keys};

use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;
use crate::widgets::PageView;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into.
/// A field whose value isn't a number.
const NOT_A_NUMBER: Msg = msg!("{option}: must be a number from {min} to {max}");
/// The JSON of the other options doesn't read.
const BAD_JSON: Msg = msg!("Other options: {error}");
/// The JSON isn't an object.
const NOT_AN_OBJECT: Msg = msg!("Other options must be one JSON object, in braces");
/// The name of a new custom module isn't one.
const BAD_NAME: Msg =
    msg!("The name can't be empty or have spaces, and no other module may have it");
/// The preview of a format.
const PREVIEW: Msg = msg!("Shows as: {text}");

/// A field of the form and how its value is read.
enum Field {
    Text(gtk::Entry),
    Number(gtk::Entry, Kind),
    Choice(gtk::DropDown, Vec<Option<Json>>),
}

/// What the form of a module holds.
struct Form {
    fields: Vec<(&'static str, Field)>,
    buffer: gtk::TextBuffer,
    error: gtk::Label,
    name_entry: Option<gtk::Entry>,
}

/// Opens the settings of module `name`, over the window of `anchor`. With `add_to` (a list
/// key) and `name` `custom/`, it makes a new custom module and adds it to that list.
pub fn open(anchor: &gtk::Widget, model: &Model, name: &str, add_to: Option<&'static str>) {
    let modules = match model.bar_modules() {
        Some(Ok(modules)) => modules,
        Some(Err(error)) => {
            let window = anchor.root().and_downcast::<gtk::Window>();
            gtk::AlertDialog::builder()
                .message(error.text().translated())
                .modal(true)
                .build()
                .show(window.as_ref());
            return;
        }
        None => return,
    };
    let creating = add_to.is_some();
    let definition = if creating {
        Map::new()
    } else {
        modules.definition(name).unwrap_or_default()
    };
    let title = if creating {
        tr(msg!("New Custom Module")).to_owned()
    } else {
        trf(msg!("Module {name}"), &[("name", &name)])
    };
    let (window, content) = window_for(anchor, &title);
    let form = Rc::new(form(&content, &definition, name, creating));
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(8);
    if !creating && modules.is_stored(name) {
        let reset = gtk::Button::with_label(tr(msg!("Reset to Default")));
        reset.set_tooltip_text(Some(tr(msg!(
            "Back to the module's definition in waybar's default config, or none"
        ))));
        let name = name.to_owned();
        reset.connect_clicked(glib::clone!(
            #[weak]
            model,
            #[weak]
            window,
            move |_| {
                if let Some(Ok(mut modules)) = model.bar_modules() {
                    modules.reset(&name);
                    model.set_bar_modules(modules);
                }
                window.close();
            }
        ));
        buttons.append(&reset);
    }
    let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
    cancel.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));
    buttons.append(&cancel);
    let save = gtk::Button::with_label(tr(if creating { msg!("Add") } else { msg!("Save") }));
    save.add_css_class("suggested-action");
    buttons.append(&save);
    window.set_default_widget(Some(&save));
    content.append(&buttons);
    let name = name.to_owned();
    let anchor = anchor.clone();
    save.connect_clicked(glib::clone!(
        #[weak]
        model,
        #[weak]
        window,
        move |_| {
            if store(&form, &model, &anchor, &name, add_to) {
                window.close();
            }
        }
    ));
    window.present();
}

/// A dialog window titled `title` over the window of `anchor`, closed with Escape, and the box
/// its content goes into.
fn window_for(anchor: &gtk::Widget, title: &str) -> (gtk::Window, gtk::Box) {
    let window = gtk::Window::builder()
        .modal(true)
        .destroy_with_parent(true)
        .default_width(520)
        .default_height(560)
        .title(title)
        .build();
    window.set_transient_for(anchor.root().and_downcast::<gtk::Window>().as_ref());
    window.add_css_class("st-dialog");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_top(20);
    content.set_margin_bottom(16);
    content.set_margin_start(20);
    content.set_margin_end(20);
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("title-2");
    heading.set_xalign(0.0);
    heading.set_accessible_role(gtk::AccessibleRole::Heading);
    content.append(&heading);
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_hscrollbar_policy(gtk::PolicyType::Never);
    scrolled.set_child(Some(&content));
    window.set_child(Some(&scrolled));
    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(|widget, _| {
            if let Some(window) = widget.downcast_ref::<gtk::Window>() {
                window.close();
            }
            glib::Propagation::Stop
        })),
    ));
    window.add_controller(escape);
    (window, content)
}

/// The form for module `name` with `definition`: its name for a new custom module, a field per
/// option of the catalog, and the other options as JSON.
fn form(content: &gtk::Box, definition: &Map<String, Json>, name: &str, creating: bool) -> Form {
    let module = catalog::find(name);
    if let Some(module) = module {
        let about = gtk::Label::new(Some(tr(module.description)));
        about.set_xalign(0.0);
        about.set_wrap(true);
        about.add_css_class(style::DIM);
        content.append(&about);
    }
    let name_entry = creating.then(|| {
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("updates"));
        labeled(content, tr(msg!("Name (custom/…)")), &entry);
        entry
    });
    let options: &[Opt] = module.map_or(&[], |module| module.options);
    let mut others = definition.clone();
    let mut fields: Vec<(&'static str, Field)> = Vec::new();
    for option in options {
        let current = others.get(option.key).cloned();
        let (field, taken) = field_for(option, current.as_ref());
        if taken {
            others.remove(option.key);
        }
        labeled(content, tr(option.title), field_widget(&field));
        if option.kind == Kind::Format {
            if let Field::Text(entry) = &field {
                content.append(&preview(entry, name));
            }
        }
        fields.push((option.key, field));
    }
    let other = gtk::Label::new(Some(tr(msg!("Other options (JSON)"))));
    other.set_xalign(0.0);
    other.add_css_class("heading");
    content.append(&other);
    let buffer = gtk::TextBuffer::new(None);
    buffer.set_text(&if others.is_empty() {
        "{}".to_owned()
    } else {
        serde_json::to_string_pretty(&Json::Object(others)).unwrap_or_default()
    });
    let text = gtk::TextView::with_buffer(&buffer);
    text.add_css_class("monospace");
    text.set_monospace(true);
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    text.set_size_request(-1, 120);
    text.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Other options (JSON)"
    )))]);
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&text));
    content.append(&frame);
    let error = gtk::Label::new(None);
    error.set_xalign(0.0);
    error.set_wrap(true);
    error.add_css_class("error");
    error.set_visible(false);
    content.append(&error);
    Form {
        fields,
        buffer,
        error,
        name_entry,
    }
}

/// Stores what `form` holds as the definition of module `name` (or of the new custom module it
/// names, added to the list `add_to`). Shows what is wrong and says `false` if it can't.
fn store(
    form: &Form,
    model: &Model,
    anchor: &gtk::Widget,
    name: &str,
    add_to: Option<&str>,
) -> bool {
    let fail = |message: &str| {
        form.error.set_text(message);
        form.error.set_visible(true);
        false
    };
    let definition = match collect(&form.fields, &form.buffer) {
        Ok(definition) => definition,
        Err(message) => return fail(&message),
    };
    let target = match &form.name_entry {
        Some(entry) => {
            let wanted = format!("custom/{}", entry.text().trim());
            if !is_module_name(&wanted) || wanted == "custom/" || taken(anchor, &wanted) {
                return fail(tr(BAD_NAME));
            }
            wanted
        }
        None => name.to_owned(),
    };
    let Some(Ok(mut modules)) = model.bar_modules() else {
        return false;
    };
    modules.set(&target, definition);
    model.set_bar_modules(modules);
    if let Some(key) = add_to {
        add_to_list(anchor, key, &target);
    }
    true
}

/// The field for `option` with its `current` value, and whether the field holds it (a value of
/// another type stays in the JSON).
fn field_for(option: &Opt, current: Option<&Json>) -> (Field, bool) {
    match option.kind {
        Kind::Format | Kind::Text | Kind::Command => {
            let entry = gtk::Entry::new();
            entry.set_placeholder_text(Some(tr(msg!("waybar's default"))));
            if option.kind == Kind::Command {
                entry.add_css_class("monospace");
            }
            let taken = match current {
                Some(Json::String(text)) => {
                    entry.set_text(text);
                    true
                }
                None => true,
                Some(_) => false,
            };
            (Field::Text(entry), taken)
        }
        Kind::Integer { .. } | Kind::Decimal { .. } => {
            let entry = gtk::Entry::new();
            entry.set_placeholder_text(Some(tr(msg!("waybar's default"))));
            entry.set_input_purpose(gtk::InputPurpose::Number);
            let taken = match current {
                Some(Json::Number(number)) => {
                    entry.set_text(&number.to_string());
                    true
                }
                None => true,
                Some(_) => false,
            };
            (Field::Number(entry, option.kind), taken)
        }
        Kind::Bool => {
            let values = vec![None, Some(Json::Bool(true)), Some(Json::Bool(false))];
            let labels = [
                tr(msg!("waybar's default")),
                tr(msg!("On")),
                tr(msg!("Off")),
            ];
            choice(&labels, values, current)
        }
        Kind::Choice(words) => {
            let mut labels = vec![tr(msg!("waybar's default"))];
            let mut values = vec![None];
            for word in words {
                labels.push(if word.is_empty() {
                    tr(msg!("Plain text"))
                } else {
                    word
                });
                values.push(Some(Json::String((*word).to_owned())));
            }
            choice(&labels, values, current)
        }
    }
}

/// A drop-down of `labels` for `values`, with `current` picked if it is one of them.
fn choice(labels: &[&str], values: Vec<Option<Json>>, current: Option<&Json>) -> (Field, bool) {
    let dropdown = gtk::DropDown::from_strings(labels);
    let at = values
        .iter()
        .position(|value| value.as_ref() == current)
        .and_then(|at| u32::try_from(at).ok());
    if let Some(at) = at {
        dropdown.set_selected(at);
    }
    (Field::Choice(dropdown, values), at.is_some())
}

fn field_widget(field: &Field) -> &gtk::Widget {
    match field {
        Field::Text(entry) | Field::Number(entry, _) => entry.upcast_ref(),
        Field::Choice(dropdown, _) => dropdown.upcast_ref(),
    }
}

/// `widget` under a label, with the label as its accessible name.
fn labeled(content: &gtk::Box, label: &str, widget: &impl IsA<gtk::Widget>) {
    let title = gtk::Label::new(Some(label));
    title.set_xalign(0.0);
    content.append(&title);
    widget
        .as_ref()
        .update_property(&[gtk::accessible::Property::Label(label)]);
    content.append(widget);
}

/// The preview under a format field, following what is typed.
fn preview(entry: &gtk::Entry, name: &str) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.add_css_class(style::DIM);
    let name = name.to_owned();
    let update = glib::clone!(
        #[weak]
        label,
        move |entry: &gtk::Entry| {
            let text = entry.text();
            if text.is_empty() {
                label.set_text("");
                return;
            }
            let now = glib::DateTime::now_local().map_or((0, 0, 0), |now| {
                (
                    u32::try_from(now.hour()).unwrap_or(0),
                    u32::try_from(now.minute()).unwrap_or(0),
                    u32::try_from(now.second()).unwrap_or(0),
                )
            });
            let shown = catalog::preview(&name, &text, now);
            label.set_text(&trf(PREVIEW, &[("text", &shown)]));
        }
    );
    update(entry);
    entry.connect_changed(update);
    label
}

/// The definition the form holds: the JSON of the other options with the fields that are set.
fn collect(
    fields: &[(&'static str, Field)],
    buffer: &gtk::TextBuffer,
) -> Result<Map<String, Json>, String> {
    let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
    let mut definition = if text.trim().is_empty() {
        Map::new()
    } else {
        match sway_things_core::bar::jsonc::parse(&text) {
            Ok(Json::Object(map)) => map,
            Ok(_) => return Err(tr(NOT_AN_OBJECT).to_owned()),
            Err(error) => {
                return Err(trf(BAD_JSON, &[("error", &error.text().translated())]));
            }
        }
    };
    for (key, field) in fields {
        let value = match field {
            Field::Text(entry) => {
                let text = entry.text();
                (!text.is_empty()).then(|| Json::String(text.to_string()))
            }
            Field::Number(entry, kind) => number(entry, *kind, key)?,
            Field::Choice(dropdown, values) => usize::try_from(dropdown.selected())
                .ok()
                .and_then(|at| values.get(at))
                .and_then(Clone::clone),
        };
        if let Some(value) = value {
            definition.insert((*key).to_owned(), value);
        }
    }
    Ok(definition)
}

/// The number in `entry`, checked against `kind`; `None` while it is empty.
fn number(entry: &gtk::Entry, kind: Kind, key: &str) -> Result<Option<Json>, String> {
    let text = entry.text();
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let (min, max, whole) = match kind {
        #[expect(clippy::cast_precision_loss, reason = "the ranges are far below 2^52")]
        Kind::Integer { min, max } => (min as f64, max as f64, true),
        Kind::Decimal { min, max } => (min, max, false),
        _ => (f64::MIN, f64::MAX, false),
    };
    let wrong = || {
        trf(
            NOT_A_NUMBER,
            &[("option", &key), ("min", &min), ("max", &max)],
        )
    };
    if whole {
        let value: i64 = text.parse().map_err(|_| wrong())?;
        #[expect(clippy::cast_precision_loss, reason = "the ranges are far below 2^52")]
        let in_range = (min..=max).contains(&(value as f64));
        return if in_range {
            Ok(Some(Json::from(value)))
        } else {
            Err(wrong())
        };
    }
    let value: f64 = text.parse().map_err(|_| wrong())?;
    if !(min..=max).contains(&value) {
        return Err(wrong());
    }
    Number::from_f64(value)
        .map(|number| Some(Json::Number(number)))
        .ok_or_else(wrong)
}

/// Whether a module list of the page `anchor` is on has `name` already.
fn taken(anchor: &gtk::Widget, name: &str) -> bool {
    let Some(page) = anchor
        .ancestor(PageView::static_type())
        .and_downcast::<PageView>()
    else {
        return false;
    };
    [keys::MODULES_LEFT, keys::MODULES_CENTER, keys::MODULES_RIGHT]
        .iter()
        .filter_map(|key| page.row(key))
        .any(|row| matches!(row.value(), Some(Value::List(list)) if list.iter().any(|item| item == name)))
}

/// Adds `name` to the end of the list `key` on the page `anchor` is on.
fn add_to_list(anchor: &gtk::Widget, key: &str, name: &str) {
    let Some(row) = anchor
        .ancestor(PageView::static_type())
        .and_downcast::<PageView>()
        .and_then(|page| page.row(key))
    else {
        return;
    };
    let mut list = match row.value() {
        Some(Value::List(list)) => list,
        _ => Vec::new(),
    };
    list.push(name.to_owned());
    row.change(Value::List(list));
}
