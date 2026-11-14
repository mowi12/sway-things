// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Bluetooth page: the adapter's power, the paired devices and the ones around, live from
//! BlueZ.
//!
//! The page reads a new snapshot whenever BlueZ reports a change, at most every [`THROTTLE`], and
//! updates its rows in place. While it is on screen and the radio is on, it looks for devices.
//! _Connect_ on a device that isn't paired pairs it first; the pairing agent's questions come up
//! as dialogs. A soft-blocked radio is unblocked through rfkill when switched on.
//!
//! Device names are untrusted and only ever set as plain label text.

mod dialogs;

use std::time::Duration;

use futures_lite::StreamExt;
use futures_lite::future::poll_once;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib};
use sway_things_core::bluetooth::rfkill::{self, Rfkill};
use sway_things_core::bluetooth::{
    AgentEvent, Answer, Bluez, Device, Error, Prompt, Request, Snapshot, check_alias,
    parse_passkey, valid_pin,
};
use sway_things_core::delegate;

use self::dialogs::{Dialog, Field, Spec};
use super::Placeholder;
use crate::style;

/// How long to collect a burst of changes before reading the snapshot again.
const THROTTLE: Duration = Duration::from_millis(200);
/// How long to give the kernel after lifting the soft block before powering the adapter.
const UNBLOCK_SETTLE: Duration = Duration::from_millis(500);
/// The command that lifts the soft block, for users without access to `/dev/rfkill`.
const UNBLOCK_COMMAND: &str = "rfkill unblock bluetooth";

/// Where the page finds BlueZ.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Bus {
    /// The system bus, where BlueZ runs.
    #[default]
    System,
    /// Another bus, with a stand-in for BlueZ on it (tests), and without rfkill.
    Address(String),
    /// No bus: the page shows Bluetooth as unavailable (windows in tests that don't need it).
    Off,
}

/// What runs on a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Pair,
    Connect,
    Disconnect,
    Forget,
    Rename,
}

impl Action {
    /// What the device's status and button show while it runs.
    const fn running(self) -> &'static str {
        match self {
            Self::Pair => "Pairing…",
            Self::Connect => "Connecting…",
            Self::Disconnect => "Disconnecting…",
            Self::Forget => "Forgetting…",
            Self::Rename => "Renaming…",
        }
    }

    /// The start of the message when it fails.
    const fn failure(self) -> &'static str {
        match self {
            Self::Pair => "Couldn't pair with",
            Self::Connect => "Couldn't connect to",
            Self::Disconnect => "Couldn't disconnect from",
            Self::Forget => "Couldn't forget",
            Self::Rename => "Couldn't rename",
        }
    }
}

/// The widgets of a device's row, kept across refreshes so focus and state stay.
#[derive(Debug, Clone)]
struct DeviceRow {
    row: gtk::ListBoxRow,
    icon: gtk::Image,
    name: gtk::Label,
    status: gtk::Label,
    button: gtk::Button,
    menu: gtk::MenuButton,
    /// Whether the row is in the list of paired devices.
    paired: bool,
}

mod imp {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::bluetooth::Bluez;
    use sway_things_core::bluetooth::Snapshot;
    use sway_things_core::bluetooth::rfkill::Block;

    use super::dialogs::Dialog;
    use super::{Action, DeviceRow};

    #[derive(Default)]
    pub struct BluetoothPage {
        /// The only child: holds the placeholder and the controls.
        pub content: gtk::Box,
        /// Shown instead of the controls while Bluetooth isn't available.
        pub missing: gtk::Box,
        /// The sections with the adapter and the devices.
        pub controls: gtk::Box,
        /// The adapter drop-down's row, shown with two or more adapters.
        pub adapter_row: gtk::ListBoxRow,
        pub adapters: gtk::DropDown,
        pub adapter_names: gtk::StringList,
        pub power: gtk::Switch,
        pub power_subtitle: gtk::Label,
        /// Why the soft block couldn't be lifted, with the command to do it.
        pub unblock_help: gtk::Box,
        /// The error of the last action that failed.
        pub error: gtk::Label,
        pub mine: gtk::ListBox,
        pub mine_hint: gtk::Label,
        pub others_heading: gtk::Box,
        pub searching: gtk::Box,
        /// Where the spinner is: it is replaced each time searching shows, since a spinner's
        /// animation stops when it leaves the screen and doesn't start again by itself.
        pub spinner_slot: gtk::Box,
        pub spinner: RefCell<Option<gtk::Spinner>>,
        pub others: gtk::ListBox,
        pub others_hint: gtk::Label,
        /// The "Advanced…" button for the page header, if blueman is installed.
        pub advanced: RefCell<Option<gtk::Button>>,

        pub bluez: RefCell<Option<Bluez>>,
        /// What the page shows.
        pub snapshot: RefCell<Option<Snapshot>>,
        /// The path of the adapter the page shows.
        pub selected: RefCell<Option<String>>,
        /// The rfkill block of the Bluetooth radios; `None` without rfkill.
        pub block: Cell<Option<Block>>,
        /// The adapter the page asked to look for devices.
        pub discovery: RefCell<Option<String>>,
        /// The rows by device path.
        pub(super) rows: RefCell<HashMap<String, DeviceRow>>,
        /// What runs on which device.
        pub(super) running: RefCell<HashMap<String, Action>>,
        /// The open dialog of the pairing agent, and the device it is about.
        pub dialog: RefCell<Option<(String, Dialog)>>,
        /// Follow BlueZ, rfkill and the agent for as long as the page lives.
        pub tasks: RefCell<Vec<glib::JoinHandle<()>>>,
        /// Whether the adapter drop-down is being filled (its signal isn't a choice then).
        pub filling: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BluetoothPage {
        const NAME: &'static str = "StBluetoothPage";
        type Type = super::BluetoothPage;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-bluetooth");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for BluetoothPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.content.set_parent(&*self.obj());
            self.obj().build();
        }

