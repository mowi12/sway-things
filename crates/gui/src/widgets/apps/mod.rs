// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Apps & Autostart page's own parts, which [`PageView`](super::PageView) adds to the rows
//! built from the registry:
//!
//! - the terminal and the launcher as drop-downs of the installed ones, with what the user's
//!   config binds to their keys now and which binding wins;
//! - each default app as a drop-down of the installed apps that open its types, with their
//!   icons, what opens the type now and whether the choice made here wins; above them, a note
//!   while `XDG_CURRENT_DESKTOP` doesn't name sway, since then no app reads the choices;
//! - below the runner of the XDG autostart entries, what runs them in this session, with the
//!   offer to have Sway Things start them;
//! - the list of everything that starts with the session ([`AutostartList`]).
//!
//! Names, icons and command lines of desktop entries are someone else's data: they only ever
//! become label text, and an icon is looked up by name or read as an image file.

mod autostart;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::apps::autostart::{self as core_autostart, RUNNER, Runner, Runners, Systemd};
use sway_things_core::apps::launchers::{self, Binding};
use sway_things_core::apps::mime::{self, Kind, Source};
use sway_things_core::apps::{App, Dirs, installed};
use sway_things_core::bindings::{ClashKind, DEFAULT_MODE, Keymap, Origin, RowKind};
use sway_things_core::registry::{Registry, Setting, Value};

pub use autostart::AutostartList;

use super::SettingRow;
use super::setting_row::tilde;
use super::shortcuts::{GdkKeymap, clash_line, place};
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The label of the button that opens the user's config at a line.
const OPEN_IN_EDITOR: Msg = msg!("Open in Editor");

thread_local! {
    /// The XDG directories of this session, read once.
    static DIRS: Option<Dirs> = Dirs::from_env();
    /// The installed apps, read once.
    static APPS: Vec<App> = DIRS.with(|dirs| dirs.as_ref().map(installed::installed).unwrap_or_default());
    /// What systemd said about its autostart target, once it answered.
    static SYSTEMD: Cell<Option<Systemd>> = const { Cell::new(None) };
}

/// The XDG directories of this session.
fn dirs() -> Option<Dirs> {
    DIRS.with(Clone::clone)
}

/// The installed apps.
fn apps() -> Vec<App> {
    APPS.with(Clone::clone)
}

/// Whether `setting` gets a row of its own: not the lists of autostart entries, which the
/// [`AutostartList`] shows.
pub fn has_row(setting: &Setting) -> bool {
    ![core_autostart::ENTRIES, core_autostart::HIDDEN].contains(&setting.key())
}

/// The choices of a setting the page offers as a drop-down, each a value and its label: the
/// installed terminals, launchers and apps for each default. `None` for other settings.
pub fn choices(setting: &Setting) -> Option<Vec<(String, String)>> {
    let key = setting.key();
    let dirs = dirs()?;
    let keep = |label: Msg| (String::new(), tr(label).to_owned());
    if key == launchers::TERMINAL {
        let mut choices = vec![keep(msg!("As your config binds it"))];
        choices.extend(launchers::terminals(&dirs, &apps()));
        return Some(choices);
    }
    if key == launchers::LAUNCHER {
        let mut choices = vec![keep(msg!("As your config binds it"))];
        choices.extend(launchers::launchers(&dirs));
        return Some(choices);
    }
    let kind = Kind::of(key)?;
    let apps = apps();
    let candidates = kind.candidates(&apps);
    let mut choices = vec![keep(msg!("Not chosen here"))];
    for app in &candidates {
        // Two apps of one name are told apart by their ID.
        let twice = candidates
            .iter()
            .filter(|other| other.name == app.name)
            .count()
            > 1;
        let label = if twice {
            format!("{} ({})", app.name, app.id)
        } else {
            app.name.clone()
        };
        choices.push((app.id.clone(), label));
    }
    Some(choices)
}

