// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows of the printers: one line per printer, made once and updated in place from each
//! snapshot. A line holds the printer's icon, its title with a _Default_ badge, its state in
//! words (with the jobs waiting), its ink or toner as small bars, a warning sign when it can't
//! print, and _Resume_ while it is paused. A click on the line opens its details
//! ([`super::details`]), where everything else is.

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::printers::Printer;

use super::{PrinterRow, PrintersPage, set_class, supplies, title_of};
use crate::i18n::{msg, msg_plural, ntrf, tr, trf};
use crate::style;
use crate::widgets::list::{arrange, remove_row, set_hint};

impl PrintersPage {
    /// Updates the rows, and the open details dialog, from the snapshot.
    pub(super) fn render(&self) {
        let imp = self.imp();
        let guard = imp.snapshot.borrow();
        let Some(snapshot) = guard.as_ref() else {
            return;
        };
        let default = snapshot.effective_default().map(ToOwned::to_owned);
        let mut rows = Vec::new();
        for printer in snapshot.listed() {
            let row = self.ensure_row(&printer.name);
            let jobs = snapshot.jobs_of(&printer.name).count();
            let is_default = default
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(&printer.name));
            self.update_row(&row, printer, is_default, jobs);
            rows.push(row.row.clone());
        }
        imp.rows.borrow_mut().retain(|name, row| {
            let keep = snapshot.listed().any(|printer| &printer.name == name);
            if !keep {
                remove_row(&row.row);
            }
            keep
        });
        arrange(&imp.printers, &rows);
        imp.printers.set_visible(!rows.is_empty());
        set_hint(
            &imp.printers_hint,
            rows.is_empty().then(|| {
                tr(msg!(
                    "No printers yet. Add one with Add Printer…, or plug one in: CUPS may set up \
                     a driverless printer by itself."
                ))
            }),
        );
        let note = match (&snapshot.user_default, &snapshot.default) {
            (Some(own), all) if all.as_deref() != Some(own.as_str()) => Some(trf(
                msg!(
                    "“{printer}” is your own default printer, which apps use: it is set in \
                     ~/.cups/lpoptions (lpoptions -d). Set as Default changes the default for \
                     every user."
                ),
                &[("printer", &title_of(snapshot, own))],
            )),
            _ => None,
        };
        imp.note.set_text(note.as_deref().unwrap_or_default());
        imp.note.set_visible(note.is_some());
        drop(guard);
        self.update_details();
    }

    /// The row of the printer `name`, made if it isn't there yet.
    pub(super) fn ensure_row(&self, name: &str) -> PrinterRow {
        if let Some(row) = self.imp().rows.borrow().get(name) {
            return row.clone();
        }
        let row = self.new_row(name);
        self.imp()
            .rows
            .borrow_mut()
            .insert(name.to_owned(), row.clone());
        row
    }

    /// A new row for the printer `name`.
    pub(super) fn new_row(&self, name: &str) -> PrinterRow {
        let title = gtk::Label::new(None);
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.add_css_class("st-row-title");
        let badge = gtk::Label::new(Some(tr(msg!("Default"))));
        badge.add_css_class("st-device-badge");
        badge.set_valign(gtk::Align::Center);
        let title_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        title_line.append(&title);
        title_line.append(&badge);

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status.add_css_class("st-device-subtitle");
        let problem = gtk::Image::from_icon_name("dialog-warning-symbolic");
        problem.set_accessible_role(gtk::AccessibleRole::Presentation);
        problem.add_css_class("st-device-warning");
        problem.set_pixel_size(14);
        problem.set_visible(false);
        let status_line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        status_line.append(&problem);
        status_line.append(&status);

        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        texts.set_hexpand(true);
        texts.set_valign(gtk::Align::Center);
        texts.append(&title_line);
        texts.append(&status_line);

        let levels = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        levels.set_valign(gtk::Align::Center);
        levels.add_css_class("st-printer-levels");

        let resume = gtk::Button::with_label(tr(msg!("Resume")));
        resume.set_valign(gtk::Align::Center);
        let id = name.to_owned();
        let weak = self.downgrade();
        resume.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.resume(&id);
            }
        });

        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        line.add_css_class("st-device-row");
        line.append(&device_tile("printer-symbolic"));
        line.append(&texts);
        line.append(&levels);
        line.append(&resume);
        line.append(&chevron());
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        row.set_activatable(true);
        row.set_widget_name(name);
        row.set_tooltip_text(Some(tr(msg!("Show the details"))));
        PrinterRow {
            row,
            title,
            badge,
            status,
            problem,
            levels,
            resume,
        }
    }

    /// Fills `row` from `printer`, the default printer if `is_default`, with `jobs` jobs.
    pub(super) fn update_row(
        &self,
        row: &PrinterRow,
        printer: &Printer,
        is_default: bool,
        jobs: usize,
    ) {
        let running = self.imp().running.borrow().get(&printer.name).copied();
        row.title.set_text(printer.title());
        row.badge.set_visible(is_default);
        let status = running.map_or_else(|| status_with_jobs(printer, jobs), ToOwned::to_owned);
        row.status.set_text(&status);
        // A printer that can't print says so with a sign in the warning color, and its state in
        // the text color: the theme's error color is too faint on a list in some themes.
        let problem = running.is_none() && printer.problem().is_some();
        row.problem.set_visible(problem);
        set_class(&row.status, style::DIM, !problem);
        supplies::fill_compact(&row.levels, &printer.markers);
        row.resume
            .set_visible(printer.paused() || !printer.accepting);
        row.resume.set_sensitive(running.is_none());
        row.resume
            .update_property(&[gtk::accessible::Property::Label(&trf(
                msg!("Resume {printer}"),
                &[("printer", &printer.title())],
            ))]);
        let mut said = vec![status];
        if is_default {
            said.push(tr(msg!("Default printer")).to_owned());
        }
        said.extend(printer.markers.iter().map(|marker| {
            format!(
                "{}: {}",
                supplies::name_of(marker),
                supplies::level_text(marker)
            )
        }));
        row.row.update_property(&[
            gtk::accessible::Property::Label(printer.title()),
            gtk::accessible::Property::Description(&said.join(". ")),
        ]);
    }
}

/// The state of `printer` in words, with the jobs waiting for it: `Paused · Toner low · 2 jobs`.
pub(super) fn status_with_jobs(printer: &Printer, jobs: usize) -> String {
    let status = printer.status().translated();
    if jobs == 0 {
        return status;
    }
    let jobs = ntrf(
        msg_plural!("{count} job", "{count} jobs"),
        jobs,
        &[("count", &jobs)],
    );
    format!("{status} · {jobs}")
}

/// The icon `name` of a device on a rounded tile, as the lines of printers and scanners start.
pub fn device_tile(name: &str) -> gtk::Box {
    let icon = gtk::Image::from_icon_name(name);
    icon.set_pixel_size(20);
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    icon.set_hexpand(true);
    let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tile.add_css_class("st-device-tile");
    tile.set_size_request(36, 36);
    // The icon's expansion centers it; the tile itself stays its size.
    tile.set_hexpand(false);
    tile.set_valign(gtk::Align::Center);
    tile.append(&icon);
    tile
}

/// The sign at the end of a line that opens a dialog.
pub fn chevron() -> gtk::Image {
    let next = gtk::Image::from_icon_name("go-next-symbolic");
    next.set_accessible_role(gtk::AccessibleRole::Presentation);
    next.add_css_class(style::DIM);
    next.set_valign(gtk::Align::Center);
    next
}
