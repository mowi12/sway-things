// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The BlueZ backend: its objects on the system bus, read in one call, the methods behind the
//! actions, the pairing agent, and a stream of its signals.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use async_channel::Receiver;
use futures_lite::{Stream, StreamExt};
use serde::Serialize;
use zbus::message::Type;
use zbus::zvariant::{DynamicType, ObjectPath, OwnedObjectPath, Value};
use zbus::{Connection, MatchRule, Message, MessageStream};

use super::agent::{self, Agent, AgentEvent};
use super::battery::{self, Battery, ChargeSource, Level, Supply};
use super::{Adapter, Device, Error, Snapshot, check_alias, clean, sort};
use crate::dbus::{self, Properties, flag, text};
use crate::i18n::Text;
use crate::msg;

/// The bus name of BlueZ.
pub(super) const SERVICE: &str = "org.bluez";
/// The interface of an adapter.
const ADAPTER: &str = "org.bluez.Adapter1";
/// The interface of a device.
const DEVICE: &str = "org.bluez.Device1";
/// The interface of a device's battery.
const BATTERY: &str = "org.bluez.Battery1";
/// The interface that takes pairing agents, on [`ROOT`].
const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
/// The standard interface for properties.
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// The standard interface that lists objects and reports new and removed ones.
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
/// Where BlueZ's objects are.
const ROOT: &str = "/org/bluez";

/// Properties that change all the time while devices around advertise, and that the snapshot
/// doesn't show: a change of only these isn't reported.
const NOISY: &[&str] = &[
    "RSSI",
    "TxPower",
    "ManufacturerData",
    "ServiceData",
    "AdvertisingFlags",
    "AdvertisingData",
];

/// The properties of one interface.
/// The interfaces of one object with their properties.
type Interfaces = HashMap<String, Properties>;

/// A connection to BlueZ on the system bus, with our pairing agent on it. Cheap to clone; every
/// clone shares the connection. UPower, on the same bus, says whether devices charge.
#[derive(Debug, Clone)]
pub struct Bluez {
    connection: Connection,
    agent_events: Receiver<AgentEvent>,
    /// Where the kernel's power supplies are read without UPower; `None` on a bus other than the
    /// system bus, whose devices aren't the machine's.
    power_supply: Option<PathBuf>,
    /// The object paths of UPower's batteries that power the computer, as last read: their
    /// changes aren't reported.
    upower_system: Arc<Mutex<Vec<String>>>,
}

impl Bluez {
    /// Connects to the system bus. Succeeds while BlueZ isn't running too: the change stream
    /// reports when it starts.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] without a system bus.
    pub async fn connect() -> Result<Self, Error> {
        let builder = zbus::connection::Builder::system().map_err(|error| {
            Error::Unavailable(Text::new(msg!("no system bus ({error})")).arg("error", error))
        })?;
        Self::build(builder, Some(PathBuf::from(battery::POWER_SUPPLY))).await
    }