        fn dispose(&self) {
            for task in self.tasks.take() {
                task.abort();
            }
            if let Some((_, dialog)) = self.dialog.take() {
                dialog.close();
            }
            self.obj().stop_discovery();
            self.content.unparent();
        }
    }

    impl WidgetImpl for BluetoothPage {
        fn map(&self) {
            self.parent_map();
            self.obj().new_spinner();
            self.obj().update_discovery();
        }

        fn unmap(&self) {
            self.parent_unmap();
            self.obj().update_discovery();
        }
    }
}

glib::wrapper! {
    /// The adapter's power switch and the devices, live from BlueZ.
    pub struct BluetoothPage(ObjectSubclass<imp::BluetoothPage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl BluetoothPage {
    /// The page on `bus`, which connects to BlueZ right away and follows it.
    pub fn new(bus: &Bus) -> Self {
        let page: Self = glib::Object::new();
        page.follow(bus.clone());
        if *bus == Bus::System {
            page.follow_rfkill();
        }
        page
    }

    /// The "Advanced…" button for the page header: it starts blueman-manager. `None` if blueman
    /// isn't installed.
    pub fn advanced_button(&self) -> Option<gtk::Button> {
        let imp = self.imp();
        if imp.advanced.borrow().is_none() && delegate::BLUEMAN.find().is_some() {
            let button = gtk::Button::with_label("Advanced…");
            button.set_tooltip_text(Some("Open Blueman for adapter settings and more"));
            let weak = self.downgrade();
            button.connect_clicked(move |_| {
                if let Some(page) = weak.upgrade() {
                    page.open_blueman();
                }
            });
            imp.advanced.replace(Some(button));
        }
        imp.advanced.borrow().clone()
    }

    /// Our name on the bus, which the agent's questions go to; for tests.
    pub fn bus_name(&self) -> Option<String> {
        self.imp()
            .bluez
            .borrow()
            .as_ref()
            .and_then(Bluez::unique_name)
    }

    /// The adapter drop-down, for tests.
    pub fn adapter_choice(&self) -> &gtk::DropDown {
        &self.imp().adapters
    }

    /// The power switch, for tests.
    pub fn power_switch(&self) -> &gtk::Switch {
        &self.imp().power
    }

    /// The name, status and button label of each row of the paired devices (`mine`) or the
    /// others, in order; for tests.
    pub fn device_rows(&self, mine: bool) -> Vec<(String, String, String)> {
        let imp = self.imp();
        let list = if mine { &imp.mine } else { &imp.others };
        let rows = imp.rows.borrow();
        let mut out = Vec::new();
        let mut child = list.first_child();
        while let Some(widget) = child {
            if let Some(row) = rows
                .values()
                .find(|row| row.row.upcast_ref::<gtk::Widget>() == &widget)
            {
                out.push((
                    row.name.text().to_string(),
                    row.status.text().to_string(),
                    row.button
                        .label()
                        .map(|label| label.to_string())
                        .unwrap_or_default(),
                ));
            }
            child = widget.next_sibling();
        }
        out
    }

    /// The action button of the device named `name`, for tests.
    pub fn device_button(&self, name: &str) -> Option<gtk::Button> {
        self.imp()
            .rows
            .borrow()
            .values()
            .find(|row| row.name.text() == name)
            .map(|row| row.button.clone())
    }

    /// The window of the open pairing dialog, for tests.
    pub fn pairing_dialog(&self) -> Option<gtk::Window> {
        self.imp()
            .dialog
            .borrow()
            .as_ref()
            .map(|(_, dialog)| dialog.window().clone())
    }

    /// The text of the error under the controls; empty without one. For tests.
    pub fn error_text(&self) -> String {
        let error = &self.imp().error;
        if error.is_visible() {
            error.text().to_string()
        } else {
            String::new()
        }
    }

    /// Lays out the widgets.
    fn build(&self) {
        let imp = self.imp();
        imp.content.set_orientation(gtk::Orientation::Vertical);
        imp.content.append(&imp.missing);
        imp.content.append(&imp.controls);
        imp.missing.set_visible(false);
        imp.controls.set_visible(false);
        imp.controls.set_orientation(gtk::Orientation::Vertical);

        imp.controls.append(&heading("Bluetooth", true));
        let section = section_list("Bluetooth");
        imp.adapters.set_model(Some(&imp.adapter_names));
        imp.adapters.set_valign(gtk::Align::Center);
        imp.adapters
            .update_property(&[gtk::accessible::Property::Label("Adapter")]);
        let weak = self.downgrade();
        imp.adapters.connect_selected_notify(move |_| {
            if let Some(page) = weak.upgrade() {
                page.adapter_chosen();
            }
        });
        let adapter_hint = gtk::Label::new(Some("The Bluetooth radio this page shows"));
        adapter_hint.add_css_class(style::DIM);
        imp.adapter_row.set_child(Some(&row(
            &gtk::Label::new(Some("Adapter")),
            &adapter_hint,
            &[imp.adapters.upcast_ref()],
        )));
        plain_row(&imp.adapter_row);
        imp.adapter_row.set_visible(false);
        section.append(&imp.adapter_row);

        imp.power_subtitle.add_css_class(style::DIM);
        imp.power.set_valign(gtk::Align::Center);
        imp.power
            .update_property(&[gtk::accessible::Property::Label("Bluetooth")]);
        let weak = self.downgrade();
        imp.power.connect_state_set(move |_, on| {
            weak.upgrade()
                .map_or(glib::Propagation::Proceed, |page| page.power_changed(on))
        });
        section.append(&list_row(&row(
            &gtk::Label::new(Some("Bluetooth")),
            &imp.power_subtitle,
            &[imp.power.upcast_ref()],
        )));
        // Its explanations of a block are longer than a line.
        imp.power_subtitle
            .set_ellipsize(gtk::pango::EllipsizeMode::None);
        imp.power_subtitle.set_wrap(true);
        imp.controls.append(&section);

        self.build_unblock_help();
        imp.controls.append(&imp.unblock_help);

        imp.error.set_xalign(0.0);
        imp.error.set_wrap(true);
        imp.error.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        imp.error.set_margin_top(8);
        imp.error.add_css_class("error");
        imp.error.set_visible(false);
        imp.controls.append(&imp.error);

        imp.controls.append(&heading("My devices", false));
        device_list(&imp.mine, "My devices");
        imp.controls.append(&imp.mine);
        hint(&imp.mine_hint);
        imp.controls.append(&imp.mine_hint);

        imp.others_heading
            .set_orientation(gtk::Orientation::Horizontal);
        imp.others_heading.set_spacing(8);
        imp.others_heading.set_margin_top(24);
        imp.others_heading.set_margin_bottom(8);
        let others = heading("Other devices", false);
        others.set_margin_top(0);
        others.set_margin_bottom(0);
        imp.others_heading.append(&others);
        imp.searching.set_spacing(6);
        imp.spinner_slot.set_valign(gtk::Align::Center);
        imp.searching.append(&imp.spinner_slot);
        let searching = gtk::Label::new(Some("Searching while this page is open"));
        searching.add_css_class(style::DIM);
        imp.searching.append(&searching);
        imp.others_heading.append(&imp.searching);
        imp.controls.append(&imp.others_heading);
        device_list(&imp.others, "Other devices");
        imp.controls.append(&imp.others);
        hint(&imp.others_hint);
        imp.controls.append(&imp.others_hint);
    }

    /// The box that says why the soft block couldn't be lifted, with the command to copy.
    fn build_unblock_help(&self) {
        let help = &self.imp().unblock_help;
        help.set_orientation(gtk::Orientation::Vertical);
        help.set_spacing(6);
        help.set_margin_top(8);
        help.set_visible(false);
        let text = gtk::Label::new(Some(
            "Bluetooth is blocked, and Sway Things has no access to /dev/rfkill to lift the \
             block. Run this in a terminal:",
        ));
        text.set_xalign(0.0);
        text.set_wrap(true);
        text.add_css_class("error");
        help.append(&text);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let command = gtk::Label::new(Some(UNBLOCK_COMMAND));
        command.set_xalign(0.0);
        command.set_hexpand(true);
        command.set_selectable(true);
        command.add_css_class("monospace");
        let copy = gtk::Button::with_label("Copy");
        copy.update_property(&[gtk::accessible::Property::Label("Copy the command")]);
        copy.connect_clicked(|button| {
            button.clipboard().set_text(UNBLOCK_COMMAND);
        });
        line.append(&command);
        line.append(&copy);
        help.append(&line);
    }

    /// Connects to BlueZ, shows what it reports and follows its changes and the agent's questions
    /// until the page goes.
    fn follow(&self, bus: Bus) {
        let weak = self.downgrade();
        let task = glib::spawn_future_local(async move {
            let bluez = match &bus {
                Bus::System => Bluez::connect().await,
                Bus::Address(address) => Bluez::connect_to(address).await,
                Bus::Off => Err(Error::Unavailable("no bus".to_owned())),
            };
            let bluez = match bluez {
                Ok(bluez) => bluez,
                Err(error) => {
                    if let Some(page) = weak.upgrade() {
                        page.show(Err(error));
                    }
                    return;
                }
            };
            let Some(page) = weak.upgrade() else {
                return;
            };
            page.imp().bluez.replace(Some(bluez.clone()));
            page.follow_agent(&bluez);
            drop(page);
            // Subscribed before the first snapshot, so no change between the two gets lost.
            let changes = bluez.changes().await;
            if !refresh(&weak, &bluez).await {
                return;
            }
            let mut changes = match changes {
                Ok(changes) => changes,
                Err(error) => {
                    if let Some(page) = weak.upgrade() {
                        page.show_error(&format!("The page doesn't update on its own: {error}"));
                    }
                    return;
                }
            };
            while changes.next().await.is_some() {
                glib::timeout_future(THROTTLE).await;
                while let Some(Some(())) = poll_once(changes.next()).await {}
                if !refresh(&weak, &bluez).await {
                    return;
                }
            }
        });
        self.imp().tasks.borrow_mut().push(task);
    }

    /// Follows the rfkill block of the Bluetooth radios. Without rfkill (no `/dev/rfkill`, no
    /// read access), the page knows only what BlueZ says.
    fn follow_rfkill(&self) {
        let Ok(mut rfkill) = Rfkill::open() else {
            return;
        };
        self.imp().block.set(Some(rfkill.block()));
        let weak = self.downgrade();
        let task = glib::spawn_future_local(async move {
            while let Ok(block) = rfkill.changed().await {
                let Some(page) = weak.upgrade() else {
                    return;
                };
                page.imp().block.set(Some(block));
                if !block.soft {
                    page.imp().unblock_help.set_visible(false);
                }
                page.render();
            }
            if let Some(page) = weak.upgrade() {
                page.imp().block.set(None);
            }
        });
        self.imp().tasks.borrow_mut().push(task);
    }

    /// Shows the agent's questions as dialogs while the page lives.
    fn follow_agent(&self, bluez: &Bluez) {
        let events = bluez.agent_events();
        let weak = self.downgrade();
        let task = glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                let Some(page) = weak.upgrade() else {
                    return;
                };
                match event {
                    AgentEvent::Prompt(prompt) => page.prompt(prompt),
                    AgentEvent::Cancel => page.close_dialog(),
                }
            }
        });
        self.imp().tasks.borrow_mut().push(task);
    }

    /// Shows `snapshot`, or why there is none.
    fn show(&self, snapshot: Result<Snapshot, Error>) {
        let imp = self.imp();
        match snapshot {
            Ok(snapshot) => {
                let selected = imp.selected.borrow().clone();
                let keep = selected.filter(|path| snapshot.adapter(path).is_some());
                let selected = keep.or_else(|| snapshot.adapters.first().map(|a| a.path.clone()));
                imp.selected.replace(selected);
                imp.snapshot.replace(Some(snapshot));
            }
            Err(error) => {
                imp.snapshot.replace(None);
                imp.selected.replace(None);
                // BlueZ is gone, and with it the discovery.
                imp.discovery.replace(None);
                self.close_dialog();
                self.show_missing(&error);
                return;
            }
        }
        self.fill_adapters();
        self.render();
        self.update_discovery();
    }

    /// Puts the adapters into the drop-down, the selected one chosen.
    fn fill_adapters(&self) {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return;
        };
        let names: Vec<String> = snapshot
            .adapters
            .iter()
            .map(|adapter| format!("{} ({})", adapter.name, adapter.id()))
            .collect();
        let current: Vec<String> = (0..imp.adapter_names.n_items())
            .filter_map(|i| imp.adapter_names.string(i).map(|s| s.to_string()))
            .collect();
        let selected = imp.selected.borrow().clone();
        let position = snapshot
            .adapters
            .iter()
            .position(|adapter| Some(&adapter.path) == selected.as_ref())
            .and_then(|i| u32::try_from(i).ok())
            .unwrap_or(0);
        imp.filling.set(true);
        if names != current {
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            imp.adapter_names
                .splice(0, imp.adapter_names.n_items(), &names);
        }
        if imp.adapters.selected() != position {
            imp.adapters.set_selected(position);
        }
        imp.filling.set(false);
        imp.adapter_row.set_visible(snapshot.adapters.len() > 1);
    }

    /// The user chose another adapter.
    fn adapter_chosen(&self) {
        let imp = self.imp();
        if imp.filling.get() {
            return;
        }
        let path = imp.snapshot.borrow().as_ref().and_then(|snapshot| {
            let index = usize::try_from(imp.adapters.selected()).ok()?;
            snapshot
                .adapters
                .get(index)
                .map(|adapter| adapter.path.clone())
        });
        if path.is_some() && path != *imp.selected.borrow() {
            imp.selected.replace(path);
            self.show_error("");
            self.render();
            self.update_discovery();
        }
    }

    /// Shows the selected adapter, its devices and the rfkill block.
    fn render(&self) {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return;
        };
        let selected = imp.selected.borrow().clone();
        let Some(adapter) = selected.as_deref().and_then(|path| snapshot.adapter(path)) else {
            self.show_missing(&Error::Unavailable(String::new()));
            return;
        };
        imp.missing.set_visible(false);
        imp.controls.set_visible(true);

        let block = imp.block.get().unwrap_or_default();
        imp.power.set_active(adapter.powered);
        imp.power.set_state(adapter.powered);
        imp.power.set_sensitive(!block.hard);
        imp.power_subtitle.set_text(&if block.hard {
            "Turned off by a hardware switch or key. Use it to turn Bluetooth back on.".to_owned()
        } else if block.soft && !adapter.powered {
            "Blocked by software, e.g. airplane mode. Turning it on lifts the block.".to_owned()
        } else {
            format!("Visible as “{}” \u{b7} {}", adapter.name, adapter.address)
        });

        let searching = adapter.powered && adapter.discovering;
        let started = searching && !imp.searching.is_visible();
        imp.searching.set_visible(searching);
        if started {
            self.new_spinner();
        }

        let devices: Vec<&Device> = snapshot.devices_of(&adapter.path).collect();
        let running = imp.running.borrow().clone();
        let mut rows = imp.rows.borrow_mut();
        rows.retain(|path, row| {
            let keep = devices.iter().any(|device| device.path == *path);
            if !keep {
                remove_row(&row.row);
            }
            keep
        });
        for device in &devices {
            let row = rows
                .entry(device.path.clone())
                .or_insert_with(|| self.device_row(&device.path));
            update_row(
                row,
                device,
                running.get(&device.path).copied(),
                adapter.powered,
            );
        }
        let mine: Vec<&DeviceRow> = devices
            .iter()
            .filter(|device| device.paired)
            .filter_map(|device| rows.get(&device.path))
            .collect();
        let others: Vec<&DeviceRow> = devices
            .iter()
            .filter(|device| !device.paired)
            .filter_map(|device| rows.get(&device.path))
            .collect();
        arrange(&imp.mine, &mine);
        arrange(&imp.others, &others);
        let (mine, others) = (mine.len(), others.len());
        drop(rows);

        imp.mine.set_visible(mine > 0);
        set_hint(
            &imp.mine_hint,
            (mine == 0)
                .then_some("No paired devices yet. Pick one under Other devices to connect it."),
        );
        imp.others_heading.set_visible(adapter.powered);
        imp.others.set_visible(adapter.powered && others > 0);
        set_hint(
            &imp.others_hint,
            if !adapter.powered {
                None
            } else if others == 0 {
                Some(
                    "No other devices found yet. Put the device you want to connect into pairing mode.",
                )
            } else {
                None
            },
        );
    }

    /// Shows a placeholder for `error` (or for no adapter) instead of the controls.
    fn show_missing(&self, error: &Error) {
        let imp = self.imp();
        let placeholder = match error {
            Error::Unavailable(reason) if reason.is_empty() => Placeholder::new(
                "bluetooth-disabled-symbolic",
                "No Bluetooth adapter",
                "Plug in a Bluetooth adapter or turn it on in the firmware settings; this page \
                 updates by itself.",
            ),
            Error::Unavailable(_) => Placeholder::new(
                "bluetooth-disabled-symbolic",
                "Bluetooth isn't available",
                "The Bluetooth service isn't running. Start it with `systemctl start bluetooth`; \
                 this page updates by itself.",
            ),
            other => Placeholder::new(
                "dialog-error-symbolic",
                "Bluetooth can't be read",
                &other.to_string(),
            ),
        };
        while let Some(child) = imp.missing.first_child() {
            imp.missing.remove(&child);
        }
        placeholder.set_hexpand(true);
        imp.missing.append(&placeholder);
        imp.controls.set_visible(false);
        imp.missing.set_visible(true);
    }

    /// Shows `message` under the adapter's controls, or hides it when empty.
    fn show_error(&self, message: &str) {
        let error = &self.imp().error;
        error.set_text(message);
        error.set_visible(!message.is_empty());
    }

    /// A new row for the device at `path`; [`update_row`] fills it.
    fn device_row(&self, path: &str) -> DeviceRow {
        // Decorative: the name and state say it all.
        let icon = gtk::Image::builder()
            .pixel_size(24)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();
        let name = gtk::Label::new(None);
        let status = gtk::Label::new(None);
        status.add_css_class(style::DIM);
        let button = gtk::Button::new();
        button.set_valign(gtk::Align::Center);
        let weak = self.downgrade();
        let device = path.to_owned();
        button.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.main_action(&device);
            }
        });

        let menu_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&menu_box));
        for (label, action) in [("Rename…", Action::Rename), ("Forget…", Action::Forget)] {
            let item = gtk::Button::with_label(label);
            item.add_css_class("flat");
            if let Some(child) = item.child().and_downcast::<gtk::Label>() {
                child.set_xalign(0.0);
            }
            let weak = self.downgrade();
            let device = path.to_owned();
            item.connect_clicked(glib::clone!(
                #[weak]
                popover,
                move |_| {
                    popover.popdown();
                    if let Some(page) = weak.upgrade() {
                        page.run(action, &device);
                    }
                }
            ));
            menu_box.append(&item);
        }
        let menu = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .popover(&popover)
            .valign(gtk::Align::Center)
            .tooltip_text("More actions")
            .build();
        menu.add_css_class("flat");

        let line = row(&name, &status, &[button.upcast_ref(), menu.upcast_ref()]);
        line.prepend(&icon);
        DeviceRow {
            row: list_row(&line),
            icon,
            name,
            status,
            button,
            menu,
            paired: false,
        }
    }

    /// The device at `path`, as last read.
    fn device(&self, path: &str) -> Option<Device> {
        self.imp()
            .snapshot
            .borrow()
            .as_ref()
            .and_then(|snapshot| snapshot.device(path).cloned())
    }

    /// What the row's button does: connect (pairing first if needed) or disconnect.
    fn main_action(&self, path: &str) {
        let Some(device) = self.device(path) else {
            return;
        };
        let action = if device.connected {
            Action::Disconnect
        } else if device.paired {
            Action::Connect
        } else {
            Action::Pair
        };
        self.run(action, path);
    }

    /// Runs `action` on the device at `path`; a failure shows under the controls. Pairing goes on
    /// to connect; forgetting asks first, renaming asks for the name.
    fn run(&self, action: Action, path: &str) {
        let imp = self.imp();
        let (Some(bluez), Some(device)) = (imp.bluez.borrow().clone(), self.device(path)) else {
            return;
        };
        if imp.running.borrow().contains_key(path) {
            return;
        }
        if action == Action::Rename {
            self.rename(&bluez, &device);
            return;
        }
        let weak = self.downgrade();
        let window = self.root().and_downcast::<gtk::Window>();
        glib::spawn_future_local(async move {
            if action == Action::Forget
                && !dialogs::confirm_forget(&device.name, window.as_ref()).await
            {
                return;
            }
            let mut action = action;
            let result = loop {
                set_running(&weak, &device.path, Some(action));
                let result = match action {
                    Action::Pair => bluez.pair(&device.path).await,
                    Action::Connect => bluez.connect_device(&device.path).await,
                    Action::Disconnect => bluez.disconnect_device(&device.path).await,
                    Action::Forget => bluez.remove(&device.adapter, &device.path).await,
                    Action::Rename => Ok(()),
                };
                if action == Action::Pair {
                    if let Some(page) = weak.upgrade() {
                        page.close_dialog_of(&device.path);
                    }
                }
                match result {
                    // A device pairs to be used: connect it right away.
                    Ok(()) if action == Action::Pair => action = Action::Connect,
                    result => break result,
                }
            };
            set_running(&weak, &device.path, None);
            if let Some(page) = weak.upgrade() {
                page.show_error(&match result {
                    Ok(()) => String::new(),
                    Err(error) => format!("{} “{}”: {error}", action.failure(), device.name),
                });
            }
            refresh(&weak, &bluez).await;
        });
    }

    /// Asks for a new name for `device` and sets it.
    fn rename(&self, bluez: &Bluez, device: &Device) {
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let bluez = bluez.clone();
        let path = device.path.clone();
        let old = device.name.clone();
        Dialog::open(
            window.as_ref(),
            &Spec {
                title: format!("Rename “{}”", device.name),
                body: String::new(),
                code: None,
                field: Some(Field {
                    label: "Name",
                    text: device.name.clone(),
                    valid: |name| check_alias(name).is_ok(),
                }),
                note: Some("Leave it empty to use the name the device announces.".to_owned()),
                accept: Some("Rename"),
            },
            move |name| {
                let Some(name) = name else {
                    return;
                };
                glib::spawn_future_local(async move {
                    set_running(&weak, &path, Some(Action::Rename));
                    let result = bluez.set_alias(&path, &name).await;
                    set_running(&weak, &path, None);
                    if let Some(page) = weak.upgrade() {
                        page.show_error(&match result {
                            Ok(()) => String::new(),
                            Err(error) => format!("{} “{old}”: {error}", Action::Rename.failure()),
                        });
                    }
                    refresh(&weak, &bluez).await;
                });
            },
        );
    }

    /// Shows the agent's `prompt` as a dialog, or updates the open one.
    fn prompt(&self, prompt: Prompt) {
        let imp = self.imp();
        let name = self
            .device(&prompt.device)
            .map_or_else(|| prompt.device.clone(), |device| device.name);
        if let Request::DisplayPasskey { passkey, entered } = prompt.request {
            if let Some((path, dialog)) = imp.dialog.borrow().as_ref() {
                if *path == prompt.device {
                    dialog.set_typed(passkey, entered);
                    return;
                }
            }
        }
        self.close_dialog();
        let window = self.root().and_downcast::<gtk::Window>();
        let (spec, passkey) = prompt_spec(&prompt.request, &name);
        let device = prompt.device.clone();
        let weak = self.downgrade();
        let request = prompt.request.clone();
        let mut prompt = Some(prompt);
        let dialog = Dialog::open(window.as_ref(), &spec, move |answer| {
            let Some(prompt) = prompt.take() else {
                return;
            };
            if !prompt.request.needs_answer() {
                // Only shown: closing it cancels the pairing.
                if answer.is_none() {
                    if let Some(page) = weak.upgrade() {
                        page.cancel_pairing(&prompt.device);
                    }
                }
                return;
            }
            prompt.answer(match (answer, &request) {
                (None, _) => Answer::Reject,
                (Some(pin), Request::PinCode) => Answer::PinCode(pin.trim().to_owned()),
                (Some(text), Request::Passkey) => {
                    parse_passkey(&text).map_or(Answer::Reject, Answer::Passkey)
                }
                (Some(_), _) => Answer::Accept,
            });
            if let Some(page) = weak.upgrade() {
                page.imp().dialog.replace(None);
            }
        });
        if let Some((passkey, entered)) = passkey {
            dialog.set_typed(passkey, entered);
        }
        imp.dialog.replace(Some((device, dialog)));
    }

    /// Stops the pairing of the device at `path`.
    fn cancel_pairing(&self, path: &str) {
        let Some(bluez) = self.imp().bluez.borrow().clone() else {
            return;
        };
        let path = path.to_owned();
        glib::spawn_future_local(async move {
            // The pairing may have ended already; its result is reported by the pairing itself.
            let _ = bluez.cancel_pairing(&path).await;
        });
    }

    /// Closes the agent's dialog, if one is open.
    fn close_dialog(&self) {
        if let Some((_, dialog)) = self.imp().dialog.take() {
            dialog.close();
        }
    }

    /// Closes the agent's dialog if it is about the device at `path`.
    fn close_dialog_of(&self, path: &str) {
        let open = self
            .imp()
            .dialog
            .borrow()
            .as_ref()
            .is_some_and(|(device, _)| device == path);
        if open {
            self.close_dialog();
        }
    }

    /// The handler of the power switch: turns the radio on (lifting a soft block first) or off.
    fn power_changed(&self, on: bool) -> glib::Propagation {
        let imp = self.imp();
        let adapter = imp.selected.borrow().clone().and_then(|path| {
            let snapshot = imp.snapshot.borrow();
            let powered = snapshot.as_ref()?.adapter(&path)?.powered;
            Some((path, powered))
        });
        let (Some(bluez), Some((path, powered))) = (imp.bluez.borrow().clone(), adapter) else {
            return glib::Propagation::Proceed;
        };
        if powered == on {
            return glib::Propagation::Proceed;
        }
        let soft = imp.block.get().is_some_and(|block| block.soft);
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            if on && soft {
                if rfkill::unblock().is_err() {
                    if let Some(page) = weak.upgrade() {
                        page.imp().unblock_help.set_visible(true);
                        page.render();
                    }
                    return;
                }
                glib::timeout_future(UNBLOCK_SETTLE).await;
            }
            let result = bluez.set_powered(&path, on).await;
            if let Some(page) = weak.upgrade() {
                let blocked = matches!(result, Err(Error::Blocked));
                page.imp().unblock_help.set_visible(blocked);
                page.show_error(&match result {
                    Ok(()) | Err(Error::Blocked) => String::new(),
                    Err(error) => format!(
                        "Couldn't turn Bluetooth {}: {error}",
                        if on { "on" } else { "off" }
                    ),
                });
            }
            refresh(&weak, &bluez).await;
        });
        // The switch shows the new position; its state follows once BlueZ reports it.
        glib::Propagation::Stop
    }

    /// Puts a new, turning spinner next to "Searching while this page is open".
    fn new_spinner(&self) {
        let imp = self.imp();
        if let Some(old) = imp.spinner.take() {
            imp.spinner_slot.remove(&old);
        }
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        imp.spinner_slot.append(&spinner);
        imp.spinner.replace(Some(spinner));
    }

    /// Whether the spinner next to "Searching while this page is open" is shown and turns; for
    /// tests.
    pub fn is_searching(&self) -> bool {
        let imp = self.imp();
        imp.searching.is_visible()
            && imp
                .spinner
                .borrow()
                .as_ref()
                .is_some_and(gtk::Spinner::is_spinning)
    }

    /// Looks for devices on the selected adapter while the page is on screen and its radio is
    /// on, and stops otherwise.
    fn update_discovery(&self) {
        let imp = self.imp();
        let wanted = if self.is_mapped() {
            imp.selected.borrow().clone().filter(|path| {
                imp.snapshot
                    .borrow()
                    .as_ref()
                    .and_then(|snapshot| snapshot.adapter(path))
                    .is_some_and(|adapter| adapter.powered)
            })
        } else {
            None
        };
        if wanted == *imp.discovery.borrow() {
            return;
        }
        self.stop_discovery();
        let (Some(path), Some(bluez)) = (wanted, imp.bluez.borrow().clone()) else {
            return;
        };
        imp.discovery.replace(Some(path.clone()));
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            if let Err(error) = bluez.start_discovery(&path).await {
                if let Some(page) = weak.upgrade() {
                    page.show_error(&format!("Couldn't look for devices: {error}"));
                }
            }
        });
    }

    /// Stops the discovery the page started, so the radio doesn't keep scanning once the page is
    /// gone or hidden. BlueZ also stops it when the connection closes.
    fn stop_discovery(&self) {
        let imp = self.imp();
        let Some(path) = imp.discovery.take() else {
            return;
        };
        if let Some(bluez) = imp.bluez.borrow().clone() {
            glib::spawn_future_local(async move {
                // The radio may be off or BlueZ gone; then there's nothing to stop.
                let _ = bluez.stop_discovery(&path).await;
            });
        }
    }

    /// Starts blueman-manager.
    fn open_blueman(&self) {
        match delegate::BLUEMAN.launch() {
            Ok(mut child) => {
                // Waiting reaps it once it ends, so it doesn't stay behind as a zombie.
                glib::spawn_future_local(async move {
                    let _ = child.status().await;
                });
            }
            Err(error) => self.show_error(&format!("Blueman can't be started: {error}")),
        }
    }
}

