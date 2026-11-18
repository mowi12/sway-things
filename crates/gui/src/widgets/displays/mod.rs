// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Displays page: the arrangement of the outputs to drag into place, and the rows of the
//! output picked on it.
//!
//! [`Arrangement`] is the first section: the [`Canvas`] with the outputs that are on, a hint how
//! to use it, and a warning with a fix when outputs overlap or don't touch, so the pointer can't
//! reach every one. [`OutputPicker`] is the first row of the "Display" section: it picks the
//! output the rows below are for, among all outputs (also those that are off or not connected,
//! which aren't on the canvas) and every output at once. The canvas and the picker follow each
//! other. [`follow`] keeps all of it current with the model, also when a monitor is plugged in or
//! out.
//!
//! Output names, makes and models are external strings: they are only shown as plain text.

mod canvas;
mod identify;
mod mirroring;
mod workspaces;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::command::output::Mode;
use sway_things_core::generate;
use sway_things_core::outputs::kanshi;

use sway_things_core::outputs::layout::{self, overlaps};
use sway_things_core::outputs::{MODE, Output, Outputs, Rect};
use sway_things_core::registry::{Scope, Value};

pub use canvas::Canvas;
pub use mirroring::Mirroring;
pub use workspaces::WorkspaceMap;

use super::SettingRow;
use crate::model::Model;
use crate::style;

/// The label of the picker's entry for every output at once.
const ALL_DISPLAYS: &str = "All displays";

/// The arrangement section's content: the canvas, its hint and the warning about gaps.
#[derive(Debug, Clone)]
pub struct Arrangement {
    root: gtk::Box,
    canvas: Canvas,
    /// Shown in place of the canvas while there is nothing to arrange.
    empty: gtk::Label,
    /// The hint and the "Identify" button.
    hint: gtk::Widget,
    /// Shown while outputs overlap or don't touch.
    gaps: gtk::Box,
    /// Says how the arrangement is remembered: by kanshi, or what that takes.
    kanshi: gtk::Box,
    identify: gtk::Button,
    /// The windows with the numbers while they are shown, and what they show.
    identifying: Rc<RefCell<Option<Identifying>>>,
}

/// The windows "Identify" shows, and the outputs' names, numbers and labels on them.
#[derive(Debug)]
struct Identifying {
    windows: Vec<gtk::Window>,
    numbered: Vec<(String, usize, String)>,
}

