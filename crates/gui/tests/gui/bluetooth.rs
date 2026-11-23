// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Bluetooth page against BlueZ's stand-in.

use super::*;
/// The address of the headset of [`bluetooth_stand_in`], paired, with a battery.
pub(crate) const HEADSET: &str = "AA:BB:CC:DD:EE:FF";
/// The address of its speaker, not paired.
pub(crate) const SPEAKER: &str = "11:22:33:44:55:66";

/// A stand-in for BlueZ with two adapters, a paired headset and a speaker nearby.
pub(crate) fn bluetooth_stand_in() -> Result<StandIn, String> {
    async_io::block_on(async {
        let stand_in = StandIn::start().await?;
        stand_in.add_adapter("hci0", "laptop").await?;
        stand_in.add_adapter("hci1", "dongle").await?;
        let headset = stand_in.add_device("hci0", HEADSET, "Headset").await?;
        stand_in.pair_device("hci0", HEADSET).await?;
        stand_in
            .add_properties(
                &headset,
                "org.bluez.Battery1",
                &[("Percentage", zbus::zvariant::Value::from(80u8))],
            )
            .await?;
        stand_in.add_device("hci0", SPEAKER, "Speaker").await?;
        Ok::<_, sway_things_core::bluetooth::stand_in::Error>(stand_in)
    })
    .map_err(|error| format!("  BlueZ's stand-in: {error}"))
}

/// Points the Bluetooth page of `window` at `stand_in`, if there is one.
pub(crate) fn use_stand_in(window: &Window, stand_in: Option<&StandIn>) {
    if let Some(stand_in) = stand_in {
        window.use_bluetooth_bus(BluetoothBus::Address(stand_in.address().to_owned()));
    }
}

/// The Bluetooth page against the stand-in: the devices in their lists, discovery while it is
/// shown, connecting, pairing through the agent's dialog, and switching the radio off.
pub(crate) fn check_bluetooth() -> Outcome {
    let stand_in = bluetooth_stand_in()?;
    let page = BluetoothPage::new(&BluetoothBus::Address(stand_in.address().to_owned()));
    let window = gtk::Window::new();
    window.set_default_size(720, 700);
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let shown = run_until(Duration::from_secs(10), || {
        page.device_rows(true).len() == 1 && page.device_rows(false).len() == 1
    });
    let rows = |mine| page.device_rows(mine);
    ensure(
        shown,
        &format!(
            "  the devices didn't show: {:?} / {:?}",
            rows(true),
            rows(false)
        ),
    )?;
    let row = |name: &str, status: &str, button: &str| {
        (name.to_owned(), status.to_owned(), button.to_owned())
    };
    ensure(
        rows(true)
            == [row(
                "Headset",
                "Not connected \u{b7} Battery 80 %",
                "Connect",
            )],
        &format!("  my devices: {:?}", rows(true)),
    )?;
    ensure(
        rows(false) == [row("Speaker", "Not paired", "Connect")],
        &format!("  other devices: {:?}", rows(false)),
    )?;
    ensure(
        page.adapter_choice().is_visible(),
        "  two adapters but no adapter choice",
    )?;
    let calls = |path: &str, method: &str| {
        async_io::block_on(stand_in.calls(path, method)).map_or(0, |calls| calls.len())
    };
    ensure(
        run_until(Duration::from_secs(5), || {
            calls("/org/bluez/hci0", "StartDiscovery") > 0
        }),
        "  the page didn't look for devices while shown",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || page.is_searching()),
        "  no turning spinner while looking for devices",
    )?;
    ensure(
        !page.search_button().is_visible(),
        "  a Search button while the page searches by itself",
    )?;

    check_bluetooth_cancel(&page, &stand_in)?;

    page.device_button("Headset")
        .ok_or("  no button for the headset")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || {
            rows(true).first().is_some_and(|(_, status, button)| {
                status.starts_with("Connected") && button == "Disconnect"
            })
        }),
        &format!("  the headset didn't connect: {:?}", rows(true)),
    )?;

    check_bluetooth_pairing(&page, &stand_in)?;
    check_bluetooth_adapter(&page, &stand_in)?;

    page.power_switch().set_active(false);
    ensure(
        run_until(Duration::from_secs(5), || !page.power_switch().state()),
        "  Bluetooth didn't turn off",
    )?;
    ensure(
        !page.visible_switch().is_sensitive() && !page.adapter_rename_button().is_sensitive(),
        "  visibility and renaming stayed available with the radio off",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || {
            calls("/org/bluez/hci0", "StopDiscovery") > 0
        }),
        "  the page didn't stop looking for devices when the radio went off",
    )?;
    check_bluetooth_off(&page)?;
    window.destroy();
    Ok(())
}

