// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How the bar runs, below the bar program: whether waybar is installed and runs our files,
//! what starts it and the line to change there (with a copy button), a bar of the user's own
//! config next to ours, and the import of the user's waybar config.
//!
//! What runs is read from `/proc` when the model changes and every three seconds while the page
//! is shown, so a waybar that starts or ends shows soon. Lines of the user's config, paths and
//! names are data: they go into labels as plain text.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bar::import::{self, Found};
use sway_things_core::bar::process::{self, Starter};
use sway_things_core::bar::{Backend, WaybarFiles, tilde};
use sway_things_core::registry::{Value, bar as keys};
use sway_things_core::sway_config::Disk;

use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into.
/// The button that copies a line.
const COPY: Msg = msg!("Copy");
/// Its label right after a click.
const COPIED: Msg = msg!("Copied");
/// The title of the import's question.
const IMPORT_QUESTION: Msg = msg!("Use your waybar config as the starting point?");
/// What the import does, in its question.
const IMPORT_DETAIL: Msg = msg!(
    "The bar set up here is replaced by the one in {file}: where it stands, its modules and \
     their options. Your file stays as it is, and your stylesheet is read after the one of Sway \
     Things."
);
/// The buttons of the question.
const CANCEL: Msg = msg!("Cancel");
/// The button that imports.
const IMPORT: Msg = msg!("Import");
/// The heading of what the import couldn't take over.
const IMPORT_NOTES: Msg = msg!("Imported, with notes");

/// How often what runs is read again while the page is shown.
const LOOK_EVERY: Duration = Duration::from_secs(3);

/// The status box of the Bar page.
#[derive(Debug, Clone)]
pub struct Status {
    root: gtk::Box,
    lines: gtk::Box,
}

impl Status {
    /// The box, following `model`.
    pub fn new(model: &Model) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("st-bar-status");
        let lines = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.append(&lines);
        let status = Self { root, lines };
        let shown = status.clone();
        super::follow(
            model,
            &status.root,
            glib::clone!(
                #[weak]
                model,
                move || shown.update(&model)
            ),
        );
        // While shown, look again now and then: waybar may start or end.
        let timer: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
        status.root.connect_map(glib::clone!(
            #[weak]
            model,
            #[strong]
            timer,
            #[strong]
            status,
            move |_| {
                let looked = status.clone();
                let id = glib::timeout_add_local(
                    LOOK_EVERY,
                    glib::clone!(
                        #[weak]
                        model,
                        #[upgrade_or]
                        glib::ControlFlow::Break,
                        move || {
                            looked.update(&model);
                            glib::ControlFlow::Continue
                        }
                    ),
                );
                if let Some(old) = timer.replace(Some(id)) {
                    old.remove();
                }
            }
        ));
        status.root.connect_unmap(move |_| {
            if let Some(id) = timer.take() {
                id.remove();
            }
        });
        status
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// Shows how the bar runs now.
    fn update(&self, model: &Model) {
        while let Some(child) = self.lines.first_child() {
            self.lines.remove(&child);
        }
        let backend = match sway_things_core::registry::Registry::builtin()
            .get(keys::BACKEND)
            .and_then(|setting| model.view(setting, None))
            .map(|view| view.value)
        {
            Some(Value::Choice(id)) if id == "waybar" => Backend::Waybar,
            Some(Value::Choice(id)) if id == "swaybar" => Backend::Swaybar,
            _ => Backend::None,
        };
        let (Some(status), Some(files), Some(locations)) =
            (model.status(), model.waybar_files(), model.locations())
        else {
            return;
        };
        let generated = locations.directory().to_path_buf();
        for (file, line) in process::swaybars(status.config(), &generated) {
            if backend != Backend::None {
                self.note(
                    &trf(
                        msg!(
                            "Your sway config starts swaybar too ({place}). Remove that bar \
                             block, or both bars show."
                        ),
                        &[("place", &format!("{}:{line}", tilde(&file)))],
                    ),
                    true,
                );
            }
        }
        match backend {
            Backend::None => self.none(model, &files),
            Backend::Swaybar => self.note(
                tr(msg!(
                    "swaybar is set up in Sway Things' sway include (75-bar.conf); a change \
                     shows at once."
                )),
                false,
            ),
            Backend::Waybar => self.waybar(model, &status, &files, &generated),
        }
    }

