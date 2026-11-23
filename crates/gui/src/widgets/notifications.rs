// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Notifications page's own parts, which [`PageView`](super::PageView) adds to the rows
//! built from the registry:
//!
//! - **Do Not Disturb** at the top: the switch, the daemon that shows notifications and what
//!   starts it, _Send Test Notification_, and what keeps the page's settings from reaching the
//!   daemon, each with what to do and a copy button: no daemon (with an offer to start one),
//!   two daemons started, mako's config without our include, swaync without our files. It reads
//!   the daemon again on each change it reports, while the page is on screen every three
//!   seconds, and after every change of the settings.
//! - Below the rows of swaync's widgets and buttons, a list to switch each on or off and move it
//!   up or down.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::notifications::files::{Files, Inclusion};
use sway_things_core::notifications::starts::{self, Origin, StartLine};
use sway_things_core::notifications::{Daemon, Notifications, Server, mako, swaync};
use sway_things_core::registry::Value;
use sway_things_core::registry::notifications::{self as keys, swaync as swaync_keys};

use super::SettingRow;
use super::keyboard::plain_row;
use super::list::{list_row, section_list};
use super::live::follow_changes;
use crate::i18n::{msg, tr, trf};
use crate::model::Model;

/// How often the page reads the daemon while it is on screen: for changes that send no signal
/// (mako 1.9's modes) and a daemon that was started or ended meanwhile.
const LOOK_AGAIN: Duration = Duration::from_secs(3);
/// The changes the daemon reports within this time are read together.
const THROTTLE: Duration = Duration::from_millis(100);

/// What the import button says once it imported.
const IMPORTED: crate::i18n::Msg = msg!("Imported");

/// What the panel last saw.
#[derive(Debug, Clone, Default, PartialEq)]
struct Seen {
    server: Option<Server>,
    dnd: Option<bool>,
    /// The config the running daemon was given with `-c`.
    config: Option<std::path::PathBuf>,
}

/// The Do Not Disturb section's content.
struct Panel {
    model: Model,
    root: gtk::Box,
    dnd: gtk::Switch,
    dnd_subtitle: gtk::Label,
    daemon: gtk::Label,
    test: gtk::Button,
    notes: gtk::Box,
    seen: RefCell<Seen>,
    /// Set while the switch is moved by what the daemon reports, not by the user.
    updating: Cell<bool>,
    /// While on screen: the timer that looks again and the task that follows the bus.
    watching: RefCell<Option<(glib::SourceId, glib::JoinHandle<()>)>>,
    /// The window the page is in, and our handler of its `suspended`, while mapped.
    window: RefCell<Option<(gtk::Window, glib::SignalHandlerId)>>,
    /// Set while the daemon is read, so that reads don't overlap.
    reading: Cell<bool>,
    /// Set when a read was asked for during one: another follows it.
    again: Cell<bool>,
    on_daemon: Box<dyn Fn(Option<Daemon>)>,
}

/// The Do Not Disturb section's content; `on_daemon` is told which daemon the page is about
/// whenever that may have changed: the one that runs, else the one chosen to start, else
/// `None`.
pub fn panel(model: &Model, on_daemon: impl Fn(Option<Daemon>) + 'static) -> gtk::Widget {
    let dnd = gtk::Switch::new();
    dnd.update_property(&[gtk::accessible::Property::Label(tr(msg!("Do Not Disturb")))]);
    let dnd_row = plain_row(
        tr(msg!("Do Not Disturb")),
        Some(tr(msg!(
            "Notifications wait silently until you switch it off"
        ))),
        &dnd,
    );
    let dnd_subtitle = subtitle_of(&dnd_row);
    let test = gtk::Button::with_label(tr(msg!("Send Test Notification")));
    let daemon_row = plain_row(tr(msg!("Notification daemon")), Some(""), &test);
    let daemon = subtitle_of(&daemon_row);
    let list = section_list(tr(msg!("Do Not Disturb")));
    for child in [&dnd_row, &daemon_row] {
        list.append(&list_row(child));
    }
    let notes = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.append(&list);
    root.append(&notes);
    let panel = Rc::new(Panel {
        model: model.clone(),
        root: root.clone(),
        dnd,
        dnd_subtitle,
        daemon,
        test,
        notes,
        seen: RefCell::default(),
        updating: Cell::new(false),
        watching: RefCell::default(),
        window: RefCell::default(),
        reading: Cell::new(false),
        again: Cell::new(false),
        on_daemon: Box::new(on_daemon),
    });
    connect(&panel);
    // The widget keeps the panel: the signals only hold it weakly.
    let keep = RefCell::new(Some(Rc::clone(&panel)));
    root.connect_destroy(move |_| {
        keep.take();
    });
    root.upcast()
}

