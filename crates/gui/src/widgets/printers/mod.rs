// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Printers page: the print queues with their state and supplies, and the jobs, live from
//! CUPS; adding a driverless printer, and what can be done with a printer and a job.
//!
//! The page reads a new snapshot every [`POLL`] while it is on screen, at once when it comes on
//! screen, and after each action, and updates its rows in place. Changes that need an
//! administrator go to CUPS as the user, then to cups-pk-helper (polkit); when neither may, the
//! error says what to install or which group to join.
//!
//! Printer names, models, locations, job titles and owners come from printers and other users:
//! they are only ever set as plain label text.

mod dialogs;
mod supplies;

use std::path::PathBuf;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::delegate;
use sway_things_core::printers::{Bus, Cups, Endpoint, Error, Job, JobState, Printer, Snapshot};

use super::{Placeholder, search_button};
use crate::i18n::{Msg, Text, msg, tr, trf};
use crate::style;

/// How often the page reads CUPS's state while it is on screen.
const POLL: Duration = Duration::from_secs(2);

/// Where the page finds CUPS.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Service {
    /// The CUPS of this computer, with cups-pk-helper and Avahi on the system bus.
    #[default]
    System,
    /// A CUPS listening on `socket`, with stand-ins for cups-pk-helper and Avahi on the bus at
    /// `bus`, if given (tests).
    At {
        /// The domain socket of CUPS.
        socket: PathBuf,
        /// The address of the bus with cups-pk-helper and Avahi.
        bus: Option<String>,
    },
    /// No CUPS: the page shows it as unavailable (windows in tests that don't need it).
    Off,
}

impl Service {
    /// The bus where cups-pk-helper and Avahi are.
    fn bus(&self) -> Bus {
        match self {
            Self::System => Bus::System,
            Self::At {
                bus: Some(address), ..
            } => Bus::Address(address.clone()),
            Self::At { bus: None, .. } | Self::Off => Bus::Off,
        }
    }
}

/// The widgets of a printer's row, kept across refreshes so focus and state stay.
#[derive(Debug, Clone)]
struct PrinterRow {
    row: gtk::ListBoxRow,
    title: gtk::Label,
    subtitle: gtk::Label,
    status: gtk::Label,
    /// The sign before the status of a printer that can't print.
    problem: gtk::Image,
    supplies: gtk::Box,
    resume: gtk::Button,
    menu: gtk::MenuButton,
    set_default: gtk::Button,
    pause: gtk::Button,
    cancel_all: gtk::Button,
}

/// The widgets of a job's row.
#[derive(Debug, Clone)]
struct JobRow {
    row: gtk::ListBoxRow,
    title: gtk::Label,
    subtitle: gtk::Label,
    cancel: gtk::Button,
}

mod imp {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::printers::{Cups, Snapshot};

    use super::{JobRow, PrinterRow, Service};

    #[derive(Default)]
    pub struct PrintersPage {
        /// The only child: holds the placeholder and the controls.
        pub content: gtk::Box,
        /// Shown instead of the controls while CUPS can't be reached.
        pub missing: gtk::Box,
        /// The sections.
        pub controls: gtk::Box,
        /// The error of the last action that failed.
        pub error: gtk::Label,
        /// A note about the user's own default printer.
        pub note: gtk::Label,
        pub printers: gtk::ListBox,
        pub printers_hint: gtk::Label,
        pub add: gtk::Button,
        pub jobs: gtk::ListBox,
        pub jobs_hint: gtk::Label,
        /// The "Advanced…" button for the page header.
        pub advanced: RefCell<Option<gtk::Button>>,

