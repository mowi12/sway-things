// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Network page against NetworkManager's stand-in.

use super::*;
/// The NetworkManager manager object.
pub(crate) const NM_MANAGER: &str = "/org/freedesktop/NetworkManager";

/// A stand-in for NetworkManager with a Wi-Fi adapter that sees a saved network ("Home", WPA2),
/// a new one ("Office", WPA2), an open one ("Cafe") and an 802.1X one ("eduroam"), a saved
/// network that isn't around ("Attic"), a wired device with a saved connection and a VPN; the
/// path of the Wi-Fi device too.
pub(crate) fn network_stand_in() -> Result<(NmStandIn, String), String> {
    async_io::block_on(async {
        let stand_in = NmStandIn::start().await?;
        let wlan = stand_in.add_wifi_device("wlan0", "wlan0").await?;
        for (name, ssid, strength, security) in [
            ("home", "Home", 90, nm_stand_in::WPA2),
            ("office", "Office", 70, nm_stand_in::WPA2),
            ("cafe", "Cafe", 40, nm_stand_in::OPEN),
            ("eduroam", "eduroam", 60, 0x200),
        ] {
            stand_in
                .add_access_point(&wlan, name, ssid, "00:11:22:33:44:55", strength, security)
                .await?;
        }
        stand_in.add_wifi_connection(&wlan, "home", "Home").await?;
        let value = |text: &str| zbus::zvariant::Value::from(text.to_owned());
        let settings = |id: &str, kind: &str| {
            std::collections::HashMap::from([(
                "connection",
                std::collections::HashMap::from([("id", value(id)), ("type", value(kind))]),
            )])
        };
        let mut attic = settings("Attic", "802-11-wireless");
        attic.insert(
            "802-11-wireless",
            std::collections::HashMap::from([(
                "ssid",
                zbus::zvariant::Value::from(b"Attic".to_vec()),
            )]),
        );
        stand_in.add_connection(&attic).await?;
        let eth = stand_in
            .add_ethernet_device("eth0", "eth0", nm_stand_in::DISCONNECTED)
            .await?;
        let wired = stand_in
            .add_connection(&settings("Wired", "802-3-ethernet"))
            .await?;
        stand_in.make_available(&eth, &wired).await?;
        let mut vpn = settings("Office VPN", "vpn");
        vpn.insert(
            "vpn",
            std::collections::HashMap::from([(
                "service-type",
                value("org.freedesktop.NetworkManager.openvpn"),
            )]),
        );
        stand_in.add_connection(&vpn).await?;
        Ok::<_, nm_stand_in::Error>((stand_in, wlan))
    })
    .map_err(|error| format!("  NetworkManager's stand-in: {error}"))
}

