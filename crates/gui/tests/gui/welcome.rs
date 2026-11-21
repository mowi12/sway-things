// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The welcome screen and the read-only banner, with their spinner.

use super::*;
/// The welcome screen and a page with the read-only banner, checked like the pages.
pub(crate) fn check_read_only_screens(
    screenshots: Option<&Path>,
    scheme: &str,
) -> (Outcome, Outcome) {
    let fixture = match fixture(SWAYFX, STORED, USERS, false) {
        Ok(fixture) => fixture,
        Err(error) => return (Err(error.clone()), Err(error)),
    };
    let window = Window::with_model(&fixture.model);
    window.present();
    let welcome = settle(&window)
        .and_then(|()| wait_for_paint(&window))
        .and_then(|()| {
            let path = screenshots.map(|dir| dir.join(format!("welcome--{scheme}.png")));
            let problems = check_screen(&window, path)?;
            ensure(problems.is_empty(), problems.trim_start())
        });
    window.look_around();
    let directory = screenshots.map(|dir| dir.join("read-only"));
    let banner = check_page(&window, Page::MouseTouchpad, directory.as_deref(), scheme);
    window.destroy();
    (welcome, banner)
}

/// Without the include line the window opens on the welcome screen; "Look around first" and
/// opening a page lead to the pages with the banner, whose button leads back.
pub(crate) fn check_welcome() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, false)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(window.is_welcome_shown(), "the welcome screen isn't shown")?;
    ensure(window.welcome().is_waiting(), "the spinner doesn't spin")?;
    ensure(
        window.welcome().line().starts_with("include ")
            && window.welcome().line().ends_with("/*.conf"),
        &format!("the welcome screen asks for {:?}", window.welcome().line()),
    )?;
    ensure(
        window.sidebar().selected().is_none(),
        "a page is marked in the sidebar",
    )?;
    ensure(
        window.status_banner().is_none(),
        "the banner shows with the welcome screen",
    )?;
    let [_, _, look_around] = window.welcome().buttons();
    ensure(
        look_around.is_focusable(),
        "\"Look around first\" can't take the focus",
    )?;
    look_around.emit_clicked();
    settle(&window)?;
    ensure(
        !window.is_welcome_shown(),
        "\"Look around first\" didn't leave the welcome screen",
    )?;
    ensure(
        !window.welcome().is_waiting(),
        "the spinner spins while hidden",
    )?;
    ensure(
        window.page() == Page::ALL.first().copied(),
        "no page is open",
    )?;
    ensure(
        window
            .status_banner()
            .is_some_and(|text| text.starts_with("Not enabled yet")),
        "the read-only banner isn't shown",
    )?;
    window.status_button().emit_clicked();
    settle(&window)?;
    ensure(
        window.is_welcome_shown(),
        "\"Show instructions\" didn't bring back the welcome screen",
    )?;
    ensure(
        window.welcome().is_waiting(),
        "the spinner doesn't spin after coming back",
    )?;
    // Picking a page from the sidebar counts as looking around.
    window.show_page(Page::Keyboard);
    settle(&window)?;
    ensure(
        !window.is_welcome_shown() && window.page() == Some(Page::Keyboard),
        "opening a page didn't leave the welcome screen",
    )?;
    ensure(
        window.status_banner().is_some(),
        "the banner is gone after opening a page",
    )?;
    // A change from the model, like any row's, doesn't bring the welcome screen back.
    fixture.model.set_status(fixture.status(USERS, false));
    settle(&window)?;
    ensure(
        !window.is_welcome_shown(),
        "the welcome screen came back by itself",
    )?;
    window.destroy();
    Ok(())
}

/// Once the user adds the line, the welcome screen and the banner go away by themselves.
pub(crate) fn check_welcome_closes() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, false)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(window.is_welcome_shown(), "the welcome screen isn't shown")?;
    fixture.model.set_status(fixture.status(USERS, true));
    settle(&window)?;
    ensure(
        !window.is_welcome_shown(),
        "the welcome screen stayed after the line was added",
    )?;
    ensure(
        window.page() == Page::ALL.first().copied(),
        "no page is open",
    )?;
    ensure(
        window.status_banner().is_none(),
        "a banner stayed after the line was added",
    )?;
    let tap = row(&window, "input.touchpad.tap")?;
    ensure(
        tap.controls()
            .iter()
            .all(gtk::prelude::WidgetExt::is_sensitive),
        "the rows stayed locked after the line was added",
    )?;
    window.destroy();
    Ok(())
}

/// With lines of the user's after the include, a banner explains that they win, once.
pub(crate) fn check_not_last() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let after = fixture.status_of(&format!("{}gaps inner 3\n", fixture.include_line()));
    fixture.model.set_status(after);
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(
        !window.is_welcome_shown(),
        "the welcome screen shows with the line there",
    )?;
    let text = window
        .status_banner()
        .ok_or_else(|| "  no banner for the lines after the include".to_owned())?;
    ensure(
        text.contains("1 of your config lines comes after them"),
        &format!("the banner says {text:?}"),
    )?;
    window.status_button().emit_clicked();
    settle(&window)?;
    ensure(
        window.status_banner().is_none(),
        "the banner can't be dismissed",
    )?;
    window.destroy();
    Ok(())
}

/// The welcome screen: its spinner, looking around and back, closing once the line is there,
/// and a line that isn't last.
pub(crate) fn check_welcomes(settings: &gtk::Settings) -> Vec<(String, Outcome)> {
    vec![
        (
            "welcome: the spinner turns after coming back".to_owned(),
            check_spinner(settings),
        ),
        ("welcome: look around and back".to_owned(), check_welcome()),
        (
            "welcome: closes once the line is there".to_owned(),
            check_welcome_closes(),
        ),
        (
            "welcome: a line that isn't last".to_owned(),
            check_not_last(),
        ),
    ]
}
