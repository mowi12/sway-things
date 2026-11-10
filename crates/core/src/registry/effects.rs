// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SwayFX effects on the Windows page.

use super::{ConfigFile, Directive, Page, Setting};
use crate::command::effects;
use crate::version::SwayFxVersion;

const CORNER_RADIUS: &str = "effects.corner_radius";

pub(super) fn settings() -> Vec<Setting> {
    vec![
        Setting::integer(CORNER_RADIUS, Page::Windows, 0, 0, 99)
            .title("Corner radius")
            .subtitle("Rounded window corners, in pixels")
            .keywords(&["rounded", "round corners"])
            .swayfx_only(SwayFxVersion::OLDEST_SUPPORTED)
            .sway(
                Directive::top("corner_radius"),
                ConfigFile::Effects,
                |ctx| Ok(vec![effects::corner_radius(ctx.integer(CORNER_RADIUS)?)?]),
            )
            .build(),
    ]
}
