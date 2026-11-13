// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preferences of Sway Things itself, stored under `[app]`.

use super::{Choice, Page, Setting};

const ACCENT_SOURCES: &[Choice] = &[
    Choice::new("sway", "Sway\u{2019}s focus color"),
    Choice::new("theme", "The GTK theme\u{2019}s accent"),
];

pub(super) fn settings() -> Vec<Setting> {
    vec![
        Setting::integer("app.safety_net_timeout", Page::About, 15, 5, 120)
            .section("Sway Things")
            .title("Safety net timeout")
            .subtitle("Seconds until a risky change is reverted unless you keep it")
            .keywords(&["revert", "countdown", "keep"])
            .build(),
        Setting::choice("app.accent_source", Page::About, "sway", ACCENT_SOURCES)
            .section("Sway Things")
            .title("Accent color")
            .subtitle("The color of switches, sliders and selections in this app")
            .keywords(&["theme", "highlight", "color"])
            .build(),
        Setting::toggle("app.show_directives", Page::About, false)
            .section("Sway Things")
            .title("Show sway directives")
            .subtitle("Show the line of the sway config below each setting")
            .keywords(&["config", "directive", "monospace", "learn"])
            .build(),
        Setting::toggle("app.remember_collapsed_sections", Page::About, true)
            .section("Sway Things")
            .title("Remember collapsed sections")
            .subtitle("Keep sections you folded away folded when the app starts again")
            .keywords(&["collapse", "fold", "expand", "sections"])
            .build(),
    ]
}
