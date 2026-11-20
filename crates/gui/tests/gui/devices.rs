// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The device selectors and devices that come and go.

use super::*;
/// The checks of the device selectors and the device list.
pub(crate) fn check_devices() -> Vec<(String, Outcome)> {
    vec![
        (
            "devices: a row for one device".to_owned(),
            check_device_rows(),
        ),
        (
            "devices: a type without devices shows a hint".to_owned(),
            check_no_devices(),
        ),
        ("devices: plugged in".to_owned(), check_plugged_in()),
        ("devices: unplugged".to_owned(), check_unplugged()),
        ("devices: the test area".to_owned(), check_test_area()),
    ]
}

/// The device selector of `kind` on the open page of `window`.
pub(crate) fn selector(window: &Window, kind: InputType) -> Result<DeviceSelector, String> {
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    all::<DeviceSelector>(&content)
        .into_iter()
        .find(|selector| selector.kind() == Some(kind))
        .ok_or_else(|| format!("  no device selector for {kind:?}"))
}

/// Whether the rows of `key` on the open page of `window` are shown, and whether the hint that
/// starts with `hint` is.
pub(crate) fn shown_or_hint(
    window: &Window,
    key: &str,
    hint: &str,
) -> Result<(bool, bool), String> {
    let row = row(window, key)?;
    let list = row
        .ancestor(gtk::ListBox::static_type())
        .ok_or_else(|| format!("  the row of {key} isn't in a list"))?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let hint = all::<gtk::Label>(&content)
        .into_iter()
        .find(|label| label.has_css_class("st-section-hint") && label.text().starts_with(hint))
        .ok_or_else(|| format!("  no hint {hint:?}"))?;
    Ok((list.is_visible(), hint.is_visible()))
}

/// The mouse selector lists the mice, also one that is only stored; picking one points the rows
/// at it, and a change is stored for it on this machine.
pub(crate) fn check_device_rows() -> Outcome {
    let sway = laptop();
    let stored = format!(
        "{STORED}\n[device.\"1133:16500:Logitech_G305\".input.pointer]\nnatural_scroll = true\n"
    );
    let fixture = live_fixture(&sway, &stored)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let mice = selector(&window, InputType::Pointer)?;
    let labels = mice.labels();
    ensure(
        labels == ["All mice", "MX Master 3", "Logitech_G305 (not connected)"],
        &format!("  the mouse selector offers {labels:?}"),
    )?;
    let touchpads = selector(&window, InputType::Touchpad)?.labels();
    ensure(
        touchpads == ["All touchpads", "Synaptics TM3276-022"],
        &format!("  the touchpad selector offers {touchpads:?}"),
    )?;

    let accel = row(&window, "input.pointer.pointer_accel")?;
    ensure(accel.note().is_none(), "a row for all mice has a note")?;
    mice.select_device(Some(MOUSE_ID));
    settle(&window)?;
    ensure(
        accel.device().as_deref() == Some(MOUSE_ID),
        "the row isn't for the mouse picked",
    )?;
    ensure(
        row(&window, "input.touchpad.tap")?.device().is_none(),
        "a touchpad row follows the mouse selector",
    )?;
    let note = accel.note().unwrap_or_default();
    ensure(
        note == "Same as all mice",
        &format!("  the row of one mouse says {note:?}"),
    )?;
    let spin = accel
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::SpinButton>().ok())
        .ok_or_else(|| "  the speed row has no number field".to_owned())?;
    spin.set_value(0.5);
    wait_for_model(&fixture.model)?;
    let expected = format!("input \"{MOUSE_ID}\" pointer_accel 0.5");
    ensure(
        sway.commands().last() == Some(&expected),
        &format!("  sent {:?}", sway.commands()),
    )?;
    let setting = Registry::builtin()
        .get("input.pointer.pointer_accel")
        .ok_or_else(|| "  no pointer speed".to_owned())?;
    let view = fixture
        .model
        .view(setting, Some(MOUSE_ID))
        .ok_or_else(|| "  no settings".to_owned())?;
    ensure(
        view.source
            == Source::Stored {
                layer: Layer::Host,
                for_type: false,
            },
        &format!("  the mouse's value comes from {:?}", view.source),
    )?;
    ensure(
        accel.note().is_none(),
        "the row still says it is the same as all mice",
    )?;
    mice.select_device(None);
    settle(&window)?;
    ensure(accel.device().is_none(), "the row isn't back to all mice")?;
    window.destroy();
    Ok(())
}

