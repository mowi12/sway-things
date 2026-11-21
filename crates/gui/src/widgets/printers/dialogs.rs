// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialogs of the Printers page: adding a printer (found automatically or by its address), a
//! printer's details, and removing one.
//!
//! What printers announce about themselves is untrusted: it only ever goes into label text and
//! dialog messages, which GTK shows as plain text.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::printers::{
    Bus, Cups, Discovered, Discovery, Error, Found, NewPrinter, Printer, device_uri, queue_name,
};

use crate::i18n::{msg, tr, trf};
use crate::style;

/// What a dialog calls once with its answer.
type Done<T> = Box<dyn FnOnce(T)>;

/// The dialog for adding a printer, while it is open.
#[derive(Clone)]
pub struct AddDialog {
    /// Its window.
    pub window: gtk::Window,
    /// The list of the printers found automatically.
    pub found: gtk::ListBox,
    /// The field for an address.
    pub address: gtk::Entry,
    /// _Add_ for the address.
    pub add_address: gtk::Button,
    /// What is being done, or what failed.
    pub status: gtk::Label,
    state: Rc<AddState>,
}

/// What the dialog keeps while it is open.
struct AddState {
    cups: Cups,
    /// The names CUPS's queues have.
    taken: Vec<String>,
    /// The device URIs of the printers set up.
    known: Vec<String>,
    /// The rows of the printers found, by their key.
    rows: RefCell<HashMap<String, (gtk::ListBoxRow, gtk::Button, Found)>>,
    hint: gtk::Label,
    spinner: gtk::Spinner,
    busy: Cell<bool>,
    done: RefCell<Option<Done<bool>>>,
    discovery: RefCell<Option<glib::JoinHandle<()>>>,
}

impl AddDialog {
    /// Opens the dialog over `parent`: it lists what Avahi finds on `bus` and adds to `cups`,
    /// whose queues are named `taken` and print to `known`. `done` gets whether a printer was
    /// added once it closes.
    pub fn open(
        parent: Option<&gtk::Window>,
        cups: Cups,
        bus: Bus,
        taken: Vec<String>,
        known: Vec<String>,
        done: impl FnOnce(bool) + 'static,
    ) -> Self {
        let window = dialog_window(parent, tr(msg!("Add Printer")));
        let content = content(&window, tr(msg!("Add Printer")));
        let (found_heading, spinner) = searching_heading();
        content.append(&found_heading);
        let found = gtk::ListBox::new();
        found.set_selection_mode(gtk::SelectionMode::None);
        found.add_css_class("boxed-list");
        found.update_property(&[gtk::accessible::Property::Label(tr(msg!(
            "Found automatically"
        )))]);
        found.set_visible(false);
        content.append(&found);
        let hint = dim(tr(msg!(
            "Looking for driverless printers on the network and on USB…"
        )));
        content.append(&hint);

        content.append(&small_heading(tr(msg!("By address"))));
        let address = gtk::Entry::builder()
            .placeholder_text("ipp://printer.local/ipp/print")
            .activates_default(true)
            .hexpand(true)
            .build();
        address.update_property(&[gtk::accessible::Property::Label(tr(msg!(
            "Address of the printer"
        )))]);
        let add_address = gtk::Button::with_label(tr(msg!("Add")));
        add_address.set_sensitive(false);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.append(&address);
        line.append(&add_address);
        content.append(&line);
        content.append(&dim(tr(msg!(
            "A host name, an IP address, or an ipp:// or ipps:// address. The printer must be \
             driverless (IPP Everywhere or AirPrint); for others, use Advanced… on the page."
        ))));

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        status.set_visible(false);
        content.append(&status);

        let close = gtk::Button::with_label(tr(msg!("Close")));
        close.set_halign(gtk::Align::End);
        close.set_margin_top(8);
        content.append(&close);

        let dialog = Self {
            window,
            found,
            address,
            add_address,
            status,
            state: Rc::new(AddState {
                cups,
                taken,
                known,
                rows: RefCell::new(HashMap::new()),
                hint,
                spinner,
                busy: Cell::new(false),
                done: RefCell::new(Some(Box::new(done))),
                discovery: RefCell::new(None),
            }),
        };
        dialog.connect(&close);
        dialog.discover(bus);
        dialog.window.present();
        dialog.address.grab_focus();
        dialog
    }

