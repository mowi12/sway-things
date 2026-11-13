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

pub(super) fn settings() -> Vec<Setting> {
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
