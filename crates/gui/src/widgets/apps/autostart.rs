// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Everything that starts with the session, in one list, a line per entry: its icon, its name
//! (or command line), where it comes from in plain words, and at the end a switch where Sway
//! Things can switch it off, else a lock that says why it can't. A click on a line opens its
//! details ([`super::dialogs`]): the command, the file and line, and what can be done.
//!
//! - Sway Things' own entries come first, switched on and off here.
//! - Then the XDG autostart entries that start in this session or are switched off here: a system
//!   entry is switched off with Sway Things' own override; the user's own files are only opened
//!   in the editor.
//! - Then the `exec` lines of the user's config, read only.
//! - The XDG autostart entries that don't start in this session at all (only for GNOME, their
//!   program missing, switched off by their own file) are folded away below the list.
//!
//! _Add App…_ opens a list of the installed apps, searchable, whose first item adds a command.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::apps::autostart::{
    self as core_autostart, ENTRIES, Entry, HIDDEN, HandExec, Item, Place, Runner, Starts, XdgEntry,
};
use sway_things_core::registry::Value;

use super::dialogs::{self, Details};
use super::{DIRS, app, command_tile, runners, show_icon, systemd};
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;
use crate::widgets::list;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The button that adds an entry.
pub(super) const ADD_APP: Msg = msg!("Add App…");

/// What a click on a line of the list opens.
type Open = Rc<dyn Fn(&gtk::Widget, &Model)>;

/// What a line's switch does when switched: stores its new state.
type Toggle = Rc<dyn Fn(&Model, bool)>;

/// The list of everything that starts with the session, with the button that adds an entry.
#[derive(Clone)]
pub struct AutostartList {
    root: gtk::Box,
    list: gtk::ListBox,
    empty: gtk::Label,
    more: gtk::Expander,
    more_list: gtk::ListBox,
    add: gtk::Button,
    /// What a click on each line of `list` and of `more_list` opens, by index.
    opens: Rc<RefCell<(Vec<Open>, Vec<Open>)>>,
    /// Counts the reads of the XDG autostart entries, so only the newest is shown.
    turn: Rc<Cell<u64>>,
}

impl std::fmt::Debug for AutostartList {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AutostartList")
            .finish_non_exhaustive()
    }
}