/// What the dialog for `request` about the device `name` shows, and the passkey with the digits
/// typed so far for a passkey to type.
fn prompt_spec(request: &Request, name: &str) -> (Spec, Option<(u32, u16)>) {
    let pair = format!("Pair with “{name}”");
    let spec = |title: String, body: String, code, field, accept| Spec {
        title,
        body,
        code,
        field,
        note: None,
        accept,
    };
    match request {
        Request::Confirmation(passkey) => (
            spec(
                format!("{pair}?"),
                format!("Check that “{name}” shows this code too."),
                Some(dialogs::grouped(*passkey)),
                None,
                Some("Pair"),
            ),
            None,
        ),
        Request::Authorization => (
            spec(
                format!("{pair}?"),
                format!(
                    "“{name}” shows no code to compare. Pair only if you just started pairing on it."
                ),
                None,
                None,
                Some("Pair"),
            ),
            None,
        ),
        Request::Service(uuid) => (
            spec(
                format!("Allow “{name}” to connect?"),
                format!("It asks to use the service {uuid}."),
                None,
                None,
                Some("Allow"),
            ),
            None,
        ),
        Request::PinCode => (
            spec(
                pair,
                format!(
                    "Enter the PIN that “{name}” expects. Its manual has it; often 0000 or 1234."
                ),
                None,
                Some(Field {
                    label: "PIN",
                    text: String::new(),
                    valid: |pin| valid_pin(pin.trim()),
                }),
                Some("Pair"),
            ),
            None,
        ),
        Request::Passkey => (
            spec(
                pair,
                format!("Enter the six-digit code that “{name}” shows."),
                None,
                Some(Field {
                    label: "Code",
                    text: String::new(),
                    valid: |code| parse_passkey(code).is_some(),
                }),
                Some("Pair"),
            ),
            None,
        ),
        Request::DisplayPinCode(pin) => (
            spec(
                pair,
                format!("Type this PIN on “{name}”, then press Enter on it."),
                Some(pin.clone()),
                None,
                None,
            ),
            None,
        ),
        Request::DisplayPasskey { passkey, entered } => (
            spec(
                pair,
                format!("Type this code on “{name}”, then press Enter on it."),
                Some(dialogs::grouped(*passkey)),
                None,
                None,
            ),
            Some((*passkey, *entered)),
        ),
    }
}

