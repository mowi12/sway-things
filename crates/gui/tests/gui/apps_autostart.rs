// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Apps & Autostart: one list of everything that starts at login, a line each, switched there,
//! with the details, removing and adding in dialogs; the terminal named after the config's binding
//! of its keys, and the choice that takes its place; the page with entries from every source in
//! light and dark.

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
            "apps: one list of what starts, a line each, switched here".to_owned(),
            check_autostart_list(),
        ),
        (
            "apps: the details of an entry, and removing one there".to_owned(),
            check_autostart_details(),
        ),
        (
            "apps: a command added from the list of apps".to_owned(),
            check_autostart_add(),
        ),
        (
            "apps: the terminal named after the config's binding, and the choice in its place"
                .to_owned(),
            check_terminal_binding(),
        ),
    ]
}

/// The lines of the autostart list on the open page, with their texts.
fn autostart_rows(window: &Window) -> Vec<(gtk::Box, String)> {
    window
        .content()
        .map(|content| all::<gtk::Box>(&content))
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.has_css_class("st-autostart-row"))
        .map(|row| (row.clone(), texts(row.upcast_ref())))
        .collect()
}

/// The texts of the labels in `widget`, joined by ` | `.
fn texts(widget: &gtk::Widget) -> String {
    all::<gtk::Label>(widget)
        .iter()
        .map(|label| label.text().to_string())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The line of `rows` that shows `text`.
fn line_with(rows: &[(gtk::Box, String)], text: &str) -> Result<(gtk::Box, String), String> {
    rows.iter()
        .find(|(_, shown)| shown.contains(text))
        .cloned()
        .ok_or_else(|| {
            format!(
                "  no line with {text:?} in {:?}",
                rows.iter().map(|(_, text)| text).collect::<Vec<_>>()
            )
        })
}

/// The dialog of the autostart list open now, the one opened last if there are two.
fn autostart_dialog() -> Option<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::Window>().ok())
        .filter(|window| window.has_css_class("st-autostart-dialog") && window.is_visible())
        .last()
}

/// Opens the details of the line of `window`'s autostart list that shows `text`, and returns
/// the dialog once it is shown.
fn open_details(window: &Window, text: &str) -> Result<gtk::Window, String> {
    let (line, _) = line_with(&autostart_rows(window), text)?;
    let row = line
        .parent()
        .and_downcast::<gtk::ListBoxRow>()
        .ok_or_else(|| "  the line isn't in a list".to_owned())?;
    row.activate();
    let shown = run_until(Duration::from_secs(2), || autostart_dialog().is_some());
    ensure(shown, &format!("  no details of {text:?}"))?;
    let dialog = autostart_dialog().ok_or_else(|| "  the dialog is gone".to_owned())?;
    settle(&dialog)?;
    Ok(dialog)
}

/// The button of `widget` whose label, or the label in its content, is `label`.
fn button_with(widget: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    button_labeled(widget, label).or_else(|| {
        all::<gtk::Button>(widget).into_iter().find(|button| {
            all::<gtk::Label>(button.upcast_ref())
                .iter()
                .any(|shown| shown.text() == label)
        })
    })
}

fn check_autostart_list() -> Outcome {
    let fixture = fixture(SWAY, STORED_APPS, APPS_CONFIG, true)?;
    let window = open(&fixture, Page::AppsAutostart)?;
    let rows = autostart_rows(&window);
    let (applet, shown) = line_with(&rows, "nm-applet --indicator")?;
    ensure(shown.contains("Command added here"), &format!("  {shown}"))?;
    let switch_of = |line: &gtk::Box| {
        all::<gtk::Switch>(line.upcast_ref())
            .into_iter()
            .next()
            .ok_or_else(|| "  the line has no switch".to_owned())
    };
    ensure(switch_of(&applet)?.is_active(), "  nm-applet is off")?;
    let (idle, _) = line_with(&rows, "swayidle -w")?;
    ensure(!switch_of(&idle)?.is_active(), "  swayidle is on")?;
    // The config's lines are read only: a lock, no switch, where they are in plain words.
    let (kanshi, shown) = line_with(&rows, "kanshi")?;
    ensure(
        shown.contains("Your config, line 5, and at every reload"),
        &format!("  {shown}"),
    )?;
    ensure(
        switch_of(&kanshi).is_err()
            && all::<gtk::Image>(kanshi.upcast_ref())
                .iter()
                .any(|image| image.has_css_class("st-autostart-lock")),
        "  the config's line has no lock, or a switch",
    )?;
    let (_, shown) = line_with(&rows, "mako")?;
    ensure(shown.contains("Your config, line 4"), &format!("  {shown}"))?;
    ensure(
        row(&window, "autostart.entries").is_err() && row(&window, "autostart.hidden").is_err(),
        "  the lists of entries have rows of their own",
    )?;
    // Switching an entry off stores it with `# `.
    switch_of(&applet)?.set_active(false);
    wait_for_model(&fixture.model)?;
    let items = fixture.model.list("autostart.entries");
    ensure(
        items == ["# exec nm-applet --indicator", "# exec swayidle -w"],
        &format!("  stored {items:?}"),
    )?;
    window.destroy();
    Ok(())
}

