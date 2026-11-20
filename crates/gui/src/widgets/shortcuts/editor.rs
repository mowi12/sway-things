// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialog that adds a binding or changes one: a field that takes the keys as they are
//! pressed, what the binding does (an app, a command, focus or move a window, a workspace, or a
//! sway command) and its options. It warns before the keys take the place of another binding,
//! and refuses keys it can't take (a media key group's, a line of the user's config read after
//! Sway Things' file).
//!
//! While the field listens, the dialog asks sway to pass every key to it
//! (`keyboard-shortcuts-inhibit`), so a binding that exists already doesn't run instead.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bindings::ours::{self, BINDINGS, MODIFIER};
use sway_things_core::bindings::{
    Action, BindCommand, DEFAULT_MODE, Direction, Keymap, Line, MAX_WORKSPACE, Modifiers, Row,
    RowKind, edit,
};
use sway_things_core::registry::{Registry, Value};

use super::GdkKeymap;
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;

/// The kinds of action the dialog offers, in the order of its drop-down.
const ACTIONS: [Msg; 10] = [
    msg!("Open an app"),
    msg!("Run a command"),
    msg!("Focus a window"),
    msg!("Move the window"),
    msg!("Go to a workspace"),
    msg!("Move the window to a workspace"),
    msg!("Close the window"),
    msg!("Toggle fullscreen"),
    msg!("Toggle floating"),
    msg!("Sway command"),
];

/// The directions, in the order of their drop-down.
const DIRECTIONS: [Msg; 4] = [msg!("Left"), msg!("Right"), msg!("Up"), msg!("Down")];

/// What the key field made of a key press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Captured {
    /// Only a modifier: the field waits for the key.
    Wait,
    /// Escape alone: the field stops listening and keeps the keys it had.
    Cancel,
    /// Backspace alone: the keys are cleared.
    Clear,
    /// The keys as an item writes them (`$mod+Shift+e`).
    Keys(String),
}

/// What the key press of `name` (the keysym of the key's first level) with the modifiers
/// `state` makes. `main` is the main modifier, which becomes `$mod`; `modifier` says whether the
/// key itself is a modifier. `level` is the shift level the key gave: GTK 4 has no mask for
/// `Mod5`, but a third level or above (`AltGr`, which XKB maps to `Mod5`) shows in it. GTK 4 has
/// none for `Mod3` either, which no common layout uses.
pub fn capture(
    name: &str,
    state: gdk::ModifierType,
    level: u32,
    main: Modifiers,
    modifier: bool,
) -> Captured {
    if modifier {
        return Captured::Wait;
    }
    let held = [
        (gdk::ModifierType::SUPER_MASK, Modifiers::SUPER, "Mod4"),
        (gdk::ModifierType::ALT_MASK, Modifiers::ALT, "Mod1"),
        (gdk::ModifierType::CONTROL_MASK, Modifiers::CTRL, "Ctrl"),
        (gdk::ModifierType::SHIFT_MASK, Modifiers::SHIFT, "Shift"),
    ];
    let mut parts: Vec<&str> = held
        .iter()
        .filter(|(mask, _, _)| state.contains(*mask))
        .map(|(_, modifier, word)| if *modifier == main { "$mod" } else { word })
        .collect();
    if level >= 2 {
        parts.push("Mod5");
    }
    if parts.is_empty() {
        match name {
            "Escape" => return Captured::Cancel,
            "BackSpace" => return Captured::Clear,
            _ => {}
        }
    }
    parts.push(name);
    Captured::Keys(parts.join("+"))
}

/// The dialog's widgets and what it edits.
#[derive(Debug, Clone)]
pub struct Editor {
    window: gtk::Window,
    capture: gtk::Button,
    conflict: gtk::Label,
    action: gtk::DropDown,
    stack: gtk::Stack,
    command: gtk::Entry,
    sway: gtk::Entry,
    direction: gtk::DropDown,
    workspace: gtk::SpinButton,
    app: gtk::DropDown,
    locked: gtk::CheckButton,
    to_code: gtk::CheckButton,
    error: gtk::Label,
    save: gtk::Button,
    state: Rc<RefCell<Editing>>,
}

