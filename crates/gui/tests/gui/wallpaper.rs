// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Wallpaper page: the display selector, the pictures of the wallpaper folder and the notice
//! about wallpaper tools that the user's config starts.

use super::*;

pub(crate) fn check_wallpaper() -> Vec<(String, Outcome)> {
    vec![
        (
            "wallpaper: a picture for one display".to_owned(),
            check_wallpaper_picker(),
        ),
        (
            "wallpaper: a tool of the config is managed externally".to_owned(),
            check_wallpaper_notice(),
        ),
    ]
}

/// The screen of a laptop, without a serial, so it is stored by its name.
pub(crate) const LAPTOP_SCREEN: &str = r#"[{"name": "eDP-1", "make": "BOE", "model": "0x0BCA",
  "serial": "Unknown", "active": true, "primary": false,
  "modes": [{"width": 1920, "height": 1080, "refresh": 60000}],
  "current_mode": {"width": 1920, "height": 1080, "refresh": 60000},
  "rect": {"x": 0, "y": 0, "width": 1920, "height": 1080}}]"#;

/// The display selector lists the screen of the session; the pictures of the wallpaper folder
/// are offered, and picking one sets it for the display picked and selects it.
fn check_wallpaper_picker() -> Outcome {
    let pictures = tempfile::tempdir().map_err(|error| error.to_string())?;
    for name in ["a.png", "b.jpg", "notes.txt"] {
        std::fs::write(pictures.path().join(name), b"").map_err(|error| error.to_string())?;
    }
    let sway = with_outputs(&[LAPTOP_SCREEN]);
    let stored = format!(
        "{STORED}\n[app]\nwallpaper_folder = {:?}\n",
        pictures.path().display().to_string()
    );
    let fixture = displays_fixture(&sway, &stored)?;
    let window = open(&fixture, Page::Wallpaper)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let displays = all::<DeviceSelector>(&content)
        .into_iter()
        .find(|selector| selector.kind().is_none())
        .ok_or_else(|| "  no display selector".to_owned())?;
    let listed = run_until(Duration::from_secs(5), || displays.labels().len() == 2);
    let labels = displays.labels();
    ensure(
        listed && labels == ["All displays", "eDP-1 · BOE 0x0BCA"],
        &format!("  the display selector offers {labels:?}"),
    )?;
    let tiles = || all::<gtk::FlowBoxChild>(&content);
    let loaded = run_until(Duration::from_secs(5), || tiles().len() == 2);
    ensure(loaded, &format!("  {} pictures are offered", tiles().len()))?;

    displays.select_device(Some("eDP-1"));
    settle(&window)?;
    let second = tiles()
        .into_iter()
        .nth(1)
        .ok_or_else(|| "  no second picture".to_owned())?;
    second.emit_activate();
    wait_for_model(&fixture.model)?;
    let path = pictures.path().join("b.jpg");
    let expected = format!("output \"eDP-1\" bg \"'{}'\" fill #000000", path.display());
    ensure(
        sway.commands().last() == Some(&expected),
        &format!("  sent {:?}", sway.commands()),
    )?;
    let image = Registry::builtin()
        .get("wallpaper.image")
        .ok_or_else(|| "  no wallpaper image".to_owned())?;
    let view = fixture
        .model
        .view(image, Some("eDP-1"))
        .ok_or_else(|| "  no settings".to_owned())?;
    ensure(
        view.source
            == Source::Stored {
                layer: Layer::Host,
                for_type: false,
            },
        &format!("  the display's image comes from {:?}", view.source),
    )?;
    settle(&window)?;
    ensure(second.is_selected(), "the picture in use isn't selected")?;
    displays.select_device(None);
    settle(&window)?;
    ensure(
        !second.is_selected(),
        "the picture of one display is selected for all of them",
    )?;
    window.destroy();
    Ok(())
}

/// A wallpaper tool that the user's config starts is named with its line.
fn check_wallpaper_notice() -> Outcome {
    let users = format!("{USERS}exec swww-daemon\n");
    let fixture = fixture(SWAY, STORED, &users, true)?;
    let window = open(&fixture, Page::Wallpaper)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let notice = all::<gtk::Label>(&content)
        .into_iter()
        .find(|label| label.text().starts_with("Your config starts swww-daemon ("));
    ensure(
        notice.is_some_and(|label| label.is_drawable() && label.text().contains(":3)")),
        "  no notice about swww-daemon at line 3",
    )?;
    window.destroy();
    Ok(())
}
