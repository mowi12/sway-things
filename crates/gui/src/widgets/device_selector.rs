// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The row that picks which input devices the per-device rows below it are for: all devices of
//! one type, or one of them. Devices that aren't connected but have stored values are listed
//! with "(not connected)" after their name, so those values stay reachable.
//!
//! Device names are external strings: they are only shown as plain text in the drop-down.

use gtk::glib;
use gtk::glib::closure_local;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::command::input::InputType;
use sway_things_core::devices::Devices;

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::sync::OnceLock;

    use gtk::glib;
    use gtk::glib::subclass::Signal;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::command::input::InputType;

    #[derive(Debug, Default)]
    pub struct DeviceSelector {
        pub(super) kind: OnceCell<InputType>,
        pub(super) title: gtk::Label,
        pub(super) dropdown: gtk::DropDown,
        pub(super) labels: gtk::StringList,
        /// The device of each entry of the drop-down after the first, which is all of them.
        pub(super) identifiers: RefCell<Vec<String>>,
        /// Set while the list is rebuilt, so that isn't reported as a choice.
        pub(super) updating: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DeviceSelector {
        const NAME: &'static str = "StDeviceSelector";
        type Type = super::DeviceSelector;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-device-selector");
            klass.set_layout_manager_type::<gtk::BoxLayout>();
        }
    }

    impl ObjectImpl for DeviceSelector {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                // The device picked: its identifier, or "" for all devices of the type.
                vec![
                    Signal::builder("device-changed")
                        .param_types([String::static_type()])
                        .build(),
                ]
            })
        }

        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for DeviceSelector {}
}

glib::wrapper! {
    /// Picks all input devices of one type, or one of them.
    pub struct DeviceSelector(ObjectSubclass<imp::DeviceSelector>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DeviceSelector {
    /// A selector for the devices of type `kind`, set to all of them.
    pub fn new(kind: InputType) -> Self {
        let selector: Self = glib::Object::new();
        let imp = selector.imp();
        let _ = imp.kind.set(kind);
        if let Some(layout) = selector.layout_manager().and_downcast::<gtk::BoxLayout>() {
            layout.set_spacing(24);
        }
        imp.title.set_text("Settings for");
        imp.title.set_xalign(0.0);
        imp.title.set_hexpand(true);
        imp.title.set_wrap(true);
        imp.title.add_css_class("st-row-title");
        imp.title.set_parent(&selector);

        imp.labels.append(&capitalized(&all_of_type(kind)));
        imp.dropdown.set_model(Some(&imp.labels));
        imp.dropdown.set_valign(gtk::Align::Center);
        imp.dropdown
            .update_property(&[gtk::accessible::Property::Label("Settings for")]);
        imp.dropdown.connect_selected_notify(glib::clone!(
            #[weak]
            selector,
            move |_| {
                if !selector.imp().updating.get() {
                    selector.emit_changed();
                }
            }
        ));
        imp.dropdown.set_parent(&selector);
        selector
    }

    /// The input type the selector is for.
    pub fn kind(&self) -> Option<InputType> {
        self.imp().kind.get().copied()
    }

    /// The drop-down.
    pub fn dropdown(&self) -> &gtk::DropDown {
        &self.imp().dropdown
    }

    /// What the drop-down offers, in order: all devices first.
    pub fn labels(&self) -> Vec<String> {
        let labels = &self.imp().labels;
        (0..labels.n_items())
            .filter_map(|index| labels.string(index))
            .map(String::from)
            .collect()
    }

    /// The device picked, or `None` for all devices of the type.
    pub fn selected_device(&self) -> Option<String> {
        let imp = self.imp();
        let index = usize::try_from(imp.dropdown.selected()).ok()?;
        imp.identifiers.borrow().get(index.checked_sub(1)?).cloned()
    }

    /// Picks the device with this identifier, or with `None` all devices of the type. A device
    /// that isn't listed picks all of them.
    pub fn select_device(&self, device: Option<&str>) {
        let imp = self.imp();
        let index = device
            .and_then(|device| {
                imp.identifiers
                    .borrow()
                    .iter()
                    .position(|known| known == device)
            })
            .map_or(0, |index| index + 1);
        if let Ok(index) = u32::try_from(index) {
            imp.dropdown.set_selected(index);
        }
    }

    /// Lists the devices of the type in `devices`. The device picked stays picked; if it is no
    /// longer listed, all devices are picked again and that is reported.
    pub fn update(&self, devices: &Devices) {
        let imp = self.imp();
        let Some(kind) = self.kind() else { return };
        let listed: Vec<(String, String)> = devices
            .of_type(kind)
            .map(|device| {
                let label = if device.is_connected() {
                    device.name().to_owned()
                } else {
                    format!("{} (not connected)", device.name())
                };
                (device.identifier().to_owned(), label)
            })
            .collect();
        let labels: Vec<String> = listed.iter().map(|(_, label)| label.clone()).collect();
        let same_devices = imp
            .identifiers
            .borrow()
            .iter()
            .eq(listed.iter().map(|(identifier, _)| identifier));
        if same_devices && self.labels().get(1..) == Some(&labels[..]) {
            return;
        }
        let selected = self.selected_device();
        imp.updating.set(true);
        imp.identifiers.replace(
            listed
                .into_iter()
                .map(|(identifier, _)| identifier)
                .collect(),
        );
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        imp.labels
            .splice(1, imp.labels.n_items().saturating_sub(1), &label_refs);
        self.select_device(selected.as_deref());
        imp.updating.set(false);
        if self.selected_device() != selected {
            self.emit_changed();
        }
    }

    fn emit_changed(&self) {
        let device = self.selected_device().unwrap_or_default();
        self.emit_by_name::<()>("device-changed", &[&device]);
    }

    /// Calls `callback` with the device picked, or `None` for all devices of the type.
    pub fn connect_device_changed<F: Fn(&Self, Option<&str>) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "device-changed",
            false,
            closure_local!(move |selector: &Self, device: String| {
                let device = (!device.is_empty()).then_some(device.as_str());
                callback(selector, device);
            }),
        )
    }
}

