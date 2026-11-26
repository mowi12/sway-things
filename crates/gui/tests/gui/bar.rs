// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Bar page with Waybar: the preview and its summary, the lanes moved with the keyboard,
//! the menu and a drop, a removal undone, the picker, an item's basic settings, a command of
//! the user's with its test run, the looks and the switch to swaybar, the handover of a waybar
//! the user's sway config starts; screenshots of each part in light and dark.

use super::*;

/// The labels' texts in `widget` and below it.
fn texts(widget: &gtk::Widget) -> Vec<String> {
    all::<gtk::Label>(widget)
        .iter()
        .map(|label| label.text().to_string())
        .collect()
}

/// The open window titled `title`, waited for.
fn window_titled(title: &str) -> Result<gtk::Window, String> {
    let mut found = None;
    run_until(Duration::from_secs(3), || {
        found = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::Window>().ok())
            .find(|window| window.title().as_deref() == Some(title) && window.is_visible());
        found.is_some()
    });
    found.ok_or_else(|| format!("  no window \"{title}\""))
}

/// The button in `widget` whose label or texts say `label`.
fn button_with(widget: &gtk::Widget, label: &str) -> Result<gtk::Button, String> {
    all::<gtk::Button>(widget)
        .into_iter()
        .find(|button| {
            button.label().as_deref() == Some(label)
                || texts(button.upcast_ref()).iter().any(|text| text == label)
        })
        .ok_or_else(|| format!("  no button \"{label}\""))
}

/// The three lanes of the open Bar page.
fn lanes(window: &Window) -> Result<Vec<gtk::ListBox>, String> {
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let lanes: Vec<gtk::ListBox> = all::<gtk::ListBox>(&content)
        .into_iter()
        .filter(|list| list.has_css_class("st-bar-lane-list"))
        .collect();
    ensure(lanes.len() == 3, &format!("  {} lanes", lanes.len()))?;
    Ok(lanes)
}

/// The rows of `lane`.
fn rows_of(lane: &gtk::ListBox) -> Vec<gtk::ListBoxRow> {
    all::<gtk::ListBoxRow>(lane.upcast_ref())
}

/// The box of `lane`, with its heading and _Add Item…_.
fn lane_box(lane: &gtk::ListBox) -> Result<gtk::Widget, String> {
    lane.parent()
        .ok_or_else(|| "  a lane has no box".to_owned())
}