    /// Connects the address field, its button, _Close_ and the window's end.
    fn connect(&self, close: &gtk::Button) {
        let check = {
            let dialog = self.clone();
            move || {
                let valid = device_uri(&dialog.address.text()).is_ok();
                dialog
                    .add_address
                    .set_sensitive(valid && !dialog.state.busy.get());
            }
        };
        self.address.connect_changed(move |_| check());
        let dialog = self.clone();
        self.add_address
            .connect_clicked(move |_| dialog.add_by_address());
        let dialog = self.clone();
        self.address.connect_activate(move |_| {
            if dialog.add_address.is_sensitive() {
                dialog.add_by_address();
            }
        });
        let dialog = self.clone();
        close.connect_clicked(move |_| dialog.finish(false));
        // The compositor's close: the page learns of it before GTK destroys the window.
        let state = Rc::clone(&self.state);
        self.window.connect_close_request(move |_| {
            end(&state, false);
            glib::Propagation::Proceed
        });
        let dialog = self.clone();
        close_on_escape(&self.window, move || dialog.finish(false));
    }

    /// Closes the dialog: stops looking for printers, tells the page whether a printer was
    /// `added`, and destroys the window. The page hears of it here, never through the window's
    /// `destroy` signal, which GTK only sends once nothing holds the window any more.
    fn finish(&self, added: bool) {
        end(&self.state, added);
        self.window.destroy();
    }

    /// Lists what Avahi finds until the dialog closes.
    fn discover(&self, bus: Bus) {
        let dialog = self.clone();
        let task =
            glib::spawn_future_local(async move {
                let mut discovery =
                    match Discovery::start(&bus).await {
                        Ok(discovery) => discovery,
                        Err(error) => {
                            dialog.state.spinner.set_visible(false);
                            dialog.state.hint.set_text(&trf(
                        msg!(
                            "Printers can't be found automatically: {error}. Type the address \
                             of the printer below."
                        ),
                        // Discovery is unavailable when Avahi is: its reason says so.
                        &[("error", &match &error {
                            Error::Unavailable(reason) => reason.translated(),
                            other => other.text().translated(),
                        })],
                    ));
                            return;
                        }
                    };
                while let Some(event) = discovery.next().await {
                    match event {
                        Discovered::Added(found) => dialog.show_found(found),
                        Discovered::Removed(key) => dialog.hide_found(&key),
                    }
                }
            });
        self.state.discovery.replace(Some(task));
        // Avahi reports what it knows at once; after a while, say if that is nothing.
        let dialog = self.clone();
        glib::timeout_add_seconds_local_once(5, move || {
            if dialog.state.rows.borrow().is_empty() && dialog.state.spinner.is_visible() {
                dialog.state.hint.set_text(tr(msg!(
                    "No driverless printer found yet. Make sure it is on and on the same \
                     network, or type its address below."
                )));
            }
        });
    }