/// What the dialog edits.
#[derive(Debug, Default)]
struct Editing {
    /// The keys as an item writes them; `None` until pressed.
    keys: Option<String>,
    /// The binding being changed, as the list showed it.
    row: Option<Row>,
    /// Its line as stored: with `$mod` for one of Sway Things.
    base: Option<Line>,
    /// Whether the field listens for keys.
    capturing: bool,
    /// The command lines of the apps of the drop-down.
    apps: Vec<String>,
    model: Option<Model>,
    keymap: Rc<Option<GdkKeymap>>,
}

impl Editor {
    /// Opens the dialog for a new binding (`row` is `None`) or for the binding of the row at
    /// `row` of the model's listing, over the window of `from`.
    pub fn open(
        from: &impl IsA<gtk::Widget>,
        model: &Model,
        row: Option<usize>,
        keymap: Rc<Option<GdkKeymap>>,
    ) -> Self {
        let editor = Self::build(model, keymap);
        if let Some(parent) = from.as_ref().root().and_downcast::<gtk::Window>() {
            editor.window.set_transient_for(Some(&parent));
        }
        editor.load(model, row);
        editor.refresh();
        editor.window.present();
        editor
    }

    /// The dialog's window.
    pub fn window(&self) -> &gtk::Window {
        &self.window
    }

    /// The field that takes the keys.
    pub fn capture_button(&self) -> &gtk::Button {
        &self.capture
    }

    /// The warning about keys in use, if shown.
    pub fn conflict(&self) -> Option<String> {
        self.conflict
            .is_visible()
            .then(|| self.conflict.text().to_string())
    }

    /// Why the binding can't be saved, if it can't.
    pub fn error(&self) -> Option<String> {
        self.error
            .is_visible()
            .then(|| self.error.text().to_string())
    }

    /// The button that saves.
    pub fn save_button(&self) -> &gtk::Button {
        &self.save
    }

    /// Sets the keys as an item writes them, as if they were pressed.
    pub fn set_keys(&self, keys: &str) {
        self.state.borrow_mut().keys = Some(keys.to_owned());
        self.refresh();
    }

    /// Sets the action to run `command` (`Run a command`).
    pub fn set_run(&self, command: &str) {
        self.action.set_selected(1);
        self.command.set_text(command);
        self.refresh();
    }

    /// Sets whether the binding also works on the lock screen.
    pub fn set_locked(&self, locked: bool) {
        self.locked.set_active(locked);
        self.refresh();
    }

    /// Sets the action to the sway command `command`.
    pub fn set_sway(&self, command: &str) {
        self.action.set_selected(9);
        self.sway.set_text(command);
        self.refresh();
    }

    fn build(model: &Model, keymap: Rc<Option<GdkKeymap>>) -> Self {
        let window = gtk::Window::builder()
            .modal(true)
            .default_width(480)
            .resizable(true)
            .title(tr(msg!("Shortcut")))
            .build();
        window.add_css_class("st-dialog");
        let capture = gtk::Button::new();
        capture.add_css_class("st-shortcut-capture");
        let action = gtk::DropDown::from_strings(&ACTIONS.map(tr));
        let direction = gtk::DropDown::from_strings(&DIRECTIONS.map(tr));
        let workspace = gtk::SpinButton::with_range(1.0, f64::from(MAX_WORKSPACE), 1.0);
        let (app, apps) = apps();
        let editor = Self {
            window,
            capture,
            conflict: hint(),
            action,
            stack: gtk::Stack::new(),
            command: gtk::Entry::new(),
            sway: gtk::Entry::new(),
            direction,
            workspace,
            app,
            locked: gtk::CheckButton::with_label(tr(msg!("Also on the lock screen"))),
            to_code: gtk::CheckButton::with_label(tr(msg!(
                "The same key in every keyboard layout"
            ))),
            error: hint(),
            save: gtk::Button::with_label(tr(msg!("Save"))),
            state: Rc::new(RefCell::new(Editing {
                apps,
                model: Some(model.clone()),
                keymap,
                ..Editing::default()
            })),
        };
        editor.lay_out();
        editor.connect();
        editor
    }

