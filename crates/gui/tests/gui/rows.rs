// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows built from the registry, their groups and their controls.

use super::*;
/// The checks of the rows built from the registry.
pub(crate) fn check_setting_rows() -> Vec<(String, Outcome)> {
    let mut results = vec![
        ("rows: one per setting".to_owned(), check_rows()),
        (
            "rows: a change reaches the settings".to_owned(),
            check_change(),
        ),
        (
            "rows: a locked setting can't change".to_owned(),
            check_locked(),
        ),
        ("rows: a failed change shows why".to_owned(), check_failed()),
        (
            "rows: a dragged slider snaps to the default, keys don't".to_owned(),
            check_slider_snap(),
        ),
        (
            "rows: a control keeps its place when the reset buttons appear".to_owned(),
            check_control_stays(),
        ),
        (
            "rows: a setting waits for the one it depends on".to_owned(),
            check_dependency(),
        ),
        (
            "rows: the directive line follows the preference".to_owned(),
            check_directives(),
        ),
        (
            "groups: one row with the values and the line".to_owned(),
            check_group_row(),
        ),
        (
            "groups: Enter and Space expand and collapse".to_owned(),
            check_group_keyboard(),
        ),
    ];
    results.extend(check_editor());
    results
}

/// The five colors of `client.focused` are rows below one group row: collapsed, it shows a
/// swatch per color and, while the preference is on, the line once; the members never show it.
pub(crate) fn check_group_row() -> Outcome {
    let fixture = fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true)?;
    let window = open(&fixture, Page::Windows)?;
    let group = group(&window, "windows.focused")?;
    let keys: Vec<&str> = group
        .members()
        .iter()
        .filter_map(|member| {
            member
                .setting()
                .map(sway_things_core::registry::Setting::key)
        })
        .collect();
    ensure(
        keys == [
            "windows.focused.border",
            "windows.focused.background",
            "windows.focused.text",
            "windows.focused.indicator",
            "windows.focused.child_border",
        ],
        &format!("  the group has the members {keys:?}"),
    )?;
    ensure(!group.is_expanded(), "the group starts expanded")?;
    ensure(
        group
            .members()
            .iter()
            .all(|member| member.parent().is_some_and(|item| !item.is_visible())),
        "a collapsed group shows its members",
    )?;
    let expected = fixture
        .model
        .view(setting("windows.focused.border")?, None)
        .and_then(|view| view.line)
        .ok_or_else(|| "  client.focused has no line".to_owned())?;
    ensure(
        group.line().as_deref() == Some(expected.as_str()),
        &format!(
            "  the group shows {:?}, expected {expected:?}",
            group.line()
        ),
    )?;
    ensure(
        group.members().iter().all(|member| member.line().is_none()),
        "a member repeats the line",
    )?;
    let swatches: Vec<String> = group
        .swatches()
        .iter()
        .map(gtk::gdk::RGBA::to_str)
        .map(|color| color.to_string())
        .collect();
    ensure(
        swatches.len() == 5 && swatches.last().map(String::as_str) == Some("rgb(59,110,79)"),
        &format!("  the swatches are {swatches:?}, the last one the stored #3b6e4f"),
    )?;

    // A change shows in the summary and the line.
    fixture.model.set(
        "windows.focused.border",
        None,
        Value::Color("#102030".parse().map_err(|_| "  not a color".to_owned())?),
    );
    wait_for_model(&fixture.model)?;
    ensure(
        group
            .swatches()
            .first()
            .map(|color| color.to_str().to_string())
            == Some("rgb(16,32,48)".to_owned()),
        "  the swatch didn't follow the change",
    )?;
    ensure(
        group
            .line()
            .is_some_and(|line| line.starts_with("client.focused #102030 ")),
        &format!("  the line didn't follow the change: {:?}", group.line()),
    )?;
    fixture
        .model
        .set("app.show_directives", None, Value::Bool(false));
    wait_for_model(&fixture.model)?;
    ensure(
        group.line().is_none(),
        "the group keeps its line after the preference went off",
    )?;
    window.destroy();
    Ok(())
}