    /// While no bar is set up here: what choosing one does, and the import.
    fn none(&self, model: &Model, files: &WaybarFiles) {
        self.note(
            tr(msg!(
                "Choose waybar or swaybar to set up the bar here. Until then, your own bar stays \
                 as it is."
            )),
            false,
        );
        self.import_line(model, files);
    }

    /// With waybar: installed, what starts it, whether ours runs, the import and our files.
    fn waybar(
        &self,
        model: &Model,
        status: &sway_things_core::include::IncludeStatus,
        files: &WaybarFiles,
        generated: &std::path::Path,
    ) {
        if !process::installed() {
            self.note(
                tr(msg!(
                    "waybar isn't installed. Install the waybar package, or choose swaybar, \
                     which comes with sway."
                )),
                true,
            );
        }
        let running = process::running();
        let ours = running.iter().any(|waybar| waybar.runs(files));
        let starter = Starter::detect(status.config(), generated, &running, files);
        self.starter_notes(&starter, files);
        if let Some(problem) = process::last_problem() {
            self.note(&problem.text().translated(), true);
        }
        let state = if ours {
            tr(msg!("waybar runs with the config of Sway Things."))
        } else if running.is_empty() {
            tr(msg!("No waybar runs now."))
        } else {
            tr(msg!("waybar runs, but with another config."))
        };
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let label = gtk::Label::new(Some(state));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        line.append(&label);
        let reload = gtk::Button::with_label(tr(msg!("Reload waybar")));
        reload.set_tooltip_text(Some(tr(msg!(
            "Has waybar read the config of Sway Things again"
        ))));
        reload.set_sensitive(ours);
        reload.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |_| model.reload_bar()
        ));
        line.append(&reload);
        self.lines.append(&line);
        self.import_line(model, files);
        let written = gtk::Label::new(Some(&trf(
            msg!("Written to {config} and {style}"),
            &[
                ("config", &tilde(&files.config())),
                ("style", &tilde(&files.style())),
            ],
        )));
        written.set_xalign(0.0);
        written.set_wrap(true);
        written.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        written.add_css_class(style::DIM);
        self.lines.append(&written);
    }

    /// What starts waybar, with the line to change there.
    fn starter_notes(&self, starter: &Starter, files: &WaybarFiles) {
        match starter {
            Starter::Nothing => self.note(
                tr(msg!(
                    "Sway Things starts waybar with its own config when sway starts \
                     (75-bar.conf)."
                )),
                false,
            ),
            Starter::Exec(line) | Starter::SwaybarCommand(line) if line.ours => self.note(
                &trf(
                    msg!("Your sway config starts waybar with the files of Sway Things ({place})."),
                    &[("place", &format!("{}:{}", tilde(&line.file), line.line))],
                ),
                false,
            ),
            Starter::Exec(line) | Starter::SwaybarCommand(line) => {
                self.note(
                    &trf(
                        msg!(
                            "Your sway config starts waybar with its own config ({place}). To \
                             use the bar set up here, change that line to:"
                        ),
                        &[("place", &format!("{}:{}", tilde(&line.file), line.line))],
                    ),
                    true,
                );
                if let Some(text) = process::handover(starter, files) {
                    self.copyable(&text);
                }
            }
            Starter::Maybe(line) => {
                self.note(
                    &trf(
                        msg!(
                            "Your sway config runs something that may start waybar ({place}); \
                             Sway Things can't tell, so it doesn't start a second one. To use \
                             the bar set up here, start waybar there with:"
                        ),
                        &[("place", &format!("{}:{}", tilde(&line.file), line.line))],
                    ),
                    true,
                );
                if let Some(text) = process::handover(starter, files) {
                    self.copyable(&text);
                }
            }
            Starter::Unit | Starter::UnitStart(_) => {
                self.note(
                    &trf(
                        msg!(
                            "waybar runs as a systemd service. Point it at the files of Sway \
                             Things with a drop-in: run {command} and add:"
                        ),
                        &[("command", &process::UNIT_EDIT)],
                    ),
                    true,
                );
                if let Some(text) = process::handover(starter, files) {
                    self.copyable(&text);
                }
            }
        }
    }

    /// The user's own waybar config, with _Import as starting point…_, if there is one.
    fn import_line(&self, model: &Model, files: &WaybarFiles) {
        let Some(found) = import::find(&Disk, &files.dir) else {
            return;
        };
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let label = gtk::Label::new(Some(&trf(
            msg!("Your waybar config: {file}"),
            &[("file", &tilde(&found.config))],
        )));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        line.append(&label);
        let button = gtk::Button::with_label(tr(msg!("Import as Starting Point…")));
        button.set_tooltip_text(Some(tr(msg!("Reads your config; changes nothing in it"))));
        button.set_sensitive(model.status().is_some_and(|status| status.is_active()));
        button.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |button| ask_import(button, &model, found.clone())
        ));
        line.append(&button);
        self.lines.append(&line);
    }

    /// A line of text; a warning with an icon.
    fn note(&self, text: &str, warning: bool) {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        if warning {
            let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
            icon.set_valign(gtk::Align::Start);
            icon.set_accessible_role(gtk::AccessibleRole::Presentation);
            line.append(&icon);
        }
        let label = gtk::Label::new(Some(text));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        line.append(&label);
        self.lines.append(&line);
    }

    /// `text` in monospace, selectable, with a copy button.
    fn copyable(&self, text: &str) {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let label = gtk::Label::new(Some(text));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_selectable(true);
        label.add_css_class("monospace");
        line.append(&label);
        let copy = gtk::Button::with_label(tr(COPY));
        copy.set_valign(gtk::Align::Center);
        let text = text.to_owned();
        copy.connect_clicked(move |button| {
            button.clipboard().set_text(&text);
            button.set_label(tr(COPIED));
            glib::timeout_add_local_once(
                Duration::from_secs(2),
                glib::clone!(
                    #[weak]
                    button,
                    move || button.set_label(tr(COPY))
                ),
            );
        });
        line.append(&copy);
        self.lines.append(&line);
    }
}

