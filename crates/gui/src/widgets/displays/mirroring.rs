// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The "Mirroring" section of the Displays page: a display can show another one instead of its
//! own content (wl-mirror), remembered for the combination of displays connected now.
//!
//! A row per display that is on, from left to right, chooses what it shows: its own content,
//! or another display; and for a mirror, how it fits.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::outputs::Outputs;
use sway_things_core::outputs::mirror::{self, Mirror, Scaling};
use sway_things_core::outputs::workspaces::left_to_right;

use super::canvas::short_label;
use crate::model::Model;

/// The section's contents.
#[derive(Debug, Clone)]
pub struct Mirroring {
    root: gtk::Box,
    note: gtk::Label,
    rows: gtk::Box,
    updating: Rc<Cell<bool>>,
}

impl Mirroring {
    /// The section, applied through `model`.
    pub fn new() -> Self {
        let note = gtk::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("st-row-detail");
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.append(&note);
        root.append(&rows);
        Self {
            root,
            note,
            rows,
            updating: Rc::new(Cell::new(false)),
        }
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// Shows what each display of `outputs` shows, as `model` remembers it; `number` gives a
    /// display's number as on the canvas.
    pub fn update(&self, model: &Model, outputs: &Outputs, number: &dyn Fn(&str) -> Option<usize>) {
        let kanshi = model.kanshi_runs();
        let installed = mirror::installed();
        let note = if !kanshi {
            Some("Needs kanshi, which remembers each combination of displays (see above).")
        } else if !installed {
            Some("Needs wl-mirror, which shows one display on another.")
        } else {
            None
        };
        self.note.set_text(note.unwrap_or(
            "A display can show another one instead of its own content, like a projector the \
             laptop's screen; remembered for this combination of displays.",
        ));
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }
        if note.is_some() {
            return;
        }
        let mirrors = model.mirrors().unwrap_or_default();
        let displays: Vec<(String, String)> = left_to_right(outputs)
            .into_iter()
            .map(|output| {
                let shown = number(output.identifier()).map_or_else(
                    || output.label(),
                    |n| format!("{n}  {}", short_label(output)),
                );
                (output.identifier().to_owned(), shown)
            })
            .collect();
        if displays.len() < 2 {
            self.note
                .set_text("Mirroring needs a second display that is on.");
            return;
        }
        self.updating.set(true);
        for (target, shown) in &displays {
            self.rows
                .append(&self.row(model, target, shown, &displays, &mirrors));
        }
        self.updating.set(false);
    }

    /// The row of the display `target`, titled `shown`.
    fn row(
        &self,
        model: &Model,
        target: &str,
        shown: &str,
        displays: &[(String, String)],
        mirrors: &BTreeMap<String, Mirror>,
    ) -> gtk::Box {
        let title = gtk::Label::new(Some(shown));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.add_css_class("st-row-title");
        // No chains, no cycles: only a display that shows its own content can be shown, and a
        // display shown elsewhere keeps its own.
        let shown_elsewhere = mirrors.values().any(|mirror| mirror.of == target);
        let sources: Vec<(Option<String>, String)> =
            std::iter::once((None, "Its own content".to_owned()))
                .chain(
                    displays
                        .iter()
                        .filter(|(id, _)| id != target && !mirrors.contains_key(id))
                        .filter(|_| !shown_elsewhere)
                        .map(|(id, other)| (Some(id.clone()), format!("Mirror of {other}"))),
                )
                .collect();
        let labels: Vec<&str> = sources.iter().map(|(_, label)| label.as_str()).collect();
        let source = gtk::DropDown::from_strings(&labels);
        if shown_elsewhere {
            source.set_sensitive(false);
            source.set_tooltip_text(Some(
                "Shown on another display, so it shows its own content",
            ));
        }
        source.update_property(&[gtk::accessible::Property::Label(&format!(
            "What {shown} shows"
        ))]);
        let current = mirrors.get(target);
        let index = sources
            .iter()
            .position(|(id, _)| id.as_deref() == current.map(|mirror| mirror.of.as_str()))
            .unwrap_or(0);
        source.set_selected(u32::try_from(index).unwrap_or(0));
        let scaling_labels = ["Fit", "Cover", "Exact Multiples"];
        let scaling = gtk::DropDown::from_strings(&scaling_labels);
        scaling.update_property(&[gtk::accessible::Property::Label("How the mirror fits")]);
        let scaling_index = Scaling::ALL
            .iter()
            .position(|scaling| Some(*scaling) == current.map(|mirror| mirror.scaling))
            .unwrap_or(0);
        scaling.set_selected(u32::try_from(scaling_index).unwrap_or(0));
        scaling.set_visible(current.is_some());
        let sources = Rc::new(sources);
        let send = {
            let sources = Rc::clone(&sources);
            let target = target.to_owned();
            let updating = Rc::clone(&self.updating);
            let model = model.downgrade();
            move |source: &gtk::DropDown, scaling: &gtk::DropDown| {
                if updating.get() {
                    return;
                }
                let Some(model) = model.upgrade() else {
                    return;
                };
                let of = usize::try_from(source.selected())
                    .ok()
                    .and_then(|index| sources.get(index))
                    .and_then(|(id, _)| id.clone());
                let fits = usize::try_from(scaling.selected())
                    .ok()
                    .and_then(|index| Scaling::ALL.get(index).copied())
                    .unwrap_or_default();
                model.set_mirror(&target, of.map(|of| Mirror { of, scaling: fits }));
            }
        };
        let send = Rc::new(send);
        source.connect_selected_notify(glib::clone!(
            #[strong]
            send,
            #[weak]
            scaling,
            move |source| send(source, &scaling)
        ));
        scaling.connect_selected_notify(glib::clone!(
            #[strong]
            send,
            #[weak]
            source,
            move |scaling| send(&source, scaling)
        ));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.append(&title);
        row.append(&source);
        row.append(&scaling);
        row
    }
}

impl Default for Mirroring {
    fn default() -> Self {
        Self::new()
    }
}
