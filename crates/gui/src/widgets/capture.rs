// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Screenshots & Clipboard page's own parts, which [`PageView`](super::PageView) adds to the
//! rows built from the registry:
//!
//! - the keys as a button that takes them as they are pressed, like the field of the Keyboard
//!   Shortcuts page's dialog, in place of a text field (the keys of the terminal and the launcher
//!   of Apps & Autostart too); below each, what it takes the place of;
//! - below each switch, the programs it needs that aren't installed and the packages that have
//!   them, or what it works with;
//! - below the file names, the name of the next file;
//! - below the clipboard history: the note that it keeps passwords unless the password manager
//!   marks them, whether this system leaves those out, a line of the user's config that starts a
//!   history already, and _Clear History…_;
//! - below the recording switch, while a recording runs, the sign of it with _Stop_.
//!
//! The page also has the watcher of the history follow its switch and size at once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::apps::launchers;
use sway_things_core::bindings::ours::MODIFIER;
use sway_things_core::bindings::{Keymap, Line, Modifiers, RowKind, keys};
use sway_things_core::capture::shortcuts::{Shortcut, check_keys};
use sway_things_core::capture::tools::{self, Program};
use sway_things_core::capture::{
    AFTERWARDS, FILE_NAME, FOLDER, HISTORY, MAX_ENTRIES, RECORDING, RECORDING_FILE_NAME,
    SCREENSHOTS, clipboard, name, record, shot,
};
use sway_things_core::registry::{ConfigFile, Page, Registry, Value, ValueError};

use super::SettingRow;
use super::shortcuts::{self, Captured, GdkKeymap};
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The button of keys while it listens.
const PRESS: Msg = msg!("Press the keys…");
/// The button of keys without keys.
const NO_KEY: Msg = msg!("No key");
/// The line of the next file name.
const NEXT: Msg = msg!("Next: {name}");
/// The sign of a recording that runs.
const RECORDING_NOW: Msg = msg!("Recording now: {file}");
/// That the history couldn't be cleared.
const NOT_CLEARED: Msg = msg!("The history couldn’t be cleared: {error}");
/// The sign of a recording that runs, without a file.
const RECORDING_NOW_PLAIN: Msg = msg!("Recording now");

/// The generated files the page writes to: the keys go to the bindings, the history's watcher
/// is started from the hooks.
pub fn files(page: Page) -> Vec<ConfigFile> {
    if page == Page::ScreenshotsClipboard {
        vec![ConfigFile::Bindings, ConfigFile::Hooks]
    } else {
        Vec::new()
    }
}

/// Whether `key` is the keys of the terminal or the launcher of Apps & Autostart, whose rows
/// take keys as they are pressed too.
fn is_app_keys(key: &str) -> bool {
    launchers::BINDINGS
        .iter()
        .any(|binding| binding.keys_key == key)
}

/// The binding the keys `text` of the setting `key` make, with `$mod` as `modifier`, to show
/// them; `None` for no keys or keys that can't be bound.
fn keys_line(key: &str, text: &str, modifier: Modifiers) -> Option<Line> {
    if let Some(shortcut) = Shortcut::of(key) {
        return shortcut.line(text, modifier);
    }
    if !is_app_keys(key) || text.is_empty() {
        return None;
    }
    launchers::line(&keys::with_mod(text, modifier), "true").ok()
}

/// The check of new keys for the setting `key`.
fn check_keys_of(key: &str, keys: &str) -> Result<(), ValueError> {
    if is_app_keys(key) {
        launchers::check_keys(keys)
    } else {
        check_keys(keys)
    }
}

