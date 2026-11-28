// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How the bar runs, below the bar program: whether waybar is installed and runs our files,
//! what starts it and the line to change there (with a copy button), a bar of the user's own
//! config next to ours, and the import of the user's waybar config.
//!
//! What runs is read from `/proc` when the model changes and every three seconds while the page
//! is shown, so a waybar that starts or ends shows soon. Lines of the user's config, paths and
//! names are data: they go into labels as plain text.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bar::import::{self, Found};
use sway_things_core::bar::process::{self, Edit, Handover, Starter, TakesEffect};
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

/// _Reload waybar_ and _Reload Sway_, and what is said while and after they run.
const RELOAD_WAYBAR: Msg = msg!("Reload waybar");
/// While waybar reads its config again.
const RELOADING: Msg = msg!("Reloading…");
/// The button that reloads sway.
const RELOAD_SWAY: Msg = msg!("Reload Sway");
/// The button that opens the user's sway config at the line.
const OPEN_IN_EDITOR: Msg = msg!("Open in Editor");

/// How often what runs is read again while the page is shown.
const LOOK_EVERY: Duration = Duration::from_secs(3);
/// How long a reload is reported after it ran.
const REPORTED: Duration = Duration::from_secs(8);

/// Where a reload that the page started stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reload {
    /// None was started, or its report is over.
    Idle,
    /// Asked for; the model hasn't finished it yet.
    Running,
    /// Finished at this moment.
    Done(Instant),
}

impl Reload {
    /// Whether it finished within [`REPORTED`].
    fn just_done(self) -> bool {
        matches!(self, Self::Done(at) if at.elapsed() < REPORTED)
    }
}

/// What is told the state: the line, whether it needs attention, whether the details say more.
type Tell = Box<dyn Fn(&str, bool, bool)>;

/// The status box of the Bar page.
#[derive(Clone)]
pub struct Status {
    root: gtk::Box,
    lines: gtk::Box,
    /// Whether a line needs the user's attention, while the box is being filled.
    attention: Rc<Cell<bool>>,
    /// Told the state in one line, whether it needs attention, and whether the details say
    /// more about it, after each look.
    told: Rc<RefCell<Option<Tell>>>,
    /// The reload of waybar the page started.
    waybar_reload: Rc<Cell<Reload>>,
    /// The reload of sway the page started, for the handover.
    sway_reload: Rc<Cell<Reload>>,
    /// Counts the reads of the running waybars, so only the newest is shown.
    turn: Rc<Cell<u64>>,
}

impl std::fmt::Debug for Status {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Status").finish_non_exhaustive()
    }
}

