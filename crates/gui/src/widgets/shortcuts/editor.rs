// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialog of a binding, opened by a click on its row or by _Add Shortcut…_. At the top, the
//! keys as large keycaps in a field that takes new ones as they are pressed; while it listens,
//! the keys already in use with the modifiers held show below it, and once keys are pressed, a
//! line says whether they are free, in use (saving puts the new binding in their place) or taken
//! by a binding that can't be replaced. Then what the binding does (an app, a command, focus or
//! move a window, a workspace, or a sway command) and its options, the clashes of the binding,
//! and the details: its command, its options in words, where it is (with _Open in Editor_) and
//! what Sway Things made of it. _Remove_ and _Back to Your Config's Binding_ sit at the start
//! of the buttons.
//!
//! A binding that can't be changed here opens the same dialog without the parts that change it:
//! why it can't, its details, and a button to the setting that owns it, if one does.
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
    Action, BindCommand, DEFAULT_MODE, Direction, Keymap, Line, Listing, MAX_WORKSPACE, Modifiers,
    Row, RowKind, edit,
};
use sway_things_core::registry::{Registry, Value};

use super::{GdkKeymap, parts};
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

/// The most keys the dialog lists as in use with the modifiers held.
const MAX_TAKEN: usize = 48;

thread_local! {
    /// The dialog opened last, for [`Editor::last_opened`].
    static LAST: RefCell<Option<Editor>> = const { RefCell::new(None) };
}

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

/// What the line below the keys says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// No other binding is on the keys.
    Free,
    /// Other bindings are on the keys; saving takes their place or adds to them.
    InUse,
    /// A binding that can't be replaced here is on the keys.
    Blocked,
    /// The binding can't be saved, for another reason.
    Error,
}

