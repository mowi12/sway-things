// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How the Network page says things: the details of a device and its addresses, the signal and
//! the security of a network, the kind of a VPN.

use std::fmt::Write as _;
use sway_things_core::network::{ConnectionKind, Device, Error, IpConfig, Security};

use crate::i18n::{Msg, msg, tr, trf};

/// The details of `device`: its speed, addresses and hardware address.
pub(super) fn device_details(device: &Device) -> Vec<(&'static str, String)> {
    let mut rows = Vec::new();
    if let Some(speed) = device.speed {
        rows.push((
            tr(msg!("Link speed")),
            trf(msg!("{speed} Mb/s"), &[("speed", &speed)]),
        ));
    }
    rows.extend(ip_details(device.ip4.as_ref(), device.ip6.as_ref()));
    if !device.hw_address.is_empty() {
        rows.push((tr(msg!("Hardware address")), device.hw_address.clone()));
    }
    rows.push((tr(msg!("Interface")), device.interface.clone()));
    rows
}

/// The addresses, gateways and DNS servers of `ip4` and `ip6`.
pub(super) fn ip_details(
    ip4: Option<&IpConfig>,
    ip6: Option<&IpConfig>,
) -> Vec<(&'static str, String)> {
    let mut rows = Vec::new();
    for (label, config) in [
        (tr(msg!("IPv4 address")), ip4),
        (tr(msg!("IPv6 address")), ip6),
    ] {
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
        rows.push((tr(msg!("Gateway")), gateways.join("\n")));
    }
    let dns: Vec<&str> = [ip4, ip6]
        .into_iter()
        .flatten()
        .flat_map(|c| c.dns.iter().map(String::as_str))
        .collect();
    if !dns.is_empty() {
        rows.push((tr(msg!("DNS")), dns.join("\n")));
    }
    rows
}

/// The first IPv4 address of `config`, without its prefix length.
pub(super) fn first_address(config: Option<&IpConfig>) -> Option<String> {
    let address = config?.addresses.first()?;
    Some(address.split('/').next().unwrap_or(address).to_owned())
}

/// `bytes` in hexadecimal, as a key that holds any SSID.
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// The symbolic icon of a signal `level` (0 to 4 bars).
pub(super) fn signal_icon(level: u8) -> &'static str {
    match level {
        4 => "network-wireless-signal-excellent-symbolic",
        3 => "network-wireless-signal-good-symbolic",
        2 => "network-wireless-signal-ok-symbolic",
        1 => "network-wireless-signal-weak-symbolic",
        _ => "network-wireless-signal-none-symbolic",
    }
}

/// How good a signal of `level` (0 to 4) is, as the details say it.
pub(super) fn signal_quality(level: u8) -> &'static str {
    let quality = match level {
        4 => {
            // TRANSLATORS: How good the signal of a Wi-Fi network is: the best of four levels.
            msg!("Excellent")
        }
        3 => {
            // TRANSLATORS: How good the signal of a Wi-Fi network is.
            msg!("Good")
        }
        2 => {
            // TRANSLATORS: How good the signal of a Wi-Fi network is.
            msg!("OK")
        }
        1 => {
            // TRANSLATORS: How good the signal of a Wi-Fi network is.
            msg!("Weak")
        }
        _ => {
            // TRANSLATORS: How good the signal of a Wi-Fi network is: no signal.
            msg!("None")
        }
    };
    tr(quality)
}

/// The signal of `level` (0 to 4), for screen readers.
pub(super) fn signal_label(level: u8) -> &'static str {
    tr(match level {
        4 => msg!("Signal excellent"),
        3 => msg!("Signal good"),
        2 => msg!("Signal ok"),
        1 => msg!("Signal weak"),
        _ => msg!("Signal none"),
    })
}

/// The message of an action on `name` that failed with `error`.
pub(super) fn failed(text: Msg, name: &str, error: &Error) -> String {
    trf(
        text,
        &[
            ("network", &name),
            ("vpn", &name),
            ("error", &error.text().translated()),
        ],
    )
}

/// How a row names `security`.
pub(super) fn security_text(security: Security) -> &'static str {
    match security {
        Security::Open => tr(msg!("Open, not encrypted")),
        Security::EnhancedOpen => tr(msg!("Enhanced Open")),
        Security::Wep => tr(msg!("WEP, insecure")),
        Security::Personal => "WPA2",
        Security::Wpa3Personal => "WPA3",
        Security::Enterprise => tr(msg!("Enterprise (802.1X)")),
    }
}

/// How the details name `security`.
pub(super) fn security_name(security: Security) -> &'static str {
    match security {
        Security::Open => {
            // TRANSLATORS: The security of a Wi-Fi network: none.
            tr(msg!("None"))
        }
        Security::EnhancedOpen => tr(msg!("Enhanced Open (OWE)")),
        Security::Wep => "WEP",
        Security::Personal => tr(msg!("WPA2 Personal")),
        Security::Wpa3Personal => tr(msg!("WPA3 Personal")),
        Security::Enterprise => tr(msg!("WPA Enterprise (802.1X)")),
    }
}

/// The kind of a VPN connection, as its row names it.
pub(super) fn vpn_kind(kind: &ConnectionKind) -> &'static str {
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
        assert_eq!(signal_quality(3), "Good");
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
