// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A stand-in for NetworkManager, for tests: python-dbusmock's `networkmanager` template on a
//! private D-Bus bus, with methods to add devices, access points and connections, change their
//! properties, and have NetworkManager ask our secret agent. Built with the `test-support`
//! feature.
//!
//! The template is extended where it falls short of NetworkManager: an agent manager, a device's
//! `Disconnect`, `ActivateConnection` with the best connection for a device, a
//! `DeactivateConnection` that only touches its own devices, and an `AddAndActivateConnection`
//! that keeps the settings it was given.
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

use super::NetworkManager;
use super::agent;

/// NetworkManager's bus name.
const SERVICE: &str = "org.freedesktop.NetworkManager";
/// The VPN service of the fake plugin of [`fake_vpn_plugin`].
pub const OPENVPN: &str = "org.freedesktop.NetworkManager.openvpn";
/// The template's main object, which has its helpers.
const MAIN: &str = "/org/freedesktop";
/// The manager object.
const MANAGER_PATH: &str = "/org/freedesktop/NetworkManager";
/// The interface dbusmock puts on every object, with the template's helpers.
const DBUS_MOCK: &str = "org.freedesktop.DBus.Mock";
/// The interface of the helpers this stand-in adds to the manager.
const TEST: &str = "org.freedesktop.NetworkManager.Test";
/// The interface of a device.
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
/// The interface of a Wi-Fi device.
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";

/// The auth dialog of a fake VPN plugin (see [`fake_vpn_plugin`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeDialog {
    /// None: the plugin's GNOME part isn't installed.
    Missing,
    /// Its own window: answers `password` = `hunter2`, then waits for `QUIT`.
    Window,
    /// External UI mode: asks for `username` (now `alice`) and the secret `password`, and knows
    /// the secret `cert-pass` = `known`.
    ExternalUi,
    /// The user cancels it.
    Canceled,
    /// It waits until it is ended.
    Hangs,
}

/// Puts a fake plugin for [`OPENVPN`] with `dialog` into `dir`, which then stands in for
/// `/usr/lib/NetworkManager/VPN` (see [`NetworkManager::use_vpn_plugin_dirs`]). The auth dialog
/// writes its arguments, one per line, to `args.txt` in `dir`, what it reads to `input.txt`, and
/// the line after its secrets to `quit.txt`.
///
/// # Errors
///
/// If the files can't be written.
pub fn fake_vpn_plugin(dir: &std::path::Path, dialog: FakeDialog) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;

    let program = dir.join("auth-dialog");
    let mut name = format!(
        "[VPN Connection]\nname=openvpn\nservice={OPENVPN}\n\
         program=/usr/libexec/nm-openvpn-service\n"
    );
    if dialog != FakeDialog::Missing {
        name = format!("{name}\n[GNOME]\nauth-dialog={}\n", program.display());
    }
    if dialog == FakeDialog::ExternalUi {
        name.push_str("supports-external-ui-mode=true\n");
    }
    let file = dir.join("nm-openvpn-service.name");
    std::fs::write(&file, name)?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
    let answer = match dialog {
        FakeDialog::Missing => return Ok(()),
        FakeDialog::Window => {
            "printf 'password\\nhunter2\\n\\n\\n'\n\
                               while IFS= read -r quit && [ -z \"$quit\" ]; do :; done\n\
                               printf '%s\\n' \"$quit\" > \"$dir/quit.txt\""
        }
        FakeDialog::ExternalUi => {
            "printf '%s\\n' '[VPN Plugin UI]' Version=2 \
                                   'Description=Sign in to Office' '' '[username]' Value=alice \
                                   Label=Username: IsSecret=false ShouldAsk=true '' \
                                   '[password]' Value= Label=Password: IsSecret=true \
                                   ShouldAsk=true '' '[cert-pass]' Value=known IsSecret=true \
                                   ShouldAsk=false"
        }
        FakeDialog::Canceled => "exit 1",
        FakeDialog::Hangs => "exec sleep 600",
    };
    let script = format!(
        "#!/bin/sh\ndir='{}'\nprintf '%s\\n' \"$@\" > \"$dir/args.txt\"\n\
         while IFS= read -r line; do\n  printf '%s\\n' \"$line\" >> \"$dir/input.txt\"\n  \
         [ \"$line\" = DONE ] && break\ndone\n{answer}\n",
        dir.display()
    );
    std::fs::write(&program, script)?;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// How a fake openconnect (see [`fake_openconnect`]) signs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeSignIn {
    /// Asks for `Username:` and `Password:`, then after the server's message "MFA: Enter your
    /// TOTP as the second factor" for `Response:`, and signs in.
    Totp,
    /// Asks for a `GROUP: [Students|Staff]:` choice, then signs in.
    Group,
    /// Asks for `Username:` and `Password:`, then fails with "Login failed.".
    Fails,
    /// Asks to accept a certificate that failed verification.
    Certificate,
}

