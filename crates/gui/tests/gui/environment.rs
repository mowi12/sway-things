// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Environment: the switches for older apps, the list of variables with the dialog that adds
//! and edits one and refuses what would break the login, and the check of the portals.

use super::*;

/// Stored: a switch on and a variable of the user's.
pub(crate) const STORED_ENVIRONMENT: &str = "version = 1\n\n[environment]\n\
    qt_wayland = true\n\
    variables = [\"EDITOR=vim\"]\n";

pub(crate) fn check_environment() -> Vec<(String, Outcome)> {
    vec![
        (
            "environment: the switches and the list of variables".to_owned(),
            check_environment_page(),
        ),
        (
            "environment: a variable added, refused and removed".to_owned(),
            check_environment_dialog(),
        ),
        (
            "environment: the portals row and its dialog".to_owned(),
            check_portals(),
        ),
    ]
}

/// The lines of the list of variables on the open page, with their texts.
fn variable_rows(window: &Window) -> Vec<(gtk::Box, String)> {
    window
        .content()
        .map(|content| all::<gtk::Box>(&content))
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.has_css_class("st-environment-row"))
        .map(|row| {
            let shown = all::<gtk::Label>(row.upcast_ref())
                .iter()
                .map(|label| label.text().to_string())
                .collect::<Vec<_>>()
                .join(" | ");
            (row, shown)
        })
        .collect()
}

/// The dialog of the Environment page open now.
fn environment_dialog() -> Option<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::Window>().ok())
        .filter(|window| window.has_css_class("st-environment-dialog") && window.is_visible())
        .last()
}

fn check_environment_page() -> Outcome {
    let fixture = fixture(SWAY, STORED_ENVIRONMENT, USERS, true)?;
    let window = open(&fixture, Page::Environment)?;
    for key in [
        "environment.qt_wayland",
        "environment.sdl_wayland",
        "environment.java_nonreparenting",
        "environment.electron_wayland",
    ] {
        row(&window, key)?;
    }
    ensure(
        row(&window, "environment.variables").is_err(),
        "  the variables have a row of their own",
    )?;
    let rows = variable_rows(&window);
    ensure(
        rows.iter()
            .any(|(_, shown)| shown.starts_with("EDITOR | vim")),
        &format!("  the list shows {rows:?}"),
    )?;
    let sdl = row(&window, "environment.sdl_wayland")?;
    let switch = sdl
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Switch>().ok())
        .ok_or_else(|| "  the SDL row has no switch".to_owned())?;
    switch.set_active(true);
    wait_for_model(&fixture.model)?;
    let state = fixture
        .model
        .state()
        .ok_or_else(|| "  no state".to_owned())?;
    let text = sway_things_core::environment::text(&sway_things_core::environment::variables(
        Registry::builtin(),
        &state,
    ))
    .unwrap_or_default();
    ensure(
        text.contains("SDL_VIDEODRIVER=wayland,x11\nEDITOR=vim\n"),
        &format!("  the file would be {text:?}"),
    )?;
    window.destroy();
    Ok(())
}

fn check_environment_dialog() -> Outcome {
    let fixture = fixture(SWAY, STORED_ENVIRONMENT, USERS, true)?;
    let window = open(&fixture, Page::Environment)?;
    let content = window
        .content()
        .ok_or_else(|| "  the window has no content".to_owned())?;
    let add = button_labeled(&content, "Add Variable…")
        .or_else(|| {
            all::<gtk::Button>(&content).into_iter().find(|button| {
                all::<gtk::Label>(button.upcast_ref())
                    .iter()
                    .any(|label| label.text() == "Add Variable…")
            })
        })
        .ok_or_else(|| "  no Add Variable…".to_owned())?;
    add.emit_clicked();
    ensure(
        run_until(Duration::from_secs(2), || environment_dialog().is_some()),
        "  no dialog",
    )?;
    let dialog = environment_dialog().ok_or_else(|| "  the dialog is gone".to_owned())?;
    let entries = all::<gtk::Entry>(dialog.upcast_ref());
    let [name, value] = entries.as_slice() else {
        return Err(format!("  the dialog has {} entries", entries.len()));
    };
    let save = button_labeled(dialog.upcast_ref(), "Save")
        .ok_or_else(|| "  the dialog has no Save".to_owned())?;
    // A variable of the session is refused, with the reason.
    name.set_text("HOME");
    value.set_text("/tmp");
    ensure(!save.is_sensitive(), "  HOME can be saved")?;
    let said: Vec<String> = all::<gtk::Label>(dialog.upcast_ref())
        .iter()
        .map(|label| label.text().to_string())
        .collect();
    ensure(
        said.iter()
            .any(|text| text.contains("the session sets it itself")),
        &format!("  the dialog says {said:?}"),
    )?;
    // PATH that keeps the system's programs is taken, with a warning.
    name.set_text("PATH");
    value.set_text("/opt/bin");
    ensure(
        !save.is_sensitive(),
        "  a PATH without /usr/bin can be saved",
    )?;
    value.set_text("$HOME/.local/bin:$PATH");
    ensure(save.is_sensitive(), "  PATH can't be saved")?;
    save.emit_clicked();
    wait_for_model(&fixture.model)?;
    let items = fixture.model.list("environment.variables");
    ensure(
        items == ["EDITOR=vim", "PATH=$HOME/.local/bin:$PATH"],
        &format!("  stored {items:?}"),
    )?;
    ensure(
        variable_rows(&window)
            .iter()
            .any(|(_, shown)| shown.starts_with("PATH | $HOME/.local/bin:$PATH")),
        "  the list doesn't show PATH",
    )?;
    window.destroy();
    Ok(())
}

fn check_portals() -> Outcome {
    let fixture = fixture(SWAY, STORED_ENVIRONMENT, USERS, true)?;
    let window = open(&fixture, Page::Environment)?;
    row(&window, "environment.pass_session_variables")?;
    ensure(
        row(&window, "environment.portal_config").is_err(),
        "  the portal configuration has a row of its own",
    )?;
    let content = window
        .content()
        .ok_or_else(|| "  the window has no content".to_owned())?;
    let details = all::<gtk::Button>(&content)
        .into_iter()
        .find(|button| button.has_css_class("st-portals-details"))
        .ok_or_else(|| "  no Details on the Portals row".to_owned())?;
    // The row says what the check found once it ran.
    ensure(
        run_until(Duration::from_secs(5), || {
            all::<gtk::Label>(&content)
                .iter()
                .all(|label| label.text() != "Checking…")
        }),
        "  the check doesn't end",
    )?;
    details.emit_clicked();
    ensure(
        run_until(Duration::from_secs(2), || environment_dialog().is_some()),
        "  no dialog",
    )?;
    let dialog = environment_dialog().ok_or_else(|| "  the dialog is gone".to_owned())?;
    let said: Vec<String> = all::<gtk::Label>(dialog.upcast_ref())
        .iter()
        .map(|label| label.text().to_string())
        .collect();
    for step in ["Installed", "Configured", "Session variables", "Running"] {
        ensure(
            said.iter().any(|text| text == step),
            &format!("  the dialog lacks {step}: {said:?}"),
        )?;
    }
    ensure(
        said.iter().any(|text| text.starts_with("ScreenCast: ")),
        &format!("  the dialog names no backend: {said:?}"),
    )?;
    dialog.close();
    window.destroy();
    Ok(())
}
