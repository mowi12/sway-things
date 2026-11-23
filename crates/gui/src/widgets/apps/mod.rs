// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Apps & Autostart page's own parts, which [`PageView`](super::PageView) adds to the rows
//! built from the registry:
//!
//! - each default app as a drop-down of the installed apps that open its type, with their icons.
//!   Without a choice it shows the app that opens the type now (and, in its open list, where that
//!   comes from); choosing that app again means no choice. Below the row only what goes wrong:
//!   the app chosen isn't installed, doesn't open the type, or another list wins over the choice.
//!   Above the rows, a note while `XDG_CURRENT_DESKTOP` doesn't name sway, since then no app
//!   reads the choices;
//! - the terminal and the launcher as drop-downs of the installed ones, the first item what the
//!   user's config binds to their keys now; their keys are buttons that take keys as they are
//!   pressed ([`capture`](super::capture)). Below the row only a binding that keeps the keys from
//!   the choice;
//! - below the runner of the XDG autostart entries, what runs them in this session, a line each,
//!   with the offer to have Sway Things start them;
//! - the list of everything that starts with the session ([`AutostartList`]), with its dialogs.
//!
//! Names, icons and command lines of desktop entries are someone else's data: they only ever
//! become label text, and an icon is looked up by name or read as an image file.

mod autostart;
mod dialogs;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::apps::autostart::{self as core_autostart, RUNNER, Runner, Runners, Systemd};
use sway_things_core::apps::launchers::{self, Binding};
use sway_things_core::apps::mime::{self, Kind, Source};
use sway_things_core::apps::{App, Dirs, installed};
use sway_things_core::bindings::{ClashKind, DEFAULT_MODE, Keymap, Listing, Origin, RowKind};
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

/// The icon of an app without one, or with one the theme doesn't have.
const FALLBACK_ICON: &str = "application-x-executable";

thread_local! {
    /// The XDG directories of this session, read once.
    static DIRS: Option<Dirs> = Dirs::from_env();
    /// The installed apps, read once.
    static APPS: Vec<App> = DIRS.with(|dirs| dirs.as_ref().map(installed::installed).unwrap_or_default());
    /// The icon of each program an installed app runs, by the program's file name, read once.
    static PROGRAM_ICONS: HashMap<String, String> = program_icons();
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

/// The icons of the programs the installed apps run, by file name (`foot`, `alacritty`).
fn program_icons() -> HashMap<String, String> {
    let Some(dirs) = dirs() else {
        return HashMap::new();
    };
    let mut icons = HashMap::new();
    APPS.with(|apps| {
        for app in apps {
            let Some(icon) = &app.icon else {
                continue;
            };
            let Ok(args) = app.entry.command(&app.path, &dirs.languages) else {
                continue;
            };
            if let Some(program) = args.first().and_then(|program| program.rsplit('/').next()) {
                icons
                    .entry(program.to_owned())
                    .or_insert_with(|| icon.clone());
            }
        }
    });
    icons
}

/// The icon of the app that runs the program of the command line `command`, if one is
/// installed.
fn command_icon(command: &str) -> Option<String> {
    let program = command.split_whitespace().next()?;
    let name = program.rsplit('/').next()?;
    PROGRAM_ICONS.with(|icons| icons.get(name).cloned())
}

/// Whether `setting` gets a row of its own: not the lists of autostart entries, which the
/// [`AutostartList`] shows.
pub fn has_row(setting: &Setting) -> bool {
    ![core_autostart::ENTRIES, core_autostart::HIDDEN].contains(&setting.key())
}

/// What an item of a drop-down of apps shows besides its label: an icon, and a line below the
/// label in the open list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Pick {
    /// The icon: a name of the icon theme or an image file; `None` for none.
    icon: Option<String>,
    /// The line below the label, in the open list.
    detail: Option<String>,
}

/// The choices of a drop-down of apps, each a value and its label, and what each shows besides.
type Picks = (Vec<(String, String)>, Vec<Pick>);

