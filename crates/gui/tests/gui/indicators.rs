// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The indicators of a row, the resets and the accent.

use super::*;
/// The checks of the indicators of the rows and of the page reset.
pub(crate) fn check_indicators() -> Vec<(String, Outcome)> {
    vec![
        (
            "indicators: reset and this machine only".to_owned(),
            check_reset_and_machine(),
        ),
        (
            "indicators: conflicts with the user's config".to_owned(),
            check_conflicts(),
        ),
        (
            "indicators: a risky reset goes through the safety net".to_owned(),
            check_risky_reset(),
        ),
        (
            "indicators: a device's own value is reset".to_owned(),
            check_device_reset(),
        ),
        ("indicators: reset a page".to_owned(), check_page_reset()),
    ]
}

pub(crate) const TOUCHPAD_ACCEL: &str = "input.touchpad.pointer_accel";

/// The setting `key`.
pub(crate) fn setting(key: &str) -> Result<&'static sway_things_core::registry::Setting, String> {
    Registry::builtin()
        .get(key)
        .ok_or_else(|| format!("  no setting {key}"))
}

/// A stored value has a reset button and a "This machine only" toggle; the toggle moves it to
/// the host overlay, the reset removes it. A value that isn't stored has neither.
pub(crate) fn check_reset_and_machine() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let accel = row(&window, TOUCHPAD_ACCEL)?;
    let tap = row(&window, "input.touchpad.tap")?;
    ensure(
        tap.reset_button().is_none() && tap.machine_toggle().is_none(),
        "a value that isn't stored has indicators",
    )?;
    let machine = accel
        .machine_toggle()
        .ok_or_else(|| "  the stored speed has no machine toggle".to_owned())?;
    ensure(
        !machine.is_active(),
        "a shared value is for this machine only",
    )?;
    machine.set_active(true);
    wait_for_model(&fixture.model)?;
    ensure(
        fixture.model.stored_in(setting(TOUCHPAD_ACCEL)?, None) == Some(Layer::Host),
        "the value didn't move to the host overlay",
    )?;
    let reset = accel
        .reset_button()
        .ok_or_else(|| "  the stored speed has no reset button".to_owned())?;
    reset.emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        fixture
            .model
            .stored_in(setting(TOUCHPAD_ACCEL)?, None)
            .is_none(),
        "the reset value is still stored",
    )?;
    ensure(
        accel.reset_button().is_none() && accel.machine_toggle().is_none(),
        "the reset row still has its indicators",
    )?;
    window.destroy();
    Ok(())
}

/// A line of the user's config before the include line is overridden, one after it wins; both
/// show a badge and the ⋮ menu with the line. Letting the config win removes the stored value.
pub(crate) fn check_conflicts() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let before = format!(
        "input type:touchpad pointer_accel 0.1\n{}",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&before));
    let window = open(&fixture, Page::MouseTouchpad)?;
    let accel = row(&window, TOUCHPAD_ACCEL)?;
    ensure(
        accel.badges() == ["Overridden"],
        &format!("  a line before the include shows {:?}", accel.badges()),
    )?;
    let (title, place, line) = accel
        .conflict_details()
        .ok_or_else(|| "  no ⋮ menu for the overridden line".to_owned())?;
    ensure(
        title == "Overridden in your config"
            && place.ends_with("config:1")
            && line == "input type:touchpad pointer_accel 0.1",
        &format!("  the menu shows {title:?}, {place:?}, {line:?}"),
    )?;
    ensure(
        row(&window, "input.touchpad.tap")?
            .conflict_button()
            .is_none(),
        "a row without a conflict has a ⋮ menu",
    )?;

    let after = format!(
        "{}input type:touchpad pointer_accel 0.5\n",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&after));
    settle(&window)?;
    ensure(
        accel.badges() == ["Your config wins"],
        &format!("  a line after the include shows {:?}", accel.badges()),
    )?;
    accel.let_config_win();
    wait_for_model(&fixture.model)?;
    let view = fixture
        .model
        .view(setting(TOUCHPAD_ACCEL)?, None)
        .ok_or_else(|| "  no settings".to_owned())?;
    ensure(
        view.value == Value::Decimal(0.5) && matches!(view.source, Source::Config { .. }),
        &format!("  after letting the config win: {view:?}"),
    )?;
    ensure(
        accel.badges().is_empty() && accel.conflict_button().is_none(),
        "the row still shows the conflict",
    )?;
    window.destroy();
    Ok(())
}