    /// Connects to the bus at `address` instead of the system bus: a private bus with a stand-in
    /// for BlueZ, in tests.
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
        Self::build(builder, None).await
    }

    /// Connects with the agent served from the start: an object added to a connection that is
    /// already up can miss the calls that arrive right after.
    async fn build(
        builder: zbus::connection::Builder<'_>,
        power_supply: Option<PathBuf>,
    ) -> Result<Self, Error> {
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
            power_supply,
            upower_system: Arc::default(),
        })
    }

    /// Our unique name on the bus (`:1.42`), which BlueZ sends the agent's questions to.
    pub fn unique_name(&self) -> Option<String> {
        self.connection.unique_name().map(ToString::to_string)
    }

    /// What the pairing agent asks while [`pair`](Self::pair) runs. Every clone of this
    /// connection shares one queue, so one place should read it.
    pub fn agent_events(&self) -> Receiver<AgentEvent> {
        self.agent_events.clone()
    }

    /// The adapters and their devices, as BlueZ reports them now, with whether the devices
    /// charge as UPower reports it, or without UPower the kernel's power supplies.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while BlueZ isn't running, [`Error::Failed`] if the reply can't be
    /// read.
    pub async fn snapshot(&self) -> Result<Snapshot, Error> {
        let reply = self
            .connection
            .call_method(
                Some(SERVICE),
                "/",
                Some(OBJECT_MANAGER),
                "GetManagedObjects",
                &(),
            )
            .await
            .map_err(failure)?;
        let objects: HashMap<OwnedObjectPath, Interfaces> =
            reply.body().deserialize().map_err(failure)?;
        let mut snapshot = snapshot(&objects);
        if let Some(upower) = battery::upower(&self.connection).await {
            attach(&mut snapshot, &upower.supplies);
            snapshot.charge_source = ChargeSource::UPower;
            *self
                .upower_system
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = upower.system_paths;
        } else if let Some(dir) = &self.power_supply {
            let supplies = battery::kernel_unblocked(dir.clone()).await;
            let matched = attach(&mut snapshot, &supplies);
            // Read again now and then while a battery may show up or change: a supply can
            // appear a moment after its device connects.
            let connected = snapshot
                .devices
                .iter()
                .any(|device| device.paired && device.connected);
            if matched || connected || !supplies.is_empty() {
                snapshot.charge_source = ChargeSource::Kernel;
            }
        }
        Ok(snapshot)
    }

    /// UPower's version (`1.91.5`), which says whether devices report charging through it;
    /// `None` while it isn't running.
    pub async fn upower_version(&self) -> Option<String> {
        battery::upower_version(&self.connection).await
    }

    /// Turns the radio of `adapter` (its path) on or off.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while BlueZ isn't running, [`Error::Blocked`] if rfkill blocks
    /// the radio, [`Error::Failed`] with BlueZ's error if it refuses or fails otherwise.
    pub async fn set_powered(&self, adapter: &str, on: bool) -> Result<(), Error> {
        self.set(adapter, ADAPTER, "Powered", Value::from(on)).await
    }

    /// Renames `adapter` (its path): the name other devices see. An empty `name` goes back to the
    /// adapter's own name.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidName`] for a name BlueZ wouldn't take (see
    /// [`check_alias`](super::check_alias)), [`Error::Unavailable`] while BlueZ isn't running,
    /// [`Error::Failed`] with BlueZ's error if it refuses or fails.
    pub async fn set_adapter_alias(&self, adapter: &str, name: &str) -> Result<(), Error> {
        self.set_name(adapter, ADAPTER, name).await
    }

    /// Makes `adapter` (its path) visible to other devices or hides it. BlueZ hides it again
    /// after its `DiscoverableTimeout`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while BlueZ isn't running, [`Error::Failed`] with BlueZ's error if
    /// it refuses or fails (while the radio is off, for one).
    pub async fn set_discoverable(&self, adapter: &str, on: bool) -> Result<(), Error> {
        self.set(adapter, ADAPTER, "Discoverable", Value::from(on))
            .await
    }

    /// Starts looking for devices. BlueZ stops when [`stop_discovery`](Self::stop_discovery) is
    /// called or this connection closes.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while BlueZ isn't running, [`Error::Failed`] with BlueZ's error
    /// if it refuses or fails.
    pub async fn start_discovery(&self, adapter: &str) -> Result<(), Error> {
        self.call_unless(
            adapter,
            ADAPTER,
            "StartDiscovery",
            "org.bluez.Error.InProgress",
        )
        .await
    }

    /// Stops looking for devices.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while BlueZ isn't running, [`Error::Failed`] with BlueZ's error
    /// if it refuses or fails.
    pub async fn stop_discovery(&self, adapter: &str) -> Result<(), Error> {
        self.call(adapter, ADAPTER, "StopDiscovery", &()).await
    }

    /// Connects `device` (its path). Can take a few seconds. A device that is connected already
    /// (as one is right after pairing, often) counts as success.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while BlueZ isn't running, [`Error::Failed`] with BlueZ's error
    /// if it refuses or fails.
    pub async fn connect_device(&self, device: &str) -> Result<(), Error> {
        self.call_unless(
            device,
            DEVICE,
            "Connect",
            "org.bluez.Error.AlreadyConnected",
        )
        .await
    }

    /// Disconnects `device`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while BlueZ isn't running, [`Error::Failed`] with BlueZ's error
    /// if it refuses or fails.
    pub async fn disconnect_device(&self, device: &str) -> Result<(), Error> {
        self.call(device, DEVICE, "Disconnect", &()).await
    }

    /// Pairs `device` and trusts it, so it can connect again on its own. Registers our agent
    /// first, so BlueZ asks it (see [`agent_events`](Self::agent_events)) for a PIN, a passkey
    /// or a confirmation. A device that is paired already counts as success.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while BlueZ isn't running, [`Error::Failed`] with BlueZ's error
    /// if it refuses or fails, or the user rejected or canceled.
    pub async fn pair(&self, device: &str) -> Result<(), Error> {
        self.register_agent().await?;
        self.call_unless(device, DEVICE, "Pair", "org.bluez.Error.AlreadyExists")
            .await?;
        self.set(device, DEVICE, "Trusted", Value::from(true)).await
    }

    /// Stops a pairing of `device` that [`pair`](Self::pair) started.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while BlueZ isn't running, [`Error::Failed`] with BlueZ's error
    /// if it refuses or fails (no pairing running, for one).
    pub async fn cancel_pairing(&self, device: &str) -> Result<(), Error> {
        self.call(device, DEVICE, "CancelPairing", &()).await
    }

    /// Renames `device`: sets its alias, which BlueZ keeps. An empty name gives it its own name
    /// back.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidName`] for a name BlueZ wouldn't take (see
    /// [`check_alias`](super::check_alias)), [`Error::Unavailable`] while BlueZ isn't running,
    /// [`Error::Failed`] with BlueZ's error if it refuses or fails.
    pub async fn set_alias(&self, device: &str, name: &str) -> Result<(), Error> {
        self.set_name(device, DEVICE, name).await
    }

    /// Sets the `Alias` of `interface` on `path` to `name`, checked with [`check_alias`].
    async fn set_name(&self, path: &str, interface: &str, name: &str) -> Result<(), Error> {
        let name = check_alias(name).map_err(Error::InvalidName)?;
        self.set(path, interface, "Alias", Value::from(name.as_str()))
            .await
    }

    /// Forgets `device`: removes the pairing and what BlueZ knows about it.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] while BlueZ isn't running, [`Error::Failed`] with BlueZ's error
    /// if it refuses or fails.
    pub async fn remove(&self, adapter: &str, device: &str) -> Result<(), Error> {
        let device = object_path(device)?;
        self.call(adapter, ADAPTER, "RemoveDevice", &(device,))
            .await
    }

    /// A stream with one item whenever something the snapshot shows may have changed: a property
    /// of an adapter, a device or a battery, an object that came or went, BlueZ that started or
    /// stopped, or the same of UPower's devices and UPower. Read a new
    /// [`snapshot`](Self::snapshot) then; items can come in bursts. The kernel's power supplies
    /// report nothing: with [`ChargeSource::Kernel`], read a snapshot now and then.
    ///
    /// # Errors
    ///
    /// [`Error::Failed`] if the bus refuses the subscriptions.
    pub async fn changes(&self) -> Result<impl Stream<Item = ()> + Unpin + use<>, Error> {
        let properties = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE)
            .and_then(|rule| rule.interface(PROPERTIES))
            .and_then(|rule| rule.member("PropertiesChanged"))
            .and_then(|rule| rule.path_namespace(ROOT))
            .map_err(failure)?
            .build();
        let objects = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE)
            .and_then(|rule| rule.interface(OBJECT_MANAGER))
            .and_then(|rule| rule.path("/"))
            .map_err(failure)?
            .build();
        let owner = |name: &'static str| {
            MatchRule::builder()
                .msg_type(Type::Signal)
                .sender("org.freedesktop.DBus")
                .and_then(|rule| rule.interface("org.freedesktop.DBus"))
                .and_then(|rule| rule.member("NameOwnerChanged"))
                .and_then(|rule| rule.arg(0, name))
                .map(zbus::match_rule::Builder::build)
                .map_err(failure)
        };
        let upower_properties = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(battery::UPOWER)
            .and_then(|rule| rule.interface(PROPERTIES))
            .and_then(|rule| rule.member("PropertiesChanged"))
            .and_then(|rule| rule.path_namespace(battery::UPOWER_DEVICES))
            .map_err(failure)?
            .build();
        // `DeviceAdded` and `DeviceRemoved`.
        let upower_devices = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(battery::UPOWER)
            .and_then(|rule| rule.interface(battery::UPOWER))
            .and_then(|rule| rule.path(battery::UPOWER_PATH))
            .map_err(failure)?
            .build();
        let upower_system = Arc::clone(&self.upower_system);
        let stream = |rule| MessageStream::for_match_rule(rule, &self.connection, None);
        let properties = stream(properties).await.map_err(failure)?;
        let objects = stream(objects).await.map_err(failure)?;
        let bluez_owner = stream(owner(SERVICE)?).await.map_err(failure)?;
        let upower_properties = stream(upower_properties).await.map_err(failure)?;
        let upower_devices = stream(upower_devices).await.map_err(failure)?;
        let upower_owner = stream(owner(battery::UPOWER)?).await.map_err(failure)?;
        Ok(properties
            .or(objects)
            .or(bluez_owner)
            .or(upower_properties)
            .or(upower_devices)
            .or(upower_owner)
            .filter_map(move |message| {
                let system = upower_system.lock().unwrap_or_else(PoisonError::into_inner);
                relevant(&message.ok()?, &system).then_some(())
            }))
    }

    /// Registers our agent with BlueZ, unless it is already. BlueZ forgets it when it restarts,
    /// so this runs before each pairing.
    async fn register_agent(&self) -> Result<(), Error> {
        let path = object_path(agent::PATH)?;
        match self
            .connection
            .call_method(
                Some(SERVICE),
                ROOT,
                Some(AGENT_MANAGER),
                "RegisterAgent",
                &(path, agent::CAPABILITY),
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(zbus::Error::MethodError(name, _, _))
                if name.as_str() == "org.bluez.Error.AlreadyExists" =>
            {
                Ok(())
            }
            Err(error) => Err(failure(error)),
        }
    }

    /// Sets the property `name` of `interface` on the object at `path`.
    async fn set(
        &self,
        path: &str,
        interface: &str,
        name: &str,
        value: Value<'_>,
    ) -> Result<(), Error> {
        self.call(path, PROPERTIES, "Set", &(interface, name, value))
            .await
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

    /// Calls `method` of `interface` on the object at `path` without arguments, and counts the
    /// D-Bus error `done` as success: it says the call's goal is already reached.
    async fn call_unless(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        done: &str,
    ) -> Result<(), Error> {
        let path = object_path(path)?;
        match self
            .connection
            .call_method(Some(SERVICE), path, Some(interface), method, &())
            .await
        {
            Ok(_) => Ok(()),
            Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == done => Ok(()),
            Err(error) => Err(failure(error)),
        }
    }
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
    if dbus::is_gone(name) {
        return Error::Unavailable(msg!("BlueZ isn't running").into());
    }
    let description = description.map(clean).unwrap_or_default();
    // BlueZ says `org.bluez.Error.Blocked`, or `Failed` with "Blocked through rfkill".
    if name == "org.bluez.Error.Blocked" || description.contains("rfkill") {
        return Error::Blocked;
    }
    // `org.bluez.Error.AuthenticationFailed` → `AuthenticationFailed`
    let short = dbus::short_name(name);
    if description.is_empty() {
        Error::Failed(Text::data(short))
    } else {
        Error::Failed(Text::data(format!("{description} ({short})")))
    }
}

