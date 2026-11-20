// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keyboard Shortcuts: every binding of the config and of Sway Things is listed and found by keys
//! and command, the dialog adds and changes bindings through the items of `shortcuts.bindings`,
//! warns of keys in use and refuses keys it can't take; the page and the dialog are checked for
//! contrast with bindings of every kind.

use super::*;

/// A config with bindings of every kind: before the include line, in a mode, by keycode, on the
/// same keys twice, and one after the include line.
pub(crate) const BINDINGS_CONFIG: &str = "set $mod Mod4\n\
    bindsym $mod+Return exec foot\n\
    bindsym $mod+d exec wmenu-run\n\
    bindsym $mod+Shift+q kill\n\
    bindsym --locked XF86AudioMute exec pactl set-sink-mute @DEFAULT_SINK@ toggle\n\
    bindsym $mod+f fullscreen\n\
    bindsym Super+F fullscreen toggle\n\
    bindcode --release $mod+38 focus left\n\
    mode \"resize\" {\n\
        bindsym Escape mode \"default\"\n\
    }\n";

/// Stored: a change of the user's Super+Return, a binding of Sway Things, the volume keys.
pub(crate) const STORED_BINDINGS: &str = "version = 1\n\n[shortcuts]\nbindings = [\
    \"unbindsym Mod4+Return => bindsym $mod+t exec foot\", \"bindsym $mod+x exec \\\"firefox\\\"\"]\n\n\
    [shortcuts.media]\nvolume = true\n";

pub(crate) fn check_shortcuts() -> Vec<(String, Outcome)> {
    vec![
        (
            "shortcuts: every binding is listed and searchable".to_owned(),
            check_shortcut_list(),
        ),
        (
            "shortcuts: the dialog adds and changes bindings".to_owned(),
            check_shortcut_editor(),
        ),
        (
            "shortcuts: the dialog warns of keys in use".to_owned(),
            check_shortcut_conflicts(),
        ),
    ]
}

/// A model with the stored bindings and the config of bindings, with the include line and one
/// line after it.
pub(crate) fn shortcut_fixture() -> Result<Fixture, String> {
    let fixture = fixture(SWAY, STORED_BINDINGS, BINDINGS_CONFIG, true)?;
    let after = format!(
        "{BINDINGS_CONFIG}{}bindsym $mod+Shift+space floating toggle\n",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&after));
    Ok(fixture)
}

/// The rows of bindings shown on the open page.
pub(crate) fn shortcut_rows(window: &Window) -> Vec<gtk::Box> {
    window
        .content()
        .map(|content| all::<gtk::Box>(&content))
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.has_css_class("st-shortcut-row"))
        .collect()
}

pub(crate) fn check_shortcut_list() -> Outcome {
    let fixture = shortcut_fixture()?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    let listing = fixture
        .model
        .bindings(None)
        .ok_or_else(|| "  no bindings".to_owned())?;
    let shown = shortcut_rows(&window).len();
    ensure(
        shown == listing.rows().len() && shown > 10,
        &format!("  {shown} rows for {} bindings", listing.rows().len()),
    )?;
    let search = widget::<gtk::SearchEntry>(&window)?;
    for (query, expected) in [
        ("wmenu", 1),
        ("super enter", 0),
        ("super t foot", 1),
        ("mute", 3),
    ] {
        search.set_text(query);
        let found = run_until(Duration::from_secs(2), || {
            shortcut_rows(&window).len() == expected
        });
        ensure(
            found,
            &format!(
                "  {query:?} shows {} rows, not {expected}",
                shortcut_rows(&window).len()
            ),
        )?;
    }
    search.set_text("");
    ensure(
        run_until(Duration::from_secs(2), || {
            shortcut_rows(&window).len() == shown
        }),
        "  an empty search doesn't show every row",
    )?;
    // The switch of the volume keys says that they take the place of the user's mute key.
    let clashes = window
        .content()
        .map(|content| all::<gtk::Box>(&content))
        .unwrap_or_default()
        .into_iter()
        .filter(|widget| widget.has_css_class("st-shortcut-group-clashes"))
        .any(|widget| widget.is_visible());
    let has_switch = row(&window, "shortcuts.media.volume").is_ok();
    ensure(
        !has_switch || clashes,
        "  the volume keys don't say what they replace",
    )?;
    ensure(
        row(&window, "shortcuts.bindings").is_err(),
        "  the list of items has a row of its own",
    )?;
    window.destroy();
    Ok(())
}

pub(crate) fn editor(fixture: &Fixture, window: &Window, row: Option<usize>) -> ShortcutEditor {
    ShortcutEditor::open(
        window,
        &fixture.model,
        row,
        std::rc::Rc::new(GdkKeymap::of_default()),
    )
}