/// Draws the items of a default app's drop-down with the apps' icons.
pub fn follow(row: &SettingRow) {
    if row
        .setting()
        .and_then(|setting| Kind::of(setting.key()))
        .is_none()
    {
        return;
    }
    let Some(dropdown) = row.dropdown() else {
        return;
    };
    let Some(choices) = row.setting().and_then(choices) else {
        return;
    };
    let apps = apps();
    let icons: HashMap<String, Option<String>> = choices
        .iter()
        .map(|(id, label)| {
            let icon = apps
                .iter()
                .find(|app| app.id == *id)
                .and_then(|app| app.icon.clone());
            (label.clone(), icon)
        })
        .collect();
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let image = gtk::Image::new();
        image.set_pixel_size(20);
        image.set_accessible_role(gtk::AccessibleRole::Presentation);
        content.append(&image);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        content.append(&label);
        item.set_child(Some(&content));
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(title) = item
            .item()
            .and_downcast::<gtk::StringObject>()
            .map(|object| object.string().to_string())
        else {
            return;
        };
        let Some(content) = item.child() else {
            return;
        };
        if let Some(image) = content.first_child().and_downcast::<gtk::Image>() {
            show_icon(&image, icons.get(&title).cloned().flatten().as_deref());
        }
        if let Some(label) = content.last_child().and_downcast::<gtk::Label>() {
            label.set_text(&title);
        }
    });
    dropdown.set_list_factory(Some(&factory));
}

/// Shows the icon of a desktop entry in `image`: a name of the icon theme, or an image file
/// given by its absolute path (small enough to be an icon); a generic icon otherwise.
pub(crate) fn show_icon(image: &gtk::Image, icon: Option<&str>) {
    const FALLBACK: &str = "application-x-executable";
    let Some(icon) = icon else {
        image.set_icon_name(Some(FALLBACK));
        return;
    };
    if icon.starts_with('/') {
        let path = Path::new(icon);
        let small = path
            .metadata()
            .is_ok_and(|meta| meta.is_file() && meta.len() <= 1024 * 1024);
        if small {
            image.set_from_file(Some(path));
        } else {
            image.set_icon_name(Some(FALLBACK));
        }
        return;
    }
    let theme = gtk::IconTheme::for_display(&image.display());
    image.set_icon_name(Some(if theme.has_icon(icon) { icon } else { FALLBACK }));
}

/// The widgets the page shows below `row`, each as a row of its own.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let Some(setting) = row.setting() else {
        return Vec::new();
    };
    if let Some(binding) = Binding::of(setting.key()) {
        return vec![binding_note(model, binding)];
    }
    if let Some(kind) = Kind::of(setting.key()) {
        return vec![default_note(model, kind)];
    }
    if setting.key() == RUNNER {
        return vec![runner_note(model)];
    }
    Vec::new()
}

/// A box of notes that is filled again whenever `model` changes, by `fill`.
fn following(model: &Model, fill: impl Fn(&Model, &gtk::Box) + 'static) -> gtk::Widget {
    let note = gtk::Box::new(gtk::Orientation::Vertical, 4);
    note.add_css_class("st-apps-note");
    let refill = glib::clone!(
        #[weak]
        note,
        move |model: &Model| {
            while let Some(child) = note.first_child() {
                note.remove(&child);
            }
            fill(model, &note);
            note.set_visible(note.first_child().is_some());
        }
    );
    refill(model);
    let handler = RefCell::new(Some(model.connect_changed(refill)));
    note.connect_destroy(glib::clone!(
        #[weak]
        model,
        move |_| {
            if let Some(handler) = handler.take() {
                model.disconnect(handler);
            }
        }
    ));
    note.upcast()
}

/// A dim, wrapping label.
fn note_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class(style::DIM);
    label
}

/// A line with a warning sign.
fn warning(text: &str) -> gtk::Box {
    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    icon.set_valign(gtk::Align::Start);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.add_css_class("st-apps-warning");
    line.append(&icon);
    line.append(&note_label(text));
    line
}

