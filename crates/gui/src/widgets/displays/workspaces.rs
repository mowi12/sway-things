// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The "Workspaces" section of the Displays page: which workspace goes on which display, for the
//! displays connected now (opt-in, with kanshi).
//!
//! A card per display that is on, from left to right, holds the numbers of its workspaces. A
//! click on a number sends it to the next display; dragging it onto another card puts it there.
//! The presets fill the cards as a whole. Each change is remembered for the combination and
//! applied at once ([`Model::place_workspaces`]).

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::outputs::Outputs;
use sway_things_core::outputs::workspaces::{NAMES, Preset, left_to_right};

use super::canvas::short_label;
use crate::model::{Model, Placement};

/// The section's contents.
#[derive(Debug, Clone)]
pub struct WorkspaceMap {
    root: gtk::Box,
    switch: gtk::Switch,
    presets: gtk::Box,
    cards: gtk::Box,
    note: gtk::Label,
    updating: Rc<Cell<bool>>,
}

impl WorkspaceMap {
    /// The map, applied through `model`.
    pub fn new(model: &Model) -> Self {
        let title = gtk::Label::new(Some("Place workspaces on the displays"));
        title.set_xalign(0.0);
        title.add_css_class("st-row-title");
        let subtitle = gtk::Label::new(Some(
            "Workspaces 1 to 10 go where you put them, remembered for each combination of \
             displays and placed again whenever it comes back.",
        ));
        subtitle.set_xalign(0.0);
        subtitle.set_wrap(true);
        subtitle.add_css_class("st-row-detail");
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        texts.set_hexpand(true);
        texts.append(&title);
        texts.append(&subtitle);
        let switch = gtk::Switch::new();
        switch.set_valign(gtk::Align::Center);
        switch.update_property(&[gtk::accessible::Property::Label(
            "Place workspaces on the displays",
        )]);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        header.append(&texts);
        header.append(&switch);

        let presets = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let cards = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        cards.set_homogeneous(true);
        let note = gtk::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("st-row-detail");

        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.add_css_class("st-display-workspaces");
        root.append(&header);
        root.append(&note);
        root.append(&presets);
        root.append(&cards);
        let map = Self {
            root,
            switch,
            presets,
            cards,
            note,
            updating: Rc::new(Cell::new(false)),
        };
        map.add_presets(model);
        let updating = Rc::clone(&map.updating);
        map.switch.connect_active_notify(glib::clone!(
            #[weak]
            model,
            move |switch| {
                if updating.get() {
                    return;
                }
                // On without a placement for these displays: the one the cards show.
                let empty = model
                    .placement()
                    .is_none_or(|(_, placement)| placement.is_empty());
                let placement = if switch.is_active() && empty {
                    let mirrors: Vec<String> =
                        model.mirrors().unwrap_or_default().into_keys().collect();
                    Placement::Of(
                        Preset::LeftToRight.place(&model.outputs().unwrap_or_default(), &mirrors),
                    )
                } else {
                    Placement::Kept
                };
                model.place_workspaces(switch.is_active(), placement);
            }
        ));
        map
    }

