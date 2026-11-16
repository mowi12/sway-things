// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The BlueZ backend against python-dbusmock's `bluez5` template on a private bus.
//!
//! These tests need `dbus-daemon` and python-dbusmock, so a plain `cargo test` skips them.
//! `scripts/check-integration.sh` runs them with `--ignored` in the containers, which have both.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::{StreamExt, future};
use sway_things_core::bluetooth::stand_in::StandIn;
use sway_things_core::bluetooth::{
    Adapter, AgentEvent, Answer, Bluez, Error as BtError, Request, Snapshot,
};
use zbus::zvariant::Value;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const PHONE: &str = "11:22:33:44:55:66";
const HEADSET: &str = "AA:BB:CC:DD:EE:FF";

/// Runs a test, which fails instead of hanging if the stand-in doesn't answer.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(30)).await;
        Err("the stand-in for BlueZ didn't answer within 30 seconds".into())
    }))
}

/// The stand-in with the adapter `hci0` and a phone found nearby; our connection to it.
async fn with_phone() -> Result<(StandIn, Bluez, String)> {
    let stand_in = StandIn::start().await?;
    stand_in.add_adapter("hci0", "laptop").await?;
    let phone = stand_in.add_device("hci0", PHONE, "Phone").await?;
    let bluez = stand_in.bluez().await?;
    Ok((stand_in, bluez, phone))
}

/// Waits until the snapshot satisfies `done`.
async fn until(bluez: &Bluez, done: impl Fn(&Snapshot) -> bool) -> Result<Snapshot> {
    for _ in 0..50 {
        let snapshot = bluez.snapshot().await?;
        if done(&snapshot) {
            return Ok(snapshot);
        }
        Timer::after(Duration::from_millis(100)).await;
    }
    Err(format!("the snapshot didn't change: {:?}", bluez.snapshot().await?).into())
}