/// The Network page against the stand-in: the networks in their lists, scans and the agent while
/// it is shown, connecting to a saved network and to a new one with its password, the agent's
/// dialog for a wrong password, the wired device and the VPN, the captive portal note, and
/// switching Wi-Fi off.
pub(crate) fn check_network() -> Outcome {
    let (stand_in, wlan) = network_stand_in()?;
    let page = NetworkPage::new(&NetworkBus::Address(stand_in.address().to_owned()));
    let window = gtk::Window::new();
    window.set_default_size(720, 900);
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let rows = |list: &str| page.rows(list);
    let row = |name: &str, status: &str, control: &str| {
        (name.to_owned(), status.to_owned(), control.to_owned())
    };
    ensure(
        run_until(Duration::from_secs(10), || {
            rows("known").len() == 2 && rows("others").len() == 3
        }),
        &format!(
            "  the networks didn't show: {:?} / {:?}",
            rows("known"),
            rows("others")
        ),
    )?;
    ensure(
        rows("known")
            == [
                row("Home", "Saved \u{b7} WPA2", "Connect"),
                row("Attic", "Not in range", ""),
            ],
        &format!("  known networks: {:?}", rows("known")),
    )?;
    let editor = sway_things_core::delegate::NM_CONNECTION_EDITOR
        .find()
        .is_some();
    let eduroam = if editor {
        row("eduroam", "Enterprise (802.1X)", "Set Up…")
    } else {
        row(
            "eduroam",
            "Enterprise (802.1X) \u{b7} needs nm-connection-editor",
            "",
        )
    };
    ensure(
        rows("others")
            == [
                eduroam,
                row("Office", "WPA2", "Connect"),
                row("Cafe", "Open, not encrypted", "Connect"),
            ],
        &format!("  other networks: {:?}", rows("others")),
    )?;
    ensure(
        rows("wired") == [row("Wired", "Off", "off")],
        &format!("  wired: {:?}", rows("wired")),
    )?;
    ensure(
        rows("vpns") == [row("Office VPN", "OpenVPN \u{b7} Off", "off")],
        &format!("  VPN: {:?}", rows("vpns")),
    )?;
    check_network_while_shown(&page, &stand_in, &wlan)?;

    page.row_button("Home")
        .ok_or("  no button for Home")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || {
            rows("known").first().is_some_and(|(_, status, button)| {
                status.starts_with("Connected") && button == "Disconnect"
            })
        }),
        &format!(
            "  Home didn't connect: {:?}; error: {}",
            rows("known"),
            page.error_text()
        ),
    )?;

    check_network_password(&page, &stand_in)?;
    check_network_agent(&page, &stand_in)?;

    check_network_wired_vpn(&page, &stand_in)?;

    page.wifi_switch().set_active(false);
    ensure(
        run_until(Duration::from_secs(5), || !page.wifi_switch().state()),
        "  Wi-Fi didn't turn off",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || !page.is_searching()),
        "  the page kept scanning with Wi-Fi off",
    )?;
    check_network_wifi_off(&page, &stand_in)?;
    window.destroy();
    Ok(())
}

/// The placeholder of `page` while Wi-Fi is off: it replaces the known and other networks, the
/// wired and VPN sections stay, a hardware block says so, and turning Wi-Fi back on with the
/// switch brings the networks back.
pub(crate) fn check_network_wifi_off(page: &NetworkPage, stand_in: &NmStandIn) -> Outcome {
    let texts = page
        .off_placeholder()
        .ok_or("  no placeholder while Wi-Fi is off")?;
    ensure(
        texts.first().map(String::as_str) == Some("Wi-Fi is off"),
        &format!("  placeholder: {texts:?}"),
    )?;
    ensure(
        !page
            .row_button("Home")
            .is_some_and(|button| button.is_drawable()),
        "  the known networks still show while Wi-Fi is off",
    )?;
    ensure(
        page.rows("wired").len() == 1 && page.rows("vpns").len() == 1,
        "  the wired and VPN sections went with Wi-Fi",
    )?;

    let hardware = |enabled: bool| {
        async_io::block_on(stand_in.update(
            NM_MANAGER,
            "org.freedesktop.NetworkManager",
            &[(
                "WirelessHardwareEnabled",
                zbus::zvariant::Value::from(enabled),
            )],
        ))
        .map_err(|error| format!("  {error}"))
    };
    hardware(false)?;
    ensure(
        run_until(Duration::from_secs(5), || {
            page.off_placeholder()
                .is_some_and(|texts| texts.iter().any(|text| text.contains("hardware switch")))
        }),
        &format!("  hardware block: {:?}", page.off_placeholder()),
    )?;
    hardware(true)?;
    ensure(
        run_until(Duration::from_secs(5), || page.wifi_switch().is_sensitive()),
        "  the switch stayed locked once the hardware block went",
    )?;
    page.wifi_switch().set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || {
            page.wifi_switch().state() && page.off_placeholder().is_none()
        }),
        "  turning Wi-Fi on didn't remove the placeholder",
    )?;
    Ok(())
}

/// The Network page as it scans by itself and as it scans only on request.
pub(crate) fn check_network_pages() -> Vec<(String, Outcome)> {
    vec![
        (
            "network: networks, passwords, wired, VPN and Wi-Fi".to_owned(),
            check_network(),
        ),
        (
            "network: scanning only with the Search button".to_owned(),
            check_network_manual(false),
        ),
        (
            "network: scanning only with the Search button (dark)".to_owned(),
            check_network_manual(true),
        ),
    ]
}

