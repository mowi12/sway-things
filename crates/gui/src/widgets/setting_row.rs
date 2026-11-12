// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A row for one setting, built from its registry entry: title, subtitle, the line of the sway
//! config in monospace, and the control for its value type.
//!
//! | Value type | Control |
//! | --- | --- |
//! | on/off | switch |
//! | whole or decimal number | slider over the setting's slider range, and a number field for every value it accepts |
//! | choice | drop-down |
//! | color | color button |
//! | text | entry, applied on Enter or when it loses the focus |
//!
//! A change goes to the [`Model`] at once; the row shows the value in effect again when the model
//! reports a change, and the error under the title when the change failed. A locked setting (no
//! include line, a compositor too old) shows why and can't be changed.
//!
//! The row of a per-device setting is for all devices of its type or, after
//! [`SettingRow::set_device`], for one of them. Until that device has a value of its own, the
//! row says it is the same as for all devices of the type.

use std::str::FromStr;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::command::Color;
use sway_things_core::registry::{Kind, Scope, Setting, Value};
use sway_things_core::view::{Locked, SettingView, Source};

use super::device_selector::all_of_type;
use crate::i18n::tr;
use crate::model::Model;
use crate::style;

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
}

impl Control {
    /// The widgets of the control, in order.
    fn widgets(&self) -> Vec<gtk::Widget> {
        match self {
            Self::Toggle(switch) => vec![switch.clone().upcast()],
            Self::Number { scale, spin, .. } => vec![spin.clone().upcast(), scale.clone().upcast()],
            Self::Choice { dropdown, .. } => vec![dropdown.clone().upcast()],
            Self::Color(button) => vec![button.clone().upcast()],
            Self::Text(entry) => vec![entry.clone().upcast()],
        }
    }
}

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::registry::Setting;

    use crate::model::Model;

    #[derive(Debug, Default)]
    pub struct SettingRow {
        pub(super) setting: OnceCell<&'static Setting>,
        pub(super) device: RefCell<Option<String>>,
        pub(super) model: OnceCell<Model>,
        pub(super) control: OnceCell<super::Control>,
        /// Set while the row writes a value into its control, so that isn't sent as a change.
        pub(super) updating: Cell<bool>,
        pub(super) title: gtk::Label,
        pub(super) subtitle: gtk::Label,
        pub(super) line: gtk::Label,
        pub(super) note: gtk::Label,
        pub(super) error: gtk::Label,
        pub(super) controls: gtk::Box,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SettingRow {
        const NAME: &'static str = "StSettingRow";
        type Type = super::SettingRow;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-setting-row");
            klass.set_layout_manager_type::<gtk::BoxLayout>();
        }
    }

    impl ObjectImpl for SettingRow {
        fn dispose(&self) {
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
    /// `None` for a value type that has no row (lists get their own widget).
    pub fn new(model: &Model, setting: &'static Setting, device: Option<&str>) -> Option<Self> {
        let control = control(setting)?;
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
        texts.set_parent(self);
        imp.controls.set_parent(self);
        if let Some(layout) = self.layout_manager().and_downcast::<gtk::BoxLayout>() {
            layout.set_spacing(24);
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
        self.imp()
            .control
            .get()
            .map(Control::widgets)
            .unwrap_or_default()
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
        imp.device.replace(device.map(str::to_owned));
        self.show_error(None);
        self.refresh();
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

    /// Sends `value` as the new value of the setting.
    fn send(&self, value: Value) {
        let imp = self.imp();
        if imp.updating.get() {
            return;
        }
        let (Some(setting), Some(model)) = (self.setting(), imp.model.get()) else {
            return;
        };
        self.show_error(None);
        model.set(setting.key(), imp.device.borrow().as_deref(), value);
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
        if !model.is_pending(setting.key(), device.as_deref()) {
            imp.updating.set(true);
            show_value(control, setting, &view.value);
            imp.updating.set(false);
        }
        imp.line.set_text(view.line.as_deref().unwrap_or_default());
        imp.line.set_visible(view.line.is_some());
        let note = note(&view, setting, device.as_deref());
        imp.note.set_text(&note);
        imp.note.set_visible(!note.is_empty());
        let reason = view.locked.as_ref().map(ToString::to_string);
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
            Control::Number { scale, spin, whole } => {
                let whole = *whole;
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
                        row.send(number(value, whole));
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
                        row.send(number(value, whole));
                    }
                ));
            }
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
        }
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

/// The control for the value type of `setting`, or `None` if it has none.
fn control(setting: &'static Setting) -> Option<Control> {
    Some(match setting.kind() {
        Kind::Bool { .. } => {
            let switch = gtk::Switch::new();
            switch.set_valign(gtk::Align::Center);
            Control::Toggle(switch)
        }
        Kind::Integer { min, max, .. } => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "the ranges of whole-number settings are far below 2^52"
            )]
            let range = (*min as f64, *max as f64);
            number_control(setting, range, true)?
        }
        Kind::Decimal { min, max, .. } => number_control(setting, (*min, *max), false)?,
        Kind::Choice { choices, .. } => {
            let labels: Vec<&str> = choices.iter().map(|choice| tr(choice.label)).collect();
            let dropdown = gtk::DropDown::from_strings(&labels);
            dropdown.set_valign(gtk::Align::Center);
            Control::Choice {
                dropdown,
                ids: choices.iter().map(|choice| choice.id).collect(),
            }
        }
        Kind::Color { .. } => {
            let dialog = gtk::ColorDialog::new();
            dialog.set_with_alpha(true);
            let button = gtk::ColorDialogButton::new(Some(dialog));
            button.set_valign(gtk::Align::Center);
            Control::Color(button)
        }
        Kind::Text { .. } => {
            let entry = gtk::Entry::new();
            entry.set_width_chars(20);
            entry.set_valign(gtk::Align::Center);
            Control::Text(entry)
        }
        Kind::List { .. } => return None,
    })
}

