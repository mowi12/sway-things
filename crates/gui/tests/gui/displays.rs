// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Displays page and the Workspaces map against a stand-in that reports outputs.

use super::*;
/// A desk: two monitors side by side, the left one with the focus, and a laptop panel that is
/// off.
pub(crate) const DESK_OUTPUTS: &str = r#"[
  {"name": "DP-4", "active": true, "primary": false, "make": "Samsung", "model": "LF27T850",
   "serial": "A1", "scale": 1.0, "transform": "normal", "focused": true,
   "modes": [{"width": 2560, "height": 1440, "refresh": 59951},
             {"width": 1920, "height": 1080, "refresh": 60000}],
   "current_mode": {"width": 2560, "height": 1440, "refresh": 59951},
   "rect": {"x": 0, "y": 0, "width": 2560, "height": 1440}},
  {"name": "DP-5", "active": true, "primary": false, "make": "Samsung", "model": "LF27T850",
   "serial": "B2", "scale": 1.0, "transform": "normal",
   "modes": [{"width": 2560, "height": 1440, "refresh": 59951}],
   "current_mode": {"width": 2560, "height": 1440, "refresh": 59951},
   "rect": {"x": 2560, "y": 0, "width": 2560, "height": 1440}},
  {"name": "eDP-1", "active": false, "primary": false, "make": "BOE", "model": "0x0BCA",
   "serial": "C3", "modes": [{"width": 1920, "height": 1080, "refresh": 60000}],
   "rect": {"x": 0, "y": 0, "width": 0, "height": 0}}
]"#;
pub(crate) const LEFT: &str = "Samsung LF27T850 A1";
pub(crate) const RIGHT: &str = "Samsung LF27T850 B2";
pub(crate) const PANEL: &str = "BOE 0x0BCA C3";

/// A model in the session of `sway`, once it has read the outputs.
pub(crate) fn displays_fixture(sway: &FakeSway, stored: &str) -> Result<Fixture, String> {
    let fixture = fixture_in(SWAY, stored, USERS, true, Some(sway))?;
    let known = run_until(Duration::from_secs(5), || fixture.model.outputs_known());
    ensure(known, "  the outputs weren't read within 5 s")?;
    Ok(fixture)
}

/// The arrangement and the output picker of the Displays page of `window`.
pub(crate) fn displays(window: &Window) -> Result<(Arrangement, OutputPicker), String> {
    let view = page_view(window).ok_or_else(|| "  no page".to_owned())?;
    let arrangement = view
        .arrangement()
        .ok_or_else(|| "  no arrangement".to_owned())?;
    let picker = view
        .output_picker()
        .ok_or_else(|| "  no output picker".to_owned())?;
    Ok((arrangement, picker))
}

pub(crate) fn check_displays() -> Vec<(String, Outcome)> {
    vec![
        (
            "workspaces: the rule's placement shows, a click makes it fixed".to_owned(),
            check_workspace_map(),
        ),
        (
            "displays: the outputs, the one with the focus picked".to_owned(),
            check_display_outputs(),
        ),
        (
            "displays: picking on the canvas points the rows at the output".to_owned(),
            check_display_pick(),
        ),
        (
            "displays: a moved output is settled, stored and undone".to_owned(),
            check_display_move(),
        ),
        (
            "displays: a gap is shown and closed".to_owned(),
            check_display_gap(),
        ),
        ("displays: plugged in".to_owned(), check_display_plugged()),
        (
            "displays: a scale is sent once, when the control stays still".to_owned(),
            check_display_scale_waits(),
        ),
        (
            "displays: the last output can't be switched off".to_owned(),
            check_display_last(),
        ),
        (
            "displays: without a session".to_owned(),
            check_display_no_session(),
        ),
        (
            "displays: identify shows the numbers on the displays".to_owned(),
            check_display_identify(),
        ),
        (
            "contrast: displays with outputs (light)".to_owned(),
            check_display_contrast(false),
        ),
        (
            "contrast: displays with outputs (dark)".to_owned(),
            check_display_contrast(true),
        ),
    ]
}