/// The subtitle label of a row from [`plain_row`].
fn subtitle_of(row: &gtk::Box) -> gtk::Label {
    row.first_child()
        .and_then(|texts| texts.last_child())
        .and_then(|label| label.downcast::<gtk::Label>().ok())
        .unwrap_or_default()
}

/// Wires the switch, the button, the model, the bus and the timer to the panel.
fn connect(panel: &Rc<Panel>) {
    panel.dnd.connect_state_set(glib::clone!(
        #[weak]
        panel,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, on| {
            if panel.updating.get() {
                return glib::Propagation::Proceed;
            }
            glib::spawn_future_local(switch_dnd(panel, on));
            glib::Propagation::Stop
        }
    ));
    panel.test.connect_clicked(glib::clone!(
        #[weak]
        panel,
        move |_| {
            glib::spawn_future_local(send_test(panel));
        }
    ));
    let weak = Rc::downgrade(panel);
    panel.model.connect_changed(move |_| {
        if let Some(panel) = weak.upgrade() {
            glib::spawn_future_local(refresh(panel));
        }
    });
    // While on screen: the daemon's own signals, and a look every few seconds. A window on a
    // hidden workspace stays mapped, but is suspended.
    let weak = Rc::downgrade(panel);
    panel.root.connect_map(move |root| {
        let Some(panel) = weak.upgrade() else {
            return;
        };
        if let Some(window) = root.root().and_downcast::<gtk::Window>() {
            let weak = Rc::downgrade(&panel);
            let handler = window.connect_suspended_notify(move |_| {
                if let Some(panel) = weak.upgrade() {
                    update_watch(&panel);
                }
            });
            if let Some((old, handler)) = panel.window.replace(Some((window, handler))) {
                old.disconnect(handler);
            }
        }
        update_watch(&panel);
    });
    let weak = Rc::downgrade(panel);
    panel.root.connect_unmap(move |_| {
        let Some(panel) = weak.upgrade() else {
            return;
        };
        if let Some((window, handler)) = panel.window.take() {
            window.disconnect(handler);
        }
        update_watch(&panel);
    });
    glib::spawn_future_local(refresh(Rc::clone(panel)));
}

/// Follows the daemon while the page is on screen: mapped, in a window that isn't suspended on
/// a hidden workspace. Coming on screen, it reads the daemon at once.
fn update_watch(panel: &Rc<Panel>) {
    let shown = panel.root.is_mapped()
        && panel
            .root
            .root()
            .and_downcast::<gtk::Window>()
            .is_none_or(|window| !window.is_suspended());
    if shown == panel.watching.borrow().is_some() {
        return;
    }
    if let Some((timer, watch)) = panel.watching.take() {
        timer.remove();
        watch.abort();
        return;
    }
    glib::spawn_future_local(refresh(Rc::clone(panel)));
    let weak = Rc::downgrade(panel);
    let timer = glib::timeout_add_local(LOOK_AGAIN, move || {
        let Some(panel) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        glib::spawn_future_local(refresh(panel));
        glib::ControlFlow::Continue
    });
    let watch = glib::spawn_future_local(follow_bus(Rc::downgrade(panel)));
    panel.watching.replace(Some((timer, watch)));
}

/// Reads the daemon again on each change it reports, at most every [`THROTTLE`].
async fn follow_bus(panel: std::rc::Weak<Panel>) {
    let Some(bus) = panel.upgrade().and_then(|panel| panel.model.daemon_bus()) else {
        return;
    };
    let Ok(changes) = bus.changes().await else {
        return;
    };
    follow_changes(changes, THROTTLE, || {
        let panel = panel.upgrade();
        async move {
            let Some(panel) = panel else {
                return false;
            };
            refresh(panel).await;
            true
        }
    })
    .await;
}

