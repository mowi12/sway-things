// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Bar page with waybar: the module chips, a module's settings and the presets.

use super::*;

/// The settings of the first module of `area` (the window title after the moves above: the
/// left area starts with `sway/mode`): its dialog shows the format in a field, the options the
/// catalog doesn't know as JSON, and _Save_ stores the definition.
pub(crate) fn check_module_dialog(fixture: &Fixture, area: &gtk::FlowBox) -> Outcome {
    let chip = all::<gtk::FlowBoxChild>(area.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  the left area has no chips".to_owned())?;
    let name = chip.widget_name().to_string();
    area.emit_by_name::<()>("child-activated", &[&chip]);
    let title = format!("Module {name}");
    let mut dialog = None;
    run_until(Duration::from_secs(3), || {
        dialog = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::Window>().ok())
            .find(|window| window.title().as_deref() == Some(title.as_str()));
        dialog.is_some()
    });
    let dialog = dialog.ok_or_else(|| format!("  no dialog \"{title}\""))?;
    let entries = all::<gtk::Entry>(dialog.upcast_ref());
    let format = entries
        .first()
        .ok_or_else(|| "  the dialog has no format field".to_owned())?;
    ensure(
        format.text().contains("{}"),
        &format!("  the format field shows {:?}", format.text()),
    )?;
    format.set_text("<b>{}</b> !");
    let save = all::<gtk::Button>(dialog.upcast_ref())
        .into_iter()
        .find(|button| button.label().as_deref() == Some("Save"))
        .ok_or_else(|| "  no Save button".to_owned())?;
    save.emit_clicked();
    wait_for_model(&fixture.model)?;
    let stored = fixture
        .model
        .bar_modules()
        .and_then(Result::ok)
        .and_then(|modules| modules.definition(&name))
        .and_then(|definition| definition.get("format").cloned());
    ensure(
        stored == Some(serde_json::json!("<b>{}</b> !")),
        &format!("  {name} stores the format {stored:?}"),
    )
}

/// The value of the list setting `key` in `model`.
pub(crate) fn list_value(model: &Model, key: &str) -> Result<Vec<String>, String> {
    match model.view(setting(key)?, None).map(|view| view.value) {
        Some(Value::List(list)) => Ok(list),
        other => Err(format!("  {key} is {other:?}")),
    }
}

/// The checks of the Bar page, each with its name.
pub(crate) fn check_bar_pages() -> Vec<(String, Outcome)> {
    vec![(
        "bar: the module chips move and go with the keyboard, a preset sets the style".to_owned(),
        check_bar(),
    )]
}

/// The Bar page with waybar: an area of chips per module list, in waybar's default layout;
/// Alt+Right moves a chip within its area, Alt+Down to the next area, Delete removes it, and the
/// Glass preset sets the style at once.
pub(crate) fn check_bar() -> Outcome {
    use sway_things_core::registry::bar as keys;
    let fixture = fixture(
        SWAY,
        "version = 1\n\n[bar]\nbackend = \"waybar\"\n",
        USERS,
        true,
    )?;
    let window = open(&fixture, Page::Bar)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let areas: Vec<gtk::FlowBox> = all::<gtk::FlowBox>(&content)
        .into_iter()
        .filter(|flow| flow.has_css_class("st-bar-chips"))
        .collect();
    ensure(
        areas.len() == 3,
        &format!("  {} areas of chips", areas.len()),
    )?;
    let chips = |area: &gtk::FlowBox| -> Vec<gtk::FlowBoxChild> {
        all::<gtk::FlowBoxChild>(area.upcast_ref())
    };
    let names: Vec<String> = chips(&areas[0])
        .iter()
        .map(|chip| chip.widget_name().to_string())
        .collect();
    ensure(
        names == keys::DEFAULT_LEFT,
        &format!("  the left area shows {names:?}"),
    )?;
    let press = |chip: &gtk::FlowBoxChild, key: gtk::gdk::Key, modifiers| -> Outcome {
        use gtk::glib::translate::IntoGlib as _;
        let controller = chip
            .observe_controllers()
            .into_iter()
            .filter_map(Result::ok)
            .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .ok_or_else(|| "  a chip doesn't listen to keys".to_owned())?;
        let handled: bool =
            controller.emit_by_name("key-pressed", &[&key.into_glib(), &0u32, &modifiers]);
        ensure(handled, &format!("  {key:?} isn't handled"))
    };
    let alt = gtk::gdk::ModifierType::ALT_MASK;
    press(&chips(&areas[0])[0], gtk::gdk::Key::Right, alt)?;
    wait_for_model(&fixture.model)?;
    let left = list_value(&fixture.model, keys::MODULES_LEFT)?;
    ensure(
        left.first().map(String::as_str) == Some("sway/mode")
            && left.get(1).map(String::as_str) == Some("sway/workspaces"),
        &format!("  Alt+Right gave {left:?}"),
    )?;
    settle(&window)?;
    press(&chips(&areas[1])[0], gtk::gdk::Key::Down, alt)?;
    wait_for_model(&fixture.model)?;
    let center = list_value(&fixture.model, keys::MODULES_CENTER)?;
    let right = list_value(&fixture.model, keys::MODULES_RIGHT)?;
    ensure(
        center.is_empty() && right.last().map(String::as_str) == Some("sway/window"),
        &format!("  Alt+Down gave {center:?} and {right:?}"),
    )?;
    settle(&window)?;
    let last = chips(&areas[2])
        .pop()
        .ok_or_else(|| "  the right area has no chips".to_owned())?;
    press(
        &last,
        gtk::gdk::Key::Delete,
        gtk::gdk::ModifierType::empty(),
    )?;
    wait_for_model(&fixture.model)?;
    let right = list_value(&fixture.model, keys::MODULES_RIGHT)?;
    ensure(
        !right.iter().any(|name| name == "sway/window"),
        &format!("  Delete left {right:?}"),
    )?;
    check_module_dialog(&fixture, &areas[0])?;
    let glass = buttons_labeled(&window, "Glass")
        .into_iter()
        .next()
        .ok_or_else(|| "  no Glass preset".to_owned())?;
    glass.emit_clicked();
    wait_for_model(&fixture.model)?;
    let theme = fixture
        .model
        .view(setting(keys::THEME)?, None)
        .map(|view| view.value);
    let radius = fixture
        .model
        .view(setting(keys::RADIUS)?, None)
        .map(|view| view.value);
    ensure(
        theme == Some(Value::Choice("custom".to_owned())) && radius == Some(Value::Integer(999)),
        &format!("  the Glass preset gave {theme:?} and {radius:?}"),
    )?;
    window.destroy();
    Ok(())
}