/// The choices of a setting the page offers as a drop-down, each a value and its label: the
/// installed terminals, launchers and apps for each default. `None` for other settings. Once
/// [`below`] has the row's widgets, the row follows the model, its first item named after what
/// is in effect without a choice.
pub fn choices(setting: &Setting) -> Option<Vec<(String, String)>> {
    let key = setting.key();
    if let Some(binding) = Binding::of(key) {
        return Some(binding_picks(None, binding).0);
    }
    let kind = Kind::of(key)?;
    Some(default_picks(None, kind)?.0)
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

/// The installed app of ID `id`, if any.
fn app(id: &str) -> Option<App> {
    APPS.with(|apps| apps.iter().find(|app| app.id == id).cloned())
}

/// The name of the app of ID `id`, or the ID if it isn't installed.
fn app_name(id: &str) -> String {
    app(id).map_or_else(|| id.to_owned(), |app| app.name)
}

/// The icon of the app of ID `id`, the generic one if it isn't installed or has none.
fn app_icon(id: &str) -> String {
    app(id)
        .and_then(|app| app.icon)
        .unwrap_or_else(|| FALLBACK_ICON.to_owned())
}

/// The choices of the default app of `kind`. The first is no choice, named after the app that
/// opens the type without one; that app isn't offered a second time, unless it is the choice
/// stored (as `model` says, if given).
fn default_picks(model: Option<&Model>, kind: Kind) -> Option<Picks> {
    let dirs = dirs()?;
    let apps = apps();
    let shown = mime::shown(&dirs, kind, &apps);
    let stored = model
        .and_then(|model| text_of(model, kind.key()))
        .filter(|id| !id.is_empty());
    let base = shown.without_ours;
    let base_id = base.as_ref().map(|base| base.id.clone());
    let mut first = match &base {
        Some(base) => (
            app_name(&base.id),
            Pick {
                icon: Some(app_icon(&base.id)),
                detail: Some(match &base.source {
                    Source::List(path) => {
                        trf(msg!("Default now, from {file}"), &[("file", &tilde(path))])
                    }
                    Source::Fallback => {
                        tr(msg!("Default now: the first app that opens it")).to_owned()
                    }
                }),
            },
        ),
        None => (tr(msg!("No app")).to_owned(), Pick::default()),
    };
    let candidates: Vec<&App> = kind
        .candidates(&apps)
        .into_iter()
        .filter(|app| Some(&app.id) != base_id.as_ref() || Some(&app.id) == stored.as_ref())
        .collect();
    let mut labels = Vec::new();
    let mut picks = Vec::new();
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
        labels.push((app.id.clone(), label));
        picks.push(Pick {
            icon: Some(app.icon.clone().unwrap_or_else(|| FALLBACK_ICON.to_owned())),
            detail: None,
        });
    }
    // A choice that isn't installed or doesn't open the type stays, to be seen.
    if let Some(id) = stored.filter(|id| !labels.iter().any(|(value, _)| value == id)) {
        picks.push(Pick {
            icon: Some(app_icon(&id)),
            detail: None,
        });
        labels.push((id.clone(), app_name(&id)));
    }
    if labels.iter().any(|(_, label)| *label == first.0) {
        first.0 = trf(msg!("{app} (system default)"), &[("app", &first.0)]);
    }
    let mut choices = vec![(String::new(), first.0)];
    choices.extend(labels);
    let mut all = vec![first.1];
    all.extend(picks);
    Some((choices, all))
}

/// The binding of the user's that a choice of `binding` takes the place of, with its command
/// and where it is; `None` without a choice or without such a binding.
fn replaced(listing: &Listing, binding: Binding) -> Option<(String, String)> {
    listing
        .rows()
        .iter()
        .find(|row| row.kind == (RowKind::App { key: binding.key }))?
        .clashes
        .iter()
        .find(|clash| clash.kind == ClashKind::Replaces)
        .map(|clash| {
            (
                clash.command.clone().unwrap_or_default(),
                place(&clash.source),
            )
        })
}