/// A flat button that opens `file` (at `line`) in the editor.
fn editor_button(model: &Model, file: &Path, line: Option<usize>) -> gtk::Button {
    let button = gtk::Button::with_label(tr(OPEN_IN_EDITOR));
    button.add_css_class("flat");
    button.set_halign(gtk::Align::Start);
    let file = file.to_owned();
    button.connect_clicked(glib::clone!(
        #[weak]
        model,
        move |button| {
            let reporter = model.clone();
            crate::editor::open(button, Some(&model), &file, line, move |error| {
                if let Some(error) = error {
                    reporter.emit_problem(error);
                }
            });
        }
    ));
    button
}

/// The text value of the setting `key` in effect.
fn text_of(model: &Model, key: &str) -> Option<String> {
    let setting = Registry::builtin().get(key)?;
    match model.view(setting, None)?.value {
        Value::Text(text) => Some(text),
        Value::Choice(id) => Some(id),
        _ => None,
    }
}

/// What the terminal's or the launcher's keys do now: with a choice made here, which bindings
/// of the user's it takes the place of or loses to; without one, what the user's config binds
/// to the keys, and its variable (`$term`, `$menu`).
fn binding_note(model: &Model, binding: Binding) -> gtk::Widget {
    following(model, move |model, note| {
        let keymap = GdkKeymap::of_default();
        let keymap = keymap.as_ref().map(|keymap| keymap as &dyn Keymap);
        let Some(listing) = model.bindings(keymap) else {
            return;
        };
        if let Some(row) = listing
            .rows()
            .iter()
            .find(|row| row.kind == (RowKind::App { key: binding.key }))
        {
            for clash in &row.clashes {
                // Taking the place of the user's binding is what choosing here means.
                if clash.kind == ClashKind::Replaces {
                    note.append(&note_label(&trf(
                        msg!("Takes the place of {command} at {place}"),
                        &[
                            ("command", &clash.command.as_deref().unwrap_or_default()),
                            ("place", &place(&clash.source)),
                        ],
                    )));
                    continue;
                }
                note.append(&clash_line(clash));
                if clash.source.origin == Origin::User && clash.source.after_ours {
                    note.append(&editor_button(
                        model,
                        &clash.source.file,
                        Some(clash.source.line),
                    ));
                }
            }
            return;
        }
        let keys = text_of(model, binding.keys_key).unwrap_or_else(|| binding.default_keys.into());
        let Ok(line) = launchers::line(&keys, "true") else {
            return;
        };
        for other in listing.users_of(&line, DEFAULT_MODE, keymap, None) {
            if other.source.origin != Origin::User {
                continue;
            }
            let command = other.line.command.clone().unwrap_or_default();
            let place = place(&other.source);
            if other.source.after_ours {
                note.append(&warning(&trf(
                    msg!(
                        "Your config binds these keys to {command} at {place}, read after Sway \
                         Things' file: that line keeps them, so change it there"
                    ),
                    &[("command", &command), ("place", &place)],
                )));
                note.append(&editor_button(
                    model,
                    &other.source.file,
                    Some(other.source.line),
                ));
            } else {
                note.append(&note_label(&trf(
                    msg!(
                        "Now your config binds these keys to {command} at {place}. A choice here \
                         takes its place"
                    ),
                    &[("command", &command), ("place", &place)],
                )));
            }
        }
        let variable = if binding.key == launchers::TERMINAL {
            "$term"
        } else {
            "$menu"
        };
        if let Some(value) = model.status().and_then(|status| {
            status
                .config()
                .directives
                .iter()
                .rev()
                .find(|directive| {
                    directive.is(&[], "set")
                        && directive.args.first().map(String::as_str) == Some(variable)
                })
                .map(|directive| directive.args[1..].join(" "))
        }) {
            note.append(&note_label(&trf(
                msg!("Your config sets {variable} to {value}"),
                &[("variable", &variable), ("value", &value)],
            )));
        }
    })
}

