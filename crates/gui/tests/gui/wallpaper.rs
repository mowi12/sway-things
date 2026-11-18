// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Wallpaper page: the preview of the displays, the pictures of the wallpaper folder and the
//! notice about wallpaper tools that the user's config starts.

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

/// A wallpaper folder with two pictures, a file that isn't one, and a subfolder with a picture.
fn wallpaper_folder() -> Result<tempfile::TempDir, String> {
    let pictures = tempfile::tempdir().map_err(|error| error.to_string())?;
    for name in ["a.png", "b.jpg", "notes.txt"] {
        std::fs::write(pictures.path().join(name), b"").map_err(|error| error.to_string())?;
    }
    std::fs::create_dir(pictures.path().join("Holidays")).map_err(|error| error.to_string())?;
    std::fs::write(pictures.path().join("Holidays/c.webp"), b"")
        .map_err(|error| error.to_string())?;
    Ok(pictures)
}

/// The preview shows the screen of the session; switched to one display at a time, the tile of
/// the display is the one edited. The pictures of the wallpaper folder and its subfolders are
/// offered, and picking one sets it for that display and selects it.
fn check_wallpaper_picker() -> Outcome {
    let pictures = wallpaper_folder()?;
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
    let same = all::<gtk::Switch>(&content)
        .into_iter()
        .next()
        .ok_or_else(|| "  no switch for all displays".to_owned())?;
    ensure(
        same.is_active(),
        "  the displays start out edited one by one",
    )?;
    let tiles = || -> Vec<gtk::Button> {
        all::<gtk::Button>(&content)
            .into_iter()
            .filter(|button| button.has_css_class("st-wallpaper-tile"))
            .collect()
    };
    let shown = run_until(Duration::from_secs(5), || tiles().len() == 1);
    let names: Vec<String> = tiles()
        .iter()
        .filter_map(|tile| tile.tooltip_text().map(Into::into))
        .collect();
    ensure(
        shown && names == ["BOE 0x0BCA · eDP-1"],
        &format!("  the preview shows {names:?}"),
    )?;
    // The pictures: their tiles hold a box with the picture and its name.
    let pictures_shown = || -> Vec<gtk::FlowBoxChild> {
        all::<gtk::FlowBoxChild>(&content)
            .into_iter()
            .filter(|child| child.child().and_downcast::<gtk::Box>().is_some())
            .collect()
    };
    let loaded = run_until(Duration::from_secs(5), || pictures_shown().len() == 3);
    ensure(
        loaded,
        &format!("  {} pictures are offered", pictures_shown().len()),
    )?;
    let chips: Vec<String> = all::<gtk::ToggleButton>(&content)
        .into_iter()
        .filter(|chip| chip.has_css_class("st-wallpaper-chip"))
        .filter_map(|chip| chip.label().map(Into::into))
        .collect();
    ensure(
        chips == ["All", "Holidays"],
        &format!("  the chips are {chips:?}"),
    )?;

    same.set_active(false);
    settle(&window)?;
    ensure(
        tiles().iter().any(|tile| tile.has_css_class("st-editing")),
        "  the display isn't marked as edited",
    )?;
    let second = pictures_shown()
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
    let selected = pictures_shown().into_iter().nth(1);
    ensure(
        selected.is_some_and(|picture| picture.is_selected()),
        "the picture in use isn't selected",
    )?;
    let title = all::<gtk::Label>(&content)
        .into_iter()
        .any(|label| label.has_css_class("st-wallpaper-title") && label.text() == "b.jpg");
    ensure(title, "  the picture in use isn't named")?;
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
