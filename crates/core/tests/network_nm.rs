// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The NetworkManager backend against python-dbusmock's `networkmanager` template on a private
//! bus.
//!
//! These tests need `dbus-daemon` and python-dbusmock, so a plain `cargo test` skips them.
//! `scripts/check-integration.sh` runs them with `--ignored` in the containers, which have both.

use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::{StreamExt, future};
use sway_things_core::network::stand_in::{self, StandIn};
use sway_things_core::network::{
    ActiveState, AgentEvent, Connectivity, DeviceKind, DeviceState, Error as NetError,
    NetworkManager, SecretPrompt, Security, Snapshot,
};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const MANAGER: &str = "/org/freedesktop/NetworkManager";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
/// `NM_SECRET_AGENT_GET_SECRETS_FLAG_ALLOW_INTERACTION`.
const ALLOW_INTERACTION: u32 = 0x1;
/// `NM_SECRET_AGENT_GET_SECRETS_FLAG_REQUEST_NEW`.
const REQUEST_NEW: u32 = 0x2;

/// Runs a test, which fails instead of hanging if the stand-in doesn't answer.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(30)).await;
        Err("the stand-in for NetworkManager didn't answer within 30 seconds".into())
    }))
}

/// The stand-in with the Wi-Fi device `wlan0`, which sees "Home" (WPA2, two access points) and
/// "Cafe" (open); our connection to it, and the device's path.
async fn with_wifi() -> Result<(StandIn, NetworkManager, String)> {
    let stand_in = StandIn::start().await?;
    let wlan = stand_in.add_wifi_device("wlan0", "wlan0").await?;
    stand_in
        .add_access_point(
            &wlan,
            "home1",
            "Home",
            "00:11:22:33:44:01",
            40,
            stand_in::WPA2,
        )
        .await?;
    stand_in
        .add_access_point(
            &wlan,
            "home2",
            "Home",
            "00:11:22:33:44:02",
            90,
            stand_in::WPA2,
        )
        .await?;
    stand_in
        .add_access_point(
            &wlan,
            "cafe",
            "Cafe",
            "00:11:22:33:44:03",
            60,
            stand_in::OPEN,
        )
        .await?;
    let nm = stand_in.network_manager().await?;
    Ok((stand_in, nm, wlan))
}

/// Waits until the snapshot satisfies `done`.
async fn until(nm: &NetworkManager, done: impl Fn(&Snapshot) -> bool) -> Result<Snapshot> {
    for _ in 0..50 {
        let snapshot = nm.snapshot().await?;
        if done(&snapshot) {
            return Ok(snapshot);
        }
        Timer::after(Duration::from_millis(100)).await;
    }
    Err(format!("the snapshot didn't change: {:?}", nm.snapshot().await?).into())
}

/// The next prompt of the agent.
async fn next_prompt(nm: &NetworkManager) -> Result<SecretPrompt> {
    match nm.agent_events().recv().await? {
        AgentEvent::Prompt(prompt) => Ok(prompt),
        AgentEvent::Cancel(connection) => Err(format!("canceled: {connection}").into()),
    }
}

