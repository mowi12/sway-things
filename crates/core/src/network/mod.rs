// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Networks through NetworkManager on the system D-Bus: the devices, the Wi-Fi networks around,
//! the saved and the active connections, what can be done with them, and the secret agent that
//! asks for a Wi-Fi password again.
//!
//! [`NetworkManager`] is the backend; the types here are plain data, free of D-Bus. SSIDs come
//! from anyone in radio range, and connection names and NetworkManager's messages often carry
//! them: they are data, shown as plain text and never put into commands.
//!
//! Nothing here is stored by Sway Things: the connections, their passwords and the radio's state
//! are NetworkManager's own, which it keeps across reboots.

mod agent;
mod nm;
#[cfg(any(test, feature = "test-support"))]
pub mod stand_in;

use std::fmt;

pub use agent::{AgentEvent, SecretPrompt};
pub use nm::NetworkManager;

/// The kind of a network device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// Ethernet: a cable.
    Ethernet,
    /// Wi-Fi.
    Wifi,
    /// Anything else (a modem, a bridge, a VPN tunnel); NetworkManager's `NMDeviceType`.
    Other(u32),
}

impl DeviceKind {
    fn from_nm(kind: u32) -> Self {
        match kind {
            1 => Self::Ethernet,
            2 => Self::Wifi,
            other => Self::Other(other),
        }
    }
}

/// The state of a device, after NetworkManager's `NMDeviceState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Unknown to NetworkManager.
    Unknown,
    /// Not managed by NetworkManager.
    Unmanaged,
    /// Can't be used: no cable, the radio off, the firmware missing.
    Unavailable,
    /// Ready, but not connected.
    Disconnected,
    /// Connecting: preparing, authenticating or getting an address.
    Connecting,
    /// Waiting for a password.
    NeedAuth,
    /// Connected.
    Connected,
    /// Disconnecting.
    Disconnecting,
    /// The last connection attempt failed.
    Failed,
}

impl DeviceState {
    fn from_nm(state: u32) -> Self {
        match state {
            10 => Self::Unmanaged,
            20 => Self::Unavailable,
            30 => Self::Disconnected,
            60 => Self::NeedAuth,
            40..=90 => Self::Connecting,
            100 => Self::Connected,
            110 => Self::Disconnecting,
            120 => Self::Failed,
            _ => Self::Unknown,
        }
    }
}

/// The IP addresses of a device or connection, from one `IP4Config` or `IP6Config` object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IpConfig {
    /// The addresses with their prefix length (`192.168.1.20/24`).
    pub addresses: Vec<String>,
    /// The default gateway, if there is one.
    pub gateway: Option<String>,
    /// The DNS servers.
    pub dns: Vec<String>,
}

/// A network device: an Ethernet port or a Wi-Fi adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// The D-Bus object path, which the actions take.
    pub path: String,
    /// The kernel's name of the interface (`wlan0`, `enp3s0`).
    pub interface: String,
    /// What kind of device it is.
    pub kind: DeviceKind,
    /// Its state.
    pub state: DeviceState,
    /// Why it is in that state: NetworkManager's `NMDeviceStateReason`, see [`reason_text`].
    pub reason: u32,
    /// Whether NetworkManager manages it.
    pub managed: bool,
    /// The path of its active connection, if it has one.
    pub active_connection: Option<String>,
    /// Its hardware (MAC) address.
    pub hw_address: String,
    /// The speed of the link in Mbit/s: the cable's, or the Wi-Fi's current bit rate. `None`
    /// while it isn't known.
    pub speed: Option<u32>,
    /// Whether a cable is plugged in (Ethernet).
    pub carrier: bool,
    /// The path of the access point a Wi-Fi device is connected to.
    pub access_point: Option<String>,
    /// The paths of the saved connections the device can use now.
    pub available_connections: Vec<String>,
    /// Its IPv4 addresses, while it has some.
    pub ip4: Option<IpConfig>,
    /// Its IPv6 addresses, while it has some.
    pub ip6: Option<IpConfig>,
}