/// The group row takes the focus, and `Enter` and `Space` expand and collapse it; its accessible
/// state follows.
pub(crate) fn check_group_keyboard() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::Windows)?;
    let group = group(&window, "windows.focused")?;
    let item = group.item();
    ensure(
        item.is_focusable() && item.grab_focus(),
        "the group row can't take the focus",
    )?;
    let keys = item
        .observe_controllers()
        .into_iter()
        .filter_map(Result::ok)
        .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
        .ok_or_else(|| "  the group row doesn't listen to keys".to_owned())?;
    let press = |key: gtk::gdk::Key| -> bool {
        use gtk::glib::translate::IntoGlib as _;
        keys.emit_by_name::<bool>(
            "key-pressed",
            &[&key.into_glib(), &0u32, &gtk::gdk::ModifierType::empty()],
        )
    };
    let members_shown = || {
        group
            .members()
            .iter()
            .all(|member| member.parent().is_some_and(|item| item.is_visible()))
    };
    ensure(
        press(gtk::gdk::Key::Return) && group.is_expanded() && members_shown(),
        "Enter doesn't expand the group",
    )?;
    ensure(
        press(gtk::gdk::Key::space) && !group.is_expanded() && !members_shown(),
        "Space doesn't collapse the group",
    )?;
    ensure(
        press(gtk::gdk::Key::KP_Enter) && group.is_expanded(),
        "Enter on the keypad doesn't expand the group",
    )?;
    ensure(
        !press(gtk::gdk::Key::a) && group.is_expanded(),
        "another key toggles the group",
    )?;
    window.destroy();
    Ok(())
}

/// Every page with settings shows a row for each of them, in the order of the registry.
pub(crate) fn check_rows() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    // The Wallpaper page is built of its own parts, not rows: `check_wallpaper` covers it.
    for page in Page::ALL
        .into_iter()
        .filter(|page| *page != Page::Wallpaper)
    {
        let window = open(&fixture, page)?;
        let expected: Vec<&str> = Registry::builtin()
            .iter()
            .filter(|setting| setting.page() == page)
            .map(sway_things_core::registry::Setting::key)
            // The list of bindings and the lists of autostart entries are the pages' own lists; a
            // switch of media keys shows while its program is installed, the switch of its
            // on-screen display while SwayOSD is.
            .filter(|key| {
                use sway_things_core::bindings::ours::{BINDINGS, Group};
                ![
                    BINDINGS,
                    sway_things_core::apps::autostart::ENTRIES,
                    sway_things_core::apps::autostart::HIDDEN,
                ]
                .contains(key)
                    && Group::of(key).is_none_or(Group::installed)
                    && Group::of_osd(key).is_none_or(|group| {
                        group.installed() && sway_things_core::swayosd::installed()
                    })
            })
            .collect();
        let content = window
            .content()
            .ok_or_else(|| "  no content pane".to_owned())?;
        let shown: Vec<&str> = rows(&content)
            .iter()
            .filter_map(|row| row.setting().map(sway_things_core::registry::Setting::key))
            .collect();
        ensure(
            shown == expected,
            &format!("{}: rows {shown:?}, expected {expected:?}", page.id()),
        )?;
        let unfocusable: Vec<String> = rows(&content)
            .iter()
            .flat_map(SettingRow::controls)
            .filter(|control| {
                !control.is_focusable()
                    && control
                        .first_child()
                        .is_none_or(|child| !child.is_focusable())
            })
            .map(|control| control.type_().name().to_owned())
            .collect();
        ensure(
            unfocusable.is_empty(),
            &format!("{}: controls without focus: {unfocusable:?}", page.id()),
        )?;
        window.destroy();
    }
    Ok(())
}

