// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! About & Maintenance: the system details and the project's links.

use super::*;
/// The texts About & Maintenance shows besides the versions: the labels of the system details,
/// their value for what doesn't run in tests (no bus, no sound server) and the project's links.
pub(crate) const ABOUT_TEXTS: [&str; 15] = [
    "Sway Things",
    "Compositor",
    "GTK",
    "Distribution",
    "NetworkManager",
    "Not running",
    "Sound server",
    "Kernel",
    "nm-connection-editor",
    "blueman-manager",
    "pwvucontrol",
    "pavucontrol",
    "Website",
    "Source Code (GitHub)",
    "Report a Bug",
];

/// About & Maintenance starts with the system details, a button that copies them, and the
/// project's links.
pub(crate) fn check_about() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::About)?;
    let content = window
        .content()
        .ok_or_else(|| "  About & Maintenance has no content".to_owned())?;
    let copy = all::<gtk::Button>(&content)
        .into_iter()
        .find(|button| button.label().as_deref() == Some("Copy System Details"))
        .ok_or_else(|| "  no Copy System Details button".to_owned())?;
    ensure(
        run_until(Duration::from_secs(5), || copy.is_sensitive()),
        "  the system details weren't read within 5 s",
    )?;
    let texts: Vec<String> = all::<gtk::Label>(&content)
        .iter()
        .map(|label| label.label().to_string())
        .collect();
    let gtk_version = sway_things_gui::app::gtk_version();
    let dynamic = [
        env!("CARGO_PKG_VERSION").to_owned(),
        SWAYFX.to_string(),
        gtk_version.clone(),
    ];
    for expected in ABOUT_TEXTS
        .iter()
        .copied()
        .chain(dynamic.iter().map(String::as_str))
    {
        ensure(
            texts.iter().any(|text| text == expected),
            &format!("  no {expected:?} among {texts:?}"),
        )?;
    }
    let links: Vec<String> = all::<gtk::Button>(&content)
        .iter()
        .filter_map(|button| button.tooltip_text().map(|text| text.to_string()))
        .collect();
    let theme = gtk::IconTheme::for_display(&WidgetExt::display(&window));
    for icon in [
        sway_things_gui::icons::WEBSITE,
        sway_things_gui::icons::SOURCE_CODE,
        sway_things_gui::icons::BUG,
    ] {
        ensure(
            theme.has_icon(icon),
            &format!("  the icon theme has no {icon}"),
        )?;
    }
    let repository = env!("CARGO_PKG_REPOSITORY");
    for uri in [
        env!("CARGO_PKG_HOMEPAGE").to_owned(),
        repository.to_owned(),
        format!("{repository}/issues"),
    ] {
        ensure(
            links.contains(&uri),
            &format!("  no link to {uri} among {links:?}"),
        )?;
    }
    // The copied block has the same lines, in the order of the bug form.
    copy.emit_clicked();
    let copied = std::rc::Rc::new(std::cell::RefCell::new(None));
    copy.clipboard()
        .read_text_async(gtk::gio::Cancellable::NONE, {
            let copied = copied.clone();
            move |text| {
                copied.replace(Some(text.ok().flatten().map(|text| text.to_string())));
            }
        });
    run_until(Duration::from_secs(5), || copied.borrow().is_some());
    ensure(
        copy.label().as_deref() == Some("Copied"),
        &format!("  after a click the button says {:?}", copy.label()),
    )?;
    ensure(
        run_until(Duration::from_secs(5), || {
            copy.label().as_deref() == Some("Copy System Details")
        }),
        "  the button doesn't go back to Copy System Details",
    )?;
    let copied = copied.borrow().clone().flatten().unwrap_or_default();
    let start = format!(
        "Sway Things: {}\nCompositor: {SWAYFX}\nGTK: {gtk_version}\nDistribution: ",
        env!("CARGO_PKG_VERSION")
    );
    ensure(
        copied.starts_with(&start)
            && copied
                .contains("\nNetworkManager: not running\nSound server: not running\nKernel: ")
            && copied.contains("\nnm-connection-editor: ")
            && copied.ends_with("installed\n"),
        &format!("  Copy System Details copied {copied:?}"),
    )?;
    window.destroy();
    Ok(())
}
