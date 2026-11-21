// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Sound page against the stand-in sound server.

use super::*;
/// A sound server with two outputs, a microphone, an app that plays and one that records.
pub(crate) fn sound_stand_in() -> Result<SoundStandIn, String> {
    let start = || {
        let mut stand_in = SoundStandIn::start()?;
        stand_in.add_output("speakers", "Speakers")?;
        stand_in.add_output("hdmi", "HDMI")?;
        stand_in.add_input("mic", "Microphone")?;
        stand_in.pactl(&["set-default-sink", "speakers"])?;
        stand_in.pactl(&["set-default-source", "mic"])?;
        stand_in.play("Music")?;
        stand_in.record("Recorder")?;
        Ok::<_, sway_things_core::sound::stand_in::Error>(stand_in)
    };
    start().map_err(|error| format!("  the sound server's stand-in: {error}"))
}

pub(crate) fn check_sound_pages() -> Vec<(String, Outcome)> {
    vec![
        ("sound: devices, volume and apps".to_owned(), check_sound()),
        (
            "sound: a server that goes away and comes back".to_owned(),
            check_sound_server_gone(),
        ),
    ]
}

/// The Sound page against the stand-in: the devices and apps, switching the output, its volume
/// and mute reaching the server, and sliders above 100 % only with the preference.
pub(crate) fn check_sound() -> Outcome {
    let stand_in = sound_stand_in()?;
    let page = SoundPage::new(&SoundServer::Address(stand_in.server()));
    let window = gtk::Window::new();
    window.set_default_size(720, 900);
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let items = |choice: &gtk::DropDown| choice.model().map_or(0, |model| model.n_items());
    ensure(
        run_until(Duration::from_secs(10), || {
            items(page.output_choice()) == 2 && page.app_rows().len() == 2
        }),
        &format!(
            "  the devices and apps didn't show: {} outputs, apps {:?}",
            items(page.output_choice()),
            page.app_rows()
        ),
    )?;
    let names: Vec<String> = page.app_rows().into_iter().map(|(name, _)| name).collect();
    ensure(
        names == ["Music", "Recorder"],
        &format!("  apps: {names:?}"),
    )?;
    ensure(
        page.app_rows()[1].1 == "Recording",
        &format!("  the recording app: {:?}", page.app_rows()),
    )?;
    let pactl = |args: &[&str]| stand_in.pactl(args).unwrap_or_default();
    // Sorted by name: HDMI first.
    page.output_choice().set_selected(0);
    ensure(
        run_until(Duration::from_secs(5), || {
            pactl(&["info"]).contains("Default Sink: hdmi")
        }),
        "  choosing HDMI didn't make it the default output",
    )?;
    let (scale, mute) = page.output_volume().ok_or("  no output volume")?;
    ensure(
        run_until(Duration::from_secs(5), || {
            page.output_name().as_deref() == Some("hdmi")
        }),
        "  the page didn't follow the new default output",
    )?;
    check_sound_device_volume(&page, &stand_in, &scale, &mute)?;
    let music = page.app_volume("Music").ok_or("  no volume for Music")?;
    music.set_value(0.25);
    ensure(
        run_until(Duration::from_secs(5), || {
            pactl(&["list", "sink-inputs"]).contains(" 25%")
        }),
        "  the app's volume didn't reach the server",
    )?;
    ensure(
        (scale.adjustment().upper() - 1.0).abs() < f64::EPSILON,
        "  the slider goes above 100 % without the preference",
    )?;
    page.set_above_100(true);
    ensure(
        (scale.adjustment().upper() - 1.5).abs() < f64::EPSILON,
        "  the slider doesn't go to 150 % with the preference",
    )?;
    ensure(
        page.error_text().is_empty(),
        &format!("  an error: {}", page.error_text()),
    )?;
    check_sound_unknown_default(&page, &stand_in)?;
    shoot(&window, "sound")?;
    window.destroy();
    Ok(())
}