/// Fills `row` from `device`; `running` is what runs on it.
fn update_row(row: &mut DeviceRow, device: &Device, running: Option<Action>, powered: bool) {
    row.paired = device.paired;
    row.icon
        .set_icon_name(Some(&icon_name(device.icon.as_deref())));
    // Untrusted: plain text, never markup.
    row.name.set_text(&device.name);
    if device.named {
        row.name.remove_css_class(style::DIM);
    } else {
        row.name.add_css_class(style::DIM);
    }
    let state = if let Some(action) = running {
        action.running().to_owned()
    } else if device.connected {
        "Connected".to_owned()
    } else if device.paired {
        "Not connected".to_owned()
    } else if device.named {
        "Not paired".to_owned()
    } else {
        "Not paired \u{b7} no name".to_owned()
    };
    row.status.set_text(&match device.battery {
        Some(percent) if device.paired => format!("{state} \u{b7} Battery {percent} %"),
        _ => state,
    });
    let label = match running {
        Some(action) => action.running(),
        None if device.connected => "Disconnect",
        None => "Connect",
    };
    row.button.set_label(label);
    row.button.set_sensitive(powered && running.is_none());
    row.button
        .update_property(&[gtk::accessible::Property::Label(&format!(
            "{label} {}",
            device.name
        ))]);
    row.menu.set_visible(device.paired);
    row.menu.set_sensitive(running.is_none());
    row.menu
        .update_property(&[gtk::accessible::Property::Label(&format!(
            "More actions for {}",
            device.name
        ))]);
}

