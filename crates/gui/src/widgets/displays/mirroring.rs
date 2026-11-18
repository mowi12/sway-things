// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The "Show" row among the settings of the picked display: it shows its own content, or another
//! display's (wl-mirror), remembered for the combination of displays connected now.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use sway_things_core::outputs::Outputs;
use sway_things_core::outputs::mirror::{self, Mirror, Scaling};
use sway_things_core::outputs::workspaces::left_to_right;

use super::canvas::short_label;
use crate::model::Model;

/// The row, for the display picked above the settings.
#[derive(Debug, Clone)]
pub struct Mirroring {
    item: gtk::ListBoxRow,
    note: gtk::Label,
    source: gtk::DropDown,
    scaling: gtk::DropDown,
    /// The display the row is for.
    target: Rc<RefCell<Option<String>>>,
    /// The choices of the source drop-down as offered: `None` for its own content.
    sources: Rc<RefCell<Vec<Option<String>>>>,
    updating: Rc<Cell<bool>>,
}

impl Mirroring {
    /// The row, applied through `model`.
    pub fn new(model: &Model) -> Self {
        let title = gtk::Label::new(Some("Show"));
        title.set_xalign(0.0);
        title.add_css_class("st-row-title");
        let subtitle = gtk::Label::new(Some(
            "Its own content, or the picture of another display, like a projector showing the \
             laptop's screen",
        ));
        subtitle.set_xalign(0.0);
        subtitle.set_wrap(true);
        subtitle.add_css_class(crate::style::DIM);
        let note = gtk::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class(crate::style::DIM);
        note.add_css_class("st-row-detail");
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 3);
        texts.set_hexpand(true);
        texts.set_valign(gtk::Align::Center);
        texts.append(&title);
        texts.append(&subtitle);
        texts.append(&note);
        let source = gtk::DropDown::from_strings(&[]);
        source.set_valign(gtk::Align::Center);
        source.update_property(&[gtk::accessible::Property::Label("What the display shows")]);
        let scaling = gtk::DropDown::from_strings(&["Fit", "Cover", "Exact Multiples"]);
        scaling.set_valign(gtk::Align::Center);
        scaling.set_tooltip_text(Some("How the picture fits the display"));
        scaling.update_property(&[gtk::accessible::Property::Label("How the picture fits")]);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        controls.set_valign(gtk::Align::Center);
        controls.append(&source);
        controls.append(&scaling);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        content.add_css_class("st-mirror-row");
        content.append(&texts);
        content.append(&controls);
        let item = gtk::ListBoxRow::new();
        item.set_activatable(false);
        item.set_focusable(false);
        item.set_child(Some(&content));
        let row = Self {
            item,
            note,
            source,
            scaling,
            target: Rc::new(RefCell::new(None)),
            sources: Rc::new(RefCell::new(Vec::new())),
            updating: Rc::new(Cell::new(false)),
        };
        row.connect(model);
        row
    }

    fn connect(&self, model: &Model) {
        let source = self.source.clone();
        let scaling = self.scaling.clone();
        let target = Rc::clone(&self.target);
        let sources = Rc::clone(&self.sources);
        let updating = Rc::clone(&self.updating);
        let model = model.downgrade();
        let send = Rc::new(move || {
            if updating.get() {
                return;
            }
            let (Some(model), Some(target)) = (model.upgrade(), target.borrow().clone()) else {
                return;
            };
            let of = usize::try_from(source.selected())
                .ok()
                .and_then(|index| sources.borrow().get(index).cloned())
                .flatten();
            let fits = usize::try_from(scaling.selected())
                .ok()
                .and_then(|index| Scaling::ALL.get(index).copied())
                .unwrap_or_default();
            model.set_mirror(
                &target,
                of.map(|of| Mirror {
                    of,
                    scaling: fits,
                    gathered: Vec::new(),
                }),
            );
        });
        let on_source = Rc::clone(&send);
        self.source.connect_selected_notify(move |_| on_source());
        self.scaling.connect_selected_notify(move |_| send());
    }

    /// The row to put among the settings of the displays.
    pub fn item(&self) -> &gtk::ListBoxRow {
        &self.item
    }

    /// Shows the row for the display `target` of `outputs` (hidden for every display at once,
    /// for a display that is off and with a single display), as `model` remembers it; `number`
    /// gives a display's number as on the canvas.
    pub fn update(
        &self,
        model: &Model,
        outputs: &Outputs,
        target: Option<&str>,
        number: &dyn Fn(&str) -> Option<usize>,
    ) {
        let on = left_to_right(outputs);
        let target = target.filter(|target| on.iter().any(|output| output.identifier() == *target));
        self.item.set_visible(target.is_some() && on.len() > 1);
        let Some(target) = target else {
            self.target.replace(None);
            return;
        };
        self.target.replace(Some(target.to_owned()));
        let mirrors = model.mirrors().unwrap_or_default();
        // No chains, no cycles: only a display that shows its own content can be shown, and a
        // display shown elsewhere keeps its own.
        let shown_elsewhere = mirrors.values().any(|mirror| mirror.of == target);
        let mut sources: Vec<Option<String>> = vec![None];
        let mut labels: Vec<String> = vec!["Its own content".to_owned()];
        if !shown_elsewhere {
            let others = on.iter().filter(|output| {
                output.identifier() != target && !mirrors.contains_key(output.identifier())
            });
            for output in others {
                let name = number(output.identifier()).map_or_else(
                    || output.label(),
                    |n| format!("{n}  {}", short_label(output)),
                );
                sources.push(Some(output.identifier().to_owned()));
                labels.push(format!("Picture of {name}"));
            }
        }
        let current = mirrors.get(target);
        let index = sources
            .iter()
            .position(|source| source.as_deref() == current.map(|mirror| mirror.of.as_str()))
            .unwrap_or(0);
        let scaling_index = Scaling::ALL
            .iter()
            .position(|scaling| Some(*scaling) == current.map(|mirror| mirror.scaling))
            .unwrap_or(0);
        // A change that waits to be kept or reverted: the picture waits too (setting it writes
        // kanshi's profiles, which would put back what the trial changed).
        let waits = model.pending_change().is_some();
        let note = if waits {
            Some("Keep or revert the change first")
        } else if !model.kanshi_runs() {
            Some("Needs kanshi, which remembers each combination of displays (see above)")
        } else if !mirror::installed() {
            Some("Needs wl-mirror, which shows one display on another")
        } else if shown_elsewhere {
            Some("Shown on another display, so it shows its own content")
        } else {
            None
        };
        let strings: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.updating.set(true);
        self.source.set_model(Some(&gtk::StringList::new(&strings)));
        self.source.set_selected(u32::try_from(index).unwrap_or(0));
        self.scaling
            .set_selected(u32::try_from(scaling_index).unwrap_or(0));
        self.updating.set(false);
        self.sources.replace(sources);
        // A mirror that runs can always be ended, except while a change waits.
        self.source
            .set_sensitive(!waits && (note.is_none() || current.is_some()));
        self.scaling.set_sensitive(!waits);
        self.scaling.set_visible(current.is_some());
        self.note.set_text(note.unwrap_or_default());
        self.note.set_visible(note.is_some());
    }
}