impl AutostartList {
    /// The list, following `model` until it is gone.
    pub fn new(model: &Model) -> Self {
        let add = gtk::Button::new();
        let add_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        add_content.append(&gtk::Image::from_icon_name("list-add-symbolic"));
        add_content.append(&gtk::Label::new(Some(tr(ADD_APP))));
        add.set_child(Some(&add_content));
        add.update_property(&[gtk::accessible::Property::Label(tr(ADD_APP))]);
        let intro = gtk::Label::new(Some(tr(msg!(
            "The apps and commands that start when you log in"
        ))));
        intro.set_xalign(0.0);
        intro.set_wrap(true);
        intro.set_hexpand(true);
        intro.add_css_class(style::DIM);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        bar.add_css_class("st-autostart-bar");
        bar.append(&intro);
        bar.append(&add);
        let list = list::section_list(tr(msg!("Starts with your session")));
        list.add_css_class("st-autostart-list");
        let empty = gtk::Label::new(None);
        list::hint(&empty);
        let more_list = list::section_list(tr(msg!("Not started in this session")));
        more_list.add_css_class("st-autostart-list");
        let more = gtk::Expander::new(None);
        more.add_css_class("st-autostart-more");
        more.set_child(Some(&more_list));
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.add_css_class("st-autostart");
        root.append(&bar);
        root.append(&list);
        root.append(&empty);
        root.append(&more);
        let shown = Self {
            root,
            list,
            empty,
            more,
            more_list,
            add,
            opens: Rc::new(RefCell::new((Vec::new(), Vec::new()))),
            turn: Rc::new(Cell::new(0)),
        };
        shown.update(model);
        for (list, second) in [(&shown.list, false), (&shown.more_list, true)] {
            let opens = Rc::clone(&shown.opens);
            list.connect_row_activated(glib::clone!(
                #[weak]
                model,
                move |_, row| {
                    let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
                    let open = {
                        let opens = opens.borrow();
                        let opens = if second { &opens.1 } else { &opens.0 };
                        opens.get(index).cloned()
                    };
                    if let Some(open) = open {
                        open(row.upcast_ref(), &model);
                    }
                }
            ));
        }
        let weak = shown.root.downgrade();
        let follower = shown.clone();
        let handler = RefCell::new(Some(model.connect_changed(move |model| {
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
            move |button| dialogs::open_add(button.upcast_ref(), &model)
        ));
        shown
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// The button that adds an app or a command.
    pub fn add_button(&self) -> &gtk::Button {
        &self.add
    }

    /// The expander of the entries that don't start in this session.
    pub fn more(&self) -> &gtk::Expander {
        &self.more
    }

    /// Shows what starts, as `model` says now, once the XDG autostart entries are read off the
    /// main context; only the newest read is shown.
    fn update(&self, model: &Model) {
        let started = self.turn.get().wrapping_add(1);
        self.turn.set(started);
        let dirs = DIRS.with(Clone::clone);
        let shown = self.clone();
        let model = model.downgrade();
        glib::spawn_future_local(async move {
            let xdg = match dirs {
                Some(dirs) => gtk::gio::spawn_blocking(move || core_autostart::xdg_entries(&dirs))
                    .await
                    .unwrap_or_default(),
                None => Vec::new(),
            };
            if let Some(model) = model.upgrade() {
                if shown.turn.get() == started {
                    shown.show(&model, &xdg);
                }
            }
        });
    }

    /// Shows what starts, as `model` and the XDG autostart entries `xdg` say.
    fn show(&self, model: &Model, xdg: &[XdgEntry]) {
        let writable = model.status().is_some_and(|status| status.is_active());
        self.add.set_sensitive(writable);
        let mut main: Vec<Line> = Vec::new();
        let mut more: Vec<Line> = Vec::new();
        for (index, item) in model.list(ENTRIES).iter().enumerate() {
            if let Ok(entry) = Entry::parse(item) {
                main.push(our_line(index, &entry, writable));
            }
        }
        let runs = runners(model, systemd()).any();
        let hidden = model.list(HIDDEN);
        for entry in xdg.iter().filter(|entry| entry.starts != Starts::NotAnApp) {
            let line = xdg_line(entry, runs, &hidden, writable);
            if starts_here(entry) {
                main.push(line);
            } else {
                more.push(line);
            }
        }
        let hand = model
            .status()
            .zip(model.locations())
            .map(|(status, locations)| {
                core_autostart::hand_written(&status.config().directives, locations.directory())
            })
            .unwrap_or_default();
        main.extend(hand.iter().map(hand_line));
        let mut opens = self.opens.borrow_mut();
        opens.0 = fill(&self.list, main, model);
        opens.1 = fill(&self.more_list, more, model);
        self.list.set_visible(!opens.0.is_empty());
        list::set_hint(
            &self.empty,
            opens
                .0
                .is_empty()
                .then(|| tr(msg!("Nothing starts with your session yet"))),
        );
        self.more.set_visible(!opens.1.is_empty());
        self.more.set_label(Some(&trf(
            msg!("Not started in this session ({count})"),
            &[("count", &opens.1.len())],
        )));
    }
}

/// Whether an XDG autostart entry belongs in the main list: it starts in this session (or would,
/// once something runs the entries), or Sway Things switched it off.
fn starts_here(entry: &XdgEntry) -> bool {
    entry.starts == Starts::Yes || entry.place == Place::Ours
}

/// What a line of the list shows and does.
struct Line {
    /// Its icon.
    icon: Icon,
    /// Its title: a name, or a command line.
    title: String,
    /// Whether the title is a command line, shown in a monospace font.
    command: bool,
    /// Where it comes from, in words, on one line below the title.
    subtitle: String,
    /// A problem, as the tooltip of a warning sign.
    problem: Option<String>,
    /// The control at its end.
    control: Control,
    /// What its details dialog shows.
    details: Details,
}

/// The icon of a line.
enum Icon {
    /// An app's icon (a generic one without).
    App(Option<String>),
    /// A command line.
    Command,
}

/// The control at the end of a line.
enum Control {
    /// A switch, on or off, that stores what `toggle` makes of the stored lists.
    Switch {
        /// Whether it is on.
        on: bool,
        /// Whether it can be switched.
        sensitive: bool,
        /// Stores the switch's new state.
        toggle: Toggle,
    },
    /// A lock: it can't be changed here, for the reason given as its tooltip.
    Lock(String),
    /// Nothing.
    None,
}

/// Puts `lines` into `list`, replacing what it had, and returns what a click on each opens.
fn fill(list: &gtk::ListBox, lines: Vec<Line>, model: &Model) -> Vec<Open> {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let mut opens = Vec::new();
    for line in lines {
        let (row, open) = build(line, model);
        list.append(&row);
        opens.push(open);
    }
    opens
}

/// The row of `line`, and what a click on it opens.
fn build(line: Line, model: &Model) -> (gtk::ListBoxRow, Open) {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.add_css_class("st-autostart-row");
    content.append(&icon_of(&line.icon));
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(Some(&line.title));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("st-row-title");
    if line.command {
        title.add_css_class("monospace");
        title.set_tooltip_text(Some(&line.title));
    }
    texts.append(&title);
    let subtitle = gtk::Label::new(Some(&line.subtitle));
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
    subtitle.add_css_class(style::DIM);
    subtitle.add_css_class("st-autostart-subtitle");
    texts.append(&subtitle);
    content.append(&texts);
    let mut said = vec![line.subtitle.clone()];
    if let Some(problem) = &line.problem {
        let sign = gtk::Image::from_icon_name("dialog-warning-symbolic");
        sign.add_css_class("st-autostart-warning");
        sign.set_valign(gtk::Align::Center);
        sign.set_tooltip_text(Some(problem));
        content.append(&sign);
        said.push(problem.clone());
    }
    match line.control {
        Control::Switch {
            on,
            sensitive,
            toggle,
        } => {
            let switch = gtk::Switch::new();
            switch.set_active(on);
            switch.set_valign(gtk::Align::Center);
            switch.set_sensitive(sensitive);
            switch.update_property(&[gtk::accessible::Property::Label(&trf(
                msg!("Start {name} at login"),
                &[("name", &line.title)],
            ))]);
            switch.connect_state_set(glib::clone!(
                #[weak]
                model,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, on| {
                    toggle(&model, on);
                    glib::Propagation::Proceed
                }
            ));
            content.append(&switch);
        }
        Control::Lock(reason) => {
            let lock = gtk::Image::from_icon_name("changes-prevent-symbolic");
            lock.add_css_class("st-autostart-lock");
            lock.add_css_class(style::DIM);
            lock.set_valign(gtk::Align::Center);
            lock.set_tooltip_text(Some(&reason));
            content.append(&lock);
            said.push(reason);
        }
        Control::None => {}
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    row.set_activatable(true);
    row.add_css_class("st-autostart-item");
    row.set_tooltip_text(Some(tr(msg!("Show the details"))));
    row.update_property(&[
        gtk::accessible::Property::Label(&line.title),
        gtk::accessible::Property::Description(&said.join(". ")),
    ]);
    let details = Rc::new(line.details);
    let open: Open = Rc::new(move |from, model| dialogs::open_details(from, model, &details));
    (row, open)
}

/// The image of a line's icon: an app's icon as it is, a command as a terminal sign on a tile.
fn icon_of(icon: &Icon) -> gtk::Widget {
    match icon {
        Icon::App(name) => {
            let image = gtk::Image::new();
            image.set_pixel_size(32);
            image.set_valign(gtk::Align::Center);
            image.set_accessible_role(gtk::AccessibleRole::Presentation);
            show_icon(&image, name.as_deref());
            image.upcast()
        }
        Icon::Command => command_tile(),
    }
}

/// Stores the list `key` as `edit` changes it.
pub(super) fn change(model: &Model, key: &'static str, edit: impl FnOnce(&mut Vec<String>)) {
    let old = model.list(key);
    let mut items = old.clone();
    edit(&mut items);
    if items != old {
        model.set(key, None, Value::List(items));
    }
}

/// The line of Sway Things' entry at `index`.
fn our_line(index: usize, entry: &Entry, writable: bool) -> Line {
    let toggle: Toggle = Rc::new(move |model, on| {
        change(model, ENTRIES, |items| {
            if let Some(item) = items.get_mut(index) {
                if let Ok(mut entry) = Entry::parse(item) {
                    entry.on = on;
                    *item = entry.text();
                }
            }
        });
    });
    let control = Control::Switch {
        on: entry.on,
        sensitive: writable,
        toggle,
    };
    let status = tr(if entry.on {
        msg!("Starts at login")
    } else {
        msg!("Switched off")
    })
    .to_owned();
    match &entry.item {
        Item::App(id) => {
            let installed = app(id);
            let title = installed
                .as_ref()
                .map_or_else(|| id.clone(), |app| app.name.clone());
            let problem = installed
                .is_none()
                .then(|| tr(msg!("Not installed: it doesn't start")).to_owned());
            Line {
                icon: Icon::App(installed.as_ref().and_then(|app| app.icon.clone())),
                subtitle: tr(msg!("Added here")).to_owned(),
                problem: problem.clone(),
                control,
                details: Details {
                    title: title.clone(),
                    icon: installed.as_ref().and_then(|app| app.icon.clone()),
                    status: problem.unwrap_or(status),
                    command: None,
                    from: tr(msg!("Added in Sway Things")).to_owned(),
                    file: None,
                    app: Some(id.clone()),
                    notes: Vec::new(),
                    editor: None,
                    ours: Some(index),
                },
                title,
                command: false,
            }
        }
        Item::Exec(line) => Line {
            icon: Icon::Command,
            title: line.clone(),
            command: true,
            subtitle: tr(msg!("Command added here")).to_owned(),
            problem: None,
            control,
            details: Details {
                title: line.clone(),
                icon: None,
                status,
                command: Some(line.clone()),
                from: tr(msg!("Added in Sway Things")).to_owned(),
                file: None,
                app: None,
                notes: vec![
                    tr(msg!(
                        "sway runs it with the shell at login, like an exec line of your config"
                    ))
                    .to_owned(),
                ],
                editor: None,
                ours: Some(index),
            },
        },
    }
}

/// The line of an `exec` line of the user's config: read only, its details with the editor.
fn hand_line(line: &HandExec) -> Line {
    let number = line.line.to_string();
    let subtitle = match (line.runner.is_some(), line.always) {
        (true, _) => trf(
            msg!("Your config, line {line}: starts the XDG autostart entries"),
            &[("line", &number)],
        ),
        (false, true) => trf(
            msg!("Your config, line {line}, and at every reload"),
            &[("line", &number)],
        ),
        (false, false) => trf(msg!("Your config, line {line}"), &[("line", &number)]),
    };
    let mut notes = Vec::new();
    if line.always {
        notes.push(
            tr(msg!(
                "exec_always runs it again at every reload of sway, too"
            ))
            .to_owned(),
        );
    }
    match line.runner {
        Some(Runner::Dex) => {
            notes.push(tr(msg!("It starts the XDG autostart entries, with dex")).to_owned());
        }
        Some(Runner::Systemd) => {
            notes.push(tr(msg!("It starts the XDG autostart entries, with systemd")).to_owned());
        }
        None => {}
    }
    notes.push(
        tr(msg!(
            "Sway Things doesn't change your config: edit the line there"
        ))
        .to_owned(),
    );
    Line {
        icon: Icon::Command,
        title: line.command.clone(),
        command: true,
        subtitle,
        problem: None,
        control: Control::Lock(tr(msg!("In your config: change it there")).to_owned()),
        details: Details {
            title: line.command.clone(),
            icon: None,
            status: tr(if line.always {
                msg!("Starts at login and at every reload")
            } else {
                msg!("Starts at login")
            })
            .to_owned(),
            command: Some(line.command.clone()),
            from: tr(msg!("Your sway config")).to_owned(),
            file: Some((line.file.clone(), Some(line.line))),
            app: None,
            notes,
            editor: Some((line.file.clone(), Some(line.line))),
            ours: None,
        },
    }
}

/// Whether and why an XDG autostart entry starts, in words.
fn starts_text(entry: &XdgEntry, runs: bool) -> String {
    match &entry.starts {
        Starts::Yes if runs => tr(msg!("Starts at login")).to_owned(),
        Starts::Yes => tr(msg!(
            "Doesn't start: nothing runs the XDG autostart entries in this session"
        ))
        .to_owned(),
        Starts::Hidden => match entry.place {
            Place::Ours => tr(msg!("Switched off here")).to_owned(),
            Place::User => tr(msg!("Switched off by your own file (Hidden=true)")).to_owned(),
            Place::System => tr(msg!("Switched off by the system (Hidden=true)")).to_owned(),
        },
        Starts::OnlyIn(desktops) => trf(
            msg!("Only for {desktops}"),
            &[("desktops", &desktops.join(", "))],
        ),
        Starts::NotIn(desktops) => trf(
            msg!("Not for {desktops}"),
            &[("desktops", &desktops.join(", "))],
        ),
        Starts::NotInstalled(program) => {
            trf(msg!("{program} isn't installed"), &[("program", program)])
        }
        Starts::NoCommand | Starts::NotAnApp => tr(msg!("It has no command to run")).to_owned(),
    }
}

/// The line of an XDG autostart entry: a switch for a system entry (switched off with Sway
/// Things' override), a lock for the user's own.
fn xdg_line(entry: &XdgEntry, runs: bool, hidden: &[String], writable: bool) -> Line {
    let status = starts_text(entry, runs);
    let started = starts_here(entry);
    let (from, subtitle) = match entry.place {
        Place::User if entry.system.is_some() => (
            tr(msg!("Your autostart folder, in place of the system's")).to_owned(),
            tr(msg!("Your autostart folder")).to_owned(),
        ),
        Place::User => (
            tr(msg!("Your autostart folder")).to_owned(),
            tr(msg!("Your autostart folder")).to_owned(),
        ),
        Place::Ours | Place::System => (
            tr(msg!("The system's autostart folder")).to_owned(),
            tr(msg!("System autostart")).to_owned(),
        ),
    };
    // Off the main list, the line says why it doesn't start.
    let subtitle = if started { subtitle } else { status.clone() };
    let problem = (entry.starts == Starts::Yes && !runs).then(|| status.clone());
    let mut notes = Vec::new();
    if entry.gnome_disabled && entry.starts == Starts::Yes {
        notes.push(
            tr(msg!(
                "Only GNOME reads its X-GNOME-Autostart-enabled=false: it starts here anyway"
            ))
            .to_owned(),
        );
    }
    let (control, editor) = match entry.place {
        Place::User => {
            notes.push(tr(msg!("Your own file: Sway Things doesn't change it")).to_owned());
            (
                Control::Lock(
                    tr(msg!("Your own file: change it in ~/.config/autostart")).to_owned(),
                ),
                Some((entry.path.clone(), None)),
            )
        }
        // Only what would start can be switched off; Sway Things' own override switched on.
        Place::Ours | Place::System if started => {
            notes.push(
                tr(msg!(
                    "The switch hides it with a file of Sway Things in ~/.config/autostart"
                ))
                .to_owned(),
            );
            let off = entry.place == Place::Ours || hidden.contains(&entry.id);
            let id = entry.id.clone();
            let toggle: Toggle = Rc::new(move |model, on| {
                change(model, HIDDEN, |ids| {
                    ids.retain(|known| *known != id);
                    if !on {
                        ids.push(id.clone());
                    }
                });
            });
            (
                Control::Switch {
                    on: !off,
                    sensitive: writable,
                    toggle,
                },
                None,
            )
        }
        Place::Ours | Place::System => (Control::None, None),
    };
    let file = Some((
        entry
            .system
            .clone()
            .filter(|_| entry.place == Place::Ours)
            .unwrap_or_else(|| entry.path.clone()),
        None,
    ));
    Line {
        icon: Icon::App(entry.icon.clone()),
        title: entry.name.clone(),
        command: false,
        subtitle,
        problem,
        control,
        details: Details {
            title: entry.name.clone(),
            icon: Some(entry.icon.clone().unwrap_or_default()).filter(|icon| !icon.is_empty()),
            status,
            command: entry.exec.clone(),
            from,
            file,
            app: None,
            notes,
            editor,
            ours: None,
        },
    }
}
