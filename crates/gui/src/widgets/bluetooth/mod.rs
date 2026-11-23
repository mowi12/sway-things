// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Bluetooth page: the adapter's power, the paired devices and the ones around, live from
//! BlueZ.
//!
//! The page reads a new snapshot whenever BlueZ reports a change, at most every [`THROTTLE`], and
//! updates its rows in place. By default it looks for devices the whole time it is on screen and
//! the radio is on; with the preference set to manual, only for [`SEARCH_FOR`] after _Search_,
//! which shows only then. A search stops once the page leaves the screen, the radio goes off or
//! another adapter is chosen.
//! _Connect_ on a device that isn't paired pairs it first; the pairing agent's questions come up
//! as dialogs. A soft-blocked radio is unblocked through rfkill when switched on.
//!
//! Device names are untrusted and only ever set as plain label text.

mod actions;
mod dialogs;
mod prompts;
mod rows;
mod search;

use std::time::Duration;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib};
use sway_things_core::bluetooth::rfkill::{self, Rfkill};
use sway_things_core::bluetooth::{
    AgentEvent, Bluez, Device, Error, Request, Snapshot, check_alias, parse_passkey, valid_pin,
};
use sway_things_core::delegate;

use self::dialogs::{Field, Spec};
use super::search_button;
use crate::i18n::{msg, msg_plural, ntrf, tr, trf};
use crate::style;
use crate::widgets::list::{heading, hint, list_row, plain_row, row, section_list};
use crate::widgets::live::follow_changes;