/// Presses `key` with `modifiers` on `row`.
fn press(row: &gtk::ListBoxRow, key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> Outcome {
    use gtk::glib::translate::IntoGlib as _;
    let controller = row
        .observe_controllers()
        .into_iter()
        .filter_map(Result::ok)
        .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
        .ok_or_else(|| "  a row doesn't listen to keys".to_owned())?;
    let handled: bool =
        controller.emit_by_name("key-pressed", &[&key.into_glib(), &0u32, &modifiers]);
    ensure(handled, &format!("  {key:?} isn't handled"))
}

/// The value of the list setting `key` in `model`.
pub(crate) fn list_value(model: &Model, key: &str) -> Result<Vec<String>, String> {
    match model.view(setting(key)?, None).map(|view| view.value) {
        Some(Value::List(list)) => Ok(list),
        other => Err(format!("  {key} is {other:?}")),
    }
}

/// The value of setting `key` in `model`.
fn value(model: &Model, key: &str) -> Result<Option<Value>, String> {
    Ok(model.view(setting(key)?, None).map(|view| view.value))
}

/// Scrolls the page of `window` so `widget` shows.
fn scroll_to(window: &Window, widget: &impl IsA<gtk::Widget>) -> Outcome {
    let viewport = window
        .content()
        .and_then(|content| find::<gtk::Viewport>(&content))
        .ok_or_else(|| "  the page doesn't scroll".to_owned())?;
    viewport.scroll_to(widget, None);
    settle(window)
}

/// The checks of the Bar page, each with its name.
pub(crate) fn check_bar_pages() -> Vec<(String, Outcome)> {
    vec![
        (
            "bar: the lanes, the picker, an item, a command with its test, the looks, the program"
                .to_owned(),
            check_bar(),
        ),
        (
            "bar: the preview draws waybar's widgets with the user's own stylesheet".to_owned(),
            check_bar_preview(),
        ),
        (
            "bar: the user's own bar is taken over and given back".to_owned(),
            check_bar_takeover(),
        ),
        (
            "bar: a wide preview pans with a drag and the wheel".to_owned(),
            check_bar_pan(),
        ),
        (
            "bar: the looks' cards are drawn as the preview is".to_owned(),
            check_bar_looks(),
        ),
        (
            "bar: items become groups in the lanes, by a drop, the keyboard and the menus"
                .to_owned(),
            check_bar_groups(),
        ),
        (
            "bar: the user's own style is a look, and other looks turn it off".to_owned(),
            check_bar_own_style(),
        ),
    ]
}

/// The Bar page with Waybar: every part, used as a user would, and its screenshots.
fn check_bar() -> Outcome {
    let settings = gtk::Settings::default().ok_or_else(|| "  no GTK settings".to_owned())?;
    check_bar_parts()?;
    for dark in [false, true] {
        set_dark(&settings, dark);
        let scheme = if dark { "dark" } else { "light" };
        let result = shoot_bar(scheme).and_then(|()| check_handover(scheme));
        if result.is_err() {
            set_dark(&settings, false);
            return result;
        }
    }
    set_dark(&settings, false);
    Ok(())
}

/// The interactions of the Bar page.
#[expect(clippy::too_many_lines, reason = "one user's way through the page")]
fn check_bar_parts() -> Outcome {
    use gtk::gdk::{Key, ModifierType};
    use sway_things_core::registry::bar as keys;
    let fixture = fixture(
        SWAY,
        "version = 1\n\n[bar]\nbackend = \"waybar\"\n",
        USERS,
        true,
    )?;
    let model = &fixture.model;
    let window = open(&fixture, Page::Bar)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    // The preview says the bar in words.
    let summary = all::<gtk::Label>(&content)
        .into_iter()
        .find(|label| label.has_css_class("st-bar-summary"))
        .ok_or_else(|| "  no summary of the bar".to_owned())?;
    ensure(
        summary
            .text()
            .starts_with("Left: Workspaces, Binding mode (resize)")
            && summary
                .text()
                .contains("Center: Window title (Sway Things).")
            && summary.text().contains("Clock (")
            && !summary
                .text()
                .chars()
                .any(|char| ('\u{e000}'..='\u{f8ff}').contains(&char)),
        &format!("  the summary says {:?}", summary.text()),
    )?;
    let lane = lanes(&window)?;
    let names: Vec<String> = rows_of(&lane[0])
        .iter()
        .map(|row| row.widget_name().to_string())
        .collect();
    ensure(
        names == keys::DEFAULT_LEFT,
        &format!("  the left lane shows {names:?}"),
    )?;
    // Alt+Down moves an item down its lane, Alt+Right to the next lane.
    press(&rows_of(&lane[0])[0], Key::Down, ModifierType::ALT_MASK)?;
    wait_for_model(model)?;
    let left = list_value(model, keys::MODULES_LEFT)?;
    ensure(
        left.get(..2) == Some(&strings(&["sway/mode", "sway/workspaces"])[..]),
        &format!("  Alt+Down gave {left:?}"),
    )?;
    settle(&window)?;
    let focused = rows_of(&lane[0])
        .iter()
        .position(gtk::prelude::WidgetExt::is_focus)
        .ok_or_else(|| "  no row of the left lane has the focus after the move".to_owned())?;
    ensure(
        focused == 1,
        &format!("  row {focused} has the focus, not the moved one"),
    )?;
    press(&rows_of(&lane[1])[0], Key::Right, ModifierType::ALT_MASK)?;
    wait_for_model(model)?;
    let center = list_value(model, keys::MODULES_CENTER)?;
    let right = list_value(model, keys::MODULES_RIGHT)?;
    ensure(
        center.is_empty() && right.last().map(String::as_str) == Some("sway/window"),
        &format!("  Alt+Right gave {center:?} and {right:?}"),
    )?;
    settle(&window)?;
    // Ctrl+Z on the page takes the move back.
    let body = page_view(&window)
        .ok_or_else(|| "  no page".to_owned())?
        .content();
    let undo = all::<gtk::Widget>(body.upcast_ref())
        .into_iter()
        .flat_map(|widget| {
            widget
                .observe_controllers()
                .into_iter()
                .filter_map(Result::ok)
                .collect::<Vec<_>>()
        })
        .filter_map(|controller| controller.downcast::<gtk::ShortcutController>().ok())
        .find_map(|controller| {
            let shortcut = controller.item(0).and_downcast::<gtk::Shortcut>()?;
            let trigger = shortcut.trigger()?.to_str();
            (trigger == "<Control>z")
                .then(|| shortcut.action())
                .flatten()
        })
        .ok_or_else(|| "  no Ctrl+Z on the page".to_owned())?;
    undo.activate(gtk::ShortcutActionFlags::empty(), &body, None);
    wait_for_model(model)?;
    ensure(
        list_value(model, keys::MODULES_CENTER)? == strings(&["sway/window"]),
        "  Ctrl+Z didn't bring the window title back to the center",
    )?;
    settle(&window)?;
    // The menu moves an item to another lane.
    let row = rows_of(&lane[1])
        .into_iter()
        .next()
        .ok_or_else(|| "  the center lane is empty".to_owned())?;
    let menu = all::<gtk::MenuButton>(row.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  a row has no menu".to_owned())?;
    let popover = menu
        .popover()
        .ok_or_else(|| "  the menu has no popover".to_owned())?;
    button_with(popover.upcast_ref(), "Move to Left")?.emit_clicked();
    wait_for_model(model)?;
    ensure(
        list_value(model, keys::MODULES_LEFT)?
            .last()
            .map(String::as_str)
            == Some("sway/window"),
        "  the menu didn't move the window title to the left",
    )?;
    settle(&window)?;
    // A drop puts an item where it lands; the center lane is empty, so at its end.
    let drop = lane_box(&lane[1])?
        .observe_controllers()
        .into_iter()
        .filter_map(Result::ok)
        .find_map(|controller| controller.downcast::<gtk::DropTarget>().ok())
        .ok_or_else(|| "  the center lane takes no drops".to_owned())?;
    let left = list_value(model, keys::MODULES_LEFT)?;
    // The signal takes the dropped value as a boxed GValue.
    let payload = gtk::glib::value::BoxedValue(format!("0\n{}", left.len() - 1).to_value());
    let dropped: bool = drop.emit_by_name("drop", &[&payload, &10.0f64, &60.0f64]);
    ensure(dropped, "  the drop wasn't taken")?;
    wait_for_model(model)?;
    ensure(
        list_value(model, keys::MODULES_CENTER)? == strings(&["sway/window"]),
        "  the drop didn't put the window title in the center",
    )?;
    settle(&window)?;
    // Delete removes an item, and the note's Undo brings it back.
    let last = rows_of(&lane[2])
        .pop()
        .ok_or_else(|| "  the right lane has no rows".to_owned())?;
    let removed = last.widget_name().to_string();
    press(&last, Key::Delete, ModifierType::empty())?;
    wait_for_model(model)?;
    ensure(
        !list_value(model, keys::MODULES_RIGHT)?.contains(&removed),
        &format!("  Delete left {removed}"),
    )?;
    let toast = all::<gtk::Revealer>(&content)
        .into_iter()
        .find(gtk::Revealer::reveals_child)
        .ok_or_else(|| "  no note after the removal".to_owned())?;
    ensure(
        texts(toast.upcast_ref())
            .iter()
            .any(|text| text.starts_with("Removed ")),
        "  the note doesn't say what was removed",
    )?;
    button_with(toast.upcast_ref(), "Undo")?.emit_clicked();
    wait_for_model(model)?;
    ensure(
        list_value(model, keys::MODULES_RIGHT)?.last() == Some(&removed),
        "  Undo didn't bring the item back",
    )?;
    settle(&window)?;
    check_picker(&fixture, &window)?;
    check_item_dialog(&fixture, &window)?;
    check_command(&fixture, &window)?;
    check_presets(&fixture, &window)?;
    check_switch(&fixture, &window)?;
    window.destroy();
    Ok(())
}

/// _Add Item…_ in the center opens the picker; its search finds the keyboard layout, Enter
/// adds the first match.
fn check_picker(fixture: &Fixture, window: &Window) -> Outcome {
    use sway_things_core::registry::bar as keys;
    let lane = lanes(window)?;
    button_with(&lane_box(&lane[1])?, "Add Item…")?.emit_clicked();
    let picker = window_titled("Add an Item to the Center")?;
    let search = all::<gtk::SearchEntry>(picker.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  the picker has no search".to_owned())?;
    let rows = all::<gtk::ListBoxRow>(picker.upcast_ref());
    ensure(
        rows.len() > 20,
        &format!("  the picker offers {} items", rows.len()),
    )?;
    search.set_text("keyboard layout");
    let shown = |rows: &[gtk::ListBoxRow]| rows.iter().filter(|row| row.is_visible()).count();
    run_until(Duration::from_secs(2), || shown(&rows) < 4);
    ensure(
        shown(&rows) >= 1 && shown(&rows) < 4,
        &format!("  the search shows {} items", shown(&rows)),
    )?;
    search.emit_by_name::<()>("activate", &[]);
    wait_for_model(&fixture.model)?;
    let center = list_value(&fixture.model, keys::MODULES_CENTER)?;
    ensure(
        center.iter().any(|name| name.starts_with("sway/language")),
        &format!("  the picker added nothing to the center: {center:?}"),
    )?;
    ensure(!picker.is_visible(), "  the picker stays open")?;
    settle(window)
}

/// The clock's settings: a format picked from the examples; seconds update every second.
fn check_item_dialog(fixture: &Fixture, window: &Window) -> Outcome {
    let lane = lanes(window)?;
    let clock = rows_of(&lane[2])
        .into_iter()
        .find(|row| row.widget_name() == "clock")
        .ok_or_else(|| "  no clock on the right".to_owned())?;
    lane[2].emit_by_name::<()>("row-activated", &[&clock]);
    let dialog = window_titled("Clock")?;
    let formats = all::<gtk::CheckButton>(dialog.upcast_ref());
    ensure(
        formats.len() >= 6,
        &format!("  {} clock formats", formats.len()),
    )?;
    ensure(formats[0].is_active(), "  waybar's own format isn't picked")?;
    formats[2].set_active(true);
    button_with(dialog.upcast_ref(), "Save")?.emit_clicked();
    wait_for_model(&fixture.model)?;
    let definition = fixture
        .model
        .bar_modules()
        .and_then(Result::ok)
        .and_then(|modules| modules.definition("clock"))
        .unwrap_or_default();
    ensure(
        definition.get("format") == Some(&serde_json::json!("{:%H:%M:%S}"))
            && definition.get("interval") == Some(&serde_json::json!(1))
            && definition.contains_key("tooltip-format"),
        &format!("  the clock stores {definition:?}"),
    )?;
    settle(window)
}

/// A command of the user's: its program and arguments, JSON output, a test run without a shell,
/// and the quoted `exec` it stores.
fn check_command(fixture: &Fixture, window: &Window) -> Outcome {
    use sway_things_core::registry::bar as keys;
    let lane = lanes(window)?;
    button_with(&lane_box(&lane[2])?, "Add Item…")?.emit_clicked();
    let picker = window_titled("Add an Item to the Right")?;
    let search = all::<gtk::SearchEntry>(picker.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  the picker has no search".to_owned())?;
    search.set_text("your own command");
    let rows = all::<gtk::ListBoxRow>(picker.upcast_ref());
    run_until(Duration::from_secs(2), || {
        rows.iter().filter(|row| row.is_visible()).count() == 1
    });
    search.emit_by_name::<()>("activate", &[]);
    let dialog = window_titled("Your Own Command")?;
    let entries = all::<gtk::Entry>(dialog.upcast_ref());
    let by_placeholder = |placeholder: &str| {
        entries
            .iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some(placeholder))
            .cloned()
            .ok_or_else(|| format!("  no field \"{placeholder}\""))
    };
    by_placeholder("updates")?.set_text("hello");
    by_placeholder("A program or the path of a script")?.set_text("printf");
    let arguments = all::<gtk::TextView>(dialog.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  no field for the arguments".to_owned())?;
    arguments
        .buffer()
        .set_text("{\"text\": \"hi $USER\", \"class\": \"ok\"}\\n");
    let output = all::<gtk::DropDown>(dialog.upcast_ref())
        .into_iter()
        .find(|dropdown| {
            dropdown
                .model()
                .and_downcast::<gtk::StringList>()
                .and_then(|list| list.string(0))
                .as_deref()
                == Some("Plain text")
        })
        .ok_or_else(|| "  no choice of the output".to_owned())?;
    output.set_selected(1);
    button_with(dialog.upcast_ref(), "Test")?.emit_clicked();
    let result = all::<gtk::Label>(dialog.upcast_ref())
        .into_iter()
        .find(|label| label.has_css_class("st-bar-test-result"))
        .ok_or_else(|| "  no result of the test".to_owned())?;
    run_until(Duration::from_secs(10), || {
        result.text().contains("Exit code")
    });
    let shown = result.text();
    ensure(
        shown.contains("Exit code 0") && shown.contains("text = hi $USER"),
        &format!("  the test says {shown:?}"),
    )?;
    shoot(&dialog, "bar-command")?;
    button_with(dialog.upcast_ref(), "Add")?.emit_clicked();
    wait_for_model(&fixture.model)?;
    let definition = fixture
        .model
        .bar_modules()
        .and_then(Result::ok)
        .and_then(|modules| modules.definition("custom/hello"))
        .unwrap_or_default();
    ensure(
        definition.get("exec")
            == Some(&serde_json::json!(
                "printf '{\"text\": \"hi $USER\", \"class\": \"ok\"}\\n'"
            ))
            && definition.get("return-type") == Some(&serde_json::json!("json"))
            && definition.get("interval") == Some(&serde_json::json!(10)),
        &format!("  the command stores {definition:?}"),
    )?;
    ensure(
        list_value(&fixture.model, keys::MODULES_RIGHT)?.contains(&"custom/hello".to_owned()),
        "  the command isn't on the right",
    )?;
    settle(window)
}

/// A look's card sets the style and leaves the items; _Undo_ takes it back.
fn check_presets(fixture: &Fixture, window: &Window) -> Outcome {
    use sway_things_core::registry::bar as keys;
    let model = &fixture.model;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let before = list_value(model, keys::MODULES_RIGHT)?;
    let card = all::<gtk::Button>(&content)
        .into_iter()
        .find(|button| {
            button.has_css_class("st-bar-preset")
                && texts(button.upcast_ref())
                    .iter()
                    .any(|text| text == "Floating islands")
        })
        .ok_or_else(|| "  no card Floating islands".to_owned())?;
    card.emit_clicked();
    wait_for_model(model)?;
    ensure(
        value(model, keys::THEME)? == Some(Value::Choice("custom".to_owned()))
            && value(model, keys::SHAPE)? == Some(Value::Choice("pills".to_owned())),
        "  the card didn't set the look",
    )?;
    ensure(
        list_value(model, keys::MODULES_RIGHT)? == before,
        "  the card changed the items",
    )?;
    settle(window)?;
    ensure(
        card.has_css_class("st-current"),
        "  the card isn't marked as in use",
    )?;
    let toast = all::<gtk::Revealer>(&content)
        .into_iter()
        .find(gtk::Revealer::reveals_child)
        .ok_or_else(|| "  no note after the look".to_owned())?;
    button_with(toast.upcast_ref(), "Undo")?.emit_clicked();
    wait_for_model(model)?;
    ensure(
        value(model, keys::THEME)? == Some(Value::Choice("waybar".to_owned())),
        "  Undo didn't take the look back",
    )?;
    settle(window)
}

/// _Change…_ switches to swaybar after saying what it can't show; the lanes stay, greyed.
fn check_switch(fixture: &Fixture, window: &Window) -> Outcome {
    use sway_things_core::registry::bar as keys;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    button_with(&content, "Change…")?.emit_clicked();
    let dialog = window_titled("Bar Program")?;
    let radios = all::<gtk::CheckButton>(dialog.upcast_ref());
    ensure(radios.len() == 3, &format!("  {} programs", radios.len()))?;
    radios[1].set_active(true);
    let said = texts(dialog.upcast_ref()).join("\n");
    ensure(
        said.contains("Not shown by swaybar") && said.contains("stay saved"),
        &format!("  the dialog says {said:?}"),
    )?;
    shoot(&dialog, "bar-switch")?;
    button_with(dialog.upcast_ref(), "Use swaybar")?.emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        value(&fixture.model, keys::BACKEND)? == Some(Value::Choice("swaybar".to_owned())),
        "  the program didn't change",
    )?;
    settle(window)?;
    let lane = lanes(window)?;
    ensure(
        !lane[0].is_sensitive() && !rows_of(&lane[0]).is_empty(),
        "  the lanes aren't kept, greyed, with swaybar",
    )
}

/// With `bar { swaybar_command waybar }` in the user's sway config, the details point at _Use
/// Sway Things' Bar_, which needs no edit. With `exec_always waybar`, which can't be taken over,
/// they say how the user changes that file themselves (Sway Things never writes it): a command
/// for a terminal that keeps a copy and comments the line out, the one that takes it back, or the
/// file opened at the line; then a reload of sway. _Reload waybar_ can't be used while no
/// waybar of Sway Things runs, and says why. Once the line is gone, Sway Things starts waybar
/// itself.
fn check_handover(scheme: &str) -> Outcome {
    let block = "set $mod Mod4\nbar {\n    swaybar_command waybar\n    position top\n}\n";
    let fixture = fixture(
        SWAY,
        "version = 1\n\n[bar]\nbackend = \"waybar\"\n",
        block,
        true,
    )?;
    // Which bar block a line is in is read from the file itself.
    let on_disk = |text: &str| -> Outcome {
        if let Some(parent) = fixture.main.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&fixture.main, format!("{text}{}", fixture.include_line()))
            .map_err(|error| error.to_string())
    };
    on_disk(block)?;
    let window = open(&fixture, Page::Bar)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let details = all::<gtk::Expander>(&content)
        .into_iter()
        .find(|expander| expander.has_css_class("st-bar-details"))
        .ok_or_else(|| "  no details".to_owned())?;
    ensure(details.is_expanded(), "  the details didn't open")?;
    let said = texts(details.upcast_ref()).join("\n");
    ensure(
        said.contains("Use Sway Things' Bar, above")
            && !said.contains("Remove that whole bar")
            && !said.contains("starts swaybar too"),
        &format!("  with a bar block the details say {said:?}"),
    )?;
    button_with(&content, "Use Sway Things' Bar")?;
    // A line that can't be taken over: the steps in the file.
    let exec = "set $mod Mod4\nexec_always waybar\n";
    on_disk(exec)?;
    fixture.model.set_status(fixture.status(exec, true));
    settle(&window)?;
    let said = texts(details.upcast_ref()).join("\n");
    for step in [
        "Sway Things never changes your files",
        "1. Run this in a terminal.",
        ".bak-sway-things",
        "sed -i '2s/^/# /'",
        "To take it back later",
        "sed -i '2s/^# //'",
        "2. Or open the file and put # in front of line 2 yourself.",
        "3. Reload sway",
        "Nothing to reload",
    ] {
        ensure(
            said.contains(step),
            &format!("  {step:?} isn't in {said:?}"),
        )?;
    }
    let reload = button_with(details.upcast_ref(), "Reload waybar")?;
    ensure(
        !reload.is_sensitive(),
        "  Reload waybar can be used without a waybar of Sway Things",
    )?;
    button_with(details.upcast_ref(), "Open in Editor")?;
    button_with(details.upcast_ref(), "Reload Sway")?;
    scroll_to(&window, &details)?;
    shoot(window.upcast_ref(), &format!("bar-handover-steps-{scheme}"))?;
    // The user removes the line; the watch of their config tells the model.
    on_disk("set $mod Mod4\n")?;
    fixture
        .model
        .set_status(fixture.status("set $mod Mod4\n", true));
    settle(&window)?;
    let said = texts(details.upcast_ref()).join("\n");
    ensure(
        said.contains("Sway Things starts waybar with its own config when sway starts")
            && !said.contains("Run this in a terminal"),
        &format!("  without the line the details say {said:?}"),
    )?;
    shoot(
        window.upcast_ref(),
        &format!("bar-handover-removed-{scheme}"),
    )
}

/// Screenshots of the page's parts in `scheme`, each checked for contrast. On SwayFX, so the
/// looks include Glass.
fn shoot_bar(scheme: &str) -> Outcome {
    let fixture = fixture(
        SWAYFX,
        "version = 1\n\n[bar]\nbackend = \"waybar\"\n",
        USERS,
        true,
    )?;
    let window = open(&fixture, Page::Bar)?;
    shoot(window.upcast_ref(), &format!("bar-top-{scheme}"))?;
    let lane = lanes(&window)?;
    scroll_to(&window, &lane[0])?;
    shoot(window.upcast_ref(), &format!("bar-lanes-{scheme}"))?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let presets = all::<gtk::Box>(&content)
        .into_iter()
        .find(|widget| widget.has_css_class("st-bar-presets"))
        .ok_or_else(|| "  no looks".to_owned())?;
    scroll_to(&window, &presets)?;
    shoot(window.upcast_ref(), &format!("bar-presets-{scheme}"))?;
    button_with(&lane_box(&lane[1])?, "Add Item…")?.emit_clicked();
    let picker = window_titled("Add an Item to the Center")?;
    shoot(&picker, &format!("bar-picker-{scheme}"))?;
    picker.close();
    let clock = rows_of(&lane[2])
        .into_iter()
        .find(|row| row.widget_name() == "clock")
        .ok_or_else(|| "  no clock on the right".to_owned())?;
    lane[2].emit_by_name::<()>("row-activated", &[&clock]);
    let dialog = window_titled("Clock")?;
    shoot(&dialog, &format!("bar-clock-{scheme}"))?;
    dialog.close();
    button_with(&content, "Change…")?.emit_clicked();
    let program = window_titled("Bar Program")?;
    shoot(&program, &format!("bar-program-{scheme}"))?;
    program.close();
    window.destroy();
    Ok(())
}

/// The widget named `name` in `widget` and below it.
fn named(widget: &gtk::Widget, name: &str) -> Result<gtk::Widget, String> {
    all::<gtk::Widget>(widget)
        .into_iter()
        .find(|widget| widget.widget_name() == name)
        .ok_or_else(|| format!("  no widget #{name} in the preview"))
}

/// The children of `widget`.
fn children(widget: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut children = Vec::new();
    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        children.push(widget);
    }
    children
}

/// Whether `widget`'s text color is `rgb(red, green, blue)`.
fn colored(widget: &gtk::Widget, [red, green, blue]: [u8; 3]) -> bool {
    let color = widget.color();
    let near = |channel: f32, wanted: u8| (channel * 255.0 - f32::from(wanted)).abs() < 1.0;
    near(color.red(), red) && near(color.green(), green) && near(color.blue(), blue)
}

/// The preview of waybar: waybar's widgets with its names and classes, a drawer as its first
/// member and a group as all of them, a command of the user's as its format without running it,
/// drawn with the user's own stylesheet, which reaches nothing else of the app; what can't be
/// parsed or would be fetched is left out.
#[expect(clippy::too_many_lines, reason = "one picture, checked part by part")]
fn check_bar_preview() -> Outcome {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let extra = dir.path().join("style.css");
    std::fs::write(
        &extra,
        "@define-color accent #b162d5;\n\
         @import url(\"https://example.com/remote.css\");\n\
         window#waybar { background: transparent; }\n\
         label { color: rgb(10, 20, 30); }\n\
         #workspaces button.focused { background: @accent; }\n\
         #clock { color: rgb(1, 2, 3); -gtk-icon-effect: dim; }\n\
         #custom-media { background-image: url(\"https://example.com/a.png\"); \
         color: rgb(7, 8, 9); }\n\
         #cpu { color: nonsense(; }\n\
         this is { not css\n",
    )
    .map_err(|error| error.to_string())?;
    let stored = format!(
        "version = 1\n\n[bar]\nbackend = \"waybar\"\nmodules_left = [\"sway/workspaces\"]\n\
         modules_center = [\"group/time\"]\nmodules_right = [\"group/info\", \"custom/media\"]\n\n\
         [bar.style]\ntheme = \"plain\"\nextra_css = {:?}\n",
        extra.to_string_lossy()
    );
    let fixture = fixture(SWAY, &stored, USERS, true)?;
    let model = &fixture.model;
    let mut modules = sway_things_core::bar::Modules::default();
    for (name, definition) in [
        (
            "group/time",
            serde_json::json!({"modules": ["clock", "custom/pomodoro"]}),
        ),
        (
            "group/info",
            serde_json::json!({"drawer": {}, "modules": ["cpu", "memory"]}),
        ),
        (
            "custom/media",
            serde_json::json!({"exec": "touch /nonexistent/ran", "format": "♪ {}"}),
        ),
    ] {
        if let Some(definition) = definition.as_object() {
            modules.set(name, definition.clone());
        }
    }
    model.set_bar_modules(modules);
    wait_for_model(model)?;
    let window = open(&fixture, Page::Bar)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let bar = all::<gtk::Box>(&content)
        .into_iter()
        .find(|widget| {
            widget
                .css_classes()
                .iter()
                .any(|class| class.starts_with("st-wb-"))
        })
        .ok_or_else(|| "  no waybar window in the preview".to_owned())?;
    let bar: &gtk::Widget = bar.upcast_ref();
    ensure(
        bar.has_css_class("top"),
        "  the bar has no class for its position",
    )?;
    // The lanes, each module in a `widget`.
    for lane in ["modules-left", "modules-center", "modules-right"] {
        let found = all::<gtk::Box>(bar)
            .into_iter()
            .any(|widget| widget.has_css_class(lane));
        ensure(found, &format!("  no .{lane}"))?;
    }
    let workspaces = named(bar, "workspaces")?;
    ensure(
        workspaces
            .parent()
            .is_some_and(|parent| parent.css_name() == "widget"),
        "  the workspaces aren't in a `widget`",
    )?;
    let buttons = all::<gtk::Button>(&workspaces);
    ensure(
        buttons.len() == 3
            && buttons[0].has_css_class("focused")
            && !buttons[1].has_css_class("focused"),
        "  the workspaces aren't three buttons with the first focused",
    )?;
    // A group shows its members; a drawer only its first.
    let time = children(&named(bar, "time")?);
    let names: Vec<String> = time
        .iter()
        .flat_map(children)
        .map(|widget| widget.widget_name().to_string())
        .collect();
    ensure(
        names == strings(&["clock", "custom-pomodoro"]),
        &format!("  the group shows {names:?}"),
    )?;
    // A drawer: its first member, then the others in a closed revealer with the drawer's class.
    let info = children(&named(bar, "info")?);
    let names: Vec<String> = info
        .iter()
        .filter(|widget| widget.css_name() == "widget")
        .flat_map(children)
        .map(|widget| widget.widget_name().to_string())
        .collect();
    ensure(
        names == strings(&["cpu"]),
        &format!("  the drawer shows {names:?}"),
    )?;
    let drawer = info
        .iter()
        .find_map(|widget| widget.clone().downcast::<gtk::Revealer>().ok())
        .ok_or_else(|| "  the drawer has no revealer".to_owned())?;
    let hidden: Vec<gtk::Widget> = all::<gtk::Widget>(drawer.upcast_ref())
        .into_iter()
        .filter(|widget| widget.css_name() == "widget")
        .collect();
    ensure(
        !drawer.reveals_child()
            && drawer.has_css_class("drawer")
            && hidden.len() == 1
            && hidden[0].has_css_class("drawer-child")
            && named(&hidden[0], "memory").is_ok(),
        "  the drawer's other members aren't in a closed revealer as drawer children",
    )?;
    // A command of the user's shows its format with its name, and never ran.
    let media = named(bar, "custom-media")?
        .downcast::<gtk::Label>()
        .map_err(|_| "  the command isn't a label".to_owned())?;
    ensure(
        media.text() == "♪ media",
        &format!("  the command shows {:?}", media.text()),
    )?;
    ensure(!Path::new("/nonexistent/ran").exists(), "  the command ran")?;
    // The user's stylesheet draws the preview, rule by rule, and nothing else.
    settle(&window)?;
    ensure(
        colored(&named(bar, "clock")?, [1, 2, 3]),
        "  #clock isn't in the color of the user's stylesheet",
    )?;
    ensure(
        colored(media.upcast_ref(), [7, 8, 9]),
        "  a rule with a remote picture lost its other declarations",
    )?;
    ensure(
        colored(&named(bar, "custom-pomodoro")?, [10, 20, 30]),
        "  the user's `label` rule doesn't reach the preview's labels",
    )?;
    // The cards of the looks are pictures of the bar too, with the user's stylesheet.
    let outside = all::<gtk::Label>(&content)
        .into_iter()
        .filter(|label| {
            let mut parent = label.parent();
            while let Some(widget) = parent {
                if widget.has_css_class("st-bar-picture") {
                    return false;
                }
                parent = widget.parent();
            }
            true
        })
        .filter(|label| colored(label.upcast_ref(), [10, 20, 30]))
        .map(|label| label.text().to_string())
        .collect::<Vec<_>>();
    ensure(
        outside.is_empty(),
        &format!("  the user's stylesheet reaches the app: {outside:?}"),
    )?;
    // The caption says that commands don't run here.
    let caption = all::<gtk::Label>(&content)
        .into_iter()
        .map(|label| label.text().to_string())
        .find(|text| text.starts_with("Sample values"))
        .ok_or_else(|| "  no caption".to_owned())?;
    ensure(
        caption.contains("Your scripts don't run here"),
        &format!("  the caption says {caption:?}"),
    )?;
    shoot(window.upcast_ref(), "bar-preview-extra-css")?;
    window.destroy();
    Ok(())
}

/// The labels' texts in `widget` and below it, joined.
fn all_text(widget: &gtk::Widget) -> String {
    texts(widget).join("\n")
}

/// Where the user's sway config starts their own waybar in a `bar` block: _Use Sway Things'
/// Bar_ takes it over by its ID and asks to keep it; _Switch Back_ and _Switch Back to My Own
/// Bar_ give it back; screenshots of each state.
fn check_bar_takeover() -> Outcome {
    use sway_things_core::registry::bar as keys;
    let users = "bar {\n    swaybar_command waybar\n}\n";
    let fixture = fixture(SWAY, "version = 1\n", users, true)?;
    let model = &fixture.model;
    // Hard rule 1: the user's config stays as it is, byte for byte and with its time.
    if let Some(parent) = fixture.main.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&fixture.main, format!("{users}{}", fixture.include_line()))
        .map_err(|error| error.to_string())?;
    let untouched = || -> Result<(Vec<u8>, std::time::SystemTime), String> {
        let bytes = std::fs::read(&fixture.main).map_err(|error| error.to_string())?;
        let modified = std::fs::metadata(&fixture.main)
            .and_then(|meta| meta.modified())
            .map_err(|error| error.to_string())?;
        Ok((bytes, modified))
    };
    let before = untouched()?;
    let window = open(&fixture, Page::Bar)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let card = all::<gtk::Box>(&content)
        .into_iter()
        .find(|widget| widget.has_css_class("st-bar-takeover"))
        .ok_or_else(|| "  no part about the user's own bar".to_owned())?;
    let card: &gtk::Widget = card.upcast_ref();
    ensure(
        all_text(card).contains("Your bar: your own Waybar, started by your sway config."),
        &format!("  the card says {:?}", all_text(card)),
    )?;
    let use_ours = button_with(card, "Use Sway Things' Bar")?;
    shoot(window.upcast_ref(), "bar-takeover-offer")?;
    if !sway_things_core::bar::process::installed() {
        // Without waybar, nothing to switch to.
        ensure(
            !use_ours.is_sensitive() && all_text(card).contains("Install the waybar package"),
            "  the button works without waybar",
        )?;
        window.destroy();
        return Ok(());
    }
    use_ours.emit_clicked();
    wait_for_model(model)?;
    ensure(
        value(model, keys::BACKEND)? == Some(Value::Choice("waybar".to_owned()))
            && list_value(model, keys::TAKE_OVER)? == strings(&["bar-0"]),
        "  Use Sway Things' Bar didn't take bar-0 over",
    )?;
    // Until Sway Things' waybar runs, it can't be kept yet, only switched back.
    ensure(
        all_text(card).contains("Starting Sway Things' bar…")
            && !button_with(card, "Keep")?.is_sensitive(),
        &format!("  no banner: {:?}", all_text(card)),
    )?;
    shoot(window.upcast_ref(), "bar-takeover-trial")?;
    button_with(card, "Switch Back")?.emit_clicked();
    wait_for_model(model)?;
    ensure(
        value(model, keys::BACKEND)? == Some(Value::Choice("none".to_owned()))
            && list_value(model, keys::TAKE_OVER)?.is_empty(),
        "  Switch Back didn't give the bar back",
    )?;
    // The stand-in runs no waybar: it switches back by itself and says why.
    button_with(card, "Use Sway Things' Bar")?.emit_clicked();
    wait_for_model(model)?;
    let said = run_until(Duration::from_secs(15), || {
        all_text(card).contains("Sway Things' Waybar didn't start")
    });
    wait_for_model(model)?;
    ensure(
        said && list_value(model, keys::TAKE_OVER)?.is_empty(),
        &format!("  without a waybar: {:?}", all_text(card)),
    )?;
    // While it shows in place of theirs, the way back.
    model.set(keys::TAKE_OVER, None, Value::List(strings(&["bar-0"])));
    model.set(keys::BACKEND, None, Value::Choice("waybar".to_owned()));
    wait_for_model(model)?;
    settle(&window)?;
    let back = button_with(card, "Switch Back to My Own Bar")?;
    shoot(window.upcast_ref(), "bar-takeover-active")?;
    back.emit_clicked();
    wait_for_model(model)?;
    ensure(
        list_value(model, keys::TAKE_OVER)?.is_empty()
            && all_text(card).contains("Your own bar is back"),
        &format!("  after Switch Back to My Own Bar: {:?}", all_text(card)),
    )?;
    ensure(
        untouched()? == before,
        "  the user's sway config changed while their bar was taken over and given back",
    )?;
    window.destroy();
    Ok(())
}

