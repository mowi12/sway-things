// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Network page: Wi-Fi with its networks, the wired devices and the VPNs, live from
//! NetworkManager.
//!
//! The page reads a new snapshot whenever NetworkManager reports a change, at most every
//! [`THROTTLE`], and updates its rows in place. While it is on screen and Wi-Fi is on, it asks for
//! a scan every [`SCAN_EVERY`]. While it is on screen, and until a connection it started has
//! finished, our secret agent is registered, so a wrong or changed password brings up a dialog.
//!
//! SSIDs and connection names are untrusted and only ever set as plain label text.

mod dialogs;

use std::fmt::Write as _;
use std::time::Duration;

use futures_lite::StreamExt;
use futures_lite::future::poll_once;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::delegate;
use sway_things_core::network::{
    ActiveConnection, ActiveState, AgentEvent, Connection, ConnectionKind, Connectivity, Device,
    DeviceState, Error, IpConfig, Network, NetworkManager, SavedNetwork, SecretPrompt, Security,
    Snapshot, band, signal_level,
};

use self::dialogs::PasswordSpec;
use super::Placeholder;
use crate::style;

/// How long to collect a burst of changes before reading the snapshot again.
const THROTTLE: Duration = Duration::from_millis(200);
/// How often to ask for a scan while the page is on screen, as GNOME Settings does.
const SCAN_EVERY: u32 = 15;
/// The key of the hidden network's connection, which has no row.
const HIDDEN: &str = "hidden";

/// Where the page finds NetworkManager.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Bus {
    /// The system bus, where NetworkManager runs.
    #[default]
    System,
    /// Another bus, with a stand-in for NetworkManager on it (tests).
    Address(String),
    /// No bus: the page shows NetworkManager as unavailable (windows in tests that don't need
    /// it).
    Off,
}

/// What a row stands for, found again by its key in the latest snapshot.
#[allow(
    clippy::large_enum_variant,
    reason = "made for one action at a time and never stored"
)]
enum Target {
    /// A Wi-Fi network the selected device sees.
    Network(Network, Device),
    /// A saved Wi-Fi network that isn't around.
    Away(SavedNetwork),
    /// A wired device.
    Wired(Device),
    /// A VPN or WireGuard connection, and its active connection while it is on.
    Vpn(Connection, Option<ActiveConnection>),
}

/// The widgets of a row, kept across refreshes so focus and state stay. A row uses its button or
/// its switch, not both.
#[derive(Debug, Clone)]
struct NetRow {
    row: gtk::ListBoxRow,
    icon: gtk::Image,
    name: gtk::Label,
    status: gtk::Label,
    button: gtk::Button,
    switch: gtk::Switch,
    menu: gtk::MenuButton,
    details: gtk::Button,
    edit: gtk::Button,
    forget: gtk::Button,
}

mod imp {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::network::{NetworkManager, Snapshot};

    use super::NetRow;
    use super::dialogs::Dialog;

    #[derive(Default)]
    pub struct NetworkPage {
        /// The only child: holds the placeholder and the controls.
        pub content: gtk::Box,
        /// Shown instead of the controls while there is nothing to show.
        pub missing: gtk::Box,
        /// The sections.
        pub controls: gtk::Box,
        /// The note about a captive portal.
        pub portal: gtk::Box,
        pub portal_open: gtk::Button,
        /// The error of the last action that failed.
        pub error: gtk::Label,
        /// The Wi-Fi section: the adapter drop-down and the switch.
        pub wifi: gtk::ListBox,
        /// Shown instead of the Wi-Fi section without a Wi-Fi adapter.
        pub wifi_hint: gtk::Label,
        /// The adapter drop-down's row, shown with two or more adapters.
        pub adapter_row: gtk::ListBoxRow,
        pub adapters: gtk::DropDown,
        pub adapter_names: gtk::StringList,
        pub power: gtk::Switch,
        pub power_subtitle: gtk::Label,
        pub known_heading: gtk::Label,
        pub known: gtk::ListBox,
        pub known_hint: gtk::Label,
        pub others_heading: gtk::Box,
        pub searching: gtk::Box,
        /// Where the spinner is: replaced each time searching shows, since a spinner's
        /// animation stops when it leaves the screen and doesn't start again by itself.
        pub spinner_slot: gtk::Box,
        pub spinner: RefCell<Option<gtk::Spinner>>,
        pub others: gtk::ListBox,
        /// The last row of the other networks.
        pub hidden_row: gtk::ListBoxRow,
        pub others_hint: gtk::Label,
        pub wired_heading: gtk::Label,
        pub wired: gtk::ListBox,
        pub vpn_heading: gtk::Label,
        pub vpns: gtk::ListBox,
        /// The "Advanced…" button for the page header, if nm-connection-editor is installed.
        pub advanced: RefCell<Option<gtk::Button>>,
        /// Whether nm-connection-editor is installed.
        pub editor: Cell<bool>,

        pub nm: RefCell<Option<NetworkManager>>,
        /// What the page shows.
        pub snapshot: RefCell<Option<Snapshot>>,
        /// Who owns NetworkManager's name: when it changes, NetworkManager restarted.
        pub owner: RefCell<Option<String>>,
        /// The path of the Wi-Fi device the page shows.
        pub selected: RefCell<Option<String>>,
        /// The rows by key.
        pub(super) rows: RefCell<HashMap<String, NetRow>>,
        /// What runs on which row (its key), as the row shows it.
        pub(super) running: RefCell<HashMap<String, &'static str>>,
        /// The open dialog of the secret agent, and the connection it is about.
        pub prompt: RefCell<Option<(String, Dialog<String>)>>,
        /// The open dialog of the page itself (a password, a hidden network, details).
        pub dialog: RefCell<Option<gtk::Window>>,
        /// Whether our agent is registered (or on its way).
        pub agent: Cell<bool>,
        /// How many connections the page started are still on their way.
        pub pending: Cell<u32>,
        /// The scans while the page is on screen.
        pub scan: RefCell<Option<glib::SourceId>>,
        /// Follow NetworkManager and the agent for as long as the page lives.
        pub tasks: RefCell<Vec<glib::JoinHandle<()>>>,
        /// Whether the adapter drop-down is being filled (its signal isn't a choice then).
        pub filling: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NetworkPage {
        const NAME: &'static str = "StNetworkPage";
        type Type = super::NetworkPage;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-network");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for NetworkPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.content.set_parent(&*self.obj());
            self.obj().build();
        }

        fn dispose(&self) {
            for task in self.tasks.take() {
                task.abort();
            }
            if let Some(scan) = self.scan.take() {
                scan.remove();
            }
            if let Some((_, dialog)) = self.prompt.take() {
                dialog.close();
            }
            if let Some(dialog) = self.dialog.take() {
                dialog.destroy();
            }
            // NetworkManager also forgets the agent when our connection closes.
            if self.agent.replace(false) {
                if let Some(nm) = self.nm.borrow().clone() {
                    glib::spawn_future_local(async move {
                        let _ = nm.unregister_agent().await;
                    });
                }
            }
            self.content.unparent();
        }
    }

    impl WidgetImpl for NetworkPage {
        fn map(&self) {
            self.parent_map();
            self.obj().new_spinner();
            self.obj().update_scan();
            self.obj().update_agent();
        }

        fn unmap(&self) {
            self.parent_unmap();
            self.obj().update_scan();
            self.obj().update_agent();
        }
    }
}