/// "all mice", "all touchpads", …: every device of type `kind`.
pub(crate) fn all_of_type(kind: InputType) -> String {
    format!("all {}", plural(kind))
}

/// What a page says in place of the rows of type `kind` while none of its devices is connected
/// and nothing is stored for them.
pub(crate) fn none_connected(kind: InputType) -> String {
    format!(
        "No {} is connected. Its settings show up once one is.",
        singular(kind)
    )
}

fn singular(kind: InputType) -> &'static str {
    match kind {
        InputType::Pointer => "mouse",
        InputType::Touchpad => "touchpad",
        InputType::Keyboard => "keyboard",
        InputType::Touch => "touchscreen",
        InputType::TabletTool => "tablet pen",
        InputType::TabletPad => "tablet pad",
        InputType::Switch => "switch",
    }
}

fn plural(kind: InputType) -> &'static str {
    match kind {
        InputType::Pointer => "mice",
        InputType::Touchpad => "touchpads",
        InputType::Keyboard => "keyboards",
        InputType::Touch => "touchscreens",
        InputType::TabletTool => "tablet pens",
        InputType::TabletPad => "tablet pads",
        InputType::Switch => "switches",
    }
}

/// `text` with its first letter in upper case.
pub(crate) fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use sway_things_core::command::input::InputType;

    use super::{all_of_type, capitalized, none_connected};

    #[test]
    fn types_are_named_for_people() {
        assert_eq!(capitalized(&all_of_type(InputType::Pointer)), "All mice");
        assert_eq!(all_of_type(InputType::Touchpad), "all touchpads");
        assert_eq!(
            none_connected(InputType::Touchpad),
            "No touchpad is connected. Its settings show up once one is."
        );
    }
}
