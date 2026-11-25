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
use sway_things_core::network::stand_in::{self, FakeDialog, FakeSignIn, StandIn};
use sway_things_core::network::{
    ActiveState, AgentEvent, Connectivity, DeviceKind, DeviceState, Error as NetError,
    NetworkManager, SecretPrompt, Security, SignIn, SignInStep, Snapshot, VpnField, VpnPrompt,
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
        other => Err(format!("not a Wi-Fi prompt: {other:?}").into()),
    }
}

/// The next VPN prompt of the agent.
async fn next_vpn_prompt(nm: &NetworkManager) -> Result<VpnPrompt> {
    match nm.agent_events().recv().await? {
        AgentEvent::Vpn(prompt) => Ok(prompt),
        other => Err(format!("not a VPN prompt: {other:?}").into()),
    }
}

/// A stand-in with the VPN "Office" and an agent that finds the fake plugin with `dialog` in the
/// returned directory: the stand-in, our connection, the VPN's path, the agent's name.
async fn with_vpn(
    dialog: FakeDialog,
) -> Result<(StandIn, NetworkManager, String, String, tempfile::TempDir)> {
    let (stand_in, nm, _) = with_wifi().await?;
    let vpn = stand_in.add_connection(&vpn_settings("Office")).await?;
    let plugins = tempfile::tempdir()?;
    stand_in::fake_vpn_plugin(plugins.path(), dialog)?;
    nm.use_vpn_plugin_dirs(vec![plugins.path().to_owned()]);
    let agent = nm.unique_name().ok_or("no unique name")?;
    Ok((stand_in, nm, vpn, agent, plugins))
}

/// The next sign-in of the agent.
async fn next_sign_in(nm: &NetworkManager) -> Result<SignIn> {
    match nm.agent_events().recv().await? {
        AgentEvent::SignIn(sign_in) => Ok(sign_in),
        other => Err(format!("not a sign-in: {other:?}").into()),
    }
}

/// The message and field of the next step of `sign_in`, which must ask.
async fn next_ask(sign_in: &SignIn) -> Result<(String, VpnField)> {
    match sign_in.next().await {
        Some(SignInStep::Ask { message, field }) => Ok((message, field)),
        other => Err(format!("not a question: {other:?}").into()),
    }
}

/// A stand-in with the OpenConnect VPN "Uni" and a fake openconnect that signs in as `kind`
/// says, in the returned directory: the stand-in, our connection, the VPN's path, the agent's
/// name.
async fn with_openconnect(
    kind: FakeSignIn,
) -> Result<(StandIn, NetworkManager, String, String, tempfile::TempDir)> {
    let (stand_in, nm, _) = with_wifi().await?;
    let vpn = stand_in
        .add_connection(&openconnect_settings("Uni"))
        .await?;
    let dir = tempfile::tempdir()?;
    nm.use_openconnect(stand_in::fake_openconnect(dir.path(), kind)?);
    let agent = nm.unique_name().ok_or("no unique name")?;
    Ok((stand_in, nm, vpn, agent, dir))
}

/// The data of the `vpn` setting of the saved connection at `path`.
async fn connection_data(stand_in: &StandIn, path: &str) -> Result<HashMap<String, String>> {
    let settings: HashMap<String, HashMap<String, OwnedValue>> = stand_in
        .connection()
        .call_method(
            Some("org.freedesktop.NetworkManager"),
            path,
            Some("org.freedesktop.NetworkManager.Settings.Connection"),
            "GetSettings",
            &(),
        )
        .await?
        .body()
        .deserialize()?;
    let data = settings
        .get("vpn")
        .and_then(|vpn| vpn.get("data"))
        .ok_or("no vpn data")?
        .try_clone()?;
    Ok(data.try_into()?)
}

