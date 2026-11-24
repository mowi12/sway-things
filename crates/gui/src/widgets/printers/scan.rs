// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Scanning a document into a file, opened by a click on a scanner's line:
//!
//! - on the left, the preview: a quick scan at a low resolution, then the last page scanned;
//! - on the right, what the scanner offers, read with `scanimage -A`: the source (glass or
//!   document feeder), the colors (color, gray, black and white), the resolution and the paper
//!   size; and the file format: PDF, PNG, JPEG or TIFF, as far as this scanimage writes them;
//! - _Scan_ scans a page, or every sheet in the feeder for a PDF, with its progress and _Cancel_;
//!   a PDF takes more pages with _Scan Another Page_; _Save…_ asks where in a file chooser
//!   (Documents, `Scan 2026-11-22 09.31.pdf`), which asks before it replaces a file, and only
//!   that file is written; then _Open_ and _Show in Folder_.
//!
//! A scan runs in a directory of our own in the cache, removed when the dialog closes; closing
//! the dialog cancels a scan that runs. Errors say what to do in plain words, with what
//! scanimage said under _Details_.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use sway_things_core::scanners::options::{self, Paper};
use sway_things_core::scanners::{
    Canceler, Error as ScanError, Format, ModeKind, Options, Progress, Request, Sane, SaveAs,
    ScanDir, Scanner, SourceKind, save,
};

use crate::i18n::{Msg, msg, msg_plural, ntrf, tr, trf};
use crate::style;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The primary button before anything is scanned.
const SCAN: Msg = msg!("Scan");
/// The button that saves what was scanned.
const SAVE: Msg = msg!("Save…");

/// What a scan is saved as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// A PDF, of one page or several.
    Pdf,
    /// A PNG image.
    Png,
    /// A JPEG image.
    Jpeg,
    /// A TIFF image.
    Tiff,
}

impl FileType {
    /// Its name in the list and its file extension.
    fn name(self) -> (&'static str, &'static str) {
        match self {
            Self::Pdf => ("PDF", "pdf"),
            Self::Png => ("PNG", "png"),
            Self::Jpeg => ("JPEG", "jpg"),
            Self::Tiff => ("TIFF", "tif"),
        }
    }

    /// The format scanimage writes for it, in the mode `mode`: for a PDF, JPEG pages, or PNM
    /// bitmaps in black and white.
    fn format(self, mode: Option<ModeKind>) -> Format {
        match self {
            Self::Pdf if mode == Some(ModeKind::Lineart) => Format::Pnm,
            Self::Pdf | Self::Jpeg => Format::Jpeg,
            Self::Png => Format::Png,
            Self::Tiff => Format::Tiff,
        }
    }
}

/// The open scan dialog.
#[derive(Clone)]
pub struct ScanDialog {
    /// Its window.
    pub window: gtk::Window,
    parts: Rc<Parts>,
}

impl std::fmt::Debug for ScanDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScanDialog")
            .field("device", &self.parts.scanner.device)
            .finish_non_exhaustive()
    }
}

/// A choice of a drop-down: its label, and what it stands for.
type Choices<T> = RefCell<Vec<(String, T)>>;

/// The parts of the dialog, and what it keeps.
struct Parts {
    sane: Sane,
    scanner: Scanner,
    options: RefCell<Options>,
    /// The picture: the preview, or the last page scanned.
    picture: gtk::Picture,
    /// What the frame says while it has no picture.
    empty: gtk::Label,
    /// The option rows: their label and drop-down.
    source: (gtk::Label, gtk::DropDown),
    mode: (gtk::Label, gtk::DropDown),
    resolution: (gtk::Label, gtk::DropDown),
    paper: (gtk::Label, gtk::DropDown),
    format: gtk::DropDown,
    sources: Choices<Option<String>>,
    modes: Choices<Option<(String, ModeKind)>>,
    resolutions: Choices<Option<u32>>,
    papers: Choices<Option<Paper>>,
    formats: Choices<FileType>,
    /// What a PDF from the feeder does, or an image, and why some formats are missing.
    note: gtk::Label,
    /// Why some formats are missing, if some are.
    missing: RefCell<String>,
    /// Whether this scanimage writes PNG, for the preview.
    writes_png: Cell<bool>,
    /// What is going on: reading the options, scanning page 2, saved.
    status: gtk::Label,
    progress: gtk::ProgressBar,
    error: gtk::Label,
    details: gtk::Expander,
    details_text: gtk::Label,
    /// The file saved last, with _Open_ and _Show in Folder_.
    saved: gtk::Box,
    saved_label: gtk::Label,
    preview: gtk::Button,
    cancel: gtk::Button,
    discard: gtk::Button,
    another: gtk::Button,
    primary: gtk::Button,
    /// Where the pages go until they are saved.
    dir: RefCell<Option<ScanDir>>,
    /// The pages scanned and not saved yet, with their resolution, and what they are for.
    pages: RefCell<Vec<(PathBuf, u32)>>,
    pages_type: Cell<Option<FileType>>,
    /// The scan that runs.
    running: RefCell<Option<Canceler>>,
    /// Whether the pages are being saved, off the main loop.
    saving: Cell<bool>,
    /// The file saved last.
    saved_path: RefCell<Option<PathBuf>>,
    /// Whether the dialog closed: what still runs drops its result.
    closed: Cell<bool>,
}

