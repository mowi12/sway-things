// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The reload banner and the re-login badge.

use super::*;
/// The checks of the reload banner and the badges of settings that don't apply live.
pub(crate) fn check_reload() -> Vec<(String, Outcome)> {
    vec![
        (
            "reload: the banner counts the waiting changes and goes after the reload".to_owned(),
            check_reload_banner(),
        ),
        (
            "reload: a setting for the next login has its badge".to_owned(),
            check_relogin_badge(),
        ),
    ]
}

pub(crate) const WORKSPACE_LAYOUT: &str = "behavior.workspace_layout";
pub(crate) const XWAYLAND: &str = "behavior.xwayland";

/// A window on the Window Behavior page in the session of `sway`, with the default layout of new
/// workspaces changed, which waits for a reload of sway.
pub(crate) fn reload_waits(sway: &FakeSway) -> Result<(Fixture, Window), String> {
    let fixture = live_fixture(sway, STORED)?;
    let window = open(&fixture, Page::WindowBehavior)?;
    ensure(
        window.reload_banner().is_none(),
        "the reload banner shows before a change",
    )?;
    fixture
        .model
        .set(WORKSPACE_LAYOUT, None, Value::Choice("tabbed".to_owned()));
    wait_for_model(&fixture.model)?;
    settle(&window)?;
    Ok((fixture, window))
}

/// A change for the config only shows the banner; a change for the next login doesn't count;
/// the reload saves, asks sway and hides the banner.
pub(crate) fn check_reload_banner() -> Outcome {
    let sway = laptop();
    let (fixture, window) = reload_waits(&sway)?;
    let expected = Some("1 change applies after sway reloads its config".to_owned());
    ensure(
        window.reload_banner() == expected,
        &format!("  the reload banner says {:?}", window.reload_banner()),
    )?;
    ensure(
        window.reload_button().is_sensitive(),
        "Reload sway… can't be pressed",
    )?;
    fixture
        .model
        .set(XWAYLAND, None, Value::Choice("force".to_owned()));
    wait_for_model(&fixture.model)?;
    ensure(
        window.reload_banner() == expected,
        &format!(
            "  after a change for the next login the banner says {:?}",
            window.reload_banner()
        ),
    )?;
    fixture.model.reload_sway();
    wait_for_model(&fixture.model)?;
    ensure(
        sway.commands().iter().any(|command| command == "reload"),
        "sway wasn't asked to reload",
    )?;
    ensure(
        window.reload_banner().is_none(),
        "the reload banner stays after the reload",
    )?;
    ensure(
        window.problem().is_none(),
        &format!("  problem: {:?}", window.problem()),
    )?;
    let generated = std::fs::read_to_string(fixture.locations.path(ConfigFile::Behavior))
        .map_err(|error| format!("  {error}"))?;
    ensure(
        generated.contains("workspace_layout tabbed"),
        "the change wasn't saved before the reload",
    )?;
    window.destroy();
    Ok(())
}

/// Xwayland, which sway only reads at login, says so in a badge.
pub(crate) fn check_relogin_badge() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::WindowBehavior)?;
    let xwayland = row(&window, XWAYLAND)?;
    ensure(
        xwayland.badges() == ["Applies after re-login"],
        &format!("  Xwayland has the badges {:?}", xwayland.badges()),
    )?;
    window.destroy();
    let window = open(&fixture, Page::Workspaces)?;
    let layout = row(&window, WORKSPACE_LAYOUT)?;
    ensure(
        layout.badges() == ["Applies after reload"],
        &format!("  the default layout has the badges {:?}", layout.badges()),
    )?;
    window.destroy();
    Ok(())
}

/// The Window Behavior page with the reload banner, checked like the pages.
pub(crate) fn check_reload_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let sway = laptop();
    let (_fixture, window) = reload_waits(&sway)?;
    ensure(window.reload_banner().is_some(), "no reload banner")?;
    let directory = screenshots.map(|dir| dir.join("reload"));
    let outcome = check_page(&window, Page::WindowBehavior, directory.as_deref(), scheme);
    window.destroy();
    outcome
}
