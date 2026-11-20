// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Collapsing sections, remembered in `ui.toml`, and links into collapsed ones.

use super::*;
/// The checks of the collapsible sections.
pub(crate) fn check_sections() -> Vec<(String, Outcome)> {
    vec![
        (
            "sections: a header collapses and expands, and it is remembered".to_owned(),
            check_section_collapse(),
        ),
        (
            "sections: collapsed ones come back, unless the preference is off".to_owned(),
            check_sections_restored(),
        ),
        (
            "sections: a deep link opens a collapsed section without storing it".to_owned(),
            check_section_link(),
        ),
    ]
}

/// The id, header and expansion of the section `id` on the open page of `window`.
pub(crate) fn section(window: &Window, id: &str) -> Result<(gtk::Button, bool), String> {
    page_view(window)
        .and_then(|view| {
            view.sections()
                .into_iter()
                .find(|(section, _, _)| section == id)
        })
        .map(|(_, header, expanded)| (header, expanded))
        .ok_or_else(|| format!("  no section {id}"))
}

/// The text of `ui.toml` of `fixture`, if it was written.
pub(crate) fn ui_file(fixture: &Fixture) -> Option<String> {
    std::fs::read_to_string(&fixture.ui).ok()
}

/// Every section starts expanded; a header collapses its section and is saved, and expands it
/// again.
pub(crate) fn check_section_collapse() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let view = page_view(&window).ok_or_else(|| "  no page".to_owned())?;
    ensure(
        view.sections().iter().all(|(_, _, expanded)| *expanded),
        "a section starts collapsed",
    )?;
    let ids: Vec<String> = view.sections().into_iter().map(|(id, _, _)| id).collect();
    ensure(
        ids.first().map(String::as_str) == Some("mouse")
            && ids.last().map(String::as_str) == Some("test-area"),
        &format!("  the sections are {ids:?}"),
    )?;
    let (header, _) = section(&window, "mouse")?;
    let speed = row(&window, "input.pointer.pointer_accel")?;
    // `activate` is what Enter and Space do on the focused header.
    header.activate();
    // A button clicks a moment after it is activated.
    let collapsed = run_until(Duration::from_secs(2), || {
        section(&window, "mouse").is_ok_and(|(_, expanded)| !expanded)
    });
    ensure(collapsed, "Enter on the header didn't collapse the section")?;
    ensure(
        !speed.is_mapped() || !speed.is_drawable(),
        "the rows stay shown",
    )?;
    ensure(
        ui_file(&fixture).is_some_and(|text| text.contains("mouse-touchpad = [\"mouse\"]")),
        &format!("  ui.toml holds {:?}", ui_file(&fixture)),
    )?;
    header.emit_clicked();
    ensure(
        section(&window, "mouse")?.1,
        "a click didn't expand the section",
    )?;
    ensure(
        ui_file(&fixture).is_some_and(|text| !text.contains("mouse-touchpad")),
        &format!("  after expanding, ui.toml holds {:?}", ui_file(&fixture)),
    )?;
    window.destroy();
    Ok(())
}

/// A section collapsed in `ui.toml` starts collapsed; with the preference off, it doesn't and
/// nothing is saved.
pub(crate) fn check_sections_restored() -> Outcome {
    let ui = "version = 1\n[collapsed]\nwindows = [\"gaps\", \"no-such-section\"]\n";
    let fixture = fixture_with_ui(SWAYFX, STORED, USERS, true, None, Some(ui))?;
    let window = open(&fixture, Page::Windows)?;
    ensure(
        !section(&window, "gaps")?.1,
        "the stored section isn't collapsed",
    )?;
    ensure(
        section(&window, "borders")?.1,
        "another section is collapsed",
    )?;
    window.destroy();

    let off = format!("{STORED}\n[app]\nremember_collapsed_sections = false\n");
    let fixture = fixture_with_ui(SWAYFX, &off, USERS, true, None, Some(ui))?;
    let window = open(&fixture, Page::Windows)?;
    ensure(
        section(&window, "gaps")?.1,
        "with the preference off, a section starts collapsed",
    )?;
    let (header, _) = section(&window, "borders")?;
    header.emit_clicked();
    ensure(
        !section(&window, "borders")?.1,
        "the header didn't collapse the section",
    )?;
    ensure(
        ui_file(&fixture).as_deref() == Some(ui),
        &format!(
            "  with the preference off, ui.toml became {:?}",
            ui_file(&fixture)
        ),
    )?;
    window.destroy();
    Ok(())
}

/// A link to a setting in a collapsed section expands it, and the file still has it collapsed.
pub(crate) fn check_section_link() -> Outcome {
    let ui = "version = 1\n[collapsed]\nwindows = [\"gaps\"]\n";
    let fixture = fixture_with_ui(SWAYFX, STORED, USERS, true, None, Some(ui))?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let link = Link::parse("windows.gaps.inner", Registry::builtin())
        .map_err(|error| error.to_string())?;
    window.open_link(&link);
    settle(&window)?;
    ensure(
        section(&window, "gaps")?.1,
        "the link didn't expand the section",
    )?;
    ensure(
        ui_file(&fixture).as_deref() == Some(ui),
        &format!("  the link changed ui.toml to {:?}", ui_file(&fixture)),
    )?;
    window.destroy();
    Ok(())
}
