// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decoding NetworkManager's properties into a [`Snapshot`]: devices, access points, saved
//! connections and IP configuration. Pure functions, without D-Bus calls.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use super::super::agent::Settings;
use super::super::{
    AccessPoint, ActiveConnection, ActiveState, Connection as Saved, ConnectionKind, Connectivity,
    Device, DeviceKind, DeviceState, IpConfig, Security, Snapshot, clean,
};
use super::{ACCESS_POINT, ACTIVE, DEVICE, IP4, IP6, NONE, Objects, Properties, WIRED, WIRELESS};
pub(super) use crate::dbus::{flag, number, text};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

/// The snapshot of NetworkManager's `manager` properties, its `objects` and the `settings` of
/// its saved connections (by path).
pub(super) fn snapshot(
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
pub(super) fn access_point(path: &str, device: &str, properties: &Properties) -> AccessPoint {
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
pub(super) fn saved(path: &str, settings: &Settings) -> Saved {
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
pub(super) fn ip_config(properties: &Properties, interface: &str) -> IpConfig {
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
pub(super) fn state_reason(properties: &Properties) -> u32 {
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
pub(super) fn bytes(value: &OwnedValue) -> Option<Vec<u8>> {
    value_bytes(value)
}

/// `value` as bytes, if it is an array of bytes.
pub(in crate::network) fn value_bytes(value: &Value<'_>) -> Option<Vec<u8>> {
    match value {
        Value::Array(array) => array.iter().map(|v| u8::try_from(v).ok()).collect(),
        Value::Value(inner) => value_bytes(inner),
        _ => None,
    }
}

/// The array of dictionaries `name` (`aa{sv}`).
pub(super) fn dicts(properties: &Properties, name: &str) -> Vec<HashMap<String, OwnedValue>> {
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
pub(super) fn paths(properties: &Properties, name: &str) -> Vec<String> {
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
pub(super) fn object_ref(properties: &Properties, name: &str) -> Option<String> {
    let path = <&ObjectPath<'_>>::try_from(properties.get(name)?).ok()?;
    (path.as_str() != NONE).then(|| path.to_string())
}