/// The placeholder of `page` while its radio is off: it replaces the devices, and turning the
/// radio back on with the switch brings the devices back.
pub(crate) fn check_bluetooth_off(page: &BluetoothPage) -> Outcome {
    let texts = page
        .off_placeholder()
        .ok_or("  no placeholder while Bluetooth is off")?;
    ensure(
        texts.first().map(String::as_str) == Some("Bluetooth is off"),
        &format!("  placeholder: {texts:?}"),
    )?;
    ensure(
        !page
            .device_button("Headset")
            .is_some_and(|button| button.is_drawable()),
        "  the devices still show while Bluetooth is off",
    )?;
    page.power_switch().set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || {
            page.power_switch().state() && page.off_placeholder().is_none()
        }),
        "  turning Bluetooth on didn't remove the placeholder",
    )?;
    Ok(())
}

/// Renaming the selected adapter of [`bluetooth_stand_in`] on `page` through its dialog, and
/// making it visible; the switch follows BlueZ when the time is up.
pub(crate) fn check_bluetooth_adapter(page: &BluetoothPage, stand_in: &StandIn) -> Outcome {
    let bluez = async_io::block_on(stand_in.bluez()).map_err(|error| format!("  {error}"))?;
    let name = || {
        async_io::block_on(bluez.snapshot())
            .ok()
            .and_then(|snapshot| Some(snapshot.adapter("/org/bluez/hci0")?.name.clone()))
    };
    page.adapter_rename_button().emit_clicked();
    let dialog = || {
        gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::Window>().ok())
            .find(|window| {
                window.is_visible() && button_labeled(window.upcast_ref(), "Rename").is_some()
            })
    };
    ensure(
        run_until(Duration::from_secs(5), || dialog().is_some()),
        "  Rename… didn't ask for a name",
    )?;
    let dialog = dialog().ok_or("  no dialog")?;
    find::<gtk::Entry>(dialog.upcast_ref())
        .ok_or("  the dialog has no field")?
        .set_text("Work laptop");
    button_labeled(dialog.upcast_ref(), "Rename")
        .ok_or("  the dialog has no Rename button")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || {
            name().as_deref() == Some("Work laptop")
        }),
        &format!("  the adapter wasn't renamed: {:?}", name()),
    )?;

    ensure(
        page.visible_switch().is_sensitive(),
        "  the visibility switch isn't usable with the radio on",
    )?;
    // The stand-in makes the adapter visible when discovery starts: hide it first.
    let hide = || {
        async_io::block_on(stand_in.update(
            "/org/bluez/hci0",
            "org.bluez.Adapter1",
            &[("Discoverable", zbus::zvariant::Value::from(false))],
        ))
        .map_err(|error| format!("  {error}"))
    };
    hide()?;
    ensure(
        run_until(Duration::from_secs(5), || !page.visible_switch().state()),
        "  the switch didn't follow BlueZ hiding the adapter",
    )?;
    page.visible_switch().set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || page.visible_switch().state()),
        &format!("  the adapter didn't become visible: {}", page.error_text()),
    )?;
    ensure(
        async_io::block_on(bluez.snapshot())
            .ok()
            .and_then(|s| Some(s.adapter("/org/bluez/hci0")?.discoverable))
            == Some(true),
        "  BlueZ didn't make the adapter visible",
    )?;
    hide()?;
    ensure(
        run_until(Duration::from_secs(5), || !page.visible_switch().state()),
        "  the switch didn't follow BlueZ when the time was up",
    )?;
    Ok(())
}