/// A slider over the slider range and a number field for every value the setting accepts.
fn number_control(setting: &Setting, (min, max): (f64, f64), whole: bool) -> Option<Control> {
    let slider = setting.slider()?;
    let scale = gtk::Scale::with_range(
        gtk::Orientation::Horizontal,
        slider.min,
        slider.max,
        slider.step,
    );
    scale.set_draw_value(false);
    scale.set_size_request(200, -1);
    scale.set_valign(gtk::Align::Center);
    let spin = gtk::SpinButton::with_range(min, max, slider.step);
    spin.set_digits(digits(slider.step, whole));
    spin.set_width_chars(5);
    spin.set_valign(gtk::Align::Center);
    Some(Control::Number { scale, spin, whole })
}

/// The decimal places for numbers in steps of `step`.
fn digits(step: f64, whole: bool) -> u32 {
    if whole {
        return 0;
    }
    (0..4)
        .find(|places| {
            let scaled = step * 10f64.powi(*places);
            (scaled - scaled.round()).abs() < 1e-9
        })
        .and_then(|places| u32::try_from(places).ok())
        .unwrap_or(4)
}

/// The number value of a control position.
fn number(value: f64, whole: bool) -> Value {
    if whole {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the control's range is that of the setting, far within i64"
        )]
        Value::Integer(value.round() as i64)
    } else {
        // The slider moves in binary fractions; the stored value is what the field shows.
        Value::Decimal((value * 1e4).round() / 1e4)
    }
}

/// Puts `value` into the control.
fn show_value(control: &Control, setting: &Setting, value: &Value) {
    match (control, value) {
        (Control::Toggle(switch), Value::Bool(on)) => {
            switch.set_active(*on);
            switch.set_state(*on);
        }
        (Control::Number { scale, spin, .. }, Value::Integer(number)) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "the ranges of whole-number settings are far below 2^52"
            )]
            let number = *number as f64;
            spin.set_value(number);
            scale.set_value(number);
        }
        (Control::Number { scale, spin, .. }, Value::Decimal(number)) => {
            spin.set_value(*number);
            scale.set_value(*number);
        }
        (Control::Choice { dropdown, ids }, Value::Choice(id)) => {
            if let Some(index) = ids.iter().position(|known| known == id) {
                if let Ok(index) = u32::try_from(index) {
                    dropdown.set_selected(index);
                }
            }
        }
        (Control::Color(button), Value::Color(color)) => {
            if let Ok(rgba) = gtk::gdk::RGBA::parse(color.to_string()) {
                button.set_rgba(&rgba);
            }
        }
        (Control::Text(entry), Value::Text(text)) => {
            if entry.text() != *text {
                entry.set_text(text);
            }
        }
        _ => glib::g_warning!(
            "sway-things",
            "{}: value {value:?} doesn't fit its row",
            setting.key()
        ),
    }
}

/// The color of `rgba`, with an alpha only if it isn't opaque.
fn color(rgba: &gtk::gdk::RGBA) -> Option<Color> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the channel is clamped to 0 ..= 255 first"
    )]
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (red, green, blue, alpha) = (
        channel(rgba.red()),
        channel(rgba.green()),
        channel(rgba.blue()),
        channel(rgba.alpha()),
    );
    let text = if alpha == 0xff {
        format!("#{red:02x}{green:02x}{blue:02x}")
    } else {
        format!("#{red:02x}{green:02x}{blue:02x}{alpha:02x}")
    };
    Color::from_str(&text).ok()
}

/// The note under the line: where the value comes from, or why it can't be changed. For one
/// `device` without a value of its own, that it is the value of all devices of the type.
fn note(view: &SettingView, setting: &Setting, device: Option<&str>) -> String {
    let mut parts = Vec::new();
    match (&view.source, setting.scope(), device) {
        (Source::Config { file, line }, _, _) => {
            parts.push(format!("From {}:{line}", tilde(file)));
        }
        (
            Source::Stored { for_type: true, .. } | Source::Default,
            Scope::PerDevice(kind),
            Some(_),
        ) => {
            parts.push(format!("Same as {}", all_of_type(kind)));
        }
        _ => {}
    }
    match &view.locked {
        Some(locked @ Locked::Unsupported(_)) => parts.push(locked.to_string()),
        // The include line is explained once for the whole window, not on every row.
        Some(Locked::ReadOnly(_)) | None => {}
    }
    parts.join(" · ")
}

/// `path` with the home directory written as `~`.
pub(crate) fn tilde(path: &std::path::Path) -> String {
    let home = glib::home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{digits, number};
    use sway_things_core::registry::Value;

    #[test]
    fn decimal_places_follow_the_step() {
        assert_eq!(digits(1.0, true), 0);
        assert_eq!(digits(0.05, false), 2);
        assert_eq!(digits(0.5, false), 1);
        assert_eq!(digits(0.02, false), 2);
        assert_eq!(digits(1.0, false), 0);
    }

    #[test]
    fn control_positions_become_values() {
        assert_eq!(number(599.6, true), Value::Integer(600));
        assert_eq!(number(0.300_000_000_4, false), Value::Decimal(0.3));
    }
}