/// Puts `rows` into `list` in this order, moving only what is out of place, so a row with the
/// focus keeps it unless it moves.
fn arrange(list: &gtk::ListBox, rows: &[&DeviceRow]) {
    let current: Vec<gtk::Widget> = {
        let mut out = Vec::new();
        let mut child = list.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            out.push(widget);
        }
        out
    };
    let wanted: Vec<gtk::Widget> = rows.iter().map(|row| row.row.clone().upcast()).collect();
    if current == wanted {
        return;
    }
    let focus = list
        .root()
        .and_then(|root| root.focus())
        .filter(|focus| rows.iter().any(|row| focus.is_ancestor(&row.row)));
    for row in rows {
        remove_row(&row.row);
    }
    for row in rows {
        list.append(&row.row);
    }
    if let Some(focus) = focus {
        focus.grab_focus();
    }
}

/// Takes `row` out of the list it is in.
fn remove_row(row: &gtk::ListBoxRow) {
    if let Some(list) = row.parent().and_downcast::<gtk::ListBox>() {
        list.remove(row);
    }
}

/// Marks what runs on the device at `path` and shows it.
fn set_running(page: &glib::WeakRef<BluetoothPage>, path: &str, action: Option<Action>) {
    let Some(page) = page.upgrade() else {
        return;
    };
    {
        let mut running = page.imp().running.borrow_mut();
        match action {
            Some(action) => running.insert(path.to_owned(), action),
            None => running.remove(path),
        };
    }
    page.render();
}