/// The Network page with scans only on request: no scan until _Search_, one after it, and the
/// search ends once NetworkManager has the new list.
pub(crate) fn check_network_manual(dark: bool) -> Outcome {
    let style = if dark { "dark" } else { "light" };
    let settings = gtk::Settings::default().ok_or("  no GTK settings")?;
    set_dark(&settings, dark);
    let (stand_in, wlan) = network_stand_in()?;
    let page = NetworkPage::new(&NetworkBus::Address(stand_in.address().to_owned()));
    page.set_manual_search(true);
    let window = gtk::Window::new();
    window.set_default_size(720, 900);
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let scans =
        || async_io::block_on(stand_in.calls(&wlan, "RequestScan")).map_or(0, |calls| calls.len());
    ensure(
        run_until(Duration::from_secs(10), || page.rows("others").len() == 3),
        "  the networks didn't show",
    )?;
    ensure(
        !run_until(Duration::from_secs(2), || scans() > 0),
        "  the page asked for a scan without Search",
    )?;
    ensure(!page.is_searching(), "  a spinner without a search")?;
    ensure(page.search_button().is_visible(), "  no Search button")?;
    shoot(&window, &format!("network-manual-idle-{style}"))?;
    page.search_button().emit_clicked();
    ensure(
        page.search_text() == "Stop" && page.is_searching(),
        "  no spinner and Stop while searching",
    )?;
    shoot(&window, &format!("network-manual-running-{style}"))?;
    ensure(
        run_until(Duration::from_secs(5), || scans() == 1),
        "  Search didn't ask for a scan",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || {
            page.search_text() == "Search" && !page.is_searching()
        }),
        "  the search didn't end with the new list",
    )?;
    ensure(scans() == 1, "  more than one scan for one Search")?;
    // Switched to automatic, the button goes and the page scans by itself.
    page.set_manual_search(false);
    ensure(
        !page.search_button().is_visible(),
        "  the Search button stayed in automatic mode",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || scans() > 1),
        "  automatic mode didn't scan",
    )?;
    window.destroy();
    set_dark(&settings, false);
    Ok(())
}

/// What `page` does while it is shown: it asks the Wi-Fi device `wlan` for scans, with the
/// spinner, and registers the agent.
pub(crate) fn check_network_while_shown(
    page: &NetworkPage,
    stand_in: &NmStandIn,
    wlan: &str,
) -> Outcome {
    let calls = |path: &str, method: &str| {
        async_io::block_on(stand_in.calls(path, method)).map_or(0, |calls| calls.len())
    };
    ensure(
        run_until(Duration::from_secs(5), || calls(wlan, "RequestScan") > 0),
        "  the page didn't ask for a scan while shown",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || page.is_searching()),
        "  no turning spinner while scanning",
    )?;
    ensure(
        !page.search_button().is_visible(),
        "  a Search button while the page scans by itself",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || {
            calls(stand_in.agent_manager(), "RegisterWithCapabilities") > 0
        }),
        "  the agent wasn't registered while the page is shown",
    )?;
    Ok(())
}

/// The wired device and the VPN on `page`, switched on, and the captive portal note.
pub(crate) fn check_network_wired_vpn(page: &NetworkPage, stand_in: &NmStandIn) -> Outcome {
    page.row_switch("Wired")
        .ok_or("  no switch for the wired device")?
        .set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || {
            page.rows("wired")
                .first()
                .is_some_and(|(_, status, on)| status.starts_with("Connected") && on == "on")
        }),
        &format!("  the cable didn't connect: {:?}", page.rows("wired")),
    )?;
    page.row_switch("Office VPN")
        .ok_or("  no switch for the VPN")?
        .set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || {
            page.rows("vpns")
                .first()
                .is_some_and(|(_, status, on)| status == "OpenVPN \u{b7} Connected" && on == "on")
        }),
        &format!("  the VPN didn't connect: {:?}", page.rows("vpns")),
    )?;

    // Unplugged: nothing to switch, the row only says so.
    async_io::block_on(stand_in.update(
        "/org/freedesktop/NetworkManager/Devices/eth0",
        "org.freedesktop.NetworkManager.Device",
        &[("State", zbus::zvariant::Value::from(20u32))],
    ))
    .map_err(|error| format!("  {error}"))?;
    ensure(
        run_until(Duration::from_secs(5), || {
            page.rows("wired")
                == [(
                    "Wired".to_owned(),
                    "Cable unplugged".to_owned(),
                    String::new(),
                )]
        }),
        &format!("  the unplugged cable: {:?}", page.rows("wired")),
    )?;

    ensure(
        page.portal_note() == (false, false),
        "  a portal note without a portal",
    )?;
    async_io::block_on(async {
        stand_in
            .add_properties(
                NM_MANAGER,
                "org.freedesktop.NetworkManager",
                &[(
                    "ConnectivityCheckUri",
                    zbus::zvariant::Value::from("http://check.invalid/"),
                )],
            )
            .await?;
        stand_in
            .update(
                NM_MANAGER,
                "org.freedesktop.NetworkManager",
                &[("Connectivity", zbus::zvariant::Value::from(2u32))],
            )
            .await
    })
    .map_err(|error| format!("  {error}"))?;
    ensure(
        run_until(Duration::from_secs(5), || {
            page.portal_note() == (true, true)
        }),
        "  no note and login button behind a captive portal",
    )?;
    Ok(())
}

