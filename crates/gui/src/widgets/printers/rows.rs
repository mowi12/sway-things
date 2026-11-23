// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows of the Printers page: a row per printer with its state, supplies and ⋮ menu, and a
//! row per job, made once and updated in place from each snapshot.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::printers::{Job, JobState, Printer};

use super::{JobRow, PrinterRow, PrintersPage, set_class, supplies, title_of};
use crate::i18n::{Msg, msg, tr, trf};
use crate::style;
use crate::widgets::list::{arrange, list_row, remove_row, row, set_hint};

impl PrintersPage {
    /// Updates the rows from the snapshot.
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
        self.render_jobs();
    }

    /// Updates the rows of the jobs from the snapshot.
    pub(super) fn render_jobs(&self) {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return;
        };
        let mut rows = Vec::new();
        for job in &snapshot.jobs {
            let row = self.ensure_job_row(job.id);
            self.update_job_row(&row, job, &title_of(snapshot, &job.printer));
            rows.push(row.row.clone());
        }
        imp.job_rows.borrow_mut().retain(|id, row| {
            let keep = snapshot.jobs.iter().any(|job| job.id == *id);
            if !keep {
                remove_row(&row.row);
            }
            keep
        });
        arrange(&imp.jobs, &rows);
        imp.jobs.set_visible(!rows.is_empty());
        set_hint(
            &imp.jobs_hint,
            rows.is_empty()
                .then(|| tr(msg!("Nothing is waiting to print."))),
        );
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
        let icon = gtk::Image::from_icon_name("printer-symbolic");
        icon.set_pixel_size(32);
        icon.set_valign(gtk::Align::Start);
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        let title = gtk::Label::new(None);
        let subtitle = gtk::Label::new(None);
        subtitle.add_css_class(style::DIM);
        let status = gtk::Label::new(None);
        status.set_wrap(true);
        status.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        status.set_xalign(0.0);
        let supplies = gtk::Box::new(gtk::Orientation::Vertical, 2);
        supplies.set_margin_top(4);
        let resume = gtk::Button::with_label(tr(msg!("Resume")));
        resume.set_valign(gtk::Align::Center);
        let id = name.to_owned();
        let weak = self.downgrade();
        resume.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.resume(&id);
            }
        });
        let (menu, items) = self.menu(name);
        let [set_default, pause, cancel_all] = items;
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        for label in [&title, &subtitle] {
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(label);
        }
        let problem = gtk::Image::from_icon_name("dialog-warning-symbolic");
        problem.set_accessible_role(gtk::AccessibleRole::Presentation);
        problem.set_valign(gtk::Align::Start);
        problem.set_visible(false);
        let status_line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        status_line.append(&problem);
        status_line.append(&status);
        text.append(&status_line);
        text.append(&supplies);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        line.append(&icon);
        line.append(&text);
        line.append(&resume);
        line.append(&menu);
        PrinterRow {
            row: list_row(&line),
            title,
            subtitle,
            status,
            problem,
            supplies,
            resume,
            menu,
            set_default,
            pause,
            cancel_all,
        }
    }

    /// The ⋮ menu of the printer `name`, with the items that come and go: Set as Default, Pause
    /// and Cancel All Jobs.
    pub(super) fn menu(&self, name: &str) -> (gtk::MenuButton, [gtk::Button; 3]) {
        let menu_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&menu_box));
        let item = |label: Msg, action: fn(&Self, &str)| {
            let item = gtk::Button::with_label(tr(label));
            item.add_css_class("flat");
            if let Some(child) = item.child().and_downcast::<gtk::Label>() {
                child.set_xalign(0.0);
            }
            let weak = self.downgrade();
            let id = name.to_owned();
            item.connect_clicked(glib::clone!(
                #[weak]
                popover,
                move |_| {
                    popover.popdown();
                    if let Some(page) = weak.upgrade() {
                        action(&page, &id);
                    }
                }
            ));
            menu_box.append(&item);
            item
        };
        let set_default = item(msg!("Set as Default"), Self::set_default);
        let pause = item(msg!("Pause"), Self::pause);
        item(msg!("Print Test Page"), Self::print_test_page);
        let cancel_all = item(msg!("Cancel All Jobs"), Self::cancel_all);
        item(msg!("Details…"), Self::show_details);
        item(msg!("Remove…"), Self::remove);
        let menu = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .popover(&popover)
            .valign(gtk::Align::Center)
            .tooltip_text(tr(msg!("More actions")))
            .build();
        menu.add_css_class("flat");
        (menu, [set_default, pause, cancel_all])
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
        let mut subtitle = Vec::new();
        if is_default {
            subtitle.push(tr(msg!("Default printer")).to_owned());
        }
        if printer.temporary {
            subtitle.push(tr(msg!("Set up for a print job only")).to_owned());
        }
        for part in [&printer.model, &printer.location] {
            if !part.is_empty() {
                subtitle.push(part.clone());
            }
        }
        if printer.shared {
            subtitle.push(tr(msg!("Shared")).to_owned());
        }
        row.subtitle.set_text(&subtitle.join(" · "));
        row.subtitle.set_visible(!subtitle.is_empty());
        let status = running.map_or_else(|| printer.status().translated(), ToOwned::to_owned);
        row.status.set_text(&status);
        // A printer that can't print says so in the text color, with a sign: the theme's error
        // color is too faint on a list in some themes.
        let problem = running.is_none() && printer.problem().is_some();
        row.problem.set_visible(problem);
        set_class(&row.status, style::DIM, !problem);
        supplies::fill(&row.supplies, &printer.markers);
        row.resume
            .set_visible(printer.paused() || !printer.accepting);
        row.set_default
            .set_visible(!is_default && !printer.temporary);
        row.pause.set_visible(!printer.paused());
        row.cancel_all.set_visible(jobs > 0);
        let idle = running.is_none();
        row.resume.set_sensitive(idle);
        row.menu.set_sensitive(idle);
        row.menu
            .update_property(&[gtk::accessible::Property::Label(&trf(
                msg!("More actions for {printer}"),
                &[("printer", &printer.title())],
            ))]);
        row.resume
            .update_property(&[gtk::accessible::Property::Label(&trf(
                msg!("Resume {printer}"),
                &[("printer", &printer.title())],
            ))]);
    }

    /// The row of the job `id`, made if it isn't there yet.
    pub(super) fn ensure_job_row(&self, id: i32) -> JobRow {
        if let Some(row) = self.imp().job_rows.borrow().get(&id) {
            return row.clone();
        }
        let title = gtk::Label::new(None);
        let subtitle = gtk::Label::new(None);
        subtitle.add_css_class(style::DIM);
        let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
        cancel.set_valign(gtk::Align::Center);
        let weak = self.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.cancel_job(id);
            }
        });
        let line = row(&title, &subtitle, &[cancel.upcast_ref()]);
        let row = JobRow {
            row: list_row(&line),
            title,
            subtitle,
            cancel,
        };
        self.imp().job_rows.borrow_mut().insert(id, row.clone());
        row
    }

    /// Fills `row` from `job`, which waits for the printer titled `printer`.
    pub(super) fn update_job_row(&self, row: &JobRow, job: &Job, printer: &str) {
        let running = self
            .imp()
            .running
            .borrow()
            .get(&format!("job:{}", job.id))
            .copied();
        let title = if job.title.is_empty() {
            tr(msg!("Untitled document")).to_owned()
        } else {
            job.title.clone()
        };
        row.title.set_text(&title);
        let mut subtitle = vec![printer.to_owned()];
        if !job.owner.is_empty() {
            subtitle.push(job.owner.clone());
        }
        subtitle.push(running.map_or_else(|| job_state(job.state).to_owned(), ToOwned::to_owned));
        if let Some(time) = job.created.and_then(time_text) {
            subtitle.push(time);
        }
        row.subtitle.set_text(&subtitle.join(" · "));
        row.cancel.set_sensitive(running.is_none());
        row.cancel
            .update_property(&[gtk::accessible::Property::Label(&trf(
                msg!("Cancel {job}"),
                &[("job", &title)],
            ))]);
    }
}

/// The state of a job in words.
fn job_state(state: JobState) -> &'static str {
    tr(state.text())
}

/// When a job was sent, `created` in seconds since the epoch: the time today, else the date and
/// time, in the user's locale.
fn time_text(created: i64) -> Option<String> {
    let time = glib::DateTime::from_unix_local(created).ok()?;
    let now = glib::DateTime::now_local().ok()?;
    let format = if time.ymd() == now.ymd() {
        "%X"
    } else {
        "%x %X"
    };
    time.format(format).ok().map(|text| text.to_string())
}