/// The name of the app of ID `id`, or the ID if it isn't installed.
fn app_name(id: &str) -> String {
    APPS.with(|apps| {
        apps.iter()
            .find(|app| app.id == id)
            .map_or_else(|| id.to_owned(), |app| app.name.clone())
    })
}

/// What opens the type of `kind` now and why, and whether the choice made here wins.
fn default_note(model: &Model, kind: Kind) -> gtk::Widget {
    following(model, move |model, note| {
        let Some(dirs) = dirs() else {
            return;
        };
        let apps = apps();
        let shown = mime::shown(&dirs, kind, &apps);
        let chosen = text_of(model, kind.key()).filter(|id| !id.is_empty());
        let now = shown.now.as_ref().map(|now| {
            let from = match &now.source {
                Source::List(path) => tilde(path),
                Source::Fallback => tr(msg!("the first app that opens it")).to_owned(),
            };
            (app_name(&now.id), from, now.source.clone())
        });
        let Some(id) = chosen else {
            note.append(&note_label(&match now {
                Some((app, from, _)) => trf(
                    msg!("Now {app}, from {from}"),
                    &[("app", &app), ("from", &from)],
                ),
                None => tr(msg!("No app opens it yet")).to_owned(),
            }));
            return;
        };
        match apps.iter().find(|app| app.id == id) {
            None => note.append(&warning(&trf(
                msg!("{id} isn't installed, so apps take another one"),
                &[("id", &id)],
            ))),
            Some(app) if !kind.offers(app) => note.append(&warning(&trf(
                msg!("{app} doesn't say it opens this, so apps take another one"),
                &[("app", &app.name)],
            ))),
            Some(_) => {
                // Another list read before Sway Things' own wins over the choice.
                if let Some((app, from, Source::List(path))) = now {
                    if !shown.ours_wins && dirs.is_sway() && path != mime::ours(&dirs) {
                        note.append(&warning(&trf(
                            msg!(
                                "Opens with {app} instead: {from} comes first and wins over the \
                                 choice here"
                            ),
                            &[("app", &app), ("from", &from)],
                        )));
                    }
                }
            }
        }
    })
}

/// The note above the default apps while `XDG_CURRENT_DESKTOP` doesn't name sway: no app reads
/// the choices then. Hidden otherwise.
pub fn desktop_note() -> gtk::Widget {
    let Some(dirs) = dirs() else {
        return gtk::Box::new(gtk::Orientation::Vertical, 0).upcast();
    };
    let note = gtk::Box::new(gtk::Orientation::Vertical, 4);
    note.add_css_class("st-apps-desktop");
    if !dirs.is_sway() {
        let desktop = if dirs.desktops.is_empty() {
            tr(msg!("XDG_CURRENT_DESKTOP isn't set")).to_owned()
        } else {
            trf(
                msg!("XDG_CURRENT_DESKTOP is {desktop}"),
                &[("desktop", &dirs.desktops.join(":"))],
            )
        };
        note.append(&warning(&trf(
            msg!(
                "These choices don't apply: {desktop}, so apps don't read sway-mimeapps.list. Set \
                 XDG_CURRENT_DESKTOP=sway before sway starts: in the line that starts it \
                 (XDG_CURRENT_DESKTOP=sway sway), or with DesktopNames=sway in the session file of \
                 your login manager"
            ),
            &[("desktop", &desktop)],
        )));
    }
    note.set_visible(note.first_child().is_some());
    note.upcast()
}

/// What runs the XDG autostart entries in this session, with the offer to have Sway Things
/// start them, and a button that starts them now.
fn runner_note(model: &Model) -> gtk::Widget {
    let note = following(model, |model, note| fill_runner(model, note, systemd()));
    ask_systemd(model);
    note
}

