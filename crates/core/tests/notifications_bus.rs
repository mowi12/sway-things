// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The notification daemons on a private bus: a stand-in for mako, swaync and dunst
//! (`notifications::stand_in`), asked and switched as the page and the command line do.
//!
//! These tests need `dbus-daemon`, so a plain `cargo test` skips them.
//! `scripts/check-integration.sh` runs them with `--ignored` in the containers.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::{StreamExt, future};
use sway_things_core::notifications::bus::Error as BusError;
use sway_things_core::notifications::stand_in::StandIn;
use sway_things_core::notifications::{Daemon, Notifications};

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
fn without_a_daemon_there_is_no_server_and_nothing_starts() -> Result<()> {
    run(async {
        let stand_in = StandIn::start()?;
        let notifications = Notifications::new(stand_in.connect().await?);
        assert_eq!(notifications.server().await?, None);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn each_daemon_s_do_not_disturb_is_read_and_switched() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start()?;
        let notifications = Notifications::new(stand_in.connect().await?);
        for daemon in [Daemon::Mako, Daemon::Swaync, Daemon::Dunst] {
            stand_in.run(daemon, false).await?;
            let server = notifications.server().await?.ok_or("no server")?;
            assert_eq!(server.daemon, daemon);
            assert!(server.pid.is_some());
            assert!(!notifications.dnd(&server).await?, "{daemon:?}");
            let mut changes = notifications.changes().await?;
            assert!(notifications.set_dnd(&server, true).await?, "{daemon:?}");
            assert!(notifications.dnd(&server).await?, "{daemon:?}");
            // Switched on again, it stays on once (mako's modes don't get it twice).
            assert!(notifications.set_dnd(&server, true).await?, "{daemon:?}");
            assert!(
                ticks(&mut changes).await,
                "no change reported by {daemon:?}"
            );
            assert!(!notifications.set_dnd(&server, false).await?, "{daemon:?}");
            notifications.reload(&server).await?;
        }
        assert_eq!(stand_in.record().reloads, 3);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn another_daemon_has_no_do_not_disturb_but_takes_notifications() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start()?;
        stand_in.run(Daemon::Other, false).await?;
        let notifications = Notifications::new(stand_in.connect().await?);
        let server = notifications.server().await?.ok_or("no server")?;
        assert_eq!(server.daemon, Daemon::Other);
        assert_eq!(server.name, "fnott");
        assert!(matches!(
            notifications.dnd(&server).await,
            Err(BusError::Unsupported(_))
        ));
        let id = notifications.notify(&server, "Test", "Body").await?;
        assert_eq!(id, 1);
        assert_eq!(
            stand_in.record().notified,
            [(
                "Sway Things".to_owned(),
                "Test".to_owned(),
                "Body".to_owned()
            )]
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn a_new_daemon_is_reported_and_found() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start()?;
        let notifications = Notifications::new(stand_in.connect().await?);
        let mut changes = notifications.changes().await?;
        stand_in.run(Daemon::Mako, true).await?;
        assert!(ticks(&mut changes).await);
        let server = notifications.server().await?.ok_or("no server")?;
        assert!(notifications.dnd(&server).await?);
        stand_in.stop().await;
        assert!(ticks(&mut changes).await);
        assert_eq!(notifications.server().await?, None);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn a_click_on_mako_s_notification_invokes_its_default_action() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start()?;
        stand_in.run(Daemon::Mako, false).await?;
        let notifications = Notifications::new(stand_in.connect().await?);
        let server = notifications.server().await?.ok_or("no server")?;
        assert_eq!(
            notifications.mako_notification(&server, 7).await?,
            Some(("Signal".to_owned(), "signal-desktop".to_owned()))
        );
        assert_eq!(notifications.mako_notification(&server, 8).await?, None);
        notifications.mako_click(&server, 7).await?;
        assert_eq!(stand_in.record().invoked, [(7, "default".to_owned())]);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn swaync_s_panel_is_closed() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start()?;
        stand_in.run(Daemon::Swaync, false).await?;
        let notifications = Notifications::new(stand_in.connect().await?);
        let server = notifications.server().await?.ok_or("no server")?;
        notifications.close_panel(&server).await?;
        assert!(stand_in.record().panel_closed);
        Ok(())
    })
}