/// The controller named `name` of `widget`.
fn controller_named<T: IsA<gtk::EventController> + IsA<gtk::glib::Object>>(
    widget: &gtk::Widget,
    name: &str,
) -> Result<T, String> {
    widget
        .observe_controllers()
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|controller| controller.downcast::<T>().ok())
        .find(|controller| controller.name().as_deref() == Some(name))
        .ok_or_else(|| format!("  no controller {name}"))
}

/// A bar wider than the preview pans sideways: a drag beyond a few pixels moves it (a smaller
/// one stays a click), a vertical wheel scrolls it across, and the scrollbar shows at the
/// bottom edge of the preview, below the bar.
fn check_bar_pan() -> Outcome {
    let items: Vec<String> = (0..30)
        .map(|at| format!("\"custom/item{at:02}\""))
        .collect();
    let stored = format!(
        "version = 1\n\n[bar]\nbackend = \"waybar\"\nmodules_right = [{}]\n",
        items.join(", ")
    );
    let fixture = fixture(SWAY, &stored, USERS, true)?;
    let window = open(&fixture, Page::Bar)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let bar = all::<gtk::Box>(&content)
        .into_iter()
        .find(|widget| widget.has_css_class("st-bar-picture"))
        .ok_or_else(|| "  no waybar window in the preview".to_owned())?;
    let scroller = bar
        .ancestor(gtk::ScrolledWindow::static_type())
        .and_downcast::<gtk::ScrolledWindow>()
        .ok_or_else(|| "  the preview doesn't scroll".to_owned())?;
    settle(&window)?;
    let adjustment = scroller.hadjustment();
    ensure(
        adjustment.upper() > adjustment.page_size() + 1.0,
        "  30 items fit the preview",
    )?;
    let scrollbar = all::<gtk::Scrollbar>(&content)
        .into_iter()
        .find(|widget| widget.has_css_class("st-bar-scrollbar"))
        .ok_or_else(|| "  no scrollbar".to_owned())?;
    ensure(
        scrollbar.is_visible()
            && scrollbar.next_sibling().is_none()
            && scroller.hscrollbar_policy() == gtk::PolicyType::External,
        "  the scrollbar isn't the preview's own at its bottom edge",
    )?;
    shoot(window.upcast_ref(), "bar-preview-scroll-start")?;
    // The item with the keyboard focus scrolls into view, also when Tab gets there.
    let chips: Vec<gtk::Widget> = all::<gtk::Widget>(bar.upcast_ref())
        .into_iter()
        .filter(|widget| widget.css_name() == "widget" && widget.is_focusable())
        .collect();
    let last = chips.last().ok_or_else(|| "  no items".to_owned())?;
    ensure(
        chips.len() >= 30,
        &format!("  {} items to focus", chips.len()),
    )?;
    let grabbed = chips[chips.len() - 2].grab_focus();
    settle(&window)?;
    window.child_focus(gtk::DirectionType::TabForward);
    settle(&window)?;
    let now = gtk::prelude::GtkWindowExt::focus(&window).map(|widget| {
        format!(
            "{} {:?} {}",
            widget.css_name(),
            widget.css_classes(),
            widget.widget_name()
        )
    });
    ensure(
        last.is_focus(),
        &format!("  Tab doesn't go from item to item (grabbed {grabbed}, now {now:?})"),
    )?;
    ensure(
        adjustment.value() + adjustment.page_size() >= adjustment.upper() - 1.0,
        &format!(
            "  the last item has the focus, but the bar shows {}..{} of {}",
            adjustment.value(),
            adjustment.value() + adjustment.page_size(),
            adjustment.upper()
        ),
    )?;
    shoot(window.upcast_ref(), "bar-preview-scroll-end")?;
    adjustment.set_value(0.0);
    let scroller: &gtk::Widget = scroller.upcast_ref();
    // A vertical wheel scrolls across.
    let wheel = controller_named::<gtk::EventControllerScroll>(scroller, "st-bar-wheel")?;
    let _: bool = wheel.emit_by_name("scroll", &[&0.0f64, &2.0f64]);
    let after_wheel = adjustment.value();
    ensure(after_wheel > 0.0, "  the wheel didn't move the bar")?;
    // A small move stays a click; a drag pans.
    let drag = controller_named::<gtk::GestureDrag>(scroller, "st-bar-pan")?;
    drag.emit_by_name::<()>("drag-begin", &[&100.0f64, &10.0f64]);
    drag.emit_by_name::<()>("drag-update", &[&-3.0f64, &0.0f64]);
    ensure(
        (adjustment.value() - after_wheel).abs() < f64::EPSILON,
        "  a press that hardly moved panned the bar",
    )?;
    drag.emit_by_name::<()>("drag-update", &[&-80.0f64, &0.0f64]);
    ensure(
        (adjustment.value() - (after_wheel + 80.0)).abs() < 1.0,
        &format!(
            "  a drag of 80 px moved the bar from {after_wheel} to {}",
            adjustment.value()
        ),
    )?;
    window.destroy();
    Ok(())
}