/// Whether `message` can change the snapshot: every message of the streams except a
/// `PropertiesChanged` of another interface or of noisy properties only.
fn relevant(message: &Message, upower_system: &[String]) -> bool {
    let header = message.header();
    if header.member().map(zbus::names::MemberName::as_str) != Some("PropertiesChanged") {
        return true;
    }
    if let Some(path) = header.path() {
        if !relevant_path(path.as_str(), upower_system) {
            return false;
        }
    }
    let Ok((interface, changed, invalidated)) = message
        .body()
        .deserialize::<(String, Properties, Vec<String>)>()
    else {
        return false;
    };
    relevant_change(&interface, changed.keys().chain(&invalidated))
}

/// Whether a change of the object at `path` can concern a Bluetooth device: not UPower's
/// display device nor one of its batteries that power the computer (`upower_system`).
fn relevant_path(path: &str, upower_system: &[String]) -> bool {
    path != battery::UPOWER_DISPLAY_DEVICE && !upower_system.iter().any(|system| system == path)
}

/// Whether a change of the properties `names` of `interface` can change the snapshot.
fn relevant_change<'a>(interface: &str, mut names: impl Iterator<Item = &'a String>) -> bool {
    if interface == battery::UPOWER_DEVICE {
        return names.any(|name| battery::UPOWER_SHOWN.contains(&name.as_str()));
    }
    [ADAPTER, DEVICE, BATTERY].contains(&interface)
        && names.any(|name| !NOISY.contains(&name.as_str()))
}