impl Status {
    /// The box, following `model`.
    pub fn new(model: &Model) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("st-bar-status");
        let lines = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.append(&lines);
        let status = Self {
            root,
            lines,
            attention: Rc::new(Cell::new(false)),
            told: Rc::new(RefCell::new(None)),
            waybar_reload: Rc::new(Cell::new(Reload::Idle)),
            sway_reload: Rc::new(Cell::new(Reload::Idle)),
            turn: Rc::new(Cell::new(0)),
        };
        let shown = status.clone();
        super::follow(
            model,
            &status.root,
            glib::clone!(
                #[weak]
                model,
                move || {
                    // The model tells its rows once a queued reload has run.
                    for reload in [&shown.waybar_reload, &shown.sway_reload] {
                        if reload.get() == Reload::Running && !model.is_busy() {
                            reload.set(Reload::Done(Instant::now()));
                        }
                    }
                    shown.update(&model);
                }
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

    /// Calls `tell` with the state in one line and whether something needs attention, now and
    /// after each look.
    pub fn connect_state(&self, tell: impl Fn(&str, bool, bool) + 'static) {
        self.told.replace(Some(Box::new(tell)));
    }

    /// Whether the box shows anything.
    pub fn is_empty(&self) -> bool {
        self.lines.first_child().is_none()
    }

    /// Shows how the bar runs now, once the running waybars are read off the main context, and
    /// tells the state in one line; only the newest read is shown.
    pub fn update(&self, model: &Model) {
        let started = self.turn.get().wrapping_add(1);
        self.turn.set(started);
        let shown = self.clone();
        let model = model.downgrade();
        glib::spawn_future_local(async move {
            let running = gtk::gio::spawn_blocking(process::running)
                .await
                .unwrap_or_default();
            if let Some(model) = model.upgrade() {
                if shown.turn.get() == started {
                    shown.show(&model, &running);
                }
            }
        });
    }

    /// Shows how the bar runs with the waybars `running`, and tells the state in one line.
    fn show(&self, model: &Model, running: &[process::Waybar]) {
        let (state, missing) = self.fill(model, running);
        let warned = self.attention.get();
        if let Some(tell) = self.told.borrow().as_ref() {
            tell(&state, warned || missing, warned);
        }
    }

    /// Fills the box; the state in one line, and whether the bar program isn't installed.
    fn fill(&self, model: &Model, running: &[process::Waybar]) -> (String, bool) {
        self.attention.set(false);
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
            return (String::new(), false);
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
            Backend::None => {
                self.none(model, &files);
                (tr(msg!("Your own bar stays as it is.")).to_owned(), false)
            }
            Backend::Swaybar if self.attention.get() => (
                tr(msg!(
                    "Another swaybar of your config shows next to it: see Details."
                ))
                .to_owned(),
                false,
            ),
            Backend::Swaybar => {
                self.note(
                    tr(msg!(
                        "swaybar is set up in Sway Things' sway include (75-bar.conf); a change \
                         shows at once."
                    )),
                    false,
                );
                (
                    tr(msg!("Built into sway; a change shows at once.")).to_owned(),
                    false,
                )
            }
            Backend::Waybar => self.waybar(model, &status, &files, &generated, running),
        }
    }

    /// While no bar is set up here: what choosing one does, and the import.
    fn none(&self, model: &Model, files: &WaybarFiles) {
        self.import_line(model, files);
    }

    /// With waybar: installed, what starts it, whether ours runs, the import and our files.
    fn waybar(
        &self,
        model: &Model,
        status: &sway_things_core::include::IncludeStatus,
        files: &WaybarFiles,
        generated: &std::path::Path,
        running: &[process::Waybar],
    ) -> (String, bool) {
        let installed = process::installed();
        // The state line says it; the details have nothing to add.
        let missing = !installed;
        let ours = running.iter().any(|waybar| waybar.runs(files));
        let starter = Starter::detect(status.config(), generated, running, files);
        let takeable = sway_things_core::bar::takeover::bars(status.config(), generated)
            .iter()
            .any(sway_things_core::bar::takeover::Bar::can_take);
        self.starter_notes(model, &starter, files, ours, takeable);
        // The user's config or a unit starts Waybar, with a line to change there.
        let elsewhere = match &starter {
            Starter::Exec(line) | Starter::SwaybarCommand(line) => !line.ours,
            Starter::Maybe(_) | Starter::Unit | Starter::UnitStart(_) => true,
            Starter::Nothing | Starter::TakenOver(_) => false,
        };
        let problem = process::last_problem();
        if let Some(problem) = problem {
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
        if ours {
            let icon = gtk::Image::from_icon_name("object-select-symbolic");
            icon.add_css_class("success");
            icon.set_accessible_role(gtk::AccessibleRole::Presentation);
            line.append(&icon);
        }
        let label = gtk::Label::new(Some(state));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        line.append(&label);
        line.append(&self.reload_button(model, ours));
        self.lines.append(&line);
        // Why the button can't be used, in words too: a tooltip alone isn't seen.
        if !ours {
            let why = if running.is_empty() {
                tr(msg!(
                    "Nothing to reload: no waybar runs with the config of Sway Things."
                ))
            } else {
                tr(msg!(
                    "Nothing to reload yet: the waybar that runs has its own config, and Sway \
                     Things leaves it alone. Once waybar runs with the config of Sway Things, it \
                     can be reloaded here."
                ))
            };
            self.dim(why);
        } else if self.waybar_reload.get().just_done() && problem.is_none() {
            self.note_with_icon(
                tr(msg!("waybar read the config of Sway Things again.")),
                "object-select-symbolic",
            );
        }
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
        let state = if !installed {
            tr(msg!("Not installed: install the waybar package.")).to_owned()
        } else if elsewhere {
            tr(msg!("Your sway config starts Waybar itself: see Details.")).to_owned()
        } else if ours {
            tr(msg!("Runs with these settings.")).to_owned()
        } else if let Some(problem) = problem {
            problem.text().translated()
        } else if running.is_empty() {
            tr(msg!("Not running; it starts with sway.")).to_owned()
        } else {
            tr(msg!("Another Waybar runs: see Details.")).to_owned()
        };
        (state, missing)
    }

    /// _Reload waybar_: only for a waybar that runs our config (the user's own is never
    /// signaled), insensitive while one runs, and reported once it ran.
    fn reload_button(&self, model: &Model, ours: bool) -> gtk::Button {
        let running = self.waybar_reload.get() == Reload::Running;
        let reload = gtk::Button::with_label(tr(if running { RELOADING } else { RELOAD_WAYBAR }));
        reload.set_valign(gtk::Align::Center);
        reload.set_tooltip_text(Some(if ours {
            tr(msg!("Has waybar read the config of Sway Things again"))
        } else {
            tr(msg!("No waybar runs with the config of Sway Things"))
        }));
        reload.set_sensitive(ours && !running);
        let state = self.waybar_reload.clone();
        reload.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |button| {
                state.set(Reload::Running);
                button.set_label(tr(RELOADING));
                button.set_sensitive(false);
                model.reload_bar();
            }
        ));
        reload
    }