/// Resetting a risky value tries it with the safety net, and removes it on Keep.
pub(crate) fn check_risky_reset() -> Outcome {
    let sway = laptop();
    let stored = "version = 1\n\n[input.pointer]\nevents = false\n";
    let fixture = live_fixture(&sway, stored)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let enabled = row(&window, POINTER_EVENTS)?;
    let reset = enabled
        .reset_button()
        .ok_or_else(|| "  the stored value has no reset button".to_owned())?;
    reset.emit_clicked();
    wait_for_model(&fixture.model)?;
    let banner = window.safety_banner().unwrap_or_default();
    ensure(
        banner.starts_with("Keep the change to Mouse › Enabled?"),
        &format!("  the banner says {banner:?}"),
    )?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command.ends_with("events enabled")),
        "the default wasn't tried",
    )?;
    window.keep_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        fixture
            .model
            .stored_in(setting(POINTER_EVENTS)?, None)
            .is_none(),
        "the kept reset left the value stored",
    )?;
    window.destroy();
    Ok(())
}

/// The reset of a device's own value goes back to the value of all mice.
pub(crate) fn check_device_reset() -> Outcome {
    let sway = laptop();
    let stored =
        format!("version = 1\n\n[device.\"{MOUSE_ID}\".input.pointer]\npointer_accel = 0.5\n");
    let fixture = live_fixture(&sway, &stored)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    selector(&window, InputType::Pointer)?.select_device(Some(MOUSE_ID));
    settle(&window)?;
    let accel = row(&window, "input.pointer.pointer_accel")?;
    let reset = accel
        .reset_button()
        .ok_or_else(|| "  the mouse's own value has no reset button".to_owned())?;
    ensure(
        reset.tooltip_text().as_deref() == Some("Use the value of all mice"),
        &format!("  the reset button says {:?}", reset.tooltip_text()),
    )?;
    reset.emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        accel.note().as_deref() == Some("Same as all mice"),
        &format!("  after the reset the row says {:?}", accel.note()),
    )?;
    window.destroy();
    Ok(())
}

/// A page reset removes every stored value of the page but those that can lock the user out.
pub(crate) fn check_page_reset() -> Outcome {
    let stored = "version = 1\n\n[input.pointer]\nevents = false\n\n\
                  [input.touchpad]\npointer_accel = 0.3\ntap = true\n";
    let fixture = fixture(SWAYFX, stored, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let (removed, kept) = fixture.model.page_reset(Page::MouseTouchpad);
    let kept: Vec<&str> = kept.iter().map(|setting| setting.key()).collect();
    ensure(
        removed == 2 && kept == [POINTER_EVENTS],
        &format!("  the page reset removes {removed} and keeps {kept:?}"),
    )?;
    let content = window
        .content()
        .and_downcast::<PageView>()
        .ok_or_else(|| "  the page isn't a page view".to_owned())?;
    let button = content
        .reset_button()
        .ok_or_else(|| "  no Reset page button".to_owned())?;
    ensure(button.is_sensitive(), "Reset page can't be pressed")?;
    fixture.model.reset_page(Page::MouseTouchpad);
    wait_for_model(&fixture.model)?;
    ensure(
        fixture.model.page_reset(Page::MouseTouchpad).0 == 0,
        "values are left after the page reset",
    )?;
    ensure(
        fixture
            .model
            .stored_in(setting(POINTER_EVENTS)?, None)
            .is_some(),
        "the page reset removed a risky value",
    )?;
    ensure(
        !button.is_sensitive(),
        "Reset page can be pressed with only risky values left",
    )?;
    window.destroy();
    Ok(())
}

/// The Mouse & Touchpad page with stored values, both kinds of conflict and the machine toggle,
/// checked like the pages.
pub(crate) fn check_indicators_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let stored = STORED.replace("pointer_accel = 0.3\n", "pointer_accel = 0.3\ntap = true\n");
    let fixture = fixture(SWAYFX, &format!("{stored}{DIRECTIVES}"), USERS, true)?;
    let after = format!(
        "input type:touchpad tap disabled\n{}input type:touchpad pointer_accel 0.5\n",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&after));
    let window = Window::with_model(&fixture.model);
    window.present();
    let directory = screenshots.map(|dir| dir.join("indicators"));
    let outcome = check_page(&window, Page::MouseTouchpad, directory.as_deref(), scheme);
    window.destroy();
    outcome
}

/// The accent is the stored focus color, follows a change of it, and gives way to the theme's own
/// with the preference.
pub(crate) fn check_accent() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::About)?;
    let css = theme::accent_css();
    ensure(
        css.contains("@define-color accent_bg_color #3b6e4f;"),
        &format!("  the accent isn't the stored focus color:\n{css}"),
    )?;
    fixture.model.set(
        "windows.focused.child_border",
        None,
        Value::Color(sway_things_core::command::Color::rgb(0xe0, 0x1b, 0x24)),
    );
    wait_for_model(&fixture.model)?;
    ensure(
        theme::accent_css().contains("@define-color accent_bg_color #e01b24;"),
        "the accent didn't follow the focus color",
    )?;
    fixture
        .model
        .set("app.accent_source", None, Value::Choice("theme".to_owned()));
    wait_for_model(&fixture.model)?;
    ensure(
        theme::accent_css().is_empty(),
        "the accent stays with the theme's accent chosen",
    )?;
    window.destroy();
    Ok(())
}