fn check_autostart_details() -> Outcome {
    let fixture = fixture(SWAY, STORED_APPS, APPS_CONFIG, true)?;
    let window = open(&fixture, Page::AppsAutostart)?;
    // A line of the config: where it is, with the editor, and nothing that changes it.
    let dialog = open_details(&window, "mako")?;
    let shown = texts(dialog.upcast_ref());
    ensure(
        shown.contains("Your sway config") && shown.contains("config:4"),
        &format!("  the details say {shown}"),
    )?;
    ensure(
        button_with(dialog.upcast_ref(), "Open in Editor").is_some()
            && button_with(dialog.upcast_ref(), "Remove").is_none(),
        "  the details of a config line can change it, or not open it",
    )?;
    dialog.close();
    ensure(
        run_until(Duration::from_secs(2), || autostart_dialog().is_none()),
        "  the dialog stays",
    )?;
    // Sway Things' own: the command, and Remove, which removes it.
    let dialog = open_details(&window, "swayidle -w")?;
    let shown = texts(dialog.upcast_ref());
    ensure(
        shown.contains("Switched off") && shown.contains("Sway Things"),
        &format!("  the details say {shown}"),
    )?;
    let remove = button_with(dialog.upcast_ref(), "Remove")
        .ok_or_else(|| "  no Remove in the details".to_owned())?;
    ensure(
        button_with(dialog.upcast_ref(), "Start Now").is_some(),
        "  no Start Now in the details",
    )?;
    remove.emit_clicked();
    wait_for_model(&fixture.model)?;
    let items = fixture.model.list("autostart.entries");
    ensure(
        items == ["exec nm-applet --indicator"],
        &format!("  stored {items:?}"),
    )?;
    ensure(
        run_until(Duration::from_secs(2), || autostart_dialog().is_none()),
        "  the dialog stays after Remove",
    )?;
    ensure(
        line_with(&autostart_rows(&window), "swayidle").is_err(),
        "  the removed entry is still listed",
    )?;
    window.destroy();
    Ok(())
}

