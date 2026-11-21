// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keyboard Shortcuts: every binding of the config and of Sway Things is listed and found by keys
//! and command, a click on a row opens its dialog, which takes keys as they are pressed, shows
//! the keys in use with the modifiers held, says whether the keys are free, adds, changes and
//! removes bindings through the items of `shortcuts.bindings`, warns of keys in use and refuses
//! keys it can't take; a binding that can't be changed opens read only. The page and the dialog
//! are checked for contrast with a realistic config of bindings of every kind.

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

/// A realistic config: sway's default bindings and some more, with a binding on the same keys
/// twice, a keycode binding, a mute key the volume keys take, a mode, and after the include line
/// one that can't be changed here (added by [`realistic_fixture`]).
pub(crate) const REALISTIC_CONFIG: &str = "set $mod Mod4\n\
    set $left h\n\
    bindsym $mod+Return exec foot\n\
    bindsym $mod+Shift+q kill\n\
    bindsym $mod+d exec wmenu-run\n\
    bindsym $mod+Shift+c reload\n\
    bindsym $mod+Shift+e exec swaynag -t warning -m 'Exit sway?' -B 'Yes, exit' 'swaymsg exit'\n\
    bindsym $mod+$left focus left\n\
    bindsym $mod+j focus down\n\
    bindsym $mod+k focus up\n\
    bindsym $mod+l focus right\n\
    bindsym $mod+Shift+h move left\n\
    bindsym $mod+Shift+j move down\n\
    bindsym $mod+Shift+k move up\n\
    bindsym $mod+Shift+l move right\n\
    bindsym $mod+1 workspace number 1\n\
    bindsym $mod+2 workspace number 2\n\
    bindsym $mod+3 workspace number 3\n\
    bindsym $mod+4 workspace number 4\n\
    bindsym $mod+Shift+1 move container to workspace number 1\n\
    bindsym $mod+Shift+2 move container to workspace number 2\n\
    bindsym $mod+Shift+3 move container to workspace number 3\n\
    bindsym $mod+b splith\n\
    bindsym $mod+v splitv\n\
    bindsym $mod+s layout stacking\n\
    bindsym $mod+w layout tabbed\n\
    bindsym $mod+e layout toggle split\n\
    bindsym $mod+f fullscreen\n\
    bindsym $mod+space focus mode_toggle\n\
    bindsym $mod+a focus parent\n\
    bindsym $mod+Shift+minus move scratchpad\n\
    bindsym $mod+minus scratchpad show\n\
    bindsym --locked XF86AudioMute exec pactl set-sink-mute @DEFAULT_SINK@ toggle\n\
    bindsym Print exec grim\n\
    bindsym $mod+Shift+s exec grim -g \"$(slurp)\" - | wl-copy\n\
    bindsym Super+F fullscreen toggle\n\
    bindcode --release $mod+38 focus left\n\
    bindsym $mod+r mode \"resize\"\n\
    mode \"resize\" {\n\
        bindsym h resize shrink width 10px\n\
        bindsym l resize grow width 10px\n\
        bindsym Return mode \"default\"\n\
        bindsym Escape mode \"default\"\n\
    }\n";

/// Stored for the realistic config: Super+Return changed to Super+T, Super+B removed, a binding
/// of Sway Things for firefox and the volume keys.
pub(crate) const REALISTIC_STORED: &str = "version = 1\n\n[shortcuts]\nbindings = [\
    \"unbindsym Mod4+Return => bindsym $mod+t exec foot\", \"unbindsym Mod4+b\", \
    \"bindsym $mod+x exec \\\"firefox\\\"\"]\n\n\
    [shortcuts.media]\nvolume = true\n";

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
        (
            "shortcuts: rows open their dialog, which takes keys, saves and removes".to_owned(),
            check_shortcut_clicks(),
        ),
        (
            "shortcuts: the dialog removes, the row puts back, Esc closes".to_owned(),
            check_shortcut_removal(),
        ),
        (
            "shortcuts: a binding that can't be changed here opens read only".to_owned(),
            check_shortcut_read_only(),
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

/// A model with the realistic config and its stored bindings, with the include line and one
/// line after it.
pub(crate) fn realistic_fixture() -> Result<Fixture, String> {
    let fixture = fixture(SWAY, REALISTIC_STORED, REALISTIC_CONFIG, true)?;
    let after = format!(
        "{REALISTIC_CONFIG}{}bindsym $mod+Shift+x exec swaylock -f -c 000000\n",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&after));
    Ok(fixture)
}

