// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preferences of Sway Things itself, stored under `[app]`.

use super::{Page, Setting};

pub(super) fn settings() -> Vec<Setting> {
    vec![
        Setting::integer("app.safety_net_timeout", Page::About, 15, 5, 120)
            .title("Safety net timeout")
            .subtitle("Seconds until a risky change is reverted unless you keep it")
            .keywords(&["revert", "countdown", "keep"])
            .build(),
    ]
}
