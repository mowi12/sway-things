// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialogs of the list of what starts with the session:
//!
//! - the details of an entry, opened by a click on its line: how it starts now, its command,
//!   where it comes from (file and line), and why it can't be changed here, if so; with _Open in
//!   Editor_ for the user's own files, and _Start Now_ and _Remove_ for Sway Things' entries;
//! - _Add App…_: the installed apps that menus show, searchable by name, the ones added already
//!   marked; its first item, _Run a Command…_, asks for a command line instead.

use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::apps::autostart::{ENTRIES, Entry, Item};
use sway_things_core::apps::installed::App;

use super::autostart::change;
use super::{DIRS, apps, command_tile, editor_button, show_icon};
use crate::i18n::{Msg, msg, tr};
use crate::model::Model;
use crate::style;
use crate::widgets::network::dialogs::Dialog;
use crate::widgets::setting_row::tilde;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The title of the dialog that adds a command.
const ADD_COMMAND: Msg = msg!("Add a Command");
/// The button of the dialog that adds a command.
const ADD: Msg = msg!("Add");
/// The button that starts an entry now.
const START_NOW: Msg = msg!("Start Now");
/// The button that removes an entry.
const REMOVE: Msg = msg!("Remove");
/// The line of an app added already.
const ADDED: Msg = msg!("Added already");

/// What the details dialog of an entry shows.
#[derive(Debug, Clone)]
pub(super) struct Details {
    /// Its title: the entry's name or command line.
    pub title: String,
    /// The icon of its app, if any.
    pub icon: Option<String>,
    /// Whether and why it starts, in words.
    pub status: String,
    /// The command line it runs.
    pub command: Option<String>,
    /// Where it comes from, in words.
    pub from: String,
    /// The file it is in, and the line.
    pub file: Option<(PathBuf, Option<usize>)>,
    /// The desktop file ID of the app it starts.
    pub app: Option<String>,
    /// What else to know about it, a paragraph each: why it can't be changed here, how it is
    /// switched off.
    pub notes: Vec<String>,
    /// The file (and line) _Open in Editor_ opens, for the user's own.
    pub editor: Option<(PathBuf, Option<usize>)>,
    /// The index of Sway Things' entry in `autostart.entries`, for _Start Now_ and _Remove_.
    pub ours: Option<usize>,
}

/// The window `from` is in.
fn parent_of(from: &gtk::Widget) -> Option<gtk::Window> {
    from.root().and_downcast::<gtk::Window>()
}

/// A dialog window titled `title` over `parent`, which Escape closes: resizable, so long lines
/// wrap at its width. Returns it, its content and its heading.
fn dialog_window(parent: Option<&gtk::Window>, title: &str) -> (gtk::Window, gtk::Box, gtk::Label) {
    let window = gtk::Window::builder()
        .modal(true)
        .resizable(true)
        .destroy_with_parent(true)
        .default_width(460)
        .title(title)
        .build();
    window.set_transient_for(parent);
    window.add_css_class("st-dialog");
    window.add_css_class("st-autostart-dialog");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(24);
    content.set_margin_bottom(16);
    content.set_margin_start(20);
    content.set_margin_end(20);
    let heading = gtk::Label::new(Some(title));
    heading.set_wrap(true);
    heading.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    heading.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
    heading.set_justify(gtk::Justification::Center);
    heading.set_max_width_chars(36);
    heading.add_css_class("title-2");
    heading.set_accessible_role(gtk::AccessibleRole::Heading);
    content.append(&heading);
    window.set_child(Some(&content));
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
    (window, content, heading)
}

/// Opens the details of an entry.
pub(super) fn open_details(from: &gtk::Widget, model: &Model, details: &Details) {
    let (window, content, heading) = dialog_window(parent_of(from).as_ref(), &details.title);
    if details.command.as_deref() == Some(details.title.as_str()) {
        heading.add_css_class("monospace");
    }
    if let Some(icon) = &details.icon {
        let image = gtk::Image::new();
        image.set_pixel_size(64);
        image.set_accessible_role(gtk::AccessibleRole::Presentation);
        show_icon(&image, Some(icon));
        content.prepend(&image);
    }
    let status = gtk::Label::new(Some(&details.status));
    status.set_wrap(true);
    status.set_justify(gtk::Justification::Center);
    status.add_css_class(style::DIM);
    content.append(&status);
    content.append(&facts(details));
    for note in &details.notes {
        let label = gtk::Label::new(Some(note));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_max_width_chars(48);
        label.add_css_class(style::DIM);
        label.add_css_class("st-autostart-note");
        content.append(&label);
    }
    let buttons = actions(model, details, &window);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    buttons.append(&spacer);
    let close = gtk::Button::with_label(tr(msg!("Close")));
    close.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));
    buttons.append(&close);
    content.append(&buttons);
    // Not a value: a selectable one would take the focus with its text selected.
    GtkWindowExt::set_focus(&window, Some(&close));
    window.present();
}