/// The cards of the looks show the user's own items as the preview builds them (waybar's
/// widgets, names and lanes), each with a stylesheet of its own; on SwayFX Glass too.
fn check_bar_looks() -> Outcome {
    let fixture = fixture(
        SWAYFX,
        "version = 1\n\n[bar]\nbackend = \"waybar\"\n",
        USERS,
        true,
    )?;
    let window = open(&fixture, Page::Bar)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let cards = all::<gtk::Button>(&content)
        .into_iter()
        .filter(|button| button.has_css_class("st-bar-preset"))
        .collect::<Vec<_>>();
    ensure(
        cards.iter().filter(|card| card.is_visible()).count()
            == sway_things_core::bar::style::PRESETS.len(),
        &format!("  {} cards, Glass included?", cards.len()),
    )?;
    let mut classes = Vec::new();
    for card in &cards {
        let bar = all::<gtk::Box>(card.upcast_ref())
            .into_iter()
            .find(|widget| widget.has_css_class("st-bar-picture"))
            .ok_or_else(|| "  a card has no waybar window".to_owned())?;
        let class = bar
            .css_classes()
            .iter()
            .find(|class| class.starts_with("st-wb-look"))
            .map(ToString::to_string)
            .ok_or_else(|| "  a card's bar has no class of its own".to_owned())?;
        classes.push(class);
        let bar: &gtk::Widget = bar.upcast_ref();
        ensure(
            all::<gtk::Box>(bar)
                .iter()
                .any(|lane| lane.has_css_class("modules-left"))
                && named(bar, "workspaces").is_ok_and(|widget| {
                    widget
                        .parent()
                        .is_some_and(|parent| parent.css_name() == "widget")
                }),
            "  a card's bar isn't built as the preview builds it",
        )?;
    }
    classes.sort();
    classes.dedup();
    ensure(classes.len() == cards.len(), "  cards share a stylesheet")?;
    window.destroy();
    Ok(())
}

