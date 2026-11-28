// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Change, FollowUp, follow_ups, merged};
use crate::generate::tests::state;

#[test]
fn a_change_of_the_clipboard_history_has_its_watcher_follow_and_nothing_else() {
    let (_dir, state) = state(
        "version = 1\n\n[clipboard]\nenabled = true\n",
        "version = 1\n",
    );
    let keys = [crate::capture::HISTORY];
    let change = Change {
        keys: &keys,
        state: &state,
        config: None,
    };
    assert_eq!(
        follow_ups(&change),
        [FollowUp::SyncClipboard { on: true, max: 750 }]
    );
    let keys = [crate::registry::region::AUTOMATIC_TIMEZONE];
    let change = Change {
        keys: &keys,
        state: &state,
        config: None,
    };
    assert_eq!(follow_ups(&change), []);
}

#[test]
fn the_night_light_follows_unless_the_user_s_config_starts_one() {
    let (_dir, state) = state("version = 1\n", "version = 1\n");
    let keys = [crate::night_light::ENABLED];
    let config = crate::sway_config::parse(
        std::path::Path::new("/config"),
        &crate::sway_config::MemoryFiles::new().file("/config", "exec wlsunset\n"),
    );
    let change = Change {
        keys: &keys,
        state: &state,
        config: Some(&config),
    };
    assert!(matches!(
        follow_ups(&change).as_slice(),
        [FollowUp::SyncNightLight { external: true, .. }]
    ));
}

#[test]
fn restarting_the_portals_restarts_the_screen_sharing_too() {
    assert_eq!(
        merged(vec![
            FollowUp::RestartScreencast,
            FollowUp::ReloadUserManager,
            FollowUp::RestartPortals,
            FollowUp::ReloadUserManager,
        ]),
        [FollowUp::ReloadUserManager, FollowUp::RestartPortals]
    );
}
