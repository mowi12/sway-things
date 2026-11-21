// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Everything that starts with the session, in one list: Sway Things' own entries (switched on
//! and off, started now, removed), the `exec` lines of the user's config (read only, with file
//! and line), and the XDG autostart entries with whether they start in this session. A system
//! entry is switched off with Sway Things' own override; the user's own files are only opened in
//! the editor.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::apps::autostart::{
    self as core_autostart, ENTRIES, Entry, HIDDEN, Item, Place, Runner, Starts, XdgEntry,
};
use sway_things_core::apps::installed::App;
use sway_things_core::registry::Value;

use super::{APPS, DIRS, apps, editor_button, note_label, runners, show_icon, systemd};
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;
use crate::widgets::network::dialogs::Dialog;
use crate::widgets::setting_row::tilde;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The title of the dialog that adds an app.
const ADD_APP: Msg = msg!("Add App…");
/// The title of the dialog that adds a command.
const ADD_COMMAND: Msg = msg!("Add Command…");
/// The button of the dialog that adds an entry.
const ADD: Msg = msg!("Add");
/// The menu item that starts an entry now.
const START_NOW: Msg = msg!("Start Now");
/// The menu item that removes an entry.
const REMOVE: Msg = msg!("Remove");

/// The list of everything that starts with the session, with the buttons that add an entry.
#[derive(Debug, Clone)]
pub struct AutostartList {
    root: gtk::Box,
    lists: gtk::Box,
    add_app: gtk::Button,
    add_command: gtk::Button,
}

impl AutostartList {
    /// The list, following `model` until it is gone.
    pub fn new(model: &Model) -> Self {
        let add_app = gtk::Button::with_label(tr(ADD_APP));
        let add_command = gtk::Button::with_label(tr(ADD_COMMAND));
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar.add_css_class("st-apps-bar");
        let title = gtk::Label::new(Some(tr(msg!("What starts at login"))));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.add_css_class("heading");
        bar.append(&title);
        bar.append(&add_app);
        bar.append(&add_command);
        let lists = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("st-autostart");
        root.append(&bar);
        root.append(&lists);
        let list = Self {
            root,
            lists,
            add_app,
            add_command,
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
        list.add_app.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |button| open_add_app(button, &model)
        ));
        list.add_command.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |button| open_add_command(button, &model)
        ));
        list
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// The buttons that add an app and a command.
    pub fn add_buttons(&self) -> (&gtk::Button, &gtk::Button) {
        (&self.add_app, &self.add_command)
    }

    /// Shows what starts, as `model` says now.
    fn update(&self, model: &Model) {
        while let Some(child) = self.lists.first_child() {
            self.lists.remove(&child);
        }
        let writable = model.status().is_some_and(|status| status.is_active());
        self.add_app.set_sensitive(writable);
        self.add_command.set_sensitive(writable);
        let items = model.list(ENTRIES);
        let ours: Vec<gtk::Box> = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let entry = Entry::parse(item).ok()?;
                Some(our_row(model, index, &entry, writable))
            })
            .collect();
        self.add_list(
            tr(msg!("Started by Sway Things")),
            ours,
            tr(msg!("Nothing yet: add an app or a command")),
        );
        let hand: Vec<gtk::Box> = model
            .status()
            .zip(model.locations())
            .map(|(status, locations)| {
                core_autostart::hand_written(&status.config().directives, locations.directory())
            })
            .unwrap_or_default()
            .iter()
            .map(|line| hand_row(model, line))
            .collect();
        self.add_list(
            tr(msg!("Started by your config")),
            hand,
            tr(msg!("Your config has no exec lines")),
        );
        let runs = runners(model, systemd()).any();
        let hidden = model.list(HIDDEN);
        let xdg: Vec<gtk::Box> = DIRS
            .with(|dirs| dirs.as_ref().map(core_autostart::xdg_entries))
            .unwrap_or_default()
            .iter()
            .filter(|entry| entry.starts != Starts::NotAnApp)
            .map(|entry| xdg_row(model, entry, runs, &hidden, writable))
            .collect();
        self.add_list(
            tr(msg!(
                "XDG autostart (~/.config/autostart, /etc/xdg/autostart)"
            )),
            xdg,
            tr(msg!("No XDG autostart entries")),
        );
    }

    /// Adds a heading `title` and a list of `rows`, or `empty` without any.
    fn add_list(&self, title: &str, rows: Vec<gtk::Box>, empty: &str) {
        let heading = gtk::Label::new(Some(title));
        heading.set_xalign(0.0);
        heading.set_wrap(true);
        heading.add_css_class("st-apps-heading");
        heading.add_css_class(style::DIM);
        self.lists.append(&heading);
        if rows.is_empty() {
            let label = note_label(empty);
            label.add_css_class("st-section-hint");
            self.lists.append(&label);
            return;
        }
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("st-section");
        list.update_property(&[gtk::accessible::Property::Label(title)]);
        for row in rows {
            let item = gtk::ListBoxRow::new();
            item.set_activatable(false);
            item.set_focusable(false);
            item.set_child(Some(&row));
            list.append(&item);
        }
        self.lists.append(&list);
    }
}