/// The next agent event, answered with `answer` if it is a prompt.
async fn answer_next(bluez: &Bluez, answer: Answer) -> Result<Request> {
    match bluez.agent_events().recv().await? {
        AgentEvent::Prompt(prompt) => {
            let request = prompt.request.clone();
            prompt.answer(answer);
            Ok(request)
        }
        AgentEvent::Cancel => Err("the agent was canceled".into()),
    }
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_snapshot_has_the_adapters_and_their_devices() -> Result<()> {
    run(async {
        let stand_in = StandIn::start().await?;
        let bluez = stand_in.bluez().await?;
        assert_eq!(bluez.snapshot().await?, Snapshot::default());

        stand_in.add_adapter("hci1", "dongle").await?;
        stand_in.add_adapter("hci0", "laptop").await?;
        let phone = stand_in.add_device("hci0", PHONE, "Phone").await?;
        stand_in.add_device("hci1", HEADSET, "Headset").await?;
        stand_in.pair_device("hci1", HEADSET).await?;
        let snapshot = bluez.snapshot().await?;
        let adapters: Vec<&str> = snapshot.adapters.iter().map(Adapter::id).collect();
        assert_eq!(adapters, ["hci0", "hci1"]);
        let names: Vec<&str> = snapshot.devices.iter().map(|d| d.name.as_str()).collect();
        // Paired first.
        assert_eq!(names, ["Headset", "Phone"]);
        let device = snapshot.device(&phone).ok_or("no phone")?;
        assert_eq!(device.adapter, "/org/bluez/hci0");
        assert_eq!(device.address, PHONE);
        assert!(!device.paired && !device.connected);
        assert_eq!(snapshot.devices_of("/org/bluez/hci0").count(), 1);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn bluez_going_away_and_coming_back_is_reported() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start().await?;
        let bluez = stand_in.bluez().await?;
        let mut changes = bluez.changes().await?;
        stand_in.stop_bluez();
        changes.next().await.ok_or("the stream ended")?;
        assert!(matches!(
            bluez.snapshot().await,
            Err(BtError::Unavailable(_))
        ));
        stand_in.start_bluez().await?;
        changes.next().await.ok_or("the stream ended")?;
        assert_eq!(bluez.snapshot().await?, Snapshot::default());
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn power_and_discovery_are_switched() -> Result<()> {
    run(async {
        let (_stand_in, bluez, _) = with_phone().await?;
        let adapter = "/org/bluez/hci0";
        bluez.set_powered(adapter, false).await?;
        until(&bluez, |s| s.adapters.first().is_some_and(|a| !a.powered)).await?;
        bluez.set_powered(adapter, true).await?;
        until(&bluez, |s| s.adapters.first().is_some_and(|a| a.powered)).await?;
        bluez.start_discovery(adapter).await?;
        until(&bluez, |s| {
            s.adapters.first().is_some_and(|a| a.discovering)
        })
        .await?;
        bluez.stop_discovery(adapter).await?;
        until(&bluez, |s| {
            s.adapters.first().is_some_and(|a| !a.discovering)
        })
        .await?;
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_adapter_is_renamed_and_made_visible() -> Result<()> {
    run(async {
        let (_stand_in, bluez, _) = with_phone().await?;
        let adapter = "/org/bluez/hci0";
        let snapshot = bluez.snapshot().await?;
        let first = snapshot.adapter(adapter).ok_or("no adapter")?;
        assert!(!first.discoverable);
        assert_eq!(first.discoverable_timeout, 180);

        bluez.set_adapter_alias(adapter, " Work laptop ").await?;
        until(&bluez, |s| {
            s.adapter(adapter).is_some_and(|a| a.name == "Work laptop")
        })
        .await?;
        assert!(matches!(
            bluez.set_adapter_alias(adapter, "two\nlines").await,
            Err(BtError::InvalidName(_))
        ));

        bluez.set_discoverable(adapter, true).await?;
        until(&bluez, |s| {
            s.adapter(adapter).is_some_and(|a| a.discoverable)
        })
        .await?;
        bluez.set_discoverable(adapter, false).await?;
        until(&bluez, |s| {
            s.adapter(adapter).is_some_and(|a| !a.discoverable)
        })
        .await?;
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn a_device_is_paired_connected_renamed_and_forgotten() -> Result<()> {
    run(async {
        let (stand_in, bluez, phone) = with_phone().await?;
        bluez.pair(&phone).await?;
        let snapshot = until(&bluez, |s| s.device(&phone).is_some_and(|d| d.paired)).await?;
        assert!(snapshot.device(&phone).is_some_and(|d| d.trusted));
        // Our agent was registered, not as the default agent.
        let registered = stand_in.calls("/org/bluez", "RegisterAgent").await?;
        assert_eq!(registered.len(), 1);
        let default = stand_in.calls("/org/bluez", "RequestDefaultAgent").await?;
        assert_eq!(default.len(), 0, "{default:?}");
        // A second pairing is no error.
        bluez.pair(&phone).await?;

        bluez.connect_device(&phone).await?;
        until(&bluez, |s| s.device(&phone).is_some_and(|d| d.connected)).await?;
        bluez.connect_device(&phone).await?;
        bluez.disconnect_device(&phone).await?;
        until(&bluez, |s| s.device(&phone).is_some_and(|d| !d.connected)).await?;

        bluez.set_alias(&phone, " My phone ").await?;
        until(&bluez, |s| {
            s.device(&phone).is_some_and(|d| d.name == "My phone")
        })
        .await?;
        assert!(matches!(
            bluez.set_alias(&phone, "two\nlines").await,
            Err(BtError::InvalidName(_))
        ));

        bluez.remove("/org/bluez/hci0", &phone).await?;
        until(&bluez, |s| s.device(&phone).is_none()).await?;
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn changes_are_reported_but_noise_is_not() -> Result<()> {
    run(async {
        let (stand_in, bluez, phone) = with_phone().await?;
        let mut changes = bluez.changes().await?;
        stand_in
            .update(
                &phone,
                "org.bluez.Device1",
                &[("RSSI", Value::from(-40i16))],
            )
            .await?;
        let quiet = future::or(async { changes.next().await.is_some() }, async {
            Timer::after(Duration::from_millis(500)).await;
            false
        })
        .await;
        assert!(!quiet, "a change of the signal strength was reported");
        stand_in
            .update(
                &phone,
                "org.bluez.Device1",
                &[("Connected", Value::from(true))],
            )
            .await?;
        changes.next().await.ok_or("the stream ended")?;
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_battery_level_is_read() -> Result<()> {
    run(async {
        let (stand_in, bluez, phone) = with_phone().await?;
        stand_in
            .add_properties(
                &phone,
                "org.bluez.Battery1",
                &[("Percentage", Value::from(42u8))],
            )
            .await?;
        let snapshot = bluez.snapshot().await?;
        assert_eq!(snapshot.device(&phone).and_then(|d| d.battery), Some(42));
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_agent_confirms_a_passkey() -> Result<()> {
    run(async {
        let (stand_in, bluez, phone) = with_phone().await?;
        stand_in
            .pair_asks(
                &phone,
                &bluez.unique_name().ok_or("no unique name")?,
                "agent.RequestConfirmation(device, dbus.UInt32(12345))",
                "True",
            )
            .await?;
        let (paired, request) =
            future::zip(bluez.pair(&phone), answer_next(&bluez, Answer::Accept)).await;
        paired?;
        assert_eq!(request?, Request::Confirmation(12_345));
        until(&bluez, |s| s.device(&phone).is_some_and(|d| d.paired)).await?;
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn a_rejected_confirmation_fails_the_pairing() -> Result<()> {
    run(async {
        let (stand_in, bluez, phone) = with_phone().await?;
        stand_in
            .pair_asks(
                &phone,
                &bluez.unique_name().ok_or("no unique name")?,
                "agent.RequestConfirmation(device, dbus.UInt32(12345))",
                "True",
            )
            .await?;
        let (paired, _) =
            future::zip(bluez.pair(&phone), answer_next(&bluez, Answer::Reject)).await;
        match paired {
            Err(BtError::Failed(reason)) => assert!(reason.contains("Rejected"), "{reason}"),
            other => return Err(format!("pairing didn't fail: {other:?}").into()),
        }
        assert!(
            !bluez
                .snapshot()
                .await?
                .device(&phone)
                .is_some_and(|d| d.paired)
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_agent_hands_over_a_passkey_and_a_pin() -> Result<()> {
    run(async {
        let (stand_in, bluez, phone) = with_phone().await?;
        stand_in
            .pair_asks(
                &phone,
                &bluez.unique_name().ok_or("no unique name")?,
                "agent.RequestPasskey(device)",
                "answer == 4711",
            )
            .await?;
        let (paired, request) = future::zip(
            bluez.pair(&phone),
            answer_next(&bluez, Answer::Passkey(4711)),
        )
        .await;
        paired?;
        assert_eq!(request?, Request::Passkey);

        let headset = stand_in.add_device("hci0", HEADSET, "Headset").await?;
        stand_in
            .pair_asks(
                &headset,
                &bluez.unique_name().ok_or("no unique name")?,
                "agent.RequestPinCode(device)",
                "answer == '0000'",
            )
            .await?;
        // A PIN Bluetooth wouldn't take is rejected by the agent itself.
        let (paired, _) = future::zip(
            bluez.pair(&headset),
            answer_next(&bluez, Answer::PinCode("00 00".to_owned())),
        )
        .await;
        assert!(paired.is_err());
        let (paired, request) = future::zip(
            bluez.pair(&headset),
            answer_next(&bluez, Answer::PinCode("0000".to_owned())),
        )
        .await;
        paired?;
        assert_eq!(request?, Request::PinCode);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_agent_shows_a_passkey_and_hears_a_cancel() -> Result<()> {
    run(async {
        let (stand_in, bluez, phone) = with_phone().await?;
        stand_in
            .pair_asks(
                &phone,
                &bluez.unique_name().ok_or("no unique name")?,
                "[agent.DisplayPasskey(device, dbus.UInt32(42), dbus.UInt16(2)), agent.Cancel()]",
                "True",
            )
            .await?;
        bluez.pair(&phone).await?;
        let events = bluez.agent_events();
        match events.recv().await? {
            AgentEvent::Prompt(prompt) => {
                assert_eq!(prompt.device, phone);
                assert_eq!(
                    prompt.request,
                    Request::DisplayPasskey {
                        passkey: 42,
                        entered: 2
                    }
                );
            }
            AgentEvent::Cancel => return Err("no passkey shown".into()),
        }
        assert!(matches!(events.recv().await?, AgentEvent::Cancel));
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn the_agent_answers_only_bluez() -> Result<()> {
    run(async {
        let (stand_in, bluez, phone) = with_phone().await?;
        let unique = bluez.unique_name().ok_or("no unique name")?;
        let reply = stand_in
            .connection()
            .call_method(
                Some(unique.as_str()),
                "/io/github/mowi12/SwayThings/agent",
                Some("org.bluez.Agent1"),
                "RequestAuthorization",
                &(zbus::zvariant::ObjectPath::try_from(phone.as_str())?,),
            )
            .await;
        match reply {
            Err(zbus::Error::MethodError(name, _, _)) => {
                assert_eq!(name.as_str(), "org.bluez.Error.Rejected");
            }
            other => return Err(format!("a stranger's call was answered: {other:?}").into()),
        }
        assert!(bluez.agent_events().is_empty());
        Ok(())
    })
}
