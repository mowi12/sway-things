// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scanners of Printers & Scanners: a line per scanner SANE lists, with how it is connected;
//! a line per driverless scanner Avahi finds on the network that SANE doesn't list, with a
//! warning; and the hints for what is missing (scanimage, sane-airscan, ipp-usb, the access to a
//! USB scanner). _Open Document Scanner_ (or Skanpage, `xsane`) hands over to a
//! scan app.
//!
//! SANE has no change events, and listing takes a few seconds while its backends look on USB
//! and the network: the list is read when the page comes on screen and with _Search Again_,
//! never in the background. Names come from scanners and the network: they are only ever set
//! as plain label text.

use std::path::PathBuf;
use std::time::Duration;

use futures_lite::future;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::delegate;
use sway_things_core::printers::{Bus, Discovered, Discovery, Found};
use sway_things_core::scanners::{self, Error as ScanError, Look, Sane, Scanner};

use super::PrintersPage;
use super::rows::{chevron, device_tile};
use super::scan::ScanDialog;
use crate::i18n::{msg, tr, trf};
use crate::style;
use crate::widgets::list::{heading, section, set_hint};
use crate::widgets::search_button;

/// The style class of the line of a scanner SANE doesn't list.
const UNSEEN: &str = "st-scanner-unseen";

/// How long Avahi is listened to for driverless scanners, at least.
const AVAHI_TIME: Duration = Duration::from_secs(3);

/// Where the page finds SANE.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ScanService {
    /// The scanimage in `PATH`, and Avahi on the system bus.
    #[default]
    System,
    /// The scanimage at `program`, with SANE's configuration in `config` if given, Avahi on the
    /// bus at `bus` if given, and what is installed looked at below `root` (tests).
    At {
        /// The scanimage to run; it may be missing.
        program: PathBuf,
        /// The directory SANE reads its configuration from (`SANE_CONFIG_DIR`).
        config: Option<PathBuf>,
        /// The address of the bus with Avahi.
        bus: Option<String>,
        /// The root of the installation to look at for the hints.
        root: Option<PathBuf>,
    },
    /// No scanners: the section isn't shown (windows in tests that don't need it).
    Off,
}

impl ScanService {
    /// scanimage, if it is there.
    pub(super) fn sane(&self) -> Option<Sane> {
        match self {
            Self::System => Sane::find(),
            Self::At {
                program, config, ..
            } => {
                let sane = Sane::at(program);
                Some(match config {
                    Some(config) => sane.with_env("SANE_CONFIG_DIR", config),
                    None => sane,
                })
            }
            Self::Off => None,
        }
    }

    /// The bus where Avahi is.
    fn bus(&self) -> Bus {
        match self {
            Self::System => Bus::System,
            Self::At {
                bus: Some(address), ..
            } => Bus::Address(address.clone()),
            Self::At { bus: None, .. } | Self::Off => Bus::Off,
        }
    }

    /// What is installed.
    fn look(&self) -> Look {
        match self {
            Self::System => Look::read(),
            Self::At { program, root, .. } => {
                let root = root.clone().unwrap_or_else(|| PathBuf::from("/"));
                let mut look = Look::read_at(&root, None);
                look.scanimage = program.is_file();
                look
            }
            Self::Off => Look::default(),
        }
    }
}

/// What a search found.
#[derive(Debug, Clone, Default)]
pub(super) struct Searched {
    /// The scanners SANE lists.
    pub scanners: Vec<Scanner>,
    /// The driverless scanners Avahi finds that SANE doesn't list.
    pub unseen: Vec<Found>,
    /// Why SANE couldn't be asked, if it couldn't.
    pub error: Option<ScanError>,
    /// The hints for what is missing.
    pub hints: Vec<scanners::Hint>,
}

