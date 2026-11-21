// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The picker of the output the rows are for: a drop-down of the outputs by number, with every
//! output at once first.
use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::outputs::{Output, Outputs};

use super::ALL_DISPLAYS;
use crate::i18n::{msg, tr, trf};

/// The entry of an output in the picker: its number, make and model, and its name or that it
/// is off or not connected.
pub(super) fn entry(number: usize, output: &Output) -> String {
    let label = output.label();
    if !output.is_connected() {
        trf(
            // TRANSLATORS: A display in the drop-down of displays: its number and name.
            msg!("{number}  {label} (not connected)"),
            &[("number", &number), ("label", &label)],
        )
    } else if !output.is_enabled() {
        trf(
            // TRANSLATORS: A display in the drop-down of displays: its number, name and connector.
            msg!("{number}  {label} \u{b7} {name} (off)"),
            &[
                ("number", &number),
                ("label", &label),
                ("name", &output.name()),
            ],
        )
    } else if label == output.name() {
        format!("{number}  {label}")
    } else {
        format!("{number}  {label} \u{b7} {}", output.name())
    }
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
        let title = gtk::Label::new(Some(tr(msg!("Settings for"))));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_wrap(true);
        title.add_css_class("st-row-title");
        let labels = gtk::StringList::new(&[tr(ALL_DISPLAYS)]);
        let dropdown = gtk::DropDown::new(Some(labels.clone()), gtk::Expression::NONE);
        dropdown.set_valign(gtk::Align::Center);
        dropdown.update_property(&[gtk::accessible::Property::Label(tr(msg!("Settings for")))]);
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
    pub(super) fn update(&self, outputs: &Outputs) {
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
        let shown: Vec<String> = std::iter::once(tr(ALL_DISPLAYS).to_owned())
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
    pub(super) fn connect_picked<F: Fn(Option<&str>) + 'static>(&self, callback: F) {
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
    pub(super) fn downgrade(&self) -> WeakPicker {
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
pub(super) struct WeakPicker {
    root: glib::WeakRef<gtk::Box>,
    dropdown: glib::WeakRef<gtk::DropDown>,
    labels: gtk::StringList,
    identifiers: Rc<RefCell<Vec<String>>>,
    updating: Rc<std::cell::Cell<bool>>,
}

impl WeakPicker {
    pub(super) fn upgrade(&self) -> Option<OutputPicker> {
        Some(OutputPicker {
            root: self.root.upgrade()?,
            dropdown: self.dropdown.upgrade()?,
            labels: self.labels.clone(),
            identifiers: Rc::clone(&self.identifiers),
            updating: Rc::clone(&self.updating),
        })
    }
}
