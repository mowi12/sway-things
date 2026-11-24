// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The details of a printer, opened by a click on its line, and kept up to date from each
//! snapshot while open:
//!
//! - the printer's icon, title and state in words, with the _Default_ badge;
//! - what can be done with it: _Set as Default_, _Pause_ or _Resume_, _Print Test Page_;
//! - its ink or toner, a bar with name and level each;
//! - its print queue, a line per job with _Cancel_, and _Cancel All_;
//! - its description and location, which are saved when Enter is pressed in them or the dialog
//!   closes, and whether it is shared, which is saved at once;
//! - the technical details: the queue's name, the model (CUPS's driver) and the address;
//! - _Remove Printer…_.
//!
//! Printer names, models, locations, job titles and owners come from printers and other users:
//! they only ever go into label text and entries.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::printers::{Job, JobState, Printer};

use super::{PrintersPage, set_class, supplies};
use crate::i18n::{Msg, msg, tr, trf};
use crate::style;
use crate::widgets::list::{arrange, heading, list_row, remove_row, row, section_list, set_hint};

/// What the details change of a printer: `None` for what stays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailsChange {
    /// The printer's name.
    pub name: String,
    /// Its new description.
    pub info: Option<String>,
    /// Its new location.
    pub location: Option<String>,
    /// Whether to share it now.
    pub shared: Option<bool>,
}

impl DetailsChange {
    /// Whether it changes anything.
    pub fn any(&self) -> bool {
        self.info.is_some() || self.location.is_some() || self.shared.is_some()
    }
}

/// The widgets of a job's line.
#[derive(Debug, Clone)]
struct JobRow {
    row: gtk::ListBoxRow,
    title: gtk::Label,
    subtitle: gtk::Label,
    cancel: gtk::Button,
}

/// The open details of a printer.
#[derive(Clone)]
pub struct DetailsDialog {
    /// Its window.
    pub window: gtk::Window,
    /// The name of the printer.
    pub name: String,
    parts: Rc<Parts>,
}

impl std::fmt::Debug for DetailsDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DetailsDialog")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// The parts of the dialog that change.
struct Parts {
    heading: gtk::Label,
    badge: gtk::Label,
    status: gtk::Label,
    problem: gtk::Image,
    error: gtk::Label,
    set_default: gtk::Button,
    pause: gtk::Button,
    resume: gtk::Button,
    test_page: gtk::Button,
    supplies_heading: gtk::Label,
    supplies: gtk::Box,
    /// The heading of the print queue, with _Cancel All_: what [`DetailsDialog::show_queue`]
    /// scrolls to.
    jobs_line: gtk::Box,
    jobs: gtk::ListBox,
    jobs_hint: gtk::Label,
    cancel_all: gtk::Button,
    job_rows: RefCell<HashMap<i32, JobRow>>,
    info: gtk::Entry,
    location: gtk::Entry,
    shared: gtk::Switch,
    model: gtk::Label,
    address: gtk::Label,
    queue: gtk::Label,
    /// The description, location and sharing as CUPS has them, to tell what changed.
    saved: RefCell<(String, String, bool)>,
    page: glib::WeakRef<PrintersPage>,
}

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// A job without a title.
const UNTITLED: Msg = msg!("Untitled document");

