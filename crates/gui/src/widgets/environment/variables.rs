// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The list of environment variables on the Environment page, in place of the row of
//! `environment.variables`.
//!
//! - The user's own variables come first, a line each: the name, the value as written and what
//!   it expands to, a warning sign where another file replaces it or where the variable is one
//!   every program depends on, and a ⋮ menu to edit or remove it. _Add Variable…_ opens the
//!   same dialog as _Edit…_, which checks the name and the value as they are typed.
//! - Then the variables of the other environment.d files, read only: the file and line, whether
//!   it replaces one of Sway Things' variables or is replaced itself, and _Open in Editor_ for
//!   the user's own files.

use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::environment::files::Origin;
use sway_things_core::environment::syntax;
use sway_things_core::registry::Value;
use sway_things_core::registry::environment::{
    VARIABLES, VariableError, check_name, check_variable, split, warning,
};

use super::{BASE, Listing, dialog_window, load_base};
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;
use crate::widgets::list;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The button that adds a variable.
pub(crate) const ADD_VARIABLE: Msg = msg!("Add Variable…");
/// The heading of the dialog that adds one.
const NEW_VARIABLE: Msg = msg!("New Variable");
/// The heading of the dialog that edits one.
const EDIT_VARIABLE: Msg = msg!("Edit Variable");
/// A name the user already has.
const TWICE: Msg = msg!("You already set this variable: edit that line");
/// What the value expands to, in the dialog.
const EXPANDS_TO: Msg = msg!("Expands to: {value}");
/// The button that opens a user's file.
const OPEN_IN_EDITOR: Msg = msg!("Open in Editor");

/// The list of environment variables, with the button that adds one.
#[derive(Clone)]
pub struct VariableList {
    root: gtk::Box,
    ours: gtk::ListBox,
    empty: gtk::Label,
    others: gtk::ListBox,
    others_heading: gtk::Label,
    add: gtk::Button,
}

impl std::fmt::Debug for VariableList {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VariableList")
            .finish_non_exhaustive()
    }
}

impl VariableList {
    /// The list, following `model` until it is gone.
    pub fn new(model: &Model) -> Self {
        let add = gtk::Button::new();
        let add_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        add_content.append(&gtk::Image::from_icon_name("list-add-symbolic"));
        add_content.append(&gtk::Label::new(Some(tr(ADD_VARIABLE))));
        add.set_child(Some(&add_content));
        add.update_property(&[gtk::accessible::Property::Label(tr(ADD_VARIABLE))]);
        let intro = gtk::Label::new(Some(tr(msg!(
            "Variables for every app of the session, in Sway Things' environment.d file. They \
             apply after re-login."
        ))));
        intro.set_xalign(0.0);
        intro.set_wrap(true);
        intro.set_hexpand(true);
        intro.add_css_class(style::DIM);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        bar.add_css_class("st-environment-bar");
        bar.append(&intro);
        bar.append(&add);
        let ours = list::section_list(tr(msg!("Your variables")));
        ours.add_css_class("st-environment-list");
        let empty = gtk::Label::new(None);
        list::hint(&empty);
        let others_heading = gtk::Label::new(Some(tr(msg!("Set elsewhere"))));
        others_heading.set_xalign(0.0);
        others_heading.add_css_class("st-environment-heading");
        others_heading.add_css_class(style::DIM);
        let others = list::section_list(tr(msg!("Set elsewhere")));
        others.add_css_class("st-environment-list");
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.add_css_class("st-environment");
        root.append(&bar);
        root.append(&ours);
        root.append(&empty);
        root.append(&others_heading);
        root.append(&others);
        let shown = Self {
            root,
            ours,
            empty,
            others,
            others_heading,
            add,
        };
        shown.update(model);
        let weak = shown.root.downgrade();
        let follower = shown.clone();
        let handler = std::cell::RefCell::new(Some(model.connect_changed(move |model| {
            if weak.upgrade().is_some() {
                follower.update(model);
            }
        })));
        shown.root.connect_destroy(glib::clone!(
            #[weak]
            model,
            move |_| {
                if let Some(handler) = handler.take() {
                    model.disconnect(handler);
                }
            }
        ));
        shown.add.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |button| open_dialog(button.upcast_ref(), &model, None)
        ));
        let weak = shown.root.downgrade();
        let follower = shown.clone();
        let model_weak = model.downgrade();
        load_base(move || {
            if let (Some(_), Some(model)) = (weak.upgrade(), model_weak.upgrade()) {
                follower.update(&model);
            }
        });
        shown
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// The button that adds a variable.
    pub fn add_button(&self) -> &gtk::Button {
        &self.add
    }

    /// The list of the user's own variables.
    pub fn ours(&self) -> &gtk::ListBox {
        &self.ours
    }

    /// The list of the variables of other files.
    pub fn others(&self) -> &gtk::ListBox {
        &self.others
    }

    /// Shows the variables, as `model` says now.
    fn update(&self, model: &Model) {
        let writable = model.status().is_some_and(|status| status.is_active());
        self.add.set_sensitive(writable);
        let listing = Listing::of(model);
        clear(&self.ours);
        clear(&self.others);
        let items = model.list(VARIABLES);
        for (index, item) in items.iter().enumerate() {
            let Some((name, value)) = split(item) else {
                continue;
            };
            self.ours
                .append(&our_line(model, &listing, index, name, value, writable));
        }
        self.ours.set_visible(!items.is_empty());
        list::set_hint(
            &self.empty,
            items
                .is_empty()
                .then(|| tr(msg!("No variables of your own yet"))),
        );
        let mut others = 0;
        for (index, line) in listing.lines.iter().enumerate() {
            if line.origin != Origin::Ours {
                self.others.append(&other_line(model, &listing, index));
                others += 1;
            }
        }
        self.others.set_visible(others > 0);
        self.others_heading.set_visible(others > 0);
    }
}

