// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The NetworkManager backend: its objects on the system bus, read in one call plus the settings
//! of each saved connection, the methods behind the actions, the secret agent, and a stream of
//! its signals.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use async_channel::Receiver;
use futures_lite::{Stream, StreamExt};
use serde::Serialize;
use zbus::message::Type;
use zbus::zvariant::{DynamicType, ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, Message, MessageStream};

use super::agent::{self, Agent, AgentEvent, Settings};
use super::{
    AccessPoint, ActiveConnection, ActiveState, Connection as Saved, ConnectionKind, Connectivity,
    Device, DeviceKind, DeviceState, Error, IpConfig, Network, Security, Snapshot, check_password,
    check_ssid, clean, ssid_text,
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
        let connection = builder
            .serve_at(agent::PATH, Agent { events })
            .map_err(failure)?
            .build()
            .await
            .map_err(|error| {
                Error::Unavailable(Text::new(msg!("no bus ({error})")).arg("error", error))
            })?;
        Ok(Self {
            connection,
            agent_events,
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

    /// Registers our secret agent, so NetworkManager asks it for Wi-Fi passwords (see
    /// [`agent_events`](Self::agent_events)) until [`unregister_agent`](Self::unregister_agent)
    /// or until this connection closes. NetworkManager forgets it when it restarts.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while NetworkManager isn't running, [`Error::Failed`] if it
    /// refuses.
    pub async fn register_agent(&self) -> Result<(), Error> {
        self.call(
            AGENT_MANAGER_PATH,
            AGENT_MANAGER,
            "Register",
            &(agent::IDENTIFIER,),
        )
        .await
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
    if matches!(
        name,
        "org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner"
    ) {
        return Error::Unavailable(msg!("NetworkManager isn't running").into());
    }
    let description = description.map(clean).unwrap_or_default();
    // `org.freedesktop.NetworkManager.PermissionDenied` → `PermissionDenied`
    let short = name.rsplit('.').next().unwrap_or(name);
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

/// The snapshot of NetworkManager's `manager` properties, its `objects` and the `settings` of
/// its saved connections (by path).
fn snapshot(
    manager: &Properties,
    objects: &Objects,
    settings: &HashMap<String, Settings>,
) -> Snapshot {
    let object = |path: &str| {
        objects
            .iter()
            .find(|(p, _)| p.as_str() == path)
            .map(|(_, interfaces)| interfaces)
    };
    let ip = |path: Option<String>, interface: &str| {
        path.and_then(|path| {
            object(&path)?
                .get(interface)
                .map(|p| ip_config(p, interface))
        })
    };

    let mut devices = Vec::new();
    let mut access_points = Vec::new();
    for (path, interfaces) in objects {
        let Some(properties) = interfaces.get(DEVICE) else {
            continue;
        };
        let wired = interfaces.get(WIRED);
        let wireless = interfaces.get(WIRELESS);
        let kind = DeviceKind::from_nm(number(properties, "DeviceType"));
        let hw_address = text(properties, "HwAddress")
            .or_else(|| wired.and_then(|p| text(p, "HwAddress")))
            .or_else(|| wireless.and_then(|p| text(p, "HwAddress")))
            .unwrap_or_default();
        let speed = match kind {
            DeviceKind::Wifi => wireless.map(|p| number(p, "Bitrate") / 1000),
            _ => wired.map(|p| number(p, "Speed")),
        }
        .filter(|speed| *speed > 0);
        if let Some(wireless) = wireless {
            for ap in paths(wireless, "AccessPoints") {
                if let Some(ap) = object(&ap)
                    .and_then(|interfaces| interfaces.get(ACCESS_POINT))
                    .map(|p| access_point(&ap, path.as_str(), p))
                {
                    access_points.push(ap);
                }
            }
        }
        devices.push(Device {
            path: path.to_string(),
            interface: text(properties, "Interface").unwrap_or_default(),
            kind,
            state: DeviceState::from_nm(number(properties, "State")),
            reason: state_reason(properties),
            managed: flag(properties, "Managed"),
            active_connection: object_ref(properties, "ActiveConnection"),
            hw_address,
            speed,
            carrier: wired.is_some_and(|p| flag(p, "Carrier")),
            access_point: wireless.and_then(|p| object_ref(p, "ActiveAccessPoint")),
            available_connections: paths(properties, "AvailableConnections"),
            ip4: ip(object_ref(properties, "Ip4Config"), IP4),
            ip6: ip(object_ref(properties, "Ip6Config"), IP6),
        });
    }
    devices.sort_by(|a, b| (&a.interface, &a.path).cmp(&(&b.interface, &b.path)));
    access_points.sort_by(|a, b| a.path.cmp(&b.path));

    let mut connections: Vec<Saved> = settings
        .iter()
        .map(|(path, settings)| saved(path, settings))
        .collect();
    connections.sort_by_cached_key(|c| (c.name.to_lowercase(), c.path.clone()));

    let mut active: Vec<ActiveConnection> = objects
        .iter()
        .filter_map(|(path, interfaces)| {
            let properties = interfaces.get(ACTIVE)?;
            Some(ActiveConnection {
                path: path.to_string(),
                connection: object_ref(properties, "Connection").unwrap_or_default(),
                state: ActiveState::from_nm(number(properties, "State")),
                devices: paths(properties, "Devices"),
                ip4: ip(object_ref(properties, "Ip4Config"), IP4),
                ip6: ip(object_ref(properties, "Ip6Config"), IP6),
            })
        })
        .collect();
    active.sort_by(|a, b| a.path.cmp(&b.path));

    Snapshot {
        networking_enabled: flag(manager, "NetworkingEnabled"),
        wifi_enabled: flag(manager, "WirelessEnabled"),
        wifi_hardware_enabled: flag(manager, "WirelessHardwareEnabled"),
        connectivity: Connectivity::from_nm(number(manager, "Connectivity")),
        connectivity_uri: text(manager, "ConnectivityCheckUri"),
        devices,
        access_points,
        connections,
        active,
    }
}

/// The access point at `path`, seen by `device`, with these `properties`.
fn access_point(path: &str, device: &str, properties: &Properties) -> AccessPoint {
    AccessPoint {
        path: path.to_owned(),
        device: device.to_owned(),
        ssid: properties.get("Ssid").and_then(bytes).unwrap_or_default(),
        strength: properties
            .get("Strength")
            .and_then(|value| u8::try_from(value).ok())
            .unwrap_or(0)
            .min(100),
        security: Security::from_flags(
            number(properties, "Flags"),
            number(properties, "WpaFlags"),
            number(properties, "RsnFlags"),
        ),
        frequency: number(properties, "Frequency"),
        bssid: text(properties, "HwAddress").unwrap_or_default(),
    }
}

/// The saved connection at `path` with these `settings`.
fn saved(path: &str, settings: &Settings) -> Saved {
    let get = |setting: &str, key: &str| settings.get(setting).and_then(|s| s.get(key));
    let string = |setting: &str, key: &str| {
        get(setting, key)
            .and_then(|value| <&str>::try_from(value).ok())
            .map(ToOwned::to_owned)
    };
    let kind = match string("connection", "type").as_deref() {
        Some("802-11-wireless") => ConnectionKind::Wifi(
            get("802-11-wireless", "ssid")
                .and_then(bytes)
                .unwrap_or_default(),
        ),
        Some("802-3-ethernet") => ConnectionKind::Ethernet,
        Some("vpn") => ConnectionKind::Vpn(string("vpn", "service-type").unwrap_or_default()),
        Some("wireguard") => ConnectionKind::WireGuard,
        other => ConnectionKind::Other(other.unwrap_or_default().to_owned()),
    };
    Saved {
        path: path.to_owned(),
        name: string("connection", "id")
            .map(|name| clean(&name))
            .unwrap_or_default(),
        uuid: string("connection", "uuid").unwrap_or_default(),
        kind,
    }
}

/// The addresses of an `IP4Config` or `IP6Config` object (`interface`) with these `properties`.
fn ip_config(properties: &Properties, interface: &str) -> IpConfig {
    let addresses = dicts(properties, "AddressData")
        .iter()
        .filter_map(|data| {
            let address = data.get("address").and_then(|v| <&str>::try_from(v).ok())?;
            let prefix = data
                .get("prefix")
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0);
            Some(format!("{}/{prefix}", clean(address)))
        })
        .collect();
    let gateway = text(properties, "Gateway");
    let mut dns: Vec<String> = dicts(properties, "NameserverData")
        .iter()
        .filter_map(|data| data.get("address").and_then(|v| <&str>::try_from(v).ok()))
        .map(clean)
        .collect();
    if dns.is_empty() {
        // NetworkManager before 1.14, and IPv6: the servers as numbers or bytes.
        dns = match properties.get("Nameservers").map(|v| &**v) {
            Some(Value::Array(array)) if interface == IP4 => array
                .iter()
                .filter_map(|v| u32::try_from(v).ok())
                // In network byte order.
                .map(|n| Ipv4Addr::from(n.to_ne_bytes()).to_string())
                .collect(),
            Some(Value::Array(array)) => array
                .iter()
                .filter_map(value_bytes)
                .filter_map(|b| <[u8; 16]>::try_from(b).ok())
                .map(|b| Ipv6Addr::from(b).to_string())
                .collect(),
            _ => Vec::new(),
        };
    }
    IpConfig {
        addresses,
        gateway,
        dns,
    }
}

/// The device state reason of a device's `properties`: the second number of `StateReason`.
fn state_reason(properties: &Properties) -> u32 {
    match properties.get("StateReason").map(|v| &**v) {
        Some(Value::Structure(reason)) => reason
            .fields()
            .get(1)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0),
        _ => 0,
    }
}

/// `value` as bytes, if it is an array of bytes.
fn bytes(value: &OwnedValue) -> Option<Vec<u8>> {
    value_bytes(value)
}

/// `value` as bytes, if it is an array of bytes.
pub(super) fn value_bytes(value: &Value<'_>) -> Option<Vec<u8>> {
    match value {
        Value::Array(array) => array.iter().map(|v| u8::try_from(v).ok()).collect(),
        Value::Value(inner) => value_bytes(inner),
        _ => None,
    }
}

/// The array of dictionaries `name` (`aa{sv}`).
fn dicts(properties: &Properties, name: &str) -> Vec<HashMap<String, OwnedValue>> {
    match properties.get(name).map(|v| &**v) {
        Some(Value::Array(array)) => array
            .iter()
            .filter_map(|v| match v {
                Value::Dict(dict) => {
                    HashMap::<String, OwnedValue>::try_from(dict.try_clone().ok()?).ok()
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The array of object paths `name`, without NetworkManager's "none".
fn paths(properties: &Properties, name: &str) -> Vec<String> {
    match properties.get(name).map(|v| &**v) {
        Some(Value::Array(array)) => array
            .iter()
            .filter_map(|v| match v {
                Value::ObjectPath(path) if path.as_str() != NONE => Some(path.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The object path property `name`; `None` if missing or "none".
fn object_ref(properties: &Properties, name: &str) -> Option<String> {
    let path = <&ObjectPath<'_>>::try_from(properties.get(name)?).ok()?;
    (path.as_str() != NONE).then(|| path.to_string())
}

/// The string property `name`, cleaned of control characters; `None` if missing or empty.
fn text(properties: &Properties, name: &str) -> Option<String> {
    let value = properties.get(name)?;
    let text = clean(<&str>::try_from(value).ok()?);
    (!text.is_empty()).then_some(text)
}

/// The number property `name`; 0 if missing.
fn number(properties: &Properties, name: &str) -> u32 {
    properties
        .get(name)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

/// The boolean property `name`; false if missing.
fn flag(properties: &Properties, name: &str) -> bool {
    properties
        .get(name)
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use zbus::zvariant::{Array, StructureBuilder};

    use super::*;

    fn value(value: impl Into<Value<'static>>) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    fn path(path: &str) -> OwnedObjectPath {
        OwnedObjectPath::try_from(path).unwrap()
    }

    fn object_path_value(p: &str) -> OwnedValue {
        value(ObjectPath::try_from(p.to_owned()).unwrap())
    }

    fn paths_value(paths: &[&str]) -> OwnedValue {
        let paths: Vec<ObjectPath<'static>> = paths
            .iter()
            .map(|p| ObjectPath::try_from((*p).to_owned()).unwrap())
            .collect();
        value(Array::from(paths))
    }

    fn props(entries: Vec<(&str, OwnedValue)>) -> Properties {
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect()
    }

    fn wifi_settings_of(id: &str, ssid: &[u8]) -> Settings {
        HashMap::from([
            (
                "connection".to_owned(),
                props(vec![
                    ("id", value(id.to_owned())),
                    ("uuid", value("u-1")),
                    ("type", value("802-11-wireless")),
                ]),
            ),
            (
                "802-11-wireless".to_owned(),
                props(vec![("ssid", value(ssid.to_vec()))]),
            ),
        ])
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one snapshot with every kind of object"
    )]
    fn the_snapshot_has_devices_networks_and_connections() {
        let reason = StructureBuilder::new()
            .add_field(100u32)
            .add_field(0u32)
            .build()
            .unwrap();
        let wifi = HashMap::from([
            (
                DEVICE.to_owned(),
                props(vec![
                    ("Interface", value("wlan0")),
                    ("DeviceType", value(2u32)),
                    ("State", value(100u32)),
                    ("StateReason", value(reason)),
                    ("Managed", value(true)),
                    ("ActiveConnection", object_path_value("/active/0")),
                    ("Ip4Config", object_path_value("/ip4/1")),
                    ("Ip6Config", object_path_value("/")),
                    ("AvailableConnections", paths_value(&["/settings/1"])),
                ]),
            ),
            (
                WIRELESS.to_owned(),
                props(vec![
                    ("HwAddress", value("11:22:33:44:55:66")),
                    ("Bitrate", value(866_000u32)),
                    ("AccessPoints", paths_value(&["/ap/1", "/ap/2"])),
                    ("ActiveAccessPoint", object_path_value("/ap/1")),
                ]),
            ),
        ]);
        let ethernet = HashMap::from([
            (
                DEVICE.to_owned(),
                props(vec![
                    ("Interface", value("enp3s0")),
                    ("DeviceType", value(1u32)),
                    ("State", value(20u32)),
                    ("Managed", value(true)),
                    ("ActiveConnection", object_path_value("/")),
                ]),
            ),
            (
                WIRED.to_owned(),
                props(vec![("Speed", value(0u32)), ("Carrier", value(false))]),
            ),
        ]);
        let ap = |ssid: &str, strength: u8, rsn: u32| {
            HashMap::from([(
                ACCESS_POINT.to_owned(),
                props(vec![
                    ("Ssid", value(ssid.as_bytes().to_vec())),
                    ("Strength", value(strength)),
                    ("Flags", value(u32::from(rsn != 0))),
                    ("WpaFlags", value(0u32)),
                    ("RsnFlags", value(rsn)),
                    ("Frequency", value(5180u32)),
                ]),
            )])
        };
        let address = |a: &str, prefix: u32| {
            HashMap::from([
                ("address".to_owned(), Value::from(a.to_owned())),
                ("prefix".to_owned(), Value::from(prefix)),
            ])
        };
        let ip4 = HashMap::from([(
            IP4.to_owned(),
            props(vec![
                ("AddressData", value(vec![address("192.168.1.20", 24)])),
                ("Gateway", value("192.168.1.1")),
                // 9.9.9.9 and 1.1.1.1 in network byte order.
                (
                    "Nameservers",
                    value(vec![
                        u32::from_ne_bytes([9, 9, 9, 9]),
                        u32::from_ne_bytes([1, 1, 1, 1]),
                    ]),
                ),
            ]),
        )]);
        let active = HashMap::from([(
            ACTIVE.to_owned(),
            props(vec![
                ("Connection", object_path_value("/settings/1")),
                ("State", value(2u32)),
                ("Devices", paths_value(&["/dev/1"])),
            ]),
        )]);
        let objects: Objects = HashMap::from([
            (path("/dev/1"), wifi),
            (path("/dev/2"), ethernet),
            (path("/ap/1"), ap("Home", 80, 0x188)),
            (path("/ap/2"), ap("Cafe\u{7}", 20, 0)),
            (path("/ip4/1"), ip4),
            (path("/active/0"), active),
        ]);
        let manager = props(vec![
            ("WirelessEnabled", value(true)),
            ("WirelessHardwareEnabled", value(true)),
            ("NetworkingEnabled", value(true)),
            ("Connectivity", value(2u32)),
        ]);
        let settings = HashMap::from([
            ("/settings/1".to_owned(), wifi_settings_of("Home", b"Home")),
            (
                "/settings/2".to_owned(),
                HashMap::from([
                    (
                        "connection".to_owned(),
                        props(vec![("id", value("Work VPN")), ("type", value("vpn"))]),
                    ),
                    (
                        "vpn".to_owned(),
                        props(vec![(
                            "service-type",
                            value("org.freedesktop.NetworkManager.openvpn"),
                        )]),
                    ),
                ]),
            ),
        ]);
        let snapshot = snapshot(&manager, &objects, &settings);

        assert!(snapshot.wifi_enabled && snapshot.wifi_hardware_enabled);
        assert_eq!(snapshot.connectivity, Connectivity::Portal);
        let names: Vec<&str> = snapshot
            .devices
            .iter()
            .map(|d| d.interface.as_str())
            .collect();
        assert_eq!(names, ["enp3s0", "wlan0"]);
        let wlan = snapshot.device("/dev/1").unwrap();
        assert_eq!(wlan.state, DeviceState::Connected);
        assert_eq!(wlan.speed, Some(866));
        assert_eq!(wlan.hw_address, "11:22:33:44:55:66");
        assert_eq!(wlan.access_point.as_deref(), Some("/ap/1"));
        assert_eq!(
            wlan.ip4,
            Some(IpConfig {
                addresses: vec!["192.168.1.20/24".to_owned()],
                gateway: Some("192.168.1.1".to_owned()),
                dns: vec!["9.9.9.9".to_owned(), "1.1.1.1".to_owned()],
            })
        );
        assert_eq!(wlan.ip6, None);
        let wired = snapshot.device("/dev/2").unwrap();
        assert_eq!((wired.speed, wired.carrier), (None, false));
        assert_eq!(wired.active_connection, None);

        let networks = snapshot.networks("/dev/1");
        let names: Vec<&str> = networks.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["Home", "Cafe"]);
        assert_eq!(networks[0].security, Security::Personal);
        assert!(networks[0].active.is_some());
        assert_eq!(networks[1].security, Security::Open);

        let vpns: Vec<&str> = snapshot.vpns().map(|c| c.name.as_str()).collect();
        assert_eq!(vpns, ["Work VPN"]);
        assert_eq!(
            snapshot.connection("/settings/1").map(|c| &c.kind),
            Some(&ConnectionKind::Wifi(b"Home".to_vec()))
        );
        assert!(snapshot.active_of("/settings/1").is_some());
    }

    #[test]
    fn new_wifi_connections_keep_the_password_for_everyone() {
        let settings =
            wifi_settings(b"Home", Security::Personal, Some("password1"), false).unwrap();
        assert_eq!(
            settings["connection"]["type"],
            Value::from("802-11-wireless")
        );
        assert_eq!(settings["connection"]["id"], Value::from("Home"));
        assert!(!settings["connection"].contains_key("permissions"));
        assert_eq!(
            settings["802-11-wireless"]["ssid"],
            Value::from(b"Home".to_vec())
        );
        assert!(!settings["802-11-wireless"].contains_key("hidden"));
        let security = &settings["802-11-wireless-security"];
        assert_eq!(security["key-mgmt"], Value::from("wpa-psk"));
        assert_eq!(security["psk"], Value::from("password1"));
        assert_eq!(security["psk-flags"], Value::from(0u32));

        let hidden = wifi_settings(b"Lab", Security::Wpa3Personal, Some("x"), true).unwrap();
        assert_eq!(hidden["802-11-wireless"]["hidden"], Value::from(true));
        assert_eq!(
            hidden["802-11-wireless-security"]["key-mgmt"],
            Value::from("sae")
        );

        let open = wifi_settings(b"Cafe", Security::Open, None, false).unwrap();
        assert!(!open.contains_key("802-11-wireless-security"));
        let owe = wifi_settings(b"Cafe", Security::EnhancedOpen, None, false).unwrap();
        assert!(!owe["802-11-wireless-security"].contains_key("psk"));

        assert!(matches!(
            wifi_settings(b"Home", Security::Personal, Some("short"), false),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            wifi_settings(b"Work", Security::Enterprise, None, false),
            Err(Error::Failed(_))
        ));
    }

    #[test]
    fn errors_are_mapped() {
        assert!(matches!(
            method_error("org.freedesktop.DBus.Error.ServiceUnknown", None),
            Error::Unavailable(_)
        ));
        assert_eq!(
            method_error(
                "org.freedesktop.NetworkManager.PermissionDenied",
                Some("Not\nauthorized")
            ),
            Error::Denied("Not authorized".to_owned())
        );
        assert_eq!(
            method_error("org.freedesktop.NetworkManager.UnknownConnection", Some("")),
            Error::Failed(Text::data("UnknownConnection"))
        );
    }

    #[test]
    fn noisy_changes_are_not_relevant() {
        let names = |names: &[&str]| names.iter().map(|&n| n.to_owned()).collect::<Vec<_>>();
        assert!(!relevant_change(ACCESS_POINT, names(&["LastSeen"]).iter()));
        assert!(relevant_change(
            ACCESS_POINT,
            names(&["Strength", "LastSeen"]).iter()
        ));
        assert!(!relevant_change(STATISTICS, names(&["RxBytes"]).iter()));
        assert!(relevant_change(DEVICE, names(&["State"]).iter()));
    }
}
