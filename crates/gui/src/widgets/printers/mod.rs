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

mod actions;
mod dialogs;
mod rows;
mod supplies;

use std::path::PathBuf;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::delegate;
use sway_things_core::printers::{Bus, Cups, Endpoint, Error, Snapshot};

use super::list::{heading, hint, labels_in, section};
use super::{Placeholder, search_button};
use crate::i18n::{msg, tr};
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
        /// The icon, title and text the placeholder in `missing` was built with.
        pub missing_shown: RefCell<Option<(String, String, String)>>,
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

    /// Shows the placeholder with `icon`, `title` and `text` instead of the page. It is only
    /// built again when one of them changes, so every poll doesn't make a screen reader read
    /// it again.
    pub(super) fn show_missing(&self, icon: &str, title: &str, text: &str) {
        let imp = self.imp();
        let wanted = (icon.to_owned(), title.to_owned(), text.to_owned());
        if imp.missing_shown.borrow().as_ref() != Some(&wanted) {
            while let Some(child) = imp.missing.first_child() {
                imp.missing.remove(&child);
            }
            let placeholder = Placeholder::new(icon, title, text);
            placeholder.set_hexpand(true);
            imp.missing.append(&placeholder);
            imp.missing_shown.replace(Some(wanted));
        }
        imp.controls.set_visible(false);
        imp.missing.set_visible(true);
    }

    /// Shows `message` at the top, or hides it when empty.
    pub(super) fn show_error(&self, message: &str) {
        let error = &self.imp().error;
        error.set_text(message);
        error.set_visible(!message.is_empty());
    }
}

/// The title of the printer `name` in `snapshot`, or the name.
pub(super) fn title_of(snapshot: &Snapshot, name: &str) -> String {
    snapshot
        .printer(name)
        .map_or_else(|| name.to_owned(), |printer| printer.title().to_owned())
}

/// Adds or removes the style class `class` of `widget`.
pub(super) fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
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
