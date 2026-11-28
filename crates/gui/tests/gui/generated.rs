// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Generated config window, its footers and banners.

use super::*;
/// The checks of the window of "Show generated config".
pub(crate) fn check_generated() -> Vec<(String, Outcome)> {
    vec![
        (
            "generated: a footer opens its file and follows changes".to_owned(),
            check_generated_footer(),
        ),
        (
            "generated: About & Maintenance opens it".to_owned(),
            check_generated_about(),
        ),
        (
            "generated: a file changed outside can be seen and overwritten".to_owned(),
            check_generated_edited(),
        ),
        (
            "generated: a foreign file and a read-only one aren't overwritten".to_owned(),
            check_generated_not_overwritten(),
        ),
    ]
}

/// The buttons labeled `label` in the open page of `window`.
pub(crate) fn buttons_labeled(window: &Window, label: &str) -> Vec<gtk::Button> {
    window
        .content()
        .map(|content| all::<gtk::Button>(&content))
        .unwrap_or_default()
        .into_iter()
        .filter(|button| button.label().as_deref() == Some(label))
        .collect()
}

/// The text the generated window shows for `file`, and its banner.
pub(crate) fn generated_state(
    window: &Window,
    file: ConfigFile,
) -> Result<(String, Option<String>, bool), String> {
    window
        .generated()
        .and_then(|generated| generated.file_state(file))
        .ok_or_else(|| format!("  no tab for {}", file.file_name()))
}

/// The state of `file`'s tab once `shown` holds for it: the window reads the files on disk on
/// a thread of its own, so a refresh lands a moment later. The last state after five seconds.
pub(crate) fn generated_state_when(
    window: &Window,
    file: ConfigFile,
    shown: impl Fn(&(String, Option<String>, bool)) -> bool,
) -> Result<(String, Option<String>, bool), String> {
    run_until(Duration::from_secs(5), || {
        generated_state(window, file).is_ok_and(|state| shown(&state))
    });
    generated_state(window, file)
}

/// The Show button of the Windows page footer opens 30-appearance.conf with the stored colors;
/// a change shows at once.
pub(crate) fn check_generated_footer() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::Windows)?;
    let shows = buttons_labeled(&window, "Show");
    ensure(
        shows.len() == 2,
        &format!(
            "  the Windows footer has {} Show buttons, expected 2",
            shows.len()
        ),
    )?;
    shows[0].emit_clicked();
    let generated = window
        .generated()
        .ok_or_else(|| "  Show opened no window".to_owned())?;
    settle(&generated)?;
    ensure(
        generated.shown_file() == Some(ConfigFile::Appearance),
        &format!("  it opened at {:?}", generated.shown_file()),
    )?;
    let (text, banner, _) =
        generated_state_when(&window, ConfigFile::Appearance, |(text, _, _)| {
            text.contains("client.focused ")
        })?;
    ensure(
        text.contains("client.focused ") && text.contains("#3b6e4f"),
        &format!("  30-appearance.conf shows {text:?}"),
    )?;
    ensure(banner.is_none(), "a file that isn't written has a banner")?;
    // Copy contents copies the text, not the path.
    let copy = all::<gtk::Button>(generated.upcast_ref())
        .into_iter()
        .find(|button| {
            button.is_mapped() && button.tooltip_text().as_deref() == Some("Copy contents")
        })
        .ok_or_else(|| "  no copy button".to_owned())?;
    copy.emit_clicked();
    let copied = std::rc::Rc::new(std::cell::RefCell::new(None));
    generated
        .clipboard()
        .read_text_async(gtk::gio::Cancellable::NONE, {
            let copied = copied.clone();
            move |text| {
                copied.replace(Some(text.ok().flatten().map(|text| text.to_string())));
            }
        });
    run_until(Duration::from_secs(5), || copied.borrow().is_some());
    let copied = copied.borrow().clone().flatten();
    ensure(
        copied.as_deref() == Some(text.as_str()),
        &format!("  Copy contents copied {copied:?}"),
    )?;
    // Nothing was written to show it.
    ensure(
        !fixture.locations.path(ConfigFile::Appearance).exists(),
        "showing wrote the file",
    )?;
    fixture
        .model
        .set("windows.gaps.inner", None, Value::Integer(9));
    wait_for_model(&fixture.model)?;
    let (text, _, _) = generated_state_when(&window, ConfigFile::Appearance, |(text, _, _)| {
        text.contains("gaps inner 9")
    })?;
    ensure(
        text.contains("gaps inner 9"),
        &format!("  after a change it shows {text:?}"),
    )?;
    // The other button brings the same window to 90-effects.conf.
    shows[1].emit_clicked();
    ensure(
        window.generated().as_ref() == Some(&generated)
            && generated.shown_file() == Some(ConfigFile::Effects),
        "the second Show didn't switch the open window to 90-effects.conf",
    )?;
    generated.close();
    run_until(Duration::from_millis(100), || false);
    ensure(
        window.generated().is_none(),
        "the window stays after closing",
    )?;
    window.destroy();
    Ok(())
}