glib::wrapper! {
    /// Wi-Fi, wired and VPN connections, live from NetworkManager.
    pub struct NetworkPage(ObjectSubclass<imp::NetworkPage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl NetworkPage {
    /// The page on `bus`, which connects to NetworkManager right away and follows it.
    pub fn new(bus: &Bus) -> Self {
        let page: Self = glib::Object::new();
        page.follow(bus.clone());
        page
    }

    /// The "Advanced…" button for the page header: it starts nm-connection-editor. `None` if it
    /// isn't installed.
    pub fn advanced_button(&self) -> Option<gtk::Button> {
        let imp = self.imp();
        if imp.advanced.borrow().is_none() && imp.editor.get() {
            let button = gtk::Button::with_label("Advanced…");
            button.set_tooltip_text(Some(
                "Open nm-connection-editor for VPNs, 802.1X networks and more",
            ));
            let weak = self.downgrade();
            button.connect_clicked(move |_| {
                if let Some(page) = weak.upgrade() {
                    page.open_editor(&[]);
                }
            });
            imp.advanced.replace(Some(button));
        }
        imp.advanced.borrow().clone()
    }

    /// Our name on the bus, which the agent's questions go to; for tests.
    pub fn bus_name(&self) -> Option<String> {
        self.imp()
            .nm
            .borrow()
            .as_ref()
            .and_then(NetworkManager::unique_name)
    }

    /// The Wi-Fi switch, for tests.
    pub fn wifi_switch(&self) -> &gtk::Switch {
        &self.imp().power
    }

    /// The adapter drop-down, for tests.
    pub fn adapter_choice(&self) -> &gtk::DropDown {
        &self.imp().adapters
    }

    /// Whether the note about a captive portal shows, and its button; for tests.
    pub fn portal_note(&self) -> (bool, bool) {
        let imp = self.imp();
        (imp.portal.is_visible(), imp.portal_open.is_visible())
    }

    /// The name, status and button label (or the switch's position) of each row of a list:
    /// `known`, `others`, `wired` or `vpns`; for tests.
    pub fn rows(&self, list: &str) -> Vec<(String, String, String)> {
        let imp = self.imp();
        let list = match list {
            "known" => &imp.known,
            "others" => &imp.others,
            "wired" => &imp.wired,
            _ => &imp.vpns,
        };
        let rows = imp.rows.borrow();
        let mut out = Vec::new();
        let mut child = list.first_child();
        while let Some(widget) = child {
            if let Some(row) = rows
                .values()
                .find(|row| row.row.upcast_ref::<gtk::Widget>() == &widget)
            {
                let control = if row.switch.is_visible() {
                    if row.switch.is_active() { "on" } else { "off" }.to_owned()
                } else if row.button.is_visible() {
                    row.button
                        .label()
                        .map(|label| label.to_string())
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                out.push((
                    row.name.text().to_string(),
                    row.status.text().to_string(),
                    control,
                ));
            }
            child = widget.next_sibling();
        }
        out
    }

    /// The button of the row named `name`, for tests.
    pub fn row_button(&self, name: &str) -> Option<gtk::Button> {
        self.find_row(name).map(|row| row.button)
    }

    /// The switch of the row named `name`, for tests.
    pub fn row_switch(&self, name: &str) -> Option<gtk::Switch> {
        self.find_row(name).map(|row| row.switch)
    }

    /// The window of the open dialog: the agent's, else the page's own; for tests.
    pub fn open_dialog(&self) -> Option<gtk::Window> {
        let imp = self.imp();
        imp.prompt
            .borrow()
            .as_ref()
            .map(|(_, dialog)| dialog.window().clone())
            .or_else(|| imp.dialog.borrow().clone())
    }

    /// The text of the error at the top; empty without one. For tests.
    pub fn error_text(&self) -> String {
        let error = &self.imp().error;
        if error.is_visible() {
            error.text().to_string()
        } else {
            String::new()
        }
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

    /// The row named `name`.
    fn find_row(&self, name: &str) -> Option<NetRow> {
        self.imp()
            .rows
            .borrow()
            .values()
            .find(|row| row.name.text() == name)
            .cloned()
    }

    /// Lays out the widgets.
    fn build(&self) {
        let imp = self.imp();
        imp.editor
            .set(delegate::NM_CONNECTION_EDITOR.find().is_some());
        imp.content.set_orientation(gtk::Orientation::Vertical);
        imp.content.append(&imp.missing);
        imp.content.append(&imp.controls);
        imp.missing.set_orientation(gtk::Orientation::Vertical);
        imp.missing.set_visible(false);
        imp.controls.set_visible(false);
        imp.controls.set_orientation(gtk::Orientation::Vertical);

        self.build_portal();
        imp.controls.append(&imp.portal);
        imp.error.set_xalign(0.0);
        imp.error.set_wrap(true);
        imp.error.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        imp.error.set_margin_bottom(12);
        imp.error.add_css_class("error");
        imp.error.set_visible(false);
        imp.controls.append(&imp.error);

        imp.controls.append(&heading("Wi-Fi", true));
        self.build_wifi();
        imp.controls.append(&imp.wifi);
        hint(&imp.wifi_hint);
        imp.wifi_hint
            .set_text("No Wi-Fi adapter. Plug one in; this page updates by itself.");
        imp.controls.append(&imp.wifi_hint);

        imp.known_heading
            .set_label(&"Known networks".to_uppercase());
        style_heading(&imp.known_heading, false);
        imp.controls.append(&imp.known_heading);
        section(&imp.known, "Known networks");
        imp.controls.append(&imp.known);
        hint(&imp.known_hint);
        imp.controls.append(&imp.known_hint);

        imp.others_heading
            .set_orientation(gtk::Orientation::Horizontal);
        imp.others_heading.set_spacing(8);
        imp.others_heading.set_margin_top(24);
        imp.others_heading.set_margin_bottom(8);
        let others = heading("Other networks", false);
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
        section(&imp.others, "Other networks");
        self.build_hidden_row();
        imp.controls.append(&imp.others);
        hint(&imp.others_hint);
        imp.controls.append(&imp.others_hint);

        imp.wired_heading.set_label(&"Wired".to_uppercase());
        style_heading(&imp.wired_heading, false);
        imp.controls.append(&imp.wired_heading);
        section(&imp.wired, "Wired");
        imp.controls.append(&imp.wired);

        imp.vpn_heading.set_label("VPN");
        style_heading(&imp.vpn_heading, false);
        imp.controls.append(&imp.vpn_heading);
        section(&imp.vpns, "VPN");
        imp.controls.append(&imp.vpns);
    }

    /// The note about a captive portal, with the button that opens the login page.
    fn build_portal(&self) {
        let imp = self.imp();
        imp.portal.set_orientation(gtk::Orientation::Horizontal);
        imp.portal.set_spacing(12);
        imp.portal.set_margin_bottom(16);
        imp.portal.set_visible(false);
        let icon = gtk::Image::from_icon_name("dialog-information-symbolic");
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        imp.portal.append(&icon);
        let text = gtk::Label::new(Some("This network wants you to sign in."));
        text.set_xalign(0.0);
        text.set_wrap(true);
        text.set_hexpand(true);
        imp.portal.append(&text);
        imp.portal_open.set_label("Open Login Page");
        imp.portal_open.set_valign(gtk::Align::Center);
        let weak = self.downgrade();
        imp.portal_open.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.open_portal();
            }
        });
        imp.portal.append(&imp.portal_open);
    }

    /// The Wi-Fi section: the adapter drop-down and the switch.
    fn build_wifi(&self) {
        let imp = self.imp();
        section(&imp.wifi, "Wi-Fi");
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
        let adapter_hint = gtk::Label::new(Some("The Wi-Fi adapter this page shows"));
        adapter_hint.add_css_class(style::DIM);
        imp.adapter_row.set_child(Some(&row(
            &gtk::Label::new(Some("Adapter")),
            &adapter_hint,
            &[imp.adapters.upcast_ref()],
        )));
        plain_row(&imp.adapter_row);
        imp.adapter_row.set_visible(false);
        imp.wifi.append(&imp.adapter_row);

        imp.power_subtitle.add_css_class(style::DIM);
        imp.power.set_valign(gtk::Align::Center);
        imp.power
            .update_property(&[gtk::accessible::Property::Label("Wi-Fi")]);
        let weak = self.downgrade();
        imp.power.connect_state_set(move |_, on| {
            weak.upgrade()
                .map_or(glib::Propagation::Proceed, |page| page.wifi_changed(on))
        });
        imp.wifi.append(&list_row(&row(
            &gtk::Label::new(Some("Wi-Fi")),
            &imp.power_subtitle,
            &[imp.power.upcast_ref()],
        )));
        // Its explanation of a hardware block is longer than a line.
        imp.power_subtitle
            .set_ellipsize(gtk::pango::EllipsizeMode::None);
        imp.power_subtitle.set_wrap(true);
    }

    /// The last row of the other networks: "Connect to Hidden Network…".
    fn build_hidden_row(&self) {
        let imp = self.imp();
        let label = gtk::Label::new(Some("Connect to Hidden Network…"));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        let arrow = gtk::Image::from_icon_name("go-next-symbolic");
        arrow.set_accessible_role(gtk::AccessibleRole::Presentation);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        line.append(&label);
        line.append(&arrow);
        imp.hidden_row.set_child(Some(&line));
        imp.hidden_row
            .update_property(&[gtk::accessible::Property::Label(
                "Connect to Hidden Network…",
            )]);
        imp.others.append(&imp.hidden_row);
        let weak = self.downgrade();
        imp.others.connect_row_activated(move |_, row| {
            if let Some(page) = weak.upgrade() {
                if row == &page.imp().hidden_row {
                    page.ask_hidden();
                }
            }
        });
    }

    /// Connects to NetworkManager, shows what it reports and follows its changes and the agent's
    /// questions until the page goes.
    fn follow(&self, bus: Bus) {
        let weak = self.downgrade();
        let task = glib::spawn_future_local(async move {
            let nm = match &bus {
                Bus::System => NetworkManager::connect().await,
                Bus::Address(address) => NetworkManager::connect_to(address).await,
                Bus::Off => Err(Error::Unavailable("no bus".to_owned())),
            };
            let nm = match nm {
                Ok(nm) => nm,
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
            page.imp().nm.replace(Some(nm.clone()));
            page.follow_agent(&nm);
            drop(page);
            // Subscribed before the first snapshot, so no change between the two gets lost.
            let changes = nm.changes().await;
            if !refresh(&weak, &nm).await {
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
                if !refresh(&weak, &nm).await {
                    return;
                }
            }
        });
        self.imp().tasks.borrow_mut().push(task);
    }

    /// Shows the agent's questions as dialogs while the page lives.
    fn follow_agent(&self, nm: &NetworkManager) {
        let events = nm.agent_events();
        let weak = self.downgrade();
        let task = glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                let Some(page) = weak.upgrade() else {
                    return;
                };
                match event {
                    AgentEvent::Prompt(prompt) => page.prompt(prompt),
                    AgentEvent::Cancel(connection) => page.close_prompt_of(&connection),
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
                let keep = selected
                    .filter(|path| snapshot.wifi_devices().any(|device| device.path == *path));
                let selected =
                    keep.or_else(|| snapshot.wifi_devices().next().map(|d| d.path.clone()));
                imp.selected.replace(selected);
                imp.snapshot.replace(Some(snapshot));
            }
            Err(error) => {
                imp.snapshot.replace(None);
                imp.selected.replace(None);
                // NetworkManager is gone, and with it our agent.
                imp.agent.set(false);
                self.close_prompt();
                let (title, text) = match error {
                    Error::Unavailable(_) => (
                        "NetworkManager isn't running",
                        "The network service isn't running. Start it with \
                         `systemctl start NetworkManager`; this page updates by itself."
                            .to_owned(),
                    ),
                    other => ("The network can't be read", other.to_string()),
                };
                self.show_missing("network-offline-symbolic", title, &text, false);
                self.update_scan();
                return;
            }
        }
        self.fill_adapters();
        self.render();
        self.update_scan();
        self.update_agent();
    }

    /// Puts the Wi-Fi devices into the drop-down, the selected one chosen.
    fn fill_adapters(&self) {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return;
        };
        let devices: Vec<&Device> = snapshot.wifi_devices().collect();
        let names: Vec<&str> = devices.iter().map(|d| d.interface.as_str()).collect();
        let current: Vec<String> = (0..imp.adapter_names.n_items())
            .filter_map(|i| imp.adapter_names.string(i).map(|s| s.to_string()))
            .collect();
        let selected = imp.selected.borrow().clone();
        let position = devices
            .iter()
            .position(|device| Some(&device.path) == selected.as_ref())
            .and_then(|i| u32::try_from(i).ok())
            .unwrap_or(0);
        imp.filling.set(true);
        if names != current {
            imp.adapter_names
                .splice(0, imp.adapter_names.n_items(), &names);
        }
        if imp.adapters.selected() != position {
            imp.adapters.set_selected(position);
        }
        imp.filling.set(false);
        imp.adapter_row.set_visible(devices.len() > 1);
    }

    /// The user chose another Wi-Fi adapter.
    fn adapter_chosen(&self) {
        let imp = self.imp();
        if imp.filling.get() {
            return;
        }
        let path = imp.snapshot.borrow().as_ref().and_then(|snapshot| {
            let index = usize::try_from(imp.adapters.selected()).ok()?;
            snapshot.wifi_devices().nth(index).map(|d| d.path.clone())
        });
        if path.is_some() && path != *imp.selected.borrow() {
            imp.selected.replace(path);
            self.show_error("");
            self.render();
            self.update_scan();
        }
    }

    /// Shows the snapshot: the sections and their rows.
    fn render(&self) {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return;
        };
        if !snapshot.networking_enabled {
            self.show_missing(
                "network-offline-symbolic",
                "Networking is turned off",
                "NetworkManager has networking turned off, for Wi-Fi, cables and VPNs alike.",
                true,
            );
            return;
        }
        let wifi_devices: Vec<&Device> = snapshot.wifi_devices().collect();
        let wired: Vec<&Device> = snapshot.ethernet_devices().collect();
        let vpns: Vec<&Connection> = snapshot.vpns().collect();
        if wifi_devices.is_empty() && wired.is_empty() && vpns.is_empty() {
            self.show_missing(
                "network-wired-disconnected-symbolic",
                "No network adapter",
                "Plug in a network adapter; this page updates by itself.",
                false,
            );
            return;
        }
        imp.missing.set_visible(false);
        imp.controls.set_visible(true);

        imp.portal
            .set_visible(snapshot.connectivity == Connectivity::Portal);
        imp.portal_open
            .set_visible(snapshot.connectivity_uri.is_some());

        let running = imp.running.borrow().clone();
        let mut keys: Vec<String> = Vec::new();
        let device = imp
            .selected
            .borrow()
            .as_deref()
            .and_then(|path| snapshot.device(path))
            .cloned();
        let (known, others) = self.render_wifi(snapshot, device.as_ref(), &running, &mut keys);
        let wired_rows = self.render_wired(&wired, &running, &mut keys);
        let vpn_rows = self.render_vpns(snapshot, &vpns, &running, &mut keys);

        let mut rows = imp.rows.borrow_mut();
        rows.retain(|key, row| {
            let keep = keys.contains(key);
            if !keep {
                remove_row(&row.row);
            }
            keep
        });
        let pick = |keys: &[String]| -> Vec<gtk::ListBoxRow> {
            keys.iter()
                .filter_map(|key| rows.get(key).map(|row| row.row.clone()))
                .collect()
        };
        let mut other_rows = pick(&others);
        other_rows.push(imp.hidden_row.clone());
        arrange(&imp.known, &pick(&known));
        arrange(&imp.others, &other_rows);
        arrange(&imp.wired, &pick(&wired_rows));
        arrange(&imp.vpns, &pick(&vpn_rows));
    }

    /// Shows the Wi-Fi section and the networks of `device`; the keys of the known and the other
    /// networks' rows, in order. Adds every key it shows to `keys`.
    fn render_wifi(
        &self,
        snapshot: &Snapshot,
        device: Option<&Device>,
        running: &std::collections::HashMap<String, &'static str>,
        keys: &mut Vec<String>,
    ) -> (Vec<String>, Vec<String>) {
        let imp = self.imp();
        imp.wifi.set_visible(device.is_some());
        imp.wifi_hint.set_visible(device.is_none());
        let on = snapshot.wifi_enabled && device.is_some();
        let networks = device
            .map(|d| snapshot.networks(&d.path))
            .unwrap_or_default();

        imp.power.set_active(snapshot.wifi_enabled);
        imp.power.set_state(snapshot.wifi_enabled);
        imp.power.set_sensitive(snapshot.wifi_hardware_enabled);
        let current = networks.iter().find(|n| n.active.is_some());
        imp.power_subtitle
            .set_text(&if !snapshot.wifi_hardware_enabled {
                "Turned off by a hardware switch or key. Use it to turn Wi-Fi back on.".to_owned()
            } else if !snapshot.wifi_enabled {
                "Off".to_owned()
            } else {
                match current {
                    Some(n)
                        if n.active
                            .as_ref()
                            .is_some_and(|a| a.state == ActiveState::Activated) =>
                    {
                        format!("Connected to “{}”", n.name)
                    }
                    Some(n) => format!("Connecting to “{}”…", n.name),
                    None => "Not connected".to_owned(),
                }
            });

        let searching = on && imp.scan.borrow().is_some();
        let started = searching && !imp.searching.is_visible();
        imp.searching.set_visible(searching);
        if started {
            self.new_spinner();
        }

        let (mut known, mut others) = (Vec::new(), Vec::new());
        if let (true, Some(device)) = (on, device) {
            for network in &networks {
                let key = format!("net:{}", hex(&network.ssid));
                let row = self.ensure_row(&key, false);
                self.update_network_row(&row, network, running.get(&key).copied());
                if network.saved.is_empty() && network.active.is_none() {
                    others.push(key.clone());
                } else {
                    known.push(key.clone());
                }
                keys.push(key);
            }
            for network in snapshot.networks_away(&device.path) {
                let key = format!("away:{}", hex(&network.ssid));
                let row = self.ensure_row(&key, false);
                self.update_away_row(&row, &network, running.get(&key).copied());
                known.push(key.clone());
                keys.push(key);
            }
        }
        let wifi = device.is_some();
        imp.known_heading.set_visible(wifi);
        imp.known.set_visible(on && !known.is_empty());
        set_hint(
            &imp.known_hint,
            if !wifi {
                None
            } else if !on {
                Some("Wi-Fi is off.")
            } else if known.is_empty() {
                Some("No saved networks yet. Pick one under Other networks to connect to it.")
            } else {
                None
            },
        );
        imp.others_heading.set_visible(on);
        imp.others.set_visible(on);
        set_hint(
            &imp.others_hint,
            (on && others.is_empty()).then_some("No other networks found yet."),
        );
        (known, others)
    }

    /// Updates the rows of the wired `devices`; their keys. Adds them to `keys`.
    fn render_wired(
        &self,
        devices: &[&Device],
        running: &std::collections::HashMap<String, &'static str>,
        keys: &mut Vec<String>,
    ) -> Vec<String> {
        let imp = self.imp();
        imp.wired_heading.set_visible(!devices.is_empty());
        imp.wired.set_visible(!devices.is_empty());
        let mut shown = Vec::new();
        for device in devices {
            let key = format!("wired:{}", device.path);
            let row = self.ensure_row(&key, true);
            let title = if devices.len() > 1 {
                format!("Wired ({})", device.interface)
            } else {
                "Wired".to_owned()
            };
            self.update_wired_row(&row, device, &title, running.get(&key).copied());
            shown.push(key.clone());
            keys.push(key);
        }
        shown
    }

    /// Updates the rows of the `vpns`; their keys. Adds them to `keys`.
    fn render_vpns(
        &self,
        snapshot: &Snapshot,
        vpns: &[&Connection],
        running: &std::collections::HashMap<String, &'static str>,
        keys: &mut Vec<String>,
    ) -> Vec<String> {
        let imp = self.imp();
        imp.vpn_heading.set_visible(!vpns.is_empty());
        imp.vpns.set_visible(!vpns.is_empty());
        let mut shown = Vec::new();
        for vpn in vpns {
            let key = format!("vpn:{}", vpn.path);
            let row = self.ensure_row(&key, true);
            self.update_vpn_row(
                &row,
                vpn,
                snapshot.active_of(&vpn.path),
                running.get(&key).copied(),
            );
            shown.push(key.clone());
            keys.push(key);
        }
        shown
    }

    /// The row with `key`, made if it is new: with a switch (`switched`) or a button.
    fn ensure_row(&self, key: &str, switched: bool) -> NetRow {
        if let Some(row) = self.imp().rows.borrow().get(key) {
            return row.clone();
        }
        let row = self.new_row(key, switched);
        self.imp()
            .rows
            .borrow_mut()
            .insert(key.to_owned(), row.clone());
        row
    }

    /// A new row for `key`; the `update_*_row` functions fill it.
    fn new_row(&self, key: &str, switched: bool) -> NetRow {
        let icon = gtk::Image::builder().pixel_size(16).build();
        let name = gtk::Label::new(None);
        let status = gtk::Label::new(None);
        status.add_css_class(style::DIM);
        let button = gtk::Button::new();
        button.set_valign(gtk::Align::Center);
        button.set_visible(!switched);
        let weak = self.downgrade();
        let id = key.to_owned();
        button.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.main_action(&id);
            }
        });
        let switch = gtk::Switch::new();
        switch.set_valign(gtk::Align::Center);
        switch.set_visible(switched);
        let weak = self.downgrade();
        let id = key.to_owned();
        switch.connect_state_set(move |_, on| {
            weak.upgrade()
                .map_or(glib::Propagation::Proceed, |page| page.switched(&id, on))
        });

        let menu_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&menu_box));
        let item = |label: &str, action: fn(&Self, &str)| {
            let item = gtk::Button::with_label(label);
            item.add_css_class("flat");
            if let Some(child) = item.child().and_downcast::<gtk::Label>() {
                child.set_xalign(0.0);
            }
            let weak = self.downgrade();
            let id = key.to_owned();
            item.connect_clicked(glib::clone!(
                #[weak]
                popover,
                move |_| {
                    popover.popdown();
                    if let Some(page) = weak.upgrade() {
                        action(&page, &id);
                    }
                }
            ));
            menu_box.append(&item);
            item
        };
        let details = item("Details…", Self::show_details);
        let edit = item("Edit…", Self::edit);
        let forget = item("Forget…", Self::forget);
        let menu = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .popover(&popover)
            .valign(gtk::Align::Center)
            .tooltip_text("More actions")
            .build();
        menu.add_css_class("flat");

        let line = row(
            &name,
            &status,
            &[button.upcast_ref(), switch.upcast_ref(), menu.upcast_ref()],
        );
        line.prepend(&icon);
        NetRow {
            row: list_row(&line),
            icon,
            name,
            status,
            button,
            switch,
            menu,
            details,
            edit,
            forget,
        }
    }

    /// Fills `row` from the Wi-Fi `network`; `running` is what runs on it.
    fn update_network_row(&self, row: &NetRow, network: &Network, running: Option<&'static str>) {
        let editor = self.imp().editor.get();
        let level = signal_level(network.strength);
        row.icon.set_icon_name(Some(signal_icon(level)));
        row.icon.set_visible(true);
        row.icon
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "Signal {}",
                signal_word(level)
            ))]);
        // Untrusted: plain text, never markup.
        row.name.set_text(&network.name);
        let active = network.active.as_ref().map(|a| a.state);
        let state = running.or(match active {
            Some(ActiveState::Activated) => Some("Connected"),
            Some(ActiveState::Activating | ActiveState::Unknown) => Some("Connecting…"),
            Some(ActiveState::Deactivating) => Some("Disconnecting…"),
            _ if !network.saved.is_empty() => Some("Saved"),
            _ => None,
        });
        let mut status: Vec<&str> = state.into_iter().collect();
        status.push(security_text(network.security));
        let set_up = network.saved.is_empty() && !network.security.supported();
        if set_up && !editor {
            status.push("needs nm-connection-editor");
        }
        row.status.set_text(&status.join(" \u{b7} "));
        let label = match running {
            Some(running) => running,
            None if active.is_some_and(|a| a != ActiveState::Deactivated) => "Disconnect",
            None if set_up => "Set Up…",
            None => "Connect",
        };
        row.button.set_label(label);
        row.button.set_visible(!set_up || editor);
        row.button.set_sensitive(running.is_none());
        row.button
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "{label} {}",
                network.name
            ))]);
        let saved = !network.saved.is_empty();
        update_menu(
            row,
            &network.name,
            [
                active == Some(ActiveState::Activated),
                saved && editor,
                saved,
            ],
            running.is_none(),
        );
    }

    /// Fills `row` from the saved network that isn't around.
    fn update_away_row(&self, row: &NetRow, network: &SavedNetwork, running: Option<&'static str>) {
        // Keeps the names in line with the other rows'.
        row.icon.set_icon_name(None);
        row.icon.set_visible(true);
        row.name.set_text(&network.name);
        row.status.set_text(running.unwrap_or("Not in range"));
        row.button.set_visible(false);
        update_menu(
            row,
            &network.name,
            [false, self.imp().editor.get(), true],
            running.is_none(),
        );
    }

    /// Fills `row` from the wired `device`, titled `title`.
    fn update_wired_row(
        &self,
        row: &NetRow,
        device: &Device,
        title: &str,
        running: Option<&'static str>,
    ) {
        row.icon.set_visible(false);
        row.name.set_text(title);
        let state = running.unwrap_or(match device.state {
            DeviceState::Connected => "Connected",
            DeviceState::Connecting | DeviceState::NeedAuth => "Connecting…",
            DeviceState::Disconnecting => "Disconnecting…",
            DeviceState::Unavailable => "Cable unplugged",
            DeviceState::Failed => "Couldn't connect",
            _ => "Off",
        });
        let mut status = vec![state.to_owned()];
        if running.is_none() && device.state == DeviceState::Connected {
            if let Some(speed) = device.speed {
                status.push(format!("{speed} Mb/s"));
            }
            if let Some(address) = first_address(device.ip4.as_ref()) {
                status.push(address);
            }
        }
        row.status.set_text(&status.join(" \u{b7} "));
        let on = matches!(
            device.state,
            DeviceState::Connected
                | DeviceState::Connecting
                | DeviceState::NeedAuth
                | DeviceState::Failed
        );
        row.switch.set_active(on);
        row.switch.set_state(on);
        // Without a cable there is nothing to switch: the row only says so.
        row.switch
            .set_visible(device.state != DeviceState::Unavailable);
        row.switch.set_sensitive(running.is_none());
        row.switch
            .update_property(&[gtk::accessible::Property::Label(title)]);
        let uuid = self.connection_of(device.active_connection.as_deref());
        update_menu(
            row,
            title,
            [
                device.state == DeviceState::Connected,
                uuid.is_some() && self.imp().editor.get(),
                false,
            ],
            running.is_none(),
        );
    }

    /// Fills `row` from the VPN `vpn`, `active` while it is on.
    fn update_vpn_row(
        &self,
        row: &NetRow,
        vpn: &Connection,
        active: Option<&ActiveConnection>,
        running: Option<&'static str>,
    ) {
        row.icon.set_visible(false);
        row.name.set_text(&vpn.name);
        let state = running.unwrap_or(match active.map(|a| a.state) {
            Some(ActiveState::Activated) => "Connected",
            Some(ActiveState::Activating | ActiveState::Unknown) => "Connecting…",
            Some(ActiveState::Deactivating) => "Disconnecting…",
            _ => "Off",
        });
        let mut status = vec![vpn_kind(&vpn.kind).to_owned(), state.to_owned()];
        if running.is_none() {
            if let Some(address) = first_address(active.and_then(|a| a.ip4.as_ref())) {
                status.push(address);
            }
        }
        row.status.set_text(&status.join(" \u{b7} "));
        let on = active.is_some_and(|a| a.state != ActiveState::Deactivated);
        row.switch.set_active(on);
        row.switch.set_state(on);
        row.switch.set_sensitive(running.is_none());
        row.switch
            .update_property(&[gtk::accessible::Property::Label(&vpn.name)]);
        update_menu(
            row,
            &vpn.name,
            [
                active.is_some_and(|a| a.state == ActiveState::Activated),
                self.imp().editor.get(),
                false,
            ],
            running.is_none(),
        );
    }

    /// What the row with `key` stands for now.
    fn target(&self, key: &str) -> Option<Target> {
        let imp = self.imp();
        let snapshot = imp.snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        if let Some(path) = key.strip_prefix("wired:") {
            return snapshot.device(path).cloned().map(Target::Wired);
        }
        if let Some(path) = key.strip_prefix("vpn:") {
            let vpn = snapshot.connection(path)?.clone();
            let active = snapshot.active_of(path).cloned();
            return Some(Target::Vpn(vpn, active));
        }
        let device = snapshot.device(imp.selected.borrow().as_deref()?)?.clone();
        if let Some(ssid) = key.strip_prefix("net:") {
            return snapshot
                .networks(&device.path)
                .into_iter()
                .find(|n| hex(&n.ssid) == ssid)
                .map(|network| Target::Network(network, device));
        }
        let ssid = key.strip_prefix("away:")?;
        snapshot
            .networks_away(&device.path)
            .into_iter()
            .find(|n| hex(&n.ssid) == ssid)
            .map(Target::Away)
    }

    /// The UUID of the saved connection the active connection at `active` uses.
    fn connection_of(&self, active: Option<&str>) -> Option<String> {
        let snapshot = self.imp().snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        let active = snapshot.active_connection(active?)?;
        snapshot
            .connection(&active.connection)
            .map(|c| c.uuid.clone())
    }

    /// The UUID of the first of the saved connections at `paths`.
    fn uuid_of(&self, paths: &[String]) -> Option<String> {
        let snapshot = self.imp().snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        paths
            .iter()
            .find_map(|path| snapshot.connection(path))
            .map(|c| c.uuid.clone())
    }

    /// What a network row's button does: connect (asking for a password if needed), disconnect,
    /// or set up a network the page can't connect to by itself.
    fn main_action(&self, key: &str) {
        let Some(Target::Network(network, device)) = self.target(key) else {
            return;
        };
        if self.imp().running.borrow().contains_key(key) {
            return;
        }
        if network.active.is_some() {
            let name = network.name.clone();
            self.run(key, "Disconnecting…", move |nm| async move {
                nm.disconnect_device(&device.path)
                    .await
                    .map_err(|error| format!("Couldn't disconnect from “{name}”: {error}"))
            });
        } else if network.saved.is_empty() && !network.security.supported() {
            self.open_editor(delegate::NEW_WIFI);
        } else if network.saved.is_empty() && network.security.needs_password() {
            self.ask_password(key, network, device);
        } else {
            self.connect(key, network, device, None);
        }
    }

    /// Asks for the password of `network` and connects to it.
    fn ask_password(&self, key: &str, network: Network, device: Device) {
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let id = key.to_owned();
        let spec = PasswordSpec {
            title: format!("Connect to “{}”", network.name),
            body: "Enter the password of the Wi-Fi network.",
            security: network.security,
            saved_note: true,
        };
        let dialog = dialogs::password(window.as_ref(), &spec, move |password| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            page.imp().dialog.replace(None);
            if let Some(password) = password {
                page.connect(&id, network, device, Some(password));
            }
        });
        self.imp().dialog.replace(Some(dialog.window().clone()));
    }

    /// Connects `device` to `network`, with `password` for a new one, and waits until it is
    /// connected or fails.
    fn connect(&self, key: &str, network: Network, device: Device, password: Option<String>) {
        let name = network.name.clone();
        self.run_connection(key, move |nm| async move {
            let active = nm
                .connect_network(&device, &network, password.as_deref())
                .await;
            let result = match active {
                Ok(active) => nm.wait_connected(&active, Some(&device.path)).await,
                Err(error) => Err(error),
            };
            result.map_err(|error| format!("Couldn't connect to “{name}”: {error}"))
        });
    }

    /// Asks for a hidden network and connects to it.
    fn ask_hidden(&self) {
        let Some(device) = self.imp().selected.borrow().clone() else {
            return;
        };
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let dialog = dialogs::hidden(window.as_ref(), move |hidden| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            page.imp().dialog.replace(None);
            let Some((ssid, security, password)) = hidden else {
                return;
            };
            page.run_connection(HIDDEN, move |nm| async move {
                let active = nm
                    .connect_hidden(&device, &ssid, security, password.as_deref())
                    .await;
                let result = match active {
                    Ok(active) => nm.wait_connected(&active, Some(&device)).await,
                    Err(error) => Err(error),
                };
                result.map_err(|error| format!("Couldn't connect to “{ssid}”: {error}"))
            });
        });
        self.imp().dialog.replace(Some(dialog.window().clone()));
    }

    /// The handler of a wired or VPN row's switch.
    fn switched(&self, key: &str, on: bool) -> glib::Propagation {
        if self.imp().running.borrow().contains_key(key) {
            return glib::Propagation::Stop;
        }
        match self.target(key) {
            Some(Target::Wired(device)) => {
                let is_on = matches!(
                    device.state,
                    DeviceState::Connected
                        | DeviceState::Connecting
                        | DeviceState::NeedAuth
                        | DeviceState::Failed
                );
                if is_on == on {
                    return glib::Propagation::Proceed;
                }
                if on {
                    self.run_connection(key, move |nm| async move {
                        let result = match nm.connect_device(&device.path).await {
                            Ok(active) => nm.wait_connected(&active, Some(&device.path)).await,
                            Err(error) => Err(error),
                        };
                        result.map_err(|error| format!("Couldn't connect the cable: {error}"))
                    });
                } else {
                    self.run(key, "Disconnecting…", move |nm| async move {
                        nm.disconnect_device(&device.path)
                            .await
                            .map_err(|error| format!("Couldn't disconnect the cable: {error}"))
                    });
                }
            }
            Some(Target::Vpn(vpn, active)) => {
                let is_on = active
                    .as_ref()
                    .is_some_and(|a| a.state != ActiveState::Deactivated);
                if is_on == on {
                    return glib::Propagation::Proceed;
                }
                let name = vpn.name.clone();
                if on {
                    self.run_connection(key, move |nm| async move {
                        let result = match nm.activate(&vpn.path, None).await {
                            Ok(active) => nm.wait_connected(&active, None).await,
                            Err(error) => Err(error),
                        };
                        result.map_err(|error| format!("Couldn't connect “{name}”: {error}"))
                    });
                } else if let Some(active) = active {
                    self.run(key, "Disconnecting…", move |nm| async move {
                        nm.deactivate(&active.path)
                            .await
                            .map_err(|error| format!("Couldn't disconnect “{name}”: {error}"))
                    });
                }
            }
            _ => return glib::Propagation::Proceed,
        }
        // The switch shows the new position; its state follows once NetworkManager reports it.
        glib::Propagation::Stop
    }

    /// Forgets the saved network of the row with `key`, asking first.
    fn forget(&self, key: &str) {
        let (name, saved) = match self.target(key) {
            Some(Target::Network(network, _)) => (network.name, network.saved),
            Some(Target::Away(network)) => (network.name, network.saved),
            _ => return,
        };
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let id = key.to_owned();
        glib::spawn_future_local(async move {
            if !dialogs::confirm_forget(&name, window.as_ref()).await {
                return;
            }
            if let Some(page) = weak.upgrade() {
                page.run(&id, "Forgetting…", move |nm| async move {
                    nm.forget(&saved)
                        .await
                        .map_err(|error| format!("Couldn't forget “{name}”: {error}"))
                });
            }
        });
    }

    /// Opens the saved connection of the row with `key` in nm-connection-editor.
    fn edit(&self, key: &str) {
        let uuid = match self.target(key) {
            Some(Target::Network(network, _)) => self.uuid_of(&network.saved),
            Some(Target::Away(network)) => self.uuid_of(&network.saved),
            Some(Target::Wired(device)) => self.connection_of(device.active_connection.as_deref()),
            Some(Target::Vpn(vpn, _)) => Some(vpn.uuid),
            None => None,
        };
        match uuid.as_deref().and_then(delegate::edit_connection) {
            Some([option, uuid]) => self.open_editor(&[option.as_str(), uuid.as_str()]),
            None => self.show_error("This connection can't be opened in nm-connection-editor"),
        }
    }

    /// Shows the details of the connection of the row with `key`.
    fn show_details(&self, key: &str) {
        let (title, rows) = match self.target(key) {
            Some(Target::Network(network, device)) => {
                let ap = self.imp().snapshot.borrow().as_ref().and_then(|s| {
                    s.access_points
                        .iter()
                        .find(|ap| ap.path == network.access_point)
                        .cloned()
                });
                let mut rows = vec![(
                    "Signal",
                    format!(
                        "{} ({} %)",
                        capitalized(signal_word(signal_level(network.strength))),
                        network.strength
                    ),
                )];
                if let Some(band) = ap.as_ref().and_then(|ap| band(ap.frequency)) {
                    rows.push(("Frequency", band.to_owned()));
                }
                rows.push(("Security", security_name(network.security).to_owned()));
                rows.extend(device_details(&device));
                (format!("“{}”", network.name), rows)
            }
            Some(Target::Wired(device)) => ("Wired".to_owned(), device_details(&device)),
            Some(Target::Vpn(vpn, active)) => {
                let mut rows = vec![("Type", vpn_kind(&vpn.kind).to_owned())];
                if let Some(active) = active {
                    rows.extend(ip_details(active.ip4.as_ref(), active.ip6.as_ref()));
                }
                (vpn.name, rows)
            }
            _ => return,
        };
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        let dialog = dialogs::details(window.as_ref(), &title, &rows);
        dialog.window().connect_destroy(move |_| {
            if let Some(page) = weak.upgrade() {
                page.imp().dialog.replace(None);
            }
        });
        self.imp().dialog.replace(Some(dialog.window().clone()));
    }

    /// Runs `action` for the row with `key`, which shows `label` meanwhile; its error shows at
    /// the top.
    fn run<F, Fut>(&self, key: &str, label: &'static str, action: F)
    where
        F: FnOnce(NetworkManager) -> Fut + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + 'static,
    {
        let Some(nm) = self.imp().nm.borrow().clone() else {
            return;
        };
        let weak = self.downgrade();
        let key = key.to_owned();
        set_running(&weak, &key, Some(label));
        glib::spawn_future_local(async move {
            let result = action(nm.clone()).await;
            set_running(&weak, &key, None);
            if let Some(page) = weak.upgrade() {
                page.show_error(&result.err().unwrap_or_default());
            }
            refresh(&weak, &nm).await;
        });
    }

    /// Runs `action`, a connection the page starts, for the row with `key`: our agent stays
    /// registered until it has finished, so it can ask for a password again.
    fn run_connection<F, Fut>(&self, key: &str, action: F)
    where
        F: FnOnce(NetworkManager) -> Fut + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + 'static,
    {
        let imp = self.imp();
        imp.pending.set(imp.pending.get() + 1);
        self.update_agent();
        let weak = self.downgrade();
        self.run(key, "Connecting…", move |nm| async move {
            let result = action(nm).await;
            if let Some(page) = weak.upgrade() {
                let pending = &page.imp().pending;
                pending.set(pending.get().saturating_sub(1));
                page.update_agent();
            }
            result
        });
    }

    /// The handler of the Wi-Fi switch.
    fn wifi_changed(&self, on: bool) -> glib::Propagation {
        let imp = self.imp();
        let enabled = imp.snapshot.borrow().as_ref().map(|s| s.wifi_enabled);
        let (Some(nm), Some(enabled)) = (imp.nm.borrow().clone(), enabled) else {
            return glib::Propagation::Proceed;
        };
        if enabled == on {
            return glib::Propagation::Proceed;
        }
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let result = nm.set_wifi_enabled(on).await;
            if let Some(page) = weak.upgrade() {
                page.show_error(&match result {
                    Ok(()) => String::new(),
                    Err(error) => format!(
                        "Couldn't turn Wi-Fi {}: {error}",
                        if on { "on" } else { "off" }
                    ),
                });
            }
            refresh(&weak, &nm).await;
        });
        glib::Propagation::Stop
    }

    /// Turns networking back on.
    fn turn_on_networking(&self) {
        let Some(nm) = self.imp().nm.borrow().clone() else {
            return;
        };
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let result = nm.set_networking_enabled(true).await;
            if let Some(page) = weak.upgrade() {
                page.show_error(&match result {
                    Ok(()) => String::new(),
                    Err(error) => format!("Couldn't turn networking on: {error}"),
                });
            }
            refresh(&weak, &nm).await;
        });
    }

    /// Shows the agent's `prompt` as a dialog.
    fn prompt(&self, prompt: SecretPrompt) {
        self.close_prompt();
        let window = self.root().and_downcast::<gtk::Window>();
        let spec = if prompt.retry {
            PasswordSpec {
                title: format!("Wrong password for “{}”", prompt.network),
                body: "The password didn't work, or it has changed.",
                security: if prompt.wpa3 {
                    Security::Wpa3Personal
                } else {
                    Security::Personal
                },
                saved_note: false,
            }
        } else {
            PasswordSpec {
                title: format!("Password for “{}”", prompt.network),
                body: "Enter the password of the Wi-Fi network.",
                security: if prompt.wpa3 {
                    Security::Wpa3Personal
                } else {
                    Security::Personal
                },
                saved_note: false,
            }
        };
        let connection = prompt.connection.clone();
        let weak = self.downgrade();
        let mut prompt = Some(prompt);
        let dialog = dialogs::password(window.as_ref(), &spec, move |password| {
            // Dropping the prompt without an answer cancels it.
            if let (Some(prompt), Some(password)) = (prompt.take(), password) {
                prompt.answer(password);
            }
            if let Some(page) = weak.upgrade() {
                page.imp().prompt.replace(None);
            }
        });
        self.imp().prompt.replace(Some((connection, dialog)));
    }

    /// Closes the agent's dialog, if one is open.
    fn close_prompt(&self) {
        let open = self.imp().prompt.borrow().clone();
        if let Some((_, dialog)) = open {
            dialog.close();
        }
    }

    /// Closes the agent's dialog if it is about the connection at `path`.
    fn close_prompt_of(&self, path: &str) {
        let open = self
            .imp()
            .prompt
            .borrow()
            .as_ref()
            .is_some_and(|(connection, _)| connection == path);
        if open {
            self.close_prompt();
        }
    }

    /// Registers our agent while the page is on screen or a connection it started is on its way,
    /// and unregisters it otherwise.
    fn update_agent(&self) {
        let imp = self.imp();
        let Some(nm) = imp.nm.borrow().clone() else {
            return;
        };
        let wanted = imp.snapshot.borrow().is_some() && (self.is_mapped() || imp.pending.get() > 0);
        if imp.agent.get() == wanted {
            return;
        }
        imp.agent.set(wanted);
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            if wanted {
                if nm.register_agent().await.is_err() {
                    // Tried again with the next snapshot.
                    if let Some(page) = weak.upgrade() {
                        page.imp().agent.set(false);
                    }
                }
            } else {
                // NetworkManager may be gone; then there's nothing to unregister.
                let _ = nm.unregister_agent().await;
            }
        });
    }

    /// Asks for scans while the page is on screen and Wi-Fi is on, and stops otherwise.
    fn update_scan(&self) {
        let imp = self.imp();
        let wanted = self.is_mapped()
            && imp.selected.borrow().is_some()
            && imp
                .snapshot
                .borrow()
                .as_ref()
                .is_some_and(|s| s.wifi_enabled && s.networking_enabled);
        if wanted == imp.scan.borrow().is_some() {
            return;
        }
        if let Some(scan) = imp.scan.take() {
            scan.remove();
            imp.searching.set_visible(false);
            return;
        }
        self.request_scan();
        let weak = self.downgrade();
        let source = glib::timeout_add_seconds_local(SCAN_EVERY, move || {
            if let Some(page) = weak.upgrade() {
                page.request_scan();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        imp.scan.replace(Some(source));
        self.render();
    }

    /// Asks the selected Wi-Fi device for a scan.
    fn request_scan(&self) {
        let imp = self.imp();
        let (Some(nm), Some(device)) = (imp.nm.borrow().clone(), imp.selected.borrow().clone())
        else {
            return;
        };
        glib::spawn_future_local(async move {
            // A failed scan only means an older list; NetworkManager scans by itself too.
            let _ = nm.request_scan(&device).await;
        });
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

    /// Opens NetworkManager's connectivity check address in the browser, where a captive portal
    /// shows its login page.
    fn open_portal(&self) {
        let uri = self
            .imp()
            .snapshot
            .borrow()
            .as_ref()
            .and_then(|s| s.connectivity_uri.clone());
        let Some(uri) = uri else {
            return;
        };
        let window = self.root().and_downcast::<gtk::Window>();
        let weak = self.downgrade();
        gtk::UriLauncher::new(&uri).launch(
            window.as_ref(),
            gtk::gio::Cancellable::NONE,
            move |result| {
                if let (Err(error), Some(page)) = (result, weak.upgrade()) {
                    page.show_error(&format!("The login page can't be opened: {error}"));
                }
            },
        );
    }

    /// Starts nm-connection-editor with `args`.
    fn open_editor(&self, args: &[&str]) {
        match delegate::NM_CONNECTION_EDITOR.launch_with(args) {
            Ok(mut child) => {
                // Waiting reaps it once it ends, so it doesn't stay behind as a zombie.
                glib::spawn_future_local(async move {
                    let _ = child.status().await;
                });
            }
            Err(error) => {
                self.show_error(&format!("nm-connection-editor can't be started: {error}"));
            }
        }
    }

    /// Shows a placeholder instead of the controls, with a "Turn On" button for networking if
    /// `turn_on`.
    fn show_missing(&self, icon: &str, title: &str, text: &str, turn_on: bool) {
        let imp = self.imp();
        while let Some(child) = imp.missing.first_child() {
            imp.missing.remove(&child);
        }
        let placeholder = Placeholder::new(icon, title, text);
        placeholder.set_hexpand(true);
        imp.missing.append(&placeholder);
        if turn_on {
            let button = gtk::Button::with_label("Turn On");
            button.set_halign(gtk::Align::Center);
            button.add_css_class("suggested-action");
            let weak = self.downgrade();
            button.connect_clicked(move |_| {
                if let Some(page) = weak.upgrade() {
                    page.turn_on_networking();
                }
            });
            imp.missing.append(&button);
        }
        imp.controls.set_visible(false);
        imp.missing.set_visible(true);
    }

    /// Shows `message` at the top, or hides it when empty.
    fn show_error(&self, message: &str) {
        let error = &self.imp().error;
        error.set_text(message);
        error.set_visible(!message.is_empty());
    }
}

/// Shows the ⋮ menu's `items` of `row`, named `name`: Details…, Edit… and Forget…, each if
/// true; the menu itself if it has any, usable while nothing runs (`idle`).
fn update_menu(row: &NetRow, name: &str, items: [bool; 3], idle: bool) {
    let [details, edit, forget] = items;
    row.details.set_visible(details);
    row.edit.set_visible(edit);
    row.forget.set_visible(forget);
    row.menu.set_visible(details || edit || forget);
    row.menu.set_sensitive(idle);
    row.menu
        .update_property(&[gtk::accessible::Property::Label(&format!(
            "More actions for {name}"
        ))]);
}

/// Marks what runs on the row with `key` and shows it.
fn set_running(page: &glib::WeakRef<NetworkPage>, key: &str, label: Option<&'static str>) {
    let Some(page) = page.upgrade() else {
        return;
    };
    {
        let mut running = page.imp().running.borrow_mut();
        match label {
            Some(label) => running.insert(key.to_owned(), label),
            None => running.remove(key),
        };
    }
    page.render();
}

/// Reads a new snapshot and shows it on the page. False once the page is gone.
async fn refresh(page: &glib::WeakRef<NetworkPage>, nm: &NetworkManager) -> bool {
    let snapshot = nm.snapshot().await;
    let owner = nm.owner().await;
    let Some(page) = page.upgrade() else {
        return false;
    };
    let imp = page.imp();
    if *imp.owner.borrow() != owner {
        // A new NetworkManager: it doesn't know our agent.
        imp.owner.replace(owner);
        imp.agent.set(false);
    }
    page.show(snapshot);
    true
}

/// The details of `device`: its speed, addresses and hardware address.
fn device_details(device: &Device) -> Vec<(&'static str, String)> {
    let mut rows = Vec::new();
    if let Some(speed) = device.speed {
        rows.push(("Link speed", format!("{speed} Mb/s")));
    }
    rows.extend(ip_details(device.ip4.as_ref(), device.ip6.as_ref()));
    if !device.hw_address.is_empty() {
        rows.push(("Hardware address", device.hw_address.clone()));
    }
    rows.push(("Interface", device.interface.clone()));
    rows
}

/// The addresses, gateways and DNS servers of `ip4` and `ip6`.
fn ip_details(ip4: Option<&IpConfig>, ip6: Option<&IpConfig>) -> Vec<(&'static str, String)> {
    let mut rows = Vec::new();
    for (label, config) in [("IPv4 address", ip4), ("IPv6 address", ip6)] {
        if let Some(config) = config.filter(|c| !c.addresses.is_empty()) {
            rows.push((label, config.addresses.join("\n")));
        }
    }
    let gateways: Vec<&str> = [ip4, ip6]
        .into_iter()
        .flatten()
        .filter_map(|c| c.gateway.as_deref())
        .collect();
    if !gateways.is_empty() {
        rows.push(("Gateway", gateways.join("\n")));
    }
    let dns: Vec<&str> = [ip4, ip6]
        .into_iter()
        .flatten()
        .flat_map(|c| c.dns.iter().map(String::as_str))
        .collect();
    if !dns.is_empty() {
        rows.push(("DNS", dns.join("\n")));
    }
    rows
}

/// The first IPv4 address of `config`, without its prefix length.
fn first_address(config: Option<&IpConfig>) -> Option<String> {
    let address = config?.addresses.first()?;
    Some(address.split('/').next().unwrap_or(address).to_owned())
}

/// `bytes` in hexadecimal, as a key that holds any SSID.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// The symbolic icon of a signal `level` (0 to 4 bars).
fn signal_icon(level: u8) -> &'static str {
    match level {
        4 => "network-wireless-signal-excellent-symbolic",
        3 => "network-wireless-signal-good-symbolic",
        2 => "network-wireless-signal-ok-symbolic",
        1 => "network-wireless-signal-weak-symbolic",
        _ => "network-wireless-signal-none-symbolic",
    }
}

/// A signal `level` in a word, as the icons name it.
fn signal_word(level: u8) -> &'static str {
    match level {
        4 => "excellent",
        3 => "good",
        2 => "ok",
        1 => "weak",
        _ => "none",
    }
}

/// `word` with a capital first letter.
fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// How a row names `security`.
fn security_text(security: Security) -> &'static str {
    match security {
        Security::Open => "Open, not encrypted",
        Security::EnhancedOpen => "Enhanced Open",
        Security::Wep => "WEP, insecure",
        Security::Personal => "WPA2",
        Security::Wpa3Personal => "WPA3",
        Security::Enterprise => "Enterprise (802.1X)",
    }
}