/// The dialog's widgets and what it edits.
#[derive(Debug, Clone)]
pub struct Editor {
    window: gtk::Window,
    heading: gtk::Label,
    capture: gtk::Button,
    capture_keys: gtk::Box,
    capture_hint: gtk::Label,
    status: gtk::Box,
    status_icon: gtk::Image,
    status_text: gtk::Label,
    taken: gtk::Box,
    taken_title: gtk::Label,
    taken_keys: gtk::FlowBox,
    locked: gtk::Box,
    locked_text: gtk::Label,
    edit_part: gtk::Box,
    action: gtk::DropDown,
    stack: gtk::Stack,
    command: gtk::Entry,
    sway: gtk::Entry,
    direction: gtk::DropDown,
    workspace: gtk::SpinButton,
    app: gtk::DropDown,
    locked_screen: gtk::CheckButton,
    to_code: gtk::CheckButton,
    clashes: gtk::Box,
    details: gtk::Expander,
    remove: gtk::Button,
    restore: gtk::Button,
    owner: gtk::Button,
    cancel: gtk::Button,
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
    /// Whether the binding can only be looked at here.
    read_only: bool,
    /// Whether the field listens for keys.
    capturing: bool,
    /// The modifiers held while the field listens.
    held: Modifiers,
    /// What the line below the keys says, if it shows.
    status: Option<(Status, String)>,
    /// The setting that owns the binding, for the button that opens it.
    owner: Option<&'static str>,
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
        // A new binding starts with its keys, as in GNOME Settings.
        if row.is_none() {
            editor.listen(true);
        }
        LAST.with(|last| last.replace(Some(editor.clone())));
        editor.window.connect_close_request(|window| {
            LAST.with(|last| {
                let mut last = last.borrow_mut();
                if last.as_ref().is_some_and(|editor| editor.window == *window) {
                    *last = None;
                }
            });
            glib::Propagation::Proceed
        });
        editor
    }

    /// The dialog opened last, while it is open: the one a click on a row or _Add Shortcut…_
    /// opened, for tests.
    pub fn last_opened() -> Option<Self> {
        LAST.with(|last| last.borrow().clone())
            .filter(|editor| editor.window.is_visible())
    }

    /// The dialog's window.
    pub fn window(&self) -> &gtk::Window {
        &self.window
    }

    /// The field that takes the keys.
    pub fn capture_button(&self) -> &gtk::Button {
        &self.capture
    }

    /// What the dialog says of the keys in use, if they are.
    pub fn conflict(&self) -> Option<String> {
        self.status_of(&[Status::InUse, Status::Blocked])
    }

    /// Why the binding can't be saved, if it can't for another reason than keys in use.
    pub fn error(&self) -> Option<String> {
        self.status_of(&[Status::Error])
    }

    /// Whether the dialog says the keys are free.
    pub fn says_free(&self) -> bool {
        self.status_of(&[Status::Free]).is_some()
    }

    /// The keys shown as in use with the modifiers held, while the field listens.
    pub fn taken_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        let mut child = self.taken_keys.first_child();
        while let Some(widget) = child {
            if let Some(label) = widget
                .downcast_ref::<gtk::FlowBoxChild>()
                .and_then(gtk::FlowBoxChild::child)
                .and_downcast::<gtk::Label>()
            {
                keys.push(label.text().to_string());
            }
            child = widget.next_sibling();
        }
        keys
    }

    /// Whether the field listens for keys.
    pub fn is_capturing(&self) -> bool {
        self.state.borrow().capturing
    }

    /// Whether the dialog only shows the binding, which can't be changed here.
    pub fn is_read_only(&self) -> bool {
        self.state.borrow().read_only
    }

    /// The button that saves.
    pub fn save_button(&self) -> &gtk::Button {
        &self.save
    }

    /// The button that removes the binding.
    pub fn remove_button(&self) -> &gtk::Button {
        &self.remove
    }

    /// The button that puts back the user's binding.
    pub fn restore_button(&self) -> &gtk::Button {
        &self.restore
    }

    /// The button that opens the setting that owns the binding.
    pub fn owner_button(&self) -> &gtk::Button {
        &self.owner
    }

    /// The part with the details: the command, the options, where it is.
    pub fn details(&self) -> &gtk::Expander {
        &self.details
    }

    /// Sets the keys as an item writes them, as if they were pressed.
    pub fn set_keys(&self, keys: &str) {
        self.state.borrow_mut().keys = Some(keys.to_owned());
        self.listen(false);
    }

    /// Sets the action to run `command` (`Run a command`).
    pub fn set_run(&self, command: &str) {
        self.action.set_selected(1);
        self.command.set_text(command);
        self.refresh();
    }

    /// Sets whether the binding also works on the lock screen.
    pub fn set_locked(&self, locked: bool) {
        self.locked_screen.set_active(locked);
        self.refresh();
    }

    /// Sets the action to the sway command `command`.
    pub fn set_sway(&self, command: &str) {
        self.action.set_selected(9);
        self.sway.set_text(command);
        self.refresh();
    }

    /// Starts listening for keys, as a click on the field does.
    pub fn start_capture(&self) {
        self.listen(true);
    }

    fn build(model: &Model, keymap: Rc<Option<GdkKeymap>>) -> Self {
        let window = gtk::Window::builder()
            .modal(true)
            .default_width(520)
            .resizable(true)
            .destroy_with_parent(true)
            .title(tr(msg!("Shortcut")))
            .build();
        window.add_css_class("st-dialog");
        window.add_css_class("st-shortcut-dialog");
        let action = gtk::DropDown::from_strings(&ACTIONS.map(tr));
        let direction = gtk::DropDown::from_strings(&DIRECTIONS.map(tr));
        let workspace = gtk::SpinButton::with_range(1.0, f64::from(MAX_WORKSPACE), 1.0);
        let (app, apps) = parts::apps();
        let (status, status_icon, status_text) = parts::icon_line("st-shortcut-status");
        let (locked, _, locked_text) = parts::icon_line("st-shortcut-locked");
        let editor = Self {
            window,
            heading: gtk::Label::new(None),
            capture: gtk::Button::new(),
            capture_keys: gtk::Box::new(gtk::Orientation::Horizontal, 0),
            capture_hint: gtk::Label::new(None),
            status,
            status_icon,
            status_text,
            taken: gtk::Box::new(gtk::Orientation::Vertical, 6),
            taken_title: gtk::Label::new(None),
            taken_keys: gtk::FlowBox::new(),
            locked,
            locked_text,
            edit_part: gtk::Box::new(gtk::Orientation::Vertical, 8),
            action,
            stack: gtk::Stack::new(),
            command: gtk::Entry::new(),
            sway: gtk::Entry::new(),
            direction,
            workspace,
            app,
            locked_screen: gtk::CheckButton::with_label(tr(msg!("Also on the lock screen"))),
            to_code: gtk::CheckButton::with_label(tr(msg!(
                "The same key in every keyboard layout"
            ))),
            clashes: gtk::Box::new(gtk::Orientation::Vertical, 6),
            details: gtk::Expander::new(Some(tr(msg!("Details")))),
            remove: gtk::Button::with_label(tr(msg!("Remove"))),
            restore: gtk::Button::with_label(tr(msg!("Back to Your Config's Binding"))),
            owner: gtk::Button::new(),
            cancel: gtk::Button::with_label(tr(msg!("Cancel"))),
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
        self.heading.add_css_class("title-2");
        self.heading.set_wrap(true);
        self.heading.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        self.heading.set_justify(gtk::Justification::Center);
        self.heading.set_max_width_chars(36);
        self.heading
            .set_accessible_role(gtk::AccessibleRole::Heading);
        // The field: the keys as large keycaps, or what to do, and a hint below.
        self.capture_keys.set_halign(gtk::Align::Center);
        self.capture_hint.add_css_class(style::DIM);
        self.capture_hint.add_css_class("st-shortcut-capture-hint");
        let field = gtk::Box::new(gtk::Orientation::Vertical, 8);
        field.set_valign(gtk::Align::Center);
        field.append(&self.capture_keys);
        field.append(&self.capture_hint);
        self.capture.set_child(Some(&field));
        self.capture.add_css_class("st-shortcut-capture");
        // The keys in use with the modifiers held, while the field listens.
        self.taken_title.set_xalign(0.0);
        self.taken_title.set_wrap(true);
        self.taken_title.add_css_class(style::DIM);
        self.taken_keys.set_selection_mode(gtk::SelectionMode::None);
        self.taken_keys.set_max_children_per_line(12);
        self.taken_keys.set_column_spacing(6);
        self.taken_keys.set_row_spacing(6);
        self.taken_keys.set_homogeneous(false);
        self.taken.add_css_class("st-shortcut-taken");
        self.taken.append(&self.taken_title);
        self.taken.append(&self.taken_keys);
        self.taken.set_visible(false);
        // What it does and the options.
        self.command.set_placeholder_text(Some("foot -e htop"));
        self.command.set_activates_default(true);
        self.sway.set_placeholder_text(Some("layout toggle split"));
        self.sway.add_css_class("monospace");
        self.sway.set_activates_default(true);
        self.stack.add_named(&self.app, Some("app"));
        self.stack.add_named(&self.command, Some("command"));
        self.stack.add_named(&self.direction, Some("direction"));
        self.stack.add_named(&self.workspace, Some("workspace"));
        self.stack
            .add_named(&gtk::Box::new(gtk::Orientation::Vertical, 0), Some("none"));
        self.stack.add_named(&self.sway, Some("sway"));
        self.stack.set_vhomogeneous(false);
        self.stack.set_hhomogeneous(false);
        let options = gtk::Box::new(gtk::Orientation::Vertical, 2);
        options.set_margin_top(4);
        options.append(&self.locked_screen);
        options.append(&self.to_code);
        self.edit_part.append(&caption(msg!("Action")));
        self.edit_part.append(&self.action);
        self.edit_part.append(&self.stack);
        self.edit_part.append(&options);
        self.details.add_css_class("st-shortcut-details");
        // The buttons: those that remove or put back at the start, the others at the end.
        self.remove.add_css_class("destructive-action");
        self.save.add_css_class("suggested-action");
        self.owner.add_css_class("suggested-action");
        let start = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        start.set_hexpand(true);
        start.append(&self.remove);
        start.append(&self.restore);
        let end = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        end.append(&self.cancel);
        end.append(&self.owner);
        end.append(&self.save);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.add_css_class("st-shortcut-buttons");
        buttons.append(&start);
        buttons.append(&end);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(24);
        content.set_margin_bottom(18);
        content.set_margin_start(24);
        content.set_margin_end(24);
        content.append(&self.heading);
        content.append(&self.capture);
        content.append(&self.taken);
        content.append(&self.status);
        content.append(&self.clashes);
        content.append(&self.locked);
        content.append(&self.edit_part);
        content.append(&self.details);
        content.append(&buttons);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_propagate_natural_height(true);
        scroller.set_max_content_height(720);
        scroller.set_child(Some(&content));
        self.window.set_child(Some(&scroller));
        self.window.set_default_widget(Some(&self.save));
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
        for check in [&self.locked_screen, &self.to_code] {
            let refresh = refresh.clone();
            check.connect_toggled(move |_| refresh());
        }
        let editor = self.clone();
        self.capture
            .connect_clicked(move |_| editor.start_capture());
        let editor = self.clone();
        self.save.connect_clicked(move |_| editor.store());
        let editor = self.clone();
        self.remove
            .connect_clicked(move |_| editor.change_row(edit::remove));
        let editor = self.clone();
        self.restore
            .connect_clicked(move |_| editor.change_row(edit::restore));
        let editor = self.clone();
        self.owner.connect_clicked(move |_| {
            let owner = editor.state.borrow().owner;
            // The main window, before the dialog is gone.
            let main = editor.window.transient_for();
            editor.window.close();
            if let (Some(key), Some(main)) = (owner, main) {
                super::open_setting(&main, key);
            }
        });
        let window = self.window.clone();
        self.cancel.connect_clicked(move |_| window.close());
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let editor = self.clone();
        keys.connect_key_pressed(move |controller, keyval, keycode, state| {
            editor.key_pressed(controller, keyval, keycode, state)
        });
        let editor = self.clone();
        keys.connect_key_released(move |_, keyval, _, state| {
            if editor.state.borrow().capturing {
                editor.state.borrow_mut().held =
                    parts::held_in(state).without(parts::modifier_key(keyval));
                editor.show_taken();
            }
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
        let was = {
            let mut editing = self.state.borrow_mut();
            if !on {
                editing.held = Modifiers::NONE;
            }
            std::mem::replace(&mut editing.capturing, on)
        };
        if let Some(toplevel) = self.window.surface().and_downcast::<gdk::Toplevel>() {
            if on && !was {
                toplevel.inhibit_system_shortcuts(None::<&gdk::Event>);
            } else if !on && was {
                toplevel.restore_system_shortcuts();
            }
        }
        self.refresh();
    }

    /// A key pressed on the window: taken while the field listens; Esc closes the dialog
    /// otherwise.
    fn key_pressed(
        &self,
        controller: &gtk::EventControllerKey,
        keyval: gdk::Key,
        keycode: u32,
        state: gdk::ModifierType,
    ) -> glib::Propagation {
        if !self.state.borrow().capturing {
            if keyval == gdk::Key::Escape && state.is_empty() {
                self.window.close();
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }
        let event = controller
            .current_event()
            .and_then(|event| event.downcast::<gdk::KeyEvent>().ok());
        let pressed = parts::modifier_key(keyval);
        let modifier = event
            .as_ref()
            .map_or(!pressed.is_empty(), gdk::KeyEvent::is_modifier);
        let layout = event.as_ref().map_or(0, gdk::KeyEvent::layout);
        let level = event.as_ref().map_or(0, gdk::KeyEvent::level);
        if modifier {
            self.state.borrow_mut().held = parts::held_in(state).union(pressed);
            self.show_taken();
            return glib::Propagation::Stop;
        }
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

    /// The listing of the model, with the keymap.
    fn listing(&self) -> Option<Listing> {
        let editing = self.state.borrow();
        let keymap = (*editing.keymap)
            .as_ref()
            .map(|keymap| keymap as &dyn Keymap);
        editing.model.as_ref()?.bindings(keymap)
    }

    /// Fills the dialog with the binding of the row at `index`, or leaves it empty.
    fn load(&self, model: &Model, index: Option<usize>) {
        let keymap = self.state.borrow().keymap.clone();
        let keymap = (*keymap).as_ref().map(|keymap| keymap as &dyn Keymap);
        let listing = model.bindings(keymap);
        let row = index.and_then(|index| listing.as_ref()?.rows().get(index).cloned());
        let (Some(row), Some(listing)) = (row, listing) else {
            self.window.set_title(Some(tr(msg!("Add Shortcut"))));
            self.heading.set_text(tr(msg!("Add Shortcut")));
            self.action.set_selected(1);
            self.details.set_visible(false);
            self.remove.set_visible(false);
            self.restore.set_visible(false);
            self.owner.set_visible(false);
            self.locked.set_visible(false);
            self.clashes.set_visible(false);
            return;
        };
        let read_only = !row.editable();
        let (title, _) = super::title_of(&row);
        if read_only {
            self.window.set_title(Some(&title));
            self.heading.set_text(&title);
        } else {
            self.window.set_title(Some(tr(msg!("Change Shortcut"))));
            self.heading.set_text(tr(msg!("Change Shortcut")));
        }
        // Why it can't be changed: the lock's reason, or what became of a removed binding.
        let reason = super::locked_reason(&row).or_else(|| {
            matches!(row.kind, RowKind::Removed { .. })
                .then(|| super::status_lines(&row, &listing, keymap).pop())
                .flatten()
        });
        self.locked.set_visible(reason.is_some());
        self.locked_text
            .set_text(reason.as_deref().unwrap_or_default());
        self.edit_part.set_visible(!read_only);
        self.save.set_visible(!read_only);
        self.remove.set_visible(!read_only);
        self.restore.set_visible(edit::can_restore(&row));
        self.cancel.set_label(tr(if read_only {
            msg!("Close")
        } else {
            msg!("Cancel")
        }));
        let owner = super::owner(&row);
        let owner_title = owner.and_then(super::owner_button_title);
        self.owner.set_visible(owner_title.is_some());
        self.owner
            .set_label(owner_title.as_deref().unwrap_or_default());
        if owner_title.is_some() {
            self.window.set_default_widget(Some(&self.owner));
        }
        // Shown as it is, not greyed out, but nothing to click.
        self.capture.set_can_target(!read_only);
        self.capture.set_focusable(!read_only);
        for clash in &row.clashes {
            self.clashes.append(&super::clash_line(clash));
        }
        self.clashes.set_visible(!row.clashes.is_empty());
        self.details.set_child(Some(&parts::details(
            model, &row, &listing, keymap, read_only,
        )));
        self.details.set_expanded(read_only);
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
        self.locked_screen.set_active(base.flags.locked);
        self.to_code.set_active(base.flags.to_code);
        if let Some(command) = &base.command {
            self.show_action(command);
        }
        let mut editing = self.state.borrow_mut();
        editing.keys = Some(base.keys.clone());
        editing.base = Some(base);
        editing.read_only = read_only;
        editing.owner = owner;
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
        flags.locked = self.locked_screen.is_active();
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

    /// Shows the keys, the line below them and whether the binding can be saved.
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
        self.show_taken();
        let (read_only, capturing, has_keys, unchanged) = {
            let editing = self.state.borrow();
            let unchanged = editing
                .base
                .as_ref()
                .is_some_and(|base| Some(&base.keys) == editing.keys.as_ref());
            (
                editing.read_only,
                editing.capturing,
                editing.keys.is_some(),
                unchanged,
            )
        };
        if read_only {
            self.set_status(None);
            return;
        }
        let line = self.line();
        let (status, blocked) = match &line {
            _ if capturing || !has_keys => (None, false),
            Ok(line) => {
                let (texts, blocked) = self.conflicts(line);
                let status = if texts.is_empty() && unchanged {
                    // The binding's own keys: nothing to say about them.
                    None
                } else if texts.is_empty() {
                    Some((Status::Free, tr(msg!("These keys are free.")).to_owned()))
                } else if blocked {
                    Some((Status::Blocked, texts.join("\n")))
                } else {
                    Some((Status::InUse, texts.join("\n")))
                };
                (status, blocked)
            }
            Err(error) => (Some((Status::Error, error.clone())), false),
        };
        self.set_status(status);
        let writable = self
            .state
            .borrow()
            .model
            .as_ref()
            .and_then(Model::status)
            .is_some_and(|status| status.is_active());
        self.save
            .set_sensitive(line.is_ok() && !blocked && writable && !capturing);
        self.remove.set_sensitive(writable);
        self.restore.set_sensitive(writable);
        self.to_code.set_visible(
            !self
                .state
                .borrow()
                .base
                .as_ref()
                .is_some_and(|base| base.code),
        );
    }

    /// Shows `status` in the line below the keys, or hides it.
    fn set_status(&self, status: Option<(Status, String)>) {
        for class in ["st-free", "st-in-use", "st-blocked", "st-error"] {
            self.status.remove_css_class(class);
        }
        self.status.set_visible(status.is_some());
        if let Some((kind, text)) = &status {
            let (icon, class) = match kind {
                Status::Free => ("object-select-symbolic", "st-free"),
                Status::InUse => ("dialog-warning-symbolic", "st-in-use"),
                Status::Blocked => ("dialog-error-symbolic", "st-blocked"),
                Status::Error => ("dialog-error-symbolic", "st-error"),
            };
            self.status_icon.set_icon_name(Some(icon));
            self.status.add_css_class(class);
            self.status_text.set_text(text);
        }
        self.state.borrow_mut().status = status;
    }

    /// The text of the line below the keys, if it says one of `kinds`.
    fn status_of(&self, kinds: &[Status]) -> Option<String> {
        self.state
            .borrow()
            .status
            .as_ref()
            .filter(|(kind, _)| kinds.contains(kind))
            .map(|(_, text)| text.clone())
    }

    /// Shows the keys in the field as large keycaps, or what to do.
    fn show_keys(&self) {
        while let Some(child) = self.capture_keys.first_child() {
            self.capture_keys.remove(&child);
        }
        let editing = self.state.borrow();
        let labels = editing.keys.as_ref().map(|keys| {
            ours::parse_item(&format!("bindsym {keys} nop")).map_or_else(
                |_| vec![keys.clone()],
                |line| {
                    let keymap = (*editing.keymap)
                        .as_ref()
                        .map(|keymap| keymap as &dyn Keymap);
                    line.labels(self.main_modifier(), keymap)
                        .iter()
                        .map(sway_things_core::i18n::Text::translated)
                        .collect::<Vec<_>>()
                },
            )
        });
        let (name, hint) = if editing.capturing {
            let prompt = gtk::Label::new(Some(tr(msg!("Press the new shortcut…"))));
            prompt.add_css_class("st-shortcut-prompt");
            self.capture_keys.append(&prompt);
            self.capture.add_css_class("st-capturing");
            (
                tr(msg!("Press the new shortcut…")).to_owned(),
                Some(tr(msg!("Esc to cancel · Backspace to clear"))),
            )
        } else if let Some(labels) = labels {
            let caps = super::keycaps(&labels);
            caps.add_css_class("st-keycaps-large");
            self.capture_keys.append(&caps);
            self.capture.remove_css_class("st-capturing");
            (
                labels.join(" + "),
                (!editing.read_only).then(|| tr(msg!("Click to change"))),
            )
        } else {
            let prompt = gtk::Label::new(Some(tr(msg!("Click to set the shortcut"))));
            prompt.add_css_class("st-shortcut-prompt");
            self.capture_keys.append(&prompt);
            self.capture.remove_css_class("st-capturing");
            (tr(msg!("No keys yet")).to_owned(), None)
        };
        self.capture_hint.set_visible(hint.is_some());
        self.capture_hint.set_text(hint.unwrap_or_default());
        self.capture
            .update_property(&[gtk::accessible::Property::Label(&trf(
                msg!("Keys: {keys}"),
                &[("keys", &name)],
            ))]);
    }

    /// Shows the keys in use with the modifiers held, while the field listens.
    fn show_taken(&self) {
        while let Some(child) = self.taken_keys.first_child() {
            self.taken_keys.remove(&child);
        }
        let (capturing, held) = {
            let editing = self.state.borrow();
            (editing.capturing, editing.held)
        };
        self.taken.set_visible(capturing);
        if !capturing {
            return;
        }
        if held.is_empty() {
            self.taken_title.set_text(tr(msg!(
                "Hold Super, Ctrl, Alt or Shift to see which keys are taken with them."
            )));
            self.taken_keys.set_visible(false);
            return;
        }
        let modifiers = held
            .labels()
            .iter()
            .map(sway_things_core::i18n::Text::translated)
            .collect::<Vec<_>>()
            .join(" + ");
        let keys = self
            .listing()
            .map(|listing| {
                let editing = self.state.borrow();
                let keymap = (*editing.keymap)
                    .as_ref()
                    .map(|keymap| keymap as &dyn Keymap);
                parts::taken(&listing, held, keymap)
            })
            .unwrap_or_default();
        self.taken_title.set_text(&if keys.is_empty() {
            trf(
                msg!("No shortcut uses {modifiers} yet: every key is free."),
                &[("modifiers", &modifiers)],
            )
        } else {
            trf(
                msg!("Taken with {modifiers}:"),
                &[("modifiers", &modifiers)],
            )
        });
        self.taken_keys.set_visible(!keys.is_empty());
        for (key, what) in keys.into_iter().take(MAX_TAKEN) {
            let cap = super::keycap(&key);
            cap.set_tooltip_text(Some(&what));
            cap.update_property(&[gtk::accessible::Property::Description(&what)]);
            self.taken_keys.append(&cap);
        }
    }

    /// What `line` would clash with, in words, and whether it can't be saved: the keys are a
    /// media key group's, or a line of the user's that can't be changed.
    fn conflicts(&self, line: &Line) -> (Vec<String>, bool) {
        let editing = self.state.borrow();
        let Some(model) = editing.model.clone() else {
            return (Vec::new(), false);
        };
        let keymap = (*editing.keymap)
            .as_ref()
            .map(|keymap| keymap as &dyn Keymap);
        let Some(listing) = model.bindings(keymap) else {
            return (Vec::new(), false);
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
            texts.push(parts::conflict_text(other, same, fixed));
        }
        (texts, blocked)
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
                    self.changed_meanwhile();
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

    /// Removes the binding or puts back the user's, with `change` (of [`edit`]), and closes the
    /// dialog.
    fn change_row(&self, change: fn(&Listing, &[String], usize) -> Vec<String>) {
        let (model, row) = {
            let editing = self.state.borrow();
            (editing.model.clone(), editing.row.clone())
        };
        let (Some(model), Some(row), Some(listing)) = (model, row, self.listing()) else {
            return;
        };
        let Some(index) = listing.rows().iter().position(|other| *other == row) else {
            self.changed_meanwhile();
            return;
        };
        let items = model.binding_items();
        let new = change(&listing, &items, index);
        if new != items {
            model.set(BINDINGS, None, Value::List(new));
        }
        self.window.close();
    }

    /// Says that the binding changed since the dialog opened.
    fn changed_meanwhile(&self) {
        self.set_status(Some((
            Status::Error,
            tr(msg!(
                "The shortcut changed in the meantime. Close this and try again."
            ))
            .to_owned(),
        )));
    }
}

/// The keysym of the first level of `keycode` in `layout`.
pub(crate) fn first_level(display: &gdk::Display, keycode: u32, layout: u32) -> Option<String> {
    display
        .map_keycode(keycode)?
        .into_iter()
        .find(|(key, _)| key.level() == 0 && key.group() == i32::try_from(layout).unwrap_or(0))
        .and_then(|(_, keyval)| keyval.name())
        .map(|name| name.to_string())
}

/// A caption above a part of the dialog.
fn caption(text: Msg) -> gtk::Label {
    let label = gtk::Label::new(Some(tr(text)));
    label.set_xalign(0.0);
    label.add_css_class("heading");
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
}