/// A row: with `pictured` an icon (`icon`, else a generic one), a title and lines below it.
/// Controls go at its end.
fn row(
    pictured: bool,
    icon: Option<&str>,
    title: &str,
    monospace: bool,
    lines: &[String],
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.add_css_class("st-autostart-row");
    if pictured {
        let image = gtk::Image::new();
        image.set_pixel_size(32);
        image.set_valign(gtk::Align::Center);
        image.set_accessible_role(gtk::AccessibleRole::Presentation);
        show_icon(&image, icon);
        content.append(&image);
    }
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 3);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class("st-row-title");
    if monospace {
        label.add_css_class("monospace");
    }
    texts.append(&label);
    for line in lines {
        texts.append(&note_label(line));
    }
    content.append(&texts);
    content.update_property(&[gtk::accessible::Property::Label(title)]);
    content
}

/// A switch for a row, named after the row's title for screen readers.
fn switch(title: &str, active: bool, sensitive: bool) -> gtk::Switch {
    let switch = gtk::Switch::new();
    switch.set_active(active);
    switch.set_valign(gtk::Align::Center);
    switch.set_sensitive(sensitive);
    switch.update_property(&[gtk::accessible::Property::Label(&trf(
        msg!("Start {name} at login"),
        &[("name", &title)],
    ))]);
    switch
}

/// The installed app of ID `id`, if any.
fn app(id: &str) -> Option<App> {
    APPS.with(|apps| apps.iter().find(|app| app.id == id).cloned())
}

/// The row of Sway Things' entry at `index`.
fn our_row(model: &Model, index: usize, entry: &Entry, writable: bool) -> gtk::Box {
    let (content, title) = match &entry.item {
        Item::App(id) => {
            let installed = app(id);
            let title = installed
                .as_ref()
                .map_or_else(|| id.clone(), |app| app.name.clone());
            let mut lines = vec![id.clone()];
            if installed.is_none() {
                lines.push(tr(msg!("Not installed: it doesn't start")).to_owned());
            } else {
                lines.push(on_text(entry.on));
            }
            let icon = installed.as_ref().and_then(|app| app.icon.as_deref());
            (row(true, icon, &title, false, &lines), title)
        }
        Item::Exec(line) => (
            row(
                true,
                Some("utilities-terminal"),
                line,
                true,
                &[on_text(entry.on)],
            ),
            line.clone(),
        ),
    };
    let toggle = switch(&title, entry.on, writable);
    toggle.connect_state_set(glib::clone!(
        #[weak]
        model,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, on| {
            change(&model, ENTRIES, |items| {
                if let Some(item) = items.get_mut(index) {
                    if let Ok(mut entry) = Entry::parse(item) {
                        entry.on = on;
                        *item = entry.text();
                    }
                }
            });
            glib::Propagation::Proceed
        }
    ));
    content.append(&toggle);
    let actions = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&actions));
    if let Ok(command) = entry.command() {
        actions.append(&action(&popover, START_NOW, model, move |model| {
            model.run_command(command.clone());
        }));
    }
    if writable {
        actions.append(&action(&popover, REMOVE, model, move |model| {
            change(model, ENTRIES, |items| {
                if index < items.len() {
                    items.remove(index);
                }
            });
        }));
    }
    let menu = gtk::MenuButton::new();
    menu.set_icon_name("view-more-symbolic");
    menu.set_valign(gtk::Align::Center);
    menu.add_css_class("flat");
    menu.set_popover(Some(&popover));
    menu.update_property(&[gtk::accessible::Property::Label(&trf(
        msg!("Actions of {name}"),
        &[("name", &title)],
    ))]);
    content.append(&menu);
    content
}