    /// Adds or updates the row of `found`.
    fn show_found(&self, found: Found) {
        let mut rows = self.state.rows.borrow_mut();
        if let Some((_, button, old)) = rows.get_mut(&found.key) {
            *old = found.clone();
            button.set_sensitive(!self.state.busy.get());
            return;
        }
        let title = gtk::Label::new(Some(found.title()));
        let mut details = Vec::new();
        if !found.location.is_empty() {
            details.push(found.location.clone());
        }
        if found.title() != found.name {
            details.push(found.name.clone());
        }
        let subtitle = gtk::Label::new(Some(&details.join(" · ")));
        subtitle.set_visible(!details.is_empty());
        subtitle.add_css_class(style::DIM);
        let added = self.state.known.iter().any(|uri| found.is_at(uri));
        let button = gtk::Button::with_label(tr(if added { msg!("Added") } else { msg!("Add") }));
        button.set_valign(gtk::Align::Center);
        button.set_sensitive(!added && !self.state.busy.get());
        button.update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("Add {printer}"),
            &[("printer", &found.title())],
        ))]);
        let dialog = self.clone();
        let key = found.key.clone();
        button.connect_clicked(move |_| dialog.add_found(&key));
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        for label in [&title, &subtitle] {
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(label);
        }
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        line.set_margin_top(6);
        line.set_margin_bottom(6);
        line.set_margin_start(8);
        line.set_margin_end(8);
        line.append(&text);
        line.append(&button);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        row.set_activatable(false);
        row.set_focusable(false);
        self.found.append(&row);
        self.found.set_visible(true);
        self.state.hint.set_visible(false);
        rows.insert(found.key.clone(), (row, button, found));
    }

    /// Takes away the row of the printer with `key`.
    fn hide_found(&self, key: &str) {
        let mut rows = self.state.rows.borrow_mut();
        if let Some((row, _, _)) = rows.remove(key) {
            self.found.remove(&row);
        }
        self.found.set_visible(!rows.is_empty());
        self.state.hint.set_visible(rows.is_empty());
    }

    fn add_found(&self, key: &str) {
        let found = self
            .state
            .rows
            .borrow()
            .get(key)
            .map(|(_, _, found)| found.clone());
        if let Some(found) = found {
            self.add(
                found.title().to_owned(),
                found.uri.clone(),
                found.location.clone(),
            );
        }
    }

    fn add_by_address(&self) {
        let Ok(uri) = device_uri(&self.address.text()) else {
            return;
        };
        self.add(host_of(&uri).to_owned(), uri, String::new());
    }

    /// Adds the printer at `uri`, titled `info`, standing at `location`.
    fn add(&self, info: String, uri: String, location: String) {
        let taken: Vec<&str> = self.state.taken.iter().map(String::as_str).collect();
        let name = queue_name(&info, &taken);
        self.set_busy(true);
        self.status.remove_css_class("error");
        self.status.add_css_class(style::DIM);
        self.status.set_text(&trf(
            msg!(
                "Adding “{printer}”… CUPS asks the printer what it can do, which can take a minute."
            ),
            &[("printer", &info)],
        ));
        self.status.set_visible(true);
        let dialog = self.clone();
        glib::spawn_future_local(async move {
            let printer = NewPrinter {
                name,
                uri,
                info: info.clone(),
                location,
            };
            match dialog.state.cups.add(&printer).await {
                Ok(_) => dialog.finish(true),
                Err(error) => {
                    dialog.set_busy(false);
                    dialog.status.remove_css_class(style::DIM);
                    dialog.status.add_css_class("error");
                    dialog.status.set_text(&trf(
                        msg!("Couldn't add “{printer}”: {error}"),
                        &[("printer", &info), ("error", &error.text().translated())],
                    ));
                }
            }
        });
    }

    /// Makes the buttons insensitive while a printer is being added.
    fn set_busy(&self, busy: bool) {
        self.state.busy.set(busy);
        for (_, button, found) in self.state.rows.borrow().values() {
            let added = self.state.known.iter().any(|uri| found.is_at(uri));
            button.set_sensitive(!busy && !added);
        }
        self.address.set_sensitive(!busy);
        self.add_address
            .set_sensitive(!busy && device_uri(&self.address.text()).is_ok());
    }
}

/// Stops the dialog's search and tells the page whether a printer was `added`, once.
fn end(state: &AddState, added: bool) {
    if let Some(task) = state.discovery.take() {
        task.abort();
    }
    if let Some(done) = state.done.take() {
        done(added);
    }
}