/// How a Wi-Fi network is secured, from the flags its access point announces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Security {
    /// Open: no password, nothing encrypted.
    Open,
    /// Enhanced Open (OWE): no password, but encrypted.
    EnhancedOpen,
    /// WEP: an old, broken encryption with a key.
    Wep,
    /// WPA or WPA2 Personal: a password (pre-shared key); also WPA3 networks that take WPA2.
    Personal,
    /// WPA3 Personal only (SAE): a password.
    Wpa3Personal,
    /// WPA Enterprise (802.1X): a user name and password or a certificate.
    Enterprise,
}

impl Security {
    /// Whether connecting needs a password the page asks for.
    pub fn needs_password(self) -> bool {
        matches!(self, Self::Personal | Self::Wpa3Personal)
    }

    /// Whether the page can connect to it by itself, without nm-connection-editor.
    pub fn supported(self) -> bool {
        matches!(
            self,
            Self::Open | Self::EnhancedOpen | Self::Personal | Self::Wpa3Personal
        )
    }

    /// The security of an access point with these flags (`Flags`, `WpaFlags`, `RsnFlags`).
    pub fn from_flags(flags: u32, wpa: u32, rsn: u32) -> Self {
        let both = wpa | rsn;
        if both & (ap_sec::KEY_MGMT_802_1X | ap_sec::EAP_SUITE_B_192) != 0 {
            Self::Enterprise
        } else if both & ap_sec::KEY_MGMT_PSK != 0 {
            Self::Personal
        } else if rsn & ap_sec::KEY_MGMT_SAE != 0 {
            Self::Wpa3Personal
        } else if rsn & (ap_sec::KEY_MGMT_OWE | ap_sec::KEY_MGMT_OWE_TM) != 0 {
            Self::EnhancedOpen
        } else if flags & ap_sec::PRIVACY != 0 {
            Self::Wep
        } else {
            Self::Open
        }
    }

    /// NetworkManager's `key-mgmt` for a new connection with this security, `None` for an open
    /// network (and those the page doesn't connect to).
    fn key_mgmt(self) -> Option<&'static str> {
        match self {
            Self::EnhancedOpen => Some("owe"),
            Self::Personal => Some("wpa-psk"),
            Self::Wpa3Personal => Some("sae"),
            Self::Open | Self::Wep | Self::Enterprise => None,
        }
    }
}

/// The flags of an access point's security (`NM80211ApFlags`, `NM80211ApSecurityFlags`).
mod ap_sec {
    pub const PRIVACY: u32 = 0x1;
    pub const KEY_MGMT_PSK: u32 = 0x100;
    pub const KEY_MGMT_802_1X: u32 = 0x200;
    pub const KEY_MGMT_SAE: u32 = 0x400;
    pub const KEY_MGMT_OWE: u32 = 0x800;
    pub const KEY_MGMT_OWE_TM: u32 = 0x1000;
    pub const EAP_SUITE_B_192: u32 = 0x2000;
}

/// A Wi-Fi access point a device sees: one radio of a network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessPoint {
    /// The D-Bus object path.
    pub path: String,
    /// The path of the device that sees it.
    pub device: String,
    /// The SSID as it is sent: up to 32 bytes, not always UTF-8. Empty for a hidden network.
    pub ssid: Vec<u8>,
    /// The signal strength in percent.
    pub strength: u8,
    /// How it is secured.
    pub security: Security,
    /// The frequency in MHz.
    pub frequency: u32,
    /// Its hardware address (BSSID).
    pub bssid: String,
}

/// What a saved connection is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionKind {
    /// A Wi-Fi network with this SSID.
    Wifi(Vec<u8>),
    /// Ethernet.
    Ethernet,
    /// A VPN through a NetworkManager plugin (OpenVPN, WireGuard's older plugin, …); the
    /// plugin's service name.
    Vpn(String),
    /// WireGuard, built into NetworkManager.
    WireGuard,
    /// Anything else; NetworkManager's connection type.
    Other(String),
}

/// A saved connection: a profile NetworkManager keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The D-Bus object path, which the actions take.
    pub path: String,
    /// Its name, without control characters. Untrusted: show it as plain text only.
    pub name: String,
    /// Its UUID.
    pub uuid: String,
    /// What it is for.
    pub kind: ConnectionKind,
}