/// The facts of an entry, a name and a value each: where it comes from, its command line, its
/// app and its file and line, the values after the first selectable.
fn facts(details: &Details) -> gtk::Grid {
    let grid = gtk::Grid::builder()
        .column_spacing(16)
        .row_spacing(8)
        .margin_top(8)
        .build();
    grid.add_css_class("st-autostart-facts");
    let mut rows = vec![(tr(msg!("From")), details.from.clone(), false)];
    // A command line that is the title already isn't said twice.
    if let Some(command) = details
        .command
        .as_ref()
        .filter(|command| **command != details.title)
    {
        rows.push((tr(msg!("Command")), command.clone(), true));
    }
    if let Some(id) = &details.app {
        rows.push((tr(msg!("App")), id.clone(), true));
    }
    if let Some((file, line)) = &details.file {
        let place = match line {
            Some(line) => format!("{}:{line}", tilde(file)),
            None => tilde(file),
        };
        rows.push((tr(msg!("File")), place, false));
    }
    for (row, (name, value, monospace)) in (0..).zip(rows) {
        let selectable = row > 0;
        let name = gtk::Label::new(Some(name));
        name.set_xalign(0.0);
        name.set_yalign(0.0);
        name.add_css_class(style::DIM);
        let value = gtk::Label::new(Some(&value));
        value.set_xalign(0.0);
        value.set_selectable(selectable);
        value.set_wrap(true);
        value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        // Its natural width as wrapped at words: inherited from breaking anywhere, it would be a
        // few letters, and the window as high as that makes the label.
        value.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
        value.set_hexpand(true);
        value.set_max_width_chars(40);
        if monospace {
            value.add_css_class("monospace");
        }
        value.update_relation(&[gtk::accessible::Relation::LabelledBy(&[name.upcast_ref()])]);
        grid.attach(&name, 0, row, 1, 1);
        grid.attach(&value, 1, row, 1, 1);
    }
    grid
}

/// The buttons of an entry's details at their start: _Remove_ and _Start Now_ for Sway Things'
/// own, _Open in Editor_ for the user's.
fn actions(model: &Model, details: &Details, window: &gtk::Window) -> gtk::Box {
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_margin_top(12);
    buttons.add_css_class("st-autostart-buttons");
    let writable = model.status().is_some_and(|status| status.is_active());
    if let Some(index) = details.ours {
        let remove = gtk::Button::with_label(tr(REMOVE));
        remove.add_css_class("destructive-action");
        remove.set_sensitive(writable);
        remove.connect_clicked(glib::clone!(
            #[weak]
            model,
            #[weak]
            window,
            move |_| {
                change(&model, ENTRIES, |items| {
                    if index < items.len() {
                        items.remove(index);
                    }
                });
                window.close();
            }
        ));
        buttons.append(&remove);
        let command = model
            .list(ENTRIES)
            .get(index)
            .and_then(|item| Entry::parse(item).ok())
            .and_then(|entry| entry.command().ok());
        if let Some(command) = command {
            let start = gtk::Button::with_label(tr(START_NOW));
            start.set_tooltip_text(Some(tr(msg!(
                "Starts it now, as at login. If it runs already, it may start a second time"
            ))));
            start.connect_clicked(glib::clone!(
                #[weak]
                model,
                move |_| model.run_command(command.clone())
            ));
            buttons.append(&start);
        }
    }
    if let Some((file, line)) = &details.editor {
        buttons.append(&editor_button(model, file, *line));
    }
    buttons
}

/// A line of the list of apps to add: the icon (of `command`, the command's sign), the name,
/// and a line below it.
fn app_line(icon: Option<&str>, command: bool, name: &str, line: Option<&str>) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.add_css_class("st-autostart-pick");
    if command {
        content.append(&command_tile());
    } else {
        let image = gtk::Image::new();
        image.set_pixel_size(32);
        image.set_accessible_role(gtk::AccessibleRole::Presentation);
        show_icon(&image, icon);
        content.append(&image);
    }
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_valign(gtk::Align::Center);
    texts.set_hexpand(true);
    let title = gtk::Label::new(Some(name));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    texts.append(&title);
    if let Some(line) = line {
        let label = gtk::Label::new(Some(line));
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.add_css_class(style::DIM);
        label.add_css_class("st-autostart-subtitle");
        texts.append(&label);
    }
    content.append(&texts);
    content
}

/// What the dialog that adds an entry answers.
enum Added {
    /// An installed app, by its desktop file ID.
    App(String),
    /// A command line, asked for next.
    Command,
}

