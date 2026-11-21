// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use zbus::zvariant::{Array, StructureBuilder};

use super::*;
use crate::network::{ConnectionKind, Connectivity, DeviceState, IpConfig};

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
    let settings = wifi_settings(b"Home", Security::Personal, Some("password1"), false).unwrap();
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
