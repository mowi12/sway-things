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

use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::command::Color;
use sway_things_core::conflict::{Report, UserLine};
use sway_things_core::registry::{Apply, Kind, Registry, Scope, Setting, Value};
use sway_things_core::state::Layer;
use sway_things_core::view::{Locked, SettingView, Source};

use super::device_selector::all_of_type;
use crate::i18n::tr;
use crate::model::Model;
use crate::style;

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
    use sway_things_core::registry::Setting;

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
        /// Set for a member of a group, whose row shows the line of the sway config instead.
        pub(super) grouped: Cell<bool>,
        pub(super) title: gtk::Label,
        pub(super) subtitle: gtk::Label,
        pub(super) line: gtk::Label,
        pub(super) note: gtk::Label,
        pub(super) error: gtk::Label,
        pub(super) badges: gtk::Box,
        pub(super) controls: gtk::Box,
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
            klass.set_layout_manager_type::<gtk::BoxLayout>();
        }
    }

    impl ObjectImpl for SettingRow {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![Signal::builder("device-changed").build()])
        }

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
    /// `None` for a number setting without a slider. A list gets a row without a control: the
    /// page shows its own widget for it.
    pub fn new(model: &Model, setting: &'static Setting, device: Option<&str>) -> Option<Self> {
        Self::with_control(model, setting, device, control(setting)?)
    }

    /// The row of the text setting `setting` with a drop-down of known `choices` (each a value
    /// and its label) in place of an entry. A value that isn't among them is shown at the end.
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

    /// The reset button, the "This machine only" toggle and the ⋮ menu of a conflict, after the
    /// control.
    ///
    /// They keep their room in the row while they are hidden (shown or hidden with
    /// `set_child_visible`), so the control never moves when they appear: a slider being dragged
    /// would otherwise shrink under the pointer once the first change stores a value. The ⋮ menu
    /// keeps its room only in rows whose setting the user's config sets too, the only rows where
    /// it can appear.
    fn build_indicators(&self, title: &str) {
        let imp = self.imp();
        imp.reset.set_icon_name("edit-undo-symbolic");
        imp.reset
            .update_property(&[gtk::accessible::Property::Label(&format!("Reset {title}"))]);
        imp.reset.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            self,
            move |_| row.reset()
        ));
        imp.machine.set_icon_name("computer-symbolic");
        imp.machine.set_tooltip_text(Some("This machine only"));
        imp.machine
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "{title}: this machine only"
            ))]);
        imp.machine.connect_toggled(glib::clone!(
            #[weak(rename_to = row)]
            self,
            move |toggle| row.move_to(toggle.is_active())
        ));
        imp.conflict.set_icon_name("view-more-symbolic");
        imp.conflict
            .set_tooltip_text(Some("Your config sets this too"));
        imp.conflict
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "{title}: your config sets this too"
            ))]);
        imp.conflict.set_popover(Some(&self.conflict_popover()));
        for button in [
            imp.reset.upcast_ref::<gtk::Widget>(),
            imp.machine.upcast_ref(),
            imp.conflict.upcast_ref(),
        ] {
            button.set_valign(gtk::Align::Center);
            button.set_child_visible(false);
            imp.controls.append(button);
        }
    }

    /// The menu behind ⋮: the line of the user's config, then what to do about it.
    fn conflict_popover(&self) -> gtk::Popover {
        let imp = self.imp();
        let details = gtk::Box::new(gtk::Orientation::Vertical, 4);
        details.add_css_class("st-conflict-details");
        imp.conflict_title.add_css_class("st-row-title");
        imp.conflict_place.add_css_class("monospace");
        imp.conflict_place.add_css_class(style::DIM);
        imp.conflict_line.add_css_class("monospace");
        imp.conflict_line.set_selectable(true);
        for label in [
            &imp.conflict_title,
            &imp.conflict_place,
            &imp.conflict_line,
            &imp.conflict_text,
        ] {
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_max_width_chars(48);
            details.append(label);
        }
        let popover = gtk::Popover::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&details);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        imp.let_config_win.set_label("Let my config win");
        let open = gtk::Button::with_label("Open config in editor");
        let copy = gtk::Button::with_label("Copy line");
        for button in [&imp.let_config_win, &open, &copy] {
            button.add_css_class("flat");
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_xalign(0.0);
            }
            content.append(button);
        }
        imp.let_config_win.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                row.let_config_win();
            }
        ));
        open.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                row.open_config();
            }
        ));
        copy.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                let line = row.imp().conflict_line.text();
                row.clipboard().set_text(&line);
            }
        ));
        popover.set_child(Some(&content));
        popover
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

    /// Opens the file with the line of the user's config in the desktop's text editor.
    fn open_config(&self) {
        let Some(file) = self
            .imp()
            .report
            .borrow()
            .as_ref()
            .and_then(conflict_line)
            .map(|(_, line)| line.file().to_owned())
        else {
            return;
        };
        let launcher = gtk::FileLauncher::new(Some(&gtk::gio::File::for_path(&file)));
        let window = self.root().and_downcast::<gtk::Window>();
        launcher.launch(
            window.as_ref(),
            gtk::gio::Cancellable::NONE,
            glib::clone!(
                #[weak(rename_to = row)]
                self,
                move |result| {
                    if let Err(error) = result {
                        row.show_error(Some(&format!("The config can't be opened: {error}")));
                    }
                }
            ),
        );
    }

    /// Shows the reset button, the toggle, the badges and the ⋮ menu as `view` and the stored
    /// value ask.
    fn refresh_indicators(
        &self,
        setting: &'static Setting,
        device: Option<&str>,
        view: &SettingView,
    ) {
        let imp = self.imp();
        let Some(model) = imp.model.get() else { return };
        let layer = model.stored_in(setting, device);
        let read_only = matches!(view.locked, Some(Locked::ReadOnly(_)));
        imp.reset.set_child_visible(layer.is_some());
        imp.reset.set_sensitive(!read_only);
        imp.reset
            .set_tooltip_text(Some(&match (setting.scope(), device) {
                (Scope::PerDevice(kind), Some(_)) => {
                    format!("Use the value of {}", all_of_type(kind))
                }
                _ => "Reset to default".to_owned(),
            }));
        imp.machine.set_child_visible(layer.is_some());
        imp.machine.set_sensitive(!read_only);
        imp.updating.set(true);
        imp.machine.set_active(layer == Some(Layer::Host));
        imp.updating.set(false);

        while let Some(badge) = imp.badges.first_child() {
            imp.badges.remove(&badge);
        }
        let users = model.report(setting, device);
        // Room for the ⋮ menu wherever it could appear once a value is stored.
        imp.conflict
            .set_visible(users.as_ref().and_then(conflict_line).is_some());
        let report = users.filter(|_| layer.is_some());
        let found = report.as_ref().and_then(conflict_line);
        match &found {
            Some((Conflict::Wins, line)) => {
                imp.badges
                    .append(&badge("dialog-warning-symbolic", "Your config wins", true));
                imp.conflict_title.set_text("Your config wins");
                imp.conflict_text.set_text(
                    "This line comes after the include line, so it decides after a reload.",
                );
                self.show_conflict(line);
            }
            Some((Conflict::Overridden, line)) => {
                imp.badges
                    .append(&badge("action-unavailable-symbolic", "Overridden", false));
                imp.conflict_title.set_text("Overridden in your config");
                imp.conflict_text.set_text(
                    "Sway Things\u{2019} value comes later and wins; this line is no longer in effect.",
                );
                self.show_conflict(line);
            }
            None => {}
        }
        if let Some(locked @ Locked::Unsupported(_)) = &view.locked {
            imp.badges.append(&badge(
                "changes-prevent-symbolic",
                &locked.to_string(),
                false,
            ));
        }
        match setting.apply() {
            Apply::Reload => imp.badges.append(&badge(
                "view-refresh-symbolic",
                "Applies after reload",
                false,
            )),
            Apply::Relogin => imp.badges.append(&badge(
                "system-log-out-symbolic",
                "Applies after re-login",
                false,
            )),
            Apply::Live | Apply::ToolReload | Apply::Risky => {}
        }
        imp.conflict.set_child_visible(found.is_some());
        imp.conflict.set_sensitive(!read_only);
        // The user's config can't win over a setting that can lock the user out: the safety
        // net only resets it.
        imp.let_config_win
            .set_visible(setting.apply() != Apply::Risky);
        imp.report
            .replace(found.is_some().then_some(report).flatten());
        imp.badges.set_visible(imp.badges.first_child().is_some());
    }

    fn show_conflict(&self, line: &UserLine) {
        let imp = self.imp();
        imp.conflict_place
            .set_text(&format!("{}:{}", tilde(line.file()), line.line()));
        imp.conflict_line.set_text(line.text());
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
        self.emit_by_name::<()>("device-changed", &[]);
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
            .and_then(|view| view.locked.map(|locked| locked.to_string()))
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
        imp.line
            .set_visible(view.line.is_some() && !imp.grouped.get() && shows_directives(model));
        self.refresh_indicators(setting, device.as_deref(), &view);
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
            Control::Pick {
                dropdown, values, ..
            } => self.connect_pick(dropdown, values),
            Control::None => {}
        }
    }

    /// Sends the value picked in `dropdown`, one of `values`.
    fn connect_pick(&self, dropdown: &gtk::DropDown, values: &Rc<RefCell<Vec<String>>>) {
        let values = Rc::clone(values);
        dropdown.connect_selected_notify(glib::clone!(
            #[weak(rename_to = row)]
            self,
            move |dropdown| {
                let selected = usize::try_from(dropdown.selected()).ok();
                let value = selected.and_then(|index| values.borrow().get(index).cloned());
                if let Some(value) = value {
                    row.send(Value::Text(value));
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
        Kind::List { .. } => Control::None,
    })
}

/// The share of a slider's range around the default in which dragging locks to the default.
const SNAP_ZONE: f64 = 0.02;

/// Whether a slider dragged to `value` snaps to `default`: while `value` is within [`SNAP_ZONE`]
/// of the range `min` to `max` from it. Then the result is `default`; `None` leaves the slider at
/// `value`. A default outside the range never snaps.
fn snap(value: f64, default: f64, (min, max): (f64, f64)) -> Option<f64> {
    let zone = (max - min).abs() * SNAP_ZONE;
    let in_range = (min.min(max)..=min.max(max)).contains(&default);
    (in_range && (value - default).abs() <= zone).then_some(default)
}

/// A slider over the slider range and a number field for every value the setting accepts. The
/// slider has a mark at the setting's default and snaps to it while it is dragged; keyboard steps
/// and the number field stay exact.
fn number_control(setting: &Setting, (min, max): (f64, f64), whole: bool) -> Option<Control> {
    let slider = setting.slider()?;
    let scale = gtk::Scale::with_range(
        gtk::Orientation::Horizontal,
        slider.min,
        slider.max,
        slider.step,
    );
    #[expect(
        clippy::cast_precision_loss,
        reason = "the ranges of whole-number settings are far below 2^52"
    )]
    let default = match setting.default_value() {
        Value::Integer(number) => Some(number as f64),
        Value::Decimal(number) => Some(number),
        _ => None,
    };
    let range = (slider.min, slider.max);
    if let Some(default) = default.filter(|default| (slider.min..=slider.max).contains(default)) {
        scale.add_mark(default, gtk::PositionType::Bottom, None);
        // A pointer drag or click moves the slider with `Jump`; keys and the wheel step.
        scale.connect_change_value(move |scale, scroll, value| {
            if scroll != gtk::ScrollType::Jump {
                return glib::Propagation::Proceed;
            }
            match snap(value, default, range) {
                Some(snapped) => {
                    scale.set_value(snapped);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
    }
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
        (
            Control::Pick {
                dropdown,
                labels,
                values,
            },
            Value::Text(text),
        ) => {
            let known = values.borrow().iter().position(|value| value == text);
            let index = known.unwrap_or_else(|| {
                // A value typed by hand: shown by its name.
                labels.append(text);
                values.borrow_mut().push(text.clone());
                values.borrow().len() - 1
            });
            if let Ok(index) = u32::try_from(index) {
                dropdown.set_selected(index);
            }
        }
        (Control::None, _) => {}
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

/// Whether rows show the line of the sway config, as the preference `app.show_directives` says.
pub(super) fn shows_directives(model: &Model) -> bool {
    Registry::builtin()
        .get(DIRECTIVES_KEY)
        .and_then(|setting| model.view(setting, None))
        .is_some_and(|view| view.value == Value::Bool(true))
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
    // The setting this one needs ("Turn on Blur first"). What the compositor lacks is a badge,
    // and the include line is explained once for the whole window, not on every row.
    if let Some(Locked::Needs(text)) = &view.locked {
        parts.push(text.clone());
    }
    parts.join(" · ")
}

/// How a line of the user's config conflicts with the stored value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Conflict {
    /// It comes after the generated files and decides.
    Wins,
    /// It comes before them and is no longer in effect.
    Overridden,
}

/// The line of the user's config that conflicts with a stored value in `report`: the one that
/// wins over it, else the last one it made dead.
fn conflict_line(report: &Report) -> Option<(Conflict, &UserLine)> {
    report
        .overriding()
        .map(|line| (Conflict::Wins, line))
        .or_else(|| {
            report
                .overridden()
                .last()
                .map(|line| (Conflict::Overridden, line))
        })
}

/// A badge under the line: an icon and a text. One that needs attention is marked as such.
fn badge(icon: &str, text: &str, attention: bool) -> gtk::Box {
    let badge = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    badge.add_css_class("st-badge");
    if attention {
        badge.add_css_class("st-attention");
    }
    // The icon at the size of the text; a symbolic icon is drawn sharp at whole pixels.
    let image = gtk::Image::builder()
        .icon_name(icon)
        .pixel_size(12)
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    badge.append(&image);
    badge.append(&gtk::Label::new(Some(text)));
    badge
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
    use super::{digits, number, snap};
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
    fn a_dragged_slider_snaps_to_the_default_within_two_percent() {
        // A range of 2 (-1 to 1): the zone is 0.04 on each side of the default.
        let range = (-1.0, 1.0);
        assert_eq!(snap(0.04, 0.0, range), Some(0.0));
        assert_eq!(snap(-0.04, 0.0, range), Some(0.0));
        assert_eq!(snap(0.02, 0.0, range), Some(0.0));
        assert_eq!(snap(0.0401, 0.0, range), None);
        assert_eq!(snap(-0.05, 0.0, range), None);
        // Negative values only.
        assert_eq!(snap(-49.5, -50.0, (-100.0, 0.0)), Some(-50.0));
        assert_eq!(snap(-47.9, -50.0, (-100.0, 0.0)), None);
        // A default at either end snaps from inside the range.
        assert_eq!(snap(1.5, 0.0, (0.0, 100.0)), Some(0.0));
        assert_eq!(snap(2.5, 0.0, (0.0, 100.0)), None);
        assert_eq!(snap(98.0, 100.0, (0.0, 100.0)), Some(100.0));
        assert_eq!(snap(97.9, 100.0, (0.0, 100.0)), None);
        // A default outside the range never snaps.
        assert_eq!(snap(10.5, 5.0, (10.0, 4000.0)), None);
    }

    #[test]
    fn control_positions_become_values() {
        assert_eq!(number(599.6, true), Value::Integer(600));
        assert_eq!(number(0.300_000_000_4, false), Value::Decimal(0.3));
    }
}