impl Connection {
    /// Whether it is a VPN or WireGuard connection.
    pub fn is_vpn(&self) -> bool {
        matches!(
            self.kind,
            ConnectionKind::Vpn(_) | ConnectionKind::WireGuard
        )
    }
}

/// The state of an active connection, after `NMActiveConnectionState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveState {
    /// Unknown.
    Unknown,
    /// Connecting.
    Activating,
    /// Connected.
    Activated,
    /// Disconnecting.
    Deactivating,
    /// Gone.
    Deactivated,
}

impl ActiveState {
    fn from_nm(state: u32) -> Self {
        match state {
            1 => Self::Activating,
            2 => Self::Activated,
            3 => Self::Deactivating,
            4 => Self::Deactivated,
            _ => Self::Unknown,
        }
    }
}

/// A connection in use (or on its way).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveConnection {
    /// The D-Bus object path, which [`NetworkManager::deactivate`] takes.
    pub path: String,
    /// The path of the saved connection it uses.
    pub connection: String,
    /// Its state.
    pub state: ActiveState,
    /// The paths of the devices it runs on (none for most VPNs).
    pub devices: Vec<String>,
    /// Its IPv4 addresses, while it has some (a VPN's own).
    pub ip4: Option<IpConfig>,
    /// Its IPv6 addresses, while it has some.
    pub ip6: Option<IpConfig>,
}

/// Whether the machine reaches the internet, after NetworkManager's `NMConnectivityState`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Connectivity {
    /// Not checked (checks off, or not done yet).
    #[default]
    Unknown,
    /// No network.
    None,
    /// Behind a captive portal: a login page in the way.
    Portal,
    /// A network, but no internet.
    Limited,
    /// The internet.
    Full,
}

impl Connectivity {
    fn from_nm(value: u32) -> Self {
        match value {
            1 => Self::None,
            2 => Self::Portal,
            3 => Self::Limited,
            4 => Self::Full,
            _ => Self::Unknown,
        }
    }
}

/// What NetworkManager reports right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Whether networking is on at all (`nmcli networking`).
    pub networking_enabled: bool,
    /// Whether Wi-Fi is switched on.
    pub wifi_enabled: bool,
    /// Whether a hardware switch or key allows Wi-Fi. If not, it can't be switched on.
    pub wifi_hardware_enabled: bool,
    /// Whether the machine reaches the internet.
    pub connectivity: Connectivity,
    /// The address NetworkManager checks connectivity with, if it says.
    pub connectivity_uri: Option<String>,
    /// The devices, by interface name.
    pub devices: Vec<Device>,
    /// The access points of all Wi-Fi devices.
    pub access_points: Vec<AccessPoint>,
    /// The saved connections the user may see, by name.
    pub connections: Vec<Connection>,
    /// The active connections.
    pub active: Vec<ActiveConnection>,
}

/// A Wi-Fi network as the page lists it: the access points of one SSID seen by one device,
/// merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    /// The SSID.
    pub ssid: Vec<u8>,
    /// The SSID as text, without control characters. Untrusted: show it as plain text only.
    pub name: String,
    /// The strongest signal of its access points, in percent.
    pub strength: u8,
    /// How it is secured (as its strongest access point says).
    pub security: Security,
    /// The path of its strongest access point, which connecting takes.
    pub access_point: String,
    /// The paths of the saved connections for it.
    pub saved: Vec<String>,
    /// The active connection for it on the device, if there is one.
    pub active: Option<ActiveConnection>,
}

impl Snapshot {
    /// The device at `path`.
    pub fn device(&self, path: &str) -> Option<&Device> {
        self.devices.iter().find(|device| device.path == path)
    }

    /// The Wi-Fi devices NetworkManager manages, by interface name.
    pub fn wifi_devices(&self) -> impl Iterator<Item = &Device> {
        self.devices
            .iter()
            .filter(|device| device.kind == DeviceKind::Wifi && device.managed)
    }

    /// The Ethernet devices NetworkManager manages, by interface name.
    pub fn ethernet_devices(&self) -> impl Iterator<Item = &Device> {
        self.devices
            .iter()
            .filter(|device| device.kind == DeviceKind::Ethernet && device.managed)
    }