/// Switches Do Not Disturb as the user asked, then shows what the daemon reports.
async fn switch_dnd(panel: Rc<Panel>, on: bool) {
    // Asked again: the daemon may have restarted with another owner since the last look.
    let bus = panel.model.daemon_bus();
    let server = match &bus {
        Some(bus) => bus.server().await.ok().flatten(),
        None => None,
    };
    if let (Some(bus), Some(server)) = (bus, server) {
        if let Err(error) = bus.set_dnd(&server, on).await {
            panel.dnd_subtitle.set_text(&error.text().translated());
        }
    }
    refresh(panel).await;
}

/// Sends the test notification.
async fn send_test(panel: Rc<Panel>) {
    let server = panel.seen.borrow().server.clone();
    if let (Some(bus), Some(server)) = (panel.model.daemon_bus(), server) {
        let sent = bus
            .notify(
                &server,
                tr(msg!("This is how notifications look")),
                tr(msg!(
                    "Change them on the Notifications page of Sway Things."
                )),
            )
            .await;
        if let Err(error) = sent {
            panel.daemon.set_text(&error.text().translated());
        }
    }
}

/// Reads the daemon and Do Not Disturb, and shows them with the notes that apply.
/// Reads the daemon and shows what it says. One read at a time: asked during one, it reads
/// once more when that ends, so the last answer shown is never older than the last ask.
async fn refresh(panel: Rc<Panel>) {
    if panel.reading.replace(true) {
        panel.again.set(true);
        return;
    }
    // Cleared however the read ends, also when the task that follows the bus is aborted in it.
    let _reading = Reading(Rc::clone(&panel));
    loop {
        panel.again.set(false);
        read_and_show(&panel).await;
        if !panel.again.get() {
            break;
        }
    }
}

/// Clears [`Panel::reading`] when dropped.
struct Reading(Rc<Panel>);

impl Drop for Reading {
    fn drop(&mut self) {
        self.0.reading.set(false);
    }
}

async fn read_and_show(panel: &Rc<Panel>) {
    let (server, dnd) = match panel.model.daemon_bus() {
        Some(bus) => read(&bus).await,
        None => (None, None),
    };
    let config = server
        .as_ref()
        .and_then(|server| server.pid)
        .and_then(starts::config_arg);
    let seen = Seen {
        server,
        dnd,
        config,
    };
    let chosen = chosen(&panel.model);
    let lines = start_lines(&panel.model, chosen);
    show(panel, &seen, &lines, chosen);
    let about = seen.server.as_ref().map(|server| server.daemon).or(chosen);
    panel.seen.replace(seen);
    (panel.on_daemon)(about);
}

async fn read(bus: &Notifications) -> (Option<Server>, Option<bool>) {
    let Ok(Some(server)) = bus.server().await else {
        return (None, None);
    };
    let dnd = bus.dnd(&server).await.ok();
    (Some(server), dnd)
}

/// The lines of the user's sway config (and ours) that start a daemon.
fn start_lines(model: &Model, chosen: Option<Daemon>) -> Vec<StartLine> {
    match (model.status(), model.locations()) {
        (Some(status), Some(locations)) => {
            starts::start_lines(status.config(), locations.directory(), chosen)
        }
        _ => Vec::new(),
    }
}

/// The daemon chosen to start with sway.
fn chosen(model: &Model) -> Option<Daemon> {
    let setting = sway_things_core::registry::Registry::builtin().get(keys::START)?;
    match model.view(setting, None)?.value {
        Value::Choice(program) => Daemon::from_program(&program),
        _ => None,
    }
}

