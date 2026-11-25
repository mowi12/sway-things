// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Region & Time page against the stand-in for systemd-timedated.

use super::*;

pub(crate) fn check_region_pages() -> Vec<(String, Outcome)> {
    vec![
        (
            "region: the time zone, automatic time and polkit's refusal".to_owned(),
            check_region_time(),
        ),
        (
            "region: without timedated".to_owned(),
            check_region_without_timedated(),
        ),
    ]
}

/// The Date & Time section against the stand-in: the zone as city and region, automatic time
/// switched off reaching timedated and making the clock settable, and a refusal of polkit with
/// the command to copy.
pub(crate) fn check_region_time() -> Outcome {
    let stand_in = async_io::block_on(RegionStandIn::start())
        .map_err(|error| format!("  timedated's stand-in: {error}"))?;
    let page = RegionTimePage::new(&stand_in.bus());
    let window = gtk::Window::new();
    window.set_default_size(720, 700);
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let time = page.time_section().ok_or("  no Date & Time section")?;
    ensure(
        run_until(Duration::from_secs(10), || {
            time.zone_text().starts_with("Berlin, Europe · ")
        }),
        &format!("  the time zone: {:?}", time.zone_text()),
    )?;
    ensure(
        time.ntp_switch().is_active() && !time.time_button().is_sensitive(),
        "  automatic time isn't on, or the clock can be set while it is",
    )?;
    time.ntp_switch().set_active(false);
    ensure(
        run_until(Duration::from_secs(5), || {
            !stand_in.record().ntp && time.time_button().is_sensitive()
        }),
        "  turning automatic time off didn't reach timedated or free the clock",
    )?;
    stand_in.change(|record| record.deny = true);
    time.ntp_switch().set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || {
            time.help().command() == "timedatectl set-ntp true"
        }),
        &format!(
            "  polkit's refusal: {:?}, {:?}",
            time.help().text(),
            time.help().command()
        ),
    )?;
    ensure(
        run_until(Duration::from_secs(5), || !time.ntp_switch().is_active()),
        "  the switch didn't go back after the refusal",
    )?;
    async_io::block_on(stand_in.set_timezone_elsewhere("Asia/Tokyo"))
        .map_err(|error| format!("  {error}"))?;
    ensure(
        run_until(Duration::from_secs(5), || {
            time.zone_text().starts_with("Tokyo, Asia · ")
        }),
        &format!("  a change elsewhere: {:?}", time.zone_text()),
    )?;
    shoot(&window, "region-time")?;
    window.destroy();
    Ok(())
}

/// Without timedated, a placeholder takes the section's place.
pub(crate) fn check_region_without_timedated() -> Outcome {
    let page = RegionTimePage::new(&RegionBus::Off);
    let window = gtk::Window::new();
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let time = page.time_section().ok_or("  no Date & Time section")?;
    ensure(
        run_until(Duration::from_secs(5), || time.missing_shown()),
        "  no placeholder without timedated",
    )?;
    window.destroy();
    Ok(())
}
