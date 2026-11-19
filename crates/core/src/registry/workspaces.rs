// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Workspaces page: how the numbered workspaces are placed on the displays, and
//! how many are placed.
//!
//! Neither has a sway directive of its own: the placement depends on the displays connected, so
//! `sway-things hook workspaces` sends it when sway starts or reloads and whenever kanshi applies
//! a profile, and the page sends it at once ([`crate::outputs::workspaces`]). A fixed placement
//! is remembered per combination of displays in `displays.toml`.

use super::{Apply, Choice, Page, Setting, When};
use crate::msg;

/// The key of the placement rule.
pub const PLACEMENT: &str = "workspaces.placement";
/// The key of the number of workspaces placed.
pub const COUNT: &str = "workspaces.count";

/// How the workspaces are placed, by the id of the choice.
pub const PLACEMENTS: &[Choice] = &[
    Choice::new("sway", msg!("Off (sway decides)")),
    Choice::new("round_robin", msg!("Round robin, left to right")),
    Choice::new("blocks", msg!("A block per display")),
    Choice::new(
        "one_display",
        msg!("All on one display (the laptop's, or the leftmost)"),
    ),
    Choice::new("fixed", msg!("Fixed, as you arrange them")),
];

/// The most workspaces that are placed.
pub const MAX_COUNT: i64 = 20;

pub(super) fn settings() -> Vec<Setting> {
    vec![
        Setting::choice(PLACEMENT, Page::Workspaces, "sway", PLACEMENTS)
            .section(msg!("Placement"))
            .title(msg!("Place workspaces"))
            .subtitle(msg!(
                "Which display each number goes to. Open workspaces move at once, the others with \
                 the reload; worked out again when sway starts, and with kanshi whenever displays \
                 come and go"
            ))
            .apply(Apply::Reload)
            .keywords(&[
                msg!("workspace output"),
                msg!("monitor"),
                msg!("display"),
                msg!("screen"),
                msg!("assign"),
                msg!("pin"),
                msg!("round robin"),
            ])
            .build(),
        Setting::integer(COUNT, Page::Workspaces, 10, 1, MAX_COUNT)
            .section(msg!("Placement"))
            .title(msg!("Workspaces placed"))
            .subtitle(msg!(
                "Workspaces 1 to this number. The default keys ($mod+1 to $mod+0) reach 1 to 10"
            ))
            .apply(Apply::Reload)
            .slider(1.0, 20.0, 1.0)
            .keywords(&[msg!("number of workspaces"), msg!("how many")])
            .depends_on(PLACEMENT, When::NoneOf(&["sway"]))
            .build(),
    ]
}
