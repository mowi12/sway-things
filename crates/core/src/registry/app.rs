// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preferences of Sway Things itself, stored under `[app]`.

use super::{Choice, Page, Setting};
use crate::editor;
use crate::msg;

const ACCENT_SOURCES: &[Choice] = &[
    Choice::new("sway", msg!("Sway\u{2019}s focus color")),
    Choice::new("theme", msg!("The GTK theme\u{2019}s accent")),
];

const SEARCH_MODES: &[Choice] = &[
    Choice::new("automatic", msg!("Automatic")),
    Choice::new("manual", msg!("Manual")),
];

/// The folder whose pictures the Wallpaper page shows; empty for `~/Pictures/Wallpapers`, or
/// the system's backgrounds without that folder.
pub const WALLPAPER_FOLDER: &str = "app.wallpaper_folder";

pub(super) fn settings() -> Vec<Setting> {
    let mut all = general();
    all.extend(page_preferences());
    all
}

/// The preferences that apply to the whole app.
fn general() -> Vec<Setting> {
    vec![
        Setting::integer("app.safety_net_timeout", Page::About, 15, 5, 120)
            .section(msg!("Sway Things"))
            .title(msg!("Safety net timeout"))
            .subtitle(msg!(
                "Seconds until a risky change is reverted unless you keep it"
            ))
            .keywords(&[msg!("revert"), msg!("countdown"), msg!("keep")])
            .build(),
        Setting::choice("app.accent_source", Page::About, "sway", ACCENT_SOURCES)
            .section(msg!("Sway Things"))
            .title(msg!("Accent color"))
            .subtitle(msg!(
                "The color of switches, sliders and selections in this app"
            ))
            .keywords(&[msg!("theme"), msg!("highlight"), msg!("color")])
            .build(),
        Setting::text(editor::KEY, Page::About, "")
            .section(msg!("Sway Things"))
            .title(msg!("Editor command"))
            .subtitle(msg!(
                "The program that opens your sway config, with its arguments; {file} and {line} \
                 are filled in"
            ))
            .placeholder(msg!("$VISUAL, $EDITOR or the default text editor"))
            .keywords(&[
                msg!("editor"),
                msg!("text editor"),
                msg!("open"),
                msg!("vim"),
                msg!("terminal"),
            ])
            .check_text(editor::check)
            .build(),
        Setting::toggle("app.show_directives", Page::About, false)
            .section(msg!("Sway Things"))
            .title(msg!("Show sway directives"))
            .subtitle(msg!("Show the line of the sway config below each setting"))
            .keywords(&[
                msg!("config"),
                msg!("directive"),
                msg!("monospace"),
                msg!("learn"),
            ])
            .build(),
        Setting::toggle("app.remember_collapsed_sections", Page::About, true)
            .section(msg!("Sway Things"))
            .title(msg!("Remember collapsed sections"))
            .subtitle(msg!(
                "Keep sections you folded away folded when the app starts again"
            ))
            .keywords(&[
                msg!("collapse"),
                msg!("fold"),
                msg!("expand"),
                msg!("sections"),
            ])
            .build(),
    ]
}

/// The preferences of single pages.
fn page_preferences() -> Vec<Setting> {
    vec![
        Setting::toggle("app.sound_above_100", Page::About, false)
            .section(msg!("Sway Things"))
            .title(msg!("Allow volume above 100 %"))
            .subtitle(msg!(
                "Let the volume sliders on the Sound page go up to 150 %, which can distort the \
                 sound"
            ))
            .keywords(&[
                msg!("amplify"),
                msg!("boost"),
                msg!("loud"),
                msg!("over-amplification"),
                msg!("volume"),
            ])
            .build(),
        Setting::choice(
            "app.bluetooth_search",
            Page::About,
            "automatic",
            SEARCH_MODES,
        )
        .section(msg!("Sway Things"))
        .title(msg!("Search for Bluetooth devices"))
        .subtitle(msg!(
            "Automatically while the Bluetooth page is open, or only when you press Search"
        ))
        .keywords(&[msg!("discovery"), msg!("scan"), msg!("bluetooth")])
        .build(),
        Setting::choice("app.wifi_search", Page::About, "automatic", SEARCH_MODES)
            .section(msg!("Sway Things"))
            .title(msg!("Search for Wi-Fi networks"))
            .subtitle(msg!(
                "Automatically every 15 seconds while the Network page is open, or only when you \
                 press Search"
            ))
            .keywords(&[msg!("scan"), msg!("wifi"), msg!("wireless")])
            .build(),
        Setting::text(WALLPAPER_FOLDER, Page::About, "")
            .section(msg!("Sway Things"))
            .title(msg!("Wallpaper folder"))
            .subtitle(msg!("The pictures the Wallpaper page offers"))
            .placeholder(msg!("~/Pictures/Wallpapers or the system’s backgrounds"))
            .keywords(&[msg!("pictures"), msg!("background"), msg!("folder")])
            .build(),
    ]
}