/// Ties the connected devices of `snapshot` to the batteries among `supplies`: each with a
/// battery there gets its level and whether it charges. Only connected ones: a device paired
/// with two adapters has the same address twice, and only the connected one has a live battery.
/// Whether any device got one.
fn attach(snapshot: &mut Snapshot, supplies: &[Supply]) -> bool {
    let mut any = false;
    for device in snapshot
        .devices
        .iter_mut()
        .filter(|device| device.connected)
    {
        let Some(supply) = battery::find(&device.path, &device.address, supplies) else {
            continue;
        };
        let bluez = match device.battery {
            Some(Battery {
                level: Level::Percent(percent),
                ..
            }) => Some(percent),
            _ => None,
        };
        if let Some(found) = battery::battery(bluez, Some(supply)) {
            device.battery = Some(found);
            any = true;
        }
    }
    any
}

/// The snapshot of BlueZ's `objects`.
fn snapshot(objects: &HashMap<OwnedObjectPath, Interfaces>) -> Snapshot {
    let mut adapters: Vec<Adapter> = objects
        .iter()
        .filter_map(|(path, interfaces)| Some(adapter(path.as_str(), interfaces.get(ADAPTER)?)))
        .collect();
    adapters.sort_by(|a, b| a.path.cmp(&b.path));
    let mut devices: Vec<Device> = objects
        .iter()
        .filter_map(|(path, interfaces)| device(path.as_str(), interfaces))
        .filter(|device| {
            adapters
                .iter()
                .any(|adapter| adapter.path == device.adapter)
        })
        .collect();
    sort(&mut devices);
    Snapshot {
        adapters,
        devices,
        charge_source: ChargeSource::None,
    }
}