/// How the details name `security`.
fn security_name(security: Security) -> &'static str {
    match security {
        Security::Open => "None",
        Security::EnhancedOpen => "Enhanced Open (OWE)",
        Security::Wep => "WEP",
        Security::Personal => "WPA2 Personal",
        Security::Wpa3Personal => "WPA3 Personal",
        Security::Enterprise => "WPA Enterprise (802.1X)",
    }
}

/// The kind of a VPN connection, as its row names it.
fn vpn_kind(kind: &ConnectionKind) -> &'static str {
    let ConnectionKind::Vpn(service) = kind else {
        return "WireGuard";
    };
    match service.rsplit('.').next().unwrap_or_default() {
        "openvpn" => "OpenVPN",
        "openconnect" => "OpenConnect",
        "vpnc" => "Cisco VPN",
        "pptp" => "PPTP",
        "l2tp" => "L2TP",
        "strongswan" | "libreswan" => "IPsec",
        "fortisslvpn" => "Fortinet SSL VPN",
        "wireguard" => "WireGuard",
        _ => "VPN",
    }
}

/// A section heading as on the pages built from the registry; `first` has no space above it.
fn heading(title: &str, first: bool) -> gtk::Label {
    let label = gtk::Label::new(Some(&title.to_uppercase()));
    style_heading(&label, first);
    label
}