/// Puts a fake `openconnect` that signs in as `kind` says into `dir`; its path (see
/// [`NetworkManager::use_openconnect`]). It writes its arguments, one per line, to
/// `openconnect-args.txt` in `dir` and the answers it got to `answers.txt`, and prints the cookie
/// `webvpn=fake`, the server `https://vpn.example.org/` and the fingerprint `pin-sha256:fake=`.
///
/// # Errors
///
/// If the file can't be written.
pub fn fake_openconnect(
    dir: &std::path::Path,
    kind: FakeSignIn,
) -> Result<std::path::PathBuf, Error> {
    use std::os::unix::fs::PermissionsExt;

    let ask = |label: &str, var: &str| format!("printf '%s' \"{label}\" >&2\nread -r {var}\n");
    let mut script = format!(
        "#!/bin/sh\ndir='{}'\nprintf '%s\\n' \"$@\" > \"$dir/openconnect-args.txt\"\n\
         echo 'POST https://vpn.example.org/' >&2\necho 'XML POST enabled' >&2\n",
        dir.display()
    );
    let answers = match kind {
        FakeSignIn::Totp => {
            script.push_str(&ask("Username:", "user"));
            script.push_str(&ask("Password:", "pass"));
            script.push_str("echo 'POST https://vpn.example.org/' >&2\n");
            script.push_str("echo 'MFA: Enter your TOTP as the second factor' >&2\n");
            script.push_str(&ask("Response:", "code"));
            "\"$user\" \"$pass\" \"$code\""
        }
        FakeSignIn::Group => {
            script.push_str(&ask("GROUP: [Students|Staff]:", "group"));
            "\"$group\""
        }
        FakeSignIn::Fails => {
            script.push_str(&ask("Username:", "user"));
            script.push_str(&ask("Password:", "pass"));
            script.push_str(
                "echo 'Login failed.' >&2\necho 'Failed to complete authentication' >&2\nexit 1\n",
            );
            "\"$user\""
        }
        FakeSignIn::Certificate => {
            script.push_str(
                "echo 'Certificate from VPN server \"vpn.example.org\" failed verification.' >&2\n",
            );
            script.push_str(&ask(
                "Enter 'yes' to accept, 'no' to abort; anything else to view: ",
                "accept",
            ));
            "\"$accept\""
        }
    };
    script = format!(
        "{script}printf '%s\\n' {answers} > \"$dir/answers.txt\"\n\
         echo \"COOKIE='webvpn=fake'\"\necho \"HOST='[2001:db8::1]'\"\n\
         echo \"CONNECT_URL='https://vpn.example.org/'\"\n\
         echo \"FINGERPRINT='pin-sha256:fake='\"\n"
    );
    let program = dir.join("openconnect");
    std::fs::write(&program, script)?;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
    Ok(program)
}

/// `NM_DEVICE_STATE_DISCONNECTED`.
pub const DISCONNECTED: u32 = 30;
/// `NM_DEVICE_STATE_UNAVAILABLE`.
pub const UNAVAILABLE: u32 = 20;
/// The security flags of a WPA2 Personal access point in the template (`KEY_MGMT_PSK`).
pub const WPA2: u32 = 0x100;
/// The security flags of an open access point.
pub const OPEN: u32 = 0;

/// `ActivateConnection`: with the connection "/", the first one the device can use.
const ACTIVATE: &str = "\
NM = 'org.freedesktop.NetworkManager'
conn = args[0]
if conn == '/':
    available = list(objects[args[1]].Get(NM + '.Device', 'AvailableConnections'))
    if not available:
        raise dbus.exceptions.DBusException(
            'No connection for the device', name=NM + '.UnknownConnection')
    conn = available[0]
ret = self.activate_connection(self, conn, args[1], args[2])
";

