// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! systemd-localed on a private bus: a stand-in (`region::stand_in`), read and changed as the
//! "System default…" dialog of the Region & Time page does.
//!
//! These tests need `dbus-daemon`, so a plain `cargo test` skips them.
//! `scripts/check-integration.sh` runs them with `--ignored` in the containers.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::future;
use sway_things_core::region::localed::{Localed, SystemLocale};
use sway_things_core::region::stand_in::StandIn;
use sway_things_core::region::{Bus, Error as RegionError};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Runs a test, which fails instead of hanging if the stand-in doesn't answer.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(30)).await;
        Err("the stand-in didn't answer within 30 seconds".into())
    }))
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn the_system_locale_is_read_and_set() -> Result<()> {
    run(async {
        let stand_in = StandIn::start().await?;
        let localed = stand_in.localed().await?;
        let locale = localed.locale().await?;
        assert_eq!(locale.language, "en_US.UTF-8");
        assert_eq!(locale.formats, None);
        let wanted = SystemLocale::wanted("en_US.UTF-8", Some("de_DE.UTF-8")).ok_or("wanted")?;
        localed.set_locale(&wanted).await?;
        assert_eq!(stand_in.record().locale, wanted);
        let locale = localed.locale().await?;
        assert_eq!(locale.formats.as_deref(), Some("de_DE.UTF-8"));
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn a_refusal_gives_the_command() -> Result<()> {
    run(async {
        let stand_in = StandIn::start().await?;
        let localed = stand_in.localed().await?;
        stand_in.change(|record| record.deny = true);
        let wanted = SystemLocale::wanted("de_DE.UTF-8", None).ok_or("wanted")?;
        assert_eq!(
            localed.set_locale(&wanted).await,
            Err(RegionError::Denied("Access denied".to_owned()))
        );
        assert_eq!(stand_in.record().locale, ["LANG=en_US.UTF-8"]);
        assert_eq!(
            SystemLocale::change(&wanted).command(),
            "localectl set-locale LANG=de_DE.UTF-8"
        );
        stand_in.change(|record| record.deny = false);
        assert!(matches!(
            localed.set_locale(&["EDITOR=vim".to_owned()]).await,
            Err(RegionError::Failed(_))
        ));
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn without_localed_the_system_locale_is_unavailable() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start().await?;
        stand_in.stop();
        let localed = Localed::connect(&stand_in.bus()).await?;
        assert!(matches!(
            localed.locale().await,
            Err(RegionError::Unavailable(_))
        ));
        assert!(matches!(
            Localed::connect(&Bus::Off).await,
            Err(RegionError::Unavailable(_))
        ));
        Ok(())
    })
}