/// Connecting to the new network "Office" on `page`: the password dialog takes no short
/// password, and the one given goes to NetworkManager.
pub(crate) fn check_network_password(page: &NetworkPage, stand_in: &NmStandIn) -> Outcome {
    page.row_button("Office")
        .ok_or("  no button for Office")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || page.open_dialog().is_some()),
        "  connecting to Office didn't ask for a password",
    )?;
    let dialog = page.open_dialog().ok_or("  no dialog")?;
    let entry = find::<gtk::PasswordEntry>(dialog.upcast_ref()).ok_or("  no password field")?;
    let connect =
        button_labeled(dialog.upcast_ref(), "Connect").ok_or("  the dialog has no Connect")?;
    entry.set_text("short");
    ensure(!connect.is_sensitive(), "  a short password was taken")?;
    entry.set_text("password1");
    ensure(connect.is_sensitive(), "  a good password wasn't taken")?;
    connect.emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || {
            page.rows("known")
                .iter()
                .any(|(name, status, _)| name == "Office" && status.starts_with("Connected"))
        }),
        &format!(
            "  Office didn't connect: {:?}; error: {}",
            page.rows("known"),
            page.error_text()
        ),
    )?;
    let calls = async_io::block_on(stand_in.calls(NM_MANAGER, "AddAndActivateConnection"))
        .map_err(|error| format!("  {error}"))?;
    let sent = format!("{calls:?}");
    ensure(
        sent.contains("password1"),
        &format!("  the password didn't reach NetworkManager: {sent}"),
    )?;
    ensure(page.open_dialog().is_none(), "  the dialog stayed open")?;
    Ok(())
}

/// The secret agent on `page`: NetworkManager asks again for the password of "Home", and the
/// dialog's answer goes back.
pub(crate) fn check_network_agent(page: &NetworkPage, stand_in: &NmStandIn) -> Outcome {
    let name = page.bus_name().ok_or("  the page has no bus name")?;
    let home = "/org/freedesktop/NetworkManager/Settings/home";
    // Allowed to ask, and the old password failed.
    async_io::block_on(stand_in.ask_secrets(&name, home, 0x1 | 0x2))
        .map_err(|error| format!("  {error}"))?;
    ensure(
        run_until(Duration::from_secs(5), || page.open_dialog().is_some()),
        "  the agent didn't ask for the password again",
    )?;
    let dialog = page.open_dialog().ok_or("  no dialog")?;
    ensure(
        dialog.title().as_deref() == Some("Wrong password for “Home”"),
        &format!("  the dialog is titled {:?}", dialog.title()),
    )?;
    find::<gtk::PasswordEntry>(dialog.upcast_ref())
        .ok_or("  no password field")?
        .set_text("new password");
    button_labeled(dialog.upcast_ref(), "Connect")
        .ok_or("  the dialog has no Connect")?
        .emit_clicked();
    let reply =
        async_io::block_on(stand_in.secrets_reply()).map_err(|error| format!("  {error}"))?;
    ensure(
        reply == Ok("new password".to_owned()),
        &format!("  the agent answered {reply:?}"),
    )?;
    ensure(page.open_dialog().is_none(), "  the dialog stayed open")?;
    Ok(())
}
