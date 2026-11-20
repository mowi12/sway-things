// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The safety net: keep, revert, the countdown, the lockout and closing the window.

use super::*;
/// The checks of the safety net for risky changes.
pub(crate) fn check_safety_net() -> Vec<(String, Outcome)> {
    vec![
        ("safety net: keep".to_owned(), check_keep()),
        ("safety net: revert".to_owned(), check_revert()),
        (
            "safety net: the countdown reverts".to_owned(),
            check_countdown(),
        ),
        ("safety net: lockout protection".to_owned(), check_lockout()),
        (
            "safety net: closing the window reverts".to_owned(),
            check_close_reverts(),
        ),
    ]
}

pub(crate) const POINTER_EVENTS: &str = "input.pointer.events";

/// The switch of the row of `key` on the open page of `window`.
pub(crate) fn switch(window: &Window, key: &str) -> Result<(SettingRow, gtk::Switch), String> {
    let row = row(window, key)?;
    let switch = row
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Switch>().ok())
        .ok_or_else(|| format!("  the row of {key} has no switch"))?;
    Ok((row, switch))
}

/// A window on the Mouse & Touchpad page in the session of `sway`, after pointers were switched
/// off with the safety net: the banner is up.
pub(crate) fn tried(
    sway: &FakeSway,
    stored: &str,
) -> Result<(Fixture, Window, gtk::Switch), String> {
    let fixture = live_fixture(sway, stored)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let (row, switch) = switch(&window, POINTER_EVENTS)?;
    ensure(switch.is_active(), "pointers are off before the change")?;
    switch.set_active(false);
    wait_for_model(&fixture.model)?;
    ensure(row.error().is_none(), "the tried change shows an error")?;
    let banner = window.safety_banner().unwrap_or_default();
    ensure(
        banner.starts_with("Keep the change to Mouse › Enabled? Reverting in "),
        &format!("the banner says {banner:?}"),
    )?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command.ends_with("events disabled")),
        "the tried value wasn't sent",
    )?;
    Ok((fixture, window, switch))
}

/// The value of `key` the model shows, and whether it is stored.
pub(crate) fn shown(model: &Model, key: &str) -> Result<(Value, bool), String> {
    let setting = Registry::builtin()
        .get(key)
        .ok_or_else(|| format!("  no setting {key}"))?;
    let view = model
        .view(setting, None)
        .ok_or_else(|| "  no settings".to_owned())?;
    Ok((view.value, matches!(view.source, Source::Stored { .. })))
}

/// A risky change is tried, shown with the banner (Revert has the focus), and stored on Keep.
pub(crate) fn check_keep() -> Outcome {
    let sway = laptop();
    let (fixture, window, switch) = tried(&sway, STORED)?;
    ensure(
        GtkWindowExt::focus(&window).as_ref() == Some(window.revert_button().upcast_ref()),
        "Revert doesn't have the focus",
    )?;
    ensure(!switch.is_active(), "the row doesn't show the tried value")?;
    ensure(
        shown(&fixture.model, POINTER_EVENTS)? == (Value::Bool(false), false),
        "the tried value isn't shown or is already stored",
    )?;
    window.keep_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        window.safety_banner().is_none(),
        "the banner stays after Keep",
    )?;
    ensure(
        shown(&fixture.model, POINTER_EVENTS)? == (Value::Bool(false), true),
        "the kept value isn't stored",
    )?;
    ensure(!switch.is_active(), "the row lost the kept value")?;
    window.destroy();
    Ok(())
}

/// Revert puts the old value back, live and in the row, and stores nothing.
pub(crate) fn check_revert() -> Outcome {
    let sway = laptop();
    let (fixture, window, switch) = tried(&sway, STORED)?;
    window.revert_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        window.safety_banner().is_none(),
        "the banner stays after Revert",
    )?;
    ensure(switch.is_active(), "the row doesn't show the old value")?;
    ensure(
        shown(&fixture.model, POINTER_EVENTS)? == (Value::Bool(true), false),
        "the reverted value is stored",
    )?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command.ends_with("events enabled")),
        "the old value wasn't sent",
    )?;
    window.destroy();
    Ok(())
}

/// Without an answer, the change is reverted when the countdown ends.
pub(crate) fn check_countdown() -> Outcome {
    let sway = laptop();
    let stored = "version = 1\n\n[app]\nsafety_net_timeout = 5\n";
    let (fixture, window, switch) = tried(&sway, stored)?;
    let reverted = run_until(Duration::from_secs(8), || {
        window.safety_banner().is_none() && !fixture.model.is_busy()
    });
    ensure(reverted, "the change wasn't reverted within 8 s")?;
    ensure(switch.is_active(), "the row doesn't show the old value")?;
    ensure(
        shown(&fixture.model, POINTER_EVENTS)? == (Value::Bool(true), false),
        "the reverted value is stored",
    )?;
    window.destroy();
    Ok(())
}

/// Switching off the only pointer is refused at the row, and nothing is tried.
pub(crate) fn check_lockout() -> Outcome {
    let sway = obedient_with_inputs(SWAY_1_10, format!("[{KEYBOARD}, {MOUSE}]"));
    let fixture = live_fixture(&sway, STORED)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let (row, switch) = switch(&window, POINTER_EVENTS)?;
    switch.set_active(false);
    wait_for_model(&fixture.model)?;
    ensure(
        window.safety_banner().is_none(),
        "the refused change shows the banner",
    )?;
    let error = row.error().unwrap_or_default();
    ensure(
        error.contains("wasn't changed"),
        &format!("the row says {error:?}"),
    )?;
    ensure(switch.is_active(), "the row doesn't show the old value")?;
    ensure(
        sway.commands().is_empty(),
        "a command was sent for the refused change",
    )?;
    window.destroy();
    Ok(())
}

/// Closing the window while a change waits reverts it first.
pub(crate) fn check_close_reverts() -> Outcome {
    let sway = laptop();
    let (fixture, window, _) = tried(&sway, STORED)?;
    window.close();
    let closed = run_until(Duration::from_secs(5), || !window.is_visible());
    ensure(closed, "the window didn't close within 5 s")?;
    ensure(
        fixture.model.pending_change().is_none(),
        "the change still waits",
    )?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command.ends_with("events enabled")),
        "the old value wasn't sent before closing",
    )?;
    Ok(())
}

/// The page with the safety-net banner, checked like the pages.
pub(crate) fn check_safety_net_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let sway = laptop();
    let (_fixture, window, _) = tried(&sway, STORED)?;
    let directory = screenshots.map(|dir| dir.join("safety-net"));
    let outcome = check_page(&window, Page::MouseTouchpad, directory.as_deref(), scheme);
    window.destroy();
    outcome
}