/// The settings of the call `index` of `method` on the manager.
async fn sent_settings(
    stand_in: &StandIn,
    method: &str,
    index: usize,
) -> Result<HashMap<String, HashMap<String, OwnedValue>>> {
    let calls = stand_in.calls(MANAGER, method).await?;
    let call = calls.get(index).ok_or("no such call")?;
    let settings = call.first().ok_or("no settings")?.try_clone()?;
    Ok(settings.try_into()?)
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_snapshot_has_devices_networks_and_connections() -> Result<()> {
    run(async {
        let (stand_in, nm, wlan) = with_wifi().await?;
        let eth = stand_in
            .add_ethernet_device("eth0", "eth0", stand_in::UNAVAILABLE)
            .await?;
        let saved = stand_in.add_wifi_connection(&wlan, "home", "Home").await?;
        let vpn = stand_in.add_connection(&vpn_settings("Office")).await?;
        let snapshot = nm.snapshot().await?;

        assert!(snapshot.wifi_enabled && snapshot.wifi_hardware_enabled);
        assert_eq!(snapshot.connectivity, Connectivity::Full);
        let wifi: Vec<&str> = snapshot
            .wifi_devices()
            .map(|d| d.interface.as_str())
            .collect();
        assert_eq!(wifi, ["wlan0"]);
        let wired = snapshot
            .ethernet_devices()
            .next()
            .ok_or("no wired device")?;
        assert_eq!(
            (wired.path.as_str(), wired.kind, wired.state),
            (eth.as_str(), DeviceKind::Ethernet, DeviceState::Unavailable)
        );

        let networks = snapshot.networks(&wlan);
        let names: Vec<(&str, u8, Security)> = networks
            .iter()
            .map(|n| (n.name.as_str(), n.strength, n.security))
            .collect();
        assert_eq!(
            names,
            [
                ("Home", 90, Security::Personal),
                ("Cafe", 60, Security::Open)
            ]
        );
        assert_eq!(networks[0].saved, std::slice::from_ref(&saved));
        assert!(networks[0].access_point.ends_with("/home2"));

        let vpns: Vec<&str> = snapshot.vpns().map(|c| c.name.as_str()).collect();
        assert_eq!(vpns, ["Office"]);
        assert!(snapshot.connection(&vpn).is_some());
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn network_manager_going_away_and_coming_back_is_reported() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start().await?;
        let nm = stand_in.network_manager().await?;
        let first = nm.owner().await.ok_or("no owner")?;
        let version = nm.version().await.ok_or("no version")?;
        assert!(version.contains('.'), "{version}");
        let mut changes = nm.changes().await?;
        stand_in.stop_nm();
        changes.next().await.ok_or("the stream ended")?;
        assert!(matches!(nm.snapshot().await, Err(NetError::Unavailable(_))));
        assert_eq!(nm.owner().await, None);
        assert_eq!(nm.version().await, None);
        stand_in.start_nm().await?;
        changes.next().await.ok_or("the stream ended")?;
        // Another program now: our agent would have to register again.
        assert!(nm.owner().await.is_some_and(|owner| owner != first));
        let snapshot = nm.snapshot().await?;
        assert_eq!(snapshot.devices.len(), 0);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn wifi_is_switched_and_scans_are_requested() -> Result<()> {
    run(async {
        let (stand_in, nm, wlan) = with_wifi().await?;
        nm.set_wifi_enabled(false).await?;
        until(&nm, |s| !s.wifi_enabled).await?;
        nm.set_wifi_enabled(true).await?;
        until(&nm, |s| s.wifi_enabled).await?;
        let before = nm.last_scan(&wlan).await?;
        assert_eq!(before, -1);
        nm.request_scan(&wlan).await?;
        assert_eq!(stand_in.calls(&wlan, "RequestScan").await?.len(), 1);
        assert!(nm.last_scan(&wlan).await? > before);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn a_new_network_is_saved_with_its_password_for_everyone() -> Result<()> {
    run(async {
        let (stand_in, nm, wlan) = with_wifi().await?;
        let snapshot = nm.snapshot().await?;
        let device = snapshot.device(&wlan).ok_or("no device")?.clone();
        let home = snapshot.networks(&wlan).remove(0);

        assert!(matches!(
            nm.connect_network(&device, &home, Some("short")).await,
            Err(NetError::Invalid(_))
        ));
        let active = nm
            .connect_network(&device, &home, Some("password1"))
            .await?;
        nm.wait_connected(&active, Some(&wlan)).await?;

        let settings = sent_settings(&stand_in, "AddAndActivateConnection", 0).await?;
        let security = &settings["802-11-wireless-security"];
        assert_eq!(security["key-mgmt"], Value::from("wpa-psk").try_into()?);
        assert_eq!(security["psk"], Value::from("password1").try_into()?);
        assert_eq!(security["psk-flags"], Value::from(0u32).try_into()?);
        assert!(!settings["connection"].contains_key("permissions"));
        let calls = stand_in.calls(MANAGER, "AddAndActivateConnection").await?;
        assert_eq!(
            calls[0][2],
            Value::from(ObjectPath::try_from(home.access_point.as_str())?).try_into()?
        );

        let snapshot = until(&nm, |s| {
            s.networks(&wlan)
                .first()
                .is_some_and(|n| n.active.is_some())
        })
        .await?;
        let home = &snapshot.networks(&wlan)[0];
        assert_eq!(home.name, "Home");
        assert_eq!(home.saved.len(), 1);
        assert_eq!(
            home.active.as_ref().map(|a| a.state),
            Some(ActiveState::Activated)
        );
        assert_eq!(
            snapshot.device(&wlan).map(|d| d.state),
            Some(DeviceState::Connected)
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn a_saved_network_is_activated_disconnected_and_forgotten() -> Result<()> {
    run(async {
        let (stand_in, nm, wlan) = with_wifi().await?;
        let saved = stand_in.add_wifi_connection(&wlan, "home", "Home").await?;
        let snapshot = nm.snapshot().await?;
        let device = snapshot.device(&wlan).ok_or("no device")?.clone();
        let home = snapshot.networks(&wlan).remove(0);
        let active = nm.connect_network(&device, &home, None).await?;
        nm.wait_connected(&active, Some(&wlan)).await?;
        let calls = stand_in.calls(MANAGER, "ActivateConnection").await?;
        assert_eq!(
            calls[0][0],
            Value::from(ObjectPath::try_from(saved.as_str())?).try_into()?
        );
        assert_eq!(
            stand_in
                .calls(MANAGER, "AddAndActivateConnection")
                .await?
                .len(),
            0
        );

        nm.disconnect_device(&wlan).await?;
        until(&nm, |s| {
            s.active.is_empty()
                && s.device(&wlan)
                    .is_some_and(|d| d.state == DeviceState::Disconnected)
        })
        .await?;

        nm.forget(&home.saved).await?;
        let snapshot = until(&nm, |s| s.connections.is_empty()).await?;
        assert_eq!(snapshot.networks(&wlan)[0].saved.len(), 0);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn a_hidden_network_is_added_by_its_name() -> Result<()> {
    run(async {
        let (stand_in, nm, wlan) = with_wifi().await?;
        assert!(matches!(
            nm.connect_hidden(&wlan, "", Security::Personal, Some("password1"))
                .await,
            Err(NetError::Invalid(_))
        ));
        let active = nm
            .connect_hidden(&wlan, "Lab", Security::Wpa3Personal, Some("secret"))
            .await?;
        nm.wait_connected(&active, Some(&wlan)).await?;
        let settings = sent_settings(&stand_in, "AddAndActivateConnection", 0).await?;
        assert_eq!(
            settings["802-11-wireless"]["hidden"],
            Value::from(true).try_into()?
        );
        assert_eq!(
            settings["802-11-wireless"]["ssid"],
            Value::from(b"Lab".to_vec()).try_into()?
        );
        assert_eq!(
            settings["802-11-wireless-security"]["key-mgmt"],
            Value::from("sae").try_into()?
        );
        let calls = stand_in.calls(MANAGER, "AddAndActivateConnection").await?;
        assert_eq!(
            calls[0][2],
            Value::from(ObjectPath::try_from("/")?).try_into()?
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn a_failed_connection_says_why() -> Result<()> {
    run(async {
        let (stand_in, nm, wlan) = with_wifi().await?;
        // NetworkManager gives up for want of a password: the device fails with reason 7 and
        // the active connection goes.
        stand_in
            .add_method(
                MANAGER,
                "org.freedesktop.NetworkManager",
                "AddAndActivateConnection",
                "a{sa{sv}}oo",
                "oo",
                &format!(
                    "objects['{wlan}'].Set('{DEVICE}', 'StateReason', \
                     dbus.Struct((dbus.UInt32(120), dbus.UInt32(7)), signature='uu'))\n\
                     ret = (dbus.ObjectPath('/org/freedesktop/NetworkManager/Settings/9'), \
                     dbus.ObjectPath('/org/freedesktop/NetworkManager/ActiveConnection/9'))\n"
                ),
            )
            .await?;
        let snapshot = nm.snapshot().await?;
        let device = snapshot.device(&wlan).ok_or("no device")?.clone();
        let home = snapshot.networks(&wlan).remove(0);
        let active = nm
            .connect_network(&device, &home, Some("password1"))
            .await?;
        assert_eq!(
            nm.wait_connected(&active, Some(&wlan)).await,
            Err(NetError::Connect(7))
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn wired_and_vpn_connections_are_switched() -> Result<()> {
    run(async {
        let (stand_in, nm, wlan) = with_wifi().await?;
        let eth = stand_in
            .add_ethernet_device("eth0", "eth0", stand_in::DISCONNECTED)
            .await?;
        let wired = stand_in.add_connection(&ethernet_settings("Wired")).await?;
        stand_in.make_available(&eth, &wired).await?;
        let vpn = stand_in.add_connection(&vpn_settings("Office")).await?;

        let active = nm.connect_device(&eth).await?;
        nm.wait_connected(&active, Some(&eth)).await?;
        let snapshot = nm.snapshot().await?;
        assert_eq!(
            snapshot
                .active_connection(&active)
                .map(|a| a.connection.as_str()),
            Some(wired.as_str())
        );

        let tunnel = nm.activate(&vpn, None).await?;
        nm.wait_connected(&tunnel, None).await?;
        let snapshot = until(&nm, |s| s.active_of(&vpn).is_some()).await?;
        assert!(
            snapshot
                .active_of(&vpn)
                .is_some_and(|a| a.devices.is_empty())
        );

        nm.deactivate(&tunnel).await?;
        let snapshot = until(&nm, |s| s.active_of(&vpn).is_none()).await?;
        // Only the VPN went.
        assert_eq!(
            snapshot.device(&eth).map(|d| d.state),
            Some(DeviceState::Connected)
        );
        assert_eq!(
            snapshot.device(&wlan).map(|d| d.state),
            Some(DeviceState::Disconnected)
        );

        nm.disconnect_device(&eth).await?;
        until(&nm, |s| s.active_of(&wired).is_none()).await?;
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn networking_is_turned_back_on() -> Result<()> {
    run(async {
        let (_stand_in, nm, _) = with_wifi().await?;
        nm.set_networking_enabled(false).await?;
        until(&nm, |s| !s.networking_enabled).await?;
        nm.set_networking_enabled(true).await?;
        until(&nm, |s| s.networking_enabled).await?;
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn a_vpn_without_its_password_says_so() -> Result<()> {
    run(async {
        let (stand_in, nm, _) = with_wifi().await?;
        let vpn = stand_in.add_connection(&vpn_settings("Office")).await?;
        let active = "/org/freedesktop/NetworkManager/ActiveConnection/99";
        let interface = "org.freedesktop.NetworkManager.Connection.Active";
        // The VPN starts to connect, then NetworkManager gives up for want of secrets (reason
        // 9) and the active connection goes.
        stand_in
            .add_method(
                MANAGER,
                "org.freedesktop.NetworkManager",
                "ActivateConnection",
                "ooo",
                "o",
                &format!(
                    "self.AddObject('{active}', '{interface}', \
                     {{'State': dbus.UInt32(1)}}, [])\n\
                     ret = dbus.ObjectPath('{active}')\n"
                ),
            )
            .await?;
        stand_in
            .add_method(
                MANAGER,
                "org.freedesktop.NetworkManager.Test",
                "FailVpn",
                "",
                "",
                &format!(
                    "objects['{active}'].EmitSignal('{interface}', 'StateChanged', 'uu', \
                     [dbus.UInt32(4), dbus.UInt32(9)])\n\
                     self.RemoveObject('{active}')\n"
                ),
            )
            .await?;
        let path = nm.activate(&vpn, None).await?;
        assert_eq!(path, active);
        let (result, failed) = future::zip(nm.wait_connected(&path, None), async {
            Timer::after(Duration::from_millis(300)).await;
            stand_in
                .connection()
                .call_method(
                    Some("org.freedesktop.NetworkManager"),
                    MANAGER,
                    Some("org.freedesktop.NetworkManager.Test"),
                    "FailVpn",
                    &(),
                )
                .await
        })
        .await;
        failed?;
        assert_eq!(result, Err(NetError::Vpn(9)));
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn details_have_the_addresses() -> Result<()> {
    run(async {
        let (mut stand_in, nm, wlan) = with_wifi().await?;
        stand_in
            .set_ip4(&wlan, "192.168.1.20", 24, "192.168.1.1", "192.168.1.1")
            .await?;
        let snapshot = until(&nm, |s| s.device(&wlan).is_some_and(|d| d.ip4.is_some())).await?;
        let ip4 = snapshot
            .device(&wlan)
            .and_then(|d| d.ip4.clone())
            .ok_or("no IPv4")?;
        assert_eq!(ip4.addresses, ["192.168.1.20/24"]);
        assert_eq!(ip4.gateway.as_deref(), Some("192.168.1.1"));
        assert_eq!(ip4.dns, ["192.168.1.1"]);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_change_stream_skips_the_noise() -> Result<()> {
    run(async {
        let (stand_in, nm, wlan) = with_wifi().await?;
        let ap = nm.snapshot().await?.access_points[0].path.clone();
        let mut changes = nm.changes().await?;
        let ap_interface = "org.freedesktop.NetworkManager.AccessPoint";
        stand_in
            .update(&ap, ap_interface, &[("LastSeen", Value::from(42i32))])
            .await?;
        let quiet = future::or(async { changes.next().await.is_none() }, async {
            Timer::after(Duration::from_millis(500)).await;
            true
        })
        .await;
        assert!(quiet, "a change of LastSeen was reported");
        stand_in
            .update(&ap, ap_interface, &[("Strength", Value::from(12u8))])
            .await?;
        changes.next().await.ok_or("the stream ended")?;
        let snapshot = nm.snapshot().await?;
        assert!(snapshot.access_points.iter().any(|a| a.strength == 12));
        drop(wlan);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_agent_asks_for_a_new_password() -> Result<()> {
    run(async {
        let (stand_in, nm, wlan) = with_wifi().await?;
        let saved = stand_in.add_wifi_connection(&wlan, "home", "Home").await?;
        nm.register_agent().await?;
        let registered = stand_in.calls(stand_in.agent_manager(), "Register").await?;
        assert_eq!(
            registered[0][0],
            Value::from("io.github.mowi12.SwayThings").try_into()?
        );
        let agent = nm.unique_name().ok_or("no unique name")?;

        stand_in
            .ask_secrets(&agent, &saved, ALLOW_INTERACTION | REQUEST_NEW)
            .await?;
        let prompt = next_prompt(&nm).await?;
        assert_eq!(prompt.network, "Home");
        assert_eq!(prompt.connection, saved);
        assert!(prompt.retry && !prompt.wpa3);
        prompt.answer("new password".to_owned());
        assert_eq!(
            stand_in.secrets_reply().await?,
            Ok("new password".to_owned())
        );

        // Canceled by NetworkManager: the page hears of it; the dropped prompt says so.
        stand_in
            .ask_secrets(&agent, &saved, ALLOW_INTERACTION)
            .await?;
        let prompt = next_prompt(&nm).await?;
        assert!(!prompt.retry);
        stand_in.cancel_secrets(&agent, &saved).await?;
        match nm.agent_events().recv().await? {
            AgentEvent::Cancel(connection) => assert_eq!(connection, saved),
            AgentEvent::Prompt(_) => return Err("a second prompt".into()),
        }
        drop(prompt);
        assert_eq!(
            stand_in.secrets_reply().await?,
            Err("org.freedesktop.NetworkManager.SecretAgent.UserCanceled".to_owned())
        );

        nm.unregister_agent().await?;
        assert_eq!(
            stand_in
                .calls(stand_in.agent_manager(), "Unregister")
                .await?
                .len(),
            1
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_agent_leaves_other_questions_to_others() -> Result<()> {
    run(async {
        let (stand_in, nm, wlan) = with_wifi().await?;
        let saved = stand_in.add_wifi_connection(&wlan, "home", "Home").await?;
        let agent = nm.unique_name().ok_or("no unique name")?;
        let no_secrets = Err("org.freedesktop.NetworkManager.SecretAgent.NoSecrets".to_owned());

        // Without leave to ask the user.
        stand_in.ask_secrets(&agent, &saved, 0).await?;
        assert_eq!(stand_in.secrets_reply().await?, no_secrets);
        // An 802.1X password.
        stand_in
            .ask_secrets_of(&agent, &saved, "802-1x", ALLOW_INTERACTION)
            .await?;
        assert_eq!(stand_in.secrets_reply().await?, no_secrets);
        assert!(nm.agent_events().is_empty());

        // Someone else than NetworkManager.
        let settings: HashMap<&str, HashMap<&str, Value<'_>>> = HashMap::new();
        let hints: Vec<&str> = Vec::new();
        let result = stand_in
            .connection()
            .call_method(
                nm.unique_name().as_deref(),
                stand_in.agent_path(),
                Some("org.freedesktop.NetworkManager.SecretAgent"),
                "GetSecrets",
                &(
                    settings,
                    ObjectPath::try_from(saved.as_str())?,
                    "802-11-wireless-security",
                    hints,
                    ALLOW_INTERACTION,
                ),
            )
            .await;
        match result {
            Err(zbus::Error::MethodError(name, _, _)) => assert_eq!(
                name.as_str(),
                "org.freedesktop.NetworkManager.SecretAgent.PermissionDenied"
            ),
            other => return Err(format!("a stranger got an answer: {other:?}").into()),
        }
        assert!(nm.agent_events().is_empty());
        Ok(())
    })
}

/// The settings of a saved Ethernet connection called `name`.
fn ethernet_settings(name: &str) -> HashMap<&'static str, HashMap<&'static str, Value<'static>>> {
    HashMap::from([(
        "connection",
        HashMap::from([
            ("id", Value::from(name.to_owned())),
            ("type", Value::from("802-3-ethernet")),
        ]),
    )])
}

/// The settings of a saved OpenVPN connection called `name`.
fn vpn_settings(name: &str) -> HashMap<&'static str, HashMap<&'static str, Value<'static>>> {
    HashMap::from([
        (
            "connection",
            HashMap::from([
                ("id", Value::from(name.to_owned())),
                ("type", Value::from("vpn")),
            ]),
        ),
        (
            "vpn",
            HashMap::from([(
                "service-type",
                Value::from("org.freedesktop.NetworkManager.openvpn"),
            )]),
        ),
    ])
}