/// Shows `seen` in the switch, the daemon row and the notes.
fn show(panel: &Panel, seen: &Seen, lines: &[StartLine], chosen: Option<Daemon>) {
    panel.updating.set(true);
    panel.dnd.set_active(seen.dnd == Some(true));
    panel.dnd.set_state(seen.dnd == Some(true));
    panel.updating.set(false);
    panel.dnd.set_sensitive(seen.dnd.is_some());
    panel.test.set_sensitive(seen.server.is_some());
    let files = panel
        .model
        .daemon_files()
        .map(|files| mako_files(files, seen));
    panel
        .dnd_subtitle
        .set_text(&dnd_subtitle(seen, files.as_ref()));
    panel.daemon.set_text(&daemon_text(seen, lines));
    while let Some(child) = panel.notes.first_child() {
        panel.notes.remove(&child);
    }
    for note in notes(&panel.model, seen, lines, chosen, files.as_ref()) {
        panel.notes.append(&note);
    }
}

fn dnd_subtitle(seen: &Seen, files: Option<&Files>) -> String {
    match &seen.server {
        None => tr(msg!("No notification daemon runs")).to_owned(),
        Some(server) if seen.dnd.is_none() => trf(
            msg!("{daemon} has no Do Not Disturb that Sway Things can switch"),
            &[("daemon", &server.name)],
        ),
        Some(server) if server.daemon == Daemon::Mako && !mako_hides(files) => tr(msg!(
            "mako only hides notifications with the include line below, or a \
             [mode=do-not-disturb] section of your own"
        ))
        .to_owned(),
        Some(_) => tr(msg!("Notifications wait silently until you switch it off")).to_owned(),
    }
}

/// `files` with the config the running mako was given with `-c`, which it reads instead.
fn mako_files(files: Files, seen: &Seen) -> Files {
    match (&seen.server, &seen.config) {
        (Some(server), Some(config)) if server.daemon == Daemon::Mako => {
            files.with_mako_config(config.clone())
        }
        _ => files,
    }
}

/// Whether mako hides notifications in the mode do-not-disturb: through our file, or a section
/// of the user's own.
fn mako_hides(files: Option<&Files>) -> bool {
    files.is_none_or(|files| {
        matches!(
            files.mako_inclusion(),
            Inclusion::Included | Inclusion::NoConfig
        ) || std::fs::read_to_string(&files.mako_config)
            .is_ok_and(|text| mako::has_dnd_section(&text))
    })
}

fn daemon_text(seen: &Seen, lines: &[StartLine]) -> String {
    let Some(server) = &seen.server else {
        return tr(msg!("None runs")).to_owned();
    };
    let name = match server.daemon {
        Daemon::Other => server.name.clone(),
        daemon => daemon.title().to_owned(),
    };
    let origin = match starts::origin(server.daemon, server.pid, lines) {
        Origin::Line(line) if line.ours => tr(msg!("started by Sway Things")).to_owned(),
        Origin::Line(line) => trf(
            msg!("started by {file}:{line}"),
            &[("file", &line.file.display()), ("line", &line.line)],
        ),
        Origin::Unit(unit) => trf(
            msg!("started as the systemd unit {unit}"),
            &[("unit", &unit)],
        ),
        Origin::Elsewhere => tr(msg!("started by D-Bus activation or a program")).to_owned(),
    };
    trf(
        msg!("{daemon}, {origin}"),
        &[("daemon", &name), ("origin", &origin)],
    )
}