/// Without a touchpad and nothing stored for touchpads, the touchpad section is a hint; a value
/// stored for all touchpads brings it back.
pub(crate) fn check_no_devices() -> Outcome {
    let sway = obedient_with_inputs(SWAY_1_10, format!("[{KEYBOARD}, {MOUSE}]"));
    let fixture = live_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    ensure(
        shown_or_hint(&window, "input.touchpad.tap", "No touchpad")? == (false, true),
        "the touchpad section shows without a touchpad",
    )?;
    ensure(
        shown_or_hint(&window, "input.pointer.pointer_accel", "No mouse")? == (true, false),
        "the mouse section is hidden with a mouse",
    )?;
    window.destroy();

    let fixture = live_fixture(&sway, STORED)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    ensure(
        shown_or_hint(&window, "input.touchpad.tap", "No touchpad")? == (true, false),
        "the touchpad section is hidden with a value stored for touchpads",
    )?;
    window.destroy();
    Ok(())
}

/// A touchpad that is plugged in shows up in its selector and brings its section.
pub(crate) fn check_plugged_in() -> Outcome {
    let sway = with_inputs(&[
        r#"[{"identifier": "1133:16511:Logitech_MX_Master_3", "name": "MX Master 3",
             "type": "pointer"}]"#,
        r#"[{"identifier": "1133:16511:Logitech_MX_Master_3", "name": "MX Master 3",
             "type": "pointer"},
            {"identifier": "1739:52619:Synaptics_TM3276-022", "name": "Synaptics TM3276-022",
             "type": "touchpad"}]"#,
    ]);
    let fixture = live_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let touchpads = selector(&window, InputType::Touchpad)?;
    let plugged = run_until(Duration::from_secs(5), || touchpads.labels().len() == 2);
    ensure(
        plugged,
        &format!("  the touchpad selector offers {:?}", touchpads.labels()),
    )?;
    ensure(
        shown_or_hint(&window, "input.touchpad.tap", "No touchpad")? == (true, false),
        "the touchpad section didn't come with the touchpad",
    )?;
    window.destroy();
    Ok(())
}

/// A mouse that is unplugged leaves its selector; the device picked goes back to all mice.
pub(crate) fn check_unplugged() -> Outcome {
    let sway = with_inputs(&[
        r#"[{"identifier": "1133:16511:Logitech_MX_Master_3", "name": "MX Master 3",
             "type": "pointer"},
            {"identifier": "1739:52619:Synaptics_TM3276-022", "name": "Synaptics TM3276-022",
             "type": "touchpad"}]"#,
        r#"[{"identifier": "1739:52619:Synaptics_TM3276-022", "name": "Synaptics TM3276-022",
             "type": "touchpad"}]"#,
    ]);
    let fixture = fixture_in(SWAY, "version = 1\n", USERS, true, Some(&sway))?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let gone = run_until(Duration::from_secs(5), || {
        selector(&window, InputType::Pointer).is_ok_and(|mice| mice.labels() == ["All mice"])
    });
    ensure(gone, "the unplugged mouse stays in its selector")?;
    ensure(
        shown_or_hint(&window, "input.pointer.pointer_accel", "No mouse")? == (false, true),
        "the mouse section stays without a mouse",
    )?;
    ensure(
        row(&window, "input.pointer.pointer_accel")?
            .device()
            .is_none(),
        "the rows aren't for all mice",
    )?;
    window.destroy();
    Ok(())
}

/// The Mouse & Touchpad page ends with the test area: the pad names the click, the strip
/// scrolls.
pub(crate) fn check_test_area() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let area = find::<TestArea>(&content).ok_or_else(|| "  no test area".to_owned())?;
    area.show_click(3, 1);
    ensure(
        area.last_click() == "Last: Right click",
        &format!("  the pad says {:?}", area.last_click()),
    )?;
    let lines = area.strip().vadjustment();
    ensure(
        lines.upper() > lines.page_size(),
        "the strip has nothing to scroll",
    )?;
    window.destroy();
    Ok(())
}

/// The Mouse & Touchpad page without a touchpad, checked like the pages.
pub(crate) fn check_no_devices_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let sway = obedient_with_inputs(SWAY_1_10, format!("[{KEYBOARD}, {MOUSE}]"));
    let fixture = live_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let directory = screenshots.map(|dir| dir.join("no-touchpad"));
    let outcome = check_page(&window, Page::MouseTouchpad, directory.as_deref(), scheme);
    window.destroy();
    outcome
}