/// Screenshots of `window` as `name-light` and `name-dark`.
fn shoot_both(window: &Window, name: &str) -> Outcome {
    let settings = gtk::Settings::default().ok_or_else(|| "  no GTK settings".to_owned())?;
    for dark in [false, true] {
        set_dark(&settings, dark);
        settle(window)?;
        let scheme = if dark { "dark" } else { "light" };
        let shot = shoot(window.upcast_ref(), &format!("{name}-{scheme}"));
        if shot.is_err() {
            set_dark(&settings, false);
            return shot;
        }
    }
    set_dark(&settings, false);
    settle(window)
}

/// The card of the looks named `title`.
fn look_card(content: &gtk::Widget, title: &str) -> Result<gtk::Button, String> {
    all::<gtk::Button>(content)
        .into_iter()
        .filter(|button| button.has_css_class("st-bar-preset"))
        .find(|button| texts(button.upcast_ref()).iter().any(|text| text == title))
        .ok_or_else(|| format!("  no look {title:?}"))
}

/// The user's own stylesheet is a look of its own, _Your style_, in use while it is on; any other
/// look turns it off, so that look shows in the preview and the bar, with a note and _Undo_;
/// _Your style_ turns it on again. The look in use is marked unmistakably.
fn check_bar_own_style() -> Outcome {
    use sway_things_core::registry::bar as keys;
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let extra = dir.path().join("style.css");
    std::fs::write(&extra, "#clock { color: rgb(1, 2, 3); }").map_err(|error| error.to_string())?;
    let stored = format!(
        "version = 1\n\n[bar]\nbackend = \"waybar\"\nmodules_center = [\"clock\"]\n\n\
         [bar.style]\ntheme = \"plain\"\nextra_css = {:?}\n",
        extra.to_string_lossy()
    );
    let fixture = fixture(SWAYFX, &stored, USERS, true)?;
    let model = &fixture.model;
    // Alone on the screen, so its screenshots show the page wide.
    for other in gtk::Window::list_toplevels() {
        if let Ok(other) = other.downcast::<gtk::Window>() {
            other.close();
        }
    }
    let window = open(&fixture, Page::Bar)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let preview_clock = || -> Result<gtk::Widget, String> {
        let bar = all::<gtk::Box>(&content)
            .into_iter()
            .find(|widget| widget.has_css_class("st-bar-picture"))
            .ok_or_else(|| "  no preview".to_owned())?;
        named(bar.upcast_ref(), "clock")
    };
    let own = look_card(&content, "Your style")?;
    ensure(
        own.is_visible()
            && own.has_css_class("st-current")
            && texts(own.upcast_ref())
                .iter()
                .any(|text| text == "style.css"),
        "  Your style isn't the look in use, with its file named",
    )?;
    ensure(
        colored(&preview_clock()?, [1, 2, 3]),
        "  the preview doesn't draw the user's stylesheet",
    )?;
    let presets = all::<gtk::Box>(&content)
        .into_iter()
        .find(|widget| widget.has_css_class("st-bar-presets"))
        .ok_or_else(|| "  no looks".to_owned())?;
    scroll_to(&window, &presets)?;
    shoot_both(&window, "looks-selected-own")?;
    // Another look turns it off, and that look shows.
    look_card(&content, "Glass")?.emit_clicked();
    wait_for_model(model)?;
    settle(&window)?;
    ensure(
        value(model, keys::EXTRA_CSS_ON)? == Some(Value::Bool(false))
            && value(model, keys::EXTRA_CSS)?
                == Some(Value::Text(extra.to_string_lossy().into_owned())),
        "  Glass didn't turn the user's stylesheet off, keeping its path",
    )?;
    let glass = look_card(&content, "Glass")?;
    ensure(
        glass.has_css_class("st-current") && !own.has_css_class("st-current"),
        "  Glass isn't marked as the look in use",
    )?;
    ensure(
        !colored(&preview_clock()?, [1, 2, 3]),
        "  the preview still draws the user's stylesheet over the look",
    )?;
    ensure(
        all_text(&content).contains("Your own stylesheet is off now, so this look shows."),
        "  no note that the user's stylesheet is off",
    )?;
    shoot_both(&window, "looks-selected-glass")?;
    // Undo brings it back.
    button_with(&content, "Undo")?.emit_clicked();
    wait_for_model(model)?;
    ensure(
        value(model, keys::EXTRA_CSS_ON)? == Some(Value::Bool(true)),
        "  Undo didn't turn the user's stylesheet on again",
    )?;
    // Simple, then Your style again.
    look_card(&content, "Simple")?.emit_clicked();
    wait_for_model(model)?;
    look_card(&content, "Your style")?.emit_clicked();
    wait_for_model(model)?;
    settle(&window)?;
    ensure(
        value(model, keys::EXTRA_CSS_ON)? == Some(Value::Bool(true))
            && value(model, keys::THEME)? == Some(Value::Choice("plain".to_owned()))
            && colored(&preview_clock()?, [1, 2, 3]),
        "  Your style didn't bring the user's stylesheet back",
    )?;
    window.destroy();
    Ok(())
}

