// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The NetworkManager backend: its objects on the system bus, read in one call plus the settings
//! of each saved connection, the methods behind the actions, the secret agent, and a stream of
//! its signals.

mod snapshot;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_channel::Receiver;
use futures_lite::{Stream, StreamExt};
use serde::Serialize;
use zbus::message::Type;
use zbus::zvariant::{DynamicType, ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, Message, MessageStream};

pub(super) use self::snapshot::value_bytes;
use self::snapshot::{snapshot, state_reason, text};
use super::agent::{self, Agent, AgentEvent, Settings, VpnState};
use super::{
    ActiveState, Device, Error, Network, Security, Snapshot, check_password, check_ssid, clean,
    ssid_text,
};
use crate::i18n::Text;
use crate::msg;

/// The bus name of NetworkManager.
pub(super) const SERVICE: &str = "org.freedesktop.NetworkManager";
/// Where NetworkManager's object manager is.
const ROOT: &str = "/org/freedesktop";
/// The manager object.
const MANAGER_PATH: &str = "/org/freedesktop/NetworkManager";
/// The manager's interface.
const MANAGER: &str = "org.freedesktop.NetworkManager";
/// The object that takes secret agents.
const AGENT_MANAGER_PATH: &str = "/org/freedesktop/NetworkManager/AgentManager";
/// Its interface.
const AGENT_MANAGER: &str = "org.freedesktop.NetworkManager.AgentManager";
/// The interface of a device.
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
/// The interface of an Ethernet device.
const WIRED: &str = "org.freedesktop.NetworkManager.Device.Wired";
/// The interface of a Wi-Fi device.
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
/// The interface of a device's traffic counters, which change all the time.
const STATISTICS: &str = "org.freedesktop.NetworkManager.Device.Statistics";
/// The interface of an access point.
const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
/// The interface of a saved connection.
const SETTINGS_CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
/// The interface of an active connection.
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
/// The interface of a device's or connection's IPv4 addresses.
const IP4: &str = "org.freedesktop.NetworkManager.IP4Config";
/// The interface of a device's or connection's IPv6 addresses.
const IP6: &str = "org.freedesktop.NetworkManager.IP6Config";
/// The standard interface for properties.
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// The standard interface that lists objects and reports new and removed ones.
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
/// `NM_ACTIVE_CONNECTION_STATE_REASON_NO_SECRETS`.
const NO_SECRETS: u32 = 9;
/// The path NetworkManager uses for "none".
const NONE: &str = "/";

/// Properties that change on their own and that the snapshot doesn't show: a change of only
/// these isn't reported.
const NOISY: &[&str] = &["LastSeen", "LastScan"];

/// The properties of one interface.
type Properties = HashMap<String, OwnedValue>;
/// The interfaces of one object with their properties.
type Interfaces = HashMap<String, Properties>;
/// NetworkManager's objects.
type Objects = HashMap<OwnedObjectPath, Interfaces>;

/// A connection to NetworkManager on the system bus, with our secret agent on it. Cheap to
/// clone; every clone shares the connection.
#[derive(Debug, Clone)]
pub struct NetworkManager {
    connection: Connection,
    agent_events: Receiver<AgentEvent>,
    vpn: Arc<VpnState>,
}

impl NetworkManager {
    /// Connects to the system bus. Succeeds while NetworkManager isn't running too: the change
    /// stream reports when it starts.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] without a system bus.
    pub async fn connect() -> Result<Self, Error> {
        let builder = zbus::connection::Builder::system().map_err(|error| {
            Error::Unavailable(Text::new(msg!("no system bus ({error})")).arg("error", error))
        })?;
        Self::build(builder).await
    }

    /// Connects to the bus at `address` instead of the system bus: a private bus with a stand-in
    /// for NetworkManager, in tests.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] if there is no bus at `address`.
    pub async fn connect_to(address: &str) -> Result<Self, Error> {
        let builder = zbus::connection::Builder::address(address).map_err(|error| {
            Error::Unavailable(
                Text::new(msg!("no bus at {address} ({error})"))
                    .arg("address", address)
                    .arg("error", error),
            )
        })?;
        Self::build(builder).await
    }