/// Reads a new snapshot and shows it on the page. False once the page is gone.
async fn refresh(page: &glib::WeakRef<BluetoothPage>, bluez: &Bluez) -> bool {
    let snapshot = bluez.snapshot().await;
    let Some(page) = page.upgrade() else {
        return false;
    };
    page.show(snapshot);
    true
}

/// The icon for a device of the kind BlueZ names `icon`: its symbolic variant if the theme has
/// it, else the icon itself, else a generic Bluetooth icon.
fn icon_name(icon: Option<&str>) -> String {
    const FALLBACK: &str = "bluetooth-symbolic";
    let (Some(icon), Some(display)) = (icon, gdk::Display::default()) else {
        return FALLBACK.to_owned();
    };
    let theme = gtk::IconTheme::for_display(&display);
    let symbolic = format!("{icon}-symbolic");
    if theme.has_icon(&symbolic) {
        symbolic
    } else if theme.has_icon(icon) {
        icon.to_owned()
    } else {
        FALLBACK.to_owned()
    }
}

/// A section heading as on the pages built from the registry; `first` has no space above it.
fn heading(title: &str, first: bool) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(title.to_uppercase())
        .xalign(0.0)
        .margin_start(4)
        .margin_bottom(8)
        .margin_top(if first { 0 } else { 24 })
        .accessible_role(gtk::AccessibleRole::Heading)
        .build();
    label.add_css_class("st-section-heading");
    label.add_css_class(style::DIM);
    label
}

