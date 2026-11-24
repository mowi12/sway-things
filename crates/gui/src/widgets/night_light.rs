// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The night light's own parts on Brightness & Night Light, which
//! [`PageView`](super::PageView) adds to the rows built from the registry:
//!
//! - only the rows of the schedule chosen show (the location from sunset to sunrise, the times
//!   with fixed times, no day temperature while it is always on), and _Tool_ only while both
//!   wlsunset and gammastep are installed; the latitude and longitude are number fields;
//! - below the switch: without a tool, the command that installs one; while the user's config
//!   starts a night light, or one of the user's own runs, that line with _Open config in editor_
//!   and how to hand over, the rows locked; else whether it runs, or why not;
//! - below the location, the time zone and its place.
//!
//! The page also has the tool follow the settings once they stood still for a moment, so
//! dragging a temperature restarts it only once.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::night_light::{
    self, ENABLED, FROM, LATITUDE, LOCATION, LONGITUDE, Problem, SCHEDULE, Schedule, Settings,
    TEMPERATURE_DAY, TO, TOOL, Tool,
};
use sway_things_core::registry::{ConfigFile, Page, Registry, Setting};

use super::SettingRow;
use super::brightness::{command_notice, install_line};
use super::notice::{self, Tone};
use super::setting_row::tilde;
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;

/// How long the settings stand still before the tool follows them.
const SETTLE: Duration = Duration::from_millis(800);

/// The name of the note below the location, to find its row.
const ZONE_NOTE: &str = "st-night-light-zone";

// Messages used inside closures, which xgettext can't look into: they are marked out here.
/// That the tool runs.
const RUNS: Msg = msg!("Runs with {tool}.");
/// That the tool doesn't run yet.
const STARTS: Msg = msg!("Starts once you stop changing the settings.");
/// The place of the time zone.
const ZONE_PLACE: Msg = msg!("Time zone {zone}: {latitude}° {north}, {longitude}° {east}.");

/// The generated files the page writes to: the hook that starts the night light.
pub fn files(page: Page) -> Vec<ConfigFile> {
    if page == Page::BrightnessNightLight {
        vec![ConfigFile::Hooks]
    } else {
        Vec::new()
    }
}

/// Makes the latitude and longitude number fields: their sliders are of little use.
pub fn follow(row: &SettingRow) {
    let Some(setting) = row.setting() else {
        return;
    };
    if ![LATITUDE, LONGITUDE].contains(&setting.key()) {
        return;
    }
    for control in row.controls() {
        if control.is::<gtk::Scale>() {
            control.set_visible(false);
        }
    }
}

/// What goes below a row of the night light.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    match row.setting().map(Setting::key) {
        Some(ENABLED) => vec![Ui::widget(model)],
        Some(LOCATION) => vec![zone_note()],
        _ => Vec::new(),
    }
}

/// The settings of the night light the model shows.
fn settings_of(model: &Model) -> Settings {
    Settings::read(|key| {
        Registry::builtin()
            .get(key)
            .and_then(|setting| model.view(setting, None))
            .map(|view| view.value)
    })
}

/// A line of the user's config that starts a night light.
#[derive(Clone, PartialEq, Eq)]
struct StartLine {
    file: std::path::PathBuf,
    number: usize,
    text: String,
}

/// What the notes show, to rebuild them only when it changes.
#[derive(Clone, PartialEq, Eq)]
enum Shown {
    NoTool,
    External(StartLine),
    Foreign(night_light::Foreign),
    Runs(Tool),
    Problem(String),
    Starts,
    Nothing,
}

/// The notes below the switch and what they follow.
struct Ui {
    model: glib::WeakRef<Model>,
    notes: gtk::Box,
    shown: RefCell<Option<Shown>>,
    /// The settings the tool was last made to follow.
    synced: RefCell<Option<(Settings, bool)>>,
    /// The change waiting for the settings to stand still.
    pending: RefCell<Option<glib::SourceId>>,
    /// Why the tool didn't start at the last change.
    problem: RefCell<Option<Problem>>,
    /// Whether the tool is being made to follow the settings now.
    syncing: Cell<bool>,
}