/// The Displays page with the canvas, a tile picked and one that is off, checked like the
/// pages.
pub(crate) fn check_display_contrast(dark: bool) -> Outcome {
    let settings = gtk::Settings::default().ok_or_else(|| "  no GTK settings".to_owned())?;
    set_dark(&settings, dark);
    let sway = with_outputs(&[DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let scheme = if dark { "dark" } else { "light" };
    let screenshots = std::env::var_os("SWAY_THINGS_SCREENSHOTS").map(PathBuf::from);
    let outcome = check_page(&window, Page::Displays, screenshots.as_deref(), scheme);
    window.destroy();
    set_dark(&settings, false);
    outcome
}

/// The canvas shows the outputs that are on at their places and the one that is off, the picker
/// lists them all, and the output with the focus is picked for the rows.
pub(crate) fn check_display_outputs() -> Outcome {
    let sway = with_outputs(&[DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, picker) = displays(&window)?;
    let canvas = arrangement.canvas();
    let tiles: Vec<String> = canvas.tiles().into_iter().map(|(id, _)| id).collect();
    ensure(
        tiles == [LEFT, RIGHT, PANEL],
        &format!("  the canvas shows {tiles:?}"),
    )?;
    let layout: Vec<String> = canvas.layout().into_iter().map(|(id, _)| id).collect();
    ensure(
        layout == [LEFT, RIGHT],
        &format!("  the layout is {layout:?}"),
    )?;
    ensure(
        picker.labels()
            == [
                "All displays",
                "1  Samsung LF27T850 \u{b7} DP-4",
                "2  Samsung LF27T850 \u{b7} DP-5",
                "3  BOE 0x0BCA \u{b7} eDP-1 (off)",
            ],
        &format!("  the picker offers {:?}", picker.labels()),
    )?;
    ensure(
        picker.selected().as_deref() == Some(LEFT),
        &format!("  the picker picked {:?}", picker.selected()),
    )?;
    let scale = row(&window, "output.scale")?;
    ensure(
        scale.device().as_deref() == Some(LEFT),
        "the rows aren't for the output with the focus",
    )?;
    ensure(
        scale.note().as_deref() == Some("Chosen by sway"),
        &format!("  the scale row's note is {:?}", scale.note()),
    )?;
    ensure(!arrangement.warns_of_gaps(), "a gap is reported")?;
    window.destroy();
    Ok(())
}

/// A tile clicked on the canvas picks its output in the picker, points the rows at it and
/// offers its modes.
pub(crate) fn check_display_pick() -> Outcome {
    let sway = with_outputs(&[DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, picker) = displays(&window)?;
    let (_, right) = arrangement
        .canvas()
        .tiles()
        .into_iter()
        .find(|(id, _)| id == RIGHT)
        .ok_or_else(|| "  no tile for the right monitor".to_owned())?;
    right.set_active(true);
    settle(&window)?;
    ensure(
        picker.selected().as_deref() == Some(RIGHT),
        &format!("  the picker picked {:?}", picker.selected()),
    )?;
    let mode = row(&window, "output.mode")?;
    ensure(
        mode.device().as_deref() == Some(RIGHT),
        "the mode row isn't for the picked output",
    )?;
    let dropdowns: Vec<gtk::DropDown> = mode
        .controls()
        .into_iter()
        .filter_map(|control| control.downcast::<gtk::DropDown>().ok())
        .collect();
    let offered = |dropdown: &gtk::DropDown| -> Vec<String> {
        dropdown
            .model()
            .map(|model| {
                (0..model.n_items())
                    .filter_map(|index| model.item(index).and_downcast::<gtk::StringObject>())
                    .map(|item| item.string().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    let [resolution, rate] = dropdowns.as_slice() else {
        return Err(format!("  the mode row has {} drop-downs", dropdowns.len()));
    };
    ensure(
        offered(resolution) == ["Automatic", "2560 \u{d7} 1440"],
        &format!("  the mode row offers {:?}", offered(resolution)),
    )?;
    ensure(
        offered(rate) == ["59.951 Hz"],
        &format!("  the refresh rates are {:?}", offered(rate)),
    )?;
    // The picker back to every output: no tile is pressed.
    picker.dropdown().set_selected(0);
    settle(&window)?;
    ensure(
        arrangement
            .canvas()
            .tiles()
            .iter()
            .all(|(_, tile)| !tile.is_active()),
        "a tile stays pressed for all displays",
    )?;
    ensure(
        row(&window, "output.scale")?.device().is_none(),
        "the rows aren't for all displays",
    )?;
    // What is only set for one display at a time isn't offered for all of them.
    for key in [
        "output.position",
        "output.mode",
        "output.enabled",
        "output.transform",
    ] {
        let hidden = row(&window, key)?
            .parent()
            .is_some_and(|item| !item.is_visible());
        ensure(hidden, &format!("  {key} is offered for all displays"))?;
    }
    // Every output stays picked when the model changes.
    fixture.model.emit_by_name::<()>("changed", &[]);
    settle(&window)?;
    ensure(
        picker.selected().is_none(),
        &format!("  after a change the picker picked {:?}", picker.selected()),
    )?;
    window.destroy();
    Ok(())
}

/// The right monitor dropped below the left one, overlapping it, is settled below it; both
/// positions are stored and sent; Ctrl+Z puts them back.
/// Steps of a display's scale in a row are sent as one change once the control stays still, not
/// one by one: the display changes size under the pointer with each one.
pub(crate) fn check_display_scale_waits() -> Outcome {
    let sway = with_outputs(&[DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let scale = row(&window, "output.scale")?;
    let spin = scale
        .controls()
        .into_iter()
        .find_map(|widget| widget.downcast::<gtk::SpinButton>().ok())
        .ok_or_else(|| "  no number field".to_owned())?;
    let sent = || {
        sway.commands()
            .into_iter()
            .filter(|command| command.contains(" scale "))
            .collect::<Vec<_>>()
    };
    for value in [1.05, 1.1, 1.15] {
        spin.set_value(value);
    }
    ensure(sent().is_empty(), &format!("  sent at once: {:?}", sent()))?;
    let done = run_until(Duration::from_secs(3), || !sent().is_empty());
    ensure(done, "  nothing sent within 3 s")?;
    wait_for_model(&fixture.model)?;
    ensure(
        sent() == [r#"output "Samsung LF27T850 A1" scale 1.15"#],
        &format!("  sent {:?}", sent()),
    )?;
    window.destroy();
    Ok(())
}

pub(crate) fn check_display_move() -> Outcome {
    let sway = with_outputs(&[DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, _) = displays(&window)?;
    let canvas = arrangement.canvas();
    canvas.move_output(RIGHT, 300, 1000);
    wait_for_model(&fixture.model)?;
    let layout = canvas.layout();
    let place = |id: &str| {
        layout
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, rect)| (rect.x, rect.y))
    };
    ensure(
        place(LEFT) == Some((0, 0)) && place(RIGHT) == Some((300, 1440)),
        &format!("  the layout is {layout:?}"),
    )?;
    let stored = |id: &str| {
        Registry::builtin()
            .get("output.position")
            .and_then(|setting| fixture.model.view(setting, Some(id)))
            .map(|view| view.value)
    };
    ensure(
        stored(RIGHT) == Some(Value::Text("300,1440".to_owned()))
            && stored(LEFT) == Some(Value::Text("0,0".to_owned())),
        &format!("  stored {:?} and {:?}", stored(LEFT), stored(RIGHT)),
    )?;
    let commands = sway.commands();
    ensure(
        commands
            .iter()
            .any(|command| command == r#"output "Samsung LF27T850 B2" pos 300 1440"#),
        &format!("  sent {commands:?}"),
    )?;
    ensure(canvas.undo(), "nothing to undo")?;
    wait_for_model(&fixture.model)?;
    ensure(
        stored(RIGHT) == Some(Value::Text("2560,0".to_owned())),
        &format!("  after undo, stored {:?}", stored(RIGHT)),
    )?;
    ensure(!canvas.undo(), "more to undo than was done")?;
    window.destroy();
    Ok(())
}

/// Two monitors that don't touch are reported, and "Arrange Side by Side" puts them together.
pub(crate) fn check_display_gap() -> Outcome {
    let apart = DESK_OUTPUTS.replace(
        r#""rect": {"x": 2560, "y": 0, "width": 2560, "height": 1440}"#,
        r#""rect": {"x": 3000, "y": 0, "width": 2560, "height": 1440}"#,
    );
    let sway = with_outputs(&[&apart]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, _) = displays(&window)?;
    ensure(arrangement.warns_of_gaps(), "the gap isn't reported")?;
    let button = buttons_labeled(&window, "Arrange Side by Side")
        .into_iter()
        .next()
        .ok_or_else(|| "  no button to arrange".to_owned())?;
    button.emit_clicked();
    wait_for_model(&fixture.model)?;
    let layout = arrangement.canvas().layout();
    let right = layout
        .iter()
        .find(|(id, _)| id == RIGHT)
        .map(|(_, rect)| rect.x);
    ensure(right == Some(2560), &format!("  the layout is {layout:?}"))?;
    window.destroy();
    Ok(())
}

/// A monitor that is plugged in shows up on the canvas and in the picker.
pub(crate) fn check_display_plugged() -> Outcome {
    let one = r#"[{"name": "DP-4", "active": true, "primary": false, "make": "Samsung",
        "model": "LF27T850", "serial": "A1", "scale": 1.0, "focused": true, "modes": [],
        "rect": {"x": 0, "y": 0, "width": 2560, "height": 1440}}]"#;
    let sway = with_outputs(&[one, DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, picker) = displays(&window)?;
    let plugged = run_until(Duration::from_secs(5), || {
        arrangement.canvas().tiles().len() == 3 && picker.labels().len() == 4
    });
    ensure(
        plugged,
        &format!("  the picker offers {:?}", picker.labels()),
    )?;
    ensure(
        picker.selected().as_deref() == Some(LEFT),
        "the picked output changed with the hotplug",
    )?;
    window.destroy();
    Ok(())
}

/// Switching off the only output that is on is refused, and the row says why.
pub(crate) fn check_display_last() -> Outcome {
    let single = DESK_OUTPUTS.replace(
        r#""name": "DP-5", "active": true"#,
        r#""name": "DP-5", "active": false"#,
    );
    let sway = with_outputs(&[&single]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let enabled = row(&window, "output.enabled")?;
    let switch = enabled
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Switch>().ok())
        .ok_or_else(|| "  the row has no switch".to_owned())?;
    switch.set_active(false);
    let refused = run_until(Duration::from_secs(5), || {
        enabled
            .error()
            .is_some_and(|error| error.contains("last display"))
    });
    ensure(refused, &format!("  the row says {:?}", enabled.error()))?;
    ensure(
        fixture.model.pending_change().is_none(),
        "a change waits to be kept",
    )?;
    window.destroy();
    Ok(())
}

/// Without a session the canvas gives way to a hint, and the rows are still there.
pub(crate) fn check_display_no_session() -> Outcome {
    let fixture = fixture(SWAY, "version = 1\n", USERS, true)?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, _) = displays(&window)?;
    ensure(
        !arrangement.canvas().is_visible(),
        "the canvas shows without a session",
    )?;
    row(&window, "output.scale")?;
    window.destroy();
    Ok(())
}

/// "Identify" shows the number of an output on the display GDK knows by its name, on the overlay
/// layer, and takes it away again; the tiles are outlined meanwhile. The stand-in names its output
/// like the headless sway's own, so GDK has a monitor for it. Without the layer shell (GTK's
/// Broadway backend of the screenshots), the button can't be pressed.
pub(crate) fn check_display_identify() -> Outcome {
    let headless = DESK_OUTPUTS.replacen(r#""name": "DP-4""#, r#""name": "HEADLESS-1""#, 1);
    let sway = with_outputs(&[&headless]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, _) = displays(&window)?;
    if !gtk4_layer_shell::is_supported() {
        ensure(
            !arrangement.identify_button().is_sensitive(),
            "Identify can be pressed without the layer shell",
        )?;
        window.destroy();
        return Ok(());
    }
    ensure(
        arrangement.identify_button().is_sensitive(),
        "Identify can't be pressed on sway",
    )?;
    let numbers = || {
        gtk::Window::list_toplevels()
            .into_iter()
            .filter(|window| window.has_css_class("st-identify") && window.is_visible())
            .count()
    };
    let shown = arrangement.identify();
    ensure(shown == 1, &format!("  the number is on {shown} displays"))?;
    ensure(
        arrangement.identify() == 0 && !arrangement.identify_button().is_sensitive(),
        "Identify can be pressed again while the numbers are shown",
    )?;
    ensure(
        run_until(Duration::from_secs(2), || numbers() == 1),
        "no window with the number",
    )?;
    ensure(
        arrangement.canvas().has_css_class("st-identifying"),
        "the tiles aren't outlined",
    )?;
    ensure(
        run_until(Duration::from_secs(6), || numbers() == 0),
        "the number stays",
    )?;
    ensure(
        !arrangement.canvas().has_css_class("st-identifying"),
        "the tiles stay outlined",
    )?;
    ensure(
        arrangement.identify_button().is_sensitive(),
        "Identify can't be pressed again afterwards",
    )?;
    window.destroy();
    Ok(())
}

/// The numbers on each card of the Workspaces page's map, by the card's title.
pub(crate) fn workspace_cards(window: &Window) -> Result<Vec<(String, Vec<String>)>, String> {
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    Ok(all::<gtk::Box>(&content)
        .into_iter()
        .filter(|card| card.has_css_class("st-workspace-card") && card.is_visible())
        .map(|card| {
            let title = card
                .first_child()
                .and_downcast::<gtk::Label>()
                .map(|label| label.text().to_string())
                .unwrap_or_default();
            let numbers = all::<gtk::Button>(card.upcast_ref())
                .into_iter()
                .filter_map(|button| button.label().map(|label| label.to_string()))
                .collect();
            (title, numbers)
        })
        .collect())
}

/// Round robin on the two desk displays (the panel is off): the odd numbers on the left, the
/// even ones on the right. Clicking 1 sends it to the right and makes the placement fixed; with
/// sway's own placement there is no map.
pub(crate) fn check_workspace_map() -> Outcome {
    let sway = with_outputs(&[DESK_OUTPUTS]);
    let fixture = displays_fixture(
        &sway,
        "version = 1\n\n[workspaces]\nplacement = \"round_robin\"\ncount = 6\n",
    )?;
    let window = open(&fixture, Page::Workspaces)?;
    let numbers = |list: &[&str]| list.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
    let cards = workspace_cards(&window)?;
    ensure(
        cards
            == [
                ("Samsung \u{b7} DP-4".to_owned(), numbers(&["1", "3", "5"])),
                ("Samsung \u{b7} DP-5".to_owned(), numbers(&["2", "4", "6"])),
            ],
        &format!("  the cards are {cards:?}"),
    )?;
    let one = all::<gtk::Button>(window.upcast_ref())
        .into_iter()
        .find(|button| {
            button.label().as_deref() == Some("1")
                && button
                    .ancestor(gtk::Box::static_type())
                    .is_some_and(|parent| parent.is_visible())
                && button.ancestor(gtk::FlowBox::static_type()).is_some()
        })
        .ok_or_else(|| "  no button 1".to_owned())?;
    one.grab_focus();
    one.emit_clicked();
    wait_for_model(&fixture.model)?;
    let placement = Registry::builtin()
        .get("workspaces.placement")
        .and_then(|setting| fixture.model.view(setting, None))
        .map(|view| view.value);
    ensure(
        placement == Some(Value::Choice("fixed".to_owned())),
        &format!("  the placement is {placement:?}"),
    )?;
    let cards = run_until(Duration::from_secs(5), || {
        workspace_cards(&window).is_ok_and(|cards| {
            cards
                .get(1)
                .is_some_and(|(_, numbers)| numbers.contains(&"1".to_owned()))
        })
    });
    ensure(cards, "  1 didn't move to the right")?;
    // The number keeps the focus on its new card.
    let focus = gtk::prelude::GtkWindowExt::focus(&window);
    ensure(
        focus
            .as_ref()
            .and_then(|focus| focus.downcast_ref::<gtk::Button>())
            .is_some_and(|button| {
                button.label().as_deref() == Some("1") && *button != one && button.root().is_some()
            }),
        &format!("  the focus is on {focus:?}, not on 1 on its new card"),
    )?;
    window.destroy();

    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Workspaces)?;
    ensure(
        workspace_cards(&window)?.is_empty(),
        "  a map shows while sway places the workspaces",
    )?;
    window.destroy();
    Ok(())
}