/// What keeps the page's settings from reaching the daemon, each as a note.
fn notes(
    model: &Model,
    seen: &Seen,
    lines: &[StartLine],
    chosen: Option<Daemon>,
    files: Option<&Files>,
) -> Vec<gtk::Widget> {
    let mut notes = Vec::new();
    let user_lines: Vec<&StartLine> = lines.iter().filter(|line| !line.ours).collect();
    if seen.server.is_none() && user_lines.is_empty() && chosen.is_none() {
        notes.push(offer(model));
    }
    let mut started: Vec<Daemon> = user_lines.iter().map(|line| line.daemon).collect();
    started.extend(chosen);
    started.sort_by_key(|daemon| daemon.program());
    started.dedup();
    if started.len() > 1 {
        let names: Vec<&str> = started.iter().map(|daemon| daemon.title()).collect();
        let mut text = trf(
            msg!(
                "{daemons} are all started, but only the first to start shows notifications. Remove the lines of all but one:"
            ),
            &[("daemons", &names.join(", "))],
        );
        for line in &user_lines {
            let _ = write!(
                text,
                "
{}:{}: {}",
                line.file.display(),
                line.line,
                line.text
            );
        }
        if chosen.is_some() {
            let _ = write!(text, "\n{}", tr(msg!("Start with sway, below")));
        }
        notes.push(note(&text, None));
    }
    let Some(files) = files else {
        return notes;
    };
    let running = seen.server.as_ref().map(|server| server.daemon);
    let about = running.or(chosen);
    if about == Some(Daemon::Mako) {
        notes.extend(mako_note(files));
    }
    if running == Some(Daemon::Swaync) && !swaync_uses_ours(seen, files) {
        let command = format!(
            "swaync -c {} -s {}",
            files.shown(&files.swaync_config),
            files.shown(&files.swaync_style)
        );
        notes.push(note(
            tr(msg!("swaync runs with its own config. For these settings to reach it, start it like this instead, or choose it under Start with sway after removing the line that starts it:")),
            Some(&command),
        ));
    }
    if (about == Some(Daemon::Swaync) || chosen == Some(Daemon::Swaync))
        && files.swaync_user_config.is_file()
        && !stores_swaync(model)
    {
        notes.push(import_note(model, files));
    }
    if let Some(server) = seen.server.as_ref().filter(|server| {
        server.daemon == Daemon::Swaync
            && swaync::Gtk::of_version(&server.version) == swaync::Gtk::Three
    }) {
        notes.push(note(
            &trf(
                msg!("swaync {version} is built with GTK 3: Frosted glass and Rounded corners need swaync 0.12 or newer, and only the colors reach it."),
                &[("version", &server.version)],
            ),
            None,
        ));
    }
    notes
}

/// Whether anything of swaync's own settings is stored: then the user started from their
/// config or set it up here.
fn stores_swaync(model: &Model) -> bool {
    sway_things_core::registry::Registry::builtin()
        .iter()
        .filter(|setting| setting.key().starts_with("notifications.swaync."))
        .any(|setting| model.stored_in(setting, None).is_some())
}

/// What keeps mako from reading our file: the include line missing, after a section or written
/// as mako refuses it, or no config that may be created. Says where our file lands behind a
/// symbolic link.
fn mako_note(files: &Files) -> Option<gtk::Widget> {
    let inclusion = files.mako_inclusion();
    let shown = files.shown(&files.mako_config);
    let mut text = match inclusion {
        Inclusion::Included => return None,
        Inclusion::NoConfig if files.may_create_mako_config() => return None,
        Inclusion::NoConfig => trf(
            msg!(
                "mako reads {file}, which doesn't exist, and Sway Things doesn't create it while mako runs with a config of its own or a system-wide one exists. For these settings and Do Not Disturb to reach mako, create it with this line, then run makoctl reload:"
            ),
            &[("file", &shown)],
        ),
        Inclusion::Malformed => trf(
            msg!(
                "{file} names the file of Sway Things in a way mako refuses, and with it the whole config. Write the line like this, after your own options and before the first [section], then run makoctl reload:"
            ),
            &[("file", &shown)],
        ),
        Inclusion::Missing | Inclusion::InSection => trf(
            msg!(
                "For these settings and Do Not Disturb to reach mako, add this line to {file}, after your own options and before the first [section], then run makoctl reload:"
            ),
            &[("file", &shown)],
        ),
    };
    if let Some(target) = files.mako_ours_target() {
        let _ = write!(
            text,
            "\n{}",
            trf(
                msg!(
                    "The file of Sway Things lands in {target}, where the folder of mako's config points."
                ),
                &[("target", &target.display())],
            )
        );
    }
    Some(note(&text, Some(&files.mako_include_line())))
}

/// Whether the swaync that runs was given our config with `-c`, as its command line says.
fn swaync_uses_ours(seen: &Seen, files: &Files) -> bool {
    let canonical = |path: &std::path::Path| std::fs::canonicalize(path).ok();
    seen.config.as_deref().is_some_and(|config| {
        config == files.swaync_config
            || canonical(config)
                .is_some_and(|config| canonical(&files.swaync_config) == Some(config))
    })
}

