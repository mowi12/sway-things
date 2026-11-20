// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Keyboard page: layouts, XKB options, the model and the active layout.

use super::*;
/// The checks of the Keyboard page's own widgets.
pub(crate) fn check_keyboard() -> Vec<(String, Outcome)> {
    vec![
        (
            "keyboard page: layouts move with their variants".to_owned(),
            check_layout_move(),
        ),
        (
            "keyboard page: layouts are added and the last one stays".to_owned(),
            check_layout_add_remove(),
        ),
        ("keyboard page: XKB options".to_owned(), check_xkb_options()),
        (
            "keyboard page: model and active layout".to_owned(),
            check_model_and_active_layout(),
        ),
    ]
}

pub(crate) const LAYOUTS: &str = "input.keyboard.xkb_layout";
pub(crate) const VARIANTS: &str = "input.keyboard.xkb_variant";
pub(crate) const XKB_OPTIONS: &str = "input.keyboard.xkb_options";

/// A keyboard with two layouts, the first one in use.
pub(crate) const KEYBOARD_WITH_LAYOUTS: &str = r#"{"identifier": "1:1:AT_Translated_Set_2_keyboard",
    "name": "AT Translated Set 2 keyboard", "type": "keyboard",
    "xkb_layout_names": ["English (US)", "German (no dead keys)"],
    "xkb_active_layout_index": 0, "libinput": {"send_events": "enabled"}}"#;

/// Moving a layout moves its variant too, as one change with one banner; Keep stores both.
pub(crate) fn check_layout_move() -> Outcome {
    let sway = laptop();
    let stored = "version = 1\n\n[input.keyboard]\nxkb_layout = [\"us\", \"de\"]\n\
                  xkb_variant = [\"\", \"nodeadkeys\"]\n";
    let fixture = live_fixture(&sway, stored)?;
    let window = open(&fixture, Page::Keyboard)?;
    let list = widget::<LayoutList>(&window)?;
    ensure(
        list.shown() == (strings(&["us", "de"]), strings(&["", "nodeadkeys"])),
        &format!("  the list shows {:?}", list.shown()),
    )?;
    ensure(list.rows().len() == 2, "  the list has no row per layout")?;
    list.move_layout(1, 0);
    wait_for_model(&fixture.model)?;
    let banner = window.safety_banner().unwrap_or_default();
    ensure(
        banner.starts_with("Keep the change to Layouts? Reverting in "),
        &format!("  the banner says {banner:?}"),
    )?;
    ensure(
        list.shown() == (strings(&["de", "us"]), strings(&["nodeadkeys", ""])),
        &format!("  the list shows {:?} while the change waits", list.shown()),
    )?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command.ends_with(r#"xkb_variant "nodeadkeys,""#)),
        &format!("  sway got {:?}", sway.commands()),
    )?;
    window.keep_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        shown(&fixture.model, LAYOUTS)? == (Value::List(strings(&["de", "us"])), true)
            && shown(&fixture.model, VARIANTS)? == (Value::List(strings(&["nodeadkeys"])), true),
        "  the kept layouts and variants aren't stored",
    )?;
    window.destroy();
    Ok(())
}

/// A layout added with the safety net is gone after Revert; the last layout can't be removed.
pub(crate) fn check_layout_add_remove() -> Outcome {
    let sway = laptop();
    let fixture = live_fixture(&sway, STORED)?;
    let window = open(&fixture, Page::Keyboard)?;
    let list = widget::<LayoutList>(&window)?;
    ensure(
        list.shown() == (strings(&["us"]), strings(&[""])),
        &format!("  the list shows {:?}", list.shown()),
    )?;
    let removes: Vec<gtk::Button> = list
        .rows()
        .iter()
        .flat_map(|row| all::<gtk::Button>(row.upcast_ref()))
        .filter(|button| !button.is::<gtk::ToggleButton>())
        .collect();
    ensure(
        removes.len() == 1 && removes.iter().all(|button| !button.is_sensitive()),
        "  the last layout can be removed",
    )?;
    list.remove(0);
    wait_for_model(&fixture.model)?;
    ensure(
        window.safety_banner().is_none() && sway.commands().is_empty(),
        "  removing the last layout was tried",
    )?;
    list.add("de", "nodeadkeys");
    wait_for_model(&fixture.model)?;
    ensure(
        list.shown() == (strings(&["us", "de"]), strings(&["", "nodeadkeys"])),
        &format!("  the list shows {:?} after adding", list.shown()),
    )?;
    ensure(
        window.safety_banner().is_some(),
        "  adding a layout shows no banner",
    )?;
    window.revert_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        list.shown() == (strings(&["us"]), strings(&[""])),
        &format!("  the list shows {:?} after Revert", list.shown()),
    )?;
    ensure(
        !shown(&fixture.model, LAYOUTS)?.1 && !shown(&fixture.model, VARIANTS)?.1,
        "  the reverted layouts are stored",
    )?;
    window.destroy();
    Ok(())
}