/// What systemd said about its autostart target, once it answered.
fn systemd() -> Option<Systemd> {
    SYSTEMD.with(Cell::get)
}

/// Asks systemd about its autostart target (once the page is built), then has the page look
/// again.
fn ask_systemd(model: &Model) {
    glib::spawn_future_local(glib::clone!(
        #[weak]
        model,
        async move {
            let answer = core_autostart::systemd().await;
            SYSTEMD.with(|systemd| systemd.set(Some(answer)));
            model.emit_by_name::<()>("changed", &[]);
        }
    ));
}

/// What runs the XDG autostart entries now, for `model`; `systemd` once known.
fn runners(model: &Model, systemd: Option<Systemd>) -> Runners {
    let lines = model
        .status()
        .zip(model.locations())
        .map(|(status, locations)| {
            core_autostart::hand_written(&status.config().directives, locations.directory())
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|line| line.runner.is_some())
        .collect();
    let ours = text_of(model, RUNNER).as_deref().and_then(Runner::from_id);
    Runners {
        systemd: systemd.unwrap_or_default(),
        lines,
        ours,
        dex_installed: dirs().is_some_and(|dirs| dirs.find_program("dex").is_some()),
    }
}

fn fill_runner(model: &Model, note: &gtk::Box, systemd: Option<Systemd>) {
    let runners = runners(model, systemd);
    let writable = model.status().is_some_and(|status| status.is_active());
    if runners.twice() {
        note.append(&warning(tr(msg!(
            "More than one thing starts the XDG autostart entries, so each may start twice. Keep \
             one: remove the line from your config, or choose Nothing here"
        ))));
    }
    if runners.stale() {
        note.append(&note_label(tr(msg!(
            "systemd's target still runs from before the change: the runner chosen here takes over \
             at the next login"
        ))));
    } else if systemd.is_some_and(|systemd| systemd.active) {
        note.append(&note_label(tr(msg!(
            "In this session systemd starts them: xdg-desktop-autostart.target is active"
        ))));
    }
    for line in &runners.lines {
        let place = format!("{}:{}", tilde(&line.file), line.line);
        note.append(&note_label(&trf(
            msg!("Your config starts them at {place}: {command}"),
            &[("place", &place), ("command", &line.command)],
        )));
        note.append(&editor_button(model, &line.file, Some(line.line)));
    }
    if let Some(ours) = runners.ours {
        let start = gtk::Button::with_label(tr(msg!("Start Them Now")));
        start.set_halign(gtk::Align::Start);
        start.set_tooltip_text(Some(tr(msg!(
            "Starts the XDG autostart entries now, as at the next login. One that runs already \
             may start a second time"
        ))));
        start.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |_| {
                if let Ok(line) = sway_things_core::command::SwayText::new(ours.line()) {
                    model.run_command(sway_things_core::command::SwayCmd::exec(line, false));
                }
            }
        ));
        note.append(&start);
        return;
    }
    if runners.any() || systemd.is_none() {
        return;
    }
    note.append(&warning(tr(msg!(
        "Nothing starts them in this session: sway doesn't start the XDG autostart entries itself"
    ))));
    let (label, runner) = match runners.offer() {
        Some(Runner::Systemd) => (msg!("Start Them with systemd"), Runner::Systemd),
        Some(Runner::Dex) => (msg!("Start Them with dex"), Runner::Dex),
        None => {
            note.append(&note_label(tr(msg!(
                "Install dex (or use a systemd user session) to have them started"
            ))));
            return;
        }
    };
    let offer = gtk::Button::with_label(tr(label));
    offer.add_css_class("suggested-action");
    offer.set_halign(gtk::Align::Start);
    offer.set_sensitive(writable);
    offer.connect_clicked(glib::clone!(
        #[weak]
        model,
        move |_| model.set(RUNNER, None, Value::Choice(runner.id().to_owned()))
    ));
    note.append(&offer);
}