/// Whether an entry of Sway Things starts at login, in words.
fn on_text(on: bool) -> String {
    tr(if on {
        msg!("Starts at login")
    } else {
        msg!("Switched off")
    })
    .to_owned()
}

/// A flat button of a row's menu, `title`, that closes `popover` and runs `run`.
fn action(
    popover: &gtk::Popover,
    title: Msg,
    model: &Model,
    run: impl Fn(&Model) + 'static,
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
        move |_| {
            popover.popdown();
            run(&model);
        }
    ));
    button
}

/// Stores the list `key` as `edit` changes it.
fn change(model: &Model, key: &'static str, edit: impl FnOnce(&mut Vec<String>)) {
    let old = model.list(key);
    let mut items = old.clone();
    edit(&mut items);
    if items != old {
        model.set(key, None, Value::List(items));
    }
}

/// The row of an `exec` line of the user's config.
fn hand_row(model: &Model, line: &core_autostart::HandExec) -> gtk::Box {
    let place = format!("{}:{}", tilde(&line.file), line.line);
    let mut lines = vec![if line.always {
        trf(
            msg!("{place}, exec_always: also at every reload"),
            &[("place", &place)],
        )
    } else {
        place
    }];
    match line.runner {
        Some(Runner::Dex) => {
            lines.push(tr(msg!("Starts the XDG autostart entries (dex)")).to_owned());
        }
        Some(Runner::Systemd) => {
            lines.push(tr(msg!("Starts the XDG autostart entries (systemd)")).to_owned());
        }
        None => {}
    }
    let content = row(false, None, &line.command, true, &lines);
    let button = editor_button(model, &line.file, Some(line.line));
    button.set_valign(gtk::Align::Center);
    content.append(&button);
    content
}

/// Whether and why an XDG autostart entry starts, in words.
fn starts_text(entry: &XdgEntry, runs: bool) -> String {
    match &entry.starts {
        Starts::Yes if runs => tr(msg!("Starts at login")).to_owned(),
        Starts::Yes => tr(msg!(
            "Doesn't start: nothing runs XDG autostart entries in this session"
        ))
        .to_owned(),
        Starts::Hidden => match entry.place {
            Place::Ours => tr(msg!("Switched off here")).to_owned(),
            Place::User => tr(msg!("Switched off in your own file (Hidden=true)")).to_owned(),
            Place::System => tr(msg!("Switched off by the system (Hidden=true)")).to_owned(),
        },
        Starts::OnlyIn(desktops) => trf(
            msg!("Doesn't start here: only for {desktops}"),
            &[("desktops", &desktops.join(", "))],
        ),
        Starts::NotIn(desktops) => trf(
            msg!("Doesn't start here: not for {desktops}"),
            &[("desktops", &desktops.join(", "))],
        ),
        Starts::NotInstalled(program) => trf(
            msg!("Doesn't start: {program} isn't installed"),
            &[("program", program)],
        ),
        Starts::NoCommand | Starts::NotAnApp => {
            tr(msg!("Doesn't start: it has no command to run")).to_owned()
        }
    }
}