/// `DeactivateConnection`: disconnects the connection's own devices only.
const DEACTIVATE: &str = "\
NM = 'org.freedesktop.NetworkManager'
ac = args[0]
if ac not in objects:
    raise dbus.exceptions.DBusException('Not active', name=NM + '.ConnectionNotActive')
for d in objects[ac].Get(NM + '.Connection.Active', 'Devices'):
    self.SetDeviceDisconnected(d)
acs = self.Get(NM, 'ActiveConnections')
acs.remove(dbus.ObjectPath(ac))
self.SetProperty('/org/freedesktop/NetworkManager', NM, 'ActiveConnections', acs)
self.object_manager_emit_removed(ac)
self.RemoveObject(ac)
";

/// `AddAndActivateConnection`: saves the settings as they are, offers the connection to the
/// device and activates it.
const ADD_AND_ACTIVATE: &str = "\
NM = 'org.freedesktop.NetworkManager'
conn = self.SettingsAddConnection(args[0])
dev = objects[args[1]]
available = list(dev.Get(NM + '.Device', 'AvailableConnections'))
available.append(dbus.ObjectPath(conn))
dev.Set(NM + '.Device', 'AvailableConnections', dbus.Array(available, signature='o'))
ret = (dbus.ObjectPath(conn), self.activate_connection(self, conn, args[1], args[2]))
";

/// A device's `Disconnect`.
/// A scan that is done at once: `LastScan` moves on.
const REQUEST_SCAN: &str = "\
IFACE = 'org.freedesktop.NetworkManager.Device.Wireless'
last = self.Get(IFACE, 'LastScan')
self.UpdateProperties(IFACE, {'LastScan': dbus.Int64(max(last, 0) + 1000)})
";

const DISCONNECT: &str = "\
NM = 'org.freedesktop.NetworkManager'
ac = self.Get(NM + '.Device', 'ActiveConnection')
if ac == '/':
    raise dbus.exceptions.DBusException('Not active', name=NM + '.Device.NotActive')
objects['/org/freedesktop/NetworkManager'].RemoveActiveConnection(
    self.__dbus_object_path__, ac)
";

/// The test helper `AskSecrets(agent, connection, setting, flags)`: calls the agent's
/// `GetSecrets` from NetworkManager's connection without waiting; `SecretsReply` has the answer.
const ASK_SECRETS: &str = "\
agent = dbus.Interface(
    self.connection.get_object(args[0], '/org/freedesktop/NetworkManager/SecretAgent'),
    'org.freedesktop.NetworkManager.SecretAgent')
self.secrets = []
agent.GetSecrets(
    objects[args[1]].settings, dbus.ObjectPath(args[1]), args[2],
    dbus.Array([], signature='s'), dbus.UInt32(args[3]),
    reply_handler=self.secrets.append, error_handler=self.secrets.append, timeout=60)
";

/// The test helper `SecretsReply()`: "" while the agent hasn't answered, else `ok:` and the
/// password (for a VPN its secrets as `key=value`, sorted and joined by `;`), or `error:` and
/// the D-Bus error.
const SECRETS_REPLY: &str = "\
replies = getattr(self, 'secrets', [])
if not replies:
    ret = ''
elif isinstance(replies[0], dbus.exceptions.DBusException):
    ret = 'error:' + replies[0].get_dbus_name()
elif 'vpn' in replies[0]:
    pairs = []
    for key, value in sorted(replies[0]['vpn']['secrets'].items()):
        pairs.append(str(key) + '=' + str(value))
    ret = 'ok:' + ';'.join(pairs)
else:
    ret = 'ok:' + str(replies[0]['802-11-wireless-security']['psk'])
";

/// The test helper `CancelSecrets(agent, connection)`.
const CANCEL_SECRETS: &str = "\
agent = dbus.Interface(
    self.connection.get_object(args[0], '/org/freedesktop/NetworkManager/SecretAgent'),
    'org.freedesktop.NetworkManager.SecretAgent')
agent.CancelGetSecrets(dbus.ObjectPath(args[1]), '802-11-wireless-security')
";

/// The error of a stand-in.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// A private bus with NetworkManager's stand-in on it. Stops both when dropped.
#[derive(Debug)]
pub struct StandIn {
    /// The address of the bus, for [`NetworkManager::connect_to`].
    address: String,
    /// Our own connection, to drive the stand-in.
    connection: Connection,
    daemon: Child,
    mock: Option<Child>,
    /// Holds the bus socket and dbusmock's log.
    dir: TempDir,
    /// How many IP configurations were added, to name the next.
    ip_configs: u32,
}