/// Runs `close` when Escape is pressed in `window`.
fn close_on_escape(window: &gtk::Window, close: impl Fn() + 'static) {
    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(move |_, _| {
            close();
            glib::Propagation::Stop
        })),
    ));
    window.add_controller(escape);
}

/// What the details dialog changes of a printer: `None` for what stays.
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

/// Shows the details of `printer`, with its description, location and sharing to change; `done`
/// gets the changes on _Save_, or `None` when the dialog closes any other way (_Cancel_,
/// Escape, the compositor's close).
pub fn details(
    parent: Option<&gtk::Window>,
    printer: &Printer,
    done: impl FnOnce(Option<DetailsChange>) + 'static,
) -> gtk::Window {
    let window = dialog_window(parent, printer.title());
    let content = content(&window, printer.title());
    let grid = gtk::Grid::builder()
        .column_spacing(16)
        .row_spacing(6)
        .margin_top(4)
        .build();
    let facts = [
        (tr(msg!("Name")), printer.name.clone()),
        (tr(msg!("Model")), printer.model.clone()),
        (tr(msg!("Address")), printer.device_uri.clone()),
    ];
    for (row, (label, value)) in (0..).zip(facts) {
        let name = gtk::Label::new(Some(label));
        name.set_xalign(0.0);
        name.set_yalign(0.0);
        name.add_css_class(style::DIM);
        let value = gtk::Label::new(Some(&value));
        value.set_xalign(0.0);
        value.set_selectable(true);
        value.set_wrap(true);
        value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        value.set_hexpand(true);
        value.update_relation(&[gtk::accessible::Relation::LabelledBy(&[name.upcast_ref()])]);
        grid.attach(&name, 0, row, 1, 1);
        grid.attach(&value, 1, row, 1, 1);
    }
    content.append(&grid);
    let info = labeled_entry(&content, tr(msg!("Description")), &printer.info);
    let location = labeled_entry(&content, tr(msg!("Location")), &printer.location);
    let shared = gtk::Switch::builder()
        .active(printer.shared)
        .valign(gtk::Align::Center)
        .build();
    let share_label = gtk::Label::new(Some(tr(msg!("Share on the network"))));
    share_label.set_xalign(0.0);
    share_label.set_hexpand(true);
    shared.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
        share_label.upcast_ref()
    ])]);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.set_margin_top(4);
    line.append(&share_label);
    line.append(&shared);
    content.append(&line);
    content.append(&dim(tr(msg!(
        "Other computers see a shared printer while sharing is on for CUPS as a whole."
    ))));

    let done: Rc<RefCell<Option<Done<Option<DetailsChange>>>>> =
        Rc::new(RefCell::new(Some(Box::new(done))));
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_homogeneous(true);
    buttons.set_margin_top(8);
    let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
    let save = gtk::Button::with_label(tr(msg!("Save")));
    save.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&save);
    content.append(&buttons);
    window.set_default_widget(Some(&save));
    // Every way of closing it answers `done` first: the window's `destroy` signal only comes
    // once nothing holds the window any more.
    let cancel_answer = {
        let done = Rc::clone(&done);
        let window = window.downgrade();
        move || {
            if let Some(done) = done.take() {
                done(None);
            }
            if let Some(window) = window.upgrade() {
                window.destroy();
            }
        }
    };
    let closing = Rc::clone(&done);
    window.connect_close_request(move |_| {
        if let Some(done) = closing.take() {
            done(None);
        }
        glib::Propagation::Proceed
    });
    let on_escape = cancel_answer.clone();
    close_on_escape(&window, on_escape);
    cancel.connect_clicked(move |_| cancel_answer());
    let win = window.clone();
    let before = printer.clone();
    save.connect_clicked(move |_| {
        let changed = |new: String, old: &str| (new != old).then_some(new);
        let change = DetailsChange {
            name: before.name.clone(),
            info: changed(info.text().trim().to_owned(), &before.info),
            location: changed(location.text().trim().to_owned(), &before.location),
            shared: (shared.is_active() != before.shared).then_some(shared.is_active()),
        };
        if let Some(done) = done.take() {
            let any = change.info.is_some() || change.location.is_some() || change.shared.is_some();
            done(any.then_some(change));
        }
        win.destroy();
    });
    window.present();
    window
}