    /// Connects with the agent served from the start: an object added to a connection that is
    /// already up can miss the calls that arrive right after.
    async fn build(builder: zbus::connection::Builder<'_>) -> Result<Self, Error> {
        let (events, agent_events) = agent::channel();
        let vpn = Arc::new(VpnState::default());
        let agent = Agent {
            events,
            vpn: Arc::clone(&vpn),
            running: Mutex::default(),
        };
        let connection = builder
            .serve_at(agent::PATH, agent)
            .map_err(failure)?
            .build()
            .await
            .map_err(|error| {
                Error::Unavailable(Text::new(msg!("no bus ({error})")).arg("error", error))
            })?;
        Ok(Self {
            connection,
            agent_events,
            vpn,
        })
    }

    /// Our unique name on the bus (`:1.42`), which NetworkManager sends the agent's questions to.
    pub fn unique_name(&self) -> Option<String> {
        self.connection.unique_name().map(ToString::to_string)
    }

    /// The unique name of the program that owns NetworkManager's bus name now; `None` while it
    /// isn't running. It changes when NetworkManager restarts, which forgets our agent.
    pub async fn owner(&self) -> Option<String> {
        let reply = self
            .connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "GetNameOwner",
                &(SERVICE,),
            )
            .await
            .ok()?;
        reply.body().deserialize::<String>().ok()
    }

    /// NetworkManager's version (`1.52.0`), from its `Version` property; `None` while it isn't
    /// running. Asks who owns its bus name first, so the question doesn't start it.
    pub async fn version(&self) -> Option<String> {
        self.owner().await?;
        let manager = self.properties(MANAGER_PATH, MANAGER).await.ok()?;
        text(&manager, "Version")
    }

    /// What the secret agent asks while it is registered (see
    /// [`register_agent`](Self::register_agent)). Every clone of this connection shares one
    /// queue, so one place should read it.
    pub fn agent_events(&self) -> Receiver<AgentEvent> {
        self.agent_events.clone()
    }

    /// The devices, networks and connections, as NetworkManager reports them now.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while NetworkManager isn't running, [`Error::Failed`] if the reply
    /// can't be read.
    pub async fn snapshot(&self) -> Result<Snapshot, Error> {
        let reply = self
            .connection
            .call_method(
                Some(SERVICE),
                ROOT,
                Some(OBJECT_MANAGER),
                "GetManagedObjects",
                &(),
            )
            .await
            .map_err(failure)?;
        let objects: Objects = reply.body().deserialize().map_err(failure)?;
        let manager = match objects
            .iter()
            .find(|(path, _)| path.as_str() == MANAGER_PATH)
            .and_then(|(_, interfaces)| interfaces.get(MANAGER))
        {
            Some(manager) => manager.clone(),
            None => self.properties(MANAGER_PATH, MANAGER).await?,
        };
        let mut settings = HashMap::new();
        for (path, interfaces) in &objects {
            if interfaces.contains_key(SETTINGS_CONNECTION) {
                // A connection of another user, or one that just went: not ours to show.
                if let Ok(found) = self.settings(path.as_str()).await {
                    settings.insert(path.to_string(), found);
                }
            }
        }
        Ok(snapshot(&manager, &objects, &settings))
    }

    /// Switches Wi-Fi on or off (`nmcli radio wifi`). polkit decides whether the user may.
    ///
    /// # Errors
    ///
    /// [`Error::Denied`] if polkit says no, [`Error::Unavailable`] while NetworkManager isn't
    /// running, [`Error::Failed`] if it refuses otherwise.
    pub async fn set_wifi_enabled(&self, on: bool) -> Result<(), Error> {
        self.call(
            MANAGER_PATH,
            PROPERTIES,
            "Set",
            &(MANAGER, "WirelessEnabled", Value::from(on)),
        )
        .await
    }

    /// Turns networking on or off as a whole (`nmcli networking`). polkit decides whether the
    /// user may.
    ///
    /// # Errors
    ///
    /// [`Error::Denied`] if polkit says no, [`Error::Unavailable`] while NetworkManager isn't
    /// running, [`Error::Failed`] if it refuses otherwise.
    pub async fn set_networking_enabled(&self, on: bool) -> Result<(), Error> {
        match self.call(MANAGER_PATH, MANAGER, "Enable", &(on,)).await {
            // Already in that state.
            Err(Error::Failed(reason)) if reason.english().contains("AlreadyEnabledOrDisabled") => {
                Ok(())
            }
            result => result,
        }
    }

    /// Asks the Wi-Fi `device` to look for networks now. NetworkManager refuses while it scans
    /// already or has just scanned; that counts as success.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while NetworkManager isn't running, [`Error::Failed`] if it fails
    /// otherwise (the device is off, for one).
    pub async fn request_scan(&self, device: &str) -> Result<(), Error> {
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        match self
            .call(device, WIRELESS, "RequestScan", &(options,))
            .await
        {
            Err(Error::Failed(reason)) if reason.english().contains("NotAllowed") => Ok(()),
            result => result,
        }
    }

    /// When the Wi-Fi `device` last finished a scan, in milliseconds since boot (`CLOCK_BOOTTIME`);
    /// -1 if it never did. It changes once a scan asked for with
    /// [`request_scan`](Self::request_scan) has brought a new list.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while NetworkManager isn't running, [`Error::Failed`] if the device
    /// is gone.
    pub async fn last_scan(&self, device: &str) -> Result<i64, Error> {
        let properties = self.properties(device, WIRELESS).await?;
        Ok(properties
            .get("LastScan")
            .and_then(|value| i64::try_from(value).ok())
            .unwrap_or(-1))
    }

    /// Connects `device` to `network`: through a saved connection for it, or else a new one
    /// with `password` (for networks that need one), which NetworkManager keeps for every user
    /// of the machine. Returns the path of the active connection, for
    /// [`wait_connected`](Self::wait_connected).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for a password the network can't take, [`Error::Failed`] for a
    /// network the page can't connect to by itself (WEP, 802.1X) or if NetworkManager refuses,
    /// [`Error::Denied`] if polkit says no, [`Error::Unavailable`] while NetworkManager isn't
    /// running.
    pub async fn connect_network(
        &self,
        device: &Device,
        network: &Network,
        password: Option<&str>,
    ) -> Result<String, Error> {
        let saved = network
            .saved
            .iter()
            .find(|path| device.available_connections.contains(path))
            .or_else(|| network.saved.first());
        if let Some(saved) = saved {
            return self
                .activate_with(saved, &device.path, &network.access_point)
                .await;
        }
        let settings = wifi_settings(&network.ssid, network.security, password, false)?;
        self.add_and_activate(&settings, &device.path, &network.access_point)
            .await
    }

    /// Connects `device` to the hidden network `ssid` with `security` and `password`, as a new
    /// connection that NetworkManager keeps for every user of the machine. Returns the path of
    /// the active connection.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for an SSID or password that can't be used, otherwise as
    /// [`connect_network`](Self::connect_network).
    pub async fn connect_hidden(
        &self,
        device: &str,
        ssid: &str,
        security: Security,
        password: Option<&str>,
    ) -> Result<String, Error> {
        check_ssid(ssid).map_err(Error::Invalid)?;
        let settings = wifi_settings(ssid.as_bytes(), security, password, true)?;
        self.add_and_activate(&settings, device, NONE).await
    }

    /// Activates the saved connection at `connection` (a VPN, for one), on `device` if it needs
    /// one. Returns the path of the active connection.
    ///
    /// # Errors
    ///
    /// [`Error::Denied`] if polkit says no, [`Error::Unavailable`] while NetworkManager isn't
    /// running, [`Error::Failed`] if it refuses or fails.
    pub async fn activate(&self, connection: &str, device: Option<&str>) -> Result<String, Error> {
        self.activate_with(connection, device.unwrap_or(NONE), NONE)
            .await
    }

    /// Connects `device` with the best saved connection NetworkManager has for it, as
    /// `nmcli device connect` does. Returns the path of the active connection.
    ///
    /// # Errors
    ///
    /// As [`activate`](Self::activate).
    pub async fn connect_device(&self, device: &str) -> Result<String, Error> {
        self.activate_with(NONE, device, NONE).await
    }

    /// Disconnects `device`; it doesn't connect again by itself until asked to.
    ///
    /// # Errors
    ///
    /// As [`activate`](Self::activate).
    pub async fn disconnect_device(&self, device: &str) -> Result<(), Error> {
        self.call(device, DEVICE, "Disconnect", &()).await
    }

    /// Ends the active connection at `active` (a VPN, for one).
    ///
    /// # Errors
    ///
    /// As [`activate`](Self::activate).
    pub async fn deactivate(&self, active: &str) -> Result<(), Error> {
        let active = object_path(active)?;
        self.call(MANAGER_PATH, MANAGER, "DeactivateConnection", &(active,))
            .await
    }

    /// Forgets the saved connections at `connections`: deletes them with their passwords.
    ///
    /// # Errors
    ///
    /// The first error, as [`activate`](Self::activate); the connections before it are gone.
    pub async fn forget(&self, connections: &[String]) -> Result<(), Error> {
        for connection in connections {
            self.call(connection, SETTINGS_CONNECTION, "Delete", &())
                .await?;
        }
        Ok(())
    }

    /// Waits until the active connection at `active` is connected. If it ends instead, the
    /// error says why, as far as `device` (the device it ran on) tells, or for a connection
    /// without a device (a VPN) as the connection's own state reason does.
    ///
    /// # Errors
    ///
    /// [`Error::Connect`] with the device's reason, [`Error::Vpn`] with the connection's,
    /// [`Error::Failed`] if it ended without one, [`Error::Unavailable`] while NetworkManager
    /// isn't running.
    pub async fn wait_connected(&self, active: &str, device: Option<&str>) -> Result<(), Error> {
        // The connection says why it ended only in its `StateChanged` signal, just before it
        // goes: subscribed first, so it can't be missed.
        let ended = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE)
            .and_then(|rule| rule.path(active))
            .and_then(|rule| rule.interface(ACTIVE))
            .and_then(|rule| rule.member("StateChanged"))
            .map_err(failure)?
            .build();
        let mut ended = MessageStream::for_match_rule(ended, &self.connection, None)
            .await
            .map_err(failure)?;
        let mut changes = self.changes().await?;
        loop {
            match self.active_state(active).await? {
                Some(ActiveState::Activated) => return Ok(()),
                Some(ActiveState::Activating | ActiveState::Unknown) => {}
                Some(ActiveState::Deactivating | ActiveState::Deactivated) | None => {
                    if device.is_some() {
                        return Err(self.failure_of(device).await);
                    }
                    let mut reason = 0;
                    while let Some(Some(Ok(message))) =
                        futures_lite::future::poll_once(ended.next()).await
                    {
                        if let Ok((4, why)) = message.body().deserialize::<(u32, u32)>() {
                            reason = why;
                        }
                    }
                    return Err(match reason {
                        // Unknown, none, or the user's own doing.
                        0..=2 => Error::Failed(msg!("The connection ended").into()),
                        reason => Error::Vpn(reason),
                    });
                }
            }
            if changes.next().await.is_none() {
                return Err(Error::Unavailable(msg!("NetworkManager stopped").into()));
            }
        }
    }

    /// Registers our secret agent, so NetworkManager asks it for Wi-Fi passwords and VPN
    /// secrets (see [`agent_events`](Self::agent_events)) until
    /// [`unregister_agent`](Self::unregister_agent) or until this connection closes.
    /// NetworkManager forgets it when it restarts.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while NetworkManager isn't running, [`Error::Failed`] if it
    /// refuses.
    pub async fn register_agent(&self) -> Result<(), Error> {
        self.call(
            AGENT_MANAGER_PATH,
            AGENT_MANAGER,
            "RegisterWithCapabilities",
            &(agent::IDENTIFIER, agent::VPN_HINTS),
        )
        .await
    }

    /// Makes the agent look for the VPN plugins' `.name` files in `dirs` instead of where libnm
    /// looks (`NM_VPN_PLUGIN_DIR`, `/usr/lib/NetworkManager/VPN`, `/etc/NetworkManager/VPN`);
    /// for tests, with fake plugins.
    pub fn use_vpn_plugin_dirs(&self, dirs: Vec<PathBuf>) {
        self.vpn.use_dirs(dirs);
    }

    /// Makes the agent sign in to OpenConnect VPNs with `program` instead of the installed
    /// openconnect; for tests, with a fake one.
    pub fn use_openconnect(&self, program: PathBuf) {
        self.vpn.use_openconnect(program);
    }

    /// Connects the VPN with the saved connection at `connection` and waits until it is up
    /// (see [`activate`](Self::activate) and [`wait_connected`](Self::wait_connected)).
    ///
    /// # Errors
    ///
    /// As [`wait_connected`](Self::wait_connected); if it failed for want of secrets, why our
    /// agent couldn't give them: [`Error::NoAuthDialog`], [`Error::NoOpenConnect`],
    /// [`Error::UntrustedCertificate`] or [`Error::SignIn`].
    pub async fn connect_vpn(&self, connection: &str) -> Result<(), Error> {
        self.vpn.take_failure(connection);
        // Without this NetworkManager drops the secrets of our sign-in; a profile it can't
        // change still gets its chance.
        let _ = self.prepare_openconnect(connection).await;
        let active = self.activate(connection, None).await?;
        match self.wait_connected(&active, None).await {
            Err(Error::Vpn(NO_SECRETS)) => Err(self
                .vpn
                .take_failure(connection)
                .unwrap_or(Error::Vpn(NO_SECRETS))),
            other => other,
        }
    }

    /// Unregisters our secret agent.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while NetworkManager isn't running, [`Error::Failed`] if it wasn't
    /// registered.
    pub async fn unregister_agent(&self) -> Result<(), Error> {
        self.call(AGENT_MANAGER_PATH, AGENT_MANAGER, "Unregister", &())
            .await
    }

    /// A stream with one item whenever something the snapshot shows may have changed: a
    /// property, an object that came or went, a saved connection that changed, or
    /// NetworkManager that started or stopped. Read a new [`snapshot`](Self::snapshot) then;
    /// items can come in bursts.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the bus refuses the subscriptions.
    pub async fn changes(&self) -> Result<impl Stream<Item = ()> + Unpin + use<>, Error> {
        let signals = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE)
            .and_then(|rule| rule.path_namespace(ROOT))
            .map_err(failure)?
            .build();
        let owner = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|rule| rule.interface("org.freedesktop.DBus"))
            .and_then(|rule| rule.member("NameOwnerChanged"))
            .and_then(|rule| rule.arg(0, SERVICE))
            .map_err(failure)?
            .build();
        let signals = MessageStream::for_match_rule(signals, &self.connection, None)
            .await
            .map_err(failure)?;
        let owner = MessageStream::for_match_rule(owner, &self.connection, None)
            .await
            .map_err(failure)?;
        Ok(signals
            .or(owner)
            .filter_map(|message| relevant(&message.ok()?).then_some(())))
    }

    /// Activates `connection` on `device` with the access point `specific`.
    async fn activate_with(
        &self,
        connection: &str,
        device: &str,
        specific: &str,
    ) -> Result<String, Error> {
        let args = (
            object_path(connection)?,
            object_path(device)?,
            object_path(specific)?,
        );
        let reply = self
            .connection
            .call_method(
                Some(SERVICE),
                MANAGER_PATH,
                Some(MANAGER),
                "ActivateConnection",
                &args,
            )
            .await
            .map_err(failure)?;
        let active: OwnedObjectPath = reply.body().deserialize().map_err(failure)?;
        Ok(active.to_string())
    }

    /// Saves the new connection `settings` and activates it on `device` with the access point
    /// `specific`.
    async fn add_and_activate(
        &self,
        settings: &NewSettings<'_>,
        device: &str,
        specific: &str,
    ) -> Result<String, Error> {
        let args = (settings, object_path(device)?, object_path(specific)?);
        let reply = self
            .connection
            .call_method(
                Some(SERVICE),
                MANAGER_PATH,
                Some(MANAGER),
                "AddAndActivateConnection",
                &args,
            )
            .await
            .map_err(failure)?;
        let (_, active): (OwnedObjectPath, OwnedObjectPath) =
            reply.body().deserialize().map_err(failure)?;
        Ok(active.to_string())
    }

    /// The state of the active connection at `path`; `None` once it is gone.
    async fn active_state(&self, path: &str) -> Result<Option<ActiveState>, Error> {
        let path = object_path(path)?;
        match self
            .connection
            .call_method(
                Some(SERVICE),
                path,
                Some(PROPERTIES),
                "Get",
                &(ACTIVE, "State"),
            )
            .await
        {
            Ok(reply) => {
                let value: OwnedValue = reply.body().deserialize().map_err(failure)?;
                Ok(u32::try_from(&value).ok().map(ActiveState::from_nm))
            }
            Err(zbus::Error::MethodError(name, _, _))
                if name
                    .as_str()
                    .starts_with("org.freedesktop.DBus.Error.Unknown") =>
            {
                Ok(None)
            }
            Err(error) => Err(failure(error)),
        }
    }

    /// Why a connection on `device` ended, as the device's state reason says.
    async fn failure_of(&self, device: Option<&str>) -> Error {
        let ended = || Error::Failed(msg!("The connection ended").into());
        let Some(device) = device else {
            return ended();
        };
        match self.properties(device, DEVICE).await {
            Ok(properties) => match state_reason(&properties) {
                0 => ended(),
                reason => Error::Connect(reason),
            },
            Err(error) => error,
        }
    }

    /// All properties of `interface` of the object at `path`.
    async fn properties(&self, path: &str, interface: &str) -> Result<Properties, Error> {
        let path = object_path(path)?;
        let reply = self
            .connection
            .call_method(
                Some(SERVICE),
                path,
                Some(PROPERTIES),
                "GetAll",
                &(interface,),
            )
            .await
            .map_err(failure)?;
        reply.body().deserialize().map_err(failure)
    }

    /// Marks the secrets of our OpenConnect sign-in (`cookie`, `gateway`, `gwcert`, `resolve`) as
    /// not saved (flags 2) in the saved connection at `path`, as the plugin's editor does, if it
    /// is an OpenConnect VPN that lacks the flags (made with nmcli, say): NetworkManager drops
    /// secrets an agent gives for keys it takes to be stored. Its other secrets are kept.
    async fn prepare_openconnect(&self, path: &str) -> Result<(), Error> {
        let mut settings = self.settings(path).await?;
        let Some(vpn) = settings.get_mut("vpn") else {
            return Ok(());
        };
        let service = vpn
            .get("service-type")
            .and_then(|value| <&str>::try_from(value).ok());
        if service != Some(super::openconnect::SERVICE) {
            return Ok(());
        }
        let mut data: HashMap<String, String> = vpn
            .get("data")
            .and_then(|value| value.try_clone().ok())
            .and_then(|value| HashMap::try_from(value).ok())
            .unwrap_or_default();
        let mut changed = false;
        for key in super::openconnect::SECRETS {
            if let std::collections::hash_map::Entry::Vacant(flags) =
                data.entry(format!("{key}-flags"))
            {
                flags.insert("2".to_owned());
                changed = true;
            }
        }
        if !changed {
            return Ok(());
        }
        let data = OwnedValue::try_from(Value::from(data)).map_err(|e| failure(e.into()))?;
        vpn.insert("data".to_owned(), data);
        // Update replaces the secrets too: the ones NetworkManager keeps go along.
        if let Ok(mut secrets) = self.secrets(path, "vpn").await {
            if let Some(kept) = secrets.get_mut("vpn").and_then(|vpn| vpn.remove("secrets")) {
                vpn.insert("secrets".to_owned(), kept);
            }
        }
        self.call(path, SETTINGS_CONNECTION, "Update", &(settings,))
            .await
    }

    /// The secrets of `setting` in the saved connection at `path` that NetworkManager keeps.
    async fn secrets(&self, path: &str, setting: &str) -> Result<Settings, Error> {
        let path = object_path(path)?;
        let reply = self
            .connection
            .call_method(
                Some(SERVICE),
                path,
                Some(SETTINGS_CONNECTION),
                "GetSecrets",
                &(setting,),
            )
            .await
            .map_err(failure)?;
        reply.body().deserialize().map_err(failure)
    }

    /// The settings of the saved connection at `path`, without its passwords.
    async fn settings(&self, path: &str) -> Result<Settings, Error> {
        let path = object_path(path)?;
        let reply = self
            .connection
            .call_method(
                Some(SERVICE),
                path,
                Some(SETTINGS_CONNECTION),
                "GetSettings",
                &(),
            )
            .await
            .map_err(failure)?;
        reply.body().deserialize().map_err(failure)
    }

    /// Calls `method` of `interface` on the object at `path`, ignoring what it returns.
    async fn call<B>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<(), Error>
    where
        B: Serialize + DynamicType,
    {
        let path = object_path(path)?;
        self.connection
            .call_method(Some(SERVICE), path, Some(interface), method, body)
            .await
            .map(|_| ())
            .map_err(failure)
    }
}