        pub service: RefCell<Service>,
        pub cups: RefCell<Option<Cups>>,
        /// What the page shows.
        pub snapshot: RefCell<Option<Snapshot>>,
        /// The rows of the printers, by name.
        pub(super) rows: RefCell<HashMap<String, PrinterRow>>,
        /// The rows of the jobs, by id.
        pub(super) job_rows: RefCell<HashMap<i32, JobRow>>,
        /// What runs on which printer or job (`job:ID`), as its row shows it.
        pub(super) running: RefCell<HashMap<String, &'static str>>,
        /// Whether a snapshot is being read.
        pub reading: Cell<bool>,
        /// Reads a snapshot every few seconds while the page is on screen.
        pub poll: RefCell<Option<glib::SourceId>>,
        /// The window the page is in, and our handler of its `suspended`, while mapped.
        pub window: RefCell<Option<(gtk::Window, glib::SignalHandlerId)>>,
        /// The user's own default printer from `~/.cups/lpoptions`, read when the page comes on
        /// screen rather than with every snapshot.
        pub own_default: RefCell<Option<String>>,
        /// The open dialog of the page.
        pub dialog: RefCell<Option<gtk::Window>>,
        /// The dialog for adding a printer, while it is open.
        pub(super) adding: RefCell<Option<super::dialogs::AddDialog>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PrintersPage {
        const NAME: &'static str = "StPrintersPage";
        type Type = super::PrintersPage;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-printers");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for PrintersPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.content.set_parent(&*self.obj());
            self.obj().build();
        }

        fn dispose(&self) {
            if let Some(poll) = self.poll.take() {
                poll.remove();
            }
            if let Some(dialog) = self.dialog.take() {
                dialog.destroy();
            }
            if let Some(adding) = self.adding.take() {
                adding.window.destroy();
            }
            if let Some((window, handler)) = self.window.take() {
                window.disconnect(handler);
            }
            self.content.unparent();
        }
    }

    impl WidgetImpl for PrintersPage {
        fn map(&self) {
            self.parent_map();
            // A window on a hidden workspace stays mapped, but is suspended.
            if let Some(window) = self.obj().root().and_downcast::<gtk::Window>() {
                let page = self.obj().downgrade();
                let handler = window.connect_suspended_notify(move |_| {
                    if let Some(page) = page.upgrade() {
                        page.update_poll();
                    }
                });
                if let Some((old, handler)) = self.window.replace(Some((window, handler))) {
                    old.disconnect(handler);
                }
            }
            self.own_default
                .replace(sway_things_core::printers::own_default());
            self.obj().update_poll();
        }

        fn unmap(&self) {
            self.parent_unmap();
            if let Some((window, handler)) = self.window.take() {
                window.disconnect(handler);
            }
            self.obj().update_poll();
        }
    }
}