/// Pointer speed (default 0, slider -1 to 1, stored 0.3): a drag near 0 lands exactly on 0, one
/// outside the zone and a key step land where they are.
pub(crate) fn check_slider_snap() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let speed = row(&window, "input.touchpad.pointer_accel")?;
    let scale = speed
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Scale>().ok())
        .ok_or_else(|| "  the pointer speed has no slider".to_owned())?;
    let stored = |fixture: &Fixture| -> Result<Value, String> {
        let setting = Registry::builtin()
            .get("input.touchpad.pointer_accel")
            .ok_or_else(|| "  no pointer speed setting".to_owned())?;
        fixture
            .model
            .view(setting, None)
            .map(|view| view.value)
            .ok_or_else(|| "  no settings".to_owned())
    };
    // How GTK reports a pointer drag, and an arrow key.
    let mv = |scroll: gtk::ScrollType, value: f64| {
        scale.emit_by_name::<bool>("change-value", &[&scroll, &value]);
    };
    for (scroll, to, expected) in [
        (gtk::ScrollType::Jump, 0.03, 0.0),
        (gtk::ScrollType::Jump, -0.04, 0.0),
        (gtk::ScrollType::Jump, 0.05, 0.05),
        (gtk::ScrollType::StepLeft, 0.03, 0.03),
    ] {
        mv(scroll, to);
        wait_for_model(&fixture.model)?;
        ensure(
            (scale.value() - expected).abs() < 1e-9,
            &format!("  {scroll:?} to {to} left the slider at {}", scale.value()),
        )?;
        ensure(
            stored(&fixture)? == Value::Decimal(expected),
            &format!("  {scroll:?} to {to} stored {:?}", stored(&fixture)?),
        )?;
    }
    window.destroy();
    Ok(())
}

/// The slider of a setting without a stored value stays where it is once a change stores one
/// and the reset and machine buttons appear, so a drag doesn't jump.
pub(crate) fn check_control_stays() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let key = "input.touchpad.scroll_factor";
    let speed = row(&window, key)?;
    let scale = speed
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Scale>().ok())
        .ok_or_else(|| "  the scroll speed has no slider".to_owned())?;
    let bounds = |scale: &gtk::Scale| {
        scale
            .compute_bounds(&window)
            .map(|rect| (rect.x(), rect.width()))
    };
    ensure(
        speed.reset_button().is_none(),
        "scroll speed is stored already",
    )?;
    let before = bounds(&scale);
    fixture.model.set(key, None, Value::Decimal(1.5));
    wait_for_model(&fixture.model)?;
    settle(&window)?;
    ensure(
        speed.reset_button().is_some(),
        "the reset button didn't appear",
    )?;
    let after = bounds(&scale);
    ensure(
        before.is_some() && before == after,
        &format!("  the slider moved from {before:?} to {after:?}"),
    )?;
    window.destroy();
    Ok(())
}

/// Turning on the tap switch stores the value.
pub(crate) fn check_change() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let tap = row(&window, "input.touchpad.tap")?;
    let switch = tap
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Switch>().ok())
        .ok_or_else(|| "  the tap row has no switch".to_owned())?;
    ensure(!switch.is_active(), "tap is on before the change")?;
    switch.set_active(true);
    wait_for_model(&fixture.model)?;
    let setting = Registry::builtin()
        .get("input.touchpad.tap")
        .ok_or_else(|| "  no tap setting".to_owned())?;
    let view = fixture
        .model
        .view(setting, None)
        .ok_or_else(|| "  no settings".to_owned())?;
    ensure(
        view.value == Value::Bool(true),
        "the change didn't reach the settings",
    )?;
    ensure(
        view.line.as_deref() == Some("tap enabled"),
        "the row's line didn't follow",
    )?;
    ensure(tap.error().is_none(), "the change shows an error")?;
    window.destroy();
    Ok(())
}

