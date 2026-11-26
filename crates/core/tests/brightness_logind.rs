// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Setting a backlight through logind's session on a private bus (`brightness::stand_in`).
//!
//! These tests need `dbus-daemon`, so a plain `cargo test` skips them.
//! `scripts/check-integration.sh` runs them with `--ignored` in the containers.

use std::error::Error;

use async_io::block_on;
use sway_things_core::brightness::stand_in::StandIn;
use sway_things_core::brightness::{Device, Kind};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn logind_is_asked_for_the_raw_value_within_the_limits() -> Result<()> {
    block_on(async {
        let stand_in = StandIn::start().await?;
        let logind = stand_in.logind();
        let panel = Device {
            name: "intel_backlight".into(),
            kind: Kind::Raw,
            brightness: 600,
            max: 1200,
        };
        logind.set(&panel, 0).await?;
        logind.set(&panel, 900).await?;
        let keyboard = Device {
            name: "tpacpi::kbd_backlight".into(),
            kind: Kind::Keyboard,
            brightness: 1,
            max: 2,
        };
        logind.set(&keyboard, 0).await?;
        assert_eq!(
            stand_in.calls(),
            [
                ("backlight".into(), "intel_backlight".into(), 12),
                ("backlight".into(), "intel_backlight".into(), 900),
                ("leds".into(), "tpacpi::kbd_backlight".into(), 0),
            ]
        );
        Ok(())
    })
}