/// The offer to start a daemon with sway: mako if it is installed, else swaync, else dunst.
fn offer(model: &Model) -> gtk::Widget {
    let installed = Daemon::KNOWN.into_iter().find(|daemon| daemon.installed());
    let Some(daemon) = installed else {
        return note(
            tr(msg!(
                "No notification daemon runs, and none is installed. Install mako, for instance, then start it here."
            )),
            None,
        );
    };
    let text = tr(msg!(
        "No notification daemon runs, and your sway config starts none."
    ));
    let button = gtk::Button::with_label(&trf(
        msg!("Start {daemon} with Sway"),
        &[("daemon", &daemon.title())],
    ));
    button.add_css_class("suggested-action");
    button.set_halign(gtk::Align::Start);
    button.connect_clicked(glib::clone!(
        #[weak]
        model,
        move |_| model.set(
            keys::START,
            None,
            Value::Choice(daemon.program().to_owned())
        )
    ));
    let note = note(text, None);
    if let Some(note) = note.downcast_ref::<gtk::Box>() {
        note.append(&button);
    }
    note
}

/// _Import My swaync Config_: the user's config as a starting point.
fn import_note(model: &Model, files: &Files) -> gtk::Widget {
    let text = trf(
        msg!(
            "Start from your own swaync config: the options of {file} that the page has go to its settings, the others are kept as they are. Your file isn't changed."
        ),
        &[("file", &files.shown(&files.swaync_user_config))],
    );
    let button = gtk::Button::with_label(tr(msg!("Import My swaync Config")));
    button.set_halign(gtk::Align::Start);
    let path = files.swaync_user_config.clone();
    button.connect_clicked(glib::clone!(
        #[weak]
        model,
        move |button| {
            let imported = std::fs::read_to_string(&path)
                .map_err(|error| error.to_string())
                .and_then(|text| {
                    swaync::import(&text, &swaync::default_config())
                        .map_err(|error| error.to_string())
                });
            match imported {
                Ok(imported) => {
                    for (key, value) in imported.values {
                        model.set(key, None, value);
                    }
                    model.set(swaync_keys::OTHER, None, Value::Text(imported.other));
                    button.set_label(tr(IMPORTED));
                }
                Err(error) => button.set_label(&error),
            }
        }
    ));
    let note = note(&text, None);
    if let Some(note) = note.downcast_ref::<gtk::Box>() {
        note.append(&button);
    }
    note
}

/// A note: `text`, and `copy` in monospace with a button that copies it.
fn note(text: &str, copy: Option<&str>) -> gtk::Widget {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
    root.add_css_class("st-note");
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    root.append(&label);
    if let Some(copy) = copy {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let code = gtk::Label::new(Some(copy));
        code.add_css_class("monospace");
        code.set_xalign(0.0);
        code.set_hexpand(true);
        code.set_wrap(true);
        code.set_wrap_mode(gtk::pango::WrapMode::Char);
        code.set_selectable(true);
        let button = gtk::Button::with_label(tr(msg!("Copy")));
        let text = copy.to_owned();
        button.connect_clicked(move |button| {
            button.clipboard().set_text(&text);
            button.set_label(tr(msg!("Copied")));
        });
        line.append(&code);
        line.append(&button);
        root.append(&line);
    }
    root.upcast()
}

/// The widgets the page shows below `row`: for swaync's widgets and buttons, a list to switch
/// each on or off and move it.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let Some(setting) = row.setting() else {
        return Vec::new();
    };
    let offered: &[&str] = match setting.key() {
        swaync_keys::WIDGETS => keys::WIDGETS,
        swaync_keys::BUTTONS => keys::BUTTONS,
        _ => return Vec::new(),
    };
    vec![checklist(model, row, offered)]
}