/// How long to collect a burst of changes before reading the snapshot again.
const THROTTLE: Duration = Duration::from_millis(200);
/// How long to give the kernel after lifting the soft block before powering the adapter.
const UNBLOCK_SETTLE: Duration = Duration::from_millis(500);
/// How long a search started with _Search_ looks for devices.
const SEARCH_FOR: Duration = Duration::from_secs(30);
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
    fn running(self) -> &'static str {
        tr(match self {
            Self::Pair => msg!("Pairing…"),
            Self::Connect => msg!("Connecting…"),
            Self::Disconnect => msg!("Disconnecting…"),
            Self::Forget => msg!("Forgetting…"),
            Self::Rename => msg!("Renaming…"),
        })
    }

    /// Whether it can be canceled while it runs: a connection or a pairing, which can take long
    /// with a device that is off or out of range.
    fn cancelable(self) -> bool {
        matches!(self, Self::Pair | Self::Connect)
    }

    /// The message when it fails on the device `name` with `error`.
    fn failure(self, name: &str, error: &Error) -> String {
        let text = match self {
            Self::Pair => msg!("Couldn't pair with “{device}”: {error}"),
            Self::Connect => msg!("Couldn't connect to “{device}”: {error}"),
            Self::Disconnect => msg!("Couldn't disconnect from “{device}”: {error}"),
            Self::Forget => msg!("Couldn't forget “{device}”: {error}"),
            Self::Rename => msg!("Couldn't rename “{device}”: {error}"),
        };
        trf(
            text,
            &[("device", &name), ("error", &error.text().translated())],
        )
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
    use std::collections::{HashMap, HashSet};

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
        /// The ⋮ menu of the adapter, with "Rename…".
        pub adapter_menu: gtk::MenuButton,
        pub adapter_rename: gtk::Button,
        pub visible: gtk::Switch,
        pub visible_subtitle: gtk::Label,
        /// Why the soft block couldn't be lifted, with the command to do it.
        pub unblock_help: gtk::Box,
        /// The error of the last action that failed.
        pub error: gtk::Label,
        /// Shown instead of the devices while the radio is off.
        pub off: gtk::Box,
        /// What `off` shows: whether a hard block keeps the radio off and whether it is soft
        /// blocked; `None` while it is empty.
        pub off_shown: Cell<Option<(bool, bool)>>,
        /// The sections with the devices.
        pub devices: gtk::Box,
        pub mine: gtk::ListBox,
        pub mine_hint: gtk::Label,
        pub others_heading: gtk::Box,
        pub searching: gtk::Box,
        pub searching_label: gtk::Label,
        /// _Search_, or _Stop_ while a search started with it runs.
        pub search: gtk::Button,
        /// Whether the page only looks for devices after _Search_.
        pub manual: Cell<bool>,
        /// The adapter a search started with _Search_ runs on, and when it ends.
        pub requested: RefCell<Option<(String, glib::SourceId)>>,
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
        /// The devices whose connection or pairing was canceled and hasn't ended yet.
        pub(super) canceled: RefCell<HashSet<String>>,
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
            self.obj().end_search();
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
            let button = gtk::Button::with_label(tr(msg!("Advanced…")));
            button.set_tooltip_text(Some(tr(msg!("Open Blueman for adapter settings and more"))));
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

    /// The "Visible to other devices" switch, for tests.
    pub fn visible_switch(&self) -> &gtk::Switch {
        &self.imp().visible
    }

    /// The "Rename…" item of the adapter's ⋮ menu, for tests.
    pub fn adapter_rename_button(&self) -> &gtk::Button {
        &self.imp().adapter_rename
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

        imp.controls.append(&heading(tr(msg!("Bluetooth")), true));
        let section = section_list(tr(msg!("Bluetooth")));
        imp.adapters.set_model(Some(&imp.adapter_names));
        imp.adapters.set_valign(gtk::Align::Center);
        imp.adapters
            .update_property(&[gtk::accessible::Property::Label(tr(msg!("Adapter")))]);
        let weak = self.downgrade();
        imp.adapters.connect_selected_notify(move |_| {
            if let Some(page) = weak.upgrade() {
                page.adapter_chosen();
            }
        });
        let adapter_hint = gtk::Label::new(Some(tr(msg!("The Bluetooth radio this page shows"))));
        adapter_hint.add_css_class(style::DIM);
        imp.adapter_row.set_child(Some(&row(
            &gtk::Label::new(Some(tr(msg!("Adapter")))),
            &adapter_hint,
            &[imp.adapters.upcast_ref()],
        )));
        plain_row(&imp.adapter_row);
        imp.adapter_row.set_visible(false);
        section.append(&imp.adapter_row);

        imp.power_subtitle.add_css_class(style::DIM);
        imp.power.set_valign(gtk::Align::Center);
        imp.power
            .update_property(&[gtk::accessible::Property::Label(tr(msg!("Bluetooth")))]);
        let weak = self.downgrade();
        imp.power.connect_state_set(move |_, on| {
            weak.upgrade()
                .map_or(glib::Propagation::Proceed, |page| page.power_changed(on))
        });
        self.build_adapter_menu();
        section.append(&list_row(&row(
            &gtk::Label::new(Some(tr(msg!("Bluetooth")))),
            &imp.power_subtitle,
            &[imp.power.upcast_ref(), imp.adapter_menu.upcast_ref()],
        )));
        // Its explanations of a block are longer than a line.
        imp.power_subtitle
            .set_ellipsize(gtk::pango::EllipsizeMode::None);
        imp.power_subtitle.set_wrap(true);

        self.build_visible_row(&section);
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

        imp.off.set_orientation(gtk::Orientation::Vertical);
        imp.off.set_margin_top(24);
        imp.off.set_visible(false);
        imp.controls.append(&imp.off);

        imp.devices.set_orientation(gtk::Orientation::Vertical);
        imp.controls.append(&imp.devices);
        imp.devices.append(&heading(tr(msg!("My devices")), false));
        crate::widgets::list::section(&imp.mine, tr(msg!("My devices")));
        imp.devices.append(&imp.mine);
        hint(&imp.mine_hint);
        imp.devices.append(&imp.mine_hint);

        self.build_others_heading();
        crate::widgets::list::section(&imp.others, tr(msg!("Other devices")));
        imp.devices.append(&imp.others);
        hint(&imp.others_hint);
        imp.devices.append(&imp.others_hint);
    }

    /// The heading of the other devices, with the spinner and the Search button.
    fn build_others_heading(&self) {
        let imp = self.imp();
        imp.others_heading
            .set_orientation(gtk::Orientation::Horizontal);
        // The heading and the spinner, with the margins of every heading line; the Search button
        // lies over the line (see `search_button::over`).
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.set_margin_top(24);
        line.set_margin_bottom(8);
        let others = heading(tr(msg!("Other devices")), false);
        others.set_margin_top(0);
        others.set_margin_bottom(0);
        line.append(&others);
        imp.searching.set_spacing(6);
        imp.spinner_slot.set_valign(gtk::Align::Center);
        imp.searching.append(&imp.spinner_slot);
        imp.searching_label.set_text(tr(msg!("Searching")));
        imp.searching_label.add_css_class(style::DIM);
        imp.searching.append(&imp.searching_label);
        line.append(&imp.searching);
        search_button::setup(&imp.search);
        let weak = self.downgrade();
        imp.search.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.search_clicked();
            }
        });
        let overlay = search_button::over(&line, &others, &imp.search);
        overlay.set_hexpand(true);
        imp.others_heading.append(&overlay);
        self.update_search_button();
        self.set_manual_search(false);
        imp.devices.append(&imp.others_heading);
    }

    /// The "Visible to other devices" row, appended to `section`.
    fn build_visible_row(&self, section: &gtk::ListBox) {
        let imp = self.imp();
        imp.visible_subtitle.add_css_class(style::DIM);
        imp.visible.set_valign(gtk::Align::Center);
        imp.visible
            .update_property(&[gtk::accessible::Property::Label(tr(msg!(
                "Visible to other devices"
            )))]);
        let weak = self.downgrade();
        imp.visible.connect_state_set(move |_, on| {
            weak.upgrade()
                .map_or(glib::Propagation::Proceed, |page| page.visible_changed(on))
        });
        section.append(&list_row(&row(
            &gtk::Label::new(Some(tr(msg!("Visible to other devices")))),
            &imp.visible_subtitle,
            &[imp.visible.upcast_ref()],
        )));
    }

    /// The adapter's ⋮ menu with "Rename…".
    fn build_adapter_menu(&self) {
        let imp = self.imp();
        let popover = gtk::Popover::new();
        imp.adapter_rename.set_label(tr(msg!("Rename…")));
        imp.adapter_rename.add_css_class("flat");
        if let Some(child) = imp.adapter_rename.child().and_downcast::<gtk::Label>() {
            child.set_xalign(0.0);
        }
        popover.set_child(Some(&imp.adapter_rename));
        let weak = self.downgrade();
        imp.adapter_rename.connect_clicked(glib::clone!(
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                if let Some(page) = weak.upgrade() {
                    page.rename_adapter();
                }
            }
        ));
        imp.adapter_menu.set_icon_name("view-more-symbolic");
        imp.adapter_menu.set_popover(Some(&popover));
        imp.adapter_menu.set_valign(gtk::Align::Center);
        imp.adapter_menu
            .set_tooltip_text(Some(tr(msg!("More actions"))));
        imp.adapter_menu.add_css_class("flat");
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
        let copy = gtk::Button::with_label(tr(msg!("Copy")));
        copy.update_property(&[gtk::accessible::Property::Label(tr(msg!(
            "Copy the command"
        )))]);
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
                Bus::Off => Err(Error::Unavailable(msg!("no bus").into())),
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
            let changes = match changes {
                Ok(changes) => changes,
                Err(error) => {
                    if let Some(page) = weak.upgrade() {
                        page.show_error(&trf(
                            msg!("The page doesn't update on its own: {error}"),
                            &[("error", &error.text().translated())],
                        ));
                    }
                    return;
                }
            };
            follow_changes(changes, THROTTLE, || refresh(&weak, &bluez)).await;
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
                    Err(error) => trf(
                        if on {
                            msg!("Couldn't turn Bluetooth on: {error}")
                        } else {
                            msg!("Couldn't turn Bluetooth off: {error}")
                        },
                        &[("error", &error.text().translated())],
                    ),
                });
            }
            refresh(&weak, &bluez).await;
        });
        // The switch shows the new position; its state follows once BlueZ reports it.
        glib::Propagation::Stop
    }

    /// Puts a new, turning spinner next to "Searching".
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
}