impl PrintersPage {
    /// Lays out the section of the scanners below the printers.
    pub(super) fn build_scanners(&self) {
        let imp = self.imp();
        let parts = &imp.scan;
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.set_margin_top(24);
        line.set_margin_bottom(8);
        let title = heading(tr(msg!("Scanners")), true);
        title.set_margin_top(0);
        title.set_margin_bottom(0);
        line.append(&title);
        let again = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        again.append(&parts.spinner);
        parts.spinner.set_visible(false);
        parts
            .spinner
            .update_property(&[gtk::accessible::Property::Label(tr(msg!("Searching")))]);
        let icon = gtk::Image::from_icon_name("view-refresh-symbolic");
        again.append(&icon);
        again.append(&gtk::Label::new(Some(tr(msg!("Search Again")))));
        parts.again.set_child(Some(&again));
        parts.again.add_css_class("flat");
        parts
            .again
            .update_property(&[gtk::accessible::Property::Label(tr(msg!("Search Again")))]);
        let weak = self.downgrade();
        parts.again.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.search_scanners();
            }
        });
        parts
            .root
            .append(&search_button::over(&line, &title, &parts.again));
        section(&parts.list, tr(msg!("Scanners")));
        parts.list.add_css_class("st-device-list");
        parts.list.set_activate_on_single_click(true);
        parts.list.set_visible(false);
        let weak = self.downgrade();
        parts.list.connect_row_activated(move |_, row| {
            if let Some(page) = weak.upgrade() {
                page.open_scan_dialog(&row.widget_name());
            }
        });
        parts.root.append(&parts.list);
        parts.status.set_xalign(0.0);
        parts.status.set_wrap(true);
        parts.status.set_margin_start(4);
        parts.status.add_css_class(style::DIM);
        parts.status.set_visible(false);
        parts.root.append(&parts.status);
        parts.hints.set_orientation(gtk::Orientation::Vertical);
        parts.hints.set_spacing(6);
        parts.root.append(&parts.hints);
        if let Some(app) = delegate::scan_app() {
            let label = trf(msg!("Open {app}"), &[("app", &app.name)]);
            parts.app.set_label(&label);
            parts.app.set_tooltip_text(Some(tr(msg!(
                "For what scanning here doesn't do: several pages from the glass, editing, OCR"
            ))));
            parts.app.set_halign(gtk::Align::End);
            parts.app.set_margin_top(10);
            parts.app.connect_clicked(move |_| {
                if let Ok(mut child) = app.tool.launch() {
                    // Waiting reaps it once it ends, so it doesn't stay behind as a zombie.
                    glib::spawn_future_local(async move {
                        let _ = child.status().await;
                    });
                }
            });
            parts.root.append(&parts.app);
        }
        parts.root.set_orientation(gtk::Orientation::Vertical);
        imp.controls.append(&parts.root);
    }

    /// Where the page finds SANE.
    pub(super) fn scan_service(&self) -> ScanService {
        self.imp().scan_service.borrow().clone()
    }

    /// Lists the scanners again, unless a search runs: SANE's and Avahi's, side by side.
    pub fn search_scanners(&self) {
        let imp = self.imp();
        let service = self.scan_service();
        imp.scan.root.set_visible(service != ScanService::Off);
        if service == ScanService::Off || imp.scan.searching.replace(true) {
            return;
        }
        imp.scan.spinner.set_visible(true);
        imp.scan.spinner.set_spinning(true);
        imp.scan.again.set_sensitive(false);
        if imp.scan.found.borrow().is_none() {
            set_hint(
                &imp.scan.status,
                Some(tr(msg!(
                    "Looking for scanners on USB and on the network… This takes a few seconds."
                ))),
            );
        }
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let found = search(&service).await;
            if let Some(page) = weak.upgrade() {
                let imp = page.imp();
                imp.scan.searching.set(false);
                imp.scan.spinner.set_visible(false);
                imp.scan.again.set_sensitive(true);
                imp.scan.found.replace(Some(found));
                page.render_scanners();
            }
        });
    }

    /// Opens the scan dialog for the scanner with the device name `device`, or brings it to
    /// the front.
    pub fn open_scan_dialog(&self, device: &str) {
        let imp = self.imp();
        let open = imp.scan.dialog.borrow().clone();
        if let Some(open) = open {
            if open.window.is_visible() {
                open.window.present();
                return;
            }
        }
        let scanner = imp.scan.found.borrow().as_ref().and_then(|found| {
            found
                .scanners
                .iter()
                .find(|scanner| scanner.device == device)
                .cloned()
        });
        let (Some(scanner), Some(sane)) = (scanner, self.scan_service().sane()) else {
            return;
        };
        let window = self.root().and_downcast::<gtk::Window>();
        let dialog = ScanDialog::open(window.as_ref(), sane, scanner);
        imp.scan.dialog.replace(Some(dialog));
    }

    /// The open scan dialog; for tests.
    pub fn scan_dialog(&self) -> Option<ScanDialog> {
        self.imp()
            .scan
            .dialog
            .borrow()
            .clone()
            .filter(|dialog| dialog.window.is_visible())
    }

    /// Shows what the last search found.
    fn render_scanners(&self) {
        let imp = self.imp();
        let parts = &imp.scan;
        let guard = parts.found.borrow();
        let Some(found) = guard.as_ref() else {
            return;
        };
        while let Some(child) = parts.list.first_child() {
            parts.list.remove(&child);
        }
        for scanner in &found.scanners {
            parts.list.append(&scanner_row(scanner));
        }
        for unseen in &found.unseen {
            parts.list.append(&unseen_row(unseen));
        }
        let rows = found.scanners.len() + found.unseen.len();
        parts.list.set_visible(rows > 0);
        let status = match &found.error {
            Some(ScanError::Missing) | None if rows > 0 => None,
            // scanimage missing: its hint says so.
            Some(ScanError::Missing) => None,
            Some(error) => Some(trf(
                msg!("The scanners can't be listed: {error}"),
                &[("error", &error.text().translated())],
            )),
            None => Some(
                tr(msg!(
                    "No scanner found. Make sure it is on and connected, then search again."
                ))
                .to_owned(),
            ),
        };
        set_hint(&parts.status, status.as_deref());
        while let Some(child) = parts.hints.first_child() {
            parts.hints.remove(&child);
        }
        for hint in &found.hints {
            parts.hints.append(&hint_line(&hint.text().translated()));
        }
    }

    /// The scanners listed: title and subtitle each, with whether SANE lists it; for tests.
    pub fn scanner_rows(&self) -> Vec<(String, String, bool)> {
        let mut listed = Vec::new();
        let mut child = self.imp().scan.list.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            let texts = crate::widgets::list::labels_in(&widget);
            if let (Some(title), Some(subtitle)) = (texts.first(), texts.get(1)) {
                listed.push((
                    title.clone(),
                    subtitle.clone(),
                    !widget.has_css_class(UNSEEN),
                ));
            }
        }
        listed
    }

    /// The hints shown, and the status below the list; for tests.
    pub fn scanner_hints(&self) -> Vec<String> {
        let parts = &self.imp().scan;
        let mut texts = Vec::new();
        if parts.status.is_visible() {
            texts.push(parts.status.text().to_string());
        }
        texts.extend(crate::widgets::list::labels_in(parts.hints.upcast_ref()));
        texts
    }

    /// Whether a search of the scanners runs; for tests.
    pub fn searching_scanners(&self) -> bool {
        self.imp().scan.searching.get()
    }
}

