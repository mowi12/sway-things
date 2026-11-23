// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The on-screen display of SwayOSD on Keyboard Shortcuts: its switch below the volume keys
//! shows while `swayosd-client` is installed (a stand-in in a directory of the search path) or
//! while it is stored, it turns the volume keys into `swayosd-client` bindings, and the note
//! below it says when `swayosd-server` doesn't run (a stand-in process of that name runs).

use std::os::unix::fs::PermissionsExt as _;

use super::*;

const VOLUME: &str = "shortcuts.media.volume";
const VOLUME_OSD: &str = "shortcuts.media.volume_osd";

pub(crate) fn check_swayosd() -> Vec<(String, Outcome)> {
    vec![
        (
            "swayosd: the switch shows while SwayOSD is installed or it is on".to_owned(),
            check_osd_switch(),
        ),
        (
            "swayosd: the note says when its server doesn't run, and how to start it".to_owned(),
            check_osd_server(),
        ),
    ]
}

/// A directory with an executable `name` in it, which does nothing.
fn stand_in(name: &str) -> Result<tempfile::TempDir, String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = dir.path().join(name);
    std::fs::write(&path, "#!/bin/sh\n").map_err(|error| error.to_string())?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    Ok(dir)
}

/// The search path of the model with `dir` first.
fn path_with(dir: &Path) -> std::ffi::OsString {
    let mut path = dir.as_os_str().to_owned();
    if let Some(rest) = std::env::var_os("PATH") {
        path.push(":");
        path.push(rest);
    }
    path
}

/// The text of the note below the switch of the display, while it shows.
fn server_note(window: &Window) -> Option<String> {
    let content = window.content()?;
    all::<gtk::Box>(&content)
        .into_iter()
        .find(|note| note.has_css_class("st-shortcut-osd") && note.is_visible())
        .and_then(|note| {
            all::<gtk::Label>(note.upcast_ref())
                .into_iter()
                .next()
                .map(|label| label.text().to_string())
        })
}

fn check_osd_switch() -> Outcome {
    let stored = "version = 1\n\n[shortcuts.media]\nvolume = true\n";
    // Not installed and not stored: no switch.
    let fixture = fixture(SWAY, stored, USERS, true)?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    if row(&window, VOLUME).is_err() {
        // Without wpctl the volume keys have no switch, and neither has their display.
        window.destroy();
        return Ok(());
    }
    ensure(
        row(&window, VOLUME_OSD).is_err() || sway_things_core::swayosd::installed(),
        "  the switch of the display shows without swayosd-client",
    )?;
    window.destroy();
    // Installed: the switch shows below the volume keys, and switches them to swayosd-client.
    let bin = stand_in("swayosd-client")?;
    let fixture = fixture_with_path(stored, USERS, bin.path())?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    let osd = row(&window, VOLUME_OSD)?;
    let switch = find::<gtk::Switch>(osd.upcast_ref())
        .ok_or_else(|| "  the display has no switch".to_owned())?;
    ensure(
        osd.is_sensitive() && server_note(&window).is_none(),
        "  the display's switch is off but grey or with a note",
    )?;
    ensure(
        list_item(&window, "Raise the volume").is_ok(),
        "  the volume keys don't raise the volume with wpctl",
    )?;
    switch.set_active(true);
    wait_for_model(&fixture.model)?;
    let _ = settle(&window);
    let raised = run_until(Duration::from_secs(2), || {
        list_item(&window, "Raise the volume, with an on-screen display").is_ok()
    });
    ensure(
        raised,
        "  the volume keys don't show the display once it is on",
    )?;
    let generated = fixture
        .model
        .bindings(None)
        .map(|listing| {
            listing
                .rows()
                .iter()
                .filter_map(|row| row.line.command.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    ensure(
        generated
            .iter()
            .any(|command| command == "exec \"swayosd-client --output-volume raise\""),
        &format!("  the bindings are {generated:?}"),
    )?;
    window.destroy();
    // Stored on, not installed: the switch stays, so it can be switched off.
    let fixture = super::common::fixture(
        SWAY,
        "version = 1\n\n[shortcuts.media]\nvolume = true\nvolume_osd = true\n",
        USERS,
        true,
    )?;
    fixture
        .model
        .set_search_path(Some(std::ffi::OsString::from("/nonexistent")));
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    ensure(
        row(&window, VOLUME_OSD).is_ok(),
        "  a display that is on but not installed can't be switched off",
    )?;
    window.destroy();
    Ok(())
}

/// A model with `stored` and the sway config `users`, looking for programs in `dir` first.
fn fixture_with_path(stored: &str, users: &str, dir: &Path) -> Result<Fixture, String> {
    let fixture = fixture(SWAY, stored, users, true)?;
    fixture.model.set_search_path(Some(path_with(dir)));
    Ok(fixture)
}

fn check_osd_server() -> Outcome {
    let stored = "version = 1\n\n[shortcuts.media]\nvolume = true\nvolume_osd = true\n";
    let bin = stand_in("swayosd-client")?;
    // Nothing starts the server: the note says how.
    let fixture = fixture_with_path(stored, USERS, bin.path())?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    if row(&window, VOLUME).is_err() {
        window.destroy();
        return Ok(());
    }
    let note = server_note(&window).unwrap_or_default();
    ensure(
        note.contains("isn't running") && note.contains("exec swayosd-server"),
        &format!("  without a server the note says {note:?}"),
    )?;
    window.destroy();
    // The config starts it: the note says where.
    let users = format!("{USERS}exec swayosd-server\n");
    let fixture = fixture_with_path(stored, &users, bin.path())?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    let note = server_note(&window).unwrap_or_default();
    ensure(
        note.contains("Your config starts it") && note.contains(":3"),
        &format!("  with an exec line the note says {note:?}"),
    )?;
    // It runs: no note.
    let server = tempfile::tempdir().map_err(|error| error.to_string())?;
    let program = server.path().join("swayosd-server");
    std::fs::copy("/usr/bin/sleep", &program).map_err(|error| error.to_string())?;
    let mut child = std::process::Command::new(&program)
        .arg("30")
        .spawn()
        .map_err(|error| error.to_string())?;
    let gone = run_until(Duration::from_secs(5), || {
        // Checked again with every change of the model.
        fixture.model.set_search_path(Some(path_with(bin.path())));
        server_note(&window).is_none()
    });
    let _ = child.kill();
    let _ = child.wait();
    ensure(gone, "  the note stays while the server runs")?;
    window.destroy();
    Ok(())
}