/// A dialog without a note under its field.
fn spec(
    title: String,
    body: String,
    code: Option<String>,
    field: Option<Field>,
    accept: Option<&'static str>,
) -> Spec {
    Spec {
        title,
        body,
        code,
        field,
        note: None,
        accept,
    }
}

/// What the dialog for `request` about the device `name` shows, and the passkey with the digits
/// typed so far for a passkey to type.
fn prompt_spec(request: &Request, name: &str) -> (Spec, Option<(u32, u16)>) {
    let named = |text| trf(text, &[("device", &name)]);
    let pair = named(msg!("Pair with “{device}”"));
    let pair_asked = named(msg!("Pair with “{device}”?"));
    match request {
        Request::Confirmation(passkey) => (
            spec(
                pair_asked,
                named(msg!("Check that “{device}” shows this code too.")),
                Some(dialogs::grouped(*passkey)),
                None,
                Some(tr(msg!("Pair"))),
            ),
            None,
        ),
        Request::Authorization => (
            spec(
                pair_asked,
                named(msg!(
                    "“{device}” shows no code to compare. Pair only if you just started pairing on \
                     it."
                )),
                None,
                None,
                Some(tr(msg!("Pair"))),
            ),
            None,
        ),
        Request::Service(uuid) => (
            spec(
                named(msg!("Allow “{device}” to connect?")),
                trf(
                    msg!("It asks to use the service {uuid}."),
                    &[("uuid", uuid)],
                ),
                None,
                None,
                Some(tr(msg!("Allow"))),
            ),
            None,
        ),
        Request::PinCode => (
            spec(
                pair,
                named(msg!(
                    "Enter the PIN that “{device}” expects. Its manual has it; often 0000 or 1234."
                )),
                None,
                Some(Field {
                    label: tr(msg!("PIN")),
                    text: String::new(),
                    valid: |pin| valid_pin(pin.trim()),
                }),
                Some(tr(msg!("Pair"))),
            ),
            None,
        ),
        Request::Passkey => (
            spec(
                pair,
                named(msg!("Enter the six-digit code that “{device}” shows.")),
                None,
                Some(Field {
                    label: tr(msg!("Code")),
                    text: String::new(),
                    valid: |code| parse_passkey(code).is_some(),
                }),
                Some(tr(msg!("Pair"))),
            ),
            None,
        ),
        Request::DisplayPinCode(pin) => (
            spec(
                pair,
                named(msg!("Type this PIN on “{device}”, then press Enter on it.")),
                Some(pin.clone()),
                None,
                None,
            ),
            None,
        ),
        Request::DisplayPasskey { passkey, entered } => (
            spec(
                pair,
                named(msg!(
                    "Type this code on “{device}”, then press Enter on it."
                )),
                Some(dialogs::grouped(*passkey)),
                None,
                None,
            ),
            Some((*passkey, *entered)),
        ),
    }
}