    /// The saved connection at `path`.
    pub fn connection(&self, path: &str) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|connection| connection.path == path)
    }

    /// The active connection at `path`.
    pub fn active_connection(&self, path: &str) -> Option<&ActiveConnection> {
        self.active.iter().find(|active| active.path == path)
    }

    /// The active connection of the saved connection at `path`, if it is in use.
    pub fn active_of(&self, connection: &str) -> Option<&ActiveConnection> {
        self.active
            .iter()
            .find(|active| active.connection == connection)
    }

    /// The VPN and WireGuard connections, by name.
    pub fn vpns(&self) -> impl Iterator<Item = &Connection> {
        self.connections
            .iter()
            .filter(|connection| connection.is_vpn())
    }

    /// The saved Wi-Fi connections for `ssid`.
    pub fn saved_for(&self, ssid: &[u8]) -> Vec<String> {
        self.connections
            .iter()
            .filter(|connection| matches!(&connection.kind, ConnectionKind::Wifi(s) if s == ssid))
            .map(|connection| connection.path.clone())
            .collect()
    }

    /// The Wi-Fi networks `device` sees, the one it is connected to (or connecting to) first,
    /// then by [`signal_level`] and name, so small changes of the signal don't reorder them. Hidden networks (without an SSID) are left out.
    pub fn networks(&self, device: &str) -> Vec<Network> {
        let mut networks: Vec<Network> = Vec::new();
        for ap in self
            .access_points
            .iter()
            .filter(|ap| ap.device == device && !ap.ssid.is_empty())
        {
            if let Some(network) = networks.iter_mut().find(|n| n.ssid == ap.ssid) {
                if ap.strength > network.strength {
                    network.strength = ap.strength;
                    network.security = ap.security;
                    network.access_point.clone_from(&ap.path);
                }
                continue;
            }
            let saved = self.saved_for(&ap.ssid);
            let active = self
                .active
                .iter()
                .find(|active| {
                    active.devices.iter().any(|d| d == device)
                        && saved.contains(&active.connection)
                        && active.state != ActiveState::Deactivated
                })
                .cloned();
            networks.push(Network {
                name: ssid_text(&ap.ssid),
                ssid: ap.ssid.clone(),
                strength: ap.strength,
                security: ap.security,
                access_point: ap.path.clone(),
                saved,
                active,
            });
        }
        networks.sort_by_cached_key(|network| {
            (
                network.active.is_none(),
                std::cmp::Reverse(signal_level(network.strength)),
                network.name.to_lowercase(),
                network.ssid.clone(),
            )
        });
        networks
    }

    /// The saved Wi-Fi networks `device` doesn't see now, one per SSID, by name.
    pub fn networks_away(&self, device: &str) -> Vec<SavedNetwork> {
        let seen: Vec<&[u8]> = self
            .access_points
            .iter()
            .filter(|ap| ap.device == device)
            .map(|ap| ap.ssid.as_slice())
            .collect();
        let mut away: Vec<SavedNetwork> = Vec::new();
        for connection in &self.connections {
            let ConnectionKind::Wifi(ssid) = &connection.kind else {
                continue;
            };
            if seen.contains(&ssid.as_slice()) {
                continue;
            }
            if let Some(network) = away.iter_mut().find(|n| n.ssid == *ssid) {
                network.saved.push(connection.path.clone());
                continue;
            }
            let name = ssid_text(ssid);
            away.push(SavedNetwork {
                name: if name.is_empty() {
                    connection.name.clone()
                } else {
                    name
                },
                ssid: ssid.clone(),
                saved: vec![connection.path.clone()],
            });
        }
        away.sort_by_cached_key(|network| (network.name.to_lowercase(), network.ssid.clone()));
        away
    }
}

/// A saved Wi-Fi network that isn't around.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedNetwork {
    /// The SSID.
    pub ssid: Vec<u8>,
    /// The SSID as text (else the connection's name), without control characters. Untrusted:
    /// show it as plain text only.
    pub name: String,
    /// The paths of its saved connections.
    pub saved: Vec<String>,
}