/// Makes the stand-in's `ActivateConnection` start the active connection that `FailVpn` (see
/// [`fail_vpn`]) ends for want of secrets; its path.
async fn failing_vpn(stand_in: &StandIn) -> Result<&'static str> {
    let active = "/org/freedesktop/NetworkManager/ActiveConnection/99";
    let interface = "org.freedesktop.NetworkManager.Connection.Active";
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
    // NetworkManager gives up for want of secrets (reason 9) and the active connection goes.
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
    Ok(active)
}

/// Ends the VPN of [`failing_vpn`] for want of secrets.
async fn fail_vpn(stand_in: &StandIn) -> Result<()> {
    stand_in
        .connection()
        .call_method(
            Some("org.freedesktop.NetworkManager"),
            MANAGER,
            Some("org.freedesktop.NetworkManager.Test"),
            "FailVpn",
            &(),
        )
        .await?;
    Ok(())
}

/// Waits until `ActivateConnection` was called: NetworkManager asks the agent for a VPN's
/// secrets only once the connection is activating, so a test asks then too, not before.
async fn activating(stand_in: &StandIn) -> Result<()> {
    for _ in 0..200 {
        if !stand_in
            .calls(MANAGER, "ActivateConnection")
            .await?
            .is_empty()
        {
            return Ok(());
        }
        Timer::after(Duration::from_millis(50)).await;
    }
    Err("the connection wasn't activated".into())
}