impl Ui {
    /// The notes below the switch, following `model`.
    fn widget(model: &Model) -> gtk::Widget {
        let notes = gtk::Box::new(gtk::Orientation::Vertical, 6);
        notes.add_css_class("st-capture-note");
        let ui = Rc::new(Self {
            model: model.downgrade(),
            notes: notes.clone(),
            shown: RefCell::new(None),
            synced: RefCell::new(None),
            pending: RefCell::new(None),
            problem: RefCell::new(None),
            syncing: Cell::new(false),
        });
        let weak = Rc::downgrade(&ui);
        model.connect_changed(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.refresh();
                ui.follow_settings();
            }
        });
        // The rows come after the switch: once they are in the list.
        let weak = Rc::downgrade(&ui);
        notes.connect_parent_notify(move |_| {
            let weak = weak.clone();
            glib::idle_add_local_once(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.refresh();
                    ui.follow_settings();
                }
            });
        });
        // A night light started or ended elsewhere shows while the page is on screen.
        let weak: Weak<Self> = Rc::downgrade(&ui);
        glib::timeout_add_seconds_local(3, move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if ui.notes.is_mapped() {
                ui.refresh();
            }
            glib::ControlFlow::Continue
        });
        // The closures hold the notes' state weakly; the widget keeps it until it goes.
        notes.connect_destroy(move |_| {
            let _ = &ui;
        });
        notes.upcast()
    }

    /// The list the switch and the other rows are in.
    fn list(&self) -> Option<gtk::ListBox> {
        self.notes.parent()?.parent().and_downcast::<gtk::ListBox>()
    }

    /// Shows the rows of the schedule chosen, locks them while something else runs a night
    /// light, and rebuilds the notes if what they say changed.
    fn refresh(&self) {
        let Some(model) = self.model.upgrade() else {
            return;
        };
        let settings = settings_of(&model);
        let installed = night_light::installed();
        let external = model.status().and_then(|status| {
            night_light::own_lines(status.config())
                .first()
                .map(|directive| StartLine {
                    file: directive.file.to_path_buf(),
                    number: directive.line,
                    text: directive.text.clone(),
                })
        });
        let foreign = night_light::foreign();
        let managed = external.is_some() || foreign.is_some();
        if let Some(list) = self.list() {
            show_rows(
                &list,
                &settings,
                installed.len(),
                managed || installed.is_empty(),
            );
        }
        let shown = if installed.is_empty() {
            Shown::NoTool
        } else if let Some(line) = external {
            Shown::External(line)
        } else if let Some(foreign) = foreign {
            Shown::Foreign(foreign)
        } else if !settings.on {
            Shown::Nothing
        } else if let Some(tool) = night_light::running() {
            Shown::Runs(tool)
        } else if let Some(problem) = self.problem(&settings, &installed) {
            Shown::Problem(problem)
        } else {
            Shown::Starts
        };
        if self.shown.borrow().as_ref() == Some(&shown) {
            return;
        }
        while let Some(child) = self.notes.first_child() {
            self.notes.remove(&child);
        }
        if let Some(widget) = note_for(&model, &shown) {
            self.notes.append(&widget);
        }
        self.notes.set_visible(self.notes.first_child().is_some());
        if let Some(row) = self.notes.parent() {
            row.set_visible(self.notes.is_visible());
        }
        self.shown.replace(Some(shown));
    }

    /// Why the night light that is on can't run: the settings as they are, else what the last
    /// start said.
    fn problem(&self, settings: &Settings, installed: &[Tool]) -> Option<String> {
        let tool = settings.tool_of(installed)?;
        let config = night_light::config_path().unwrap_or_default();
        if let Err(error) = night_light::plan(settings, tool, night_light::zone_location, &config) {
            return Some(error.text().translated());
        }
        if self.syncing.get() || self.pending.borrow().is_some() {
            return None;
        }
        self.problem
            .borrow()
            .as_ref()
            .map(|problem| problem.text().translated())
    }

    /// Has the tool follow the settings once they stood still, in the session only.
    fn follow_settings(self: &Rc<Self>) {
        let Some(model) = self.model.upgrade() else {
            return;
        };
        let Some(status) = model.status() else {
            return;
        };
        let external = !night_light::own_lines(status.config()).is_empty();
        let settings = settings_of(&model);
        let wanted = (settings.clone(), external);
        if self.synced.borrow().as_ref() == Some(&wanted) {
            return;
        }
        let first = self.synced.borrow().is_none();
        self.synced.replace(Some(wanted));
        if let Some(source) = self.pending.borrow_mut().take() {
            source.remove();
        }
        // At the start, only what isn't as it should be: the hook started the tool at login.
        if first && !settings.on && night_light::running().is_none() {
            return;
        }
        let weak = Rc::downgrade(self);
        let source =
            glib::timeout_add_local_once(if first { Duration::ZERO } else { SETTLE }, move || {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                ui.pending.borrow_mut().take();
                ui.syncing.set(true);
                let weak = Rc::downgrade(&ui);
                glib::spawn_future_local(async move {
                    let result = gtk::gio::spawn_blocking(move || {
                        night_light::sync_with(|| settings, external)
                    })
                    .await;
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    ui.syncing.set(false);
                    let problem = match result {
                        Ok(Ok(synced)) => synced.problem,
                        // Without a session or while the hook holds the lock, the hook does it at
                        // the next start of sway.
                        Ok(Err(_)) | Err(_) => None,
                    };
                    ui.problem.replace(problem);
                    ui.shown.replace(None);
                    ui.refresh();
                });
            });
        self.pending.replace(Some(source));
        self.shown.replace(None);
        self.refresh();
    }
}