    /// Puts the widgets in the window.
    fn lay_out(&self) {
        self.command.set_placeholder_text(Some("foot -e htop"));
        self.sway.set_placeholder_text(Some("layout toggle split"));
        self.sway.add_css_class("monospace");
        self.stack.add_named(&self.app, Some("app"));
        self.stack.add_named(&self.command, Some("command"));
        self.stack.add_named(&self.direction, Some("direction"));
        self.stack.add_named(&self.workspace, Some("workspace"));
        self.stack
            .add_named(&gtk::Box::new(gtk::Orientation::Vertical, 0), Some("none"));
        self.stack.add_named(&self.sway, Some("sway"));
        self.error.add_css_class("error");
        self.save.add_css_class("suggested-action");
        let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
        cancel.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self.window,
            move |_| window.close()
        ));
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        buttons.set_halign(gtk::Align::End);
        buttons.append(&cancel);
        buttons.append(&self.save);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(18);
        content.set_margin_bottom(18);
        content.set_margin_start(18);
        content.set_margin_end(18);
        content.append(&caption(msg!("Keys")));
        content.append(&self.capture);
        content.append(&self.conflict);
        content.append(&caption(msg!("What it does")));
        content.append(&self.action);
        content.append(&self.stack);
        content.append(&self.locked);
        content.append(&self.to_code);
        content.append(&self.error);
        content.append(&buttons);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_propagate_natural_height(true);
        scroller.set_child(Some(&content));
        self.window.set_child(Some(&scroller));
    }

    /// Follows the widgets.
    fn connect(&self) {
        let refresh = {
            let editor = self.clone();
            move || editor.refresh()
        };
        let refresh = Rc::new(refresh);
        for dropdown in [&self.action, &self.direction, &self.app] {
            let refresh = refresh.clone();
            dropdown.connect_selected_notify(move |_| refresh());
        }
        for entry in [&self.command, &self.sway] {
            let refresh = refresh.clone();
            entry.connect_changed(move |_| refresh());
        }
        {
            let refresh = refresh.clone();
            self.workspace.connect_value_changed(move |_| refresh());
        }
        for check in [&self.locked, &self.to_code] {
            let refresh = refresh.clone();
            check.connect_toggled(move |_| refresh());
        }
        let editor = self.clone();
        self.capture.connect_clicked(move |_| editor.listen(true));
        let editor = self.clone();
        self.save.connect_clicked(move |_| editor.store());
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let editor = self.clone();
        keys.connect_key_pressed(move |controller, keyval, keycode, state| {
            editor.key_pressed(controller, keyval, keycode, state)
        });
        self.window.add_controller(keys);
        let editor = self.clone();
        self.window.connect_close_request(move |_| {
            editor.listen(false);
            glib::Propagation::Proceed
        });
        let editor = self.clone();
        self.window.connect_is_active_notify(move |window| {
            if !window.is_active() {
                editor.listen(false);
            }
        });
    }

    /// Starts or stops listening for keys. While it listens, sway passes every key to the
    /// window.
    fn listen(&self, on: bool) {
        let was = std::mem::replace(&mut self.state.borrow_mut().capturing, on);
        if let Some(toplevel) = self.window.surface().and_downcast::<gdk::Toplevel>() {
            if on && !was {
                toplevel.inhibit_system_shortcuts(None::<&gdk::Event>);
            } else if !on && was {
                toplevel.restore_system_shortcuts();
            }
        }
        self.refresh();
    }

    /// A key pressed on the window: taken while the field listens.
    fn key_pressed(
        &self,
        controller: &gtk::EventControllerKey,
        keyval: gdk::Key,
        keycode: u32,
        state: gdk::ModifierType,
    ) -> glib::Propagation {
        if !self.state.borrow().capturing {
            return glib::Propagation::Proceed;
        }
        let event = controller
            .current_event()
            .and_then(|event| event.downcast::<gdk::KeyEvent>().ok());
        let modifier = event.as_ref().is_some_and(gdk::KeyEvent::is_modifier);
        let layout = event.as_ref().map_or(0, gdk::KeyEvent::layout);
        let level = event.as_ref().map_or(0, gdk::KeyEvent::level);
        let name = first_level(&WidgetExt::display(&self.window), keycode, layout)
            .or_else(|| keyval.to_lower().name().map(|name| name.to_string()))
            .unwrap_or_default();
        match capture(&name, state, level, self.main_modifier(), modifier) {
            Captured::Wait => return glib::Propagation::Stop,
            Captured::Cancel => {}
            Captured::Clear => self.state.borrow_mut().keys = None,
            Captured::Keys(keys) => self.state.borrow_mut().keys = Some(keys),
        }
        self.listen(false);
        glib::Propagation::Stop
    }

    /// The main modifier of the bindings of Sway Things.
    fn main_modifier(&self) -> Modifiers {
        let model = self.state.borrow().model.clone();
        Registry::builtin()
            .get(MODIFIER)
            .zip(model)
            .and_then(|(setting, model)| model.view(setting, None))
            .map_or(Modifiers::SUPER, |view| match view.value {
                Value::Choice(id) => ours::modifier_of(&id),
                _ => Modifiers::SUPER,
            })
    }

    /// Fills the dialog with the binding of the row at `index`, or leaves it empty.
    fn load(&self, model: &Model, index: Option<usize>) {
        let keymap = self.state.borrow().keymap.clone();
        let listing = model.bindings((*keymap).as_ref().map(|keymap| keymap as &dyn Keymap));
        let row = index.and_then(|index| listing.as_ref()?.rows().get(index).cloned());
        let Some(row) = row else {
            self.window.set_title(Some(tr(msg!("Add Shortcut"))));
            self.action.set_selected(1);
            return;
        };
        self.window.set_title(Some(tr(msg!("Change Shortcut"))));
        // A binding of Sway Things as stored, with `$mod`.
        let items = model.binding_items();
        let stored = match row.kind {
            RowKind::Added { item } | RowKind::Changed { item, .. } => items
                .get(item)
                .and_then(|item| ours::parse(item).ok())
                .and_then(|item| item.binding().cloned()),
            _ => None,
        };
        let base = stored.unwrap_or_else(|| row.line.clone());
        self.locked.set_active(base.flags.locked);
        self.to_code.set_active(base.flags.to_code);
        if let Some(command) = &base.command {
            self.show_action(command);
        }
        let mut editing = self.state.borrow_mut();
        editing.keys = Some(base.keys.clone());
        editing.base = Some(base);
        editing.row = Some(row);
    }

    /// Sets the action of the drop-downs to what `command` does.
    fn show_action(&self, command: &str) {
        let action = BindCommand::new(command).map_or_else(
            |_| Action::Sway(command.to_owned()),
            |command| Action::of(&command),
        );
        let direction = |direction: Direction| {
            Direction::ALL
                .iter()
                .position(|known| *known == direction)
                .and_then(|index| u32::try_from(index).ok())
                .unwrap_or(0)
        };
        let selected = match action {
            Action::Run(line) => {
                let app = self.state.borrow().apps.iter().position(|app| *app == line);
                if let Some(app) = app.and_then(|app| u32::try_from(app).ok()) {
                    self.app.set_selected(app);
                    0
                } else {
                    self.command.set_text(&line);
                    1
                }
            }
            Action::Focus(to) => {
                self.direction.set_selected(direction(to));
                2
            }
            Action::Move(to) => {
                self.direction.set_selected(direction(to));
                3
            }
            Action::Workspace(number) => {
                self.workspace.set_value(f64::from(number));
                4
            }
            Action::MoveToWorkspace(number) => {
                self.workspace.set_value(f64::from(number));
                5
            }
            Action::Close => 6,
            Action::Fullscreen => 7,
            Action::Floating => 8,
            Action::Sway(text) => {
                self.sway.set_text(&text);
                9
            }
        };
        self.action.set_selected(selected);
    }

    /// The action the drop-downs say.
    fn action(&self) -> Option<Action> {
        let direction = Direction::ALL
            .get(usize::try_from(self.direction.selected()).unwrap_or(0))
            .copied()
            .unwrap_or(Direction::Left);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the spin button holds whole numbers from 1 to MAX_WORKSPACE"
        )]
        let workspace = self.workspace.value() as u32;
        Some(match self.action.selected() {
            0 => {
                let index = usize::try_from(self.app.selected()).ok()?;
                Action::Run(self.state.borrow().apps.get(index)?.clone())
            }
            1 => Action::Run(self.command.text().to_string()),
            2 => Action::Focus(direction),
            3 => Action::Move(direction),
            4 => Action::Workspace(workspace),
            5 => Action::MoveToWorkspace(workspace),
            6 => Action::Close,
            7 => Action::Fullscreen,
            8 => Action::Floating,
            _ => Action::Sway(self.sway.text().to_string()),
        })
    }

    /// The binding the dialog makes, or why it can't make one.
    fn line(&self) -> Result<Line, String> {
        let editing = self.state.borrow();
        let Some(keys) = editing.keys.clone() else {
            return Err(tr(msg!("Press the keys of the shortcut.")).to_owned());
        };
        let action = self
            .action()
            .ok_or_else(|| tr(msg!("Pick an app.")).to_owned())?;
        // The command of the binding being changed stays as written while what it does stays.
        let base = editing.base.clone();
        let kept = base
            .as_ref()
            .and_then(|base| base.command.as_deref())
            .and_then(|command| BindCommand::new(command).ok())
            .filter(|command| Action::of(command) == action);
        let command = match kept {
            Some(command) => command,
            None => action
                .command()
                .map_err(|error| error.text().translated())?,
        };
        // A binding by keycode stays one while its keys don't change.
        let code = base
            .as_ref()
            .is_some_and(|base| base.code && base.keys == keys);
        let mut words = vec![if code { "bindcode" } else { "bindsym" }.to_owned()];
        let mut flags = base.as_ref().map(|base| base.flags).unwrap_or_default();
        flags.locked = self.locked.is_active();
        flags.to_code = self.to_code.is_active() && !code;
        flags.no_warn = false;
        words.extend(flags.words().into_iter().map(str::to_owned));
        if let Some(device) = base.as_ref().and_then(|base| base.input.clone()) {
            words.push(format!("--input-device={device}"));
        }
        words.push(keys);
        words.push(command.as_str().to_owned());
        let line = ours::parse_item(&words.join(" ")).map_err(|error| error.text().translated())?;
        // A key that types something, bound alone, would no longer reach any app.
        if !sway_things_core::bindings::keys::leaves_typing(&line.combo) {
            return Err(tr(msg!(
                "Add a modifier like Super or Ctrl: bound alone, this key would no longer type \
                 in any app. Only keys like F1, Print or the media keys work alone."
            ))
            .to_owned());
        }
        Ok(line)
    }

    /// Shows the keys, the warnings and whether the binding can be saved.
    fn refresh(&self) {
        let page = match self.action.selected() {
            0 => "app",
            1 => "command",
            2 | 3 => "direction",
            4 | 5 => "workspace",
            6..=8 => "none",
            _ => "sway",
        };
        self.stack.set_visible_child_name(page);
        self.show_keys();
        let line = self.line();
        let blocked = if let Ok(line) = &line {
            self.show_conflicts(line)
        } else {
            self.conflict.set_visible(false);
            false
        };
        let error = line
            .as_ref()
            .err()
            .filter(|_| self.state.borrow().keys.is_some());
        self.error.set_visible(error.is_some());
        self.error.set_text(error.map_or("", String::as_str));
        let writable = self
            .state
            .borrow()
            .model
            .as_ref()
            .and_then(Model::status)
            .is_some_and(|status| status.is_active());
        self.save
            .set_sensitive(line.is_ok() && !blocked && writable);
        self.to_code.set_visible(
            !self
                .state
                .borrow()
                .base
                .as_ref()
                .is_some_and(|base| base.code),
        );
    }

    /// Shows the keys in the field, or what to do.
    fn show_keys(&self) {
        let editing = self.state.borrow();
        let text = if editing.capturing {
            tr(msg!("Press the shortcut… (Esc stops, Backspace clears)")).to_owned()
        } else if let Some(keys) = &editing.keys {
            ours::parse_item(&format!("bindsym {keys} nop")).map_or_else(
                |_| keys.clone(),
                |line| {
                    let keymap = (*editing.keymap)
                        .as_ref()
                        .map(|keymap| keymap as &dyn Keymap);
                    line.labels(self.main_modifier(), keymap)
                        .iter()
                        .map(sway_things_core::i18n::Text::translated)
                        .collect::<Vec<_>>()
                        .join(" + ")
                },
            )
        } else {
            tr(msg!("Click, then press the shortcut")).to_owned()
        };
        self.capture.set_label(&text);
        self.capture
            .update_property(&[gtk::accessible::Property::Label(&trf(
                msg!("Keys: {keys}"),
                &[("keys", &text)],
            ))]);
    }

    /// Shows what other bindings `line` would clash with. Returns whether it can't be saved:
    /// the keys are a media key group's, or a line of the user's that can't be changed.
    fn show_conflicts(&self, line: &Line) -> bool {
        let editing = self.state.borrow();
        let Some(model) = editing.model.clone() else {
            return false;
        };
        let keymap = (*editing.keymap)
            .as_ref()
            .map(|keymap| keymap as &dyn Keymap);
        let Some(listing) = model.bindings(keymap) else {
            return false;
        };
        let skip = editing
            .row
            .as_ref()
            .and_then(|row| listing.rows().iter().position(|other| other == row));
        let identity = ours::resolve(line, listing.modifier()).identity(DEFAULT_MODE, keymap);
        let mut texts = Vec::new();
        let mut blocked = false;
        for other in listing.users_of(line, DEFAULT_MODE, keymap, skip) {
            let same = other.line.identity(&other.mode, keymap) == identity;
            let fixed = !matches!(
                other.kind,
                RowKind::Hand | RowKind::Added { .. } | RowKind::Changed { .. }
            ) || (other.kind == RowKind::Hand && !other.editable());
            blocked |= same && fixed;
            texts.push(conflict_text(other, same, fixed));
        }
        self.conflict.set_visible(!texts.is_empty());
        self.conflict.set_text(&texts.join("\n"));
        blocked
    }

    /// Stores the binding and closes the dialog.
    fn store(&self) {
        let Ok(line) = self.line() else {
            return;
        };
        let (model, row, keymap) = {
            let editing = self.state.borrow();
            (
                editing.model.clone(),
                editing.row.clone(),
                editing.keymap.clone(),
            )
        };
        let Some(model) = model else { return };
        let keymap = (*keymap).as_ref().map(|keymap| keymap as &dyn Keymap);
        let Some(listing) = model.bindings(keymap) else {
            return;
        };
        let items = model.binding_items();
        let new = match &row {
            None => edit::add(&listing, &items, &line, keymap),
            Some(row) => {
                let Some(index) = listing.rows().iter().position(|other| other == row) else {
                    self.error.set_text(tr(msg!(
                        "The shortcut changed in the meantime. Close this and try again."
                    )));
                    self.error.set_visible(true);
                    return;
                };
                edit::change(&listing, &items, index, &line, keymap)
            }
        };
        if new != items {
            model.set(BINDINGS, None, Value::List(new));
        }
        self.window.close();
    }
}