/// The Bluetooth page as it searches by itself and as it searches only on request.
pub(crate) fn check_bluetooth_pages() -> Vec<(String, Outcome)> {
    vec![
        (
            "bluetooth: devices, pairing and power".to_owned(),
            check_bluetooth(),
        ),
        (
            "bluetooth: searching only with the Search button".to_owned(),
            check_bluetooth_manual(false),
        ),
        (
            "bluetooth: searching only with the Search button (dark)".to_owned(),
            check_bluetooth_manual(true),
        ),
    ]
}

/// The Bluetooth page with searches only on request: no discovery until _Search_, one after it,
/// and _Stop_ ends it.
pub(crate) fn check_bluetooth_manual(dark: bool) -> Outcome {
    let style = if dark { "dark" } else { "light" };
    let settings = gtk::Settings::default().ok_or("  no GTK settings")?;
    set_dark(&settings, dark);
    let stand_in = bluetooth_stand_in()?;
    let page = BluetoothPage::new(&BluetoothBus::Address(stand_in.address().to_owned()));
    page.set_manual_search(true);
    let window = gtk::Window::new();
    window.set_default_size(720, 700);
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let calls = |method: &str| {
        async_io::block_on(stand_in.calls("/org/bluez/hci0", method)).map_or(0, |calls| calls.len())
    };
    ensure(
        run_until(Duration::from_secs(10), || {
            page.device_rows(false).len() == 1
        }),
        "  the devices didn't show",
    )?;
    ensure(
        !run_until(Duration::from_secs(2), || calls("StartDiscovery") > 0),
        "  the page looked for devices without Search",
    )?;
    ensure(!page.is_searching(), "  a spinner without a search")?;
    ensure(page.search_button().is_visible(), "  no Search button")?;
    shoot(&window, &format!("bluetooth-manual-idle-{style}"))?;
    page.search_button().emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || calls("StartDiscovery") == 1),
        "  Search didn't look for devices",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || page.is_searching()),
        "  no turning spinner while searching",
    )?;
    ensure(
        page.search_text() == "Stop",
        "  the button doesn't read Stop while searching",
    )?;
    window.set_focus_visible(true);
    page.search_button().grab_focus();
    shoot(&window, &format!("bluetooth-manual-running-{style}"))?;
    page.search_button().emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || calls("StopDiscovery") > 0),
        "  Stop didn't stop looking for devices",
    )?;
    ensure(
        page.search_text() == "Search",
        "  the button doesn't read Search again",
    )?;
    // Switched to automatic, the button goes and the page looks for devices by itself.
    let started = calls("StartDiscovery");
    page.set_manual_search(false);
    ensure(
        !page.search_button().is_visible(),
        "  the Search button stayed in automatic mode",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || calls("StartDiscovery") > started),
        "  automatic mode didn't look for devices",
    )?;
    window.destroy();
    set_dark(&settings, false);
    Ok(())
}