/// The line of the sway config shows below the subtitle only while the preference is on, in
/// every row, and follows the preference at once.
pub(crate) fn check_directives() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let accel = row(&window, "input.touchpad.pointer_accel")?;
    let expected = fixture
        .model
        .view(setting("input.touchpad.pointer_accel")?, None)
        .and_then(|view| view.line)
        .ok_or_else(|| "  the pointer acceleration has no line".to_owned())?;
    ensure(
        accel.line().is_none(),
        "a row shows its line with the preference off",
    )?;
    fixture
        .model
        .set("app.show_directives", None, Value::Bool(true));
    wait_for_model(&fixture.model)?;
    ensure(
        accel.line().as_deref() == Some(expected.as_str()),
        &format!("  the row shows {:?}, expected {expected:?}", accel.line()),
    )?;
    window.show_page(Page::Windows);
    settle(&window)?;
    ensure(
        row(&window, "windows.gaps.inner")?.line().is_some(),
        "a row on another page doesn't show its line",
    )?;
    fixture
        .model
        .set("app.show_directives", None, Value::Bool(false));
    wait_for_model(&fixture.model)?;
    ensure(
        row(&window, "windows.gaps.inner")?.line().is_none(),
        "a row keeps its line after the preference went off",
    )?;
    window.destroy();
    Ok(())
}

/// On vanilla sway, the SwayFX corner radius can't be changed, and says why. A setting that
/// applies after a reload says so.
pub(crate) fn check_locked() -> Outcome {
    let fixture = fixture(SWAY, STORED, USERS, true)?;
    let window = open(&fixture, Page::Windows)?;
    let radius = row(&window, "effects.corner_radius")?;
    for control in radius.controls() {
        ensure(
            !control.is_sensitive(),
            "a control of the corner radius can be used",
        )?;
        ensure(
            control.tooltip_text().as_deref() == Some("Requires SwayFX 0.5"),
            "the corner radius doesn't say why it is locked",
        )?;
    }
    ensure(
        radius.badges() == ["Requires SwayFX 0.5"],
        &format!("  the corner radius has the badges {:?}", radius.badges()),
    )?;
    let gaps = row(&window, "windows.gaps.inner")?;
    ensure(
        gaps.controls()
            .iter()
            .all(gtk::prelude::WidgetExt::is_sensitive),
        "the gaps are locked",
    )?;
    window.destroy();
    let window = open(&fixture, Page::Keyboard)?;
    let numlock = row(&window, "input.keyboard.xkb_numlock")?;
    ensure(
        numlock.badges() == ["Applies after reload"],
        &format!(
            "  Num Lock at startup has the badges {:?}",
            numlock.badges()
        ),
    )?;
    window.destroy();
    Ok(())
}

/// Blur passes can't be changed while blur is off, and say so; turning blur on unlocks them.
pub(crate) fn check_dependency() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::Windows)?;
    let passes = row(&window, "effects.blur_passes")?;
    ensure(
        passes
            .controls()
            .iter()
            .all(|control| !control.is_sensitive()),
        "blur passes can be changed with blur off",
    )?;
    ensure(
        passes.note().as_deref() == Some("Turn on Blur first"),
        &format!("  blur passes say {:?}", passes.note()),
    )?;
    let (_, blur) = switch(&window, "effects.blur")?;
    blur.set_active(true);
    wait_for_model(&fixture.model)?;
    ensure(
        passes
            .controls()
            .iter()
            .all(gtk::prelude::WidgetExt::is_sensitive),
        "blur passes stay locked with blur on",
    )?;
    ensure(passes.note().is_none(), "blur passes still say why")?;
    window.destroy();
    Ok(())
}

/// Without the include line nothing can be saved: a change fails and the row says why.
pub(crate) fn check_failed() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, false)?;
    let window = open(&fixture, Page::Windows)?;
    let gaps = row(&window, "windows.gaps.inner")?;
    ensure(
        gaps.controls()
            .iter()
            .all(|control| !control.is_sensitive()),
        "the gaps can be changed without the include line",
    )?;
    // The model refuses it too, should a change get through.
    fixture
        .model
        .set("windows.gaps.inner", None, Value::Integer(9));
    wait_for_model(&fixture.model)?;
    ensure(gaps.error().is_some(), "the refused change shows no error")?;
    window.destroy();
    Ok(())
}