/// A list of items to switch on or off and order: those of the row's value first, in its
/// order, then the others offered. It follows the model.
fn checklist(model: &Model, row: &SettingRow, offered: &'static [&'static str]) -> gtk::Widget {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("st-checklist");
    let fill = glib::clone!(
        #[weak]
        row,
        #[weak]
        list,
        move || {
            let chosen: Vec<String> = match row.value() {
                Some(Value::List(items)) => items,
                _ => Vec::new(),
            };
            let mut items: Vec<(String, bool)> =
                chosen.iter().map(|item| (item.clone(), true)).collect();
            for item in offered {
                if !chosen.iter().any(|chosen| chosen == item) {
                    items.push(((*item).to_owned(), false));
                }
            }
            // Where the focus is: the item and which of its controls.
            let focused = list.focus_child().and_then(|line| {
                let index = line.widget_name().parse::<usize>().ok()?;
                let focus = line.focus_child()?;
                let mut control = 0;
                let mut child = line.first_child();
                while let Some(widget) = child {
                    if widget == focus {
                        return Some((index, control));
                    }
                    control += 1;
                    child = widget.next_sibling();
                }
                None
            });
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            for (index, (item, on)) in items.iter().enumerate() {
                let line = item_row(&row, &items, index, item, *on);
                line.set_widget_name(&index.to_string());
                list.append(&line);
            }
            // The focus stays where it was, so the keyboard can go on.
            if let Some((index, control)) = focused {
                let mut line = list.first_child();
                for _ in 0..index {
                    line = line.and_then(|line| line.next_sibling());
                }
                let mut widget = line.and_then(|line| line.first_child());
                for _ in 0..control {
                    widget = widget.and_then(|widget| widget.next_sibling());
                }
                if let Some(widget) = widget {
                    widget.grab_focus();
                }
            }
        }
    );
    fill();
    model.connect_changed(move |_| fill());
    list.upcast()
}

/// One item of a checklist: a check button with its name, and buttons to move it.
fn item_row(
    row: &SettingRow,
    items: &[(String, bool)],
    index: usize,
    item: &str,
    on: bool,
) -> gtk::Box {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.add_css_class("st-checklist-item");
    let check = gtk::CheckButton::with_label(&item_label(item));
    check.set_active(on);
    check.set_hexpand(true);
    line.append(&check);
    let send = {
        let row = row.clone();
        move |items: Vec<(String, bool)>| {
            let value = items
                .into_iter()
                .filter(|(_, on)| *on)
                .map(|(item, _)| item)
                .collect();
            row.change(Value::List(value));
        }
    };
    let toggled = items.to_vec();
    let send_toggle = send.clone();
    check.connect_toggled(move |check| {
        let mut items = toggled.clone();
        items[index].1 = check.is_active();
        send_toggle(items);
    });
    for (icon, label, step) in [
        ("go-up-symbolic", msg!("Move up"), -1_isize),
        ("go-down-symbolic", msg!("Move down"), 1),
    ] {
        let button = gtk::Button::from_icon_name(icon);
        button.add_css_class("flat");
        let name = format!("{} {}", tr(label), item_label(item));
        button.set_tooltip_text(Some(&name));
        button.update_property(&[gtk::accessible::Property::Label(&name)]);
        let target = index
            .checked_add_signed(step)
            .filter(|target| *target < items.len());
        button.set_sensitive(on && target.is_some_and(|target| items[target].1));
        let moved = items.to_vec();
        let send = send.clone();
        button.connect_clicked(move |_| {
            if let Some(target) = target {
                let mut items = moved.clone();
                items.swap(index, target);
                send(items);
            }
        });
        line.append(&button);
    }
    line
}

/// The name to show for a widget or button of swaync's panel.
fn item_label(item: &str) -> String {
    let (kind, id) = item.split_once('#').unwrap_or((item, ""));
    let name = match kind {
        "title" => tr(msg!("Title and Clear All")),
        "dnd" => tr(msg!("Do Not Disturb")),
        "notifications" => tr(msg!("Notifications")),
        "mpris" => tr(msg!("Media player")),
        "volume" => tr(msg!("Volume")),
        "backlight" => tr(msg!("Brightness")),
        "buttons-grid" => tr(msg!("Buttons")),
        "inhibitors" => tr(msg!("Apps that hold notifications back")),
        "wifi" => tr(msg!("Wi-Fi")),
        "bluetooth" => tr(msg!("Bluetooth")),
        "sound" => tr(msg!("Sound")),
        "microphone" => tr(msg!("Microphone")),
        "settings" => tr(msg!("Sway Things")),
        other => other,
    };
    if id.is_empty() {
        name.to_owned()
    } else {
        format!("{name} ({id})")
    }
}