/// The swap switch and an option of the browser try the options through the safety net.
pub(crate) fn check_xkb_options() -> Outcome {
    let sway = laptop();
    let fixture = live_fixture(&sway, STORED)?;
    let window = open(&fixture, Page::Keyboard)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let swap_switch = all::<gtk::Switch>(&content)
        .into_iter()
        .find(|switch| {
            switch.accessible_role() == gtk::AccessibleRole::Switch
                && switch.tooltip_text().is_none()
                && switch.ancestor(SettingRow::static_type()).is_none()
        })
        .ok_or_else(|| "  no swap switch".to_owned())?;
    swap_switch.set_active(true);
    wait_for_model(&fixture.model)?;
    ensure(
        shown(&fixture.model, XKB_OPTIONS)?
            == (Value::List(strings(&["ctrl:swap_lalt_lctl"])), false),
        "  the swap switch doesn't try its option",
    )?;
    window.revert_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        !swap_switch.is_active(),
        "  the switch doesn't show the old value",
    )?;

    let browser = widget::<OptionBrowser>(&window)?;
    browser.expander().set_expanded(true);
    if let Some(check) = browser.check("compose:ralt") {
        check.set_active(true);
        wait_for_model(&fixture.model)?;
        ensure(
            shown(&fixture.model, XKB_OPTIONS)? == (Value::List(strings(&["compose:ralt"])), false),
            "  the browser doesn't try its option",
        )?;
        window.keep_button().emit_clicked();
        wait_for_model(&fixture.model)?;
        ensure(
            shown(&fixture.model, XKB_OPTIONS)? == (Value::List(strings(&["compose:ralt"])), true),
            "  the kept option isn't stored",
        )?;
    }
    window.destroy();
    Ok(())
}

/// The model is a drop-down, and _Active layout_ lists sway's layouts and switches them.
pub(crate) fn check_model_and_active_layout() -> Outcome {
    let sway = obedient_with_inputs(
        SWAY_1_10,
        format!("[{KEYBOARD_WITH_LAYOUTS}, {MOUSE}, {TOUCHPAD}]"),
    );
    let fixture = live_fixture(&sway, STORED)?;
    let window = open(&fixture, Page::Keyboard)?;
    let model_row = row(&window, "input.keyboard.xkb_model")?;
    ensure(
        model_row
            .controls()
            .iter()
            .any(ObjectExt::is::<gtk::DropDown>),
        "  the keyboard model isn't a drop-down",
    )?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let active = all::<gtk::DropDown>(&content)
        .into_iter()
        .find(|dropdown| {
            dropdown
                .model()
                .and_downcast::<gtk::StringList>()
                .and_then(|labels| labels.string(1))
                .is_some_and(|label| label == "German (no dead keys)")
        })
        .ok_or_else(|| "  no drop-down with the layouts sway reports".to_owned())?;
    ensure(
        active.is_sensitive(),
        "  the active layout can't be switched",
    )?;
    active.set_selected(1);
    wait_for_model(&fixture.model)?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command == "input type:keyboard xkb_switch_layout 1"),
        &format!("  sway got {:?}", sway.commands()),
    )?;
    window.destroy();
    Ok(())
}
