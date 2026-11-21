// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Network page: Wi-Fi with its networks, the wired devices and the VPNs, live from
//! NetworkManager.
//!
//! The page reads a new snapshot whenever NetworkManager reports a change, at most every
//! [`THROTTLE`], and updates its rows in place. By default it asks for a scan every
//! [`SCAN_EVERY`] while it is on screen and Wi-Fi is on; with the preference set to manual, only
//! for one with _Search_, which shows only then. A search stops once the page leaves the screen, Wi-Fi goes off or another
//! adapter is chosen. While it is on screen, and until a connection it started has
//! finished, our secret agent is registered, so a wrong or changed password brings up a dialog.
//!
//! SSIDs and connection names are untrusted and only ever set as plain label text.

mod actions;
mod dialogs;
mod prompts;
mod rows;
mod scan;
mod text;

use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::delegate;
use sway_things_core::network::{
    ActiveConnection, AgentEvent, Connection, Device, Error, Network, NetworkManager, SavedNetwork,
    Snapshot,
};

use super::{Placeholder, search_button};
use crate::i18n::{msg, tr, trf};
use crate::style;
use crate::widgets::list::{
    heading, hint, labels_in, list_row, plain_row, row, section, style_heading,
};
use crate::widgets::live::follow_changes;

/// How long to collect a burst of changes before reading the snapshot again.
const THROTTLE: Duration = Duration::from_millis(200);
/// How often to ask for a scan while the page is on screen, as GNOME Settings does.
const SCAN_EVERY: u32 = 15;
/// How often to look whether the scan asked for with _Search_ is done.
const SCAN_POLL: Duration = Duration::from_millis(500);
/// How long a search asked for with _Search_ waits at most for the new list.
const SCAN_TIMEOUT: Duration = Duration::from_secs(15);
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
        /// Shown instead of the known and other networks while Wi-Fi is off.
        pub off: gtk::Box,
        /// Whether `off` shows a hardware block; `None` while it is empty.
        pub off_shown: Cell<Option<bool>>,
        pub known_heading: gtk::Label,
        pub known: gtk::ListBox,
        pub known_hint: gtk::Label,
        pub others_heading: gtk::Box,
        pub searching: gtk::Box,
        pub searching_label: gtk::Label,
        /// _Search_, or _Stop_ while a scan asked for with it runs.
        pub search: gtk::Button,
        /// Whether the page asks for scans only after _Search_.
        pub manual: Cell<bool>,
        /// The scan asked for with _Search_, until NetworkManager has the new list.
        pub requested: RefCell<Option<glib::JoinHandle<()>>>,
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
            if let Some(search) = self.requested.take() {
                search.abort();
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
            let button = gtk::Button::with_label(tr(msg!("Advanced…")));
            button.set_tooltip_text(Some(tr(msg!(
                "Open nm-connection-editor for VPNs, 802.1X networks and more"
            ))));
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

    /// Whether a turning spinner shows, next to the heading or in the Search button; for
    /// tests.
    pub fn is_searching(&self) -> bool {
        let imp = self.imp();
        let heading = imp.searching.is_visible()
            && imp
                .spinner
                .borrow()
                .as_ref()
                .is_some_and(gtk::Spinner::is_spinning);
        heading || search_button::is_spinning(&imp.search)
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

        imp.controls.append(&heading(tr(msg!("Wi-Fi")), true));
        self.build_wifi();
        imp.controls.append(&imp.wifi);
        hint(&imp.wifi_hint);
        imp.wifi_hint.set_text(tr(msg!(
            "No Wi-Fi adapter. Plug one in; this page updates by itself."
        )));
        imp.controls.append(&imp.wifi_hint);

        imp.off.set_orientation(gtk::Orientation::Vertical);
        imp.off.set_margin_top(24);
        imp.off.set_visible(false);
        imp.controls.append(&imp.off);

        imp.known_heading
            .set_label(&tr(msg!("Known networks")).to_uppercase());
        style_heading(&imp.known_heading, false);
        imp.controls.append(&imp.known_heading);
        section(&imp.known, tr(msg!("Known networks")));
        imp.controls.append(&imp.known);
        hint(&imp.known_hint);
        imp.controls.append(&imp.known_hint);

        imp.others_heading
            .set_orientation(gtk::Orientation::Horizontal);
        // The heading and the spinner, with the margins of every heading line; the Search button
        // lies over the line (see `search_button::over`).
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.set_margin_top(24);
        line.set_margin_bottom(8);
        let others = heading(tr(msg!("Other networks")), false);
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
        imp.controls.append(&imp.others_heading);
        section(&imp.others, tr(msg!("Other networks")));
        self.build_hidden_row();
        imp.controls.append(&imp.others);
        hint(&imp.others_hint);
        imp.controls.append(&imp.others_hint);

        imp.wired_heading
            .set_label(&tr(msg!("Wired")).to_uppercase());
        style_heading(&imp.wired_heading, false);
        imp.controls.append(&imp.wired_heading);
        section(&imp.wired, tr(msg!("Wired")));
        imp.controls.append(&imp.wired);

        imp.vpn_heading.set_label(tr(msg!("VPN")));
        style_heading(&imp.vpn_heading, false);
        imp.controls.append(&imp.vpn_heading);
        section(&imp.vpns, tr(msg!("VPN")));
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
        let text = gtk::Label::new(Some(tr(msg!("This network wants you to sign in."))));
        text.set_xalign(0.0);
        text.set_wrap(true);
        text.set_hexpand(true);
        imp.portal.append(&text);
        imp.portal_open.set_label(tr(msg!("Open Login Page")));
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
        section(&imp.wifi, tr(msg!("Wi-Fi")));
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
        let adapter_hint = gtk::Label::new(Some(tr(msg!("The Wi-Fi adapter this page shows"))));
        adapter_hint.add_css_class(style::DIM);
        imp.adapter_row.set_child(Some(&row(
            &gtk::Label::new(Some(tr(msg!("Adapter")))),
            &adapter_hint,
            &[imp.adapters.upcast_ref()],
        )));
        plain_row(&imp.adapter_row);
        imp.adapter_row.set_visible(false);
        imp.wifi.append(&imp.adapter_row);

        imp.power_subtitle.add_css_class(style::DIM);
        imp.power.set_valign(gtk::Align::Center);
        imp.power
            .update_property(&[gtk::accessible::Property::Label(tr(msg!("Wi-Fi")))]);
        let weak = self.downgrade();
        imp.power.connect_state_set(move |_, on| {
            weak.upgrade()
                .map_or(glib::Propagation::Proceed, |page| page.wifi_changed(on))
        });
        imp.wifi.append(&list_row(&row(
            &gtk::Label::new(Some(tr(msg!("Wi-Fi")))),
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
        let label = gtk::Label::new(Some(tr(msg!("Connect to Hidden Network…"))));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        let arrow = gtk::Image::from_icon_name("go-next-symbolic");
        arrow.set_accessible_role(gtk::AccessibleRole::Presentation);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        line.append(&label);
        line.append(&arrow);
        imp.hidden_row.set_child(Some(&line));
        imp.hidden_row
            .update_property(&[gtk::accessible::Property::Label(tr(msg!(
                "Connect to Hidden Network…"
            )))]);
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
                Bus::Off => Err(Error::Unavailable(msg!("no bus").into())),
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
            follow_changes(changes, THROTTLE, || refresh(&weak, &nm)).await;
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
                    // Not asked for yet: dropping the prompt cancels it.
                    AgentEvent::Vpn(prompt) => drop(prompt),
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
                        tr(msg!("NetworkManager isn't running")),
                        tr(msg!(
                            "The network service isn't running. Start it with `systemctl start \
                             NetworkManager`; this page updates by itself."
                        ))
                        .to_owned(),
                    ),
                    other => (
                        tr(msg!("The network can't be read")),
                        other.text().translated(),
                    ),
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
            self.end_search();
            imp.selected.replace(path);
            self.show_error("");
            self.render();
            self.update_scan();
        }
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
                    page.show_error(&trf(
                        msg!("The login page can't be opened: {error}"),
                        &[("error", &error)],
                    ));
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
                self.show_error(&trf(
                    msg!("nm-connection-editor can't be started: {error}"),
                    &[("error", &error)],
                ));
            }
        }
    }

    /// Shows a placeholder instead of the controls, with a "Turn On" button for networking if
    /// `turn_on`.
    /// Shows the placeholder for Wi-Fi that is off (with whether a hardware switch keeps it off)
    /// instead of the known and other networks, or nothing for `None`.
    fn show_off(&self, off: Option<bool>) {
        let imp = self.imp();
        imp.off.set_visible(off.is_some());
        if imp.off_shown.replace(off) == off {
            return;
        }
        while let Some(child) = imp.off.first_child() {
            imp.off.remove(&child);
        }
        let Some(hard) = off else {
            return;
        };
        let text = tr(if hard {
            msg!("It's turned off by a hardware switch or key. Use it to turn Wi-Fi back on.")
        } else {
            msg!("Turn it on with the switch above to see and connect to networks.")
        });
        let placeholder = Placeholder::new(
            "network-wireless-disabled-symbolic",
            tr(msg!("Wi-Fi is off")),
            text,
        );
        placeholder.set_hexpand(true);
        imp.off.append(&placeholder);
    }

    /// The texts of the placeholder shown while Wi-Fi is off; `None` while the networks show.
    pub fn off_placeholder(&self) -> Option<Vec<String>> {
        let off = &self.imp().off;
        off.is_visible().then(|| labels_in(off.upcast_ref()))
    }

    fn show_missing(&self, icon: &str, title: &str, text: &str, turn_on: bool) {
        let imp = self.imp();
        while let Some(child) = imp.missing.first_child() {
            imp.missing.remove(&child);
        }
        let placeholder = Placeholder::new(icon, title, text);
        placeholder.set_hexpand(true);
        imp.missing.append(&placeholder);
        if turn_on {
            let button = gtk::Button::with_label(tr(msg!("Turn On")));
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
        .update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("More actions for {network}"),
            &[("network", &name)],
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
