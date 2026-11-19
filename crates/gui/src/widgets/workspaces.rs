// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The map of the Workspaces page: which workspace goes on which display, for the displays
//! connected now.
//!
//! A card per display that takes workspaces, from left to right, holds the numbers the
//! placement rule gives it ([`Model::placement`]). A click on a number sends it to the next
//! display; dragging it onto another card puts it there. Either makes the placement fixed for
//! this combination of displays ([`Model::fix_placement`]), starting from what the rule gave.
//! While sway places the workspaces itself, the map is hidden.

use std::collections::BTreeMap;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::outputs::workspaces::targets;

use super::displays::short_label;
use crate::i18n::{msg, tr, trf};
use crate::model::Model;
use crate::style;

/// The map, applied through `model`.
#[derive(Debug, Clone)]
pub struct WorkspaceMap {
    root: gtk::Box,
    cards: gtk::Box,
    note: gtk::Label,
}

impl WorkspaceMap {
    /// The map, following `model` until it is gone.
    pub fn new(model: &Model) -> Self {
        let cards = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        cards.set_homogeneous(true);
        let note = gtk::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class(style::DIM);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.add_css_class("st-workspace-map");
        root.append(&cards);
        root.append(&note);
        let map = Self { root, cards, note };
        map.update(model);
        let weak = map.root.downgrade();
        let shown = map.clone();
        let handler = std::cell::RefCell::new(Some(model.connect_changed(move |model| {
            if weak.upgrade().is_some() {
                shown.update(model);
            }
        })));
        map.root.connect_destroy(glib::clone!(
            #[weak]
            model,
            move |_| {
                if let Some(handler) = handler.take() {
                    model.disconnect(handler);
                }
            }
        ));
        map
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// Shows the placement of `model` for the displays connected now.
    fn update(&self, model: &Model) {
        while let Some(child) = self.cards.first_child() {
            self.cards.remove(&child);
        }
        let placement = model.placement().unwrap_or_default();
        let outputs = model.outputs().unwrap_or_default();
        let mirrors: Vec<String> = model.mirrors().unwrap_or_default().into_keys().collect();
        let shown = targets(&outputs, &mirrors);
        let labels: Vec<String> = shown.iter().map(|output| short_label(output)).collect();
        // Two displays of a brand are told apart by their connectors.
        let displays: Vec<(String, String)> = shown
            .iter()
            .zip(&labels)
            .map(|(output, label)| {
                let twins = labels.iter().filter(|other| *other == label).count() > 1;
                let label = if twins {
                    format!("{label} \u{b7} {}", output.name())
                } else {
                    label.clone()
                };
                (output.identifier().to_owned(), label)
            })
            .collect();
        let places = !placement.is_empty();
        self.root.set_visible(places || model.outputs().is_none());
        if model.outputs().is_none() || displays.is_empty() {
            self.note.set_text(tr(msg!(
                "The displays show here while Sway Things runs in a sway session."
            )));
            return;
        }
        if displays.len() < 2 {
            self.note.set_text(tr(msg!(
                "With one display, every workspace is on it. Connect another one to arrange them."
            )));
            return;
        }
        self.note.set_text(tr(msg!(
            "Click a number to send it to the next display, or drag it to another one. That \
             makes the placement fixed: remembered for these displays, while other combinations \
             start round robin."
        )));
        for (index, (display, shown)) in displays.iter().enumerate() {
            let next = displays
                .get((index + 1) % displays.len())
                .map(|(id, _)| id.clone())
                .unwrap_or_default();
            self.cards
                .append(&card(model, display, shown, &next, &placement));
        }
    }
}

/// The names of the workspaces of `placement`, in the order of their numbers.
fn in_order(placement: &BTreeMap<String, String>) -> Vec<&String> {
    let mut names: Vec<&String> = placement.keys().collect();
    names.sort_by_key(|name| name.parse::<u32>().unwrap_or(u32::MAX));
    names
}

/// The card of `display`, titled `shown`, with the numbers of its workspaces in `placement`. A
/// click on a number sends it to `next`.
fn card(
    model: &Model,
    display: &str,
    shown: &str,
    next: &str,
    placement: &BTreeMap<String, String>,
) -> gtk::Box {
    let title = gtk::Label::new(Some(shown));
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("st-row-title");
    let numbers = gtk::FlowBox::new();
    numbers.set_selection_mode(gtk::SelectionMode::None);
    numbers.set_max_children_per_line(5);
    numbers.set_min_children_per_line(1);
    for name in in_order(placement)
        .into_iter()
        .filter(|name| placement.get(*name).map(String::as_str) == Some(display))
    {
        let button = gtk::Button::with_label(name);
        button.set_tooltip_text(Some(tr(msg!(
            "Click to send it to the next display, or drag it"
        ))));
        button.update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("Workspace {name}, on {display}"),
            &[("name", name), ("display", &shown)],
        ))]);
        let (workspace, to) = (name.clone(), next.to_owned());
        button.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |_| model.fix_placement(&workspace, &to)
        ));
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::MOVE);
        let payload = name.clone();
        source.connect_prepare(move |_, _, _| {
            Some(gdk::ContentProvider::for_value(&payload.to_value()))
        });
        button.add_controller(source);
        numbers.insert(&button, -1);
    }
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.add_css_class("st-workspace-card");
    card.append(&title);
    card.append(&numbers);
    let target = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    let display = display.to_owned();
    target.connect_drop(glib::clone!(
        #[weak]
        model,
        #[upgrade_or]
        false,
        move |_, value, _, _| {
            let Ok(name) = value.get::<String>() else {
                return false;
            };
            model.fix_placement(&name, &display);
            true
        }
    ));
    card.add_controller(target);
    card
}