glib::wrapper! {
    /// The printers and their jobs, live from CUPS.
    pub struct PrintersPage(ObjectSubclass<imp::PrintersPage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PrintersPage {
    /// The page for the CUPS of `service`, which reads it right away.
    pub fn new(service: &Service) -> Self {
        let page: Self = glib::Object::new();
        page.imp().service.replace(service.clone());
        page.connect();
        page
    }

    /// The "Advanced…" button for the page header: it starts system-config-printer, or opens
    /// CUPS's web interface without it.
    pub fn advanced_button(&self) -> gtk::Button {
        let imp = self.imp();
        if let Some(button) = imp.advanced.borrow().as_ref() {
            return button.clone();
        }
        let button = gtk::Button::with_label(tr(msg!("Advanced…")));
        let tooltip = if delegate::SYSTEM_CONFIG_PRINTER.find().is_some() {
            msg!("Open system-config-printer for printers that need a driver, and more")
        } else {
            msg!("Open CUPS's web interface for printers that need a driver, and more")
        };
        button.set_tooltip_text(Some(tr(tooltip)));
        let weak = self.downgrade();
        button.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.open_advanced();
            }
        });
        imp.advanced.replace(Some(button.clone()));
        button
    }

    /// The printers as listed: name, title, subtitle and status each; for tests.
    pub fn rows(&self) -> Vec<(String, String, String, String)> {
        let imp = self.imp();
        let rows = imp.rows.borrow();
        let mut listed = Vec::new();
        let mut child = imp.printers.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Some((name, row)) = rows.iter().find(|(_, row)| widget == row.row) {
                listed.push((
                    name.clone(),
                    row.title.text().to_string(),
                    row.subtitle.text().to_string(),
                    row.status.text().to_string(),
                ));
            }
        }
        listed
    }

    /// The supplies of the printer `name` as their labels read; for tests.
    pub fn supplies(&self, name: &str) -> Vec<String> {
        self.imp()
            .rows
            .borrow()
            .get(name)
            .map(|row| labels_in(row.supplies.upcast_ref()))
            .unwrap_or_default()
    }

    /// The jobs as listed: id, title and subtitle each; for tests.
    pub fn job_rows(&self) -> Vec<(i32, String, String)> {
        let imp = self.imp();
        let rows = imp.job_rows.borrow();
        let mut listed = Vec::new();
        let mut child = imp.jobs.first_child();
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

    /// The _Resume_ button of the printer `name`; for tests.
    pub fn resume_button(&self, name: &str) -> Option<gtk::Button> {
        self.imp()
            .rows
            .borrow()
            .get(name)
            .map(|row| row.resume.clone())
    }

    /// The _Cancel_ button of the job `id`; for tests.
    pub fn cancel_button(&self, id: i32) -> Option<gtk::Button> {
        self.imp()
            .job_rows
            .borrow()
            .get(&id)
            .map(|row| row.cancel.clone())
    }

    /// The ⋮ menu item `label` of the printer `name`; for tests.
    pub fn menu_item(&self, name: &str, label: &str) -> Option<gtk::Button> {
        let rows = self.imp().rows.borrow();
        let popover = rows.get(name)?.menu.popover()?;
        all_buttons(popover.upcast_ref())
            .into_iter()
            .find(|button| button.label().is_some_and(|text| text == label))
    }

    /// _Add Printer…_; for tests.
    pub fn add_button(&self) -> &gtk::Button {
        &self.imp().add
    }

    /// The open dialog for adding a printer; for tests.
    pub fn add_dialog(&self) -> Option<dialogs::AddDialog> {
        self.imp().adding.borrow().clone()
    }

    /// The open details dialog; for tests.
    pub fn details_dialog(&self) -> Option<gtk::Window> {
        self.imp().dialog.borrow().clone()
    }

    /// The error shown at the top; empty if none.
    pub fn error_text(&self) -> String {
        let error = &self.imp().error;
        if error.is_visible() {
            error.text().to_string()
        } else {
            String::new()
        }
    }

    /// The note about the default printer shown at the top; empty if none.
    pub fn note_text(&self) -> String {
        let note = &self.imp().note;
        if note.is_visible() {
            note.text().to_string()
        } else {
            String::new()
        }
    }

    /// The texts of the placeholder shown instead of the page; `None` while it shows printers.
    pub fn placeholder(&self) -> Option<Vec<String>> {
        let missing = &self.imp().missing;
        missing
            .is_visible()
            .then(|| labels_in(missing.upcast_ref()))
    }

    /// Reads CUPS's state now; for tests, which don't wait for the next poll.
    pub fn refresh(&self) {
        let Some(cups) = self.imp().cups.borrow().clone() else {
            return;
        };
        let imp = self.imp();
        if imp.reading.replace(true) {
            return;
        }
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let snapshot = cups.snapshot().await;
            if let Some(page) = weak.upgrade() {
                page.imp().reading.set(false);
                page.show(snapshot);
            }
        });
    }

    /// Lays out the widgets.
    fn build(&self) {
        let imp = self.imp();
        imp.content.set_orientation(gtk::Orientation::Vertical);
        imp.content.append(&imp.missing);
        imp.content.append(&imp.controls);
        imp.missing.set_orientation(gtk::Orientation::Vertical);
        imp.missing.set_visible(false);
        imp.controls.set_visible(false);
        imp.controls.set_orientation(gtk::Orientation::Vertical);

        for (label, class) in [(&imp.error, "error"), (&imp.note, style::DIM)] {
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_margin_bottom(12);
            label.add_css_class(class);
            label.set_visible(false);
            imp.controls.append(label);
        }

        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.set_margin_bottom(8);
        let title = heading(tr(msg!("Printers")), true);
        title.set_margin_top(0);
        title.set_margin_bottom(0);
        line.append(&title);
        let add = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        add.append(&gtk::Image::from_icon_name("list-add-symbolic"));
        add.append(&gtk::Label::new(Some(tr(msg!("Add Printer…")))));
        imp.add.set_child(Some(&add));
        imp.add.add_css_class("flat");
        imp.add
            .update_property(&[gtk::accessible::Property::Label(tr(msg!("Add Printer…")))]);
        let weak = self.downgrade();
        imp.add.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.open_add_dialog();
            }
        });
        let overlay = search_button::over(&line, &title, &imp.add);
        imp.controls.append(&overlay);
        section(&imp.printers, tr(msg!("Printers")));
        imp.controls.append(&imp.printers);
        hint(&imp.printers_hint);
        imp.controls.append(&imp.printers_hint);

        imp.controls
            .append(&heading(tr(msg!("Print Queue")), false));
        section(&imp.jobs, tr(msg!("Print Queue")));
        imp.controls.append(&imp.jobs);
        hint(&imp.jobs_hint);
        imp.controls.append(&imp.jobs_hint);
    }

    /// Finds CUPS and reads its state.
    fn connect(&self) {
        self.imp()
            .own_default
            .replace(sway_things_core::printers::own_default());
        let service = self.imp().service.borrow().clone();
        let cups = match &service {
            Service::System => Cups::local(),
            Service::At { socket, .. } => {
                Ok(Cups::at(Endpoint::Socket(socket.clone())).with_admin_bus(service.bus()))
            }
            Service::Off => Err(Error::Unavailable(msg!("no CUPS").into())),
        };
        match cups {
            Ok(cups) => {
                self.imp().cups.replace(Some(cups));
                self.refresh();
            }
            Err(error) => self.show(Err(error)),
        }
    }

    /// Reads CUPS's state every [`POLL`] while the page is on screen: mapped, in a window that
    /// isn't suspended on a hidden workspace.
    fn update_poll(&self) {
        let imp = self.imp();
        let shown = self.is_mapped()
            && self
                .root()
                .and_downcast::<gtk::Window>()
                .is_none_or(|window| !window.is_suspended());
        let wanted = shown && imp.cups.borrow().is_some();
        if wanted == imp.poll.borrow().is_some() {
            return;
        }
        if let Some(poll) = imp.poll.take() {
            poll.remove();
            return;
        }
        self.refresh();
        let weak = self.downgrade();
        let source = glib::timeout_add_local(POLL, move || {
            if let Some(page) = weak.upgrade() {
                page.refresh();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        imp.poll.replace(Some(source));
    }

    /// Shows `snapshot`, or why there is none.
    fn show(&self, snapshot: Result<Snapshot, Error>) {
        let imp = self.imp();
        match snapshot {
            Ok(mut snapshot) => {
                snapshot.set_user_default(imp.own_default.borrow().as_deref());
                imp.snapshot.replace(Some(snapshot));
                imp.missing.set_visible(false);
                imp.controls.set_visible(true);
                self.render();
            }
            Err(error) => {
                imp.snapshot.replace(None);
                let (icon, title, text) = match &error {
                    Error::Unavailable(_) => (
                        "printer-error-symbolic",
                        tr(msg!("CUPS isn't running")),
                        tr(msg!(
                            "The printing service isn't running. Start it with `systemctl start \
                             cups.socket`; this page updates by itself."
                        ))
                        .to_owned(),
                    ),
                    Error::Remote(_) => (
                        "printer-network-symbolic",
                        tr(msg!("Printers are on another computer")),
                        error.text().translated(),
                    ),
                    _ => (
                        "printer-error-symbolic",
                        tr(msg!("The printers can't be read")),
                        error.text().translated(),
                    ),
                };
                self.show_missing(icon, title, &text);
            }
        }
    }

    /// Updates the rows from the snapshot.
    fn render(&self) {
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
    fn render_jobs(&self) {
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
    fn ensure_row(&self, name: &str) -> PrinterRow {
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
    fn new_row(&self, name: &str) -> PrinterRow {
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
    fn menu(&self, name: &str) -> (gtk::MenuButton, [gtk::Button; 3]) {
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
    fn update_row(&self, row: &PrinterRow, printer: &Printer, is_default: bool, jobs: usize) {
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
    fn ensure_job_row(&self, id: i32) -> JobRow {
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
    fn update_job_row(&self, row: &JobRow, job: &Job, printer: &str) {
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

    /// Runs `action` on CUPS for `key` (a printer's name, or `job:ID`), which shows `label`
    /// meanwhile; its error shows at the top, `failed` saying what failed.
    fn run<F, Fut>(&self, key: &str, label: Msg, failed: Text, action: F)
    where
        F: FnOnce(Cups) -> Fut + 'static,
        Fut: Future<Output = Result<(), Error>> + 'static,
    {
        let Some(cups) = self.imp().cups.borrow().clone() else {
            return;
        };
        let weak = self.downgrade();
        let key = key.to_owned();
        self.set_running(&key, Some(tr(label)));
        self.show_error("");
        glib::spawn_future_local(async move {
            let result = action(cups).await;
            if let Some(page) = weak.upgrade() {
                page.set_running(&key, None);
                if let Err(error) = result {
                    page.show_error(&failed.text("error", error.text()).translated());
                }
                page.refresh();
            }
        });
    }

    /// Marks what runs on `key` and shows it.
    fn set_running(&self, key: &str, label: Option<&'static str>) {
        {
            let mut running = self.imp().running.borrow_mut();
            match label {
                Some(label) => running.insert(key.to_owned(), label),
                None => running.remove(key),
            };
        }
        self.render();
    }

    /// The title of the printer `name` in the snapshot.
    fn title(&self, name: &str) -> String {
        self.imp()
            .snapshot
            .borrow()
            .as_ref()
            .map_or_else(|| name.to_owned(), |s| title_of(s, name))
    }

    fn resume(&self, name: &str) {
        let failed = Text::new(msg!("Couldn't resume “{printer}”: {error}"))
            .arg("printer", self.title(name));
        let id = name.to_owned();
        self.run(name, msg!("Resuming…"), failed, move |cups| async move {
            cups.resume(&id).await
        });
    }

    fn pause(&self, name: &str) {
        let failed =
            Text::new(msg!("Couldn't pause “{printer}”: {error}")).arg("printer", self.title(name));
        let id = name.to_owned();
        self.run(name, msg!("Pausing…"), failed, move |cups| async move {
            cups.pause(&id).await
        });
    }

    fn set_default(&self, name: &str) {
        let failed = Text::new(msg!("Couldn't make “{printer}” the default: {error}"))
            .arg("printer", self.title(name));
        let id = name.to_owned();
        self.run(
            name,
            msg!("Setting as default…"),
            failed,
            move |cups| async move { cups.set_default(&id).await },
        );
    }

    fn print_test_page(&self, name: &str) {
        let failed = Text::new(msg!("Couldn't print a test page on “{printer}”: {error}"))
            .arg("printer", self.title(name));
        let id = name.to_owned();
        self.run(
            name,
            msg!("Sending a test page…"),
            failed,
            move |cups| async move { cups.print_test_page(&id).await.map(|_| ()) },
        );
    }

    fn cancel_job(&self, id: i32) {
        let failed = Text::new(msg!("Couldn't cancel the job: {error}"));
        self.run(
            &format!("job:{id}"),
            msg!("Canceling…"),
            failed,
            move |cups| async move { cups.cancel_job(id).await },
        );
    }

    fn cancel_all(&self, name: &str) {
        let ids: Vec<i32> = self
            .imp()
            .snapshot
            .borrow()
            .as_ref()
            .map(|s| s.jobs_of(name).map(|job| job.id).collect())
            .unwrap_or_default();
        let failed = Text::new(msg!("Couldn't cancel the jobs of “{printer}”: {error}"))
            .arg("printer", self.title(name));
        self.run(name, msg!("Canceling…"), failed, move |cups| async move {
            for id in ids {
                cups.cancel_job(id).await?;
            }
            Ok(())
        });
    }

    /// Removes the printer `name`, asking first.
    fn remove(&self, name: &str) {
        let title = self.title(name);
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let id = name.to_owned();
        glib::spawn_future_local(async move {
            if !dialogs::confirm_remove(&title, window.as_ref()).await {
                return;
            }
            if let Some(page) = weak.upgrade() {
                let failed =
                    Text::new(msg!("Couldn't remove “{printer}”: {error}")).arg("printer", &title);
                page.run(
                    &id.clone(),
                    msg!("Removing…"),
                    failed,
                    move |cups| async move { cups.remove(&id).await },
                );
            }
        });
    }

    /// Shows the details of the printer `name`, where its description and location can be
    /// changed and it can be shared.
    fn show_details(&self, name: &str) {
        let printer = self
            .imp()
            .snapshot
            .borrow()
            .as_ref()
            .and_then(|s| s.printer(name).cloned());
        let Some(printer) = printer else {
            return;
        };
        if let Some(old) = self.imp().dialog.take() {
            old.destroy();
        }
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let dialog = dialogs::details(window.as_ref(), &printer, move |changes| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            page.imp().dialog.replace(None);
            if let Some(changes) = changes {
                page.apply_details(changes);
            }
        });
        self.imp().dialog.replace(Some(dialog));
    }

    /// Makes the changes of the details dialog.
    fn apply_details(&self, changes: dialogs::DetailsChange) {
        let failed = Text::new(msg!("Couldn't change “{printer}”: {error}"))
            .arg("printer", self.title(&changes.name));
        let name = changes.name.clone();
        self.run(&name, msg!("Saving…"), failed, move |cups| async move {
            if let Some(info) = &changes.info {
                cups.set_info(&changes.name, info).await?;
            }
            if let Some(location) = &changes.location {
                cups.set_location(&changes.name, location).await?;
            }
            if let Some(shared) = changes.shared {
                cups.set_shared(&changes.name, shared).await?;
            }
            Ok(())
        });
    }

    /// Opens the dialog for adding a printer.
    fn open_add_dialog(&self) {
        let imp = self.imp();
        // An open dialog comes to the front; one that is gone (whatever closed it) is
        // forgotten, never shown again empty.
        let open = imp.adding.borrow().clone();
        if let Some(open) = open {
            if open.window.is_visible() {
                open.window.present();
                return;
            }
            imp.adding.replace(None);
        }
        let Some(cups) = imp.cups.borrow().clone() else {
            return;
        };
        let taken: Vec<String> = imp
            .snapshot
            .borrow()
            .as_ref()
            .map(|s| s.printers.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let known: Vec<String> = imp
            .snapshot
            .borrow()
            .as_ref()
            .map(|s| s.printers.iter().map(|p| p.device_uri.clone()).collect())
            .unwrap_or_default();
        let window = self.root().and_downcast::<gtk::Window>();
        let bus = imp.service.borrow().bus();
        let weak = self.downgrade();
        let dialog =
            dialogs::AddDialog::open(window.as_ref(), cups, bus, taken, known, move |added| {
                if let Some(page) = weak.upgrade() {
                    page.imp().adding.replace(None);
                    if added {
                        page.refresh();
                    }
                }
            });
        imp.adding.replace(Some(dialog));
    }

    /// Starts system-config-printer, or opens CUPS's web interface without it.
    fn open_advanced(&self) {
        if let Ok(mut child) = delegate::SYSTEM_CONFIG_PRINTER.launch() {
            // Waiting reaps it once it ends, so it doesn't stay behind as a zombie.
            glib::spawn_future_local(async move {
                let _ = child.status().await;
            });
            return;
        }
        // At the port this CUPS is reached on.
        let address = self.imp().cups.borrow().as_ref().map_or_else(
            || "http://localhost:631/".to_owned(),
            |cups| cups.endpoint().web_interface(),
        );
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        gtk::UriLauncher::new(&address).launch(
            window.as_ref(),
            gtk::gio::Cancellable::NONE,
            move |result| {
                if let (Err(error), Some(page)) = (result, weak.upgrade()) {
                    page.show_error(&trf(
                        msg!("CUPS's web interface can't be opened: {error}"),
                        &[("error", &error)],
                    ));
                }
            },
        );
    }

    fn show_missing(&self, icon: &str, title: &str, text: &str) {
        let imp = self.imp();
        while let Some(child) = imp.missing.first_child() {
            imp.missing.remove(&child);
        }
        let placeholder = Placeholder::new(icon, title, text);
        placeholder.set_hexpand(true);
        imp.missing.append(&placeholder);
        imp.controls.set_visible(false);
        imp.missing.set_visible(true);
    }

    /// Shows `message` at the top, or hides it when empty.
    fn show_error(&self, message: &str) {
        let error = &self.imp().error;
        error.set_text(message);
        error.set_visible(!message.is_empty());
    }
}

/// The title of the printer `name` in `snapshot`, or the name.
fn title_of(snapshot: &Snapshot, name: &str) -> String {
    snapshot
        .printer(name)
        .map_or_else(|| name.to_owned(), |printer| printer.title().to_owned())
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

/// Adds or removes the style class `class` of `widget`.
fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

/// The buttons in `widget`, depth first.
fn all_buttons(widget: &gtk::Widget) -> Vec<gtk::Button> {
    let mut buttons = Vec::new();
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(button) = current.downcast_ref::<gtk::Button>() {
            buttons.push(button.clone());
        }
        buttons.extend(all_buttons(&current));
        child = current.next_sibling();
    }
    buttons
}

/// The texts of the labels in `widget`, in order.
fn labels_in(widget: &gtk::Widget) -> Vec<String> {
    let mut texts = Vec::new();
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(label) = current.downcast_ref::<gtk::Label>() {
            texts.push(label.text().to_string());
        }
        texts.extend(labels_in(&current));
        child = current.next_sibling();
    }
    texts
}

/// A section heading as on the pages built from the registry; `first` has no space above it.
fn heading(title: &str, first: bool) -> gtk::Label {
    let label = gtk::Label::new(Some(&title.to_uppercase()));
    label.set_xalign(0.0);
    label.set_margin_start(4);
    label.set_margin_bottom(8);
    label.set_margin_top(if first { 0 } else { 24 });
    label.set_accessible_role(gtk::AccessibleRole::Heading);
    label.add_css_class("st-section-heading");
    label.add_css_class(style::DIM);
    label
}

/// Makes `list` a bordered list of a section named `title`.
fn section(list: &gtk::ListBox, title: &str) {
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("st-section");
    list.update_property(&[gtk::accessible::Property::Label(title)]);
}

/// Styles `label` as the hint shown in place of an empty list.
fn hint(label: &gtk::Label) {
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.add_css_class(style::DIM);
    label.add_css_class("st-section-hint");
    label.set_visible(false);
}

/// Shows `text` in `label`, or hides it.
fn set_hint(label: &gtk::Label, text: Option<&str>) {
    label.set_text(text.unwrap_or_default());
    label.set_visible(text.is_some());
}

/// Puts `rows` into `list` in this order, moving only what is out of place, so a row with the
/// focus keeps it unless it moves.
fn arrange(list: &gtk::ListBox, rows: &[gtk::ListBoxRow]) {
    let mut current = Vec::new();
    let mut child = list.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        current.push(widget);
    }
    let wanted: Vec<gtk::Widget> = rows.iter().map(|row| row.clone().upcast()).collect();
    if current == wanted {
        return;
    }
    let focus = list.root().and_then(|root| root.focus()).filter(|focus| {
        rows.iter()
            .any(|row| focus.is_ancestor(row) || focus == row)
    });
    for row in rows {
        remove_row(row);
    }
    for row in rows {
        list.append(row);
    }
    if let Some(focus) = focus {
        focus.grab_focus();
    }
}

/// Takes `row` out of the list it is in.
fn remove_row(row: &gtk::ListBoxRow) {
    if let Some(list) = row.parent().and_downcast::<gtk::ListBox>() {
        list.remove(row);
    }
}

/// `child` as a row of a section list. The row itself takes no focus; its controls do.
fn list_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let item = gtk::ListBoxRow::new();
    item.set_child(Some(child));
    item.set_activatable(false);
    item.set_focusable(false);
    item
}

/// A row with `title` above `subtitle` on the left and `controls` on the right.
fn row(title: &gtk::Label, subtitle: &gtk::Label, controls: &[&gtk::Widget]) -> gtk::Box {
    for label in [title, subtitle] {
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    }
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(title);
    text.append(subtitle);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.append(&text);
    for control in controls {
        line.append(*control);
    }
    line
}