/// Puts the button of keys in place of the text field of a row of keys: those of the page, and
/// those of the terminal and the launcher of Apps & Autostart.
pub fn follow(model: &Model, row: &SettingRow) {
    let Some(setting) = row.setting() else {
        return;
    };
    if Shortcut::of(setting.key()).is_none() && !is_app_keys(setting.key()) {
        return;
    }
    let Some(entry) = row
        .controls()
        .into_iter()
        .find_map(|widget| widget.downcast::<gtk::Entry>().ok())
    else {
        return;
    };
    entry.set_visible(false);
    let button = KeyButton::new(model, row);
    // Locked with the row: while its switch is off, without the include line.
    entry
        .bind_property("sensitive", &button.button, "sensitive")
        .sync_create()
        .build();
    row.add_control(&button.button);
}

/// What goes below a row of the page.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let Some(setting) = row.setting() else {
        return Vec::new();
    };
    let key = setting.key();
    if Shortcut::of(key).is_some() {
        return vec![clashes(model, key)];
    }
    match key {
        SCREENSHOTS => vec![programs_note(&tools::SCREENSHOTS, None)],
        HISTORY => {
            follow_history(model);
            vec![
                programs_note(&tools::HISTORY, Some(launcher_line())),
                history_note(model),
            ]
        }
        RECORDING => vec![recording_note(), recording_status()],
        FILE_NAME => vec![next_name(model, FILE_NAME, &shot::EXTENSIONS)],
        RECORDING_FILE_NAME => vec![next_name(model, RECORDING_FILE_NAME, &record::EXTENSIONS)],
        AFTERWARDS => vec![note(&editor_line())],
        _ => Vec::new(),
    }
}

/// A note below a row: dim text that wraps.
fn note_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class(style::DIM);
    label
}

/// A box of notes below a row. The row of the list it goes into shows only while it does, so a
/// hidden note leaves no gap.
fn note_box() -> gtk::Box {
    let note = gtk::Box::new(gtk::Orientation::Vertical, 3);
    note.add_css_class("st-capture-note");
    let sync = |note: &gtk::Box| {
        if let Some(parent) = note.parent() {
            parent.set_visible(note.is_visible());
        }
    };
    note.connect_parent_notify(sync);
    note.connect_visible_notify(sync);
    note
}

/// A note with `text` below a row.
fn note(text: &str) -> gtk::Widget {
    let note = note_box();
    note.append(&note_label(text));
    note.upcast()
}

/// What takes the place of what for the keys of `key`, following the model.
fn clashes(model: &Model, key: &'static str) -> gtk::Widget {
    let note = note_box();
    let follow = glib::clone!(
        #[weak]
        note,
        move |model: &Model| {
            while let Some(child) = note.first_child() {
                note.remove(&child);
            }
            let keymap = GdkKeymap::of_default();
            let keymap = keymap.as_ref().map(|keymap| keymap as &dyn Keymap);
            if let Some(listing) = model.bindings(keymap) {
                for row in listing.rows() {
                    if row.kind == (RowKind::Feature { key }) {
                        for clash in &row.clashes {
                            note.append(&shortcuts::clash_line(clash));
                        }
                    }
                }
            }
            note.set_visible(note.first_child().is_some());
        }
    );
    follow(model);
    model.connect_changed(follow);
    note.upcast()
}

/// The programs of `programs` that are missing and how to get them; else `fine`, if given.
fn programs_note(programs: &[Program], fine: Option<String>) -> gtk::Widget {
    let missing = tools::missing(programs);
    let text = if missing.is_empty() {
        fine.unwrap_or_default()
    } else {
        missing_text(&missing)
    };
    let widget = note(&text);
    widget.set_visible(!text.is_empty());
    widget
}

/// That `missing` aren't installed, and which packages have them.
fn missing_text(missing: &[Program]) -> String {
    let names: Vec<&str> = missing.iter().map(|program| program.name).collect();
    trf(
        msg!(
            "Not installed: {programs}. Install the packages {packages} with your package manager."
        ),
        &[
            ("programs", &names.join(", ")),
            ("packages", &tools::packages(missing).join(", ")),
        ],
    )
}