/// About & Maintenance has a section with a button that opens the window.
pub(crate) fn check_generated_about() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::About)?;
    let shows = buttons_labeled(&window, "Show\u{2026}");
    let show = shows
        .first()
        .ok_or_else(|| "  About & Maintenance has no Show… button".to_owned())?;
    show.emit_clicked();
    let generated = window
        .generated()
        .ok_or_else(|| "  Show… opened no window".to_owned())?;
    settle(&generated)?;
    ensure(
        generated.shown_file() == Some(ConfigFile::Input),
        &format!("  it opened at {:?}", generated.shown_file()),
    )?;
    window.destroy();
    run_until(Duration::from_millis(100), || false);
    let open = gtk::Window::list_toplevels()
        .iter()
        .any(|toplevel| toplevel == generated.upcast_ref::<gtk::Widget>());
    ensure(
        !open,
        &format!(
            "  the window stays after the main window is gone (visible: {}, mapped: {})",
            generated.is_visible(),
            generated.is_mapped()
        ),
    )?;
    Ok(())
}

/// A generated file with the header but other content was changed outside: banner, the content
/// on disk on request, and Overwrite writes what Sway Things generates.
pub(crate) fn check_generated_edited() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let path = fixture.locations.path(ConfigFile::Appearance);
    let edited = format!("{}gaps inner 30\n", sway_things_core::manifest::HEADER);
    write_file(&path, &edited)?;
    let window = open(&fixture, Page::Windows)?;
    window.show_generated(Some(ConfigFile::Appearance));
    let generated = window
        .generated()
        .ok_or_else(|| "  no generated window".to_owned())?;
    settle(&generated)?;
    let (text, banner, can_overwrite) =
        generated_state_when(&window, ConfigFile::Appearance, |(_, banner, _)| {
            banner.is_some()
        })?;
    ensure(
        banner
            .as_deref()
            .is_some_and(|banner| banner.starts_with("Changed outside")),
        &format!("  the banner says {banner:?}"),
    )?;
    ensure(can_overwrite, "Overwrite can't be pressed")?;
    ensure(
        !text.contains("gaps inner 30"),
        "it shows the file on disk at first",
    )?;
    let on_disk = generated
        .on_disk_button(ConfigFile::Appearance)
        .ok_or_else(|| "  no Show file on disk button".to_owned())?;
    on_disk.set_active(true);
    let (text, _, _) = generated_state_when(&window, ConfigFile::Appearance, |(text, _, _)| {
        *text == edited
    })?;
    ensure(text == edited, &format!("  on disk it shows {text:?}"))?;
    on_disk.set_active(false);

    fixture.model.overwrite(ConfigFile::Appearance);
    wait_for_model(&fixture.model)?;
    let written = std::fs::read_to_string(&path).map_err(|error| format!("  {error}"))?;
    ensure(
        written.contains("client.focused ") && !written.contains("gaps inner 30"),
        &format!("  the file holds {written:?} after Overwrite"),
    )?;
    let (_, banner, _) =
        generated_state_when(&window, ConfigFile::Appearance, |(_, banner, _)| {
            banner.is_none()
        })?;
    ensure(banner.is_none(), "the banner stays after Overwrite")?;
    window.destroy();
    Ok(())
}

/// A file without the header isn't Sway Things', so it is never overwritten; without the
/// include line an edited file can't be overwritten either.
pub(crate) fn check_generated_not_overwritten() -> Outcome {
    let foreign = fixture(SWAYFX, STORED, USERS, true)?;
    write_file(
        &foreign.locations.path(ConfigFile::Behavior),
        "focus_follows_mouse no\n",
    )?;
    let window = open(&foreign, Page::WindowBehavior)?;
    window.show_generated(Some(ConfigFile::Behavior));
    let (_, banner, can_overwrite) =
        generated_state_when(&window, ConfigFile::Behavior, |(_, banner, _)| {
            banner.is_some()
        })?;
    ensure(
        banner
            .as_deref()
            .is_some_and(|banner| banner.starts_with("Not made by Sway Things")),
        &format!("  the banner of a foreign file says {banner:?}"),
    )?;
    ensure(!can_overwrite, "a foreign file can be overwritten")?;
    window.destroy();

    let read_only = fixture(SWAYFX, STORED, USERS, false)?;
    write_file(
        &read_only.locations.path(ConfigFile::Input),
        &format!(
            "{}input * tap enabled\n",
            sway_things_core::manifest::HEADER
        ),
    )?;
    let window = open(&read_only, Page::MouseTouchpad)?;
    window.look_around();
    window.show_generated(Some(ConfigFile::Input));
    let (_, banner, can_overwrite) =
        generated_state_when(&window, ConfigFile::Input, |(_, banner, _)| {
            banner.is_some()
        })?;
    ensure(
        banner.is_some(),
        "an edited file has no banner while read-only",
    )?;
    ensure(!can_overwrite, "Overwrite can be pressed while read-only")?;
    window.destroy();
    Ok(())
}

/// Writes `text` to `path`, with the directories it needs.
pub(crate) fn write_file(path: &Path, text: &str) -> Outcome {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("  {error}"))?;
    }
    std::fs::write(path, text).map_err(|error| format!("  {error}"))
}

/// The generated window with a file changed outside, checked for contrast.
pub(crate) fn check_generated_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    write_file(
        &fixture.locations.path(ConfigFile::Appearance),
        &format!("{}gaps inner 30\n", sway_things_core::manifest::HEADER),
    )?;
    let window = open(&fixture, Page::Windows)?;
    window.show_generated(Some(ConfigFile::Appearance));
    let generated = window
        .generated()
        .ok_or_else(|| "  no generated window".to_owned())?;
    settle(&generated)?;
    wait_for_paint(&generated)?;
    let path = screenshots.map(|dir| dir.join(format!("generated--{scheme}.png")));
    let problems = check_screen(&generated, path)?;
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}