fn check_autostart_add() -> Outcome {
    let fixture = fixture(SWAY, STORED_APPS, APPS_CONFIG, true)?;
    let window = open(&fixture, Page::AppsAutostart)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let add = button_with(&content, "Add App…").ok_or_else(|| "  no Add App…".to_owned())?;
    add.emit_clicked();
    ensure(
        run_until(Duration::from_secs(2), || autostart_dialog().is_some()),
        "  Add App… opens nothing",
    )?;
    let picker = autostart_dialog().ok_or_else(|| "  the dialog is gone".to_owned())?;
    settle(&picker)?;
    ensure(
        all::<gtk::SearchEntry>(picker.upcast_ref()).len() == 1,
        "  the list of apps can't be searched",
    )?;
    // Its first item asks for a command instead.
    let first = all::<gtk::ListBoxRow>(picker.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  the list of apps is empty".to_owned())?;
    ensure(
        texts(first.upcast_ref()).contains("Run a Command…"),
        &format!("  the first item is {}", texts(first.upcast_ref())),
    )?;
    first.activate();
    let asked = run_until(Duration::from_secs(2), || {
        autostart_dialog().is_some_and(|dialog| {
            !all::<gtk::Entry>(dialog.upcast_ref()).is_empty()
                && all::<gtk::SearchEntry>(dialog.upcast_ref()).is_empty()
        })
    });
    ensure(asked, "  Run a Command… asks for nothing")?;
    let dialog = autostart_dialog().ok_or_else(|| "  the dialog is gone".to_owned())?;
    let entry = all::<gtk::Entry>(dialog.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  no field for the command".to_owned())?;
    let accept = button_with(dialog.upcast_ref(), "Add").ok_or_else(|| "  no Add".to_owned())?;
    ensure(!accept.is_sensitive(), "  an empty command can be added")?;
    entry.set_text("waybar");
    ensure(accept.is_sensitive(), "  the command can't be added")?;
    accept.emit_clicked();
    wait_for_model(&fixture.model)?;
    let items = fixture.model.list("autostart.entries");
    ensure(
        items.last().map(String::as_str) == Some("exec waybar"),
        &format!("  stored {items:?}"),
    )?;
    line_with(&autostart_rows(&window), "waybar")?;
    window.destroy();
    Ok(())
}

/// The label of the item the drop-down of `key` shows.
fn shown_choice(window: &Window, key: &str) -> Result<String, String> {
    row(window, key)?
        .dropdown()
        .and_then(|dropdown| dropdown.selected_item())
        .and_downcast::<gtk::StringObject>()
        .map(|item| item.string().to_string())
        .ok_or_else(|| format!("  {key} shows no item"))
}

fn check_terminal_binding() -> Outcome {
    let fixture = fixture(SWAY, "version = 1\n", APPS_CONFIG, true)?;
    let window = open(&fixture, Page::AppsAutostart)?;
    // Without a choice, the terminal is what the config binds to its keys.
    let shown = shown_choice(&window, "apps.terminal")?;
    ensure(
        shown == "foot" || shown == "foot (your config)",
        &format!("  the terminal shows {shown:?}"),
    )?;
    let keys = row(&window, "apps.terminal_keys")?;
    ensure(
        button_labeled(keys.upcast_ref(), "Super + Enter").is_some(),
        "  the terminal's keys aren't a button of keys",
    )?;
    fixture
        .model
        .set("apps.terminal", None, Value::Text("kitty".to_owned()));
    wait_for_model(&fixture.model)?;
    let tooltip = || {
        row(&window, "apps.terminal")
            .ok()
            .and_then(|row| row.dropdown())
            .and_then(|dropdown| dropdown.tooltip_text())
            .map(|text| text.to_string())
            .unwrap_or_default()
    };
    let replaced = run_until(Duration::from_secs(2), || {
        tooltip().starts_with("Takes the place of exec foot at ")
    });
    ensure(
        replaced,
        &format!(
            "  the choice doesn't say whose keys it takes: {:?}",
            tooltip()
        ),
    )?;
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

/// The user's config of the realistic page: the terminal and launcher variables and their keys,
/// the runner of the XDG autostart entries and a few exec lines.
const REALISTIC_CONFIG: &str = "set $mod Mod4\n\
    set $term foot\n\
    set $menu wofi --show drun\n\
    bindsym $mod+Return exec $term\n\
    bindsym $mod+d exec $menu\n\
    exec mako\n\
    exec swayidle -w timeout 300 'swaylock -f'\n\
    exec_always kanshi\n\
    exec /usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1\n\
    exec dex --autostart --environment sway\n";

/// Stored for the realistic page: a terminal, two default apps, three entries of Sway Things
/// and a system entry switched off.
const REALISTIC_APPS: &str = "version = 1\n\n[apps]\nterminal = \"foot\"\n\n\
    [apps.default]\nbrowser = \"chromium.desktop\"\nimage_viewer = \"org.gnome.Loupe.desktop\"\n\n\
    [autostart]\nentries = [\"app org.keepassxc.KeePassXC.desktop\", \
    \"exec wl-paste --watch cliphist store\", \"# app org.telegram.desktop.desktop\"]\n\
    hidden = [\"xdg-user-dirs.desktop\"]\n";

/// The page with entries from every source, the entries that don't start unfolded, and its
/// dialogs, checked for contrast in the color `scheme` and saved to `apps/` of `screenshots`.
/// The installed apps and the XDG autostart entries are those of the session's XDG directories.
pub(crate) fn check_apps_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let fixture = fixture(SWAY, REALISTIC_APPS, REALISTIC_CONFIG, true)?;
    let directory = screenshots.map(|dir| dir.join("apps"));
    if let Some(dir) = &directory {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let window = Window::with_model(&fixture.model);
    window.present();
    window.show_page(Page::AppsAutostart);
    settle(&window)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    for more in all::<gtk::Expander>(&content) {
        if more.has_css_class("st-autostart-more") {
            more.set_expanded(true);
        }
    }
    let mut problems = String::new();
    if let Err(problem) = check_page(&window, Page::AppsAutostart, directory.as_deref(), scheme) {
        problems.push_str(&problem);
    }
    let shot = |dialog: &gtk::Window, name: &str| -> Result<String, String> {
        settle(dialog)?;
        wait_for_paint(dialog)?;
        check_screen(
            dialog,
            directory
                .as_ref()
                .map(|dir| dir.join(format!("{name}--{scheme}.png"))),
        )
    };
    // A system entry is there only with the XDG directories of the screenshots.
    for (text, name, needed) in [
        ("KeePassXC", "details-app", true),
        ("wl-paste --watch", "details-command", true),
        ("kanshi", "details-config", true),
        ("Network", "details-system", false),
    ] {
        if !needed && line_with(&autostart_rows(&window), text).is_err() {
            continue;
        }
        let dialog = open_details(&window, text)?;
        problems.push_str(&shot(&dialog, name)?);
        dialog.close();
        run_until(Duration::from_secs(2), || autostart_dialog().is_none());
    }
    if let Some(add) = button_with(&content, "Add App…") {
        add.emit_clicked();
        run_until(Duration::from_secs(2), || autostart_dialog().is_some());
        if let Some(dialog) = autostart_dialog() {
            problems.push_str(&shot(&dialog, "add")?);
            // Its first item asks for a command.
            if let Some(first) = all::<gtk::ListBoxRow>(dialog.upcast_ref())
                .into_iter()
                .next()
            {
                first.activate();
                run_until(Duration::from_secs(2), || {
                    autostart_dialog().is_some_and(|open| open != dialog)
                });
            }
            if let Some(command) = autostart_dialog().filter(|open| *open != dialog) {
                problems.push_str(&shot(&command, "add-command")?);
                command.close();
            }
        }
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}