/// What a row the new binding clashes with says: what it does and where, and what saving does to
/// it.
fn conflict_text(other: &Row, same: bool, fixed: bool) -> String {
    let what = other.description().map_or_else(
        || other.line.command.clone().unwrap_or_default(),
        |text| text.translated(),
    );
    let place = super::place(&other.source);
    let args: &[(&str, &dyn std::fmt::Display)] = &[("what", &what), ("place", &place)];
    match (same, fixed) {
        (true, true) => trf(
            msg!("Taken by {what} ({place}), which can't be replaced here. Pick other keys."),
            args,
        ),
        (true, false) => trf(
            msg!("In use by {what} ({place}). Saving puts this in its place."),
            args,
        ),
        (false, _) => trf(msg!("Also on these keys: {what} ({place})."), args),
    }
}

/// The keysym of the first level of `keycode` in `layout`.
fn first_level(display: &gdk::Display, keycode: u32, layout: u32) -> Option<String> {
    display
        .map_keycode(keycode)?
        .into_iter()
        .find(|(key, _)| key.level() == 0 && key.group() == i32::try_from(layout).unwrap_or(0))
        .and_then(|(_, keyval)| keyval.name())
        .map(|name| name.to_string())
}

/// The drop-down of the apps, by name, and the command line of each, without the field codes of
/// its desktop entry.
fn apps() -> (gtk::DropDown, Vec<String>) {
    let mut apps: Vec<(String, String)> = gtk::gio::AppInfo::all()
        .into_iter()
        .filter(gtk::gio::AppInfo::should_show)
        .filter_map(|app| {
            let line = app.commandline()?;
            let line = without_field_codes(&line.to_string_lossy());
            BindCommand::new(&format!("exec {line}"))
                .ok()
                .map(|_| (app.display_name().to_string(), line))
        })
        .collect();
    apps.sort_by_key(|(name, _)| name.to_lowercase());
    apps.dedup();
    let names: Vec<&str> = apps.iter().map(|(name, _)| name.as_str()).collect();
    let dropdown = gtk::DropDown::from_strings(&names);
    dropdown.set_enable_search(true);
    dropdown.set_search_match_mode(gtk::StringFilterMatchMode::Substring);
    (dropdown, apps.into_iter().map(|(_, line)| line).collect())
}