/// Asks whether to remove the printer titled `title`.
pub async fn confirm_remove(title: &str, window: Option<&gtk::Window>) -> bool {
    let dialog = gtk::AlertDialog::builder()
        // Plain text: the dialog doesn't read markup.
        .message(trf(msg!("Remove “{printer}”?"), &[("printer", &title)]))
        .detail(tr(msg!(
            "Its print queue is removed for every user of this computer, with the jobs waiting \
             in it. To use the printer again, add it again."
        )))
        .buttons([tr(msg!("Cancel")), tr(msg!("Remove"))])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    matches!(dialog.choose_future(window).await, Ok(1))
}

/// The host of a device URI (`ipp://HOST:631/…`), without the port.
fn host_of(uri: &str) -> &str {
    let rest = uri.split_once("://").map_or(uri, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or(rest);
    match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(v6),
        None => authority.split(':').next().unwrap_or(authority),
    }
}

/// A modal dialog window titled `title` over `parent`.
fn dialog_window(parent: Option<&gtk::Window>, title: &str) -> gtk::Window {
    let window = gtk::Window::builder()
        .modal(true)
        .resizable(false)
        .destroy_with_parent(true)
        .default_width(440)
        .title(title)
        .build();
    window.set_transient_for(parent);
    window.add_css_class("st-dialog");
    window
}

/// The content box of `window`, with the heading `title`.
fn content(window: &gtk::Window, title: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(24);
    content.set_margin_bottom(16);
    content.set_margin_start(20);
    content.set_margin_end(20);
    let heading = gtk::Label::new(Some(title));
    heading.set_wrap(true);
    heading.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    heading.set_justify(gtk::Justification::Center);
    heading.set_max_width_chars(40);
    heading.add_css_class("title-2");
    heading.set_accessible_role(gtk::AccessibleRole::Heading);
    content.append(&heading);
    window.set_child(Some(&content));
    content
}

/// The heading of the printers found, with the spinner that says the search runs.
fn searching_heading() -> (gtk::Box, gtk::Spinner) {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.append(&small_heading(tr(msg!("Found automatically"))));
    let spinner = gtk::Spinner::new();
    spinner.set_spinning(true);
    spinner.update_property(&[gtk::accessible::Property::Label(tr(msg!("Searching")))]);
    line.append(&spinner);
    (line, spinner)
}

fn small_heading(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("heading");
    label.set_accessible_role(gtk::AccessibleRole::Heading);
    label
}

fn dim(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_max_width_chars(50);
    label.add_css_class(style::DIM);
    label
}

/// An entry labeled `label` in `content`, holding `text`.
fn labeled_entry(content: &gtk::Box, label: &str, text: &str) -> gtk::Entry {
    let label = gtk::Label::new(Some(label));
    label.set_xalign(0.0);
    label.add_css_class(style::DIM);
    let entry = gtk::Entry::builder()
        .text(text)
        .activates_default(true)
        .build();
    entry.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
    content.append(&label);
    content.append(&entry);
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_of_an_address_is_found() {
        assert_eq!(
            host_of("ipp://printer.local:631/ipp/print"),
            "printer.local"
        );
        assert_eq!(host_of("ipps://[fe80::1]:631/ipp/print"), "fe80::1");
        assert_eq!(host_of("ipp://10.0.0.2/ipp/print"), "10.0.0.2");
    }
}
