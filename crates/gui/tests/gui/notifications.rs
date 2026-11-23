// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Notifications page against a stand-in notification daemon.

use super::*;
pub(crate) fn check_notifications() -> Vec<(String, Outcome)> {
    vec![(
        "notifications: Do Not Disturb follows the daemon and switches it".to_owned(),
        check_notifications_dnd(),
    )]
}

/// The Notifications page against a stand-in swaync on a private bus: the switch shows its Do
/// Not Disturb, switches it and follows a change made elsewhere, the test notification reaches
/// it, and only swaync's sections show.
fn check_notifications_dnd() -> Outcome {
    use sway_things_core::notifications::files::Files;
    use sway_things_core::notifications::stand_in::StandIn as DaemonStandIn;
    use sway_things_core::notifications::{Daemon, Notifications};
    let mut stand_in = DaemonStandIn::start().map_err(|error| error.to_string())?;
    async_io::block_on(stand_in.run(Daemon::Swaync, false)).map_err(|error| error.to_string())?;
    let page_bus = async_io::block_on(stand_in.connect()).map_err(|error| error.to_string())?;
    let test_bus = Notifications::new(
        async_io::block_on(stand_in.connect()).map_err(|error| error.to_string())?,
    );
    let server = async_io::block_on(test_bus.server())
        .map_err(|error| error.to_string())?
        .ok_or("  no stand-in daemon")?;
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let paths = Paths::new(
        &dir.path().join("config"),
        &dir.path().join("state"),
        "host",
    )
    .map_err(|error| error.to_string())?;
    let locations = Locations::new(
        &dir.path().join("sway/sway-things"),
        &dir.path().join("state"),
    );
    let mut applier = Applier::without_session(
        State::load(Registry::builtin(), paths),
        locations.clone(),
        SwayValidator::with_program("true"),
        SWAY,
    );
    let home = dir.path().join("home");
    applier.set_daemon_files(
        Files::new(Some(home.clone()), &home.join(".config")),
        Some(Notifications::new(page_bus)),
    );
    let main = dir.path().join("sway/config");
    let files = MemoryFiles::new().file(main.to_string_lossy().as_ref(), "");
    let status = IncludeStatus::detect(Some(&main), &files, &locations);
    let model = Model::with_applier(applier, status);
    let window = Window::with_model(&model);
    window.present();
    window.show_page(Page::Notifications);
    settle(&window)?;
    let content = window.content().ok_or("  no content pane")?;
    let switch = all::<gtk::Switch>(&content)
        .into_iter()
        .next()
        .ok_or("  the page has no switch")?;
    let ready = run_until(Duration::from_secs(5), || switch.is_sensitive());
    ensure(ready, "the Do Not Disturb switch never became usable")?;
    switch.set_active(true);
    let on = run_until(Duration::from_secs(5), || {
        async_io::block_on(test_bus.dnd(&server)).unwrap_or(false)
    });
    ensure(on, "the switch didn't turn swaync's Do Not Disturb on")?;
    async_io::block_on(test_bus.set_dnd(&server, false)).map_err(|error| error.to_string())?;
    let followed = run_until(Duration::from_secs(8), || !switch.is_active());
    ensure(followed, "the switch didn't follow swaync's Do Not Disturb")?;
    let test = buttons_labeled(&window, "Send Test Notification");
    let test = test.first().ok_or("  no Send Test Notification button")?;
    test.emit_clicked();
    let sent = run_until(Duration::from_secs(5), || {
        !stand_in.record().notified.is_empty()
    });
    ensure(sent, "the test notification didn't reach swaync")?;
    let shown = |key: &str| row(&window, key).map(|row| row.is_mapped());
    let sections = run_until(Duration::from_secs(5), || {
        shown("notifications.swaync.width") == Ok(true)
            && shown("notifications.mako.width") == Ok(false)
    });
    ensure(sections, "the page doesn't show swaync's sections only")?;
    window.destroy();
    Ok(())
}