/// Shows and locks the rows of the night light in `list`: by the schedule, the tools
/// installed, and `locked` while something else runs a night light or no tool is installed.
fn show_rows(list: &gtk::ListBox, settings: &Settings, tools: usize, locked: bool) {
    let mut child = list.first_child();
    while let Some(item) = child {
        child = item.next_sibling();
        let Some(inner) = item.first_child() else {
            continue;
        };
        if inner.widget_name() == ZONE_NOTE {
            let visible = settings.schedule == Schedule::Sunset && settings.custom.is_none();
            item.set_visible(visible);
            if let Some(label) = inner.downcast_ref::<gtk::Label>() {
                show_zone(label);
            }
            continue;
        }
        let Some(row) = inner.downcast_ref::<SettingRow>() else {
            continue;
        };
        let Some(key) = row.setting().map(Setting::key) else {
            continue;
        };
        let visible = match key {
            TOOL => tools > 1,
            LOCATION => settings.schedule == Schedule::Sunset,
            LATITUDE | LONGITUDE => {
                settings.schedule == Schedule::Sunset && settings.custom.is_some()
            }
            FROM | TO => settings.schedule == Schedule::Times,
            TEMPERATURE_DAY => settings.schedule != Schedule::Always,
            ENABLED | SCHEDULE => true,
            _ => continue,
        };
        item.set_visible(visible);
        // The switch stays usable while it is on, so the night light can be switched off.
        let usable = !locked || (key == ENABLED && settings.on);
        item.set_sensitive(usable);
    }
}

/// The note below the location: the time zone and its place.
fn zone_note() -> gtk::Widget {
    let label = gtk::Label::new(None);
    label.set_widget_name(ZONE_NOTE);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.add_css_class(style::DIM);
    label.add_css_class("st-capture-note");
    show_zone(&label);
    label.upcast()
}

/// Shows the time zone and its place in `label`, or why there is none.
fn show_zone(label: &gtk::Label) {
    let text = match night_light::zone_location() {
        Ok(place) => {
            let zone = night_light::location::zone().unwrap_or_default();
            let (north, south) = (tr(msg!("N")), tr(msg!("S")));
            let (east, west) = (tr(msg!("E")), tr(msg!("W")));
            trf(
                ZONE_PLACE,
                &[
                    ("zone", &zone),
                    ("latitude", &format!("{:.2}", place.latitude.abs())),
                    ("north", &if place.latitude < 0.0 { south } else { north }),
                    ("longitude", &format!("{:.2}", place.longitude.abs())),
                    ("east", &if place.longitude < 0.0 { west } else { east }),
                ],
            )
        }
        Err(zone) => night_light::PlanError::NoLocation(zone).text().translated(),
    };
    if label.text() != text {
        label.set_text(&text);
    }
}

/// The note for what the notes show.
fn note_for(model: &Model, shown: &Shown) -> Option<gtk::Widget> {
    Some(match shown {
        Shown::NoTool => command_notice(
            Tone::Warning,
            tr(msg!(
                "The night light needs wlsunset or gammastep. Install one of them, wlsunset for \
                 example:"
            )),
            &install_line(&[Tool::Wlsunset.name()]),
        )
        .upcast(),
        Shown::External(line) => external_notice(model, line).upcast(),
        Shown::Foreign(foreign) => {
            let text = notice::text(&trf(
                msg!(
                    "{program} runs already (process {pid}), started outside Sway Things, by a \
                     systemd unit or by hand. Only one night light can change the colors, so the \
                     settings here wait until it ends; then turn the night light on here."
                ),
                &[("program", &foreign.program), ("pid", &foreign.pid)],
            ));
            notice::notice(Tone::Warning, &text, None).upcast()
        }
        Shown::Runs(tool) => dim(&trf(RUNS, &[("tool", &tool.name())])),
        Shown::Problem(problem) => {
            let label = dim(problem);
            label.add_css_class("error");
            label
        }
        Shown::Starts => dim(tr(STARTS)),
        Shown::Nothing => return None,
    })
}

/// A dim line that wraps.
fn dim(text: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class(style::DIM);
    label.upcast()
}

/// The notice about a line of the user's config that starts a night light, with a button that
/// opens it in the editor.
fn external_notice(model: &Model, line: &StartLine) -> gtk::Box {
    let text = notice::text(&trf(
        // TRANSLATORS: {place} is a file and line such as ~/.config/sway/config:12, {line} the
        // line as written.
        msg!(
            "Your config starts a night light itself ({place}: {line}). Only one night light can \
             change the colors, so the settings here are off. To set the night light here, \
             remove that line, then turn the night light on here."
        ),
        &[
            ("place", &format!("{}:{}", tilde(&line.file), line.number)),
            ("line", &line.text),
        ],
    ));
    let open = gtk::Button::with_label(tr(msg!("Open config in editor")));
    let (file, number) = (line.file.clone(), line.number);
    let weak_model = model.downgrade();
    let weak_text = text.downgrade();
    open.connect_clicked(move |button| {
        let Some(model) = weak_model.upgrade() else {
            return;
        };
        let weak_text = weak_text.clone();
        crate::editor::open(button, Some(&model), &file, Some(number), move |message| {
            if let (Some(message), Some(text)) = (message, weak_text.upgrade()) {
                text.set_text(message);
            }
        });
    });
    notice::notice(Tone::Warning, &text, Some(&open))
}