/// Which launcher shows the history, or that none is installed.
fn launcher_line() -> String {
    match tools::launcher() {
        Some(launcher) => trf(
            msg!("The history opens in {launcher}."),
            &[("launcher", &launcher.program.name)],
        ),
        None => tr(msg!(
            "No launcher to show the history: install fuzzel, wofi, rofi, tofi or bemenu."
        ))
        .to_owned(),
    }
}

/// Which editor _Edit_ opens, or that none is installed.
fn editor_line() -> String {
    match tools::editor() {
        Some(editor) => trf(msg!("Edit opens {editor}."), &[("editor", &editor.name)]),
        None => tr(msg!(
            "To edit screenshots, install swappy or satty; without one there is no Edit."
        ))
        .to_owned(),
    }
}

/// What a recording needs and what it records with.
fn recording_note() -> gtk::Widget {
    let fine = tools::recorder().map(|recorder| {
        trf(
            msg!("Records with {recorder}; a region is selected with slurp."),
            &[("recorder", &recorder.name)],
        )
    });
    match fine {
        Some(fine) => programs_note(&tools::RECORDING, Some(fine)),
        None => note(&missing_text(&[tools::WF_RECORDER])),
    }
}

/// The name of the next file of the pattern `key`, at the folder set, following the model and
/// the clock.
fn next_name(model: &Model, key: &'static str, extensions: &'static [&'static str]) -> gtk::Widget {
    let label = note_label("");
    let note = note_box();
    note.append(&label);
    let text = |model: &Model, key: &str| -> String {
        let value = |key: &str| {
            Registry::builtin()
                .get(key)
                .and_then(|setting| model.view(setting, None))
                .and_then(|view| match view.value {
                    Value::Text(text) => Some(text),
                    _ => None,
                })
                .unwrap_or_default()
        };
        let folder = name::folder(&value(FOLDER), &name::Env::process());
        name::next_name(&value(key), &folder, extensions).unwrap_or_default()
    };
    let refresh = Rc::new(glib::clone!(
        #[weak]
        label,
        #[weak]
        model,
        move || {
            let name = text(&model, key);
            label.set_visible(!name.is_empty());
            label.set_text(&trf(NEXT, &[("name", &name)]));
        }
    ));
    refresh();
    let on_change = Rc::clone(&refresh);
    model.connect_changed(move |_| on_change());
    // The time in the name moves on; only while the line is on screen.
    let tick = Rc::clone(&refresh);
    glib::timeout_add_seconds_local(
        1,
        glib::clone!(
            #[weak]
            label,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                if label.is_mapped() {
                    tick();
                }
                glib::ControlFlow::Continue
            }
        ),
    );
    note.upcast()
}

/// The note about passwords, a line of the user's config that starts a history already, and
/// _Clear History…_.
fn history_note(model: &Model) -> gtk::Widget {
    let note = note_box();
    let label = note_label(tr(privacy_text(None)));
    note.append(&label);
    // Asking the tools for their versions waits for them: off the main context.
    glib::spawn_future_local(glib::clone!(
        #[weak]
        label,
        async move {
            if let Ok(skips) = gtk::gio::spawn_blocking(clipboard::skips_passwords).await {
                label.set_label(tr(privacy_text(skips)));
            }
        }
    ));
    if let Some(place) = own_history(model) {
        note.append(&note_label(&trf(
            msg!(
                "Your config starts a clipboard history already, at {place}; while it runs, Sway \
                 Things starts none."
            ),
            &[("place", &place)],
        )));
    } else if let Some(pid) = clipboard::foreign_watcher() {
        note.append(&note_label(&trf(
            msg!(
                "A clipboard history of your own runs already (wl-paste, process {pid}); while it \
                 runs, Sway Things starts none."
            ),
            &[("pid", &pid)],
        )));
    }
    let clear = gtk::Button::with_label(tr(msg!("Clear History…")));
    clear.set_halign(gtk::Align::Start);
    clear.set_sensitive(tools::CLIPHIST.installed());
    clear.connect_clicked(confirm_clear);
    note.append(&clear);
    note.upcast()
}