/// The signal level of `strength` (percent) in bars, 0 to 4, as GNOME and the symbolic icons
/// (`network-wireless-signal-none`, `weak`, `ok`, `good`, `excellent`) show it.
pub fn signal_level(strength: u8) -> u8 {
    match strength {
        81.. => 4,
        56..=80 => 3,
        31..=55 => 2,
        6..=30 => 1,
        _ => 0,
    }
}

/// The frequency band of `frequency` (MHz): `2.4 GHz`, `5 GHz` or `6 GHz`; `None` for others.
pub fn band(frequency: u32) -> Option<&'static str> {
    match frequency {
        2400..=2500 => Some("2.4 GHz"),
        5150..=5895 => Some("5 GHz"),
        5925..=7125 => Some("6 GHz"),
        _ => None,
    }
}

/// `ssid` as text: UTF-8 where it is, the replacement character where it isn't, without control
/// characters.
pub fn ssid_text(ssid: &[u8]) -> String {
    clean(&String::from_utf8_lossy(ssid))
}

/// The longest SSID, in bytes.
pub const MAX_SSID_BYTES: usize = 32;

/// Why text can't be an SSID or a password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
    /// No SSID.
    EmptySsid,
    /// An SSID longer than [`MAX_SSID_BYTES`] bytes.
    SsidTooLong,
    /// A password shorter than 8 characters (WPA Personal).
    PasswordTooShort,
    /// A password longer than 63 characters that isn't a 64-digit hexadecimal key.
    PasswordTooLong,
    /// An empty password (WPA3 Personal).
    EmptyPassword,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptySsid => "The network name is empty",
            Self::SsidTooLong => "The network name is longer than 32 bytes",
            Self::PasswordTooShort => "The password is shorter than 8 characters",
            Self::PasswordTooLong => "The password is longer than 63 characters",
            Self::EmptyPassword => "The password is empty",
        })
    }
}

/// Checks `ssid` typed in for a hidden network: 1 to 32 bytes of UTF-8, taken as it is.
///
/// # Errors
///
/// The [`InputError`] that rules it out.
pub fn check_ssid(ssid: &str) -> Result<(), InputError> {
    if ssid.is_empty() {
        Err(InputError::EmptySsid)
    } else if ssid.len() > MAX_SSID_BYTES {
        Err(InputError::SsidTooLong)
    } else {
        Ok(())
    }
}

/// Checks `password` for a network with `security`, as NetworkManager does: WPA Personal takes 8
/// to 63 bytes or 64 hexadecimal digits, WPA3 Personal anything but nothing. Other kinds take no
/// password.
///
/// # Errors
///
/// The [`InputError`] that rules it out.
pub fn check_password(security: Security, password: &str) -> Result<(), InputError> {
    match security {
        Security::Personal => {
            if password.len() == 64 && password.chars().all(|c| c.is_ascii_hexdigit()) {
                Ok(())
            } else if password.len() < 8 {
                Err(InputError::PasswordTooShort)
            } else if password.len() > 63 {
                Err(InputError::PasswordTooLong)
            } else {
                Ok(())
            }
        }
        Security::Wpa3Personal if password.is_empty() => Err(InputError::EmptyPassword),
        _ => Ok(()),
    }
}