/// The list of the dialog that adds an entry: _Run a Command…_, then `apps`, those `added`
/// already marked and not to be picked; and the names of its items in lowercase, to search.
fn app_list(apps: &[App], added: &[String]) -> (gtk::ListBox, Rc<Vec<String>>) {
    let list = gtk::ListBox::new();
    list.add_css_class("st-section");
    list.add_css_class("st-autostart-picker");
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_activate_on_single_click(true);
    let command = gtk::ListBoxRow::new();
    command.set_child(Some(&app_line(
        None,
        true,
        tr(msg!("Run a Command…")),
        Some(tr(msg!("Any command line, run by the shell"))),
    )));
    command.set_widget_name("");
    list.append(&command);
    let mut names: Vec<String> = vec![String::new()];
    for app in apps {
        let done = added.contains(&app.id);
        let item = gtk::ListBoxRow::new();
        item.set_child(Some(&app_line(
            app.icon.as_deref(),
            false,
            &app.name,
            done.then(|| tr(ADDED)),
        )));
        item.set_widget_name(&app.id);
        item.set_sensitive(!done);
        item.update_property(&[gtk::accessible::Property::Label(&app.name)]);
        list.append(&item);
        names.push(app.name.to_lowercase());
    }
    (list, Rc::new(names))
}

/// The dialog that adds an entry: the installed apps that menus show, searchable by name, after
/// _Run a Command…_.
pub(super) fn open_add(from: &gtk::Widget, model: &Model) {
    let parent = parent_of(from);
    let desktops = DIRS.with(|dirs| dirs.as_ref().map(|dirs| dirs.desktops.clone()));
    let mut apps: Vec<App> = apps()
        .into_iter()
        .filter(|app| app.shown_in(&desktops.clone().unwrap_or_default()))
        .collect();
    apps.sort_by_key(|app| app.name.to_lowercase());
    let added: Vec<String> = model
        .list(ENTRIES)
        .iter()
        .filter_map(|item| match Entry::parse(item).ok()?.item {
            Item::App(id) => Some(id),
            Item::Exec(_) => None,
        })
        .collect();
    let answer_model = model.clone();
    let answer_parent = parent.clone();
    let title = tr(msg!("Start with Your Session"));
    let (dialog, content) =
        Dialog::<Added>::new(parent.as_ref(), title, move |answer| match answer {
            Some(Added::App(id)) => {
                let entry = Entry {
                    on: true,
                    item: Item::App(id),
                };
                change(&answer_model, ENTRIES, |items| items.push(entry.text()));
            }
            Some(Added::Command) => open_add_command(answer_parent.as_ref(), &answer_model),
            None => {}
        });
    dialog.window().add_css_class("st-autostart-dialog");
    dialog.window().set_default_width(440);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(tr(msg!("Search apps"))));
    search.update_property(&[gtk::accessible::Property::Label(tr(msg!("Search apps")))]);
    content.append(&search);
    let (list, names) = app_list(&apps, &added);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .min_content_height(360)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    content.append(&scroller);
    list.set_filter_func(glib::clone!(
        #[weak]
        search,
        #[strong]
        names,
        #[upgrade_or]
        true,
        move |row| {
            let text = search.text().to_lowercase();
            let text = text.trim();
            let index = usize::try_from(row.index()).unwrap_or_default();
            // The command stays, whatever the search.
            index == 0 || names.get(index).is_none_or(|name| name.contains(text))
        }
    ));
    search.connect_search_changed(glib::clone!(
        #[weak]
        list,
        move |_| list.invalidate_filter()
    ));
    let answer = dialog.clone();
    list.connect_row_activated(move |_, row| {
        let id = row.widget_name().to_string();
        answer.finish(Some(if id.is_empty() {
            Added::Command
        } else {
            Added::App(id)
        }));
    });
    dialog.buttons(&content, None);
    dialog.window().present();
    search.grab_focus();
}

/// The dialog that adds a command line, which sway runs with the shell at login.
fn open_add_command(parent: Option<&gtk::Window>, model: &Model) {
    let model = model.clone();
    let (dialog, content) = Dialog::<String>::new(parent, tr(ADD_COMMAND), move |line| {
        if let Some(line) = line {
            change(&model, ENTRIES, |items| items.push(format!("exec {line}")));
        }
    });
    dialog.window().add_css_class("st-autostart-dialog");
    let body = gtk::Label::new(Some(tr(msg!(
        "sway runs it with the shell at login, like an exec line of your config"
    ))));
    body.set_wrap(true);
    body.set_max_width_chars(40);
    body.set_justify(gtk::Justification::Center);
    body.add_css_class(style::DIM);
    content.append(&body);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("nm-applet --indicator"));
    entry.add_css_class("monospace");
    entry.set_activates_default(true);
    entry.update_property(&[gtk::accessible::Property::Label(tr(msg!("Command")))]);
    content.append(&entry);
    if let Some(accept) = dialog.buttons(&content, Some(tr(ADD))) {
        let check = glib::clone!(
            #[weak]
            accept,
            #[weak]
            entry,
            move || {
                let text = entry.text();
                accept.set_sensitive(
                    !text.trim().is_empty()
                        && Entry::parse(&format!("exec {}", text.trim())).is_ok(),
                );
            }
        );
        check();
        entry.connect_changed(move |_| check());
        let answer = dialog.clone();
        accept.connect_clicked(glib::clone!(
            #[weak]
            entry,
            move |_| answer.finish(Some(entry.text().trim().to_owned()))
        ));
    }
    dialog.window().present();
    entry.grab_focus();
}