/// A bordered list for the rows of a section.
fn section_list(title: &str) -> gtk::ListBox {
    let list = gtk::ListBox::new();
    device_list(&list, title);
    list
}

/// Makes `list` a bordered list of a section named `title`.
fn device_list(list: &gtk::ListBox, title: &str) {
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("st-section");
    list.update_property(&[gtk::accessible::Property::Label(title)]);
}

/// Styles `label` as the hint shown in place of an empty list.
fn hint(label: &gtk::Label) {
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.add_css_class(style::DIM);
    label.add_css_class("st-section-hint");
    label.set_visible(false);
}

/// Shows `text` in `label`, or hides it.
fn set_hint(label: &gtk::Label, text: Option<&str>) {
    label.set_text(text.unwrap_or_default());
    label.set_visible(text.is_some());
}

/// `child` as a row of a section list. The row itself takes no focus; its controls do.
fn list_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let item = gtk::ListBoxRow::new();
    item.set_child(Some(child));
    plain_row(&item);
    item
}

/// Makes `row` a row that takes no focus and can't be activated; its controls can.
fn plain_row(row: &gtk::ListBoxRow) {
    row.set_activatable(false);
    row.set_focusable(false);
}

/// A row with `title` above `subtitle` on the left and `controls` on the right.
fn row(title: &gtk::Label, subtitle: &gtk::Label, controls: &[&gtk::Widget]) -> gtk::Box {
    for label in [title, subtitle] {
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    }
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(title);
    text.append(subtitle);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.append(&text);
    for control in controls {
        line.append(*control);
    }
    line
}