/// Asks SANE and Avahi side by side, and puts the hints together.
async fn search(service: &ScanService) -> Searched {
    let sane = service.sane();
    let listing = async {
        match &sane {
            Some(sane) => sane.list().await,
            None => Err(ScanError::Missing),
        }
    };
    let (listed, around) = future::zip(listing, avahi(&service.bus())).await;
    // Reading what is installed lists directories: off the main context.
    let owned = service.clone();
    let look = gtk::gio::spawn_blocking(move || ScanService::look(&owned))
        .await
        .unwrap_or_default();
    let (scanners, error) = match listed {
        Ok(scanners) => (scanners, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    let unseen: Vec<Found> = around
        .into_iter()
        .filter(|found| !scanners::sees(&scanners, found))
        .collect();
    let names: Vec<String> = unseen
        .iter()
        .map(|found| found.title().to_owned())
        .collect();
    let hints = look.hints(!scanners.is_empty(), &names);
    Searched {
        scanners,
        unseen,
        error,
        hints,
    }
}

/// The driverless scanners Avahi reports within [`AVAHI_TIME`]; none without Avahi.
async fn avahi(bus: &Bus) -> Vec<Found> {
    let Ok(mut discovery) = Discovery::scanners(bus).await else {
        return Vec::new();
    };
    let mut found: Vec<Found> = Vec::new();
    let deadline = std::time::Instant::now() + AVAHI_TIME;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break;
        }
        let event = future::or(async { discovery.next().await }, async {
            glib::timeout_future(left).await;
            None
        })
        .await;
        match event {
            Some(Discovered::Added(scanner)) => {
                found.retain(|known| known.key != scanner.key);
                found.push(scanner);
            }
            Some(Discovered::Removed(key)) => found.retain(|known| known.key != key),
            None => break,
        }
    }
    found.sort_by(|a, b| a.title().cmp(b.title()));
    found
}