/// The first controller of type `T` of `widget`.
fn controller_of<T: IsA<gtk::EventController> + IsA<gtk::glib::Object>>(
    widget: &gtk::Widget,
) -> Result<T, String> {
    widget
        .observe_controllers()
        .into_iter()
        .filter_map(Result::ok)
        .find_map(|controller| controller.downcast::<T>().ok())
        .ok_or_else(|| "  no such controller".to_owned())
}

/// The row named `name` in `lane`, at any depth.
fn row_named(lane: &gtk::ListBox, name: &str) -> Result<gtk::ListBoxRow, String> {
    rows_of(lane)
        .into_iter()
        .find(|row| row.widget_name() == name)
        .ok_or_else(|| format!("  no row {name}"))
}

/// Clicks the entry `label` of the menu button `menu`.
fn menu_entry(menu: &gtk::MenuButton, label: &str) -> Outcome {
    let popover = menu
        .popover()
        .ok_or_else(|| "  the menu has no popover".to_owned())?;
    button_with(popover.upcast_ref(), label)?.emit_clicked();
    Ok(())
}

/// The members of group `name`, as `bar.jsonc` holds them.
fn members(model: &Model, name: &str) -> Result<Vec<String>, String> {
    let modules = model
        .bar_modules()
        .and_then(Result::ok)
        .ok_or_else(|| "  bar.jsonc can't be read".to_owned())?;
    Ok(sway_things_core::bar::groups::members_of(&modules, name))
}