impl ScanDialog {
    /// Opens the dialog for `scanner` over `parent`, scanning with `sane`.
    #[expect(
        clippy::too_many_lines,
        reason = "the parts of the dialog, laid out in the order they show"
    )]
    pub(super) fn open(parent: Option<&gtk::Window>, sane: Sane, scanner: Scanner) -> Self {
        let title = scanner.title();
        let window = gtk::Window::builder()
            .modal(true)
            .resizable(true)
            .destroy_with_parent(true)
            .default_width(760)
            .title(trf(msg!("Scan with {scanner}"), &[("scanner", &title)]))
            .build();
        window.set_transient_for(parent);
        window.add_css_class("st-dialog");
        window.add_css_class("st-scan-dialog");

        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_can_shrink(true);
        picture.set_size_request(260, 340);
        picture.set_alternative_text(Some(tr(msg!("Preview of the scan"))));
        let empty = gtk::Label::new(Some(tr(msg!(
            "Put the document on the glass, then preview it here."
        ))));
        empty.set_wrap(true);
        empty.set_justify(gtk::Justification::Center);
        empty.set_max_width_chars(24);
        empty.add_css_class(style::DIM);
        let frame = gtk::Overlay::new();
        frame.set_child(Some(&picture));
        frame.add_overlay(&empty);
        frame.add_css_class("st-scan-preview");
        frame.set_valign(gtk::Align::Start);

        let grid = gtk::Grid::builder()
            .column_spacing(12)
            .row_spacing(10)
            .valign(gtk::Align::Start)
            .hexpand(true)
            .build();
        let option_row = |line: i32, name: Msg| {
            let label = gtk::Label::new(Some(tr(name)));
            label.set_xalign(0.0);
            label.add_css_class(style::DIM);
            let drop_down = gtk::DropDown::from_strings(&[]);
            drop_down.set_hexpand(true);
            drop_down
                .update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
            grid.attach(&label, 0, line, 1, 1);
            grid.attach(&drop_down, 1, line, 1, 1);
            (label, drop_down)
        };
        let source = option_row(0, msg!("Source"));
        let mode = option_row(1, msg!("Colors"));
        let resolution = option_row(2, msg!("Resolution"));
        let paper = option_row(3, msg!("Paper size"));
        let (_, format) = option_row(4, msg!("Save as"));
        let note = gtk::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_max_width_chars(36);
        note.add_css_class(style::DIM);
        note.add_css_class("st-scan-note");
        grid.attach(&note, 0, 5, 2, 1);

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_max_width_chars(36);
        let progress = gtk::ProgressBar::new();
        progress.set_visible(false);
        let error = gtk::Label::new(None);
        error.set_xalign(0.0);
        error.set_wrap(true);
        error.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        error.set_max_width_chars(36);
        error.add_css_class("st-scan-error");
        error.set_visible(false);
        let details_text = gtk::Label::new(None);
        details_text.set_xalign(0.0);
        details_text.set_wrap(true);
        details_text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        details_text.set_selectable(true);
        details_text.set_max_width_chars(36);
        details_text.add_css_class("monospace");
        let details = gtk::Expander::new(Some(tr(msg!("Details"))));
        details.set_child(Some(&details_text));
        details.set_visible(false);
        let saved_label = gtk::Label::new(None);
        saved_label.set_xalign(0.0);
        saved_label.set_wrap(true);
        saved_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        saved_label.set_max_width_chars(36);
        let open_file = gtk::Button::with_label(tr(msg!("Open")));
        let show_folder = gtk::Button::with_label(tr(msg!("Show in Folder")));
        let saved_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        saved_buttons.append(&open_file);
        saved_buttons.append(&show_folder);
        let saved = gtk::Box::new(gtk::Orientation::Vertical, 8);
        saved.append(&saved_label);
        saved.append(&saved_buttons);
        saved.set_visible(false);

        let side = gtk::Box::new(gtk::Orientation::Vertical, 12);
        side.set_hexpand(true);
        side.append(&grid);
        side.append(&status);
        side.append(&progress);
        side.append(&error);
        side.append(&details);
        side.append(&saved);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, 20);
        body.append(&frame);
        body.append(&side);

        let heading = gtk::Label::new(Some(&title));
        heading.set_wrap(true);
        heading.set_justify(gtk::Justification::Center);
        heading.add_css_class("title-2");
        heading.set_accessible_role(gtk::AccessibleRole::Heading);
        let subtitle = gtk::Label::new(Some(&format!(
            "{} · {}",
            super::scanners::connection_text(&scanner),
            scanner.device
        )));
        subtitle.add_css_class(style::DIM);
        subtitle.set_wrap(true);
        subtitle.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        subtitle.set_justify(gtk::Justification::Center);

        let preview = gtk::Button::with_label(tr(msg!("Preview")));
        let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
        cancel.set_visible(false);
        let discard = gtk::Button::with_label(tr(msg!("Discard")));
        discard.set_visible(false);
        let close = gtk::Button::with_label(tr(msg!("Close")));
        let another = gtk::Button::with_label(tr(msg!("Scan Another Page")));
        another.set_visible(false);
        let primary = gtk::Button::with_label(tr(SCAN));
        primary.add_css_class("suggested-action");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_margin_top(8);
        buttons.append(&preview);
        buttons.append(&discard);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        buttons.append(&spacer);
        buttons.append(&close);
        buttons.append(&cancel);
        buttons.append(&another);
        buttons.append(&primary);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(20);
        content.set_margin_bottom(16);
        content.set_margin_start(20);
        content.set_margin_end(20);
        content.append(&heading);
        content.append(&subtitle);
        content.append(&body);
        content.append(&buttons);
        window.set_child(Some(&content));
        window.set_default_widget(Some(&primary));

        let dialog = Self {
            window,
            parts: Rc::new(Parts {
                sane,
                scanner,
                options: RefCell::new(Options::default()),
                picture,
                empty,
                source,
                mode,
                resolution,
                paper,
                format,
                sources: RefCell::new(Vec::new()),
                modes: RefCell::new(Vec::new()),
                resolutions: RefCell::new(Vec::new()),
                papers: RefCell::new(Vec::new()),
                formats: RefCell::new(Vec::new()),
                note,
                missing: RefCell::new(String::new()),
                writes_png: Cell::new(true),
                status,
                progress,
                error,
                details,
                details_text,
                saved,
                saved_label,
                preview,
                cancel,
                discard,
                another,
                primary,
                dir: RefCell::new(None),
                pages: RefCell::new(Vec::new()),
                pages_type: Cell::new(None),
                running: RefCell::new(None),
                saving: Cell::new(false),
                saved_path: RefCell::new(None),
                closed: Cell::new(false),
            }),
        };
        dialog.connect(&close, &open_file, &show_folder);
        dialog.read_options();
        GtkWindowExt::set_focus(&dialog.window, Some(&dialog.parts.primary));
        dialog.window.present();
        dialog
    }

    /// Connects the buttons, the drop-downs and the ways of closing.
    fn connect(&self, close: &gtk::Button, open_file: &gtk::Button, show_folder: &gtk::Button) {
        let parts = &self.parts;
        let dialog = self.clone();
        parts.preview.connect_clicked(move |_| dialog.scan(true));
        let dialog = self.clone();
        parts.primary.connect_clicked(move |_| {
            if dialog.parts.pages.borrow().is_empty() {
                dialog.scan(false);
            } else {
                dialog.ask_where();
            }
        });
        let dialog = self.clone();
        parts.another.connect_clicked(move |_| dialog.scan(false));
        let dialog = self.clone();
        parts.discard.connect_clicked(move |_| dialog.discard());
        let dialog = self.clone();
        parts.cancel.connect_clicked(move |_| dialog.cancel());
        let dialog = self.clone();
        close.connect_clicked(move |_| dialog.close());
        for drop_down in [&parts.source.1, &parts.mode.1, &parts.format] {
            let dialog = self.clone();
            drop_down.connect_selected_notify(move |_| dialog.update());
        }
        let dialog = self.clone();
        open_file.connect_clicked(move |_| dialog.launch(false));
        let dialog = self.clone();
        show_folder.connect_clicked(move |_| dialog.launch(true));
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
                // Escape cancels a scan that runs, else closes.
                if dialog.parts.running.borrow().is_some() {
                    dialog.cancel();
                } else {
                    dialog.close();
                }
                glib::Propagation::Stop
            })),
        ));
        self.window.add_controller(escape);
    }

    /// Closes the dialog: a scan that runs is canceled, and the pages not saved are removed.
    pub fn close(&self) {
        self.closing();
        self.window.destroy();
    }

    fn closing(&self) {
        self.parts.closed.set(true);
        self.cancel();
        // A running scan still writes into the directory, and a save still reads from it: it
        // goes once they have ended.
        if self.parts.running.borrow().is_none() && !self.parts.saving.get() {
            self.parts.dir.replace(None);
        }
    }

    /// Reads what the scanner can do, and offers it.
    fn read_options(&self) {
        let parts = &self.parts;
        parts
            .status
            .set_text(tr(msg!("Reading what the scanner can do…")));
        parts.preview.set_sensitive(false);
        parts.primary.set_sensitive(false);
        let dialog = self.clone();
        glib::spawn_future_local(async move {
            let sane = dialog.parts.sane.clone();
            let device = dialog.parts.scanner.device.clone();
            let (options, formats) =
                futures_lite::future::zip(sane.options(&device), sane.formats()).await;
            if dialog.parts.closed.get() {
                return;
            }
            dialog.parts.status.set_text("");
            match options {
                Ok(options) => {
                    dialog.parts.options.replace(options);
                }
                Err(error) => dialog.show_error(&error),
            }
            dialog.offer(&formats);
            dialog.parts.primary.set_sensitive(true);
            dialog.update();
        });
    }

    /// Fills the drop-downs with what the scanner and scanimage offer; an option the scanner
    /// doesn't have isn't shown.
    fn offer(&self, formats: &[Format]) {
        let parts = &self.parts;
        let options = parts.options.borrow().clone();
        let sources: Vec<(String, Option<String>)> = options
            .sources
            .iter()
            .map(|source| {
                (
                    source_label(&source.value, source.kind),
                    Some(source.value.clone()),
                )
            })
            .collect();
        let selected = options.source.as_ref().and_then(|now| {
            sources
                .iter()
                .position(|(_, value)| value.as_ref() == Some(now))
        });
        fill(&parts.source, &parts.sources, sources, selected);

        let modes: Vec<(String, Option<(String, ModeKind)>)> = options
            .modes
            .iter()
            .map(|mode| {
                let label = tr(match mode.kind {
                    ModeKind::Color => msg!("Color"),
                    ModeKind::Gray => msg!("Gray"),
                    ModeKind::Lineart => msg!("Black and white"),
                });
                (label.to_owned(), Some((mode.value.clone(), mode.kind)))
            })
            .collect();
        // Color for documents, as scan apps start.
        let selected = modes.iter().position(|(_, mode)| {
            mode.as_ref()
                .is_some_and(|(_, kind)| *kind == ModeKind::Color)
        });
        fill(&parts.mode, &parts.modes, modes, selected);

        let resolutions: Vec<(String, Option<u32>)> = options
            .resolutions
            .iter()
            .map(|dpi| (trf(msg!("{dpi} dpi"), &[("dpi", dpi)]), Some(*dpi)))
            .collect();
        let selected = options.document_resolution().and_then(|dpi| {
            resolutions
                .iter()
                .position(|(_, value)| *value == Some(dpi))
        });
        fill(&parts.resolution, &parts.resolutions, resolutions, selected);

        let mut papers: Vec<(String, Option<Paper>)> = Vec::new();
        if let Some(area) = options.area {
            papers = options::papers_for(area)
                .into_iter()
                .map(|paper| (paper.name.to_owned(), Some(paper)))
                .collect();
            papers.push((tr(msg!("Whole area")).to_owned(), None));
        }
        let locale = ["LC_ALL", "LC_PAPER", "LANG"]
            .iter()
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
            .unwrap_or_default();
        let wanted = options::default_paper(&locale);
        let selected = papers
            .iter()
            .position(|(_, paper)| paper.is_some_and(|paper| paper.name == wanted))
            .or_else(|| papers.len().checked_sub(1));
        fill(&parts.paper, &parts.papers, papers, selected);

        self.offer_formats(formats);
    }

    /// Fills the drop-down of the file types with those this scanimage writes, and says why
    /// others are missing.
    fn offer_formats(&self, formats: &[Format]) {
        let parts = &self.parts;
        parts.writes_png.set(formats.contains(&Format::Png));
        let mut types = Vec::new();
        if formats.contains(&Format::Jpeg) {
            types.push(FileType::Pdf);
        }
        for (format, file_type) in [
            (Format::Png, FileType::Png),
            (Format::Jpeg, FileType::Jpeg),
            (Format::Tiff, FileType::Tiff),
        ] {
            if formats.contains(&format) {
                types.push(file_type);
            }
        }
        let names: Vec<&str> = types.iter().map(|file_type| file_type.name().0).collect();
        parts.format.set_model(Some(&gtk::StringList::new(&names)));
        parts.format.set_selected(0);
        parts.format.set_sensitive(!types.is_empty());
        let missing: Vec<&str> = [FileType::Pdf, FileType::Png, FileType::Jpeg, FileType::Tiff]
            .into_iter()
            .filter(|file_type| !types.contains(file_type))
            .map(|file_type| file_type.name().0)
            .collect();
        parts.formats.replace(
            types
                .into_iter()
                .map(|file_type| (file_type.name().0.to_owned(), file_type))
                .collect(),
        );
        if !missing.is_empty() {
            parts.missing.replace(trf(
                msg!(
                    "This scanimage doesn't write {formats}: it was built without them. PDF needs \
                     JPEG."
                ),
                &[("formats", &missing.join(", "))],
            ));
        }
    }

    /// The choice of the drop-down `which`.
    fn chosen<T: Clone>(choices: &Choices<Option<T>>, which: &gtk::DropDown) -> Option<T> {
        let index = usize::try_from(which.selected()).ok()?;
        choices
            .borrow()
            .get(index)
            .and_then(|(_, value)| value.clone())
    }

    /// The file type chosen.
    fn file_type(&self) -> Option<FileType> {
        let index = usize::try_from(self.parts.format.selected()).ok()?;
        self.parts
            .formats
            .borrow()
            .get(index)
            .map(|(_, file_type)| *file_type)
    }

    /// Whether the chosen source feeds sheets.
    fn feeds(&self) -> bool {
        let source = Self::chosen(&self.parts.sources, &self.parts.source.1);
        self.parts
            .options
            .borrow()
            .sources
            .iter()
            .any(|known| Some(&known.value) == source.as_ref() && known.kind.feeds())
    }

    /// Shows what can be done now.
    fn update(&self) {
        let parts = &self.parts;
        // While the pages are saved, nothing may scan, discard or save them again.
        let running = parts.running.borrow().is_some() || parts.saving.get();
        let pages = parts.pages.borrow().len();
        let file_type = self.file_type();
        let feeds = self.feeds();
        // A preview from the feeder would take a sheet.
        parts
            .preview
            .set_sensitive(!running && !feeds && file_type.is_some());
        parts
            .preview
            .set_tooltip_text(feeds.then(|| tr(msg!("Preview needs the glass"))));
        parts.cancel.set_visible(running && !parts.saving.get());
        parts.primary.set_visible(!running);
        parts
            .primary
            .set_label(tr(if pages == 0 { SCAN } else { SAVE }));
        parts.primary.set_sensitive(!running && file_type.is_some());
        parts.another.set_visible(!running && pages > 0);
        parts.discard.set_visible(!running && pages > 0);
        // What was scanned stays as it was scanned.
        let settled = running || pages > 0;
        for (_, drop_down) in [&parts.source, &parts.mode, &parts.resolution, &parts.paper] {
            drop_down.set_sensitive(!running);
        }
        parts
            .format
            .set_sensitive(!settled && !parts.formats.borrow().is_empty());
        let mut notes = Vec::new();
        if file_type == Some(FileType::Pdf) && feeds && pages == 0 {
            notes.push(tr(msg!(
                "Every sheet in the feeder goes into the PDF; scan more with Scan Another Page."
            )));
        } else if file_type.is_some_and(|file_type| file_type != FileType::Pdf) && pages == 0 {
            notes.push(tr(msg!(
                "An image holds one page; save as PDF for several."
            )));
        }
        let missing = parts.missing.borrow();
        if !missing.is_empty() {
            notes.push(missing.as_str());
        }
        parts.note.set_text(&notes.join(" "));
        parts.note.set_visible(!notes.is_empty());
        let pdf = parts.pages_type.get() == Some(FileType::Pdf);
        parts.another.set_label(tr(if pdf {
            msg!("Scan Another Page")
        } else {
            msg!("Scan Again")
        }));
        if !running && pages > 0 && !pdf {
            parts
                .status
                .set_text(tr(msg!("The page is scanned. Save it, or scan it again.")));
        } else if !running && pages > 0 {
            parts.status.set_text(&ntrf(
                msg_plural!(
                    "{count} page scanned. Save it, or scan another.",
                    "{count} pages scanned. Save them, or scan another."
                ),
                pages,
                &[("count", &pages)],
            ));
        }
        parts.status.set_visible(!parts.status.text().is_empty());
    }

    /// The request for a scan, or a preview at a low resolution.
    fn request(&self, preview: bool) -> Option<Request> {
        let parts = &self.parts;
        let file_type = self.file_type()?;
        let mode = Self::chosen(&parts.modes, &parts.mode.1);
        let format = if preview {
            // PNG if this scanimage writes it, else PNM, which every one does.
            if parts.writes_png.get() {
                Format::Png
            } else {
                Format::Pnm
            }
        } else {
            file_type.format(mode.as_ref().map(|(_, kind)| *kind))
        };
        let resolution = if preview {
            parts.options.borrow().preview_resolution()
        } else {
            Self::chosen(&parts.resolutions, &parts.resolution.1)
        };
        Some(Request {
            device: parts.scanner.device.clone(),
            source: Self::chosen(&parts.sources, &parts.source.1),
            mode: mode.map(|(value, _)| value),
            resolution,
            area: Self::chosen(&parts.papers, &parts.paper.1).map(|paper| paper.size),
            format,
            all_pages: !preview && file_type == FileType::Pdf && self.feeds(),
        })
    }

    /// Scans a page (or the feeder's sheets), or a preview.
    fn scan(&self, preview: bool) {
        let parts = &self.parts;
        if parts.running.borrow().is_some() || parts.saving.get() {
            return;
        }
        let Some(request) = self.request(preview) else {
            return;
        };
        self.show_error_text("", "");
        parts.saved.set_visible(false);
        let Some(batch) = self.batch() else {
            return;
        };
        let scan = match parts.sane.scan(&request, &batch) {
            Ok(scan) => scan,
            Err(error) => {
                self.show_error(&error);
                return;
            }
        };
        parts.running.replace(Some(scan.canceler()));
        parts.progress.set_fraction(0.0);
        parts.progress.set_visible(true);
        parts.status.set_text(tr(if preview {
            msg!("Previewing…")
        } else {
            msg!("Scanning…")
        }));
        self.update();
        let file_type = self.file_type();
        let dialog = self.clone();
        glib::spawn_future_local(async move {
            let parts = &dialog.parts;
            let page_now = Cell::new(1u32);
            let result = scan
                .finish(|progress| {
                    if parts.closed.get() {
                        return;
                    }
                    match progress {
                        Progress::Page(page) => page_now.set(page),
                        Progress::Percent(percent) => {
                            parts.progress.set_fraction(percent / 100.0);
                            if !preview {
                                parts.status.set_text(&trf(
                                    msg!("Scanning page {page}… {percent} %"),
                                    &[("page", &page_now.get()), ("percent", &percent.round())],
                                ));
                            }
                        }
                    }
                })
                .await;
            parts.running.replace(None);
            if parts.closed.get() {
                parts.dir.replace(None);
                return;
            }
            parts.progress.set_visible(false);
            parts.status.set_text("");
            match result {
                Ok(files) => {
                    if let Some(last) = files.last() {
                        dialog.show_picture(last);
                    }
                    if !preview {
                        // Without a resolution chosen, the scanner's own.
                        let dpi = request
                            .resolution
                            .or(parts.options.borrow().resolution)
                            .unwrap_or(300);
                        // An image holds one page: a new scan replaces it.
                        if file_type != Some(FileType::Pdf) {
                            parts.pages.borrow_mut().clear();
                        }
                        parts
                            .pages
                            .borrow_mut()
                            .extend(files.into_iter().map(|file| (file, dpi)));
                        parts.pages_type.set(file_type);
                    }
                }
                Err(ScanError::Canceled) => {
                    parts.status.set_text(tr(msg!("Canceled.")));
                }
                Err(error) => dialog.show_error(&error),
            }
            dialog.update();
        });
    }

    /// A new directory for the pages of a scan, in the dialog's own; `None` if it can't be
    /// made, which the dialog shows.
    fn batch(&self) -> Option<PathBuf> {
        let mut dir = self.parts.dir.borrow_mut();
        if dir.is_none() {
            match ScanDir::new() {
                Ok(made) => *dir = Some(made),
                Err(error) => {
                    drop(dir);
                    self.show_error(&ScanError::Io(error.to_string()));
                    return None;
                }
            }
        }
        let batch = dir.as_ref().map(ScanDir::batch);
        drop(dir);
        match batch {
            Some(Ok(batch)) => Some(batch),
            Some(Err(error)) => {
                self.show_error(&ScanError::Io(error.to_string()));
                None
            }
            None => None,
        }
    }

    /// Asks the scan that runs to stop.
    fn cancel(&self) {
        if let Some(canceler) = self.parts.running.borrow().as_ref() {
            canceler.cancel();
            self.parts.status.set_text(tr(msg!("Canceling…")));
        }
    }

    /// Forgets the pages scanned and not saved.
    fn discard(&self) {
        let parts = &self.parts;
        parts.pages.borrow_mut().clear();
        parts.pages_type.set(None);
        parts.status.set_text("");
        parts.picture.set_paintable(gtk::gdk::Paintable::NONE);
        parts.empty.set_visible(true);
        self.update();
    }

    /// Shows the image file `path` in the frame.
    fn show_picture(&self, path: &Path) {
        let parts = &self.parts;
        if let Ok(texture) = gtk::gdk::Texture::from_filename(path) {
            parts.picture.set_paintable(Some(&texture));
            parts.empty.set_visible(false);
        } else {
            parts.picture.set_paintable(gtk::gdk::Paintable::NONE);
            parts
                .empty
                .set_text(tr(msg!("Scanned. The page can't be shown here.")));
            parts.empty.set_visible(true);
        }
    }

    /// Asks where to save, in a file chooser that starts in the documents folder with a dated
    /// name and asks before it replaces a file.
    fn ask_where(&self) {
        let Some(file_type) = self.parts.pages_type.get() else {
            return;
        };
        let (_, extension) = file_type.name();
        let stamp = glib::DateTime::now_local()
            .ok()
            .and_then(|now| now.format("%Y-%m-%d %H.%M").ok())
            .map(|text| text.to_string())
            .unwrap_or_default();
        let name = format!(
            "{}.{extension}",
            trf(msg!("Scan {date}"), &[("date", &stamp)]).trim()
        );
        let folder = glib::user_special_dir(glib::UserDirectory::Documents)
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(glib::home_dir);
        let chooser = gtk::FileDialog::builder()
            .title(tr(msg!("Save the Scan")))
            .modal(true)
            .initial_name(name)
            .initial_folder(&gio::File::for_path(folder))
            .accept_label(tr(msg!("Save")))
            .build();
        let dialog = self.clone();
        chooser.save(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let Some(path) = result.ok().and_then(|file| file.path()) {
                dialog.save_to(&path);
            }
        });
    }

    /// Saves the pages into `path`, the file the user picked, off the main loop: a PDF of many
    /// pages can take a while to read and write.
    pub fn save_to(&self, path: &Path) {
        let parts = &self.parts;
        let Some(file_type) = parts.pages_type.get() else {
            return;
        };
        if parts.saving.get() || parts.running.borrow().is_some() {
            return;
        }
        let save_as = if file_type == FileType::Pdf {
            SaveAs::Pdf
        } else {
            SaveAs::Image
        };
        let pages = parts.pages.borrow().clone();
        parts.saving.set(true);
        self.show_error_text("", "");
        parts.status.set_text(tr(msg!("Saving…")));
        self.update();
        let dialog = self.clone();
        let path = path.to_owned();
        glib::spawn_future_local(async move {
            let result = {
                let path = path.clone();
                gio::spawn_blocking(move || save(&pages, save_as, &path)).await
            }
            .unwrap_or_else(|_| Err(ScanError::Io("the save stopped".to_owned())));
            dialog.saved(&path, result);
        });
    }

    /// What became of saving into `path`.
    fn saved(&self, path: &Path, result: Result<(), ScanError>) {
        let parts = &self.parts;
        parts.saving.set(false);
        if parts.closed.get() {
            // The dialog closed meanwhile: the pages go now.
            parts.dir.replace(None);
            return;
        }
        match result {
            Ok(()) => {
                parts.pages.borrow_mut().clear();
                parts.pages_type.set(None);
                parts.saved_path.replace(Some(path.to_owned()));
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                parts
                    .saved_label
                    .set_text(&trf(msg!("Saved as “{file}”."), &[("file", &name)]));
                parts.saved.set_visible(true);
                parts.status.set_text("");
            }
            Err(error) => {
                parts.status.set_text("");
                self.show_error(&error);
            }
        }
        self.update();
    }

    /// Opens the file saved last, or the folder it is in, with the desktop's app.
    fn launch(&self, folder: bool) {
        let Some(path) = self.parts.saved_path.borrow().clone() else {
            return;
        };
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&path)));
        let dialog = self.clone();
        let done = move |result: Result<(), glib::Error>| {
            if let Err(error) = result {
                dialog.show_error_text(
                    &trf(msg!("It can't be opened: {error}"), &[("error", &error)]),
                    "",
                );
            }
        };
        if folder {
            launcher.open_containing_folder(Some(&self.window), gio::Cancellable::NONE, done);
        } else {
            launcher.launch(Some(&self.window), gio::Cancellable::NONE, done);
        }
    }

    /// Shows `error`, with what scanimage said under _Details_.
    fn show_error(&self, error: &ScanError) {
        self.show_error_text(&error.text().translated(), error.details());
    }

    fn show_error_text(&self, text: &str, details: &str) {
        let parts = &self.parts;
        parts.error.set_text(text);
        parts.error.set_visible(!text.is_empty());
        parts.details_text.set_text(details);
        parts.details.set_visible(!details.is_empty());
        parts.details.set_expanded(false);
    }

    /// The error shown, and the details; for tests.
    pub fn error_text(&self) -> (String, String) {
        let parts = &self.parts;
        (
            if parts.error.is_visible() {
                parts.error.text().to_string()
            } else {
                String::new()
            },
            parts.details_text.text().to_string(),
        )
    }

    /// The status line; for tests.
    pub fn status_text(&self) -> String {
        self.parts.status.text().to_string()
    }

    /// The pages scanned and not saved; for tests.
    pub fn page_count(&self) -> usize {
        self.parts.pages.borrow().len()
    }

    /// Whether a scan runs; for tests.
    pub fn is_scanning(&self) -> bool {
        self.parts.running.borrow().is_some()
    }

    /// Whether the pages are being saved; for tests.
    pub fn is_saving(&self) -> bool {
        self.parts.saving.get()
    }

    /// Whether the options are read and a scan can start; for tests.
    pub fn is_ready(&self) -> bool {
        self.parts.primary.is_sensitive() || !self.parts.pages.borrow().is_empty()
    }

    /// Whether the frame shows a picture; for tests.
    pub fn has_picture(&self) -> bool {
        self.parts.picture.paintable().is_some()
    }

    /// The labels each drop-down offers: source, colors, resolution, paper and format; for
    /// tests.
    pub fn offered(&self) -> [Vec<String>; 5] {
        let parts = &self.parts;
        [
            labels(&parts.sources),
            labels(&parts.modes),
            labels(&parts.resolutions),
            labels(&parts.papers),
            parts
                .formats
                .borrow()
                .iter()
                .map(|(label, _)| label.clone())
                .collect(),
        ]
    }

    /// Picks the item labeled `label` of the drop-down `which` (0: source … 4: format); for
    /// tests.
    pub fn choose(&self, which: usize, label: &str) -> bool {
        let parts = &self.parts;
        let drop_down = match which {
            0 => &parts.source.1,
            1 => &parts.mode.1,
            2 => &parts.resolution.1,
            3 => &parts.paper.1,
            _ => &parts.format,
        };
        let Some(index) = self
            .offered()
            .get(which)
            .and_then(|labels| labels.iter().position(|known| known == label))
        else {
            return false;
        };
        drop_down.set_selected(u32::try_from(index).unwrap_or(u32::MAX));
        true
    }

    /// The buttons: Preview, Scan or Save…, Scan Another Page, Discard and Cancel; for tests.
    pub fn buttons(&self) -> [gtk::Button; 5] {
        let parts = &self.parts;
        [
            parts.preview.clone(),
            parts.primary.clone(),
            parts.another.clone(),
            parts.discard.clone(),
            parts.cancel.clone(),
        ]
    }
}