/// What the user's config binds to the keys of `binding`, as the command after `exec`, and
/// where: the binding a choice here takes the place of, or the one in effect without a choice.
fn configured(model: &Model, listing: &Listing, binding: Binding) -> Option<(String, String)> {
    let (command, place) = replaced(listing, binding).or_else(|| {
        let keys = text_of(model, binding.keys_key).unwrap_or_else(|| binding.default_keys.into());
        let line = launchers::line(&keys, "true").ok()?;
        let keymap = GdkKeymap::of_default();
        let keymap = keymap.as_ref().map(|keymap| keymap as &dyn Keymap);
        listing
            .users_of(&line, DEFAULT_MODE, keymap, None)
            .into_iter()
            .find(|other| other.source.origin == Origin::User)
            .map(|other| {
                (
                    other.line.command.clone().unwrap_or_default(),
                    place(&other.source),
                )
            })
    })?;
    let command = command.strip_prefix("exec ").unwrap_or(&command).trim();
    Some((command.to_owned(), place))
}

/// The choices of the terminal or the launcher of `binding`. The first is no choice, named after
/// what the user's config binds to the keys (as `model` says, if given); the same command isn't
/// offered a second time, unless it is the choice stored.
fn binding_picks(model: Option<&Model>, binding: Binding) -> Picks {
    let stored = model
        .and_then(|model| text_of(model, binding.key))
        .filter(|command| !command.trim().is_empty());
    let config = model.and_then(|model| {
        let keymap = GdkKeymap::of_default();
        let keymap = keymap.as_ref().map(|keymap| keymap as &dyn Keymap);
        let listing = model.bindings(keymap)?;
        configured(model, &listing, binding)
    });
    let found = match dirs() {
        Some(dirs) if binding.key == launchers::TERMINAL => launchers::terminals(&dirs, &apps()),
        Some(dirs) => launchers::launchers(&dirs),
        None => Vec::new(),
    };
    let icon =
        |command: &str| Some(command_icon(command).unwrap_or_else(|| FALLBACK_ICON.to_owned()));
    // The user's command goes by the name of a known one that is the same.
    let named = |command: &str| {
        found
            .iter()
            .find(|(known, _)| known == command)
            .map_or_else(|| command.to_owned(), |(_, name)| name.clone())
    };
    let mut first = match &config {
        Some((command, place)) => (
            named(command),
            Pick {
                icon: icon(command),
                detail: Some(trf(msg!("From your config, {place}"), &[("place", place)])),
            },
        ),
        None if model.is_some() => (tr(msg!("Nothing")).to_owned(), Pick::default()),
        None => (
            tr(msg!("As your config binds it")).to_owned(),
            Pick::default(),
        ),
    };
    let same = |command: &str| config.as_ref().is_some_and(|(theirs, _)| theirs == command);
    let mut labels = Vec::new();
    let mut picks = Vec::new();
    for (command, name) in found {
        if same(&command) && Some(&command) != stored.as_ref() {
            continue;
        }
        picks.push(Pick {
            icon: icon(&command),
            detail: None,
        });
        labels.push((command, name));
    }
    if let Some(command) =
        stored.filter(|command| !labels.iter().any(|(value, _)| value == command))
    {
        picks.push(Pick {
            icon: icon(&command),
            detail: None,
        });
        labels.push((command.clone(), command));
    }
    if labels.iter().any(|(_, label)| *label == first.0) {
        first.0 = trf(msg!("{name} (your config)"), &[("name", &first.0)]);
    }
    let mut choices = vec![(String::new(), first.0)];
    choices.extend(labels);
    let mut all = vec![first.1];
    all.extend(picks);
    (choices, all)
}

/// Readies the drop-down of a default app, the terminal or the launcher: its few items need no
/// search. [`below`] has it draw them with their icons.
pub fn follow(row: &SettingRow) {
    let ours = row.setting().is_some_and(|setting| {
        Kind::of(setting.key()).is_some() || Binding::of(setting.key()).is_some()
    });
    if let (true, Some(dropdown)) = (ours, row.dropdown()) {
        dropdown.set_enable_search(false);
        dropdown.add_css_class("st-app-dropdown");
    }
}

