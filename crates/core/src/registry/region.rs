// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings of Region & Time that Sway Things stores. The time zone, automatic time and the
//! clock are timedated's own and not stored ([`crate::region`]); the automatic time zone is
//! stored here and reaches the session through `sway-things hook timezone` in `80-hooks.conf`.

use super::{Page, Setting};
use crate::msg;

/// The key of the automatic time zone: off by default, since it asks geoclue, which asks a
/// location service on the network.
pub const AUTOMATIC_TIMEZONE: &str = "region.automatic_timezone";

pub(super) fn settings() -> Vec<Setting> {
    vec![
        Setting::toggle(AUTOMATIC_TIMEZONE, Page::RegionTime, false)
            .section(msg!("Date & Time"))
            .title(msg!("Automatic time zone"))
            .subtitle(msg!(
                "Sets the time zone where you are, at each login and while this page is open. \
                 geoclue finds your city through an online service, which sees your IP address \
                 and may see the Wi-Fi networks nearby."
            ))
            .keywords(&[
                msg!("location"),
                msg!("geoclue"),
                msg!("travel"),
                msg!("time zone"),
            ])
            .build(),
    ]
}
