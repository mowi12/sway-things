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
        (
            "region: the language, its preview and localed's refusal".to_owned(),
            check_region_language(),
        ),
    ]
}

/// The Date & Time section against the stand-in: the zone as city and region, automatic time
/// switched off reaching timedated and making the clock settable, and a refusal of polkit with
/// the command to copy.
pub(crate) fn check_region_time() -> Outcome {
    let stand_in = async_io::block_on(RegionStandIn::start())
        .map_err(|error| format!("  timedated's stand-in: {error}"))?;
    let page = RegionTimePage::new(&stand_in.bus(), None);
    let window = gtk::Window::new();
    window.set_default_size(720, 700);
    window.set_child(Some(&scrolled(&page)));
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

/// The Language & Formats section against the stand-in for localed: the stored language in its
/// row, the formats following it, the preview of the C locale (in every container), and a refusal
/// of polkit for the system's locale with the command to copy.
pub(crate) fn check_region_language() -> Outcome {
    let stand_in = async_io::block_on(RegionStandIn::start())
        .map_err(|error| format!("  the stand-in: {error}"))?;
    let fixture = fixture(
        SWAY,
        "version = 1\n[region]\nlanguage = \"C.UTF-8\"\n",
        USERS,
        true,
    )?;
    let page = RegionTimePage::new(&stand_in.bus(), Some(&fixture.model));
    let window = gtk::Window::new();
    window.set_default_size(720, 900);
    window.set_child(Some(&scrolled(&page)));
    window.present();
    settle(&window)?;
    let section = page
        .language_section()
        .ok_or("  no Language & Formats section")?;
    let language = section.language_row().ok_or("  no language row")?;
    ensure(
        language.value()
            == Some(sway_things_core::registry::Value::Text(
                "C.UTF-8".to_owned(),
            )),
        &format!("  the language: {:?}", language.value()),
    )?;
    ensure(
        section.formats_row().is_some() && page.rows().len() >= 2,
        "  the rows of the language and formats aren't the page's",
    )?;
    ensure(
        run_until(Duration::from_secs(10), || {
            section
                .preview_texts()
                .first()
                .is_some_and(|date| !date.is_empty() && date != "—")
        }),
        &format!("  the preview: {:?}", section.preview_texts()),
    )?;
    ensure(
        run_until(Duration::from_secs(10), || {
            section.system_button().is_sensitive()
        }),
        "  System Default… never became usable with localed there",
    )?;
    stand_in.change(|record| record.deny = true);
    section.set_system_locale("C.UTF-8", None);
    ensure(
        run_until(Duration::from_secs(5), || {
            section.help().command() == "localectl set-locale LANG=C.UTF-8"
        }),
        &format!(
            "  polkit's refusal: {:?}, {:?}",
            section.help().text(),
            section.help().command()
        ),
    )?;
    ensure(
        stand_in.record().locale == ["LANG=en_US.UTF-8"],
        "  the refused change reached localed",
    )?;
    shoot(&window, "region-language")?;
    window.destroy();

    // Without localed, the button says why it can't be used.
    let page = RegionTimePage::new(&RegionBus::Off, None);
    let window = gtk::Window::new();
    window.set_child(Some(&scrolled(&page)));
    window.present();
    settle(&window)?;
    let section = page
        .language_section()
        .ok_or("  no Language & Formats section")?;
    ensure(
        run_until(Duration::from_secs(10), || {
            section
                .system_button()
                .tooltip_text()
                .is_some_and(|text| text.contains("systemd-localed"))
        }),
        "  without localed, System Default… doesn't say why",
    )?;
    ensure(
        !section.system_button().is_sensitive(),
        "  System Default… is usable without localed",
    )?;
    window.destroy();
    Ok(())
}

/// Without timedated, a placeholder takes the section's place.
pub(crate) fn check_region_without_timedated() -> Outcome {
    let page = RegionTimePage::new(&RegionBus::Off, None);
    let window = gtk::Window::new();
    window.set_child(Some(&scrolled(&page)));
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