/// Shows the icon of a desktop entry in `image`: a name of the icon theme, or an image file
/// given by its absolute path (small enough to be an icon); a generic icon otherwise.
pub(crate) fn show_icon(image: &gtk::Image, icon: Option<&str>) {
    let Some(icon) = icon else {
        image.set_icon_name(Some(FALLBACK_ICON));
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
            image.set_icon_name(Some(FALLBACK_ICON));
        }
        return;
    }
    let theme = gtk::IconTheme::for_display(&image.display());
    image.set_icon_name(Some(if theme.has_icon(icon) {
        icon
    } else {
        FALLBACK_ICON
    }));
}

/// The sign of a command line: a terminal on a tile as large as an app's icon.
fn command_tile() -> gtk::Widget {
    let image = gtk::Image::from_icon_name("utilities-terminal-symbolic");
    image.set_pixel_size(18);
    image.set_accessible_role(gtk::AccessibleRole::Presentation);
    image.set_hexpand(true);
    image.set_vexpand(true);
    let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tile.add_css_class("st-autostart-command-icon");
    tile.set_size_request(32, 32);
    tile.set_hexpand(false);
    tile.set_valign(gtk::Align::Center);
    tile.append(&image);
    tile.upcast()
}

/// The factory of the items of a drop-down of apps, with the icons and lines of `picks` by
/// position: for the button (`listed` false) the icon and the label, for the open list the
/// line below the label too.
fn pick_factory(picks: &Rc<RefCell<Vec<Pick>>>, listed: bool) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        content.add_css_class("st-app-pick");
        let image = gtk::Image::new();
        image.set_pixel_size(if listed { 24 } else { 16 });
        image.set_valign(gtk::Align::Center);
        image.set_accessible_role(gtk::AccessibleRole::Presentation);
        content.append(&image);
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 1);
        texts.set_valign(gtk::Align::Center);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        if listed {
            label.set_max_width_chars(36);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
        } else {
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_max_width_chars(24);
        }
        texts.append(&label);
        let detail = gtk::Label::new(None);
        detail.set_xalign(0.0);
        detail.set_max_width_chars(36);
        detail.set_wrap(true);
        detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        detail.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
        detail.add_css_class(style::DIM);
        detail.add_css_class("st-app-pick-detail");
        texts.append(&detail);
        content.append(&texts);
        item.set_child(Some(&content));
    });
    let picks = Rc::clone(picks);
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let title = item
            .item()
            .and_downcast::<gtk::StringObject>()
            .map(|object| object.string().to_string())
            .unwrap_or_default();
        let pick = usize::try_from(item.position())
            .ok()
            .and_then(|index| picks.borrow().get(index).cloned())
            .unwrap_or_default();
        let Some(content) = item.child() else {
            return;
        };
        if let Some(image) = content.first_child().and_downcast::<gtk::Image>() {
            image.set_visible(pick.icon.is_some());
            show_icon(&image, pick.icon.as_deref());
        }
        let Some(texts) = content.last_child() else {
            return;
        };
        if let Some(label) = texts.first_child().and_downcast::<gtk::Label>() {
            label.set_text(&title);
        }
        if let Some(detail) = texts.last_child().and_downcast::<gtk::Label>() {
            let text = pick.detail.filter(|_| listed).unwrap_or_default();
            detail.set_visible(!text.is_empty());
            detail.set_text(&text);
        }
    });
    factory
}

/// Has the drop-down of `row` draw its items with the icons and lines of the picks returned,
/// which start empty.
fn picking(row: &SettingRow) -> Rc<RefCell<Vec<Pick>>> {
    let picks = Rc::new(RefCell::new(Vec::new()));
    if let Some(dropdown) = row.dropdown() {
        dropdown.set_factory(Some(&pick_factory(&picks, false)));
        dropdown.set_list_factory(Some(&pick_factory(&picks, true)));
    }
    picks
}

