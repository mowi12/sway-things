// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A stand-in for BlueZ, for tests: python-dbusmock's `bluez5` template on a private D-Bus bus,
//! with methods to add adapters and devices, change their properties and replace their methods.
//! Built with the `test-support` feature.
//!
//! It needs `dbus-daemon` and python-dbusmock (`python3 -m dbusmock`), which the integration
//! containers have; the tests that use it are ignored by a plain `cargo test`.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use async_io::Timer;
use tempfile::TempDir;
use zbus::Connection;
use zbus::names::BusName;
use zbus::zvariant::{DynamicType, ObjectPath, OwnedValue, Value};

use super::Bluez;
use super::agent;

/// The interface dbusmock's `bluez5` template adds its helpers to, on `/`.
const MOCK: &str = "org.bluez.Mock";
/// The interface dbusmock puts on every object.
const DBUS_MOCK: &str = "org.freedesktop.DBus.Mock";

/// The error of a stand-in.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// A private bus with BlueZ's stand-in on it. Stops both when dropped.
#[derive(Debug)]
pub struct StandIn {
    /// The address of the bus, for [`Bluez::connect_to`].
    address: String,
    /// Our own connection, to drive the stand-in.
    connection: Connection,
    daemon: Child,
    mock: Option<Child>,
    /// Holds the bus socket and dbusmock's log.
    dir: TempDir,
}

impl StandIn {
    /// Starts a bus and BlueZ's stand-in on it, without an adapter.
    ///
    /// # Errors
    ///
    /// If `dbus-daemon` or python-dbusmock is missing, or the stand-in doesn't come up within ten
    /// seconds.
    pub async fn start() -> Result<Self, Error> {
        let crate::dbus::TestBus {
            dir,
            address,
            daemon,
            connection,
        } = crate::dbus::TestBus::start().await?;
        let mut stand_in = Self {
            address,
            connection,
            daemon,
            mock: None,
            dir,
        };
        stand_in.start_bluez().await?;
        Ok(stand_in)
    }

    /// The address of the bus.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// A connection to the stand-in, like [`Bluez::connect`] makes to the system bus.
    ///
    /// # Errors
    ///
    /// If the bus refuses the connection.
    pub async fn bluez(&self) -> Result<Bluez, Error> {
        Ok(Bluez::connect_to(&self.address).await?)
    }

    /// Our own connection to the bus, a program other than BlueZ.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Stops BlueZ's stand-in, as when `bluetooth.service` stops; everything in it is gone.
    pub fn stop_bluez(&mut self) {
        if let Some(mut mock) = self.mock.take() {
            let _ = mock.kill();
            let _ = mock.wait();
        }
    }