impl DetailsDialog {
    /// Opens the details of `printer` over `parent`, for `page`, which does what its buttons ask.
    #[expect(
        clippy::too_many_lines,
        reason = "the parts of the dialog from top to bottom, in the order they show"
    )]
    pub(super) fn open(
        parent: Option<&gtk::Window>,
        page: &PrintersPage,
        printer: &Printer,
    ) -> Self {
        let window = gtk::Window::builder()
            .modal(true)
            .resizable(true)
            .destroy_with_parent(true)
            .default_width(500)
            .title(printer.title())
            .build();
        window.set_transient_for(parent);
        window.add_css_class("st-dialog");
        window.add_css_class("st-device-dialog");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(24);
        content.set_margin_bottom(16);
        content.set_margin_start(20);
        content.set_margin_end(20);

        let icon = gtk::Image::from_icon_name("printer-symbolic");
        icon.set_pixel_size(48);
        icon.add_css_class(style::DIM);
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        content.append(&icon);
        let heading_label = gtk::Label::new(Some(printer.title()));
        heading_label.set_wrap(true);
        heading_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        heading_label.set_justify(gtk::Justification::Center);
        heading_label.set_max_width_chars(36);
        heading_label.add_css_class("title-2");
        heading_label.set_accessible_role(gtk::AccessibleRole::Heading);
        content.append(&heading_label);
        let problem = gtk::Image::from_icon_name("dialog-warning-symbolic");
        problem.add_css_class("st-device-warning");
        problem.set_accessible_role(gtk::AccessibleRole::Presentation);
        problem.set_pixel_size(14);
        let status = gtk::Label::new(None);
        status.set_wrap(true);
        status.set_justify(gtk::Justification::Center);
        let badge = gtk::Label::new(Some(tr(msg!("Default"))));
        badge.add_css_class("st-device-badge");
        badge.set_valign(gtk::Align::Center);
        let status_line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        status_line.set_halign(gtk::Align::Center);
        status_line.append(&problem);
        status_line.append(&status);
        status_line.append(&badge);
        content.append(&status_line);

        let error = gtk::Label::new(None);
        error.set_wrap(true);
        error.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        error.set_xalign(0.0);
        error.add_css_class("error");
        error.set_visible(false);
        content.append(&error);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::Center);
        actions.set_margin_top(4);
        actions.add_css_class("st-device-actions");
        let set_default = gtk::Button::with_label(tr(msg!("Set as Default")));
        let pause = gtk::Button::with_label(tr(msg!("Pause")));
        let resume = gtk::Button::with_label(tr(msg!("Resume")));
        let test_page = gtk::Button::with_label(tr(msg!("Print Test Page")));
        for button in [&set_default, &pause, &resume, &test_page] {
            actions.append(button);
        }
        content.append(&actions);

        let supplies_heading = heading(tr(msg!("Ink and toner")), false);
        supplies_heading.set_margin_top(8);
        supplies_heading.set_margin_bottom(0);
        content.append(&supplies_heading);
        let supplies = gtk::Box::new(gtk::Orientation::Vertical, 4);
        supplies.set_margin_start(4);
        content.append(&supplies);

        let jobs_heading = heading(tr(msg!("Print Queue")), false);
        jobs_heading.set_margin_top(8);
        jobs_heading.set_margin_bottom(0);
        let cancel_all = gtk::Button::with_label(tr(msg!("Cancel All")));
        cancel_all.add_css_class("flat");
        cancel_all.set_halign(gtk::Align::End);
        cancel_all.set_hexpand(true);
        let jobs_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        jobs_line.append(&jobs_heading);
        jobs_line.append(&cancel_all);
        content.append(&jobs_line);
        let jobs = section_list(tr(msg!("Print Queue")));
        content.append(&jobs);
        let jobs_hint = gtk::Label::new(None);
        jobs_hint.set_xalign(0.0);
        jobs_hint.set_margin_start(4);
        jobs_hint.add_css_class(style::DIM);
        content.append(&jobs_hint);

        let settings = heading(tr(msg!("Printer")), false);
        settings.set_margin_top(8);
        settings.set_margin_bottom(0);
        content.append(&settings);
        let grid = gtk::Grid::builder()
            .column_spacing(16)
            .row_spacing(8)
            .build();
        grid.add_css_class("st-device-facts");
        let info = entry_row(&grid, 0, tr(msg!("Description")), &printer.info);
        let location = entry_row(&grid, 1, tr(msg!("Location")), &printer.location);
        let share_label = fact_name(tr(msg!("Shared")));
        let shared = gtk::Switch::builder()
            .active(printer.shared)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .tooltip_text(tr(msg!(
                "Other computers see a shared printer while sharing is on for CUPS as a whole"
            )))
            .build();
        shared.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
            share_label.upcast_ref()
        ])]);
        grid.attach(&share_label, 0, 2, 1, 1);
        grid.attach(&shared, 1, 2, 1, 1);
        let model = fact_row(&grid, 3, tr(msg!("Model")));
        let address = fact_row(&grid, 4, tr(msg!("Address")));
        let queue = fact_row(&grid, 5, tr(msg!("Queue")));
        content.append(&grid);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_margin_top(12);
        let remove = gtk::Button::with_label(tr(msg!("Remove Printer…")));
        remove.add_css_class("destructive-action");
        buttons.append(&remove);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        buttons.append(&spacer);
        let close = gtk::Button::with_label(tr(msg!("Close")));
        buttons.append(&close);
        content.append(&buttons);

        let scroller = gtk::ScrolledWindow::builder()
            .child(&content)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(760)
            .build();
        window.set_child(Some(&scroller));

        let dialog = Self {
            window,
            name: printer.name.clone(),
            parts: Rc::new(Parts {
                heading: heading_label,
                badge,
                status,
                problem,
                error,
                set_default,
                pause,
                resume,
                test_page,
                supplies_heading,
                supplies,
                jobs_line,
                jobs,
                jobs_hint,
                cancel_all,
                job_rows: RefCell::new(HashMap::new()),
                info,
                location,
                shared,
                model,
                address,
                queue,
                saved: RefCell::new((
                    printer.info.clone(),
                    printer.location.clone(),
                    printer.shared,
                )),
                page: page.downgrade(),
            }),
        };
        dialog.connect(&remove, &close);
        GtkWindowExt::set_focus(&dialog.window, Some(&close));
        dialog
    }

    /// Connects the buttons, the fields and the ways of closing.
    fn connect(&self, remove: &gtk::Button, close: &gtk::Button) {
        let parts = &self.parts;
        let action = |button: &gtk::Button, run: fn(&PrintersPage, &str)| {
            let page = parts.page.clone();
            let name = self.name.clone();
            button.connect_clicked(move |_| {
                if let Some(page) = page.upgrade() {
                    run(&page, &name);
                }
            });
        };
        action(&parts.set_default, PrintersPage::set_default);
        action(&parts.pause, PrintersPage::pause);
        action(&parts.resume, PrintersPage::resume);
        action(&parts.test_page, PrintersPage::print_test_page);
        action(&parts.cancel_all, PrintersPage::cancel_all);
        action(remove, PrintersPage::remove);
        for entry in [&parts.info, &parts.location] {
            let dialog = self.clone();
            entry.connect_activate(move |_| dialog.save());
        }
        let dialog = self.clone();
        parts.shared.connect_state_set(move |_, _| {
            dialog.save();
            glib::Propagation::Proceed
        });
        let dialog = self.clone();
        close.connect_clicked(move |_| dialog.close());
        // The compositor's close and Escape: the changes are saved, and the page forgets the
        // dialog, before GTK destroys the window.
        let dialog = self.clone();
        self.window.connect_close_request(move |_| {
            dialog.closing();
            glib::Propagation::Proceed
        });
        let escape = gtk::ShortcutController::new();
        let dialog = self.clone();
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::CallbackAction::new(move |_, _| {
                dialog.close();
                glib::Propagation::Stop
            })),
        ));
        self.window.add_controller(escape);
    }

    /// Scrolls to the print queue and puts the focus there: on _Cancel All_, or on the first
    /// job. Once the dialog is laid out, so a dialog that just opened scrolls too.
    pub(super) fn show_queue(&self) {
        let parts = Rc::clone(&self.parts);
        let window = self.window.clone();
        glib::idle_add_local_once(move || {
            let scroller = window.child().and_downcast::<gtk::ScrolledWindow>();
            let content = scroller.as_ref().and_then(|scroller| {
                scroller
                    .child()
                    .and_then(|viewport| viewport.first_child().or(Some(viewport)))
            });
            if let (Some(scroller), Some(content)) = (scroller, content) {
                if let Some(point) = parts
                    .jobs_line
                    .compute_point(&content, &gtk::graphene::Point::new(0.0, 0.0))
                {
                    scroller.vadjustment().set_value(f64::from(point.y()));
                }
            }
            if parts.cancel_all.is_sensitive() {
                parts.cancel_all.grab_focus();
            } else if let Some(row) = parts.jobs.row_at_index(0) {
                row.grab_focus();
            }
        });
    }

    /// Saves what changed and closes the dialog.
    pub fn close(&self) {
        self.closing();
        self.window.destroy();
    }

    /// Saves what changed, and tells the page the dialog is gone.
    fn closing(&self) {
        self.save();
        if let Some(page) = self.parts.page.upgrade() {
            page.details_closed(&self.window);
        }
    }

    /// Sends what changed of the description, location and sharing to the page.
    fn save(&self) {
        let parts = &self.parts;
        let change = {
            let saved = parts.saved.borrow();
            let changed = |new: String, old: &str| (new != old).then_some(new);
            DetailsChange {
                name: self.name.clone(),
                info: changed(parts.info.text().trim().to_owned(), &saved.0),
                location: changed(parts.location.text().trim().to_owned(), &saved.1),
                shared: (parts.shared.is_active() != saved.2).then_some(parts.shared.is_active()),
            }
        };
        if !change.any() {
            return;
        }
        {
            let mut saved = parts.saved.borrow_mut();
            if let Some(info) = &change.info {
                saved.0.clone_from(info);
            }
            if let Some(location) = &change.location {
                saved.1.clone_from(location);
            }
            if let Some(shared) = change.shared {
                saved.2 = shared;
            }
        }
        if let Some(page) = parts.page.upgrade() {
            page.apply_details(change);
        }
    }

    /// Shows `printer` as it is now: the default if `is_default`, with `jobs`, while `running`
    /// (what the page does with it) runs.
    pub(super) fn update(
        &self,
        printer: &Printer,
        is_default: bool,
        jobs: &[&Job],
        running: Option<&'static str>,
    ) {
        let parts = &self.parts;
        parts.heading.set_text(printer.title());
        self.window.set_title(Some(printer.title()));
        parts.badge.set_visible(is_default);
        let status = running.map_or_else(|| printer.status().translated(), ToOwned::to_owned);
        parts.status.set_text(&status);
        let problem = running.is_none() && printer.problem().is_some();
        parts.problem.set_visible(problem);
        set_class(&parts.status, style::DIM, !problem);
        let idle = running.is_none();
        parts
            .set_default
            .set_visible(!is_default && !printer.temporary);
        let paused = printer.paused() || !printer.accepting;
        parts.pause.set_visible(!paused);
        parts.resume.set_visible(paused);
        for button in [
            &parts.set_default,
            &parts.pause,
            &parts.resume,
            &parts.test_page,
            &parts.cancel_all,
        ] {
            button.set_sensitive(idle);
        }
        supplies::fill(&parts.supplies, &printer.markers);
        parts
            .supplies_heading
            .set_visible(!printer.markers.is_empty());
        parts.model.set_text(&printer.model);
        parts.address.set_text(&printer.device_uri);
        parts.queue.set_text(&printer.name);
        self.update_jobs(jobs);
    }

    /// Updates the lines of the jobs.
    fn update_jobs(&self, jobs: &[&Job]) {
        let parts = &self.parts;
        let page = parts.page.upgrade();
        let mut rows = Vec::new();
        for job in jobs {
            let row = self.ensure_job_row(job.id);
            let running = page
                .as_ref()
                .and_then(|page| page.running_on(&format!("job:{}", job.id)));
            update_job_row(&row, job, running);
            rows.push(row.row.clone());
        }
        parts.job_rows.borrow_mut().retain(|id, row| {
            let keep = jobs.iter().any(|job| job.id == *id);
            if !keep {
                remove_row(&row.row);
            }
            keep
        });
        arrange(&parts.jobs, &rows);
        parts.jobs.set_visible(!rows.is_empty());
        parts.cancel_all.set_visible(rows.len() > 1);
        set_hint(
            &parts.jobs_hint,
            rows.is_empty()
                .then(|| tr(msg!("Nothing is waiting to print."))),
        );
    }

    /// The line of the job `id`, made if it isn't there yet.
    fn ensure_job_row(&self, id: i32) -> JobRow {
        if let Some(row) = self.parts.job_rows.borrow().get(&id) {
            return row.clone();
        }
        let title = gtk::Label::new(None);
        let subtitle = gtk::Label::new(None);
        subtitle.add_css_class(style::DIM);
        let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
        cancel.set_valign(gtk::Align::Center);
        let page = self.parts.page.clone();
        cancel.connect_clicked(move |_| {
            if let Some(page) = page.upgrade() {
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
        self.parts.job_rows.borrow_mut().insert(id, row.clone());
        row
    }

    /// Shows `message`, or hides it when empty.
    pub(super) fn show_error(&self, message: &str) {
        self.parts.error.set_text(message);
        self.parts.error.set_visible(!message.is_empty());
    }

    /// The jobs as listed: id, title and subtitle each; for tests.
    pub fn job_rows(&self) -> Vec<(i32, String, String)> {
        let rows = self.parts.job_rows.borrow();
        let mut listed = Vec::new();
        let mut child = self.parts.jobs.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Some((id, row)) = rows.iter().find(|(_, row)| widget == row.row) {
                listed.push((
                    *id,
                    row.title.text().to_string(),
                    row.subtitle.text().to_string(),
                ));
            }
        }
        listed
    }

    /// The _Cancel_ button of the job `id`; for tests.
    pub fn cancel_button(&self, id: i32) -> Option<gtk::Button> {
        self.parts
            .job_rows
            .borrow()
            .get(&id)
            .map(|row| row.cancel.clone())
    }

    /// The fields of the description and the location, and the switch that shares the printer;
    /// for tests.
    pub fn fields(&self) -> (gtk::Entry, gtk::Entry, gtk::Switch) {
        (
            self.parts.info.clone(),
            self.parts.location.clone(),
            self.parts.shared.clone(),
        )
    }

    /// The status line as it reads; for tests.
    pub fn status_text(&self) -> String {
        self.parts.status.text().to_string()
    }
}

/// Fills `row` from `job`, while `running` (what the page does with it) runs.
fn update_job_row(row: &JobRow, job: &Job, running: Option<&'static str>) {
    let title = if job.title.is_empty() {
        tr(UNTITLED).to_owned()
    } else {
        job.title.clone()
    };
    row.title.set_text(&title);
    let mut subtitle = Vec::new();
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

/// The name of a fact or field.
fn fact_name(text: &str) -> gtk::Label {
    let name = gtk::Label::new(Some(text));
    name.set_xalign(0.0);
    name.add_css_class(style::DIM);
    name
}

/// A fact named `name` in line `line` of `grid`: its value, selectable.
fn fact_row(grid: &gtk::Grid, line: i32, name: &str) -> gtk::Label {
    let name = fact_name(name);
    name.set_yalign(0.0);
    let value = gtk::Label::new(None);
    value.set_xalign(0.0);
    value.set_selectable(true);
    value.set_wrap(true);
    value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    value.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
    value.set_hexpand(true);
    value.set_max_width_chars(40);
    value.update_relation(&[gtk::accessible::Relation::LabelledBy(&[name.upcast_ref()])]);
    grid.attach(&name, 0, line, 1, 1);
    grid.attach(&value, 1, line, 1, 1);
    value
}

/// A field named `name` in line `line` of `grid`, holding `text`.
fn entry_row(grid: &gtk::Grid, line: i32, name: &str, text: &str) -> gtk::Entry {
    let name = fact_name(name);
    let entry = gtk::Entry::builder().text(text).hexpand(true).build();
    entry.update_relation(&[gtk::accessible::Relation::LabelledBy(&[name.upcast_ref()])]);
    grid.attach(&name, 0, line, 1, 1);
    grid.attach(&entry, 1, line, 1, 1);
    entry
}