/// The settings of a new connection, as NetworkManager takes them.
type NewSettings<'a> = HashMap<&'static str, HashMap<&'static str, Value<'a>>>;

/// The settings of a new Wi-Fi connection to `ssid` with `security` and `password`: available
/// to all users (no `permissions`), the password kept by NetworkManager (`psk-flags` 0), as
/// `nmcli device wifi connect` makes them.
fn wifi_settings(
    ssid: &[u8],
    security: Security,
    password: Option<&str>,
    hidden: bool,
) -> Result<NewSettings<'static>, Error> {
    if !security.supported() {
        return Err(Error::Failed(
            msg!("This kind of network needs nm-connection-editor").into(),
        ));
    }
    let mut wireless = HashMap::from([("ssid", Value::from(ssid.to_vec()))]);
    if hidden {
        wireless.insert("hidden", Value::from(true));
    }
    let mut settings = HashMap::from([
        (
            "connection",
            HashMap::from([
                ("type", Value::from("802-11-wireless")),
                ("id", Value::from(ssid_text(ssid))),
            ]),
        ),
        ("802-11-wireless", wireless),
    ]);
    if let Some(key_mgmt) = security.key_mgmt() {
        let mut secured = HashMap::from([("key-mgmt", Value::from(key_mgmt))]);
        if security.needs_password() {
            let password = password.unwrap_or_default();
            check_password(security, password).map_err(Error::Invalid)?;
            secured.insert("psk", Value::from(password.to_owned()));
            secured.insert("psk-flags", Value::from(0u32));
        }
        settings.insert("802-11-wireless-security", secured);
    }
    Ok(settings)
}

