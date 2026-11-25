// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The automatic time zone on a private bus: stand-ins for geoclue and systemd-timedated
//! (`region::stand_in`), asked as the hook and the Region & Time page do.
//!
//! These tests need `dbus-daemon` and the system's tzdata, so a plain `cargo test` skips them.
//! `scripts/check-integration.sh` runs them with `--ignored` in the containers.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::future;
use sway_things_core::region::geo::{self, DetectError, Detected};
use sway_things_core::region::stand_in::StandIn;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Runs a test, which fails instead of hanging if the stand-in doesn't answer.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(40)).await;
        Err("the stand-in didn't answer within 40 seconds".into())
    }))
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn the_location_is_asked_at_city_level_as_sway_things() -> Result<()> {
    run(async {
        let stand_in = StandIn::start().await?;
        let place = geo::locate(&stand_in.bus()).await?;
        assert!((place.latitude - 35.68).abs() < 1e-9);
        assert!((place.longitude - 139.69).abs() < 1e-9);
        assert_eq!(
            stand_in.record().geo_client,
            ("io.github.mowi12.SwayThings".to_owned(), 4)
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn the_zone_found_is_set_only_when_it_differs() -> Result<()> {
    run(async {
        let stand_in = StandIn::start().await?;
        let timedate = stand_in.timedate().await?;
        assert_eq!(
            geo::detect(&stand_in.bus(), &timedate).await?,
            Detected::Set("Asia/Tokyo".to_owned())
        );
        assert_eq!(stand_in.record().timezone, "Asia/Tokyo");
        assert_eq!(
            geo::detect(&stand_in.bus(), &timedate).await?,
            Detected::Same("Asia/Tokyo".to_owned())
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn refusals_come_with_what_to_run() -> Result<()> {
    run(async {
        let stand_in = StandIn::start().await?;
        let timedate = stand_in.timedate().await?;
        stand_in.change(|record| record.geo_allowed = false);
        let refused = geo::detect(&stand_in.bus(), &timedate)
            .await
            .expect_err("geoclue refused");
        assert_eq!(refused, DetectError::Location(geo::Error::NotAllowed));
        assert_eq!(refused.command(), Some(geo::drop_in_command()));
        // Without the drop-in and an agent, geoclue holds `Start` instead of refusing.
        stand_in.change(|record| {
            record.geo_allowed = true;
            record.geo_waits = true;
        });
        assert_eq!(
            geo::locate(&stand_in.bus()).await,
            Err(geo::Error::NotAllowed)
        );
        stand_in.change(|record| record.geo_waits = false);
        stand_in.change(|record| {
            record.geo_allowed = true;
            record.deny = true;
        });
        let refused = geo::detect(&stand_in.bus(), &timedate)
            .await
            .expect_err("polkit refused");
        assert_eq!(
            refused.command().as_deref(),
            Some("timedatectl set-timezone Asia/Tokyo")
        );
        assert_eq!(stand_in.record().timezone, "Europe/Berlin");
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn without_geoclue_the_location_is_unavailable() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start().await?;
        stand_in.stop();
        assert_eq!(
            geo::locate(&stand_in.bus()).await,
            Err(geo::Error::Unavailable)
        );
        Ok(())
    })
}