/// Asks before the import, then imports.
fn ask_import(button: &gtk::Button, model: &Model, found: Found) {
    let dialog = gtk::AlertDialog::builder()
        .message(tr(IMPORT_QUESTION))
        .detail(trf(IMPORT_DETAIL, &[("file", &tilde(&found.config))]))
        .buttons([tr(CANCEL), tr(IMPORT)])
        .cancel_button(0)
        .default_button(1)
        .modal(true)
        .build();
    let window = button.root().and_downcast::<gtk::Window>();
    dialog.choose(
        window.as_ref(),
        gtk::gio::Cancellable::NONE,
        glib::clone!(
            #[weak]
            model,
            #[weak]
            button,
            move |answer| {
                if answer == Ok(1) {
                    run_import(&button, &model, &found);
                }
            }
        ),
    );
}

/// Reads the user's config into the settings and the module definitions, off the main loop
/// (includes may name many files); says what couldn't be taken over, or why nothing could.
fn run_import(button: &gtk::Button, model: &Model, found: &Found) {
    let found = found.clone();
    button.set_sensitive(false);
    glib::spawn_future_local(glib::clone!(
        #[weak]
        model,
        #[weak]
        button,
        async move {
            let read = gtk::gio::spawn_blocking(move || import::read(&found, &Disk)).await;
            button.set_sensitive(true);
            let window = button.root().and_downcast::<gtk::Window>();
            let imported = match read {
                Ok(Ok(imported)) => imported,
                Ok(Err(error)) => {
                    gtk::AlertDialog::builder()
                        .message(error.text().translated())
                        .modal(true)
                        .build()
                        .show(window.as_ref());
                    return;
                }
                Err(_) => return,
            };
            model.set(keys::BACKEND, None, Value::Choice("waybar".to_owned()));
            for (key, value) in imported.values {
                model.set(key, None, value);
            }
            model.set_bar_modules(imported.modules);
            if !imported.notes.is_empty() {
                let notes: Vec<String> = imported
                    .notes
                    .iter()
                    .map(sway_things_core::i18n::Text::translated)
                    .collect();
                gtk::AlertDialog::builder()
                    .message(tr(IMPORT_NOTES))
                    .detail(notes.join("\n"))
                    .modal(true)
                    .build()
                    .show(window.as_ref());
            }
        }
    ));
}