    /// Starts BlueZ's stand-in (again), without an adapter.
    ///
    /// # Errors
    ///
    /// If python-dbusmock is missing or doesn't come up within ten seconds.
    pub async fn start_bluez(&mut self) -> Result<(), Error> {
        self.stop_bluez();
        let mock = Command::new("python3")
            .args([
                "-m",
                "dbusmock",
                "--session",
                "--template",
                "bluez5",
                "--logfile",
            ])
            .arg(self.dir.path().join("dbusmock.log"))
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address)
            .env("DBUS_SYSTEM_BUS_ADDRESS", &self.address)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("can't start python3 -m dbusmock: {error}"))?;
        self.mock = Some(mock);
        let dbus = zbus::fdo::DBusProxy::new(&self.connection).await?;
        for _ in 0..100 {
            if dbus
                .name_has_owner(BusName::from_static_str("org.bluez")?)
                .await?
            {
                return Ok(());
            }
            Timer::after(Duration::from_millis(100)).await;
        }
        Err(format!("BlueZ's stand-in didn't start; its log:\n{}", self.log()).into())
    }

    /// What dbusmock logged: the calls it got.
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("dbusmock.log")).unwrap_or_default()
    }

    /// Adds the adapter `id` (`hci0`) with the system name `name`; its path.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn add_adapter(&self, id: &str, name: &str) -> Result<String, Error> {
        let path: String = self.mock_call("AddAdapter", &(id, name)).await?;
        // The template reads the discovery filter in StartDiscovery but only sets it in
        // SetDiscoveryFilter: without one, discovery fails with a KeyError.
        let filter: HashMap<&str, Value<'_>> = HashMap::new();
        self.call(
            &path,
            "org.bluez.Adapter1",
            "SetDiscoveryFilter",
            &(filter,),
        )
        .await?;
        Ok(path)
    }

    /// Removes the adapter `id` and its devices.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn remove_adapter(&self, id: &str) -> Result<(), Error> {
        self.mock_call("RemoveAdapterWithDevices", &(id,)).await
    }

    /// Adds a device with `address` and `alias` to the adapter `id`, as if discovery found it;
    /// its path.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn add_device(&self, id: &str, address: &str, alias: &str) -> Result<String, Error> {
        let path: String = self.mock_call("AddDevice", &(id, address, alias)).await?;
        // The template's Connect and Disconnect send the signal but leave the property as it
        // was, so GetManagedObjects would still report the old state.
        for (method, connected) in [("Connect", true), ("Disconnect", false)] {
            self.add_method(&path, method, &connection_code(connected))
                .await?;
        }
        Ok(path)
    }

    /// Makes `Connect` of `device` (its path) take `seconds` before it connects, as with a device
    /// that is slow to answer. dbusmock answers one call at a time, so a `Disconnect` sent
    /// meanwhile runs after it.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn connect_waits(&self, device: &str, seconds: u32) -> Result<(), Error> {
        let code = format!(
            "import time\ntime.sleep({seconds})\n{}",
            connection_code(true)
        );
        self.add_method(device, "Connect", &code).await
    }

    /// Makes `Pair` of `device` (its path) take `seconds` before it pairs, without asking the
    /// agent, and `CancelPairing` undo the pairing. dbusmock answers one call at a time, so a
    /// `CancelPairing` sent meanwhile runs after it and leaves the device unpaired, as BlueZ does.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn pair_waits(&self, device: &str, seconds: u32) -> Result<(), Error> {
        for (method, code) in [
            (
                "Pair",
                format!(
                    "import time\ntime.sleep({seconds})\n\
                     self.UpdateProperties('org.bluez.Device1', {{'Paired': dbus.Boolean(True)}})\n"
                ),
            ),
            (
                "CancelPairing",
                "self.UpdateProperties('org.bluez.Device1', {'Paired': dbus.Boolean(False)})\n"
                    .to_owned(),
            ),
        ] {
            self.add_method(device, method, &code).await?;
        }
        Ok(())
    }

    /// Replaces `method` of `org.bluez.Device1` on the object at `path` with the Python `code`,
    /// without arguments or a result.
    async fn add_method(&self, path: &str, method: &str, code: &str) -> Result<(), Error> {
        self.call(
            path,
            DBUS_MOCK,
            "AddMethod",
            &("org.bluez.Device1", method, "", "", code),
        )
        .await
    }

    /// Pairs the device with `address` of the adapter `id`, as if it was paired elsewhere.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn pair_device(&self, id: &str, address: &str) -> Result<(), Error> {
        self.mock_call("PairDevice", &(id, address)).await
    }

    /// Changes properties of `interface` on the object at `path`, with `PropertiesChanged`.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses (the object has no such interface, for one).
    pub async fn update(
        &self,
        path: &str,
        interface: &str,
        properties: &[(&str, Value<'_>)],
    ) -> Result<(), Error> {
        let properties: HashMap<&str, &Value<'_>> = properties
            .iter()
            .map(|(name, value)| (*name, value))
            .collect();
        self.call(
            path,
            DBUS_MOCK,
            "UpdateProperties",
            &(interface, properties),
        )
        .await
    }

    /// Adds the interface `interface` with `properties` to the object at `path` (without a
    /// signal, as dbusmock does).
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn add_properties(
        &self,
        path: &str,
        interface: &str,
        properties: &[(&str, Value<'_>)],
    ) -> Result<(), Error> {
        let properties: HashMap<&str, &Value<'_>> = properties
            .iter()
            .map(|(name, value)| (*name, value))
            .collect();
        self.call(path, DBUS_MOCK, "AddProperties", &(interface, properties))
            .await
    }

    /// Replaces the device's `Pair` with one that asks the agent of the connection named `agent`
    /// (its unique name, [`Bluez::unique_name`]) like BlueZ does:
    /// `agent_call` is the Python expression of the call on `agent` (a proxy for our agent),
    /// e.g. `agent.RequestConfirmation(device, dbus.UInt32(123456))`. `check` is a Python
    /// expression of its result `answer` that must be true for the pairing to succeed.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn pair_asks(
        &self,
        device: &str,
        agent: &str,
        agent_call: &str,
        check: &str,
    ) -> Result<(), Error> {
        let code = format!(
            "device = dbus.ObjectPath(self.__dbus_object_path__)\n\
             agent = dbus.Interface(self.connection.get_object('{agent}', '{path}'), \
             'org.bluez.Agent1')\n\
             answer = {agent_call}\n\
             if not ({check}):\n    \
             raise dbus.exceptions.DBusException('Authentication Failed', \
             name='org.bluez.Error.AuthenticationFailed')\n\
             self.UpdateProperties('org.bluez.Device1', {{'Paired': dbus.Boolean(True)}})\n",
            path = agent::PATH,
        );
        self.add_method(device, "Pair", &code).await
    }

    /// The arguments of each call of `method` on the object at `path`.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn calls(&self, path: &str, method: &str) -> Result<Vec<Vec<OwnedValue>>, Error> {
        let reply = self
            .connection
            .call_method(
                Some("org.bluez"),
                ObjectPath::try_from(path)?,
                Some(DBUS_MOCK),
                "GetMethodCalls",
                &(method,),
            )
            .await?;
        let calls: Vec<(u64, Vec<OwnedValue>)> = reply.body().deserialize()?;
        Ok(calls.into_iter().map(|(_, args)| args).collect())
    }

    /// Calls a helper of the `bluez5` template.
    async fn mock_call<B, R>(&self, method: &str, body: &B) -> Result<R, Error>
    where
        B: serde::Serialize + DynamicType,
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d> + for<'d> serde::Deserialize<'d>,
    {
        let reply = self
            .connection
            .call_method(Some("org.bluez"), "/", Some(MOCK), method, body)
            .await?;
        Ok(reply.body().deserialize()?)
    }

    /// Calls `method` of `interface` at `path`, ignoring what it returns.
    async fn call<B>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<(), Error>
    where
        B: serde::Serialize + DynamicType,
    {
        self.connection
            .call_method(
                Some("org.bluez"),
                ObjectPath::try_from(path)?,
                Some(interface),
                method,
                body,
            )
            .await?;
        Ok(())
    }
}

/// The Python code of `Connect` (`connected`) or `Disconnect`: it fails like BlueZ if the device
/// is in that state already, and sets the property, which the template's own methods leave as it
/// was (so `GetManagedObjects` would still report the old state).
fn connection_code(connected: bool) -> String {
    let (state, error) = if connected {
        ("True", "AlreadyConnected")
    } else {
        ("False", "NotConnected")
    };
    format!(
        "if bool(self.props['org.bluez.Device1']['Connected']) == {state}:\n    \
         raise dbus.exceptions.DBusException('{error}', name='org.bluez.Error.{error}')\n\
         self.UpdateProperties('org.bluez.Device1', {{'Connected': dbus.Boolean({state})}})\n"
    )
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.stop_bluez();
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}