/// Fills `row` from `device`; `running` is what runs on it, `canceled` whether that was canceled.
fn update_row(
    row: &mut DeviceRow,
    device: &Device,
    running: Option<Action>,
    canceled: bool,
    powered: bool,
) {
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
        tr(msg!("Connected")).to_owned()
    } else if device.paired {
        tr(msg!("Not connected")).to_owned()
    } else if device.named {
        tr(msg!("Not paired")).to_owned()
    } else {
        tr(msg!("Not paired · no name")).to_owned()
    };
    row.status.set_text(&match device.battery {
        Some(percent) if device.paired => trf(
            msg!("{state} · Battery {percent} %"),
            &[("state", &state), ("percent", &percent)],
        ),
        _ => state,
    });
    let cancelable = running.is_some_and(Action::cancelable);
    let label = match running {
        Some(_) if cancelable => tr(msg!("Cancel")),
        Some(action) => action.running(),
        None if device.connected => tr(msg!("Disconnect")),
        None => tr(msg!("Connect")),
    };
    row.button.set_label(label);
    row.button.set_sensitive(if cancelable {
        !canceled
    } else {
        powered && running.is_none()
    });
    row.button
        .update_property(&[gtk::accessible::Property::Label(&trf(
            // TRANSLATORS: The button of a device for screen readers: "Connect Headphones".
            msg!("{action} {device}"),
            &[("action", &label), ("device", &device.name)],
        ))]);
    row.menu.set_visible(device.paired);
    row.menu.set_sensitive(running.is_none());
    row.menu
        .update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("More actions for {device}"),
            &[("device", &device.name)],
        ))]);
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

/// The dialog that asks for a new name instead of `name`, with `note` under the field.
fn rename_spec(name: &str, note: &str) -> Spec {
    Spec {
        title: trf(msg!("Rename “{device}”"), &[("device", &name)]),
        body: String::new(),
        code: None,
        field: Some(Field {
            label: tr(msg!("Name")),
            text: name.to_owned(),
            valid: |name| check_alias(name).is_ok(),
        }),
        note: Some(note.to_owned()),
        accept: Some(tr(msg!("Rename"))),
    }
}

/// The subtitle of "Visible to other devices": how long it stays visible after `timeout`
/// seconds (0: until switched off).
fn visible_text(timeout: u32) -> String {
    let count = |text, n: u32| {
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        ntrf(text, n, &[("count", &n)])
    };
    match timeout {
        0 => tr(msg!("Stays visible until you turn this off")).to_owned(),
        1..60 => count(
            msg_plural!(
                "Stays visible for {count} second",
                "Stays visible for {count} seconds"
            ),
            timeout,
        ),
        _ => count(
            msg_plural!(
                "Stays visible for {count} minute",
                "Stays visible for {count} minutes"
            ),
            timeout / 60,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::visible_text;

    #[test]
    fn the_visible_time_is_said_in_seconds_or_minutes() {
        assert_eq!(visible_text(0), "Stays visible until you turn this off");
        assert_eq!(visible_text(30), "Stays visible for 30 seconds");
        assert_eq!(visible_text(60), "Stays visible for 1 minute");
        assert_eq!(visible_text(180), "Stays visible for 3 minutes");
    }
}