/// Makes the stand-in's `ActivateConnection` start an active connection that ends for want of
/// secrets (reason 9) at once, before the method returns.
async fn vpn_failing_at_once(stand_in: &StandIn) -> Result<()> {
    let active = "/org/freedesktop/NetworkManager/ActiveConnection/98";
    let interface = "org.freedesktop.NetworkManager.Connection.Active";
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
                 objects['{active}'].EmitSignal('{interface}', 'StateChanged', 'uu', \
                 [dbus.UInt32(4), dbus.UInt32(9)])\n\
                 self.RemoveObject('{active}')\n\
                 ret = dbus.ObjectPath('{active}')\n"
            ),
        )
        .await?;
    Ok(())
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
        let active = failing_vpn(&stand_in).await?;
        let path = nm.activate(&vpn, None).await?;
        assert_eq!(path, active);
        let (result, failed) = future::zip(nm.wait_connected(&path, None), async {
            Timer::after(Duration::from_millis(300)).await;
            fail_vpn(&stand_in).await
        })
        .await;
        failed?;
        assert_eq!(result, Err(NetError::Vpn(9)));

        // A VPN that ends before its activation returns still says why.
        vpn_failing_at_once(&stand_in).await?;
        assert_eq!(nm.connect_vpn(&vpn).await, Err(NetError::Vpn(9)));
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn a_vpn_whose_plugin_has_no_dialog_says_what_is_missing() -> Result<()> {
    run(async {
        let (stand_in, nm, vpn, agent, _plugins) = with_vpn(FakeDialog::Missing).await?;
        failing_vpn(&stand_in).await?;
        let (result, asked) = future::zip(nm.connect_vpn(&vpn), async {
            activating(&stand_in).await?;
            stand_in
                .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
                .await?;
            let reply = stand_in.secrets_reply().await?;
            fail_vpn(&stand_in).await?;
            Ok::<_, Box<dyn Error + Send + Sync>>(reply)
        })
        .await;
        assert_eq!(
            asked?,
            Err("org.freedesktop.NetworkManager.SecretAgent.NoSecrets".to_owned())
        );
        assert_eq!(result, Err(NetError::NoAuthDialog("openvpn".to_owned())));
        assert!(nm.agent_events().is_empty());
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn openconnect_signs_in_through_our_steps() -> Result<()> {
    run(async {
        let (stand_in, nm, vpn, agent, dir) = with_openconnect(FakeSignIn::Totp).await?;
        stand_in
            .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
            .await?;
        let sign_in = next_sign_in(&nm).await?;
        assert_eq!(
            (sign_in.connection.as_str(), sign_in.name.as_str()),
            (vpn.as_str(), "Uni")
        );
        let (message, field) = next_ask(&sign_in).await?;
        assert_eq!(
            (message.as_str(), field.label.as_str(), field.secret),
            ("", "Username", false)
        );
        sign_in.answer("alice".to_owned());
        let (_, field) = next_ask(&sign_in).await?;
        assert_eq!((field.label.as_str(), field.secret), ("Password", true));
        sign_in.answer("secret".to_owned());
        let (message, field) = next_ask(&sign_in).await?;
        assert_eq!(message, "MFA: Enter your TOTP as the second factor");
        assert_eq!((field.label.as_str(), field.secret), ("Response", true));
        // A line break would answer a next question too: it is taken out.
        sign_in.answer("123\n456".to_owned());
        assert_eq!(sign_in.next().await, None);
        assert_eq!(
            stand_in.secrets_reply().await?,
            Ok(
                "cookie=webvpn=fake;gateway=https://vpn.example.org/;gwcert=pin-sha256:fake="
                    .to_owned()
            )
        );
        let read = |name: &str| std::fs::read_to_string(dir.path().join(name));
        assert_eq!(read("answers.txt")?, "alice\nsecret\n123456\n");
        assert_eq!(
            read("openconnect-args.txt")?,
            "--authenticate\n--external-browser=xdg-open\n--protocol=anyconnect\n--\n\
             vpn.example.org\n"
        );

        // A choice.
        let (stand_in, nm, vpn, agent, dir) = with_openconnect(FakeSignIn::Group).await?;
        stand_in
            .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
            .await?;
        let sign_in = next_sign_in(&nm).await?;
        let (_, field) = next_ask(&sign_in).await?;
        assert_eq!(field.choices, ["Students", "Staff"]);
        sign_in.answer("Staff".to_owned());
        assert_eq!(sign_in.next().await, None);
        assert!(stand_in.secrets_reply().await?.is_ok());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("answers.txt"))?,
            "Staff\n"
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn a_failed_or_canceled_openconnect_sign_in_says_why() -> Result<()> {
    run(async {
        let canceled = Err("org.freedesktop.NetworkManager.SecretAgent.UserCanceled".to_owned());
        let no_secrets = Err("org.freedesktop.NetworkManager.SecretAgent.NoSecrets".to_owned());

        // The server refuses: the switch says what it said.
        let (stand_in, nm, vpn, agent, _dir) = with_openconnect(FakeSignIn::Fails).await?;
        failing_vpn(&stand_in).await?;
        let (result, signed) = future::zip(nm.connect_vpn(&vpn), async {
            activating(&stand_in).await?;
            stand_in
                .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
                .await?;
            let sign_in = next_sign_in(&nm).await?;
            next_ask(&sign_in).await?;
            sign_in.answer("alice".to_owned());
            next_ask(&sign_in).await?;
            sign_in.answer("wrong".to_owned());
            assert_eq!(sign_in.next().await, None);
            let reply = stand_in.secrets_reply().await?;
            fail_vpn(&stand_in).await?;
            Ok::<_, Box<dyn Error + Send + Sync>>(reply)
        })
        .await;
        assert_eq!(signed?, no_secrets);
        assert_eq!(
            result.map_err(|error| error.text().english()),
            Err("The sign-in to the VPN failed: Login failed.".to_owned())
        );

        // The profile, made without the flags of the sign-in's secrets, got them first, so
        // NetworkManager keeps what the agent gives.
        let data = connection_data(&stand_in, &vpn).await?;
        for key in ["cookie", "gateway", "gwcert", "resolve"] {
            assert_eq!(
                data.get(&format!("{key}-flags")).map(String::as_str),
                Some("2"),
                "{key}"
            );
        }
        assert_eq!(
            data.get("gateway").map(String::as_str),
            Some("vpn.example.org")
        );

        // A certificate that failed verification is refused, never accepted.
        let (stand_in, nm, vpn, agent, dir) = with_openconnect(FakeSignIn::Certificate).await?;
        stand_in
            .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
            .await?;
        let sign_in = next_sign_in(&nm).await?;
        assert_eq!(sign_in.next().await, None);
        assert_eq!(stand_in.secrets_reply().await?, no_secrets);
        assert!(!dir.path().join("answers.txt").exists());

        // The user cancels.
        let (stand_in, nm, vpn, agent, _dir) = with_openconnect(FakeSignIn::Totp).await?;
        stand_in
            .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
            .await?;
        let sign_in = next_sign_in(&nm).await?;
        next_ask(&sign_in).await?;
        drop(sign_in);
        assert_eq!(stand_in.secrets_reply().await?, canceled);

        // Without openconnect, no sign-in starts.
        let (stand_in, nm, vpn, agent, _dir) = with_openconnect(FakeSignIn::Totp).await?;
        nm.use_openconnect("/nonexistent/openconnect".into());
        stand_in
            .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
            .await?;
        assert_eq!(stand_in.secrets_reply().await?, no_secrets);
        assert!(nm.agent_events().is_empty());
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_agent_runs_the_vpn_plugins_own_dialog() -> Result<()> {
    run(async {
        let (stand_in, nm, vpn, agent, plugins) = with_vpn(FakeDialog::Window).await?;
        stand_in
            .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION | REQUEST_NEW)
            .await?;
        assert_eq!(
            stand_in.secrets_reply().await?,
            Ok("password=hunter2".to_owned())
        );
        let read = |name: &str| std::fs::read_to_string(plugins.path().join(name));
        assert_eq!(
            read("args.txt")?,
            "-u\n2f6a1d8e-5c3b-4f0a-9e7d-1b2c3d4e5f60\n-n\nOffice\n-s\n\
             org.freedesktop.NetworkManager.openvpn\n-i\n-r\n"
        );
        assert_eq!(
            read("input.txt")?,
            "DATA_KEY=remote\nDATA_VAL=vpn.example.org\n\nDONE\n"
        );
        // The dialog gets its QUIT once the secrets are read; it may still be writing it down.
        for _ in 0..50 {
            if read("quit.txt").is_ok_and(|quit| quit == "QUIT\n") {
                break;
            }
            Timer::after(Duration::from_millis(100)).await;
        }
        assert_eq!(read("quit.txt")?, "QUIT\n");
        // Nothing for the page to show.
        assert!(nm.agent_events().is_empty());
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_agent_asks_for_the_fields_of_an_external_ui_dialog() -> Result<()> {
    run(async {
        let (stand_in, nm, vpn, agent, plugins) = with_vpn(FakeDialog::ExternalUi).await?;
        stand_in
            .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
            .await?;
        let prompt = next_vpn_prompt(&nm).await?;
        assert_eq!(
            std::fs::read_to_string(plugins.path().join("args.txt"))?
                .lines()
                .nth(6),
            Some("--external-ui-mode")
        );
        assert_eq!(
            (prompt.connection.as_str(), prompt.name.as_str()),
            (vpn.as_str(), "Office")
        );
        assert_eq!(prompt.description, "Sign in to Office");
        assert!(!prompt.retry);
        assert_eq!(
            prompt.fields,
            [
                VpnField {
                    key: "username".to_owned(),
                    label: "Username".to_owned(),
                    value: "alice".to_owned(),
                    secret: false,
                    choices: Vec::new(),
                },
                VpnField {
                    key: "password".to_owned(),
                    label: "Password".to_owned(),
                    value: String::new(),
                    secret: true,
                    choices: Vec::new(),
                },
            ]
        );
        prompt.answer(vec!["bob".to_owned(), "123456".to_owned()]);
        assert_eq!(
            stand_in.secrets_reply().await?,
            Ok("cert-pass=known;password=123456;username=bob".to_owned())
        );

        // Canceled by NetworkManager: the page hears of it; the dropped prompt says so.
        stand_in
            .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
            .await?;
        let prompt = next_vpn_prompt(&nm).await?;
        stand_in.cancel_secrets(&agent, &vpn).await?;
        match nm.agent_events().recv().await? {
            AgentEvent::Cancel(connection) => assert_eq!(connection, vpn),
            other => return Err(format!("not a cancel: {other:?}").into()),
        }
        drop(prompt);
        assert_eq!(
            stand_in.secrets_reply().await?,
            Err("org.freedesktop.NetworkManager.SecretAgent.UserCanceled".to_owned())
        );

        // Without leave to ask, fields to fill in are no answer.
        stand_in.ask_secrets_of(&agent, &vpn, "vpn", 0).await?;
        assert_eq!(
            stand_in.secrets_reply().await?,
            Err("org.freedesktop.NetworkManager.SecretAgent.NoSecrets".to_owned())
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn a_canceled_or_stopped_vpn_dialog_cancels() -> Result<()> {
    run(async {
        let canceled = Err("org.freedesktop.NetworkManager.SecretAgent.UserCanceled".to_owned());
        let (stand_in, _nm, vpn, agent, _plugins) = with_vpn(FakeDialog::Canceled).await?;
        stand_in
            .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
            .await?;
        assert_eq!(stand_in.secrets_reply().await?, canceled);

        // NetworkManager stops waiting while the dialog is open: it is ended.
        let (stand_in, _nm, vpn, agent, plugins) = with_vpn(FakeDialog::Hangs).await?;
        stand_in
            .ask_secrets_of(&agent, &vpn, "vpn", ALLOW_INTERACTION)
            .await?;
        while !plugins.path().join("args.txt").exists() {
            Timer::after(Duration::from_millis(50)).await;
        }
        stand_in.cancel_secrets(&agent, &vpn).await?;
        assert_eq!(stand_in.secrets_reply().await?, canceled);
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
        let registered = stand_in
            .calls(stand_in.agent_manager(), "RegisterWithCapabilities")
            .await?;
        assert_eq!(
            registered[0],
            [
                Value::from("io.github.mowi12.SwayThings").try_into()?,
                // NM_SECRET_AGENT_CAPABILITY_VPN_HINTS
                Value::from(1_u32).try_into()?,
            ]
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
            other => return Err(format!("not a cancel: {other:?}").into()),
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
        // An 802.1X password, a VPN's secrets of a Wi-Fi network, WireGuard keys.
        let wireguard = stand_in
            .add_connection(&wireguard_settings("Tunnel"))
            .await?;
        for (connection, setting) in [
            (&saved, "802-1x"),
            (&saved, "vpn"),
            (&wireguard, "vpn"),
            (&wireguard, "wireguard"),
        ] {
            stand_in
                .ask_secrets_of(&agent, connection, setting, ALLOW_INTERACTION)
                .await?;
            assert_eq!(stand_in.secrets_reply().await?, no_secrets, "{setting}");
        }
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

/// The settings of a saved OpenConnect connection called `name` with Cisco's protocol.
fn openconnect_settings(
    name: &str,
) -> HashMap<&'static str, HashMap<&'static str, Value<'static>>> {
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
            HashMap::from([
                (
                    "service-type",
                    Value::from("org.freedesktop.NetworkManager.openconnect"),
                ),
                (
                    "data",
                    Value::from(HashMap::from([
                        ("gateway", "vpn.example.org"),
                        ("protocol", "anyconnect"),
                    ])),
                ),
            ]),
        ),
    ])
}

/// The settings of a saved WireGuard connection called `name`.
fn wireguard_settings(name: &str) -> HashMap<&'static str, HashMap<&'static str, Value<'static>>> {
    HashMap::from([(
        "connection",
        HashMap::from([
            ("id", Value::from(name.to_owned())),
            ("type", Value::from("wireguard")),
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
                ("uuid", Value::from("2f6a1d8e-5c3b-4f0a-9e7d-1b2c3d4e5f60")),
                ("type", Value::from("vpn")),
            ]),
        ),
        (
            "vpn",
            HashMap::from([
                ("service-type", Value::from(stand_in::OPENVPN)),
                (
                    "data",
                    Value::from(HashMap::from([("remote", "vpn.example.org")])),
                ),
            ]),
        ),
    ])
}