/// What the history note says about passwords, from whether the tools leave them out (`None`
/// when that isn't known).
fn privacy_text(skips: Option<bool>) -> Msg {
    match skips {
        Some(true) => msg!(
            "The history keeps everything you copy. Passwords that your password manager marks as \
         such (as KeePassXC does) are left out."
        ),
        Some(false) => msg!(
            "The history keeps everything you copy, passwords too: leaving out what a password \
         manager marks needs wl-clipboard 2.3 and cliphist 0.5 or newer."
        ),
        None => msg!(
            "The history keeps everything you copy, passwords too unless your password manager \
         marks them and wl-clipboard 2.3 and cliphist 0.5 or newer are installed."
        ),
    }
}

/// Where the user's config starts cliphist or clipman itself, as `file:line`.
fn own_history(model: &Model) -> Option<String> {
    let status = model.status()?;
    clipboard::own_history(&status.config().directives).map(|directive| {
        format!(
            "{}:{}",
            super::setting_row::tilde(&directive.file),
            directive.line
        )
    })
}

/// Asks, then removes every entry of the history.
fn confirm_clear(button: &gtk::Button) {
    let dialog = gtk::AlertDialog::builder()
        .message(tr(msg!("Clear the clipboard history?")))
        .detail(tr(msg!(
            "Every entry is removed; what is on the clipboard now stays."
        )))
        .buttons([tr(msg!("Cancel")), tr(msg!("Clear"))])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    let window = button.root().and_downcast::<gtk::Window>();
    dialog.choose(
        window.as_ref(),
        gtk::gio::Cancellable::NONE,
        glib::clone!(
            #[weak]
            button,
            move |answer| {
                if answer != Ok(1) {
                    return;
                }
                glib::spawn_future_local(async move {
                    // `cliphist wipe` is waited for: off the main context.
                    if let Ok(Err(error)) = gtk::gio::spawn_blocking(clipboard::clear).await {
                        button.set_tooltip_text(Some(&trf(
                            NOT_CLEARED,
                            &[("error", &error.text().translated())],
                        )));
                    }
                });
            }
        ),
    );
}

/// The sign of a recording that runs, with _Stop_; looked for every two seconds while its
/// section is on screen.
fn recording_status() -> gtk::Widget {
    let note = note_box();
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let icon = gtk::Image::from_icon_name("media-record-symbolic");
    let label = note_label("");
    label.set_hexpand(true);
    let stop = gtk::Button::with_label(tr(msg!("Stop")));
    line.append(&icon);
    line.append(&label);
    line.append(&stop);
    note.append(&line);
    let refresh = glib::clone!(
        #[weak]
        note,
        #[weak]
        label,
        #[weak(rename_to = stop_button)]
        stop,
        move || {
            let running = record::running();
            note.set_visible(!running.is_empty());
            if running.is_empty() {
                stop_button.set_sensitive(true);
            }
            if let Some(recording) = running.first() {
                let text = recording.file.as_deref().map_or_else(
                    || tr(RECORDING_NOW_PLAIN).to_owned(),
                    |file| trf(RECORDING_NOW, &[("file", &super::setting_row::tilde(file))]),
                );
                label.set_text(&text);
            }
        }
    );
    refresh();
    stop.connect_clicked(move |button| {
        // `sway-things record` stops it as the key does, and says where the file is; waited for
        // on the main loop, since the recorder takes a moment to finish the file.
        let argv = [
            std::ffi::OsStr::new("sway-things"),
            std::ffi::OsStr::new("record"),
        ];
        let Ok(process) = gtk::gio::Subprocess::newv(&argv, gtk::gio::SubprocessFlags::NONE) else {
            return;
        };
        // The command waits for a click on its notification; the page doesn't wait for it, the
        // sign goes once the recorder ended.
        button.set_sensitive(false);
        glib::spawn_future_local(async move {
            let _ = process.wait_future().await;
        });
    });
    glib::timeout_add_seconds_local(
        2,
        glib::clone!(
            #[weak]
            note,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                // The list the sign is in: its own row hides with it.
                let list = note.parent().and_then(|row| row.parent());
                if list.is_some_and(|list| list.is_mapped()) {
                    refresh();
                }
                glib::ControlFlow::Continue
            }
        ),
    );
    note.set_visible(!record::running().is_empty());
    note.upcast()
}