/// The output's volume slider and mute reach the server, and the server's own changes move the
/// slider; where the server keeps virtual devices at 100 % (PipeWire 1.4), there is nothing to
/// check.
pub(crate) fn check_sound_device_volume(
    page: &SoundPage,
    stand_in: &SoundStandIn,
    scale: &gtk::Scale,
    mute: &gtk::ToggleButton,
) -> Outcome {
    if !stand_in.device_volume_works("hdmi") {
        println!("note: this PipeWire doesn't change the volume of virtual devices");
        return Ok(());
    }
    let pactl = |args: &[&str]| stand_in.pactl(args).unwrap_or_default();
    // The probe's own change has to be over before the slider is moved.
    run_until(Duration::from_secs(5), || {
        (scale.value() - 1.0).abs() < 0.01
    });
    scale.set_value(0.5);
    ensure(
        run_until(Duration::from_secs(5), || {
            pactl(&["get-sink-volume", "hdmi"]).contains(" 50%")
        }),
        &format!(
            "  the volume didn't reach the server: {}",
            pactl(&["get-sink-volume", "hdmi"])
        ),
    )?;
    mute.set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || {
            pactl(&["get-sink-mute", "hdmi"]).contains("yes")
        }),
        "  muting didn't reach the server",
    )?;
    // The server's changes from elsewhere show up, once the slider's own moment is over.
    run_until(Duration::from_secs(1), || false);
    let _ = stand_in.pactl(&["set-sink-volume", "hdmi", "80%"]);
    ensure(
        run_until(Duration::from_secs(5), || {
            (scale.value() - 0.8).abs() < 0.01
        }),
        &format!("  the slider didn't follow the server: {}", scale.value()),
    )?;
    // Unmuted again, for the screenshot.
    mute.set_active(false);
    ensure(
        run_until(Duration::from_secs(5), || {
            pactl(&["get-sink-mute", "hdmi"]).contains("no")
        }),
        "  unmuting didn't reach the server",
    )?;
    ensure(
        page.error_text().is_empty(),
        &format!("  an error: {}", page.error_text()),
    )
}

/// A default the page doesn't list (the monitor of an output): the drop-down says so, and the
/// only microphone can be chosen to make it the default again.
pub(crate) fn check_sound_unknown_default(page: &SoundPage, stand_in: &SoundStandIn) -> Outcome {
    let pactl = |args: &[&str]| stand_in.pactl(args).unwrap_or_default();
    let _ = stand_in.pactl(&["set-default-source", "hdmi.monitor"]);
    let items = |choice: &gtk::DropDown| choice.model().map_or(0, |model| model.n_items());
    ensure(
        run_until(Duration::from_secs(5), || {
            page.input_choice().selected() == 0
                && items(page.input_choice()) == 2
                && page.input_choice().is_sensitive()
        }),
        "  a monitor as the default input shows the microphone as chosen",
    )?;
    page.input_choice().set_selected(1);
    ensure(
        run_until(Duration::from_secs(5), || {
            pactl(&["get-default-source"]).trim() == "mic"
        }),
        "  choosing the microphone didn't make it the default input",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || items(page.input_choice()) == 1),
        "  the entry for a device not listed stayed",
    )
}

/// The Sound page while the server goes away and comes back: the placeholder, then the controls.
pub(crate) fn check_sound_server_gone() -> Outcome {
    let mut stand_in = sound_stand_in()?;
    let page = SoundPage::new(&SoundServer::Address(stand_in.server()));
    let window = gtk::Window::new();
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    ensure(
        run_until(Duration::from_secs(10), || page.app_rows().len() == 2),
        "  the page didn't show the server",
    )?;
    stand_in.stop_server();
    ensure(
        run_until(Duration::from_secs(10), || page.missing_shown()),
        "  no placeholder after the server went away",
    )?;
    stand_in
        .start_server()
        .map_err(|error| format!("  the server didn't start again: {error}"))?;
    ensure(
        run_until(Duration::from_secs(15), || !page.missing_shown()),
        "  the page didn't come back with the server",
    )?;
    window.destroy();
    let off = SoundPage::new(&SoundServer::Off);
    ensure(off.missing_shown(), "  no placeholder without a server")?;
    Ok(())
}