/// The row of the list whose binding does `title` (as the row says it).
pub(crate) fn list_item(window: &Window, title: &str) -> Result<gtk::ListBoxRow, String> {
    shortcut_rows(window)
        .into_iter()
        .find(|row| {
            row.first_child()
                .and_downcast::<gtk::Label>()
                .is_some_and(|label| label.text() == title)
        })
        .and_then(|row| row.parent().and_downcast::<gtk::ListBoxRow>())
        .ok_or_else(|| format!("  no row of {title:?}"))
}

/// The dialog a click opened, once it is shown.
pub(crate) fn opened_editor() -> Result<ShortcutEditor, String> {
    let mut editor = None;
    run_until(Duration::from_secs(2), || {
        editor = ShortcutEditor::last_opened();
        editor.is_some()
    });
    editor.ok_or_else(|| "  no dialog opened".to_owned())
}

/// Sends a key press (or release) of `key` with the hardware `code` and the modifiers `state`
/// to the key controller of `window`, as GTK does for a key pressed on it.
pub(crate) fn key_event(
    window: &gtk::Window,
    signal: &str,
    key: gtk::gdk::Key,
    code: u32,
    state: gtk::gdk::ModifierType,
) -> Outcome {
    use gtk::glib::translate::IntoGlib;
    let controllers = window.observe_controllers();
    let controller = (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .find_map(|item| item.downcast::<gtk::EventControllerKey>().ok())
        .ok_or_else(|| "  the dialog has no key controller".to_owned())?;
    if signal == "key-pressed" {
        let _: bool = controller.emit_by_name(signal, &[&key.into_glib(), &code, &state]);
    } else {
        controller.emit_by_name::<()>(signal, &[&key.into_glib(), &code, &state]);
    }
    Ok(())
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
        ("super+shift+q", 1),
        ("Super + Shift + Q", 1),
        ("mute", 3),
        ("nothing binds this", 0),
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

pub(crate) fn check_shortcut_clicks() -> Outcome {
    use gtk::gdk::{Key, ModifierType};
    let fixture = shortcut_fixture()?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    let mut expected = fixture.model.binding_items();
    // A click on a row opens its dialog, ready to change it.
    list_item(&window, "Run wmenu-run")?.activate();
    let dialog = opened_editor()?;
    ensure(
        !dialog.is_read_only() && dialog.remove_button().is_visible(),
        "  the dialog of a binding of the user's can't change it",
    )?;
    ensure(
        !dialog.details().is_expanded(),
        "  the details of a binding that can be changed are expanded",
    )?;
    // A click on the field listens; holding Super lists the keys in use with it.
    dialog.capture_button().emit_clicked();
    ensure(dialog.is_capturing(), "  a click doesn't start listening")?;
    key_event(
        dialog.window(),
        "key-pressed",
        Key::Super_L,
        133,
        ModifierType::empty(),
    )?;
    let taken = dialog.taken_keys();
    ensure(
        taken.iter().any(|key| key == "T") && taken.iter().any(|key| key == "D"),
        &format!("  keys in use with Super: {taken:?}"),
    )?;
    key_event(
        dialog.window(),
        "key-released",
        Key::Super_L,
        133,
        ModifierType::SUPER_MASK,
    )?;
    ensure(
        dialog.taken_keys().is_empty(),
        "  the keys in use stay after Super is let go",
    )?;
    // Super+P: taken, free, and saved in place of Super+D.
    key_event(
        dialog.window(),
        "key-pressed",
        Key::p,
        33,
        ModifierType::SUPER_MASK,
    )?;
    ensure(
        !dialog.is_capturing() && dialog.says_free(),
        &format!(
            "  Super+P isn't taken as free keys: {:?} {:?}",
            dialog.conflict(),
            dialog.error()
        ),
    )?;
    dialog.save_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    expected.push("unbindsym Mod4+d => bindsym $mod+p exec wmenu-run".to_owned());
    let items = fixture.model.binding_items();
    ensure(items == expected, &format!("  stored {items:?}"))?;
    window.destroy();
    Ok(())
}

pub(crate) fn check_shortcut_removal() -> Outcome {
    use gtk::gdk::{Key, ModifierType};
    let fixture = shortcut_fixture()?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    let mut expected = fixture.model.binding_items();
    // Esc stops listening without changing the keys; Esc again closes the dialog.
    list_item(&window, "Close the window")?.activate();
    let dialog = opened_editor()?;
    dialog.start_capture();
    key_event(
        dialog.window(),
        "key-pressed",
        Key::Escape,
        9,
        ModifierType::empty(),
    )?;
    ensure(
        !dialog.is_capturing() && dialog.conflict().is_none(),
        "  Esc doesn't stop listening",
    )?;
    // Remove takes the user's binding away with an unbind of Sway Things.
    dialog.remove_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    expected.push("unbindsym Mod4+Shift+q".to_owned());
    let items = fixture.model.binding_items();
    ensure(items == expected, &format!("  stored {items:?}"))?;
    // The row of the removed binding puts it back with a click.
    let removed = list_item(&window, "Close the window")?;
    let restore = all::<gtk::Button>(removed.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  the removed binding has no button that puts it back".to_owned())?;
    restore.emit_clicked();
    wait_for_model(&fixture.model)?;
    expected.pop();
    let items = fixture.model.binding_items();
    ensure(items == expected, &format!("  stored {items:?}"))?;
    // Esc closes a dialog that doesn't listen.
    list_item(&window, "Run foot")?.activate();
    let dialog = opened_editor()?;
    ensure(
        dialog.restore_button().is_visible(),
        "  a changed binding can't be put back from its dialog",
    )?;
    key_event(
        dialog.window(),
        "key-pressed",
        Key::Escape,
        9,
        ModifierType::empty(),
    )?;
    ensure(
        run_until(Duration::from_secs(2), || !dialog.window().is_visible()),
        "  Esc doesn't close the dialog",
    )?;
    // Add Shortcut… listens at once.
    all::<gtk::Button>(window.upcast_ref())
        .into_iter()
        .find(|button| {
            button.child().is_some_and(|child| {
                all::<gtk::Label>(&child)
                    .iter()
                    .any(|label| label.text() == "Add Shortcut…")
            })
        })
        .ok_or_else(|| "  no Add Shortcut… button".to_owned())?
        .emit_clicked();
    let new = opened_editor()?;
    ensure(
        run_until(Duration::from_secs(2), || new.is_capturing()),
        "  a new shortcut doesn't listen for its keys at once",
    )?;
    new.window().close();
    window.destroy();
    Ok(())
}

pub(crate) fn check_shortcut_read_only() -> Outcome {
    let fixture = shortcut_fixture()?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    // The line after the include line: a lock on its row, a dialog that only shows it.
    let item = list_item(&window, "Toggle floating")?;
    let lock = all::<gtk::Image>(item.upcast_ref())
        .into_iter()
        .find(|image| image.icon_name().as_deref() == Some("changes-prevent-symbolic"));
    ensure(
        lock.is_some_and(|lock| lock.tooltip_text().is_some()),
        "  a binding read after Sway Things' file has no lock with a reason",
    )?;
    item.activate();
    let dialog = opened_editor()?;
    ensure(
        dialog.is_read_only()
            && !dialog.save_button().is_visible()
            && !dialog.remove_button().is_visible()
            && !dialog.capture_button().can_target()
            && dialog.details().is_expanded(),
        "  the dialog of a binding that can't be changed offers to change it",
    )?;
    dialog.window().close();
    // A key of the volume keys links to their switch.
    if row(&window, "shortcuts.media.volume").is_ok() {
        shortcut_rows(&window)
            .into_iter()
            .find(|row| {
                all::<gtk::Label>(row.upcast_ref())
                    .iter()
                    .any(|label| label.text() == "Volume Up")
            })
            .and_then(|row| row.parent().and_downcast::<gtk::ListBoxRow>())
            .ok_or_else(|| "  no row of the volume keys".to_owned())?
            .activate();
        let dialog = opened_editor()?;
        ensure(
            dialog.is_read_only() && dialog.owner_button().is_visible(),
            "  a key of the volume keys doesn't link to their switch",
        )?;
        dialog.owner_button().emit_clicked();
        ensure(
            run_until(Duration::from_secs(2), || !dialog.window().is_visible()),
            "  the link to the switch doesn't close the dialog",
        )?;
        ensure(
            run_until(Duration::from_secs(2), || {
                row(&window, "shortcuts.media.volume")
                    .ok()
                    .and_then(|row| row.parent())
                    .is_some_and(|item| item.has_css_class(style::HIGHLIGHT))
            }),
            "  the link doesn't point to the switch",
        )?;
    }
    window.destroy();
    Ok(())
}

/// The page with bindings of every kind, and the dialog with a warning, in the color `scheme`.
pub(crate) fn check_shortcut_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    use gtk::gdk::{Key, ModifierType};
    let fixture = realistic_fixture()?;
    let directory = screenshots.map(|dir| dir.join("bindings"));
    if let Some(dir) = &directory {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let count = fixture
        .model
        .bindings(None)
        .map_or(0, |listing| listing.rows().len());
    ensure(count >= 30, &format!("  only {count} bindings"))?;
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
    let shot = |dialog: &ShortcutEditor, name: &str| -> Result<String, String> {
        settle(dialog.window())?;
        wait_for_paint(dialog.window())?;
        check_screen(
            dialog.window(),
            directory
                .as_ref()
                .map(|dir| dir.join(format!("{name}--{scheme}.png"))),
        )
    };
    // A change onto keys in use.
    let index = binding_row(&fixture.model, "Mod4+t")?;
    let dialog = editor(&fixture, &window, Some(index));
    dialog.set_keys("$mod+Shift+q");
    problems.push_str(&shot(&dialog, "shortcut-dialog")?);
    dialog.window().close();
    // A binding that clashes, with its details open.
    let index = binding_row(&fixture.model, "Super+F")?;
    let dialog = editor(&fixture, &window, Some(index));
    dialog.details().set_expanded(true);
    problems.push_str(&shot(&dialog, "shortcut-clash")?);
    dialog.window().close();
    // A new shortcut while Super is held: the keys in use with it.
    let dialog = editor(&fixture, &window, None);
    dialog.start_capture();
    key_event(
        dialog.window(),
        "key-pressed",
        Key::Super_L,
        133,
        ModifierType::empty(),
    )?;
    problems.push_str(&shot(&dialog, "shortcut-capture")?);
    key_event(
        dialog.window(),
        "key-pressed",
        Key::F12,
        96,
        ModifierType::SUPER_MASK | ModifierType::CONTROL_MASK,
    )?;
    dialog.set_run("foot -e htop");
    problems.push_str(&shot(&dialog, "shortcut-free")?);
    dialog.window().close();
    // A line read after Sway Things' file.
    let index = binding_row(&fixture.model, "Mod4+Shift+x")?;
    let dialog = editor(&fixture, &window, Some(index));
    problems.push_str(&shot(&dialog, "shortcut-locked")?);
    dialog.window().close();
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}