/// Styles `label` as a section heading.
fn style_heading(label: &gtk::Label, first: bool) {
    label.set_xalign(0.0);
    label.set_margin_start(4);
    label.set_margin_bottom(8);
    label.set_margin_top(if first { 0 } else { 24 });
    label.set_accessible_role(gtk::AccessibleRole::Heading);
    label.add_css_class("st-section-heading");
    label.add_css_class(style::DIM);
}

/// Makes `list` a bordered list of a section named `title`.
fn section(list: &gtk::ListBox, title: &str) {
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

/// Puts `rows` into `list` in this order, moving only what is out of place, so a row with the
/// focus keeps it unless it moves.
fn arrange(list: &gtk::ListBox, rows: &[gtk::ListBoxRow]) {
    let mut current = Vec::new();
    let mut child = list.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        current.push(widget);
    }
    let wanted: Vec<gtk::Widget> = rows.iter().map(|row| row.clone().upcast()).collect();
    if current == wanted {
        return;
    }
    let focus = list.root().and_then(|root| root.focus()).filter(|focus| {
        rows.iter()
            .any(|row| focus.is_ancestor(row) || focus == row)
    });
    for row in rows {
        remove_row(row);
    }
    for row in rows {
        list.append(row);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_hold_any_ssid() {
        assert_eq!(hex(b"K\xc3\xb6ln\n"), "4bc3b66c6e0a");
        assert_eq!(hex(b""), "");
    }

    #[test]
    fn rows_name_things_plainly() {
        assert_eq!(capitalized("good"), "Good");
        assert_eq!(signal_icon(4), "network-wireless-signal-excellent-symbolic");
        assert_eq!(
            vpn_kind(&ConnectionKind::Vpn(
                "org.freedesktop.NetworkManager.openvpn".to_owned()
            )),
            "OpenVPN"
        );
        assert_eq!(vpn_kind(&ConnectionKind::WireGuard), "WireGuard");
        assert_eq!(
            first_address(Some(&IpConfig {
                addresses: vec!["10.8.0.2/24".to_owned()],
                gateway: None,
                dns: Vec::new(),
            })),
            Some("10.8.0.2".to_owned())
        );
        let rows = ip_details(
            Some(&IpConfig {
                addresses: vec!["192.168.1.20/24".to_owned()],
                gateway: Some("192.168.1.1".to_owned()),
                dns: vec!["192.168.1.1".to_owned()],
            }),
            None,
        );
        let labels: Vec<&str> = rows.iter().map(|(label, _)| *label).collect();
        assert_eq!(labels, ["IPv4 address", "Gateway", "DNS"]);
    }
}
