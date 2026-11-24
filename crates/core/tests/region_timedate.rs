// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! systemd-timedated on a private bus: a stand-in (`region::stand_in`), read and changed as the
//! Region & Time page does.
//!
//! These tests need `dbus-daemon`, so a plain `cargo test` skips them.
//! `scripts/check-integration.sh` runs them with `--ignored` in the containers.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::{StreamExt, future};
use sway_things_core::region::stand_in::StandIn;
use sway_things_core::region::{Bus, Error as RegionError, Timedate};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Runs a test, which fails instead of hanging if the stand-in doesn't answer.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(30)).await;
        Err("the stand-in didn't answer within 30 seconds".into())
    }))
}

/// Waits for the next tick of `changes`, at most five seconds.
async fn ticks(changes: &mut (impl futures_lite::Stream<Item = ()> + Unpin)) -> bool {
    future::or(async { changes.next().await.is_some() }, async {
        Timer::after(Duration::from_secs(5)).await;
        false
    })
    .await
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn the_clock_and_the_zones_are_read() -> Result<()> {
    run(async {
        let stand_in = StandIn::start().await?;
        let timedate = stand_in.timedate().await?;
        let clock = timedate.clock().await?;
        assert_eq!(clock.timezone, "Europe/Berlin");
        assert!(clock.ntp && clock.can_ntp && clock.synchronized);
        assert_eq!(
            timedate.zones().await?,
            ["America/New_York", "Asia/Tokyo", "Europe/Berlin", "UTC"]
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn changes_are_made_and_reported() -> Result<()> {
    run(async {
        let stand_in = StandIn::start().await?;
        let timedate = stand_in.timedate().await?;
        let mut changes = timedate.changes().await?;
        timedate.set_timezone("Asia/Tokyo").await?;
        assert!(ticks(&mut changes).await, "no change for the time zone");
        assert_eq!(timedate.clock().await?.timezone, "Asia/Tokyo");
        timedate.set_ntp(false).await?;
        assert!(ticks(&mut changes).await, "no change for automatic time");
        assert!(!timedate.clock().await?.ntp);
        timedate.set_time(1_795_552_611_000_000).await?;
        assert_eq!(stand_in.record().times, [1_795_552_611_000_000]);
        stand_in.set_timezone_elsewhere("UTC").await?;
        assert!(ticks(&mut changes).await, "no change from elsewhere");
        assert_eq!(timedate.clock().await?.timezone, "UTC");
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn refusals_say_why() -> Result<()> {
    run(async {
        let stand_in = StandIn::start().await?;
        let timedate = stand_in.timedate().await?;
        assert!(matches!(
            timedate.set_timezone("Mars/Olympus_Mons").await,
            Err(RegionError::Failed(_))
        ));
        // Automatic time is on.
        assert!(matches!(
            timedate.set_time(0).await,
            Err(RegionError::Failed(_))
        ));
        stand_in.change(|record| record.deny = true);
        assert_eq!(
            timedate.set_timezone("UTC").await,
            Err(RegionError::Denied("Access denied".to_owned()))
        );
        assert_eq!(stand_in.record().timezone, "Europe/Berlin");
        stand_in.change(|record| {
            record.deny = false;
            record.can_ntp = false;
        });
        assert!(matches!(
            timedate.set_ntp(true).await,
            Err(RegionError::Failed(_))
        ));
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn without_timedated_the_clock_is_unavailable() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start().await?;
        stand_in.stop();
        let timedate = Timedate::connect(&stand_in.bus()).await?;
        assert!(matches!(
            timedate.clock().await,
            Err(RegionError::Unavailable(_))
        ));
        assert!(matches!(
            Timedate::connect(&Bus::Off).await,
            Err(RegionError::Unavailable(_))
        ));
        Ok(())
    })
}