impl StandIn {
    /// Starts a bus and NetworkManager's stand-in on it, without devices.
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
            ip_configs: 0,
        };
        stand_in.start_nm().await?;
        Ok(stand_in)
    }

    /// The address of the bus.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// A connection to the stand-in, like [`NetworkManager::connect`] makes to the system bus.
    ///
    /// # Errors
    ///
    /// If the bus refuses the connection.
    pub async fn network_manager(&self) -> Result<NetworkManager, Error> {
        Ok(NetworkManager::connect_to(&self.address).await?)
    }

    /// Our own connection to the bus, a program other than NetworkManager.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Stops NetworkManager's stand-in, as when `NetworkManager.service` stops; everything in it
    /// is gone.
    pub fn stop_nm(&mut self) {
        if let Some(mut mock) = self.mock.take() {
            let _ = mock.kill();
            let _ = mock.wait();
        }
    }

    /// Starts NetworkManager's stand-in (again), without devices.
    ///
    /// # Errors
    ///
    /// If python-dbusmock is missing or doesn't come up within ten seconds.
    pub async fn start_nm(&mut self) -> Result<(), Error> {
        self.stop_nm();
        let mock = Command::new("python3")
            .args([
                "-m",
                "dbusmock",
                "--session",
                "--template",
                "networkmanager",
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
        self.ip_configs = 0;
        let dbus = zbus::fdo::DBusProxy::new(&self.connection).await?;
        let mut up = false;
        for _ in 0..100 {
            if dbus
                .name_has_owner(BusName::from_static_str(SERVICE)?)
                .await?
            {
                up = true;
                break;
            }
            Timer::after(Duration::from_millis(100)).await;
        }
        if !up {
            return Err(format!(
                "NetworkManager's stand-in didn't start; its log:\n{}",
                self.log()
            )
            .into());
        }
        self.extend().await
    }

    /// Adds what the template lacks.
    async fn extend(&self) -> Result<(), Error> {
        let none: HashMap<&str, Value<'_>> = HashMap::new();
        let agent_methods = vec![
            ("Register", "s", "", ""),
            ("RegisterWithCapabilities", "su", "", ""),
            ("Unregister", "", "", ""),
        ];
        self.call(
            MAIN,
            DBUS_MOCK,
            "AddObject",
            &(
                "/org/freedesktop/NetworkManager/AgentManager",
                "org.freedesktop.NetworkManager.AgentManager",
                none,
                agent_methods,
            ),
        )
        .await?;
        let manager = "org.freedesktop.NetworkManager";
        for (interface, name, inputs, outputs, code) in [
            (manager, "ActivateConnection", "ooo", "o", ACTIVATE),
            (manager, "DeactivateConnection", "o", "", DEACTIVATE),
            (
                manager,
                "AddAndActivateConnection",
                "a{sa{sv}}oo",
                "oo",
                ADD_AND_ACTIVATE,
            ),
            (TEST, "AskSecrets", "sssu", "", ASK_SECRETS),
            (TEST, "SecretsReply", "", "s", SECRETS_REPLY),
            (TEST, "CancelSecrets", "ss", "", CANCEL_SECRETS),
        ] {
            self.add_method(MANAGER_PATH, interface, name, inputs, outputs, code)
                .await?;
        }
        Ok(())
    }

    /// What dbusmock logged: the calls it got.
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("dbusmock.log")).unwrap_or_default()
    }

    /// Adds the Wi-Fi device `name` (the end of its path) with the interface `interface`,
    /// disconnected; its path.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn add_wifi_device(&self, name: &str, interface: &str) -> Result<String, Error> {
        let path: String = self
            .mock_call(
                "AddWiFiDevice",
                &(name, interface, i32::try_from(DISCONNECTED)?),
            )
            .await?;
        self.add_method(&path, DEVICE, "Disconnect", "", "", DISCONNECT)
            .await?;
        self.add_properties(&path, WIRELESS, &[("LastScan", Value::from(-1i64))])
            .await?;
        self.add_method(&path, WIRELESS, "RequestScan", "a{sv}", "", REQUEST_SCAN)
            .await?;
        Ok(path)
    }

    /// Adds the Ethernet device `name` with the interface `interface` in `state`; its path.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn add_ethernet_device(
        &self,
        name: &str,
        interface: &str,
        state: u32,
    ) -> Result<String, Error> {
        let state = i32::try_from(state)?;
        let path: String = self
            .mock_call("AddEthernetDevice", &(name, interface, state))
            .await?;
        self.add_method(&path, DEVICE, "Disconnect", "", "", DISCONNECT)
            .await?;
        Ok(path)
    }

    /// Adds the access point `name` (the end of its path) of the network `ssid` to `device`,
    /// with the hardware address `bssid`, `strength` in percent and the security flags `security`
    /// ([`OPEN`], [`WPA2`]); its path.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn add_access_point(
        &self,
        device: &str,
        name: &str,
        ssid: &str,
        bssid: &str,
        strength: u8,
        security: u32,
    ) -> Result<String, Error> {
        self.mock_call(
            "AddAccessPoint",
            &(
                device, name, ssid, bssid, 2u32, 2412u32, 54_000u32, strength, security,
            ),
        )
        .await
    }

    /// Removes the access point at `path` from `device`.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn remove_access_point(&self, device: &str, path: &str) -> Result<(), Error> {
        self.mock_call("RemoveAccessPoint", &(device, path)).await
    }

    /// Saves a Wi-Fi connection `name` (the end of its path) for the network `ssid` that
    /// `device` sees, as if it was connected to before; its path.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses (no access point of `ssid`, for one).
    pub async fn add_wifi_connection(
        &self,
        device: &str,
        name: &str,
        ssid: &str,
    ) -> Result<String, Error> {
        self.mock_call("AddWiFiConnection", &(device, name, ssid, ""))
            .await
    }

    /// Saves a connection with `settings`, as nm-connection-editor would; its path.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn add_connection(
        &self,
        settings: &HashMap<&str, HashMap<&str, Value<'_>>>,
    ) -> Result<String, Error> {
        let reply = self
            .connection
            .call_method(
                Some(SERVICE),
                "/org/freedesktop/NetworkManager/Settings",
                Some("org.freedesktop.NetworkManager.Settings"),
                "AddConnection",
                &(settings,),
            )
            .await?;
        let path: zbus::zvariant::OwnedObjectPath = reply.body().deserialize()?;
        Ok(path.to_string())
    }

    /// Offers the saved connection at `connection` to `device`, as NetworkManager does for one
    /// that fits it.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn make_available(&self, device: &str, connection: &str) -> Result<(), Error> {
        let mut available: Vec<ObjectPath<'_>> = self
            .get(device, DEVICE, "AvailableConnections")
            .await?
            .try_into()?;
        available.push(ObjectPath::try_from(connection)?);
        self.update(
            device,
            DEVICE,
            &[("AvailableConnections", Value::from(available))],
        )
        .await
    }

    /// Gives `device` an IPv4 configuration with `address`/`prefix`, `gateway` and `dns`.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn set_ip4(
        &mut self,
        device: &str,
        address: &str,
        prefix: u32,
        gateway: &str,
        dns: &str,
    ) -> Result<(), Error> {
        self.ip_configs += 1;
        let path = format!(
            "/org/freedesktop/NetworkManager/IP4Config/{}",
            self.ip_configs
        );
        let address_data = vec![HashMap::from([
            ("address", Value::from(address)),
            ("prefix", Value::from(prefix)),
        ])];
        let nameserver_data = vec![HashMap::from([("address", Value::from(dns))])];
        let properties: HashMap<&str, Value<'_>> = HashMap::from([
            ("AddressData", Value::from(address_data)),
            ("Gateway", Value::from(gateway)),
            ("NameserverData", Value::from(nameserver_data)),
        ]);
        let methods: Vec<(&str, &str, &str, &str)> = Vec::new();
        self.call(
            MAIN,
            DBUS_MOCK,
            "AddObject",
            &(
                path.as_str(),
                "org.freedesktop.NetworkManager.IP4Config",
                properties,
                methods,
            ),
        )
        .await?;
        let path = ObjectPath::try_from(path.as_str())?;
        self.add_properties(device, DEVICE, &[("Ip4Config", Value::from(&path))])
            .await?;
        // With the signal; dbusmock's UpdateProperties can't take an object path.
        self.mock_call(
            "SetProperty",
            &(device, DEVICE, "Ip4Config", Value::from(path)),
        )
        .await
    }

    /// Asks the secret agent of the connection named `agent` (its unique name,
    /// [`NetworkManager::unique_name`]) for the password of the saved connection at `connection`,
    /// from NetworkManager's own connection, with `flags` (`NMSecretAgentGetSecretsFlags`).
    /// Doesn't wait: [`secrets_reply`](Self::secrets_reply) has the answer.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn ask_secrets(
        &self,
        agent: &str,
        connection: &str,
        flags: u32,
    ) -> Result<(), Error> {
        self.call(
            MANAGER_PATH,
            TEST,
            "AskSecrets",
            &(agent, connection, "802-11-wireless-security", flags),
        )
        .await
    }

    /// Asks the secret agent of `agent` for the secrets of `setting`, as
    /// [`ask_secrets`](Self::ask_secrets) does.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn ask_secrets_of(
        &self,
        agent: &str,
        connection: &str,
        setting: &str,
        flags: u32,
    ) -> Result<(), Error> {
        self.call(
            MANAGER_PATH,
            TEST,
            "AskSecrets",
            &(agent, connection, setting, flags),
        )
        .await
    }

    /// The agent's answer to [`ask_secrets`](Self::ask_secrets) once it came: `Ok` with the
    /// password, or `Err` with the D-Bus error's name.
    ///
    /// # Errors
    ///
    /// If the agent doesn't answer within ten seconds.
    pub async fn secrets_reply(&self) -> Result<Result<String, String>, Error> {
        for _ in 0..100 {
            let reply = self
                .connection
                .call_method(Some(SERVICE), MANAGER_PATH, Some(TEST), "SecretsReply", &())
                .await?;
            let reply: String = reply.body().deserialize()?;
            if let Some(password) = reply.strip_prefix("ok:") {
                return Ok(Ok(password.to_owned()));
            }
            if let Some(error) = reply.strip_prefix("error:") {
                return Ok(Err(error.to_owned()));
            }
            Timer::after(Duration::from_millis(100)).await;
        }
        Err("the secret agent didn't answer".into())
    }

    /// Tells the secret agent of `agent` that NetworkManager no longer waits for the password of
    /// `connection`.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn cancel_secrets(&self, agent: &str, connection: &str) -> Result<(), Error> {
        self.call(MANAGER_PATH, TEST, "CancelSecrets", &(agent, connection))
            .await
    }

    /// Changes properties of `interface` on the object at `path`, with `PropertiesChanged`.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses (the object has no such interface or property, for one).
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

    /// Adds properties of `interface` to the object at `path`, without a signal.
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

    /// Replaces `method` of `interface` on the object at `path` with the Python `code`, which
    /// sees its arguments as `args` and returns `ret`.
    ///
    /// # Errors
    ///
    /// If the stand-in refuses.
    pub async fn add_method(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        inputs: &str,
        outputs: &str,
        code: &str,
    ) -> Result<(), Error> {
        self.call(
            path,
            DBUS_MOCK,
            "AddMethod",
            &(interface, method, inputs, outputs, code),
        )
        .await
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
                Some(SERVICE),
                ObjectPath::try_from(path)?,
                Some(DBUS_MOCK),
                "GetMethodCalls",
                &(method,),
            )
            .await?;
        let calls: Vec<(u64, Vec<OwnedValue>)> = reply.body().deserialize()?;
        Ok(calls.into_iter().map(|(_, args)| args).collect())
    }

    /// The path of the agent manager, whose calls [`calls`](Self::calls) reads.
    pub fn agent_manager(&self) -> &'static str {
        "/org/freedesktop/NetworkManager/AgentManager"
    }

    /// The path where NetworkManager looks for an agent.
    pub fn agent_path(&self) -> &'static str {
        agent::PATH
    }

    /// The value of a property.
    async fn get(&self, path: &str, interface: &str, name: &str) -> Result<OwnedValue, Error> {
        let reply = self
            .connection
            .call_method(
                Some(SERVICE),
                ObjectPath::try_from(path)?,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(interface, name),
            )
            .await?;
        Ok(reply.body().deserialize()?)
    }

    /// Calls a helper of the `networkmanager` template.
    async fn mock_call<B, R>(&self, method: &str, body: &B) -> Result<R, Error>
    where
        B: serde::Serialize + DynamicType,
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d> + for<'d> serde::Deserialize<'d>,
    {
        let reply = self
            .connection
            .call_method(Some(SERVICE), MAIN, Some(DBUS_MOCK), method, body)
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
                Some(SERVICE),
                ObjectPath::try_from(path)?,
                Some(interface),
                method,
                body,
            )
            .await?;
        Ok(())
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.stop_nm();
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}