/// The labels of `choices`.
fn labels<T>(choices: &Choices<T>) -> Vec<String> {
    choices
        .borrow()
        .iter()
        .map(|(label, _)| label.clone())
        .collect()
}

/// Fills the option row `row` with `choices`, `selected` chosen; hides it without choices.
fn fill<T>(
    row: &(gtk::Label, gtk::DropDown),
    store: &Choices<T>,
    choices: Vec<(String, T)>,
    selected: Option<usize>,
) {
    let labels: Vec<&str> = choices.iter().map(|(label, _)| label.as_str()).collect();
    row.1.set_model(Some(&gtk::StringList::new(&labels)));
    row.1.set_selected(
        selected
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(0),
    );
    row.0.set_visible(!choices.is_empty());
    row.1.set_visible(!choices.is_empty());
    store.replace(choices);
}

/// The label of a source: the glass and the feeder in words, others as the scanner names them.
fn source_label(value: &str, kind: SourceKind) -> String {
    match kind {
        SourceKind::Flatbed => tr(msg!("Glass (flatbed)")).to_owned(),
        SourceKind::Feeder => tr(msg!("Document feeder")).to_owned(),
        SourceKind::FeederDuplex => tr(msg!("Document feeder, both sides")).to_owned(),
        SourceKind::Other => value.to_owned(),
    }
}