/// Has the watcher of the clipboard history follow its switch and size while the page exists,
/// once they stood still for a moment, so dragging the slider restarts it only once.
fn follow_history(model: &Model) {
    let synced: Rc<Cell<Option<(bool, u32)>>> = Rc::new(Cell::new(None));
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    // What the switch and size ask for by now, read once the sync holds the lock, so of two
    // syncs waiting for it the one that gets it last still acts on the latest.
    let wanted = Arc::new(Mutex::new((false, 750)));
    let follow = move |model: &Model| {
        let view = |key: &str| {
            Registry::builtin()
                .get(key)
                .and_then(|setting| model.view(setting, None))
                .map(|view| view.value)
        };
        let on = view(HISTORY) == Some(Value::Bool(true));
        let max = match view(MAX_ENTRIES) {
            Some(Value::Integer(max)) => u32::try_from(max).unwrap_or(750),
            _ => 750,
        };
        // Only in the session, and only once the stored value is known.
        if model.status().is_none() || synced.get() == Some((on, max)) {
            return;
        }
        synced.set(Some((on, max)));
        if let Some(source) = pending.borrow_mut().take() {
            source.remove();
        }
        *wanted.lock().unwrap_or_else(PoisonError::into_inner) = (on, max);
        let done = Rc::clone(&pending);
        let wanted = Arc::clone(&wanted);
        let source = glib::timeout_add_local_once(SETTLE, move || {
            done.borrow_mut().take();
            // Off the main loop: the lock can be held for seconds, and the process scan takes a
            // moment too.
            glib::spawn_future_local(async move {
                let _ = gtk::gio::spawn_blocking(move || sync_history(&wanted)).await;
            });
        });
        pending.replace(Some(source));
    };
    follow(model);
    model.connect_changed(follow);
}

/// Has the history's watcher follow `wanted`, unless the history is off and none of ours runs.
/// Blocks while the lock is held, so it runs off the main loop.
fn sync_history(wanted: &Mutex<(bool, u32)>) {
    let read = || *wanted.lock().unwrap_or_else(PoisonError::into_inner);
    let (on, _) = read();
    if on || clipboard::watching() {
        // Without a session or while the hook holds the lock for long, the hook at the next
        // start of sway does it.
        let _ = clipboard::sync_with(read);
    }
}

/// How long the history's switch and size stand still before its watcher follows them.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(800);

/// The button of a row of keys: shows the keys as people read them, and takes new ones as they
/// are pressed.
#[derive(Clone)]
struct KeyButton {
    button: gtk::Button,
    listening: Rc<Cell<bool>>,
    row: SettingRow,
    model: Model,
    keymap: Rc<RefCell<Option<GdkKeymap>>>,
}