/// Why NetworkManager can't be read or an action failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No system bus, or NetworkManager isn't running on it. The text says which.
    Unavailable(String),
    /// NetworkManager (polkit) didn't allow it. The text is NetworkManager's, so untrusted.
    Denied(String),
    /// An SSID or password that can't be used: see [`check_ssid`] and [`check_password`].
    Invalid(InputError),
    /// Connecting failed: NetworkManager's `NMDeviceStateReason` (see [`reason_text`]).
    Connect(u32),
    /// A VPN failed to connect: NetworkManager's `NMActiveConnectionStateReason` (see
    /// [`vpn_reason_text`]).
    Vpn(u32),
    /// NetworkManager refused or failed: its message (untrusted text).
    Failed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "The network isn't available: {reason}"),
            Self::Denied(reason) if reason.is_empty() => f.write_str("Not allowed"),
            Self::Denied(reason) => write!(f, "Not allowed: {reason}"),
            Self::Invalid(error) => error.fmt(f),
            Self::Connect(reason) => f.write_str(&reason_text(*reason)),
            Self::Vpn(reason) => f.write_str(&vpn_reason_text(*reason)),
            Self::Failed(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for Error {}

/// What NetworkManager's `NMDeviceStateReason` `reason` means, for a failed connection.
pub fn reason_text(reason: u32) -> String {
    match reason {
        7 => "No password was given".to_owned(),
        8 => "The network refused the connection; the password may be wrong".to_owned(),
        9..=11 => "The network didn't accept the connection".to_owned(),
        5 | 6 | 15..=17 => "The network gave no IP address".to_owned(),
        40 => "The cable is unplugged".to_owned(),
        53 => "The network isn't in range".to_owned(),
        other => format!("NetworkManager gave up (reason {other})"),
    }
}

/// What NetworkManager's `NMActiveConnectionStateReason` `reason` means, for a VPN that failed.
pub fn vpn_reason_text(reason: u32) -> String {
    match reason {
        9 => "The VPN asks for a password, which Sway Things can't ask for yet. Save the password \
              with Edit…, or run nm-applet"
            .to_owned(),
        10 => "The VPN refused the login".to_owned(),
        6 | 7 => "The VPN didn't answer in time".to_owned(),
        8 => "The VPN's plugin didn't start".to_owned(),
        5 => "The VPN gave no usable address".to_owned(),
        other => format!("The VPN connection ended (reason {other})"),
    }
}

/// `text` without control characters (newlines, escape sequences), which an SSID could carry,
/// and without surrounding white space.
pub(crate) fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(path: &str, ssid: &str, strength: u8, security: Security) -> AccessPoint {
        AccessPoint {
            path: path.to_owned(),
            device: "/dev/wlan0".to_owned(),
            ssid: ssid.as_bytes().to_vec(),
            strength,
            security,
            frequency: 2412,
            bssid: String::new(),
        }
    }

    fn wifi(path: &str, ssid: &str) -> Connection {
        Connection {
            path: path.to_owned(),
            name: ssid.to_owned(),
            uuid: String::new(),
            kind: ConnectionKind::Wifi(ssid.as_bytes().to_vec()),
        }
    }

    #[test]
    fn security_follows_the_flags() {
        assert_eq!(Security::from_flags(0, 0, 0), Security::Open);
        assert_eq!(Security::from_flags(1, 0, 0), Security::Wep);
        assert_eq!(Security::from_flags(1, 0, 0x188), Security::Personal);
        // WPA3 transition mode takes WPA2 too.
        assert_eq!(Security::from_flags(1, 0, 0x588), Security::Personal);
        assert_eq!(Security::from_flags(1, 0, 0x488), Security::Wpa3Personal);
        assert_eq!(Security::from_flags(1, 0, 0x888), Security::EnhancedOpen);
        assert_eq!(Security::from_flags(1, 0x200, 0x288), Security::Enterprise);
        assert_eq!(Security::from_flags(1, 0, 0x2000), Security::Enterprise);
        assert!(Security::Personal.needs_password());
        assert!(!Security::EnhancedOpen.needs_password());
        assert!(!Security::Enterprise.supported() && !Security::Wep.supported());
    }

    #[test]
    fn networks_merge_access_points_and_sort() {
        let active = ActiveConnection {
            path: "/active/0".to_owned(),
            connection: "/settings/home".to_owned(),
            state: ActiveState::Activated,
            devices: vec!["/dev/wlan0".to_owned()],
            ip4: None,
            ip6: None,
        };
        let mut other_device = ap("/ap/9", "Elsewhere", 99, Security::Open);
        other_device.device = "/dev/wlan1".to_owned();
        let snapshot = Snapshot {
            access_points: vec![
                ap("/ap/1", "Cafe", 40, Security::Open),
                ap("/ap/2", "Home", 30, Security::Personal),
                ap("/ap/3", "Home", 70, Security::Personal),
                ap("/ap/4", "", 90, Security::Personal),
                ap("/ap/5", "Bar", 40, Security::Open),
                other_device,
            ],
            connections: vec![wifi("/settings/home", "Home"), wifi("/settings/old", "Old")],
            active: vec![active.clone()],
            ..Snapshot::default()
        };
        let networks = snapshot.networks("/dev/wlan0");
        let names: Vec<&str> = networks.iter().map(|n| n.name.as_str()).collect();
        // Bar and Cafe have the same level: by name.
        assert_eq!(names, ["Home", "Bar", "Cafe"]);
        assert_eq!(networks[0].strength, 70);
        assert_eq!(networks[0].access_point, "/ap/3");
        assert_eq!(networks[0].saved, ["/settings/home"]);
        assert_eq!(networks[0].active, Some(active));
        assert!(networks[1].saved.is_empty() && networks[1].active.is_none());
    }

    #[test]
    fn signal_levels_and_bands() {
        let levels: Vec<u8> = [0, 5, 6, 30, 31, 55, 56, 80, 81, 100]
            .into_iter()
            .map(signal_level)
            .collect();
        assert_eq!(levels, [0, 0, 1, 1, 2, 2, 3, 3, 4, 4]);
        assert_eq!(band(2412), Some("2.4 GHz"));
        assert_eq!(band(5180), Some("5 GHz"));
        assert_eq!(band(5955), Some("6 GHz"));
        assert_eq!(band(60_480), None);
        assert!(vpn_reason_text(9).contains("password"));
    }

    #[test]
    fn saved_networks_away_are_grouped_by_ssid() {
        let mut second = wifi("/settings/home2", "Home");
        second.name = "Home 1".to_owned();
        let snapshot = Snapshot {
            access_points: vec![ap("/ap/1", "Cafe", 40, Security::Open)],
            connections: vec![
                wifi("/settings/cafe", "Cafe"),
                wifi("/settings/home", "Home"),
                second,
                wifi("/settings/attic", "attic"),
            ],
            ..Snapshot::default()
        };
        let away = snapshot.networks_away("/dev/wlan0");
        let names: Vec<(&str, usize)> = away
            .iter()
            .map(|n| (n.name.as_str(), n.saved.len()))
            .collect();
        assert_eq!(names, [("attic", 1), ("Home", 2)]);
    }

    #[test]
    fn ssids_become_clean_text() {
        assert_eq!(ssid_text(b"K\xc3\xb6ln\n\x1b[31m"), "Köln  [31m");
        assert_eq!(ssid_text(b"\xff\xfeAB"), "\u{fffd}\u{fffd}AB");
    }

    #[test]
    fn ssids_and_passwords_are_checked() {
        assert_eq!(check_ssid(""), Err(InputError::EmptySsid));
        assert_eq!(check_ssid(&"x".repeat(32)), Ok(()));
        assert_eq!(check_ssid(&"ä".repeat(17)), Err(InputError::SsidTooLong));
        assert_eq!(check_ssid(" spaced "), Ok(()));

        let personal = |p: &str| check_password(Security::Personal, p);
        assert_eq!(personal("1234567"), Err(InputError::PasswordTooShort));
        assert_eq!(personal("12345678"), Ok(()));
        assert_eq!(personal(&"a".repeat(63)), Ok(()));
        assert_eq!(personal(&"a".repeat(64)), Ok(()));
        assert_eq!(personal(&"g".repeat(64)), Err(InputError::PasswordTooLong));
        assert_eq!(
            check_password(Security::Wpa3Personal, ""),
            Err(InputError::EmptyPassword)
        );
        assert_eq!(check_password(Security::Wpa3Personal, "short"), Ok(()));
        assert_eq!(check_password(Security::Open, ""), Ok(()));
    }

    #[test]
    fn device_states_and_reasons_are_read() {
        assert_eq!(DeviceState::from_nm(70), DeviceState::Connecting);
        assert_eq!(DeviceState::from_nm(60), DeviceState::NeedAuth);
        assert_eq!(DeviceState::from_nm(100), DeviceState::Connected);
        assert_eq!(DeviceState::from_nm(7), DeviceState::Unknown);
        assert_eq!(DeviceKind::from_nm(2), DeviceKind::Wifi);
        assert_eq!(Connectivity::from_nm(2), Connectivity::Portal);
        assert!(reason_text(8).contains("password"));
        assert_eq!(reason_text(999), "NetworkManager gave up (reason 999)");
    }
}
