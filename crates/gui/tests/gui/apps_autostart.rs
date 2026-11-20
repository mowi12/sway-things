// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Apps & Autostart: one list of everything that starts at login, switched and removed there,
//! and the terminal that takes the place of the config's binding of its keys.

use super::*;

/// The user's config of the Apps & Autostart checks: the terminal's keys, a variable and two
/// exec lines.
pub(crate) const APPS_CONFIG: &str = "set $mod Mod4\n\
    set $term foot\n\
    bindsym $mod+Return exec $term\n\
    exec mako\n\
    exec_always kanshi\n";

/// Stored: two autostart entries of Sway Things, one switched off.
pub(crate) const STORED_APPS: &str = "version = 1\n\n[autostart]\n\
    entries = [\"exec nm-applet --indicator\", \"# exec swayidle -w\"]\n";

pub(crate) fn check_apps() -> Vec<(String, Outcome)> {
    vec![
        (
            "apps: one list of what starts, switched and removed here".to_owned(),
            check_autostart_list(),
        ),
        (
            "apps: the terminal takes the place of the config's binding".to_owned(),
            check_terminal_binding(),
        ),
    ]
}

/// The rows of the autostart list on the open page, with their texts.
fn autostart_rows(window: &Window) -> Vec<(gtk::Box, String)> {
    window
        .content()
        .map(|content| all::<gtk::Box>(&content))
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.has_css_class("st-autostart-row"))
        .map(|row| {
            let text = all::<gtk::Label>(row.upcast_ref())
                .iter()
                .map(|label| label.text().to_string())
                .collect::<Vec<_>>()
                .join(" | ");
            (row, text)
        })
        .collect()
}

fn check_autostart_list() -> Outcome {
    let fixture = fixture(SWAY, STORED_APPS, APPS_CONFIG, true)?;
    let window = open(&fixture, Page::AppsAutostart)?;
    let rows = autostart_rows(&window);
    let find = |rows: &[(gtk::Box, String)], text: &str| {
        rows.iter()
            .find(|(_, shown)| shown.contains(text))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "  no row with {text:?} in {:?}",
                    rows.iter().map(|(_, text)| text).collect::<Vec<_>>()
                )
            })
    };
    let (applet, shown) = find(&rows, "nm-applet --indicator")?;
    ensure(shown.contains("Starts at login"), &format!("  {shown}"))?;
    let (_, shown) = find(&rows, "swayidle -w")?;
    ensure(shown.contains("Switched off"), &format!("  {shown}"))?;
    let (_, shown) = find(&rows, "kanshi")?;
    ensure(
        shown.contains(":5, exec_always: also at every reload"),
        &format!("  {shown}"),
    )?;
    find(&rows, "mako")?;
    ensure(
        row(&window, "autostart.entries").is_err() && row(&window, "autostart.hidden").is_err(),
        "  the lists of entries have rows of their own",
    )?;
    // Switching an entry off stores it with `# `.
    let switch = all::<gtk::Switch>(applet.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  the entry has no switch".to_owned())?;
    switch.set_active(false);
    wait_for_model(&fixture.model)?;
    let items = fixture.model.list("autostart.entries");
    ensure(
        items == ["# exec nm-applet --indicator", "# exec swayidle -w"],
        &format!("  stored {items:?}"),
    )?;
    window.destroy();
    Ok(())
}

fn check_terminal_binding() -> Outcome {
    let fixture = fixture(SWAY, "version = 1\n", APPS_CONFIG, true)?;
    let window = open(&fixture, Page::AppsAutostart)?;
    let notes = |window: &Window| {
        window
            .content()
            .map(|content| all::<gtk::Label>(&content))
            .unwrap_or_default()
            .iter()
            .map(|label| label.text().to_string())
            .collect::<Vec<_>>()
    };
    ensure(
        notes(&window)
            .iter()
            .any(|text| text.starts_with("Now your config binds these keys to exec foot at ")),
        "  the config's binding of the terminal's keys isn't shown",
    )?;
    ensure(
        notes(&window)
            .iter()
            .any(|text| text == "Your config sets $term to foot"),
        "  $term isn't shown",
    )?;
    fixture
        .model
        .set("apps.terminal", None, Value::Text("kitty".to_owned()));
    wait_for_model(&fixture.model)?;
    let replaced = run_until(Duration::from_secs(2), || {
        notes(&window)
            .iter()
            .any(|text| text.starts_with("Takes the place of exec foot at "))
    });
    ensure(replaced, "  the choice doesn't say whose keys it takes")?;
    let listing = fixture
        .model
        .bindings(None)
        .ok_or_else(|| "  no bindings".to_owned())?;
    ensure(
        listing
            .rows()
            .iter()
            .any(|row| row.in_effect && row.line.command.as_deref() == Some("exec \"kitty\"")),
        "  the terminal isn't bound",
    )?;
    window.destroy();
    Ok(())
}
