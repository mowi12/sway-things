// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialogs of the printers besides their details: adding a printer, picked from those found
//! automatically or by its address, and removing one.
//!
//! What printers announce about themselves is untrusted: it only ever goes into label text and
//! dialog messages, which GTK shows as plain text.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::printers::{
    Bus, Cups, Discovered, Discovery, Error, Found, NewPrinter, device_uri, queue_name,
};

use crate::i18n::{Msg, msg, tr, trf};
use crate::style;

/// What the dialog says while it looks for printers.
const SEARCHING: Msg = msg!("Looking for driverless printers on the network and on USB…");

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
    rows: RefCell<HashMap<String, (gtk::ListBoxRow, Found)>>,
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
        let (searching, spinner, hint) = searching_line();
        content.append(&searching);
        let found = gtk::ListBox::new();
        found.set_selection_mode(gtk::SelectionMode::None);
        found.set_activate_on_single_click(true);
        found.add_css_class("st-section");
        found.add_css_class("st-printer-picker");
        found.update_property(&[gtk::accessible::Property::Label(tr(msg!("Printers found")))]);
        found.set_visible(false);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&found)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(320)
            .build();
        content.append(&scroller);

        let by_address = small_heading(tr(msg!("Or add one by its address")));
        by_address.set_margin_top(8);
        content.append(&by_address);
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
        self.found.connect_row_activated(move |_, row| {
            if !dialog.state.busy.get() {
                dialog.add_found(&row.widget_name());
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
        let task = glib::spawn_future_local(async move {
            let mut discovery = match Discovery::start(&bus).await {
                Ok(discovery) => discovery,
                Err(error) => {
                    dialog.state.spinner.set_visible(false);
                    let text = match &error {
                        // Discovery is unavailable when Avahi is; the bus's reason is
                        // only for the log.
                        Error::Unavailable(reason) => {
                            glib::g_debug!(
                                "sway-things",
                                "Avahi can't be reached: {}",
                                reason.translated()
                            );
                            tr(msg!(
                                "Printers can't be found automatically because Avahi \
                                         isn't running. Start avahi-daemon, or type the address \
                                         of the printer below."
                            ))
                            .to_owned()
                        }
                        other => trf(
                            msg!(
                                "Printers can't be found automatically: {error}. Type \
                                         the address of the printer below."
                            ),
                            &[("error", &other.text().translated())],
                        ),
                    };
                    dialog.state.hint.set_text(&text);
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

    /// Adds or updates the line of `found`: its icon, its model, where it stands and how it is
    /// connected, and _Added_ if a printer is set up at its address. A click adds it.
    fn show_found(&self, found: Found) {
        let added = self.state.known.iter().any(|uri| found.is_at(uri));
        let mut rows = self.state.rows.borrow_mut();
        if let Some((row, old)) = rows.get_mut(&found.key) {
            *old = found.clone();
            row.set_sensitive(!added && !self.state.busy.get());
            return;
        }
        let title = gtk::Label::new(Some(found.title()));
        title.add_css_class("st-row-title");
        let mut details = Vec::new();
        if !found.location.is_empty() {
            details.push(found.location.clone());
        }
        details.push(
            tr(if found.local {
                msg!("USB")
            } else {
                msg!("Network")
            })
            .to_owned(),
        );
        let subtitle = gtk::Label::new(Some(&details.join(" · ")));
        subtitle.add_css_class(style::DIM);
        subtitle.add_css_class("st-device-subtitle");
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        for label in [&title, &subtitle] {
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(label);
        }
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        line.add_css_class("st-device-row");
        line.append(&super::rows::device_tile(if found.local {
            "printer-symbolic"
        } else {
            "printer-network-symbolic"
        }));
        line.append(&text);
        if added {
            let label = gtk::Label::new(Some(tr(msg!("Added"))));
            label.add_css_class(style::DIM);
            line.append(&label);
        } else {
            let sign = gtk::Image::from_icon_name("list-add-symbolic");
            sign.set_accessible_role(gtk::AccessibleRole::Presentation);
            line.append(&sign);
        }
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        row.set_activatable(true);
        row.set_widget_name(&found.key);
        row.set_sensitive(!added && !self.state.busy.get());
        row.set_tooltip_text(Some(&trf(
            msg!("Add {printer}"),
            &[("printer", &found.title())],
        )));
        row.update_property(&[
            gtk::accessible::Property::Label(&trf(
                msg!("Add {printer}"),
                &[("printer", &found.title())],
            )),
            gtk::accessible::Property::Description(&details.join(", ")),
        ]);
        self.found.append(&row);
        self.found.set_visible(true);
        self.state.spinner.set_visible(false);
        self.state
            .hint
            .set_text(tr(msg!("Click a printer to add it.")));
        rows.insert(found.key.clone(), (row, found));
    }

    /// Takes away the line of the printer with `key`.
    fn hide_found(&self, key: &str) {
        let mut rows = self.state.rows.borrow_mut();
        if let Some((row, _)) = rows.remove(key) {
            self.found.remove(&row);
        }
        self.found.set_visible(!rows.is_empty());
        if rows.is_empty() {
            self.state.spinner.set_visible(true);
            self.state.hint.set_text(tr(SEARCHING));
        }
    }

    /// Adds the printer found with `key`.
    pub fn add_found(&self, key: &str) {
        let found = self
            .state
            .rows
            .borrow()
            .get(key)
            .map(|(_, found)| found.clone());
        if let Some(found) = found {
            self.add(
                found.title().to_owned(),
                found.uri.clone(),
                found.location.clone(),
            );
        }
    }

    /// The printers listed, by title; for tests.
    pub fn found_titles(&self) -> Vec<String> {
        let rows = self.state.rows.borrow();
        let mut titles: Vec<String> = rows
            .values()
            .map(|(_, found)| found.title().to_owned())
            .collect();
        titles.sort();
        titles
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
        for (row, found) in self.state.rows.borrow().values() {
            let added = self.state.known.iter().any(|uri| found.is_at(uri));
            row.set_sensitive(!busy && !added);
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

/// The line above the printers found: a spinner while none is found yet, and what to do.
fn searching_line() -> (gtk::Box, gtk::Spinner, gtk::Label) {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let spinner = gtk::Spinner::new();
    spinner.set_spinning(true);
    spinner.update_property(&[gtk::accessible::Property::Label(tr(msg!("Searching")))]);
    line.append(&spinner);
    let hint = dim(tr(SEARCHING));
    hint.set_hexpand(true);
    line.append(&hint);
    (line, spinner, hint)
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