/// `path` as an object path.
fn object_path(path: &str) -> Result<ObjectPath<'_>, Error> {
    ObjectPath::try_from(path).map_err(|error| Error::Failed(Text::data(error.to_string())))
}

/// The error a failed D-Bus call stands for.
fn failure(error: zbus::Error) -> Error {
    match error {
        zbus::Error::MethodError(name, description, _) => {
            method_error(name.as_str(), description.as_deref())
        }
        other => Error::Failed(Text::data(other.to_string())),
    }
}

/// The error for the D-Bus error `name` with its `description`.
fn method_error(name: &str, description: Option<&str>) -> Error {
    if crate::dbus::is_gone(name) {
        return Error::Unavailable(msg!("NetworkManager isn't running").into());
    }
    let description = description.map(clean).unwrap_or_default();
    // `org.freedesktop.NetworkManager.PermissionDenied` → `PermissionDenied`
    let short = crate::dbus::short_name(name);
    if matches!(short, "PermissionDenied" | "AccessDenied" | "NotAuthorized") {
        return Error::Denied(description);
    }
    if description.is_empty() {
        Error::Failed(Text::data(short))
    } else {
        Error::Failed(Text::data(format!("{description} ({short})")))
    }
}

/// Whether `message` can change the snapshot: every message of the streams except a
/// `PropertiesChanged` of traffic counters or noisy properties only. NetworkManager's old
/// `PropertiesChanged` signals of its own interfaces repeat the standard ones and are skipped.
fn relevant(message: &Message) -> bool {
    let header = message.header();
    if header.member().map(zbus::names::MemberName::as_str) != Some("PropertiesChanged") {
        return true;
    }
    if header.interface().map(zbus::names::InterfaceName::as_str) != Some(PROPERTIES) {
        return false;
    }
    let Ok((interface, changed, invalidated)) = message
        .body()
        .deserialize::<(String, Properties, Vec<String>)>()
    else {
        return false;
    };
    relevant_change(&interface, changed.keys().chain(&invalidated))
}

/// Whether a change of the properties `names` of `interface` can change the snapshot.
fn relevant_change<'a>(interface: &str, mut names: impl Iterator<Item = &'a String>) -> bool {
    interface != STATISTICS && names.any(|name| !NOISY.contains(&name.as_str()))
}

#[cfg(test)]
mod tests;