/// Removes every row of `list`.
fn clear(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

/// A row with `title` (a variable's name, in monospace), the lines of `below` under it, and
/// `controls` at its end.
fn line_row(
    title: &str,
    below: &[String],
    problem: Option<&str>,
    controls: &[gtk::Widget],
) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.add_css_class("st-environment-row");
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.set_ellipsize(gtk::pango::EllipsizeMode::End);
    heading.add_css_class("st-row-title");
    heading.add_css_class("monospace");
    texts.append(&heading);
    for text in below {
        let label = gtk::Label::new(Some(text));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.add_css_class(style::DIM);
        label.add_css_class("st-environment-subtitle");
        texts.append(&label);
    }
    content.append(&texts);
    let mut said = below.to_vec();
    if let Some(problem) = problem {
        let sign = gtk::Image::from_icon_name("dialog-warning-symbolic");
        sign.add_css_class("st-environment-warning");
        sign.set_valign(gtk::Align::Center);
        sign.set_tooltip_text(Some(problem));
        content.append(&sign);
        said.push(problem.to_owned());
    }
    for control in controls {
        content.append(control);
    }
    let row = list::list_row(&content);
    row.update_property(&[
        gtk::accessible::Property::Label(title),
        gtk::accessible::Property::Description(&said.join(". ")),
    ]);
    row
}

/// The line of the user's variable at `index` of `environment.variables`.
fn our_line(
    model: &Model,
    listing: &Listing,
    index: usize,
    name: &str,
    value: &str,
    writable: bool,
) -> gtk::ListBoxRow {
    let mut below = vec![value.to_owned()];
    let at = listing.ours(name);
    if let Some(expanded) = at.and_then(|at| listing.expanded.get(at)) {
        if *expanded != syntax::unquote(value) || value.contains('$') {
            below.push(trf(EXPANDS_TO, &[("value", expanded)]));
        }
    }
    let mut problems = Vec::new();
    if let Some(replaced) = at.and_then(|at| listing.replaced(at)) {
        problems.push(replaced);
    }
    if let Some(warning) = warning(name) {
        problems.push(tr(warning).to_owned());
    }
    let problem = (!problems.is_empty()).then(|| problems.join(". "));
    let menu = menu(model, index, name, writable);
    line_row(name, &below, problem.as_deref(), &[menu.upcast()])
}