/// Groups as a newcomer meets them: a tip; an item dragged onto the middle of another shows
/// "Drop to group" and makes one group of both, whose name is edited at once; the group is a
/// card with its items inside; its menu makes it a drawer, which marks the first item _Always
/// shown_; Alt+Up moves an item into the group, the item's menu makes it the first and moves it
/// out; pointing at the group marks its pill in the preview and opens the drawer there;
/// _Ungroup_ puts the items back. With screenshots of each.
#[expect(clippy::too_many_lines, reason = "one user's way through groups")]
fn check_bar_groups() -> Outcome {
    use gtk::gdk::{Key, ModifierType};
    use sway_things_core::registry::bar as keys;
    let fixture = fixture(
        SWAY,
        "version = 1\n\n[bar]\nbackend = \"waybar\"\nmodules_right = [\"pulseaudio\", \
         \"backlight\", \"battery\", \"clock\"]\nspacing = 0\n\n[bar.style]\ntheme = \"custom\"\n\
         shape = \"pills\"\npadding = 10\nradius = 999\nopacity = 0.45\nbackground = \"#121216\"\n\
         text = \"#d4d4d8\"\nhover = \"#00000059\"\nfocused = \"#a74ed0\"\n",
        USERS,
        true,
    )?;
    let model = &fixture.model;
    let window = open(&fixture, Page::Bar)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    ensure(
        texts(&content)
            .iter()
            .any(|text| text.starts_with("Tip: drag an item onto another")),
        "  no tip about groups",
    )?;
    let lane = lanes(&window)?;
    scroll_to(&window, &lane[2])?;
    let lane_root = lane_box(&lane[2])?;
    let drop = controller_of::<gtk::DropTarget>(&lane_root)?;
    let target = row_named(&lane[2], "pulseaudio")?;
    let bounds = target
        .compute_bounds(&lane_root)
        .ok_or_else(|| "  the row has no place".to_owned())?;
    let (x, y) = (
        f64::from(bounds.x() + bounds.width() / 2.0),
        f64::from(bounds.y() + bounds.height() / 2.0),
    );
    // Over the middle of an item, the drop says it makes a group.
    let _: gtk::gdk::DragAction = drop.emit_by_name("motion", &[&x, &y]);
    let hint = all::<gtk::Label>(target.upcast_ref())
        .into_iter()
        .find(|label| label.text() == "Drop to group")
        .ok_or_else(|| "  no hint on the row".to_owned())?;
    ensure(
        target.has_css_class("st-drop-combine") && hint.is_visible(),
        "  the middle of an item doesn't offer to group",
    )?;
    let payload = gtk::glib::value::BoxedValue("2\n1".to_value());
    let dropped: bool = drop.emit_by_name("drop", &[&payload, &x, &y]);
    ensure(dropped, "  the drop wasn't taken")?;
    wait_for_model(model)?;
    settle(&window)?;
    let right = list_value(model, keys::MODULES_RIGHT)?;
    ensure(
        right.len() == 3
            && right[0].starts_with("group/")
            && right[1..] == strings(&["battery", "clock"])[..],
        &format!("  the drop gave {right:?}"),
    )?;
    ensure(
        members(model, &right[0])? == strings(&["pulseaudio", "backlight"]),
        "  the group doesn't hold the two items",
    )?;
    // Its name is edited at once.
    let editable = all::<gtk::EditableLabel>(&content)
        .into_iter()
        .find(gtk::EditableLabel::is_editing)
        .ok_or_else(|| "  the new group's name isn't being edited".to_owned())?;
    editable.set_text("Audio");
    editable.stop_editing(true);
    wait_for_model(model)?;
    settle(&window)?;
    ensure(
        list_value(model, keys::MODULES_RIGHT)?
            .first()
            .map(String::as_str)
            == Some("group/audio"),
        "  renaming didn't name the group",
    )?;
    let card = row_named(&lane[2], "group/audio")?;
    ensure(
        texts(card.upcast_ref())
            .iter()
            .any(|text| text == "2 items"),
        "  the card doesn't count its items",
    )?;
    // The group's menu makes it a drawer.
    let menu = all::<gtk::MenuButton>(card.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  the card has no menu".to_owned())?;
    menu_entry(&menu, "Show the Others Only on Hover")?;
    wait_for_model(model)?;
    settle(&window)?;
    let modules = model
        .bar_modules()
        .and_then(Result::ok)
        .ok_or_else(|| "  bar.jsonc can't be read".to_owned())?;
    ensure(
        modules
            .definition("group/audio")
            .as_ref()
            .and_then(sway_things_core::bar::groups::drawer)
            .is_some(),
        "  the menu didn't make a drawer",
    )?;
    let card = row_named(&lane[2], "group/audio")?;
    let said = texts(card.upcast_ref());
    ensure(
        said.iter().any(|text| text == "Always shown")
            && said.iter().any(|text| text == "Shown on hover"),
        &format!("  the drawer's members aren't marked: {said:?}"),
    )?;
    // Alt+Up moves the item below the group into it, at its end.
    press(
        &row_named(&lane[2], "battery")?,
        Key::Up,
        ModifierType::ALT_MASK,
    )?;
    wait_for_model(model)?;
    settle(&window)?;
    ensure(
        members(model, "group/audio")? == strings(&["pulseaudio", "backlight", "battery"]),
        &format!(
            "  Alt+Up gave the members {:?}",
            members(model, "group/audio")?
        ),
    )?;
    let battery = row_named(&lane[2], "battery")?;
    ensure(
        battery.is_focus(),
        "  the focus didn't follow the item into the group",
    )?;
    // Its menu makes it the one always shown, then moves it out again.
    let item_menu = all::<gtk::MenuButton>(battery.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  a member has no menu".to_owned())?;
    menu_entry(&item_menu, "Make Always Shown")?;
    wait_for_model(model)?;
    settle(&window)?;
    ensure(
        members(model, "group/audio")?.first().map(String::as_str) == Some("battery"),
        "  Make Always Shown didn't put the item first",
    )?;
    scroll_to(&window, &lane[2])?;
    shoot_both(&window, "groups-ux-lanes")?;
    // The same hint over the clock, for its picture.
    let clock = row_named(&lane[2], "clock")?;
    let bounds = clock
        .compute_bounds(&lane_root)
        .ok_or_else(|| "  the row has no place".to_owned())?;
    let _: gtk::gdk::DragAction = drop.emit_by_name(
        "motion",
        &[
            &f64::from(bounds.x() + bounds.width() / 2.0),
            &f64::from(bounds.y() + bounds.height() / 2.0),
        ],
    );
    settle(&window)?;
    shoot(window.upcast_ref(), "groups-ux-drop-light")?;
    drop.emit_by_name::<()>("leave", &[]);
    let card = row_named(&lane[2], "group/audio")?;
    let menu = all::<gtk::MenuButton>(card.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  the card has no menu".to_owned())?;
    // A popover is a surface of its own: its entries are shown in a window for the picture.
    let popover = menu
        .popover()
        .ok_or_else(|| "  the menu has no popover".to_owned())?;
    let entries = popover
        .child()
        .ok_or_else(|| "  the menu is empty".to_owned())?;
    popover.set_child(None::<&gtk::Widget>);
    let shown = gtk::Window::new();
    shown.set_title(Some("Group menu"));
    shown.set_child(Some(&entries));
    shown.present();
    settle(&shown)?;
    shoot(&shown, "groups-ux-menu-light")?;
    shown.set_child(None::<&gtk::Widget>);
    shown.destroy();
    popover.set_child(Some(&entries));
    ensure(
        texts(&entries)
            .iter()
            .any(|text| text == "Open Toward the Left")
            && texts(&entries).iter().any(|text| text == "Ungroup"),
        "  the group's menu lacks the drawer's direction or Ungroup",
    )?;
    let battery = row_named(&lane[2], "battery")?;
    let item_menu = all::<gtk::MenuButton>(battery.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  a member has no menu".to_owned())?;
    menu_entry(&item_menu, "Move Out of the Group")?;
    wait_for_model(model)?;
    settle(&window)?;
    ensure(
        list_value(model, keys::MODULES_RIGHT)? == strings(&["group/audio", "battery", "clock"])
            && members(model, "group/audio")? == strings(&["pulseaudio", "backlight"]),
        "  Move Out of the Group didn't put the item after the group",
    )?;
    // Pointing at the group marks its pill in the preview and opens its drawer there.
    let card = row_named(&lane[2], "group/audio")?;
    let motion = controller_of::<gtk::EventControllerMotion>(card.upcast_ref())?;
    motion.emit_by_name::<()>("enter", &[&1.0f64, &1.0f64]);
    let bar = all::<gtk::Box>(&content)
        .into_iter()
        .find(|widget| {
            widget
                .css_classes()
                .iter()
                .any(|class| class.starts_with("st-wb-"))
        })
        .ok_or_else(|| "  no waybar window in the preview".to_owned())?;
    let pill = named(bar.upcast_ref(), "audio")?;
    let outer = pill
        .parent()
        .ok_or_else(|| "  the group's pill has no `widget`".to_owned())?;
    let drawer = all::<gtk::Revealer>(&pill)
        .into_iter()
        .next()
        .ok_or_else(|| "  the drawer has no revealer in the preview".to_owned())?;
    ensure(
        outer.has_css_class("st-bar-marked") && drawer.reveals_child(),
        "  pointing at the group doesn't mark its pill and open its drawer",
    )?;
    let page = page_view(&window).ok_or_else(|| "  no page".to_owned())?;
    let viewport = find::<gtk::Viewport>(page.upcast_ref());
    if let Some(viewport) = viewport {
        viewport.scroll_to(&bar, None);
    }
    run_until(Duration::from_millis(600), || false);
    shoot_both(&window, "groups-ux-preview")?;
    motion.emit_by_name::<()>("leave", &[]);
    ensure(
        !outer.has_css_class("st-bar-marked"),
        "  the mark stays after the pointer left",
    )?;
    // A click on the group's pill opens its settings: its items and the drawer.
    let pill_click = controller_of::<gtk::GestureClick>(&outer)?;
    pill_click.emit_by_name::<()>("released", &[&1i32, &5.0f64, &5.0f64]);
    let dialog = window_titled("Audio")?;
    let said = texts(dialog.upcast_ref());
    ensure(
        said.iter()
            .any(|text| text == "Show the others only on hover")
            && said.iter().any(|text| text.contains("Volume")),
        &format!("  the group's settings say {said:?}"),
    )?;
    shoot(&dialog, "groups-ux-settings-light")?;
    dialog.close();
    // Ungroup puts the items back into the lane.
    let card = row_named(&lane[2], "group/audio")?;
    let menu = all::<gtk::MenuButton>(card.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  the card has no menu".to_owned())?;
    menu_entry(&menu, "Ungroup")?;
    wait_for_model(model)?;
    ensure(
        list_value(model, keys::MODULES_RIGHT)?
            == strings(&["pulseaudio", "backlight", "battery", "clock"]),
        "  Ungroup didn't put the items back",
    )?;
    window.destroy();
    Ok(())
}