/// The index of the row in effect with `keys` in the model's listing.
pub(crate) fn binding_row(model: &Model, keys: &str) -> Result<usize, String> {
    model
        .bindings(None)
        .and_then(|listing| {
            listing
                .rows()
                .iter()
                .position(|row| row.in_effect && row.line.keys == keys)
        })
        .ok_or_else(|| format!("  no binding of {keys}"))
}

pub(crate) fn check_shortcut_editor() -> Outcome {
    let fixture = shortcut_fixture()?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    let before = fixture.model.binding_items();
    let new = editor(&fixture, &window, None);
    ensure(
        !new.save_button().is_sensitive(),
        "  a binding without keys can be saved",
    )?;
    new.set_keys("$mod+g");
    new.set_run("foot -e htop");
    ensure(
        new.save_button().is_sensitive(),
        &format!("  the new binding can't be saved: {:?}", new.error()),
    )?;
    new.save_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    let mut expected = before.clone();
    expected.push("bindsym $mod+g exec \"foot -e htop\"".to_owned());
    let items = fixture.model.binding_items();
    ensure(items == expected, &format!("  stored {items:?}"))?;
    // A binding of the user's is changed through an unbind and a binding of ours.
    let index = binding_row(&fixture.model, "Mod4+d")?;
    let change = editor(&fixture, &window, Some(index));
    change.set_keys("$mod+p");
    change.save_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    expected.push("unbindsym Mod4+d => bindsym $mod+p exec wmenu-run".to_owned());
    let items = fixture.model.binding_items();
    ensure(items == expected, &format!("  stored {items:?}"))?;
    // A key that types, bound alone, is refused; F-keys are fine.
    let bare = editor(&fixture, &window, None);
    bare.set_run("foot");
    bare.set_keys("a");
    ensure(
        !bare.save_button().is_sensitive() && bare.error().is_some(),
        "  a bare letter can be saved",
    )?;
    bare.set_keys("F9");
    ensure(
        bare.save_button().is_sensitive(),
        &format!("  F9 alone can't be saved: {:?}", bare.error()),
    )?;
    bare.window().close();
    // A sway command that sway doesn't know is refused.
    let wrong = editor(&fixture, &window, None);
    wrong.set_keys("$mod+o");
    wrong.set_sway("fokus left");
    ensure(
        !wrong.save_button().is_sensitive() && wrong.error().is_some(),
        "  an unknown sway command can be saved",
    )?;
    wrong.window().close();
    window.destroy();
    Ok(())
}

pub(crate) fn check_shortcut_conflicts() -> Outcome {
    let fixture = shortcut_fixture()?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    let new = editor(&fixture, &window, None);
    new.set_run("foot");
    // Keys of the user's: they are replaced.
    new.set_keys("$mod+Shift+q");
    let conflict = new.conflict().unwrap_or_default();
    ensure(
        conflict.contains("Close the window") && new.save_button().is_sensitive(),
        &format!("  keys of the user's: {conflict:?}"),
    )?;
    // A line after the include line can't be replaced.
    new.set_keys("$mod+Shift+space");
    ensure(
        new.conflict().is_some() && !new.save_button().is_sensitive(),
        "  keys bound after the include line can be saved",
    )?;
    // Nor the keys of the volume group, which bind them on the lock screen too.
    new.set_locked(true);
    new.set_keys("XF86AudioRaiseVolume");
    ensure(
        new.conflict().is_some() && !new.save_button().is_sensitive(),
        "  keys of the volume keys can be saved",
    )?;
    // Free keys say nothing.
    new.set_locked(false);
    new.set_keys("$mod+Ctrl+F12");
    ensure(new.conflict().is_none(), "  free keys show a warning")?;
    new.window().close();
    window.destroy();
    Ok(())
}

/// The page with bindings of every kind, and the dialog with a warning, in the color `scheme`.
pub(crate) fn check_shortcut_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let fixture = shortcut_fixture()?;
    let directory = screenshots.map(|dir| dir.join("bindings"));
    if let Some(dir) = &directory {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let window = Window::with_model(&fixture.model);
    window.present();
    let mut problems = String::new();
    if let Err(problem) = check_page(
        &window,
        Page::KeyboardShortcuts,
        directory.as_deref(),
        scheme,
    ) {
        problems.push_str(&problem);
    }
    let index = binding_row(&fixture.model, "Mod4+t")?;
    let dialog = editor(&fixture, &window, Some(index));
    dialog.set_keys("$mod+Shift+q");
    settle(dialog.window())?;
    wait_for_paint(dialog.window())?;
    problems.push_str(&check_screen(
        dialog.window(),
        directory.map(|dir| dir.join(format!("shortcut-dialog--{scheme}.png"))),
    )?);
    dialog.window().close();
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}
