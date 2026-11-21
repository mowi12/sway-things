// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opening the config in the user's editor.

use super::*;
/// The checks of the editor command preference.
pub(crate) fn check_editor() -> Vec<(String, Outcome)> {
    vec![
        (
            "editor: the conflict menu opens the file at the line".to_owned(),
            check_editor_conflict(),
        ),
        (
            "editor: the welcome screen opens the config".to_owned(),
            check_editor_welcome(),
        ),
        (
            "editor: a command that can't be split shows why".to_owned(),
            check_editor_command_error(),
        ),
        (
            "editor: a failed open shows why until one works".to_owned(),
            check_editor_failure_clears(),
        ),
    ]
}

/// Waits until `path` exists, as a program the app started creates it.
pub(crate) fn wait_for_file(path: &Path) -> Outcome {
    ensure(
        run_until(Duration::from_secs(10), || path.exists()),
        &format!("  {} wasn't created", path.display()),
    )
}

/// The directory of the user's sway config of `fixture`, which only exists in memory.
pub(crate) fn config_dir(fixture: &Fixture) -> Outcome {
    let dir = fixture.main.parent().unwrap_or(Path::new("/"));
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())
}

/// _Open config in editor_ in the ⋮ menu runs the editor command with the file and the line of
/// the user's config filled in, without a shell.
pub(crate) fn check_editor_conflict() -> Outcome {
    let stored = format!("{STORED}\n[app]\neditor_command = \"touch '{{file}}.at {{line}}'\"\n");
    let fixture = fixture(SWAYFX, &stored, USERS, true)?;
    config_dir(&fixture)?;
    let before = format!(
        "\ninput type:touchpad pointer_accel 0.1\n{}",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&before));
    let window = open(&fixture, Page::MouseTouchpad)?;
    let accel = row(&window, TOUCHPAD_ACCEL)?;
    ensure(accel.conflict_button().is_some(), "no ⋮ menu for the line")?;
    accel.open_config();
    let mut opened = fixture.main.clone().into_os_string();
    opened.push(".at 2");
    wait_for_file(Path::new(&opened))?;
    settle(&window)?;
    ensure(accel.error().is_none(), "opening the editor shows an error")?;
    window.destroy();
    Ok(())
}

/// _Open config in editor_ on the welcome screen runs the editor command with the config.
pub(crate) fn check_editor_welcome() -> Outcome {
    let stored = "version = 1\n\n[app]\neditor_command = \"touch {file}.opened\"\n";
    let fixture = fixture(SWAYFX, stored, USERS, false)?;
    config_dir(&fixture)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(window.is_welcome_shown(), "the welcome screen isn't shown")?;
    let [open, _, _] = window.welcome().buttons();
    open.emit_clicked();
    let mut opened = fixture.main.clone().into_os_string();
    opened.push(".opened");
    wait_for_file(Path::new(&opened))?;
    window.destroy();
    Ok(())
}

/// Waits until `error` gives what `expected` says: an error or none.
pub(crate) fn wait_for_error(
    error: impl Fn() -> Option<String>,
    expected: bool,
    what: &str,
) -> Outcome {
    ensure(
        run_until(Duration::from_secs(10), || error().is_some() == expected),
        what,
    )
}

/// An editor that ends with an error shows it, under the row and on the welcome screen; the
/// next open that works takes it away.
pub(crate) fn check_editor_failure_clears() -> Outcome {
    let stored = format!("{STORED}\n[app]\neditor_command = \"false\"\n");
    let fixture = fixture(SWAYFX, &stored, USERS, true)?;
    config_dir(&fixture)?;
    let before = format!(
        "\ninput type:touchpad pointer_accel 0.1\n{}",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&before));
    let window = open(&fixture, Page::MouseTouchpad)?;
    let accel = row(&window, TOUCHPAD_ACCEL)?;
    accel.open_config();
    wait_for_error(|| accel.error(), true, "the failed editor shows no error")?;
    let error = accel.error().unwrap_or_default();
    ensure(
        error.starts_with("false ended with"),
        &format!("  the row shows {error:?}"),
    )?;
    fixture
        .model
        .set("app.editor_command", None, Value::Text("true".to_owned()));
    wait_for_model(&fixture.model)?;
    accel.open_config();
    wait_for_error(
        || accel.error(),
        false,
        "the error stays after an open that works",
    )?;
    window.destroy();
    editor_failure_clears_welcome()
}

/// The welcome screen's part of [`check_editor_failure_clears`].
pub(crate) fn editor_failure_clears_welcome() -> Outcome {
    // Without the include line, the window opens on the welcome screen, and the preference
    // can't change: the command fails until the file it tests for is there.
    let stored = "version = 1\n\n[app]\neditor_command = \"test -e {file}.ok\"\n";
    let fixture = fixture(SWAYFX, stored, USERS, false)?;
    config_dir(&fixture)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(window.is_welcome_shown(), "the welcome screen isn't shown")?;
    let [open, _, _] = window.welcome().buttons();
    let welcome = window.welcome().clone();
    open.emit_clicked();
    wait_for_error(
        || welcome.error(),
        true,
        "the failed editor shows no error on the welcome screen",
    )?;
    let mut ok = fixture.main.clone().into_os_string();
    ok.push(".ok");
    write_file(Path::new(&ok), "")?;
    open.emit_clicked();
    wait_for_error(
        || welcome.error(),
        false,
        "the welcome screen keeps the error after an open that works",
    )?;
    window.destroy();
    Ok(())
}

/// An editor command with a quote that isn't closed isn't stored, and its row says why; one
/// that splits is stored. The entry hints at what an empty command means.
pub(crate) fn check_editor_command_error() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::About)?;
    let editor = row(&window, "app.editor_command")?;
    let entry = editor
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Entry>().ok())
        .ok_or_else(|| "  the editor row has no entry".to_owned())?;
    ensure(
        entry
            .placeholder_text()
            .is_some_and(|hint| hint.contains("$EDITOR")),
        "the empty entry has no hint",
    )?;
    entry.set_text("'/opt/My Editor/bin/edit");
    entry.emit_activate();
    wait_for_model(&fixture.model)?;
    let error = editor.error().unwrap_or_default();
    ensure(
        error.contains("quote that isn't closed"),
        &format!("  the row shows {error:?}"),
    )?;
    let setting = setting("app.editor_command")?;
    ensure(
        fixture.model.stored_in(setting, None).is_none(),
        "the command was stored",
    )?;
    entry.set_text("'/opt/My Editor/bin/edit' --line {line}");
    entry.emit_activate();
    wait_for_model(&fixture.model)?;
    ensure(editor.error().is_none(), "the good command shows an error")?;
    ensure(
        fixture.model.stored_in(setting, None).is_some(),
        "the good command wasn't stored",
    )?;
    window.destroy();
    Ok(())
}