/// `line` (an `Exec` of a desktop entry) without its field codes (`%f`, `%U`, …); `%%` is `%`.
pub fn without_field_codes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '%' {
            if characters.next() == Some('%') {
                out.push('%');
            }
        } else {
            out.push(character);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A caption above a part of the dialog.
fn caption(text: Msg) -> gtk::Label {
    let label = gtk::Label::new(Some(tr(text)));
    label.set_xalign(0.0);
    label.add_css_class("heading");
    label
}

/// A line of text that shows only when there is something to say.
fn hint() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class(style::DIM);
    label.set_visible(false);
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_presses_become_keys() {
        let none = gdk::ModifierType::empty();
        let super_shift = gdk::ModifierType::SUPER_MASK | gdk::ModifierType::SHIFT_MASK;
        let keys = |keys: &str| Captured::Keys(keys.to_owned());
        let cases = [
            (
                ("e", super_shift, 0, Modifiers::SUPER, false),
                keys("$mod+Shift+e"),
            ),
            (
                ("e", super_shift, 0, Modifiers::ALT, false),
                keys("Mod4+Shift+e"),
            ),
            (
                (
                    "Return",
                    gdk::ModifierType::CONTROL_MASK,
                    0,
                    Modifiers::SUPER,
                    false,
                ),
                keys("Ctrl+Return"),
            ),
            (
                (
                    "Super_L",
                    gdk::ModifierType::SUPER_MASK,
                    0,
                    Modifiers::SUPER,
                    true,
                ),
                Captured::Wait,
            ),
            (
                ("Escape", none, 0, Modifiers::SUPER, false),
                Captured::Cancel,
            ),
            (
                ("BackSpace", none, 0, Modifiers::SUPER, false),
                Captured::Clear,
            ),
            (
                (
                    "Escape",
                    gdk::ModifierType::SUPER_MASK,
                    0,
                    Modifiers::SUPER,
                    false,
                ),
                keys("$mod+Escape"),
            ),
            // AltGr gives the third level, which sway sees as Mod5.
            (
                (
                    "q",
                    gdk::ModifierType::SUPER_MASK,
                    2,
                    Modifiers::SUPER,
                    false,
                ),
                keys("$mod+Mod5+q"),
            ),
            (("a", none, 0, Modifiers::SUPER, false), keys("a")),
        ];
        for ((name, state, level, main, modifier), expected) in cases {
            assert_eq!(
                capture(name, state, level, main, modifier),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn field_codes_are_left_out() {
        assert_eq!(without_field_codes("firefox %u"), "firefox");
        assert_eq!(
            without_field_codes("app --name=%c %F --x"),
            "app --name= --x"
        );
        assert_eq!(without_field_codes("printf 100%%"), "printf 100%");
    }
}