/// The line of a scanner SANE lists.
fn scanner_row(scanner: &Scanner) -> gtk::ListBoxRow {
    let title = scanner.title();
    let subtitle = connection_text(scanner);
    let line = device_line("scanner-symbolic", &title, &subtitle, None);
    line.append(&chevron());
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&line));
    row.set_activatable(true);
    row.set_widget_name(&scanner.device);
    row.set_tooltip_text(Some(&trf(
        msg!("Scan with {device}"),
        &[("device", &scanner.device)],
    )));
    row.update_property(&[
        gtk::accessible::Property::Label(&title),
        gtk::accessible::Property::Description(&subtitle),
    ]);
    row
}

/// How `scanner` is connected, in words; for one SANE can't tell, what SANE says it is
/// (`Virtual device` for SANE's test backend).
pub(super) fn connection_text(scanner: &Scanner) -> String {
    match scanner.connection() {
        scanners::Connection::Other if !scanner.kind.trim().is_empty() => {
            let kind = scanner.kind.trim();
            let mut chars = kind.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        }
        connection => connection.text().translated(),
    }
}

/// The line of a driverless scanner Avahi finds that SANE doesn't list: it can't be used yet.
fn unseen_row(found: &Found) -> gtk::ListBoxRow {
    let mut said = vec![
        tr(if found.local {
            msg!("USB, driverless")
        } else {
            msg!("Network, driverless")
        })
        .to_owned(),
    ];
    said.push(tr(msg!("SANE doesn't see it yet")).to_owned());
    let subtitle = said.join(" · ");
    let line = device_line(
        "scanner-symbolic",
        found.title(),
        &subtitle,
        Some(tr(msg!("See the hint below the list"))),
    );
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&line));
    row.set_activatable(false);
    row.set_focusable(false);
    row.add_css_class(UNSEEN);
    row.update_property(&[
        gtk::accessible::Property::Label(found.title()),
        gtk::accessible::Property::Description(&subtitle),
    ]);
    row
}

/// A device's line: its icon on a tile, `title` above `subtitle`, and a warning sign with
/// `problem` as its tooltip.
fn device_line(icon: &str, title: &str, subtitle: &str, problem: Option<&str>) -> gtk::Box {
    let title = gtk::Label::new(Some(title));
    title.add_css_class("st-row-title");
    let subtitle = gtk::Label::new(Some(subtitle));
    subtitle.add_css_class(style::DIM);
    subtitle.add_css_class("st-device-subtitle");
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    for label in [&title, &subtitle] {
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        texts.append(label);
    }
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.add_css_class("st-device-row");
    line.append(&device_tile(icon));
    line.append(&texts);
    if let Some(problem) = problem {
        let sign = gtk::Image::from_icon_name("dialog-warning-symbolic");
        sign.add_css_class("st-device-warning");
        sign.set_valign(gtk::Align::Center);
        sign.set_tooltip_text(Some(problem));
        sign.set_accessible_role(gtk::AccessibleRole::Presentation);
        line.append(&sign);
    }
    line
}

/// A hint: a warning sign and its text.
fn hint_line(text: &str) -> gtk::Box {
    let sign = gtk::Image::from_icon_name("dialog-warning-symbolic");
    sign.set_valign(gtk::Align::Start);
    sign.set_accessible_role(gtk::AccessibleRole::Presentation);
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_hexpand(true);
    label.set_selectable(true);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.add_css_class("st-device-hint");
    line.append(&sign);
    line.append(&label);
    line
}