/// The ⋮ menu of the user's variable at `index`: _Edit…_ and _Remove_.
fn menu(model: &Model, index: usize, name: &str, writable: bool) -> gtk::MenuButton {
    let menu_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&menu_box));
    let edit = gtk::Button::with_label(tr(msg!("Edit…")));
    let remove = gtk::Button::with_label(tr(msg!("Remove")));
    for item in [&edit, &remove] {
        item.add_css_class("flat");
        if let Some(child) = item.child().and_downcast::<gtk::Label>() {
            child.set_xalign(0.0);
        }
        menu_box.append(item);
    }
    edit.connect_clicked(glib::clone!(
        #[weak]
        model,
        #[weak]
        popover,
        move |button| {
            popover.popdown();
            open_dialog(button.upcast_ref(), &model, Some(index));
        }
    ));
    remove.connect_clicked(glib::clone!(
        #[weak]
        model,
        #[weak]
        popover,
        move |_| {
            popover.popdown();
            change(&model, |items| {
                if index < items.len() {
                    items.remove(index);
                }
            });
        }
    ));
    let button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .popover(&popover)
        .valign(gtk::Align::Center)
        .tooltip_text(tr(msg!("More actions")))
        .sensitive(writable)
        .build();
    button.add_css_class("flat");
    button.update_property(&[gtk::accessible::Property::Label(&trf(
        msg!("Actions for {name}"),
        &[("name", &name)],
    ))]);
    button
}

/// The line of another file's variable at `index` of the listing: read only.
fn other_line(model: &Model, listing: &Listing, index: usize) -> gtk::ListBoxRow {
    let Some(line) = listing.lines.get(index) else {
        return gtk::ListBoxRow::new();
    };
    let mut below = vec![line.value.clone(), Listing::place(line)];
    let replaces_ours = listing.overridden_by.iter().enumerate().any(|(at, by)| {
        *by == Some(index)
            && listing
                .lines
                .get(at)
                .is_some_and(|l| l.origin == Origin::Ours)
    });
    if replaces_ours {
        below.push(tr(msg!("Overrides yours")).to_owned());
    }
    let replaced = listing.replaced(index);
    let control: gtk::Widget = if line.origin == Origin::User {
        editor_button(model, line.file.clone(), line.line).upcast()
    } else {
        let lock = gtk::Image::from_icon_name("changes-prevent-symbolic");
        lock.add_css_class(style::DIM);
        lock.set_valign(gtk::Align::Center);
        lock.set_tooltip_text(Some(tr(msg!(
            "A file of the system: Sway Things doesn't change it"
        ))));
        lock.upcast()
    };
    if let Some(replaced) = &replaced {
        below.push(replaced.clone());
    }
    line_row(&line.name, &below, None, &[control])
}

/// A button that opens `file` at `line` in the editor.
pub(super) fn editor_button(model: &Model, file: PathBuf, line: usize) -> gtk::Button {
    let button = gtk::Button::with_label(tr(OPEN_IN_EDITOR));
    button.set_valign(gtk::Align::Center);
    button.connect_clicked(glib::clone!(
        #[weak]
        model,
        move |button| {
            let reporter = model.clone();
            crate::editor::open(button, Some(&model), &file, Some(line), move |error| {
                if let Some(error) = error {
                    reporter.emit_problem(error);
                }
            });
        }
    ));
    button
}

/// Stores `environment.variables` as `edit` changes it.
fn change(model: &Model, edit: impl FnOnce(&mut Vec<String>)) {
    let old = model.list(VARIABLES);
    let mut items = old.clone();
    edit(&mut items);
    if items != old {
        model.set(VARIABLES, None, Value::List(items));
    }
}

/// What the dialog says about a name and a value as typed: why they can't be saved, else a
/// warning and what the value expands to.
fn judge(
    model: &Model,
    editing: Option<usize>,
    name: &str,
    value: &str,
) -> Result<Vec<String>, String> {
    if name.is_empty() {
        return Err(String::new());
    }
    check_name(name).map_err(|error| error.text().translated())?;
    let twice = model
        .list(VARIABLES)
        .iter()
        .enumerate()
        .any(|(index, item)| {
            Some(index) != editing && split(item).is_some_and(|(other, _)| other == name)
        });
    if twice {
        return Err(tr(TWICE).to_owned());
    }
    match check_variable(&format!("{name}={value}")) {
        Err(VariableError::Syntax(syntax::SyntaxError::Empty)) if value.is_empty() => {
            return Err(String::new());
        }
        Err(error) => return Err(error.text().translated()),
        Ok(()) => {}
    }
    let mut said = Vec::new();
    if let Some(warning) = warning(name) {
        said.push(tr(warning).to_owned());
    }
    let expanded = BASE.with(|base| {
        let base = base.borrow();
        syntax::expand(&syntax::unquote(value), &|name| base.get(name).cloned())
    });
    said.push(trf(EXPANDS_TO, &[("value", &expanded)]));
    Ok(said)
}