    fn add_presets(&self, model: &Model) {
        let buttons: [(&str, Option<Preset>); 3] = [
            ("Left to Right", Some(Preset::LeftToRight)),
            ("A Block per Display", Some(Preset::Blocks)),
            ("As They Are Now", None),
        ];
        for (label, preset) in buttons {
            let button = gtk::Button::with_label(label);
            button.connect_clicked(glib::clone!(
                #[weak]
                model,
                move |_| {
                    let mirrors: Vec<String> =
                        model.mirrors().unwrap_or_default().into_keys().collect();
                    let placement = match preset {
                        Some(preset) => Placement::Of(
                            preset.place(&model.outputs().unwrap_or_default(), &mirrors),
                        ),
                        None => Placement::AsNow,
                    };
                    model.place_workspaces(true, placement);
                }
            ));
            self.presets.append(&button);
        }
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// Shows the placement of `model` for the displays of `outputs`; `number` gives a display's
    /// number as on the canvas.
    pub fn update(&self, model: &Model, outputs: &Outputs, number: &dyn Fn(&str) -> Option<usize>) {
        let kanshi = model.kanshi_runs();
        let placement = model.placement();
        let (place, mut placed) = placement.clone().unwrap_or_default();
        self.updating.set(true);
        self.switch.set_active(place);
        self.updating.set(false);
        self.switch.set_sensitive(kanshi && placement.is_some());
        self.note.set_text(if kanshi {
            ""
        } else {
            "Needs kanshi, which remembers each combination of displays (see above)."
        });
        self.note.set_visible(!kanshi);
        let open = kanshi && place;
        self.presets.set_visible(open);
        self.cards.set_visible(open);
        while let Some(child) = self.cards.first_child() {
            self.cards.remove(&child);
        }
        if !open {
            return;
        }
        // A display that mirrors another takes no workspaces.
        let mirrors: Vec<String> = model.mirrors().unwrap_or_default().into_keys().collect();
        let displays: Vec<(String, String)> = left_to_right(outputs)
            .into_iter()
            .filter(|output| !mirrors.iter().any(|name| name == output.identifier()))
            .map(|output| {
                let shown = number(output.identifier()).map_or_else(
                    || output.label(),
                    |n| format!("{n}  {}", short_label(output)),
                );
                (output.identifier().to_owned(), shown)
            })
            .collect();
        // Workspaces not placed on a connected display go round the displays from left to right.
        let fallback = Preset::LeftToRight.place(outputs, &mirrors);
        for name in NAMES {
            let on_display = placed
                .get(name)
                .is_some_and(|display| displays.iter().any(|(id, _)| id == display));
            if !on_display {
                if let Some(display) = fallback.get(name) {
                    placed.insert(name.to_owned(), display.clone());
                }
            }
        }
        for (index, (display, shown)) in displays.iter().enumerate() {
            let next = displays
                .get((index + 1) % displays.len())
                .map(|(id, _)| id.clone())
                .unwrap_or_default();
            self.cards
                .append(&card(model, display, shown, &next, &placed));
        }
    }
}

/// The card of `display`, titled `shown`, with the numbers of its workspaces in `placed`. A click
/// on a number sends it to `next`.
fn card(
    model: &Model,
    display: &str,
    shown: &str,
    next: &str,
    placed: &BTreeMap<String, String>,
) -> gtk::Box {
    let title = gtk::Label::new(Some(shown));
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("st-row-title");
    let numbers = gtk::FlowBox::new();
    numbers.set_selection_mode(gtk::SelectionMode::None);
    numbers.set_max_children_per_line(5);
    numbers.set_min_children_per_line(1);
    for name in NAMES
        .iter()
        .filter(|name| placed.get(**name).map(String::as_str) == Some(display))
    {
        let button = gtk::Button::with_label(name);
        button.set_tooltip_text(Some("Click to send it to the next display, or drag it"));
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Workspace {name}, on {shown}"
        ))]);
        let moved = |target: &str| {
            let mut placement = placed.clone();
            placement.insert((*name).to_owned(), target.to_owned());
            Placement::Of(placement)
        };
        let to_next = moved(next);
        button.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |_| model.place_workspaces(true, to_next.clone())
        ));
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::MOVE);
        let payload = (*name).to_owned();
        source.connect_prepare(move |_, _, _| {
            Some(gdk::ContentProvider::for_value(&payload.to_value()))
        });
        button.add_controller(source);
        numbers.insert(&button, -1);
    }
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.add_css_class("st-display-workspace-card");
    card.append(&title);
    card.append(&numbers);
    let target = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    let display = display.to_owned();
    let placed = placed.clone();
    target.connect_drop(glib::clone!(
        #[weak]
        model,
        #[upgrade_or]
        false,
        move |_, value, _, _| {
            let Ok(name) = value.get::<String>() else {
                return false;
            };
            let mut placement = placed.clone();
            placement.insert(name, display.clone());
            model.place_workspaces(true, Placement::Of(placement));
            true
        }
    ));
    card.add_controller(target);
    card
}
