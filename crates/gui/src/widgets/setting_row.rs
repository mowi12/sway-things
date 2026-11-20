// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A row for one setting, built from its registry entry: title, subtitle, the line of the sway
//! config in monospace below the subtitle while the preference `app.show_directives` is on (not
//! in a member of a group, whose [`GroupRow`](super::GroupRow) shows it once), and the control
//! for its value type.
//!
//! | Value type | Control |
//! | --- | --- |
//! | on/off | switch |
//! | whole or decimal number | slider over the setting's slider range, and a number field for every value it accepts |
//! | choice | drop-down |
//! | color | color button |
//! | text | entry, applied on Enter or when it loses the focus |
//! | text with choices ([`SettingRow::with_choices`]) | drop-down with search |
//! | list | none: the page shows its own widget below the row |
//!
//! A change goes to the [`Model`] at once; the row shows the value in effect again when the model
//! reports a change, and the error under the title when the change failed. A locked setting (no
//! include line, a compositor too old) shows why and can't be changed.
//!
//! Indicators: a stored value gets a reset button and a "This machine only" toggle (which moves
//! it between `settings.toml` and the host overlay) next to the control. A line of the user's
//! config that wins over the stored value, or that the stored value made dead, shows as a badge
//! under the line, and a ⋮ menu with the line and what to do about it. A setting the compositor
//! doesn't support shows what it requires as a badge and can't be changed, and so does one that
//! only matters while another setting is on, with that reason under its line. A setting sway
//! only reads from its config says so in a badge ("Applies after reload"), and so does one it
//! only reads at login ("Applies after re-login").
//!
//! The row of a per-device setting is for all devices of its type or, after
//! [`SettingRow::set_device`], for one of them. Until that device has a value of its own, the
//! row says it is the same as for all devices of the type.

mod commit;
mod controls;
mod indicators;

pub(super) use self::controls::shows_directives;
use self::controls::{color, control, number, show_value};
pub(crate) use self::indicators::tilde;
use self::indicators::{conflict_line, note};
use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::outputs;
use sway_things_core::registry::{Font, Scope, Setting, Value};
use sway_things_core::state::Layer;

use super::RowLayout;
use crate::i18n::{msg, tr};
use crate::model::Model;
use crate::style;

/// The width of a slider when there is room, in pixels.
const SLIDER_WIDTH: i32 = 200;

/// The width a slider shrinks to on a narrow row before the controls move below the text.
const SLIDER_MINIMUM_WIDTH: i32 = 120;

/// The preference that shows the line of the sway config in every row.
const DIRECTIVES_KEY: &str = "app.show_directives";

/// The control of a row.
#[derive(Debug)]
enum Control {
    Toggle(gtk::Switch),
    Number {
        scale: gtk::Scale,
        spin: gtk::SpinButton,
        whole: bool,
    },
    Choice {
        dropdown: gtk::DropDown,
        ids: Vec<&'static str>,
    },
    Color(gtk::ColorDialogButton),
    Text(gtk::Entry),
    /// A font description, shown as it is stored ("Adwaita Sans 11") in that font, and picked
    /// with GTK's font dialog. GTK's own font button names a font that isn't installed "None".
    Font(gtk::Button),
    /// A text picked from known values; a value that isn't one of them is added to the end.
    Pick {
        dropdown: gtk::DropDown,
        labels: gtk::StringList,
        values: Rc<RefCell<Vec<String>>>,
    },
    /// A list: the page shows its own widget for it.
    None,
}