/// The row of an XDG autostart entry: a switch for a system entry (switched off with Sway
/// Things' override), the editor for the user's own.
fn xdg_row(
    model: &Model,
    entry: &XdgEntry,
    runs: bool,
    hidden: &[String],
    writable: bool,
) -> gtk::Box {
    let mut lines = vec![starts_text(entry, runs)];
    if entry.gnome_disabled && entry.starts == Starts::Yes {
        lines.push(
            tr(msg!(
                "Its X-GNOME-Autostart-enabled=false is only read by GNOME: dex and systemd start \
                 it anyway"
            ))
            .to_owned(),
        );
    }
    match entry.place {
        Place::User if entry.system.is_some() => lines.push(
            tr(msg!(
                "Your own file in ~/.config/autostart takes the place of the system's"
            ))
            .to_owned(),
        ),
        Place::User => lines.push(tr(msg!("Your own entry in ~/.config/autostart")).to_owned()),
        Place::Ours | Place::System => {}
    }
    let content = row(true, entry.icon.as_deref(), &entry.name, false, &lines);
    match entry.place {
        Place::User => {
            let button = editor_button(model, &entry.path, None);
            button.set_valign(gtk::Align::Center);
            content.append(&button);
        }
        // Only what would start can be switched off; Sway Things' own override switched on.
        Place::Ours | Place::System
            if entry.starts == Starts::Yes || entry.place == Place::Ours =>
        {
            let off = entry.place == Place::Ours || hidden.contains(&entry.id);
            let toggle = switch(&entry.name, !off, writable);
            let id = entry.id.clone();
            toggle.connect_state_set(glib::clone!(
                #[weak]
                model,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, on| {
                    change(&model, HIDDEN, |ids| {
                        ids.retain(|known| *known != id);
                        if !on {
                            ids.push(id.clone());
                        }
                    });
                    glib::Propagation::Proceed
                }
            ));
            content.append(&toggle);
        }
        Place::Ours | Place::System => {}
    }
    content
}

/// The dialog that adds an installed app: the apps menus show, searchable by name.
fn open_add_app(button: &gtk::Button, model: &Model) {
    let parent = button.root().and_downcast::<gtk::Window>();
    let desktops = DIRS.with(|dirs| dirs.as_ref().map(|dirs| dirs.desktops.clone()));
    let apps: Vec<App> = apps()
        .into_iter()
        .filter(|app| app.shown_in(&desktops.clone().unwrap_or_default()))
        .collect();
    let model = model.clone();
    let (dialog, content) = Dialog::<String>::new(parent.as_ref(), tr(ADD_APP), move |id| {
        if let Some(id) = id {
            let entry = Entry {
                on: true,
                item: Item::App(id),
            };
            change(&model, ENTRIES, |items| items.push(entry.text()));
        }
    });
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(tr(msg!("Search apps"))));
    content.append(&search);
    let list = gtk::ListBox::new();
    list.add_css_class("st-section");
    list.set_activate_on_single_click(true);
    for app in &apps {
        let line = row(true, app.icon.as_deref(), &app.name, false, &[]);
        let item = gtk::ListBoxRow::new();
        item.set_child(Some(&line));
        item.set_widget_name(&app.id);
        list.append(&item);
    }
    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .min_content_height(320)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    content.append(&scroller);
    let names: Rc<Vec<String>> = Rc::new(apps.iter().map(|app| app.name.to_lowercase()).collect());
    list.set_filter_func(glib::clone!(
        #[weak]
        search,
        #[strong]
        names,
        #[upgrade_or]
        true,
        move |row| {
            let text = search.text().to_lowercase();
            let index = usize::try_from(row.index()).unwrap_or_default();
            names
                .get(index)
                .is_none_or(|name| name.contains(text.trim()))
        }
    ));
    search.connect_search_changed(glib::clone!(
        #[weak]
        list,
        move |_| list.invalidate_filter()
    ));
    let answer = dialog.clone();
    list.connect_row_activated(move |_, row| {
        answer.finish(Some(row.widget_name().to_string()));
    });
    dialog.buttons(&content, None);
    dialog.window().present();
}

/// The dialog that adds a command line, which sway runs with the shell at login.
fn open_add_command(button: &gtk::Button, model: &Model) {
    let parent = button.root().and_downcast::<gtk::Window>();
    let model = model.clone();
    let (dialog, content) = Dialog::<String>::new(parent.as_ref(), tr(ADD_COMMAND), move |line| {
        if let Some(line) = line {
            change(&model, ENTRIES, |items| items.push(format!("exec {line}")));
        }
    });
    let body = gtk::Label::new(Some(tr(msg!(
        "sway runs it with the shell at login, like an exec line of your config"
    ))));
    body.set_wrap(true);
    body.set_max_width_chars(40);
    content.append(&body);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("nm-applet --indicator"));
    entry.add_css_class("monospace");
    entry.set_activates_default(true);
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