/// The widgets the page shows below `row`, each as a row of its own, shown only while it says
/// something. The drop-downs of apps follow the model from here.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let Some(setting) = row.setting() else {
        return Vec::new();
    };
    if let Some(binding) = Binding::of(setting.key()) {
        let picks = picking(row);
        let weak = row.downgrade();
        return vec![following(model, move |model, note| {
            let Some(row) = weak.upgrade() else {
                return;
            };
            let (choices, all) = binding_picks(Some(model), binding);
            picks.replace(all);
            row.set_choices(&choices);
            binding_note(model, binding, &row, note);
        })];
    }
    if let Some(kind) = Kind::of(setting.key()) {
        let picks = picking(row);
        let weak = row.downgrade();
        return vec![following(model, move |model, note| {
            let Some(row) = weak.upgrade() else {
                return;
            };
            if let Some((choices, all)) = default_picks(Some(model), kind) {
                picks.replace(all);
                row.set_choices(&choices);
            }
            default_note(model, kind, note);
        })];
    }
    if setting.key() == RUNNER {
        return vec![runner_note(model)];
    }
    Vec::new()
}

/// A box of notes that is filled again whenever `model` changes, by `fill`. The row of the list
/// it goes into shows only while the box says something.
fn following(model: &Model, fill: impl Fn(&Model, &gtk::Box) + 'static) -> gtk::Widget {
    let note = gtk::Box::new(gtk::Orientation::Vertical, 6);
    note.add_css_class("st-apps-note");
    let sync = |note: &gtk::Box| {
        if let Some(parent) = note.parent() {
            parent.set_visible(note.is_visible());
        }
    };
    note.connect_parent_notify(sync);
    note.connect_visible_notify(sync);
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
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.add_css_class("st-apps-warning");
    line.append(&icon);
    let label = note_label(text);
    label.set_hexpand(true);
    line.append(&label);
    line
}

/// A button that opens `file` (at `line`) in the editor.
fn editor_button(model: &Model, file: &Path, line: Option<usize>) -> gtk::Button {
    let button = gtk::Button::with_label(tr(OPEN_IN_EDITOR));
    button.set_valign(gtk::Align::Center);
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

/// A line of `text`, dim, with `button` at its end.
fn line_with(text: &str, button: &impl IsA<gtk::Widget>) -> gtk::Box {
    let label = note_label(text);
    label.set_hexpand(true);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.append(&label);
    line.append(button);
    line
}

/// What keeps the keys of the terminal or the launcher from opening the choice made here: a
/// binding of the user's read after Sway Things' file (with _Open in Editor_), or another clash.
/// Taking the place of the user's binding is what choosing here means: the drop-down's tooltip
/// says whose.
fn binding_note(model: &Model, binding: Binding, row: &SettingRow, note: &gtk::Box) {
    let keymap = GdkKeymap::of_default();
    let keymap = keymap.as_ref().map(|keymap| keymap as &dyn Keymap);
    let Some(listing) = model.bindings(keymap) else {
        return;
    };
    if let Some(dropdown) = row.dropdown() {
        let tooltip = replaced(&listing, binding).map(|(command, place)| {
            trf(
                msg!("Takes the place of {command} at {place}"),
                &[("command", &command), ("place", &place)],
            )
        });
        dropdown.set_tooltip_text(tooltip.as_deref());
    }
    let Some(ours) = listing
        .rows()
        .iter()
        .find(|row| row.kind == (RowKind::App { key: binding.key }))
    else {
        return;
    };
    for clash in ours
        .clashes
        .iter()
        .filter(|clash| clash.kind != ClashKind::Replaces)
    {
        let line = clash_line(clash);
        if clash.source.origin == Origin::User && clash.source.after_ours {
            let button = editor_button(model, &clash.source.file, Some(clash.source.line));
            if let Some(label) = line.last_child() {
                label.set_hexpand(true);
            }
            line.append(&button);
        }
        note.append(&line);
    }
}

/// What goes wrong with the default app chosen for `kind`: it isn't installed, doesn't open the
/// type, or another list read before Sway Things' own wins over it with another app.
fn default_note(model: &Model, kind: Kind, note: &gtk::Box) {
    let Some(dirs) = dirs() else {
        return;
    };
    let Some(id) = text_of(model, kind.key()).filter(|id| !id.is_empty()) else {
        return;
    };
    let apps = apps();
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
            let shown = mime::shown(&dirs, kind, &apps);
            let Some(now) = shown.now else {
                return;
            };
            let Source::List(path) = &now.source else {
                return;
            };
            // Another list read before Sway Things' own wins over the choice, unless it names
            // the same app.
            if !shown.ours_wins && dirs.is_sway() && *path != mime::ours(&dirs) && now.id != id {
                note.append(&warning(&trf(
                    msg!(
                        "Opens with {app} instead: {from} comes first and wins over the choice \
                         here"
                    ),
                    &[("app", &app_name(&now.id)), ("from", &tilde(path))],
                )));
            }
        }
    }
}