/// Canceling on `page`, against [`bluetooth_stand_in`] with the headset not connected and the
/// speaker not paired: while a slow connection or pairing runs, the row's button reads "Cancel";
/// canceling sends `Disconnect` or `CancelPairing`, shows no error and puts the row back.
pub(crate) fn check_bluetooth_cancel(page: &BluetoothPage, stand_in: &StandIn) -> Outcome {
    let headset = format!("/org/bluez/hci0/dev_{}", HEADSET.replace(':', "_"));
    let speaker = format!("/org/bluez/hci0/dev_{}", SPEAKER.replace(':', "_"));
    let calls = |path: &str, method: &str| {
        async_io::block_on(stand_in.calls(path, method)).map_or(0, |calls| calls.len())
    };
    let row_of = |mine: bool, name: &str| {
        page.device_rows(mine)
            .into_iter()
            .find(|(row, _, _)| row == name)
    };
    for (name, path, mine, running, cancel, back) in [
        (
            "Headset",
            &headset,
            true,
            "Connecting…",
            "Disconnect",
            "Not connected",
        ),
        (
            "Speaker",
            &speaker,
            false,
            "Pairing…",
            "CancelPairing",
            "Not paired",
        ),
    ] {
        async_io::block_on(async {
            if mine {
                stand_in.connect_waits(path, 2).await
            } else {
                stand_in.pair_waits(path, 2).await
            }
        })
        .map_err(|error| format!("  {error}"))?;
        let button = page
            .device_button(name)
            .ok_or_else(|| format!("  no button for {name}"))?;
        button.emit_clicked();
        ensure(
            run_until(Duration::from_secs(2), || {
                row_of(mine, name).is_some_and(|(_, status, label)| {
                    status.starts_with(running) && label == "Cancel"
                })
            }),
            &format!(
                "  {name}: no Cancel while it runs: {:?}",
                row_of(mine, name)
            ),
        )?;
        ensure(
            button.is_sensitive(),
            &format!("  {name}: Cancel can't be pressed"),
        )?;
        // The row says "Cancel" before the call goes out (pairing registers the agent first):
        // give it time to reach the stand-in, well within the call's two seconds.
        run_until(Duration::from_millis(500), || false);
        button.emit_clicked();
        ensure(
            run_until(Duration::from_secs(10), || {
                row_of(mine, name).is_some_and(|(_, status, label)| {
                    status.starts_with(back) && label == "Connect"
                })
            }),
            &format!(
                "  {name}: the row didn't go back: {:?} / {:?}; error: {}",
                page.device_rows(true),
                page.device_rows(false),
                page.error_text()
            ),
        )?;
        ensure(
            calls(path, cancel) == 1,
            &format!("  {name}: {cancel} wasn't sent once"),
        )?;
        ensure(
            page.error_text().is_empty(),
            &format!("  {name}: an error after canceling: {}", page.error_text()),
        )?;
    }
    ensure(
        calls(&speaker, "Connect") == 0,
        "  the canceled pairing went on to connect",
    )?;
    async_io::block_on(stand_in.connect_waits(&headset, 0))
        .map_err(|error| format!("  {error}"))?;
    Ok(())
}

/// Pairing the speaker of [`bluetooth_stand_in`] on `page`: it asks to confirm a code, and
/// confirming pairs and connects it.
pub(crate) fn check_bluetooth_pairing(page: &BluetoothPage, stand_in: &StandIn) -> Outcome {
    let rows = |mine| page.device_rows(mine);
    let speaker = format!("/org/bluez/hci0/dev_{}", SPEAKER.replace(':', "_"));
    let name = page.bus_name().ok_or("  the page has no bus name")?;
    async_io::block_on(stand_in.pair_asks(
        &speaker,
        &name,
        "agent.RequestConfirmation(device, dbus.UInt32(12345))",
        "True",
    ))
    .map_err(|error| format!("  {error}"))?;
    page.device_button("Speaker")
        .ok_or("  no button for the speaker")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || page.pairing_dialog().is_some()),
        "  pairing didn't ask to confirm the code",
    )?;
    let dialog = page.pairing_dialog().ok_or("  no dialog")?;
    let code = find::<gtk::Label>(dialog.upcast_ref());
    ensure(code.is_some(), "  the dialog has no text")?;
    button_labeled(dialog.upcast_ref(), "Pair")
        .ok_or("  the dialog has no Pair button")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || {
            rows(true)
                .iter()
                .any(|(name, status, _)| name == "Speaker" && status.starts_with("Connected"))
        }),
        &format!(
            "  the speaker wasn't paired and connected: {:?} / {:?}; error: {}",
            rows(true),
            rows(false),
            page.error_text()
        ),
    )?;
    ensure(page.pairing_dialog().is_none(), "  the dialog stayed open")?;
    Ok(())
}