/// Opens the dialog that adds a variable, or edits the one at `editing`.
fn open_dialog(from: &gtk::Widget, model: &Model, editing: Option<usize>) {
    let items = model.list(VARIABLES);
    let (name, value) = editing
        .and_then(|index| items.get(index))
        .and_then(|item| split(item))
        .map_or((String::new(), String::new()), |(name, value)| {
            (name.to_owned(), value.to_owned())
        });
    let title = tr(if editing.is_some() {
        EDIT_VARIABLE
    } else {
        NEW_VARIABLE
    });
    let (window, content) = dialog_window(from, title);
    let name_entry = field(&content, msg!("Name"), &name, "EDITOR");
    let value_entry = field(&content, msg!("Value"), &value, "$HOME/.local/bin:$PATH");
    let hint = gtk::Label::new(Some(tr(msg!(
        "$NAME and ${NAME} expand, ${NAME:-default} where it is unset, ${NAME:+alternate} where \
         it is set; $$ is a $. Quotes keep spaces."
    ))));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class(style::DIM);
    content.append(&hint);
    let feedback = gtk::Label::new(None);
    feedback.set_xalign(0.0);
    feedback.set_wrap(true);
    feedback.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    feedback.set_selectable(true);
    feedback.add_css_class("st-environment-feedback");
    content.append(&feedback);
    let save = buttons(&window, &content);
    window.set_default_widget(Some(&save));
    value_entry.set_activates_default(true);

    let check: Rc<dyn Fn()> = Rc::new(glib::clone!(
        #[weak]
        model,
        #[weak]
        name_entry,
        #[weak]
        value_entry,
        #[weak]
        feedback,
        #[weak]
        save,
        move || {
            let judged = judge(&model, editing, &name_entry.text(), &value_entry.text());
            save.set_sensitive(judged.is_ok());
            match judged {
                Ok(said) => {
                    feedback.remove_css_class("st-environment-error");
                    feedback.set_text(&said.join("\n"));
                }
                Err(error) => {
                    feedback.add_css_class("st-environment-error");
                    feedback.set_text(&error);
                }
            }
        }
    ));
    check();
    for entry in [&name_entry, &value_entry] {
        let check = Rc::clone(&check);
        entry.connect_changed(move |_| check());
    }
    save.connect_clicked(glib::clone!(
        #[weak]
        model,
        #[weak]
        window,
        #[weak]
        name_entry,
        #[weak]
        value_entry,
        move |_| {
            let (name, value) = (name_entry.text(), value_entry.text());
            if judge(&model, editing, &name, &value).is_err() {
                return;
            }
            let item = format!("{name}={value}");
            change(&model, |items| {
                match editing.and_then(|index| items.get_mut(index)) {
                    Some(old) => *old = item,
                    None => items.push(item),
                }
            });
            window.close();
        }
    ));
    window.present();
}

/// A caption with `label` and an entry below it in `content`, holding `text`, with
/// `placeholder` while empty.
fn field(content: &gtk::Box, label: Msg, text: &str, placeholder: &str) -> gtk::Entry {
    let entry = gtk::Entry::new();
    entry.set_text(text);
    entry.add_css_class("monospace");
    entry.set_placeholder_text(Some(placeholder));
    let caption = gtk::Label::new(Some(tr(label)));
    caption.set_xalign(0.0);
    caption.set_mnemonic_widget(Some(&entry));
    content.append(&caption);
    content.append(&entry);
    entry.update_property(&[gtk::accessible::Property::Label(tr(label))]);
    entry
}

/// _Cancel_, which closes `window`, and _Save_ at the end of `content`. Returns _Save_.
fn buttons(window: &gtk::Window, content: &gtk::Box) -> gtk::Button {
    let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
    let save = gtk::Button::with_label(tr(msg!("Save")));
    save.add_css_class("suggested-action");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(8);
    buttons.append(&cancel);
    buttons.append(&save);
    content.append(&buttons);
    cancel.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));
    save
}