/// The adapter at `path` with these `properties`.
fn adapter(path: &str, properties: &Properties) -> Adapter {
    let address = text(properties, "Address").unwrap_or_default();
    Adapter {
        path: path.to_owned(),
        name: text(properties, "Alias")
            .or_else(|| text(properties, "Name"))
            .unwrap_or_else(|| address.clone()),
        address,
        powered: flag(properties, "Powered"),
        discovering: flag(properties, "Discovering"),
        discoverable: flag(properties, "Discoverable"),
        discoverable_timeout: properties
            .get("DiscoverableTimeout")
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
    }
}

/// The device at `path` with these `interfaces`, if it is one.
fn device(path: &str, interfaces: &Interfaces) -> Option<Device> {
    let properties = interfaces.get(DEVICE)?;
    let adapter = properties
        .get("Adapter")
        .and_then(|value| <&ObjectPath<'_>>::try_from(value).ok())?
        .to_string();
    let address = text(properties, "Address").unwrap_or_default();
    // BlueZ sets the alias to the name, or to the address with dashes when there is no name.
    let announced = text(properties, "Name");
    let alias = text(properties, "Alias");
    let named = announced.is_some()
        || alias
            .as_ref()
            .is_some_and(|alias| *alias != address.replace(':', "-"));
    let battery = interfaces
        .get(BATTERY)
        .and_then(|battery| battery.get("Percentage"))
        .and_then(|value| u8::try_from(value).ok())
        .filter(|percent| *percent <= 100)
        .and_then(|percent| battery::battery(Some(percent), None));
    Some(Device {
        path: path.to_owned(),
        adapter,
        name: alias.or(announced).unwrap_or_else(|| address.clone()),
        named,
        address,
        icon: text(properties, "Icon"),
        paired: flag(properties, "Paired"),
        connected: flag(properties, "Connected"),
        trusted: flag(properties, "Trusted"),
        battery,
    })
}

#[cfg(test)]
mod tests {
    use zbus::zvariant::OwnedValue;

    use super::*;

    fn value(value: impl Into<Value<'static>>) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    fn path(path: &str) -> OwnedObjectPath {
        OwnedObjectPath::try_from(path).unwrap()
    }

    fn adapter_object(address: &str, powered: bool) -> Interfaces {
        HashMap::from([(
            ADAPTER.to_owned(),
            HashMap::from([
                ("Address".to_owned(), value(address.to_owned())),
                ("Alias".to_owned(), value("laptop")),
                ("Powered".to_owned(), value(powered)),
                ("Discovering".to_owned(), value(false)),
                ("Discoverable".to_owned(), value(powered)),
                ("DiscoverableTimeout".to_owned(), value(180u32)),
            ]),
        )])
    }

    fn device_object(adapter: &str, address: &str, alias: &str) -> Interfaces {
        HashMap::from([(
            DEVICE.to_owned(),
            HashMap::from([
                (
                    "Adapter".to_owned(),
                    value(ObjectPath::try_from(adapter.to_owned()).unwrap()),
                ),
                ("Address".to_owned(), value(address.to_owned())),
                ("Alias".to_owned(), value(alias.to_owned())),
                ("Paired".to_owned(), value(true)),
            ]),
        )])
    }

    #[test]
    fn the_snapshot_has_every_adapter_and_its_devices() {
        let mut headset = device_object("/org/bluez/hci1", "AA:BB:CC:DD:EE:FF", "Headset\n");
        headset.insert(
            BATTERY.to_owned(),
            HashMap::from([("Percentage".to_owned(), value(80u8))]),
        );
        let objects = HashMap::from([
            (
                path("/org/bluez/hci1"),
                adapter_object("11:11:11:11:11:11", false),
            ),
            (
                path("/org/bluez/hci0"),
                adapter_object("00:00:00:00:00:00", true),
            ),
            (path("/org/bluez/hci1/dev_AA_BB_CC_DD_EE_FF"), headset),
            (
                path("/org/bluez/hci0/dev_11_22_33_44_55_66"),
                device_object("/org/bluez/hci0", "11:22:33:44:55:66", "11-22-33-44-55-66"),
            ),
            // A device of an adapter that is gone.
            (
                path("/org/bluez/hci2/dev_00_00_00_00_00_01"),
                device_object("/org/bluez/hci2", "00:00:00:00:00:01", "Ghost"),
            ),
        ]);
        let snapshot = snapshot(&objects);
        let adapters: Vec<&str> = snapshot.adapters.iter().map(Adapter::id).collect();
        assert_eq!(adapters, ["hci0", "hci1"]);
        assert!(snapshot.adapters[0].powered);
        assert!(snapshot.adapters[0].discoverable);
        assert!(!snapshot.adapters[1].discoverable);
        assert_eq!(snapshot.adapters[0].discoverable_timeout, 180);
        assert_eq!(snapshot.devices.len(), 2);
        let headset = snapshot
            .device("/org/bluez/hci1/dev_AA_BB_CC_DD_EE_FF")
            .unwrap();
        assert_eq!(headset.name, "Headset");
        assert!(headset.named);
        assert_eq!(
            headset.battery,
            Some(Battery {
                level: Level::Percent(80),
                charge: battery::Charge::Unknown
            })
        );
        let unnamed = snapshot
            .device("/org/bluez/hci0/dev_11_22_33_44_55_66")
            .unwrap();
        assert!(!unnamed.named);
        assert_eq!(unnamed.battery, None);
    }

    #[test]
    fn batteries_are_tied_to_their_devices() {
        let mut mouse = device_object("/org/bluez/hci0", "DF:54:EE:3C:AC:81", "Mouse");
        mouse.insert(
            BATTERY.to_owned(),
            HashMap::from([("Percentage".to_owned(), value(55u8))]),
        );
        let connect = |device: &mut Interfaces| {
            if let Some(properties) = device.get_mut(DEVICE) {
                properties.insert("Connected".to_owned(), value(true));
            }
        };
        // The same mouse paired with a second adapter too, not connected there: its battery
        // stays BlueZ's.
        let mut stale = mouse.clone();
        stale.insert(
            DEVICE.to_owned(),
            device_object("/org/bluez/hci1", "DF:54:EE:3C:AC:81", "Mouse")
                .remove(DEVICE)
                .unwrap(),
        );
        connect(&mut mouse);
        let mut speaker = device_object("/org/bluez/hci0", "11:22:33:44:55:66", "Speaker");
        speaker.insert(
            BATTERY.to_owned(),
            HashMap::from([("Percentage".to_owned(), value(70u8))]),
        );
        connect(&mut speaker);
        let objects = HashMap::from([
            (
                path("/org/bluez/hci0"),
                adapter_object("00:00:00:00:00:00", true),
            ),
            (
                path("/org/bluez/hci1"),
                adapter_object("11:11:11:11:11:11", true),
            ),
            (path("/org/bluez/hci0/dev_DF_54_EE_3C_AC_81"), mouse),
            (path("/org/bluez/hci1/dev_DF_54_EE_3C_AC_81"), stale),
            (path("/org/bluez/hci0/dev_11_22_33_44_55_66"), speaker),
        ]);
        let mut snapshot = snapshot(&objects);
        let supplies = [Supply {
            native_path: "hidpp_battery_7".to_owned(),
            serial: "df:54:ee:3c:ac:81".to_owned(),
            percent: Some(60.0),
            coarse: None,
            charge: battery::Charge::Charging,
            present: true,
            powers_system: false,
        }];
        assert!(attach(&mut snapshot, &supplies));
        let battery_of = |path: &str| snapshot.device(path).and_then(|device| device.battery);
        assert_eq!(
            battery_of("/org/bluez/hci0/dev_DF_54_EE_3C_AC_81"),
            Some(Battery {
                level: Level::Percent(60),
                charge: battery::Charge::Charging
            })
        );
        assert_eq!(
            battery_of("/org/bluez/hci1/dev_DF_54_EE_3C_AC_81"),
            Some(Battery {
                level: Level::Percent(55),
                charge: battery::Charge::Unknown
            })
        );
        // Only in BlueZ: its percentage, nothing about charging.
        assert_eq!(
            battery_of("/org/bluez/hci0/dev_11_22_33_44_55_66"),
            Some(Battery {
                level: Level::Percent(70),
                charge: battery::Charge::Unknown
            })
        );
        assert!(!attach(&mut snapshot, &[]));
    }

    #[test]
    fn the_computers_batteries_are_no_relevant_change() {
        let system = ["/org/freedesktop/UPower/devices/battery_BAT0".to_owned()];
        assert!(!relevant_path(
            "/org/freedesktop/UPower/devices/battery_BAT0",
            &system
        ));
        assert!(!relevant_path(battery::UPOWER_DISPLAY_DEVICE, &system));
        assert!(relevant_path(
            "/org/freedesktop/UPower/devices/mouse_hidpp_battery_7",
            &system
        ));
        assert!(relevant_path("/org/bluez/hci0", &system));
    }

    #[test]
    fn errors_are_mapped() {
        assert!(matches!(
            method_error("org.freedesktop.DBus.Error.ServiceUnknown", None),
            Error::Unavailable(_)
        ));
        assert_eq!(
            method_error("org.bluez.Error.Blocked", None),
            Error::Blocked
        );
        assert_eq!(
            method_error("org.bluez.Error.Failed", Some("Blocked through rfkill")),
            Error::Blocked
        );
        assert_eq!(
            method_error(
                "org.bluez.Error.AuthenticationFailed",
                Some("Authentication\nFailed")
            ),
            Error::Failed(Text::data("Authentication Failed (AuthenticationFailed)"))
        );
        assert_eq!(
            method_error("org.bluez.Error.NotReady", Some("")),
            Error::Failed(Text::data("NotReady"))
        );
    }

    #[test]
    fn noisy_changes_are_not_relevant() {
        let names = |names: &[&str]| names.iter().map(|&n| n.to_owned()).collect::<Vec<_>>();
        assert!(!relevant_change(DEVICE, names(&["RSSI", "TxPower"]).iter()));
        assert!(relevant_change(
            DEVICE,
            names(&["RSSI", "Connected"]).iter()
        ));
        assert!(relevant_change(BATTERY, names(&["Percentage"]).iter()));
        assert!(relevant_change(
            battery::UPOWER_DEVICE,
            names(&["UpdateTime", "State"]).iter()
        ));
        assert!(!relevant_change(
            battery::UPOWER_DEVICE,
            names(&["UpdateTime", "EnergyRate", "TimeToEmpty"]).iter()
        ));
        assert!(!relevant_change(
            "org.bluez.MediaControl1",
            names(&["Connected"]).iter()
        ));
    }
}