impl KeyButton {
    fn new(model: &Model, row: &SettingRow) -> Self {
        let button = gtk::Button::new();
        button.add_css_class("st-capture-keys");
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some(tr(msg!(
            "Press the keys; Backspace for no key, Escape to keep them"
        ))));
        let keys = Self {
            button,
            listening: Rc::new(Cell::new(false)),
            row: row.clone(),
            model: model.clone(),
            keymap: Rc::new(RefCell::new(GdkKeymap::of_default())),
        };
        keys.refresh();
        let on_click = keys.clone();
        keys.button
            .connect_clicked(move |_| on_click.listen(!on_click.listening.get()));
        let controller = gtk::EventControllerKey::new();
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let on_key = keys.clone();
        controller.connect_key_pressed(move |controller, keyval, keycode, state| {
            on_key.key_pressed(controller, keyval, keycode, state)
        });
        keys.button.add_controller(controller);
        let focus = gtk::EventControllerFocus::new();
        let on_leave = keys.clone();
        focus.connect_leave(move |_| on_leave.listen(false));
        keys.button.add_controller(focus);
        let on_change = keys.clone();
        model.connect_changed(move |_| on_change.refresh());
        keys
    }

    /// The main modifier of the bindings of Sway Things.
    fn main_modifier(&self) -> Modifiers {
        Registry::builtin()
            .get(MODIFIER)
            .and_then(|setting| self.model.view(setting, None))
            .map_or(Modifiers::SUPER, |view| match view.value {
                Value::Choice(id) => sway_things_core::bindings::ours::modifier_of(&id),
                _ => Modifiers::SUPER,
            })
    }

    /// Shows the keys, or that it listens.
    fn refresh(&self) {
        if self.listening.get() {
            self.button.set_label(tr(PRESS));
            return;
        }
        let text = match self.row.value() {
            Some(Value::Text(text)) => text,
            _ => String::new(),
        };
        let keymap = self.keymap.borrow();
        let labels = self
            .row
            .setting()
            .and_then(|setting| keys_line(setting.key(), &text, self.main_modifier()))
            .map_or_else(
                || tr(NO_KEY).to_owned(),
                |line| {
                    line.labels(Modifiers::NONE, keymap.as_ref().map(|k| k as &dyn Keymap))
                        .iter()
                        .map(sway_things_core::i18n::Text::translated)
                        .collect::<Vec<_>>()
                        .join(" + ")
                },
            );
        self.button.set_label(&labels);
        if let Some(setting) = self.row.setting() {
            self.button
                .update_property(&[gtk::accessible::Property::Label(&format!(
                    "{}: {labels}",
                    tr(setting.title())
                ))]);
        }
    }

    /// Starts or stops listening. While it listens, sway passes every key to the window, so the
    /// keys of a binding (Print) come here instead of running it.
    fn listen(&self, on: bool) {
        let was = self.listening.replace(on);
        let toplevel = self
            .button
            .root()
            .and_then(|root| root.surface())
            .and_downcast::<gdk::Toplevel>();
        if let Some(toplevel) = toplevel {
            if on && !was {
                toplevel.inhibit_system_shortcuts(None::<&gdk::Event>);
            } else if !on && was {
                toplevel.restore_system_shortcuts();
            }
        }
        self.refresh();
    }

    /// A key pressed on the button: taken while it listens.
    fn key_pressed(
        &self,
        controller: &gtk::EventControllerKey,
        keyval: gdk::Key,
        keycode: u32,
        state: gdk::ModifierType,
    ) -> glib::Propagation {
        if !self.listening.get() {
            return glib::Propagation::Proceed;
        }
        let event = controller
            .current_event()
            .and_then(|event| event.downcast::<gdk::KeyEvent>().ok());
        let modifier = event.as_ref().is_some_and(gdk::KeyEvent::is_modifier);
        let layout = event.as_ref().map_or(0, gdk::KeyEvent::layout);
        let level = event.as_ref().map_or(0, gdk::KeyEvent::level);
        let name = shortcuts::first_level(&self.button.display(), keycode, layout)
            .or_else(|| keyval.to_lower().name().map(|name| name.to_string()))
            .unwrap_or_default();
        let keys = match shortcuts::capture(&name, state, level, self.main_modifier(), modifier) {
            Captured::Wait => return glib::Propagation::Stop,
            Captured::Cancel => None,
            Captured::Clear => Some(String::new()),
            Captured::Keys(keys) => Some(keys),
        };
        self.listen(false);
        let key = self.row.setting().map_or("", |setting| setting.key());
        if let Some(keys) = keys {
            match check_keys_of(key, &keys) {
                Ok(()) => {
                    self.row.set_error(None);
                    self.row.change(Value::Text(keys));
                }
                Err(error) => self.row.set_error(Some(&error.text().translated())),
            }
        }
        glib::Propagation::Stop
    }
}