    /// _Reload Sway_, reported while it runs and once it ran.
    fn reload_sway_button(&self, model: &Model) -> gtk::Button {
        let running = self.sway_reload.get() == Reload::Running;
        let button = gtk::Button::with_label(tr(if running { RELOADING } else { RELOAD_SWAY }));
        button.set_halign(gtk::Align::Start);
        button.set_tooltip_text(Some(tr(msg!(
            "Saves the changes and has sway read its config again"
        ))));
        button.set_sensitive(!running && model.status().is_some_and(|status| status.is_active()));
        let state = self.sway_reload.clone();
        button.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |button| {
                state.set(Reload::Running);
                button.set_label(tr(RELOADING));
                button.set_sensitive(false);
                model.reload_sway();
            }
        ));
        button
    }

    /// The steps of the handover. Sway Things never writes the user's config: the user runs a
    /// command for a terminal that keeps a copy of the file and puts `#` in front of the line
    /// (or of the `bar { … }` block), with the command that takes it back, or opens the file at
    /// the line and changes it there; then sway is reloaded (or they log in again).
    fn handover_steps(&self, model: &Model, edit: &Handover) {
        let place = format!("{}:{}", tilde(&edit.line.file), edit.line.line);
        self.note(
            &trf(
                msg!(
                    "Your sway config starts its own waybar ({place}), so the bar set up here \
                     doesn't show. Sway Things never changes your files: you make the change \
                     yourself."
                ),
                &[("place", &place)],
            ),
            true,
        );
        let steps = gtk::Box::new(gtk::Orientation::Vertical, 8);
        steps.add_css_class("st-bar-handover");
        steps.set_margin_start(24);
        let what = match edit.edit {
            Edit::RemoveBar => tr(msg!(
                "the whole bar { … } block with swaybar_command waybar (sway runs that command \
                 without options, so it can't use the files of Sway Things)"
            ))
            .to_owned(),
            Edit::Replace(_) => trf(msg!("line {line}"), &[("line", &edit.line.line)]),
        };
        if let Some(commands) = process::handover_commands(edit) {
            steps.append(&step_line(
                &trf(
                    msg!(
                        "1. Run this in a terminal. It keeps a copy of your config next to it \
                         (.bak-sway-things) and puts # in front of {what}:"
                    ),
                    &[("what", &what)],
                ),
                None,
            ));
            steps.append(&copyable_line(&commands.change));
            steps.append(&step_line(tr(msg!("To take it back later, run:")), None));
            steps.append(&copyable_line(&commands.undo));
        }
        let open = gtk::Button::with_label(tr(OPEN_IN_EDITOR));
        let path = edit.line.file.clone();
        let line = edit.line.line;
        open.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |button| {
                let reporter = model.clone();
                crate::editor::open(button, Some(&model), &path, Some(line), move |error| {
                    if let Some(error) = error {
                        reporter.emit_problem(error);
                    }
                });
            }
        ));
        steps.append(&step_line(
            &trf(
                msg!("2. Or open the file and put # in front of {what} yourself."),
                &[("what", &what)],
            ),
            Some(open.upcast_ref()),
        ));
        match edit.takes_effect {
            TakesEffect::Reload => {
                steps.append(&step_line(
                    tr(msg!(
                        "3. Reload sway: with the button, or with your reload shortcut \
                         (Super+Shift+C by default)."
                    )),
                    Some(self.reload_sway_button(model).upcast_ref()),
                ));
            }
            TakesEffect::Login => steps.append(&step_line(
                tr(msg!(
                    "3. Log out and back in: sway runs exec lines only when it starts."
                )),
                None,
            )),
        }
        if self.sway_reload.get().just_done() {
            self.sway_reload.set(Reload::Idle);
        }
        self.lines.append(&steps);
    }

    /// `text` with _Reload Sway_ after it; while and right after a reload the page started,
    /// that it is starting instead.
    fn reload_prompt(&self, model: &Model, text: &str) {
        if self.sway_reload.get() != Reload::Idle {
            if self.sway_reload.get() == Reload::Running || self.sway_reload.get().just_done() {
                self.dim(tr(msg!("Sway was reloaded; waybar is starting…")));
                return;
            }
            self.sway_reload.set(Reload::Idle);
        }
        self.lines.append(&step_line(
            text,
            Some(self.reload_sway_button(model).upcast_ref()),
        ));
    }

    /// What starts waybar, with the line to change there; `ours` whether a waybar runs our
    /// config, `takeable` whether a bar of the user's can be taken over instead.
    fn starter_notes(
        &self,
        model: &Model,
        starter: &Starter,
        files: &WaybarFiles,
        ours: bool,
        takeable: bool,
    ) {
        match starter {
            Starter::Nothing => {
                self.note(
                    tr(msg!(
                        "Sway Things starts waybar with its own config when sway starts \
                         (75-bar.conf)."
                    )),
                    false,
                );
                if ours {
                    self.sway_reload.set(Reload::Idle);
                } else if process::installed() {
                    // Right after the user's own start of waybar went from their config.
                    self.reload_prompt(model, tr(msg!("Reload sway to start it now.")));
                }
            }
            Starter::Exec(line) | Starter::SwaybarCommand(line) if line.ours => {
                self.note(
                    &trf(
                        msg!(
                            "Your sway config starts waybar with the files of Sway Things \
                             ({place})."
                        ),
                        &[("place", &format!("{}:{}", tilde(&line.file), line.line))],
                    ),
                    false,
                );
                if ours {
                    self.sway_reload.set(Reload::Idle);
                    return;
                }
                // Changed, but the waybar that runs is still the old one.
                let at_reload = matches!(starter, Starter::SwaybarCommand(_))
                    || line.text.trim_start().starts_with("exec_always");
                if at_reload {
                    self.reload_prompt(
                        model,
                        tr(msg!("Reload sway so that it starts waybar with them.")),
                    );
                } else {
                    self.note(
                        tr(msg!(
                            "It takes effect when you log in again: sway runs exec lines only \
                             when it starts."
                        )),
                        false,
                    );
                }
            }
            Starter::TakenOver(line) => self.taken_over(model, line, ours),
            Starter::SwaybarCommand(_) if takeable => {
                self.note(
                    tr(msg!(
                        "Your sway config starts its own Waybar in a bar block. Use Sway \
                         Things' Bar, above, to show this bar in its place without changing \
                         your config."
                    )),
                    true,
                );
            }
            Starter::Exec(_) | Starter::SwaybarCommand(_) => {
                if let Some(edit) = process::handover_edit(starter, files) {
                    self.handover_steps(model, &edit);
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
            Starter::Unit | Starter::UnitStart(_) => self.unit_notes(starter, files),
        }
    }

    /// While waybar runs as a systemd service: the drop-in that points it at our files, or the
    /// commands that disable the service and enable it again.
    fn unit_notes(&self, starter: &Starter, files: &WaybarFiles) {
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
        let commands = process::unit_commands();
        self.dim(tr(msg!(
            "Or stop waybar's service, so Sway Things starts its own: run the first command in a \
             terminal yourself; the second one takes it back."
        )));
        self.copyable(&commands.change);
        self.copyable(&commands.undo);
    }

    /// While a bar of the user's shows Sway Things' waybar (`line` starts theirs), and whether it
    /// runs (`ours`).
    fn taken_over(&self, model: &Model, line: &sway_things_core::bar::process::Line, ours: bool) {
        self.note(
            &trf(
                msg!(
                    "Your bar ({place}) shows Sway Things' Waybar: 75-bar.conf gives it another \
                     program, and your sway config stays as it is."
                ),
                &[("place", &format!("{}:{}", tilde(&line.file), line.line))],
            ),
            false,
        );
        if ours {
            self.sway_reload.set(Reload::Idle);
        } else if process::installed() {
            self.reload_prompt(model, tr(msg!("Reload sway to start it now.")));
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
        if warning {
            self.attention.set(true);
        }
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

    /// A line of text with an icon that isn't a warning, such as a check mark.
    fn note_with_icon(&self, text: &str, icon_name: &str) {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let icon = gtk::Image::from_icon_name(icon_name);
        icon.set_valign(gtk::Align::Start);
        icon.add_css_class("success");
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        line.append(&icon);
        let label = gtk::Label::new(Some(text));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        line.append(&label);
        self.lines.append(&line);
    }

    /// A dimmed line of text.
    fn dim(&self, text: &str) {
        let label = gtk::Label::new(Some(text));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.add_css_class(style::DIM);
        self.lines.append(&label);
    }

    /// `text` in monospace, selectable, with a copy button.
    fn copyable(&self, text: &str) {
        self.lines.append(&copyable_line(text));
    }
}

/// A step of the handover: its text and, if given, a button after it.
fn step_line(text: &str, button: Option<&gtk::Widget>) -> gtk::Box {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    line.append(&label);
    if let Some(button) = button {
        button.set_valign(gtk::Align::Center);
        line.append(button);
    }
    line
}

/// `text` in monospace, selectable, with a copy button.
fn copyable_line(text: &str) -> gtk::Box {
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
    line
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
