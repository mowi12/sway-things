// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Apps Look & Feel with GSettings in memory.

use super::*;
pub(crate) fn check_look() -> Vec<(String, Outcome)> {
    vec![
        (
            "look: a row shows what GSettings has and a pick writes it there".to_owned(),
            check_look_gsettings(),
        ),
        (
            "look: a font row shows the font and keeps it".to_owned(),
            check_look_font(),
        ),
        (
            "look: a value GSettings refuses shows under its row".to_owned(),
            check_look_refused(),
        ),
    ]
}

/// GSettings without the schema: every write fails.
#[derive(Debug)]
pub(crate) struct NoSchema;

impl GSettingsStore for NoSchema {
    fn set(&self, key: GSettingsKey, _: &Value) -> Result<(), GSettingsError> {
        Err(GSettingsError::NoSchema { key })
    }
    fn reset(&self, key: GSettingsKey) -> Result<(), GSettingsError> {
        Err(GSettingsError::NoSchema { key })
    }
    fn value(&self, _: GSettingsKey) -> Option<Value> {
        None
    }
    fn check(&self, key: GSettingsKey, _: &Value) -> Result<(), GSettingsError> {
        Err(GSettingsError::NoSchema { key })
    }
}

/// A style GSettings can't take is stored, and its row says that apps keep their own.
pub(crate) fn check_look_refused() -> Outcome {
    let fixture = fixture_with_gsettings(
        SWAY,
        "version = 1\n",
        USERS,
        true,
        None,
        None,
        Some(Box::new(NoSchema)),
    )?;
    let window = open(&fixture, Page::AppsLookFeel)?;
    let style = row(&window, "look.style")?;
    let choice = style
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::DropDown>().ok())
        .ok_or_else(|| "  the style row has no drop-down".to_owned())?;
    choice.set_selected(2);
    wait_for_model(&fixture.model)?;
    let shown = run_until(Duration::from_secs(5), || style.error().is_some());
    ensure(shown, "the row shows no error")?;
    let error = style.error().unwrap_or_default();
    ensure(
        error.contains("org.gnome.desktop.interface"),
        &format!("the row shows {error:?}"),
    )?;
    window.destroy();
    Ok(())
}

/// The key of `key` in the interface schema.
pub(crate) fn interface(key: &'static str) -> GSettingsKey {
    GSettingsKey {
        schema: INTERFACE,
        key,
    }
}

/// Apps Look & Feel with GSettings in memory: a theme set with another tool shows in its row as
/// GSettings', a style picked in the app goes to `color-scheme` as its id, and a reset puts the
/// key back to its default.
pub(crate) fn check_look_gsettings() -> Outcome {
    let store = MemoryGSettings::default();
    store
        .set(
            interface("gtk-theme"),
            &Value::Text("Elsewhere-dark".to_owned()),
        )
        .map_err(|error| error.to_string())?;
    let fixture = fixture_with_gsettings(
        SWAY,
        "version = 1\n",
        USERS,
        true,
        None,
        None,
        Some(Box::new(store.clone())),
    )?;
    let window = open(&fixture, Page::AppsLookFeel)?;
    let theme = row(&window, "look.gtk_theme")?;
    let dropdown = theme
        .dropdown()
        .ok_or_else(|| "  the theme row has no drop-down".to_owned())?;
    let shown = dropdown
        .selected_item()
        .and_downcast::<gtk::StringObject>()
        .map(|item| item.string().to_string());
    // Not installed, so it shows by its name.
    ensure(
        shown.as_deref() == Some("Elsewhere-dark"),
        &format!("the theme row shows {shown:?}, not GSettings' theme"),
    )?;
    let setting = |key| {
        Registry::builtin()
            .get(key)
            .ok_or_else(|| format!("  no setting {key}"))
    };
    let view = fixture
        .model
        .view(setting("look.gtk_theme")?, None)
        .ok_or("  no settings")?;
    ensure(view.source == Source::Desktop, "the theme isn't GSettings'")?;

    let style = row(&window, "look.style")?;
    let choice = style
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::DropDown>().ok())
        .ok_or_else(|| "  the style row has no drop-down".to_owned())?;
    // Default, Light, Dark.
    choice.set_selected(2);
    wait_for_model(&fixture.model)?;
    ensure(
        store.get(INTERFACE, "color-scheme") == Some(Value::Text("prefer-dark".to_owned())),
        "Dark didn't reach color-scheme",
    )?;
    ensure(style.error().is_none(), "the change shows an error")?;
    fixture.model.reset("look.style", None);
    wait_for_model(&fixture.model)?;
    ensure(
        store.get(INTERFACE, "color-scheme").is_none(),
        "the reset didn't reset color-scheme",
    )?;
    window.destroy();
    Ok(())
}

/// The interface font shows as it is stored, in a button that opens the font dialog.
pub(crate) fn check_look_font() -> Outcome {
    let fixture = fixture(
        SWAY,
        "version = 1\n\n[look]\nfont = \"Cantarell Bold 12\"\n",
        USERS,
        true,
    )?;
    let window = open(&fixture, Page::AppsLookFeel)?;
    let font = row(&window, "look.font")?;
    let label = font
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Button>().ok())
        .and_then(|button| button.child().and_downcast::<gtk::Label>())
        .ok_or_else(|| "  the font row has no button with a label".to_owned())?;
    ensure(
        label.text() == "Cantarell Bold 12",
        &format!("the font row shows {:?}", label.text()),
    )?;
    window.destroy();
    Ok(())
}