/// The note above the default apps while `XDG_CURRENT_DESKTOP` doesn't name sway: no app reads
/// the choices then. Hidden otherwise.
pub fn desktop_note() -> gtk::Widget {
    let note = gtk::Box::new(gtk::Orientation::Vertical, 4);
    note.add_css_class("st-apps-desktop");
    if let Some(dirs) = dirs().filter(|dirs| !dirs.is_sway()) {
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
                "Apps don't read these choices: {desktop}. Start sway with \
                 XDG_CURRENT_DESKTOP=sway: XDG_CURRENT_DESKTOP=sway sway, or DesktopNames=sway \
                 in the session file of your login manager"
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

/// Fills the note of the runner: a line for each thing that runs the entries, a warning for
/// none or for two.
fn fill_runner(model: &Model, note: &gtk::Box, systemd: Option<Systemd>) {
    let runners = runners(model, systemd);
    let writable = model.status().is_some_and(|status| status.is_active());
    if runners.twice() {
        note.append(&warning(tr(msg!(
            "More than one thing starts them, so each may start twice. Keep one: remove the line \
             from your config, or choose “Your session or config”"
        ))));
    }
    if runners.stale() {
        note.append(&note_label(tr(msg!(
            "systemd still runs them as before the change: the choice here takes over at the \
             next login"
        ))));
    } else if systemd.is_some_and(|systemd| systemd.active) && runners.ours.is_none() {
        note.append(&note_label(tr(msg!("In this session systemd starts them"))));
    }
    for line in &runners.lines {
        let place = format!("{}:{}", tilde(&line.file), line.line);
        let text = match line.runner {
            Some(Runner::Systemd) => trf(
                msg!("Your config starts them with systemd, at {place}"),
                &[("place", &place)],
            ),
            _ => trf(
                msg!("Your config starts them with dex, at {place}"),
                &[("place", &place)],
            ),
        };
        note.append(&line_with(
            &text,
            &editor_button(model, &line.file, Some(line.line)),
        ));
    }
    if let Some(ours) = runners.ours {
        let start = gtk::Button::with_label(tr(msg!("Start Them Now")));
        start.set_valign(gtk::Align::Center);
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
        note.append(&line_with(
            tr(msg!("Sway Things starts them at each login")),
            &start,
        ));
        return;
    }
    if runners.any() || systemd.is_none() {
        return;
    }
    let offer = match runners.offer() {
        Some(Runner::Systemd) => Some((msg!("Start Them with systemd"), Runner::Systemd)),
        Some(Runner::Dex) => Some((msg!("Start Them with dex"), Runner::Dex)),
        None => None,
    };
    let line = warning(tr(match offer {
        Some(_) => msg!("Nothing starts them in this session"),
        None => msg!(
            "Nothing starts them in this session. Install dex, or use a systemd user session, to \
             have them started"
        ),
    }));
    if let Some((label, runner)) = offer {
        let button = gtk::Button::with_label(tr(label));
        button.add_css_class("suggested-action");
        button.set_valign(gtk::Align::Center);
        button.set_sensitive(writable);
        button.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |_| model.set(RUNNER, None, Value::Choice(runner.id().to_owned()))
        ));
        line.append(&button);
    }
    note.append(&line);
}