impl Control {
    /// The widgets of the control, in order.
    fn widgets(&self) -> Vec<gtk::Widget> {
        match self {
            Self::Toggle(switch) => vec![switch.clone().upcast()],
            Self::Number { scale, spin, .. } => vec![spin.clone().upcast(), scale.clone().upcast()],
            Self::Choice { dropdown, .. } | Self::Pick { dropdown, .. } => {
                vec![dropdown.clone().upcast()]
            }
            Self::Color(button) => vec![button.clone().upcast()],
            Self::Font(button) => vec![button.clone().upcast()],
            Self::Text(entry) => vec![entry.clone().upcast()],
            Self::None => Vec::new(),
        }
    }
}

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::registry::{Setting, Value};

    use crate::model::Model;

    use std::sync::OnceLock;

    use gtk::glib::subclass::Signal;

    #[derive(Debug, Default)]
    pub struct SettingRow {
        pub(super) setting: OnceCell<&'static Setting>,
        pub(super) device: RefCell<Option<String>>,
        pub(super) model: OnceCell<Model>,
        pub(super) control: OnceCell<super::Control>,
        /// Set while the row writes a value into its control, so that isn't sent as a change.
        pub(super) updating: Cell<bool>,
        /// Set while a pointer button is down on a number control whose changes wait for it
        /// ([`super::SettingRow::defers`]).
        pub(super) held: Cell<bool>,
        /// Set while that is a finger, which has no button the seat could report.
        pub(super) touched: Cell<bool>,
        /// The value of such a control that waits to be sent, and the timer that sends it.
        pub(super) unsent: RefCell<Option<Value>>,
        pub(super) commit: RefCell<Option<glib::SourceId>>,
        /// Set for a member of a group, whose row shows the line of the sway config instead.
        pub(super) grouped: Cell<bool>,
        pub(super) title: gtk::Label,
        pub(super) subtitle: gtk::Label,
        pub(super) line: gtk::Label,
        pub(super) note: gtk::Label,
        pub(super) error: gtk::Label,
        pub(super) badges: gtk::Box,
        pub(super) controls: gtk::Box,
        /// Controls a page added after the first ([`super::SettingRow::add_control`]).
        pub(super) added: RefCell<Vec<gtk::Widget>>,
        pub(super) reset: gtk::Button,
        pub(super) machine: gtk::ToggleButton,
        pub(super) conflict: gtk::MenuButton,
        pub(super) conflict_title: gtk::Label,
        pub(super) conflict_place: gtk::Label,
        pub(super) conflict_line: gtk::Label,
        pub(super) conflict_text: gtk::Label,
        pub(super) let_config_win: gtk::Button,
        /// The lines of the user's config behind the ⋮ menu, as of the last refresh.
        pub(super) report: RefCell<Option<sway_things_core::conflict::Report>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SettingRow {
        const NAME: &'static str = "StSettingRow";
        type Type = super::SettingRow;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-setting-row");
            klass.set_layout_manager_type::<super::super::RowLayout>();
        }
    }

    impl ObjectImpl for SettingRow {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![Signal::builder("device-changed").build()])
        }

        fn dispose(&self) {
            // A value of the number control that waits is sent before the row goes: the control
            // showed it.
            self.obj().flush_unsent();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for SettingRow {}
}