impl Arrangement {
    /// The arrangement of the outputs of `model`, applied through it.
    pub fn new(model: &Model) -> Self {
        let canvas = Canvas::new(model);
        let empty = gtk::Label::new(None);
        empty.set_wrap(true);
        empty.set_xalign(0.0);
        empty.add_css_class(style::DIM);
        empty.add_css_class("st-section-hint");
        empty.set_visible(false);
        let hint = gtk::Label::new(Some(
            "Drag the displays to where they stand on your desk. Pick one to change its \
             settings below.",
        ));
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        hint.set_hexpand(true);
        hint.add_css_class(style::DIM);
        let identify = gtk::Button::with_label("Identify");
        identify.set_valign(gtk::Align::Center);
        if identify::is_supported() {
            identify.set_tooltip_text(Some("Show each display\u{2019}s number on it"));
        } else {
            identify.set_sensitive(false);
            identify.set_tooltip_text(Some(
                "Numbers on the displays need a compositor with wlr-layer-shell, like sway",
            ));
        }
        let below = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        below.append(&hint);
        below.append(&identify);

        let warning = gtk::Label::new(Some(
            "Some displays overlap or don\u{2019}t touch, so the pointer can\u{2019}t reach every \
             one.",
        ));
        warning.set_wrap(true);
        warning.set_xalign(0.0);
        warning.set_hexpand(true);
        let fix = gtk::Button::with_label("Arrange Side by Side");
        fix.set_valign(gtk::Align::Center);
        let gaps = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        gaps.add_css_class("st-display-warning");
        gaps.append(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        gaps.append(&warning);
        gaps.append(&fix);
        gaps.set_visible(false);
        fix.connect_clicked(glib::clone!(
            #[weak]
            canvas,
            move |_| canvas.arrange_side_by_side()
        ));

        let note = gtk::Label::new(None);
        note.set_wrap(true);
        note.set_xalign(0.0);
        note.set_hexpand(true);
        note.set_selectable(true);
        let copy = gtk::Button::with_label("Copy Command");
        copy.set_valign(gtk::Align::Center);
        copy.set_visible(false);
        copy.connect_clicked(|button| {
            if let Some(line) = button.tooltip_text() {
                button.clipboard().set_text(&line);
            }
        });
        let kanshi = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        kanshi.add_css_class("st-display-warning");
        kanshi.append(&gtk::Image::from_icon_name("dialog-information-symbolic"));
        kanshi.append(&note);
        kanshi.append(&copy);
        kanshi.set_visible(false);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.add_css_class("st-display-arrangement");
        root.append(&kanshi);
        root.append(&canvas);
        root.append(&empty);
        root.append(&gaps);
        root.append(&below);
        let arrangement = Self {
            root,
            canvas,
            empty,
            hint: below.upcast(),
            gaps,
            kanshi,
            identify,
            identifying: Rc::new(RefCell::new(None)),
        };
        // Weak: the button belongs to the arrangement.
        arrangement.identify.connect_clicked(glib::clone!(
            #[weak(rename_to = canvas)]
            arrangement.canvas,
            #[strong(rename_to = identifying)]
            arrangement.identifying,
            move |button| {
                start_identify(&canvas, button, &identifying);
            }
        ));
        arrangement
    }

    /// Shows each output's number on it for a moment, and lights up the tiles meanwhile.
    /// Returns on how many outputs the number is shown.
    pub fn identify(&self) -> usize {
        start_identify(&self.canvas, &self.identify, &self.identifying)
    }

    /// The button that shows each output's number on it.
    pub fn identify_button(&self) -> &gtk::Button {
        &self.identify
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// The canvas.
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// Says how the arrangement is remembered: by kanshi per combination of displays, or what
    /// it takes for that (the include line for kanshi's config, or kanshi itself).
    fn show_kanshi(&self, known: bool, displays: generate::Displays) {
        let mut parts = std::iter::successors(self.kanshi.first_child(), WidgetExt::next_sibling);
        let (Some(_), Some(note), Some(copy)) = (parts.next(), parts.next(), parts.next()) else {
            return;
        };
        let (Ok(note), Ok(copy)) = (
            note.downcast::<gtk::Label>(),
            copy.downcast::<gtk::Button>(),
        ) else {
            return;
        };
        let files = kanshi::KanshiFiles::from_env();
        let text = match (displays, &files) {
            (generate::Displays::Kanshi { start }, _) => {
                copy.set_visible(false);
                let mut text = String::from(
                    "Each combination of displays is remembered: kanshi brings this arrangement \
                     back whenever these displays are connected together.",
                );
                if start && !sway_things_core::outputs::kanshi_runs() {
                    text.push_str(" kanshi starts with your next login.");
                }
                Some(text)
            }
            (generate::Displays::Sway, Some(files))
                if kanshi::installed() && files.inclusion() == kanshi::Inclusion::Missing =>
            {
                let command = files.append_command();
                copy.set_tooltip_text(Some(&command));
                copy.set_visible(true);
                Some(format!(
                    "So that each combination of displays is remembered, your kanshi config needs \
                     the line \u{201c}{}\u{201d} at its end. This command adds it: {command}",
                    files.include_line()
                ))
            }
            (generate::Displays::Sway, _) if !kanshi::installed() => {
                copy.set_visible(false);
                Some(
                    "With kanshi installed, each combination of displays is remembered and comes \
                     back on its own."
                        .to_owned(),
                )
            }
            (generate::Displays::Sway, _) => None,
        };
        note.set_text(text.as_deref().unwrap_or_default());
        self.kanshi.set_visible(known && text.is_some());
    }

    /// Whether the note that kanshi sets up the displays is shown.
    pub fn says_kanshi_runs(&self) -> bool {
        self.kanshi.is_visible()
    }

    /// Whether the warning about overlapping or separate outputs is shown.
    pub fn warns_of_gaps(&self) -> bool {
        self.gaps.is_visible()
    }

    /// Shows the outputs of `model`.
    fn update(&self, model: &Model, outputs: &Outputs, picker: &OutputPicker) {
        let on = outputs
            .enabled()
            .filter(|output| output.rect().is_some())
            .count();
        let known = model.outputs_known();
        let message = if !known {
            Some("Displays can be arranged while Sway Things runs in a sway session.")
        } else if on == 0 {
            Some("No display is on.")
        } else {
            None
        };
        // While a risky change waits, the layout stays as it is: Revert puts back only that change.
        let waits = model.pending_change().is_some();
        self.canvas.set_sensitive(!waits);
        if let Some(fix) = self.gaps.last_child() {
            fix.set_sensitive(!waits);
        }
        // Displays come and go with kanshi starting at a login, so look again.
        model.look_for_kanshi();
        // A model made for tests doesn't look at the machine.
        self.show_kanshi(known && model.looks_for_kanshi(), model.displays_backend());
        self.empty.set_text(message.unwrap_or_default());
        self.empty.set_visible(message.is_some());
        self.canvas.set_visible(message.is_none());
        self.hint.set_visible(message.is_none());
        self.canvas
            .set_outputs(outputs, |identifier| picker.number(identifier));
        // A display plugged in or out while the numbers are shown: they may be wrong now.
        let changed = self
            .identifying
            .borrow()
            .as_ref()
            .is_some_and(|shown| shown.numbered != shown_numbers(&self.canvas));
        if changed {
            stop_identify(&self.canvas, &self.identify, &self.identifying);
        }
        let rects: Vec<Rect> = self
            .canvas
            .layout()
            .into_iter()
            .map(|(_, rect)| rect)
            .collect();
        let overlapping = rects
            .iter()
            .enumerate()
            .any(|(index, a)| rects.iter().skip(index + 1).any(|b| overlaps(*a, *b)));
        self.gaps
            .set_visible(message.is_none() && (overlapping || !layout::is_connected(&rects)));
    }
}

/// Shows each output's number on it for [`identify::SHOWN_FOR`] and outlines the tiles of
/// `canvas` meanwhile; `button` can't be pressed again until then. Returns on how many outputs
/// the number is shown.
fn start_identify(
    canvas: &Canvas,
    button: &gtk::Button,
    identifying: &Rc<RefCell<Option<Identifying>>>,
) -> usize {
    if identifying.borrow().is_some() {
        return 0;
    }
    let numbered = shown_numbers(canvas);
    let windows = identify::show(&numbered);
    let shown = windows.len();
    identifying.replace(Some(Identifying { windows, numbered }));
    canvas.add_css_class("st-identifying");
    button.set_sensitive(false);
    let state = Rc::clone(identifying);
    glib::timeout_add_local_once(
        identify::SHOWN_FOR,
        glib::clone!(
            #[weak]
            canvas,
            #[weak]
            button,
            move || stop_identify(&canvas, &button, &state)
        ),
    );
    shown
}

/// Takes the numbers off the displays and the outline off the tiles.
fn stop_identify(
    canvas: &Canvas,
    button: &gtk::Button,
    identifying: &Rc<RefCell<Option<Identifying>>>,
) {
    let Some(shown) = identifying.take() else {
        return;
    };
    for window in shown.windows {
        window.destroy();
    }
    canvas.remove_css_class("st-identifying");
    button.set_sensitive(identify::is_supported());
}

/// The names, numbers and labels of the outputs on `canvas` that are on.
fn shown_numbers(canvas: &Canvas) -> Vec<(String, usize, String)> {
    canvas
        .numbered()
        .into_iter()
        .filter(|(_, _, _, off)| !off)
        .map(|(name, number, label, _)| (name, number, label))
        .collect()
}

/// Picks the output the rows are for: every output at once, or one of them.
#[derive(Debug, Clone)]
pub struct OutputPicker {
    root: gtk::Box,
    dropdown: gtk::DropDown,
    labels: gtk::StringList,
    /// The output of each entry after the first, which is every output.
    identifiers: Rc<RefCell<Vec<String>>>,
    /// Set while the list is rebuilt, so that isn't taken as the user's choice.
    updating: Rc<std::cell::Cell<bool>>,
}

impl Default for OutputPicker {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputPicker {
    /// A picker with only "All displays".
    pub fn new() -> Self {
        let title = gtk::Label::new(Some("Settings for"));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_wrap(true);
        title.add_css_class("st-row-title");
        let labels = gtk::StringList::new(&[ALL_DISPLAYS]);
        let dropdown = gtk::DropDown::new(Some(labels.clone()), gtk::Expression::NONE);
        dropdown.set_valign(gtk::Align::Center);
        dropdown.update_property(&[gtk::accessible::Property::Label("Settings for")]);
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        root.append(&title);
        root.append(&dropdown);
        Self {
            root,
            dropdown,
            labels,
            identifiers: Rc::new(RefCell::new(Vec::new())),
            updating: Rc::new(std::cell::Cell::new(false)),
        }
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// The drop-down.
    pub fn dropdown(&self) -> &gtk::DropDown {
        &self.dropdown
    }

    /// What the drop-down offers, in order: every output first.
    pub fn labels(&self) -> Vec<String> {
        (0..self.labels.n_items())
            .filter_map(|index| self.labels.string(index))
            .map(String::from)
            .collect()
    }

    /// The output picked, or `None` for every output.
    pub fn selected(&self) -> Option<String> {
        let index = usize::try_from(self.dropdown.selected()).ok()?;
        self.identifiers
            .borrow()
            .get(index.checked_sub(1)?)
            .cloned()
    }

    /// Picks the output `identifier`, or with `None` every output, without telling.
    pub fn select(&self, identifier: Option<&str>) {
        let index = match identifier {
            Some(identifier) => self
                .identifiers
                .borrow()
                .iter()
                .position(|known| known == identifier)
                .map_or(0, |index| index + 1),
            None => 0,
        };
        if let Ok(index) = u32::try_from(index) {
            self.updating.set(true);
            self.dropdown.set_selected(index);
            self.updating.set(false);
        }
    }

    /// The number of the output `identifier`, as the canvas and the picker show it: its place
    /// among the connected outputs, counting from 1.
    pub fn number(&self, identifier: &str) -> Option<usize> {
        self.identifiers
            .borrow()
            .iter()
            .position(|known| known == identifier)
            .map(|index| index + 1)
    }

    /// Lists the outputs of `outputs`; the output picked stays picked while it is listed.
    fn update(&self, outputs: &Outputs) {
        let picked = self.selected();
        let entries: Vec<(String, String)> = outputs
            .iter()
            .enumerate()
            .map(|(index, output)| (output.identifier().to_owned(), entry(index + 1, output)))
            .collect();
        let same = {
            let identifiers = self.identifiers.borrow();
            identifiers.len() == entries.len()
                && identifiers
                    .iter()
                    .zip(&entries)
                    .all(|(known, (identifier, _))| known == identifier)
        };
        let shown: Vec<String> = std::iter::once(ALL_DISPLAYS.to_owned())
            .chain(entries.iter().map(|(_, label)| label.clone()))
            .collect();
        if same && shown == self.labels() {
            return;
        }
        self.updating.set(true);
        let texts: Vec<&str> = shown.iter().map(String::as_str).collect();
        self.labels.splice(0, self.labels.n_items(), &texts);
        self.identifiers.replace(
            entries
                .into_iter()
                .map(|(identifier, _)| identifier)
                .collect(),
        );
        self.updating.set(false);
        self.select(picked.as_deref());
    }

    /// Calls `callback` with the output the user picked, or `None` for every output.
    fn connect_picked<F: Fn(Option<&str>) + 'static>(&self, callback: F) {
        // Not the picker itself: the drop-down would keep itself alive.
        let identifiers = Rc::clone(&self.identifiers);
        let updating = Rc::clone(&self.updating);
        self.dropdown.connect_selected_notify(move |dropdown| {
            if updating.get() {
                return;
            }
            let picked = usize::try_from(dropdown.selected())
                .ok()
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| identifiers.borrow().get(index).cloned());
            callback(picked.as_deref());
        });
    }

    /// The picker without keeping its widgets alive, for handlers of other objects.
    fn downgrade(&self) -> WeakPicker {
        WeakPicker {
            root: self.root.downgrade(),
            dropdown: self.dropdown.downgrade(),
            labels: self.labels.clone(),
            identifiers: Rc::clone(&self.identifiers),
            updating: Rc::clone(&self.updating),
        }
    }
}

/// An [`OutputPicker`] whose widgets may be gone.
#[derive(Debug)]
struct WeakPicker {
    root: glib::WeakRef<gtk::Box>,
    dropdown: glib::WeakRef<gtk::DropDown>,
    labels: gtk::StringList,
    identifiers: Rc<RefCell<Vec<String>>>,
    updating: Rc<std::cell::Cell<bool>>,
}

impl WeakPicker {
    fn upgrade(&self) -> Option<OutputPicker> {
        Some(OutputPicker {
            root: self.root.upgrade()?,
            dropdown: self.dropdown.upgrade()?,
            labels: self.labels.clone(),
            identifiers: Rc::clone(&self.identifiers),
            updating: Rc::clone(&self.updating),
        })
    }
}

impl Arrangement {
    /// The arrangement without keeping its widgets alive, for handlers of other objects.
    fn downgrade(&self) -> WeakArrangement {
        WeakArrangement {
            root: self.root.downgrade(),
            canvas: self.canvas.downgrade(),
            empty: self.empty.downgrade(),
            hint: self.hint.downgrade(),
            gaps: self.gaps.downgrade(),
            kanshi: self.kanshi.downgrade(),
            identify: self.identify.downgrade(),
            identifying: Rc::clone(&self.identifying),
        }
    }
}

/// An [`Arrangement`] whose widgets may be gone.
#[derive(Debug)]
struct WeakArrangement {
    root: glib::WeakRef<gtk::Box>,
    canvas: glib::WeakRef<Canvas>,
    empty: glib::WeakRef<gtk::Label>,
    hint: glib::WeakRef<gtk::Widget>,
    gaps: glib::WeakRef<gtk::Box>,
    kanshi: glib::WeakRef<gtk::Box>,
    identify: glib::WeakRef<gtk::Button>,
    identifying: Rc<RefCell<Option<Identifying>>>,
}

impl WeakArrangement {
    fn upgrade(&self) -> Option<Arrangement> {
        Some(Arrangement {
            root: self.root.upgrade()?,
            canvas: self.canvas.upgrade()?,
            empty: self.empty.upgrade()?,
            hint: self.hint.upgrade()?,
            gaps: self.gaps.upgrade()?,
            kanshi: self.kanshi.upgrade()?,
            identify: self.identify.upgrade()?,
            identifying: Rc::clone(&self.identifying),
        })
    }
}

/// The entry of an output in the picker: its number, make and model, and its name or that it
/// is off or not connected.
fn entry(number: usize, output: &Output) -> String {
    let label = output.label();
    if !output.is_connected() {
        format!("{number}  {label} (not connected)")
    } else if !output.is_enabled() {
        format!("{number}  {label} \u{b7} {} (off)", output.name())
    } else if label == output.name() {
        format!("{number}  {label}")
    } else {
        format!("{number}  {label} \u{b7} {}", output.name())
    }
}

/// The modes of `output` for the mode row: "Automatic" (the output's preferred mode), then its
/// modes from the largest and fastest down.
pub fn mode_choices(output: Option<&Output>) -> Vec<(String, String)> {
    mode_choices_with(output, None)
}

/// The resolutions to choose from for `output`: _Automatic_, then each size it offers, largest
/// first, each standing for its mode with the highest refresh rate; the size of `current` stands
/// for `current` itself, so the entry in use is the mode in use.
fn mode_choices_with(output: Option<&Output>, current: Option<&str>) -> Vec<(String, String)> {
    let modes = known_modes(output, current);
    let current = current.and_then(|text| Mode::parse(text).ok());
    let mut sizes: Vec<(u32, u32)> = Vec::new();
    for mode in &modes {
        if !sizes.contains(&(mode.width(), mode.height())) {
            sizes.push((mode.width(), mode.height()));
        }
    }
    std::iter::once((String::new(), "Automatic".to_owned()))
        .chain(sizes.into_iter().filter_map(|(width, height)| {
            let mode = current
                .filter(|mode| (mode.width(), mode.height()) == (width, height))
                .or_else(|| {
                    modes
                        .iter()
                        .copied()
                        .filter(|mode| (mode.width(), mode.height()) == (width, height))
                        .max_by_key(|mode| mode.refresh().unwrap_or(0))
                })?;
            Some((mode.to_string(), format!("{width} \u{d7} {height}")))
        }))
        .collect()
}

/// The refresh rates to choose from for the resolution of `current` on `output`, highest first:
/// each the whole mode, labeled with its rate. Empty for _Automatic_.
fn refresh_choices(output: Option<&Output>, current: Option<&str>) -> Vec<(String, String)> {
    let Some(mode) = current.and_then(|text| Mode::parse(text).ok()) else {
        return Vec::new();
    };
    let mut rates: Vec<Mode> = Vec::new();
    for known in known_modes(output, Some(&mode.to_string())) {
        let same_size = (known.width(), known.height()) == (mode.width(), mode.height());
        if same_size && !rates.iter().any(|rate| rate.refresh() == known.refresh()) {
            rates.push(known);
        }
    }
    rates.sort_by_key(|rate| std::cmp::Reverse(rate.refresh().unwrap_or(0)));
    rates
        .into_iter()
        .map(|rate| (rate.to_string(), refresh_label(rate)))
        .collect()
}

/// The modes of `output`, and `current` and the mode in use if it doesn't list them, largest
/// and fastest first.
fn known_modes(output: Option<&Output>, current: Option<&str>) -> Vec<Mode> {
    let mut modes: Vec<Mode> = output
        .map(|output| output.modes().to_vec())
        .unwrap_or_default();
    let extra = current
        .and_then(|text| Mode::parse(text).ok())
        .into_iter()
        .chain(output.and_then(Output::mode));
    for mode in extra {
        let listed = modes
            .iter()
            .any(|known| known.to_string() == mode.to_string());
        if !listed {
            modes.push(mode);
        }
    }
    modes.sort_by_key(|mode| {
        std::cmp::Reverse((
            u64::from(mode.width()) * u64::from(mode.height()),
            mode.width(),
            mode.refresh().unwrap_or(0),
        ))
    });
    modes
}

/// A refresh rate as people read it: `59.951 Hz`, or `Any rate` for a mode without one.
fn refresh_label(mode: Mode) -> String {
    match mode.refresh() {
        Some(millihertz) => {
            // Three decimals, as sway counts them: 59.95 and 59.951 Hz are two modes.
            let hertz = format!("{:.3}", f64::from(millihertz) / 1000.0);
            let hertz = hertz.trim_end_matches('0').trim_end_matches('.');
            format!("{hertz} Hz")
        }
        None => "Any rate".to_owned(),
    }
}

/// Adds the picker of the output before the first of the rows of `rows` that are for outputs,
/// and keeps the arrangement, the picker and those rows current with `model`: the picker and
/// the canvas pick the output the rows are for, and the mode row offers that output's modes. At
/// first, and when the output picked is gone, the output with the focus is picked; every output
/// at once stays picked once the user picked it. Returns the picker.
pub fn follow(model: &Model, arrangement: &Arrangement, rows: &[SettingRow]) -> OutputPicker {
    let picker = OutputPicker::new();
    let rows = Rc::new(insert_picker(&picker, rows));
    let point_rows = Rc::new({
        let rows = Rc::clone(&rows);
        move |model: &Model, identifier: Option<&str>| {
            let outputs = model.outputs().unwrap_or_default();
            let output = identifier.and_then(|identifier| outputs.get(identifier));
            for row in rows.iter().filter_map(glib::WeakRef::upgrade) {
                row.set_device(identifier);
                offer_modes(&row, output);
                show_for(&row, identifier);
            }
        }
    });
    picker.connect_picked(glib::clone!(
        #[weak]
        model,
        #[weak(rename_to = canvas)]
        arrangement.canvas,
        #[strong]
        point_rows,
        move |identifier| {
            canvas.select(identifier);
            point_rows(&model, identifier);
        }
    ));
    let weak_picker = picker.downgrade();
    arrangement.canvas.connect_selected(glib::clone!(
        #[weak]
        model,
        #[strong]
        point_rows,
        move |_, identifier| {
            if let Some(picker) = weak_picker.upgrade() {
                picker.select(Some(identifier));
            }
            point_rows(&model, Some(identifier));
        }
    ));
    let started = Rc::new(std::cell::Cell::new(false));
    let weak_picker = picker.downgrade();
    let weak_arrangement = arrangement.downgrade();
    let update = glib::clone!(move |model: &Model| {
        let (Some(picker), Some(arrangement)) = (weak_picker.upgrade(), weak_arrangement.upgrade())
        else {
            return;
        };
        let Some(outputs) = model.outputs() else {
            return;
        };
        let before = picker.selected();
        picker.update(&outputs);
        let gone = before.is_some() && picker.selected().is_none();
        let mut chosen = picker.selected();
        if (!started.get() || gone) && model.outputs_known() {
            started.set(true);
            chosen = focused(&outputs);
            picker.select(chosen.as_deref());
        }
        arrangement.canvas.select(chosen.as_deref());
        arrangement.update(model, &outputs, &picker);
        if chosen == before {
            // The same output, maybe with other modes after a hotplug.
            let output = chosen.as_deref().and_then(|id| outputs.get(id));
            for row in rows.iter().filter_map(glib::WeakRef::upgrade) {
                // While a change runs, sway may report the outputs before the change is done:
                // the modes are offered again once the model is done, so the choice doesn't
                // jump back and forth.
                if !model.is_busy() {
                    offer_modes(&row, output);
                }
                show_for(&row, chosen.as_deref());
            }
        } else {
            point_rows(model, chosen.as_deref());
        }
    });
    update(model);
    let handler = model.connect_changed(update);
    // The model outlives the page: it lets go of the page when the canvas goes.
    let handler = RefCell::new(Some(handler));
    arrangement.canvas.connect_destroy(glib::clone!(
        #[weak]
        model,
        move |_| {
            if let Some(handler) = handler.take() {
                model.disconnect(handler);
            }
        }
    ));
    picker
}

/// Puts `picker` before the first of the rows of `rows` that are for outputs, and returns those
/// rows.
fn insert_picker(picker: &OutputPicker, rows: &[SettingRow]) -> Vec<glib::WeakRef<SettingRow>> {
    let rows: Vec<&SettingRow> = rows
        .iter()
        .filter(|row| {
            row.setting()
                .is_some_and(|setting| setting.scope() == Scope::PerOutput)
        })
        .collect();
    let list = rows
        .first()
        .and_then(|row| row.parent())
        .and_then(|item| item.parent())
        .and_downcast::<gtk::ListBox>();
    if let Some(list) = list {
        let item = gtk::ListBoxRow::new();
        item.set_activatable(false);
        item.set_focusable(false);
        item.set_child(Some(picker.widget()));
        list.prepend(&item);
    }
    rows.into_iter().map(ObjectExt::downgrade).collect()
}

/// Shows `row` unless its setting is only for one display at a time and the rows are for every
/// display (`identifier` is `None`): a mode, a position or switching off for all of them at once
/// isn't offered.
fn show_for(row: &SettingRow, identifier: Option<&str>) {
    let one_at_a_time = row
        .setting()
        .is_some_and(sway_things_core::registry::Setting::is_one_at_a_time);
    if let Some(item) = row.parent() {
        item.set_visible(identifier.is_some() || !one_at_a_time);
    }
}

/// Offers the modes of `output` in `row`, if it is the mode row.
fn offer_modes(row: &SettingRow, output: Option<&Output>) {
    if row.setting().is_none_or(|setting| setting.key() != MODE) {
        return;
    }
    let current = match row.value() {
        Some(Value::Text(text)) => Some(text),
        _ => None,
    };
    row.set_choices(&mode_choices_with(output, current.as_deref()));
    let rates = RefreshRates::of(row);
    rates.offer(
        &refresh_choices(output, current.as_deref()),
        current.as_deref(),
        row.locked().is_none(),
    );
}

thread_local! {
    /// The refresh rates of each mode row, while it lives.
    static RATES: RefCell<Vec<(glib::WeakRef<SettingRow>, RefreshRates)>> =
        const { RefCell::new(Vec::new()) };
}

/// The second control of the mode's row: the refresh rate, apart from the resolution. Each entry
/// is a whole mode of the resolution chosen; choosing one sends it as the row's value.
#[derive(Clone)]
struct RefreshRates {
    dropdown: gtk::DropDown,
    labels: gtk::StringList,
    values: Rc<RefCell<Vec<String>>>,
    updating: Rc<std::cell::Cell<bool>>,
}

impl RefreshRates {
    /// The refresh rates of the mode's `row`, added to it the first time.
    fn of(row: &SettingRow) -> Self {
        RATES.with_borrow_mut(|rows| {
            rows.retain(|(weak, _)| weak.upgrade().is_some());
            if let Some((_, rates)) = rows
                .iter()
                .find(|(weak, _)| weak.upgrade().as_ref() == Some(row))
            {
                return rates.clone();
            }
            let rates = Self::new(row);
            rows.push((row.downgrade(), rates.clone()));
            // Let go of them with the row.
            row.connect_destroy(|row| {
                RATES.with_borrow_mut(|rows| {
                    rows.retain(|(weak, _)| weak.upgrade().is_some_and(|other| other != *row));
                });
            });
            rates
        })
    }

    fn new(row: &SettingRow) -> Self {
        let labels = gtk::StringList::new(&[]);
        let dropdown = gtk::DropDown::new(Some(labels.clone()), gtk::Expression::NONE);
        dropdown.set_valign(gtk::Align::Center);
        dropdown.update_property(&[gtk::accessible::Property::Label("Refresh rate")]);
        dropdown.set_tooltip_text(Some("Refresh rate"));
        let rates = Self {
            dropdown,
            labels,
            values: Rc::new(RefCell::new(Vec::new())),
            updating: Rc::new(std::cell::Cell::new(false)),
        };
        let weak_row = row.downgrade();
        let values = Rc::clone(&rates.values);
        let updating = Rc::clone(&rates.updating);
        rates.dropdown.connect_selected_notify(move |dropdown| {
            if updating.get() {
                return;
            }
            let value = usize::try_from(dropdown.selected())
                .ok()
                .and_then(|index| values.borrow().get(index).cloned());
            if let (Some(row), Some(value)) = (weak_row.upgrade(), value) {
                if row.value() != Some(Value::Text(value.clone())) {
                    row.change(Value::Text(value));
                }
            }
        });
        row.add_control(&rates.dropdown);
        rates
    }

    /// Offers `choices` and shows `current` among them; hidden while sway chooses the mode, and
    /// only to look at unless `open` (the row can be changed now).
    fn offer(&self, choices: &[(String, String)], current: Option<&str>, open: bool) {
        self.updating.set(true);
        let shown: Vec<&str> = choices.iter().map(|(_, label)| label.as_str()).collect();
        self.labels.splice(0, self.labels.n_items(), &shown);
        self.values
            .replace(choices.iter().map(|(value, _)| value.clone()).collect());
        let index = choices
            .iter()
            .position(|(value, _)| Some(value.as_str()) == current)
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(gtk::INVALID_LIST_POSITION);
        self.dropdown.set_selected(index);
        self.dropdown.set_visible(!choices.is_empty());
        // One rate is nothing to choose, but still says what it is.
        self.dropdown.set_sensitive(open && choices.len() > 1);
        // The row says why it is locked; open, the drop-down says what it is.
        if open {
            self.dropdown.set_tooltip_text(Some("Refresh rate"));
        }
        self.updating.set(false);
    }
}

/// Keeps `mirroring` showing what each display of `model` shows, numbered as `picker` numbers
/// them, until it is gone.
pub fn follow_mirroring(model: &Model, mirroring: &Mirroring, picker: &OutputPicker) {
    let shown = mirroring.clone();
    follow_section(
        model,
        mirroring.widget(),
        picker,
        move |model, outputs, number| {
            shown.update(model, outputs, number);
        },
    );
}

/// Keeps `map` showing the placement of the workspaces for the outputs of `model`, numbered as
/// `picker` numbers them, until the map is gone.
pub fn follow_workspaces(model: &Model, map: &WorkspaceMap, picker: &OutputPicker) {
    let shown = map.clone();
    follow_section(
        model,
        map.widget(),
        picker,
        move |model, outputs, number| {
            shown.update(model, outputs, number);
        },
    );
}

/// Calls `update` with the outputs of `model` and their numbers in `picker` now and whenever
/// the model changes, until `widget` is gone.
fn follow_section(
    model: &Model,
    widget: &gtk::Box,
    picker: &OutputPicker,
    update: impl Fn(&Model, &Outputs, &dyn Fn(&str) -> Option<usize>) + 'static,
) {
    let weak_root = widget.downgrade();
    let weak_picker = picker.downgrade();
    let refresh = move |model: &Model| {
        if weak_root.upgrade().is_none() {
            return;
        }
        let Some(outputs) = model.outputs() else {
            return;
        };
        let picker = weak_picker.upgrade();
        let number =
            |identifier: &str| picker.as_ref().and_then(|picker| picker.number(identifier));
        update(model, &outputs, &number);
    };
    refresh(model);
    let handler = RefCell::new(Some(model.connect_changed(refresh)));
    widget.connect_destroy(glib::clone!(
        #[weak]
        model,
        move |_| {
            if let Some(handler) = handler.take() {
                model.disconnect(handler);
            }
        }
    ));
}

/// The output with the focus, else the first that is on.
fn focused(outputs: &Outputs) -> Option<String> {
    outputs
        .enabled()
        .find(|output| output.is_focused())
        .or_else(|| outputs.enabled().next())
        .map(|output| output.identifier().to_owned())
}

#[cfg(test)]
mod tests {
    use super::{mode_choices_with, refresh_choices, refresh_label};
    use sway_things_core::command::output::Mode;

    #[test]
    fn rates_read_as_people_read_them() {
        let label = |text: &str| refresh_label(Mode::parse(text).unwrap());
        assert_eq!(label("2560x1440@59.951Hz"), "59.951 Hz");
        assert_eq!(label("2560x1440@59.95Hz"), "59.95 Hz");
        assert_eq!(label("1920x1080@60Hz"), "60 Hz");
        assert_eq!(label("1920x1080@143.912Hz"), "143.912 Hz");
        assert_eq!(label("1280x720"), "Any rate");
    }

    #[test]
    fn resolutions_and_rates_are_chosen_apart() {
        // Without the output's modes, only the current one is known.
        let current = Some("1920x1080@59.94Hz");
        let resolutions = mode_choices_with(None, current);
        assert_eq!(
            resolutions,
            [
                (String::new(), "Automatic".to_owned()),
                (
                    "1920x1080@59.94Hz".to_owned(),
                    "1920 \u{d7} 1080".to_owned()
                ),
            ]
        );
        assert_eq!(
            refresh_choices(None, current),
            [("1920x1080@59.94Hz".to_owned(), "59.94 Hz".to_owned())]
        );
        assert_eq!(refresh_choices(None, Some("")), [] as [(String, String); 0]);
        // With the output's modes: the rates of the size in use, fastest first, each exact.
        let json = r#"[{"name": "DP-4", "active": true, "primary": false, "make": "A",
            "model": "B", "serial": "C",
            "modes": [{"width": 2560, "height": 1440, "refresh": 59951},
                      {"width": 2560, "height": 1440, "refresh": 143912},
                      {"width": 1920, "height": 1080, "refresh": 60000}],
            "current_mode": {"width": 2560, "height": 1440, "refresh": 59951}}]"#;
        let outputs =
            sway_things_core::outputs::Outputs::new(&serde_json::from_str::<Vec<_>>(json).unwrap());
        let output = outputs.find("DP-4");
        assert_eq!(
            refresh_choices(output, Some("2560x1440@59.951Hz")),
            [
                ("2560x1440@143.912Hz".to_owned(), "143.912 Hz".to_owned()),
                ("2560x1440@59.951Hz".to_owned(), "59.951 Hz".to_owned()),
            ]
        );
        // The size in use stands for the mode in use; another size for its fastest mode.
        assert_eq!(
            mode_choices_with(output, Some("2560x1440@59.951Hz")),
            [
                (String::new(), "Automatic".to_owned()),
                (
                    "2560x1440@59.951Hz".to_owned(),
                    "2560 \u{d7} 1440".to_owned()
                ),
                ("1920x1080@60Hz".to_owned(), "1920 \u{d7} 1080".to_owned()),
            ]
        );
    }
}