glib::wrapper! {
    /// A row for one setting.
    pub struct SettingRow(ObjectSubclass<imp::SettingRow>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SettingRow {
    /// The row of `setting`, for the whole session or input type, or for one input `device`.
    /// `None` for a number setting without a slider. A list gets a row without a control: the
    /// page shows its own widget for it.
    pub fn new(model: &Model, setting: &'static Setting, device: Option<&str>) -> Option<Self> {
        Self::with_control(model, setting, device, control(setting)?)
    }

    /// The row of the text or whole-number setting `setting` with a drop-down of known `choices`
    /// (each a value as text and its label) in place of an entry or a slider. A value that isn't
    /// among them is shown at the end. [`SettingRow::set_choices`] replaces them.
    pub fn with_choices(
        model: &Model,
        setting: &'static Setting,
        device: Option<&str>,
        choices: &[(String, String)],
    ) -> Option<Self> {
        let labels: Vec<&str> = choices.iter().map(|(_, label)| label.as_str()).collect();
        let labels = gtk::StringList::new(&labels);
        let dropdown = gtk::DropDown::new(
            Some(labels.clone()),
            Some(gtk::PropertyExpression::new(
                gtk::StringObject::static_type(),
                gtk::Expression::NONE,
                "string",
            )),
        );
        dropdown.set_enable_search(true);
        dropdown.set_search_match_mode(gtk::StringFilterMatchMode::Substring);
        dropdown.set_valign(gtk::Align::Center);
        let control = Control::Pick {
            dropdown,
            labels,
            values: Rc::new(RefCell::new(
                choices.iter().map(|(value, _)| value.clone()).collect(),
            )),
        };
        Self::with_control(model, setting, device, control)
    }

    fn with_control(
        model: &Model,
        setting: &'static Setting,
        device: Option<&str>,
        control: Control,
    ) -> Option<Self> {
        let row: Self = glib::Object::new();
        let imp = row.imp();
        imp.setting.set(setting).ok()?;
        imp.device.replace(device.map(str::to_owned));
        imp.model.set(model.clone()).ok()?;
        row.build(control);
        Some(row)
    }

    fn build(&self, control: Control) {
        let imp = self.imp();
        let Some(setting) = imp.setting.get().copied() else {
            return;
        };
        let title = tr(setting.title());

        imp.title.set_text(title);
        imp.subtitle
            .set_text(setting.subtitle().map(tr).unwrap_or_default());
        imp.subtitle.set_visible(setting.subtitle().is_some());
        imp.line.add_css_class("monospace");
        imp.subtitle.add_css_class(style::DIM);
        imp.line.add_css_class(style::DIM);
        imp.note.add_css_class(style::DIM);
        imp.error.add_css_class("error");
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 3);
        texts.set_hexpand(true);
        texts.set_valign(gtk::Align::Center);
        for label in [&imp.title, &imp.subtitle, &imp.line, &imp.note, &imp.error] {
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            texts.append(label);
        }
        imp.badges.set_spacing(6);
        imp.badges.set_halign(gtk::Align::Start);
        imp.badges.set_visible(false);
        texts.append(&imp.badges);
        imp.title.add_css_class("st-row-title");
        for label in [&imp.line, &imp.note] {
            label.add_css_class("st-row-detail");
        }
        imp.error.set_visible(false);

        imp.controls.set_spacing(12);
        imp.controls.set_valign(gtk::Align::Center);
        for widget in control.widgets() {
            widget.update_property(&[gtk::accessible::Property::Label(title)]);
            imp.controls.append(&widget);
        }
        self.build_indicators(title);
        texts.set_parent(self);
        imp.controls.set_parent(self);
        if let (Control::Number { .. }, Some(layout)) = (&control, self.row_layout()) {
            layout.set_growth(SLIDER_WIDTH - SLIDER_MINIMUM_WIDTH);
        }

        self.connect_control(&control);
        let _ = imp.control.set(control);

        if let Some(model) = imp.model.get() {
            model.connect_changed(glib::clone!(
                #[weak(rename_to = row)]
                self,
                move |_| row.refresh()
            ));
            model.connect_failed(glib::clone!(
                #[weak(rename_to = row)]
                self,
                move |_, key, device, message| {
                    if row.is_for(key, device) {
                        row.show_error(Some(message));
                        row.refresh();
                    }
                }
            ));
        }
        self.refresh();
    }

    /// The layout of the row: the controls beside the text, or below it on a narrow row.
    fn row_layout(&self) -> Option<RowLayout> {
        self.layout_manager().and_downcast::<RowLayout>()
    }

    /// The narrowest the row can be with its controls beside its text, in pixels; narrower, they
    /// are below it.
    pub fn beside_width(&self) -> i32 {
        self.row_layout()
            .map_or(0, |layout| layout.beside_width(self))
    }

    /// The reset button, if the value is stored.
    pub fn reset_button(&self) -> Option<gtk::Button> {
        let reset = &self.imp().reset;
        reset.is_child_visible().then(|| reset.clone())
    }

    /// The "This machine only" toggle, if the value is stored.
    pub fn machine_toggle(&self) -> Option<gtk::ToggleButton> {
        let machine = &self.imp().machine;
        machine.is_child_visible().then(|| machine.clone())
    }

    /// The ⋮ menu button, if the user's config sets the setting too.
    pub fn conflict_button(&self) -> Option<gtk::MenuButton> {
        let conflict = &self.imp().conflict;
        conflict.is_child_visible().then(|| conflict.clone())
    }

    /// The texts of the badges under the line.
    pub fn badges(&self) -> Vec<String> {
        let mut texts = Vec::new();
        let mut child = self.imp().badges.first_child();
        while let Some(badge) = child {
            if let Some(label) = badge.last_child().and_downcast::<gtk::Label>() {
                texts.push(label.text().to_string());
            }
            child = badge.next_sibling();
        }
        texts
    }

    /// What the ⋮ menu shows: its title, the place of the line, and the line.
    pub fn conflict_details(&self) -> Option<(String, String, String)> {
        let imp = self.imp();
        imp.conflict.is_child_visible().then(|| {
            (
                imp.conflict_title.text().to_string(),
                imp.conflict_place.text().to_string(),
                imp.conflict_line.text().to_string(),
            )
        })
    }

    /// Removes the stored value.
    pub fn reset(&self) {
        let imp = self.imp();
        let (Some(setting), Some(model)) = (self.setting(), imp.model.get()) else {
            return;
        };
        self.show_error(None);
        model.reset(setting.key(), imp.device.borrow().as_deref());
    }

    /// Moves the stored value to the host overlay (`host`) or the shared file.
    fn move_to(&self, host: bool) {
        let imp = self.imp();
        if imp.updating.get() {
            return;
        }
        let (Some(setting), Some(model)) = (self.setting(), imp.model.get()) else {
            return;
        };
        let layer = if host { Layer::Host } else { Layer::Shared };
        model.move_to(setting.key(), imp.device.borrow().as_deref(), layer);
    }

    /// Lets the user's config decide the setting.
    pub fn let_config_win(&self) {
        let imp = self.imp();
        let (Some(model), Some(report)) = (imp.model.get(), imp.report.borrow().clone()) else {
            return;
        };
        self.show_error(None);
        model.let_config_win(report);
    }

    /// Opens the file with the line of the user's config in the user's text editor, at the line,
    /// as _Open config in editor_ in the ⋮ menu does.
    pub fn open_config(&self) {
        let Some((file, line)) = self
            .imp()
            .report
            .borrow()
            .as_ref()
            .and_then(conflict_line)
            .map(|(_, line)| (line.file().to_owned(), line.line()))
        else {
            return;
        };
        self.show_error(None);
        crate::editor::open(
            self,
            self.imp().model.get(),
            &file,
            Some(line),
            glib::clone!(
                #[weak(rename_to = row)]
                self,
                move |message| row.show_error(message)
            ),
        );
    }

    /// The setting of the row.
    pub fn setting(&self) -> Option<&'static Setting> {
        self.imp().setting.get().copied()
    }

    /// The device the row is for, or `None` for the whole session or input type.
    pub fn device(&self) -> Option<String> {
        self.imp().device.borrow().clone()
    }

    /// The widgets that change the value.
    pub fn controls(&self) -> Vec<gtk::Widget> {
        let mut widgets: Vec<gtk::Widget> = self
            .imp()
            .control
            .get()
            .map(Control::widgets)
            .unwrap_or_default();
        // And those a page added ([`SettingRow::add_control`]), after the first.
        let added = self.imp().added.borrow().clone();
        let at = 1.min(widgets.len());
        widgets.splice(at..at, added);
        widgets
    }

    /// The text under the title: the error of the last change, if it failed.
    pub fn error(&self) -> Option<String> {
        let error = &self.imp().error;
        error.is_visible().then(|| error.text().to_string())
    }

    /// Makes the row show and change the value for the input `device`, or with `None` for all
    /// devices of the type.
    pub fn set_device(&self, device: Option<&str>) {
        let imp = self.imp();
        if imp.device.borrow().as_deref() == device {
            return;
        }
        // A value that waits belongs to the display shown so far.
        self.flush_unsent();
        imp.device.replace(device.map(str::to_owned));
        self.show_error(None);
        self.refresh();
        self.emit_by_name::<()>("device-changed", &[]);
    }

    /// Adds `widget` after the row's own control, for a page that splits one value over two
    /// controls (a mode's resolution and refresh rate).
    pub fn add_control(&self, widget: &impl IsA<gtk::Widget>) {
        let imp = self.imp();
        imp.controls
            .insert_child_after(widget, imp.controls.first_child().as_ref());
        imp.added.borrow_mut().push(widget.clone().upcast());
    }

    /// Calls `callback` when the row is pointed at another device ([`SettingRow::set_device`]).
    pub fn connect_device_changed<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "device-changed",
            false,
            glib::closure_local!(move |row: &Self| callback(row)),
        )
    }

    /// Sends `value` as the new value of the setting, as the row's own control would. For the
    /// widget a page shows for a list setting below its row.
    pub fn change(&self, value: Value) {
        self.send(value);
    }

    /// The value the row shows: the one in effect, or the one of a risky change that waits.
    pub fn value(&self) -> Option<Value> {
        let imp = self.imp();
        let model = imp.model.get()?;
        model
            .view(self.setting()?, imp.device.borrow().as_deref())
            .map(|view| view.value)
    }

    /// Why the setting can't be changed now, if it can't.
    pub fn locked(&self) -> Option<String> {
        let imp = self.imp();
        let model = imp.model.get()?;
        model
            .view(self.setting()?, imp.device.borrow().as_deref())
            .and_then(|view| view.locked.map(|locked| locked.text().translated()))
    }

    /// Shows `message` under the title, as for a change that failed, or with `None` hides it.
    pub fn set_error(&self, message: Option<&str>) {
        self.show_error(message);
    }

    /// Makes the row a member of a group, or not: a member never shows the line of the sway
    /// config, since the group's row shows it once.
    pub fn set_grouped(&self, grouped: bool) {
        self.imp().grouped.set(grouped);
        self.refresh();
    }

    /// The line of the sway config under the subtitle, if the row shows it.
    pub fn line(&self) -> Option<String> {
        let line = &self.imp().line;
        line.is_visible().then(|| line.text().to_string())
    }

    /// The note under the line of the sway config, if there is one: where the value comes from,
    /// or why it can't be changed.
    pub fn note(&self) -> Option<String> {
        let note = &self.imp().note;
        note.is_visible().then(|| note.text().to_string())
    }

    fn is_for(&self, key: &str, device: Option<&str>) -> bool {
        self.setting().is_some_and(|setting| setting.key() == key)
            && self.imp().device.borrow().as_deref() == device
    }

    fn show_error(&self, message: Option<&str>) {
        let error = &self.imp().error;
        error.set_text(message.unwrap_or_default());
        error.set_visible(message.is_some());
    }

    /// Shows the value in effect and whether it can be changed.
    fn refresh(&self) {
        let imp = self.imp();
        let (Some(setting), Some(model), Some(control)) =
            (self.setting(), imp.model.get(), imp.control.get())
        else {
            return;
        };
        let device = imp.device.borrow().clone();
        let Some(view) = model.view(setting, device.as_deref()) else {
            return;
        };
        // While a newer value is on its way, the control already shows it.
        if !model.is_pending(setting.key(), device.as_deref()) && !self.holds_unsent() {
            imp.updating.set(true);
            show_value(control, setting, &view.value);
            imp.updating.set(false);
        }
        imp.line.set_text(view.line.as_deref().unwrap_or_default());
        imp.line
            .set_visible(view.line.is_some() && !imp.grouped.get() && shows_directives(model));
        self.refresh_indicators(setting, device.as_deref(), &view);
        let mut note = note(&view, setting, device.as_deref());
        // A display stored as off that the start of sway switched on: no other one was on.
        let on_anyway = setting.key() == outputs::ENABLED
            && view.value == Value::Bool(false)
            && device.as_deref().is_some_and(|device| {
                model
                    .outputs()
                    .and_then(|outputs| outputs.get(device).map(outputs::Output::is_enabled))
                    .unwrap_or(false)
            });
        if on_anyway {
            if !note.is_empty() {
                note.push_str(" \u{b7} ");
            }
            note.push_str(tr(msg!("On for now: no other display was on")));
        }
        imp.note.set_text(&note);
        imp.note.set_visible(!note.is_empty());
        // While a display's change waits to be kept or reverted, the others' settings wait too:
        // Revert puts back only that change.
        let waits_for_other = setting.scope() == Scope::PerOutput
            && model
                .pending_change()
                .is_some_and(|pending| pending.device() != device.as_deref());
        let reason = view
            .locked
            .as_ref()
            .map(|locked| locked.text().translated())
            .or_else(|| {
                waits_for_other.then(|| {
                    tr(msg!("Keep or revert the change of the other display first")).to_owned()
                })
            });
        // Controls a page added are locked with the row; the page opens them as it sees fit.
        if let Some(reason) = &reason {
            for widget in imp.added.borrow().iter() {
                widget.set_sensitive(false);
                widget.set_tooltip_text(Some(reason));
            }
        }
        for widget in control.widgets() {
            widget.set_sensitive(reason.is_none());
            widget.set_tooltip_text(reason.as_deref());
            match &reason {
                Some(reason) => {
                    widget.update_property(&[gtk::accessible::Property::Description(reason)]);
                }
                None => widget.reset_property(gtk::AccessibleProperty::Description),
            }
        }
    }

    fn connect_control(&self, control: &Control) {
        match control {
            Control::Toggle(switch) => {
                switch.connect_state_set(glib::clone!(
                    #[weak(rename_to = row)]
                    self,
                    #[upgrade_or]
                    glib::Propagation::Proceed,
                    move |_, on| {
                        row.send(Value::Bool(on));
                        glib::Propagation::Proceed
                    }
                ));
            }
            Control::Number { scale, spin, whole } => self.connect_number(scale, spin, *whole),
            Control::Choice { dropdown, ids } => {
                let ids = ids.clone();
                dropdown.connect_selected_notify(glib::clone!(
                    #[weak(rename_to = row)]
                    self,
                    move |dropdown| {
                        let selected = usize::try_from(dropdown.selected()).ok();
                        if let Some(id) = selected.and_then(|index| ids.get(index)) {
                            row.send(Value::Choice((*id).to_owned()));
                        }
                    }
                ));
            }
            Control::Color(button) => {
                button.connect_rgba_notify(glib::clone!(
                    #[weak(rename_to = row)]
                    self,
                    move |button| {
                        if let Some(color) = color(&button.rgba()) {
                            row.send(Value::Color(color));
                        }
                    }
                ));
            }
            Control::Font(button) => {
                button.connect_clicked(glib::clone!(
                    #[weak(rename_to = row)]
                    self,
                    move |button| row.pick_font(button)
                ));
            }
            Control::Text(entry) => {
                entry.connect_activate(glib::clone!(
                    #[weak(rename_to = row)]
                    self,
                    move |entry| row.send_text(entry)
                ));
                let focus = gtk::EventControllerFocus::new();
                focus.connect_leave(glib::clone!(
                    #[weak(rename_to = row)]
                    self,
                    #[weak]
                    entry,
                    move |_| row.send_text(&entry)
                ));
                entry.add_controller(focus);
            }
            Control::Pick {
                dropdown, values, ..
            } => self.connect_pick(dropdown, values),
            Control::None => {}
        }
    }

    /// Sends a number from `scale` or `spin`, which follow each other; `whole` for a whole
    /// number.
    fn connect_number(&self, scale: &gtk::Scale, spin: &gtk::SpinButton, whole: bool) {
        scale.connect_value_changed(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[weak]
            spin,
            move |scale| {
                if row.imp().updating.get() {
                    return;
                }
                let value = scale.value();
                row.imp().updating.set(true);
                spin.set_value(value);
                row.imp().updating.set(false);
                row.send_number(number(value, whole));
            }
        ));
        spin.connect_value_changed(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[weak]
            scale,
            move |spin| {
                if row.imp().updating.get() {
                    return;
                }
                let value = spin.value();
                row.imp().updating.set(true);
                scale.set_value(value);
                row.imp().updating.set(false);
                row.send_number(number(value, whole));
            }
        ));
        if self.defers() {
            self.follow_presses(scale.upcast_ref());
            self.follow_presses(spin.upcast_ref());
        }
    }

    /// Sends the value picked in `dropdown`, one of `values`, read as the setting's type.
    fn connect_pick(&self, dropdown: &gtk::DropDown, values: &Rc<RefCell<Vec<String>>>) {
        let values = Rc::clone(values);
        dropdown.connect_selected_notify(glib::clone!(
            #[weak(rename_to = row)]
            self,
            move |dropdown| {
                let Some(setting) = row.setting() else {
                    return;
                };
                let selected = usize::try_from(dropdown.selected()).ok();
                let value = selected.and_then(|index| values.borrow().get(index).cloned());
                if let Some(Ok(value)) = value.map(|text| setting.kind().parse(&text)) {
                    row.send(value);
                }
            }
        ));
    }

    /// Replaces the choices of a row made [with choices](SettingRow::with_choices), each a value
    /// and its label, e.g. when the outputs or the sizes of the picked theme change, or for the
    /// modes of another output, and shows the value in effect again; a value that isn't among
    /// them is shown at the end. Nothing is sent.
    pub fn set_choices(&self, choices: &[(String, String)]) {
        let imp = self.imp();
        let Some(Control::Pick { labels, values, .. }) = imp.control.get() else {
            return;
        };
        let same = values
            .borrow()
            .iter()
            .map(String::as_str)
            .eq(choices.iter().map(|(value, _)| value.as_str()))
            && (0..labels.n_items())
                .filter_map(|index| labels.string(index))
                .map(|label| label.to_string())
                .eq(choices.iter().map(|(_, label)| label.clone()));
        if same {
            return;
        }
        imp.updating.set(true);
        let texts: Vec<&str> = choices.iter().map(|(_, label)| label.as_str()).collect();
        labels.splice(0, labels.n_items(), &texts);
        values.replace(choices.iter().map(|(value, _)| value.clone()).collect());
        imp.updating.set(false);
        self.refresh();
    }

    /// The drop-down of a row made [with choices](SettingRow::with_choices), e.g. to draw its
    /// items with a picture.
    pub fn dropdown(&self) -> Option<gtk::DropDown> {
        match self.imp().control.get() {
            Some(Control::Pick { dropdown, .. }) => Some(dropdown.clone()),
            _ => None,
        }
    }

    /// Opens GTK's font dialog at the font in effect and sends the font picked there.
    fn pick_font(&self, button: &gtk::Button) {
        let Some(label) = button.child().and_downcast::<gtk::Label>() else {
            return;
        };
        let current = gtk::pango::FontDescription::from_string(&label.text());
        let window = self.root().and_downcast::<gtk::Window>();
        let dialog = gtk::FontDialog::new();
        dialog.set_title(tr(self.setting().map_or(msg!("Font"), Setting::title)));
        if self.setting().and_then(Setting::font) == Some(Font::Monospace) {
            // The dialog lists families, or faces once one is picked.
            dialog.set_filter(Some(&gtk::CustomFilter::new(|item| {
                let family = match item.downcast_ref::<gtk::pango::FontFace>() {
                    Some(face) => face.family(),
                    None => match item.downcast_ref::<gtk::pango::FontFamily>() {
                        Some(family) => family.clone(),
                        None => return true,
                    },
                };
                family.is_monospace()
            })));
        }
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = row)]
            self,
            async move {
                // Cancelling the dialog is an error too; nothing changes then.
                if let Ok(font) = dialog
                    .choose_font_future(window.as_ref(), Some(&current))
                    .await
                {
                    row.send(Value::Text(font.to_str().to_string()));
                }
            }
        ));
    }

    /// Sends the text of `entry` if it differs from the value in effect.
    fn send_text(&self, entry: &gtk::Entry) {
        let imp = self.imp();
        let (Some(setting), Some(model)) = (self.setting(), imp.model.get()) else {
            return;
        };
        let text = entry.text().to_string();
        let device = imp.device.borrow().clone();
        let current = model
            .view(setting, device.as_deref())
            .map(|view| view.value);
        if current.as_ref() != Some(&Value::Text(text.clone())) {
            self.send(Value::Text(text));
        }
    }
}
