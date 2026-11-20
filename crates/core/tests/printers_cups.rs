// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The printers backend against a real `cupsd` of its own, with a fake driverless printer on the
//! loopback address, and stand-ins for Avahi and cups-pk-helper on a private bus.
//!
//! These tests need `cupsd`, `dbus-daemon` and python-dbusmock, so a plain `cargo test` skips
//! them. `scripts/check-integration.sh` runs them with `--ignored` in the containers, which have
//! all three.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::future;
use sway_things_core::printers::stand_in::{self, FakePrinter, Services, StandIn};
use sway_things_core::printers::{
    Bus, Cups, Discovered, Discovery, Endpoint, Error as PrintError, JobState, Level, NewPrinter,
    PrinterState, ReasonKind, Severity, Snapshot, queue_name,
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// A printer CUPS knows from before: paused, low on toner, with two supplies.
const SEEDED: &str = "\
<Printer Seeded>
Info Front Desk
Location Hall
DeviceURI ipp://127.0.0.1:9/ipp/print
State Stopped
StateMessage Paused by the test
Reason paused
Reason toner-low-warning
Type 4
Accepting Yes
Shared No
Attribute marker-colors \\#000000,\\#00FFFF\\#FF00FF\\#FFFF00
Attribute marker-levels 7,55
Attribute marker-low-levels 10,10
Attribute marker-high-levels 100,100
Attribute marker-names Black Toner,Color Toner
Attribute marker-types toner,toner
</Printer>
";

/// Runs a test, which fails instead of hanging.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(90)).await;
        Err("the test didn't finish within 90 seconds".into())
    }))
}

/// The next change `discovery` reports, within ten seconds.
async fn next(discovery: &mut Discovery) -> Result<Discovered> {
    future::or(
        async {
            discovery
                .next()
                .await
                .ok_or_else(|| "Avahi went away".into())
        },
        async {
            Timer::after(Duration::from_secs(10)).await;
            Err("nothing found".into())
        },
    )
    .await
}

/// Waits until the snapshot satisfies `done`.
async fn until(cups: &Cups, done: impl Fn(&Snapshot) -> bool) -> Result<Snapshot> {
    for _ in 0..100 {
        let snapshot = cups.snapshot().await?;
        if done(&snapshot) {
            return Ok(snapshot);
        }
        Timer::after(Duration::from_millis(100)).await;
    }
    Err(format!("the snapshot didn't change: {:?}", cups.snapshot().await?).into())
}

#[test]
#[ignore = "needs cupsd; run scripts/check-integration.sh"]
fn an_empty_cups_has_no_printers() -> Result<()> {
    run(async {
        let stand_in = StandIn::start(true)?;
        let cups = stand_in.cups();
        let snapshot = cups.snapshot().await?;
        assert_eq!(snapshot.printers, []);
        assert_eq!(snapshot.jobs, []);
        assert_eq!(snapshot.default, None);
        let version = cups.version().await.ok_or("no version")?;
        assert!(version.starts_with('2'), "{version}");
        Ok(())
    })
}

#[test]
#[ignore = "needs cupsd; run scripts/check-integration.sh"]
fn a_printer_shows_its_state_and_supplies() -> Result<()> {
    run(async {
        let stand_in = StandIn::start_with(true, SEEDED)?;
        let snapshot = stand_in.cups().snapshot().await?;
        let printer = snapshot.printer("seeded").ok_or("no printer")?;
        assert_eq!(printer.name, "Seeded");
        assert_eq!(printer.title(), "Front Desk");
        assert_eq!(printer.location, "Hall");
        assert_eq!(printer.state, PrinterState::Stopped);
        assert!(printer.paused());
        assert!(
            printer
                .reasons
                .iter()
                .any(|r| r.kind == ReasonKind::TonerLow && r.severity == Severity::Warning),
            "{:?}",
            printer.reasons
        );
        assert_eq!(printer.status().english(), "Paused · Toner low");
        assert_eq!(printer.markers.len(), 2, "{:?}", printer.markers);
        assert_eq!(printer.markers[0].name, "Black Toner");
        assert_eq!(printer.markers[0].level, Level::Percent(7));
        assert!(printer.markers[0].needs_attention());
        assert_eq!(printer.markers[1].colors.len(), 3);
        assert_eq!(printer.markers[1].level, Level::Percent(55));
        Ok(())
    })
}

#[test]
#[ignore = "needs cupsd; run scripts/check-integration.sh"]
fn a_driverless_printer_is_added_used_and_removed() -> Result<()> {
    run(async {
        let stand_in = StandIn::start(true)?;
        let fake = FakePrinter::start("Acme Laser 9000")?;
        let cups = stand_in.cups();
        let name = queue_name("Acme Laser 9000", &[]);
        let added = cups
            .add(&NewPrinter {
                name: name.clone(),
                uri: fake.uri().to_owned(),
                info: "Acme Laser 9000".to_owned(),
                location: "Office".to_owned(),
            })
            .await
            .map_err(|error| format!("{error}; CUPS's log:\n{}", stand_in.log()))?;
        assert_eq!(added, name);
        let snapshot = cups.snapshot().await?;
        let printer = snapshot.printer(&name).ok_or("not added")?;
        assert_eq!(printer.title(), "Acme Laser 9000");
        assert_eq!(printer.location, "Office");
        // The driver CUPS made names the model as the printer says (2.4.20), or not (2.4.10).
        assert!(
            printer.model.contains("IPP Everywhere"),
            "{}",
            printer.model
        );
        assert_eq!(printer.device_uri, fake.uri());
        assert!(printer.accepting);
        assert!(!printer.paused());

        cups.set_default(&name).await?;
        cups.set_info(&name, "Big Laser").await?;
        cups.set_location(&name, "Room 2").await?;
        cups.set_shared(&name, true).await?;
        let snapshot = cups.snapshot().await?;
        assert_eq!(snapshot.default.as_deref(), Some(name.as_str()));
        let printer = snapshot.printer(&name).ok_or("gone")?;
        assert_eq!(printer.title(), "Big Laser");
        assert_eq!(printer.location, "Room 2");
        assert!(printer.shared);

        // Paused, the test page waits in the queue until it is canceled.
        cups.pause(&name).await?;
        let id = cups.print_test_page(&name).await?;
        let snapshot = until(&cups, |s| s.jobs_of(&name).any(|job| job.id == id)).await?;
        assert!(snapshot.printer(&name).ok_or("gone")?.paused());
        let job = snapshot
            .jobs
            .iter()
            .find(|job| job.id == id)
            .ok_or("no job")?;
        assert_eq!(job.state, JobState::Pending);
        assert_eq!(job.title, "Test Page");
        assert!(job.created.is_some());
        cups.cancel_job(id).await?;
        until(&cups, |s| s.jobs.is_empty()).await?;

        // Resumed, the next one reaches the printer.
        cups.resume(&name).await?;
        let snapshot = cups.snapshot().await?;
        assert!(!snapshot.printer(&name).ok_or("gone")?.paused());
        cups.print_test_page(&name).await?;
        for _ in 0..100 {
            if fake.jobs() > 0 {
                break;
            }
            Timer::after(Duration::from_millis(100)).await;
        }
        assert_eq!(fake.jobs(), 1, "CUPS's log:\n{}", stand_in.log());

        cups.remove(&name).await?;
        let snapshot = cups.snapshot().await?;
        assert!(snapshot.printer(&name).is_none());
        assert_eq!(snapshot.default, None);
        Ok(())
    })
}

#[test]
#[ignore = "needs cupsd; run scripts/check-integration.sh"]
fn adding_never_touches_a_queue_that_was_there() -> Result<()> {
    run(async {
        let stand_in = StandIn::start_with(true, SEEDED)?;
        let cups = stand_in.cups();
        // The name was taken after the caller chose it: a printer that isn't there fails
        // under a name of its own, and the queue of that name stays as it was.
        let error = cups
            .add(&NewPrinter {
                name: "Seeded".to_owned(),
                uri: "ipp://127.0.0.1:9/ipp/print".to_owned(),
                info: "Other".to_owned(),
                location: String::new(),
            })
            .await
            .err()
            .ok_or("added a printer that isn't there")?;
        assert!(matches!(error, PrintError::Failed(_)), "{error:?}");
        let snapshot = cups.snapshot().await?;
        let seeded = snapshot.printer("Seeded").ok_or("the queue was removed")?;
        assert_eq!(seeded.title(), "Front Desk");
        assert_eq!(seeded.device_uri, "ipp://127.0.0.1:9/ipp/print");
        assert!(snapshot.printer("Seeded_2").is_none());
        // A printer that is there gets the free name.
        let fake = FakePrinter::start("Acme Laser 9000")?;
        let name = cups
            .add(&NewPrinter {
                name: "seeded".to_owned(),
                uri: fake.uri().to_owned(),
                info: "Acme".to_owned(),
                location: String::new(),
            })
            .await?;
        assert_eq!(name, "seeded_2");
        let snapshot = cups.snapshot().await?;
        assert_eq!(
            snapshot.printer("Seeded").ok_or("gone")?.title(),
            "Front Desk"
        );
        assert_eq!(
            snapshot.printer("seeded_2").ok_or("not added")?.title(),
            "Acme"
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs cupsd; run scripts/check-integration.sh"]
fn a_printer_that_isnt_there_is_cups_error() -> Result<()> {
    run(async {
        let stand_in = StandIn::start(true)?;
        let cups = stand_in.cups();
        let error = cups
            .add(&NewPrinter {
                name: "Gone".to_owned(),
                // Nothing listens on the discard port.
                uri: "ipp://127.0.0.1:9/ipp/print".to_owned(),
                info: String::new(),
                location: String::new(),
            })
            .await
            .err()
            .ok_or("added a printer that isn't there")?;
        assert!(matches!(error, PrintError::Failed(_)), "{error:?}");
        assert!(cups.snapshot().await?.printer("Gone").is_none());
        Ok(())
    })
}

#[test]
#[ignore = "needs cupsd; run scripts/check-integration.sh"]
fn users_cancel_their_own_jobs_but_need_an_administrator_for_printers() -> Result<()> {
    run(async {
        let stand_in = StandIn::start_with(false, SEEDED)?;
        let cups = stand_in.cups();
        let id = cups.print_test_page("Seeded").await?;
        until(&cups, |s| s.jobs.iter().any(|job| job.id == id)).await?;
        cups.cancel_job(id).await?;
        until(&cups, |s| s.jobs.is_empty()).await?;
        match cups.resume("Seeded").await {
            Err(PrintError::NotAllowed(authorization)) => {
                assert_ne!(authorization.group, "");
            }
            other => return Err(format!("resumed without rights: {other:?}").into()),
        }
        assert!(
            cups.snapshot()
                .await?
                .printer("Seeded")
                .ok_or("gone")?
                .paused()
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs cupsd, dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn cups_pk_helper_makes_the_changes_cups_refuses() -> Result<()> {
    run(async {
        let stand_in = StandIn::start_with(false, SEEDED)?;
        let services = Services::start(true, false).await?;
        let cups = stand_in.cups().with_admin_bus(services.bus());
        cups.resume("Seeded").await?;
        let calls = services.pk_helper_calls("PrinterSetEnabled").await?;
        assert_eq!(calls.len(), 1);
        assert_eq!(stand_in::string(&calls[0][0]).as_deref(), Some("Seeded"));
        cups.add(&NewPrinter {
            name: "New".to_owned(),
            uri: "ipps://New._ipps._tcp.local/".to_owned(),
            info: "New One".to_owned(),
            location: String::new(),
        })
        .await?;
        let calls = services.pk_helper_calls("PrinterAdd").await?;
        let args: Vec<_> = calls[0].iter().filter_map(stand_in::string).collect();
        assert_eq!(
            args,
            [
                "New",
                "ipps://New._ipps._tcp.local/",
                "everywhere",
                "New One",
                ""
            ]
        );
        cups.remove("Seeded").await?;
        assert_eq!(services.pk_helper_calls("PrinterDelete").await?.len(), 1);

        let denied = Services::start(true, true).await?;
        let cups = stand_in.cups().with_admin_bus(denied.bus());
        assert!(
            matches!(cups.set_default("Seeded").await, Err(PrintError::Denied(_))),
            "polkit's refusal isn't reported"
        );
        let missing = Services::start(false, false).await?;
        let cups = stand_in.cups().with_admin_bus(missing.bus());
        assert!(
            matches!(
                cups.set_default("Seeded").await,
                Err(PrintError::NotAllowed(_))
            ),
            "a missing cups-pk-helper isn't reported"
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs cupsd; run scripts/check-integration.sh"]
fn a_cups_that_isnt_running_is_unavailable() -> Result<()> {
    run(async {
        let mut stand_in = StandIn::start(true)?;
        stand_in.stop();
        let cups =
            Cups::at(Endpoint::Socket(stand_in.socket().to_owned())).with_admin_bus(Bus::Off);
        assert!(matches!(
            cups.snapshot().await,
            Err(PrintError::Unavailable(_))
        ));
        assert_eq!(cups.version().await, None);
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn avahi_finds_driverless_printers() -> Result<()> {
    run(async {
        let services = Services::start(false, false).await?;
        services
            .announce(
                "Acme [A1]",
                "_ipp._tcp",
                0,
                &["ty=Acme Laser 9000", "note=Hall", "rp=ipp/print"],
            )
            .await?;
        // A queue our own CUPS shares, which isn't offered.
        services
            .announce(
                "Own Queue",
                "_ipp._tcp",
                8,
                &["ty=Whatever", "printer-type=0x1046"],
            )
            .await?;
        // A service of this computer that can't be resolved: maybe our CUPS's, so left out.
        services
            .announce("Unknown Local", "_ipp._tcp", 8, &["unresolvable"])
            .await?;
        let mut discovery = Discovery::start(&services.bus()).await?;
        let Discovered::Added(found) = next(&mut discovery).await? else {
            return Err("not added".into());
        };
        assert_eq!(found.title(), "Acme Laser 9000");
        assert_eq!(found.location, "Hall");
        assert_eq!(found.uri, "ipp://Acme%20%5BA1%5D._ipp._tcp.local/");
        assert_eq!(found.host, "printer.local");
        assert!(found.is_at("ipp://printer.local:631/ipp/print"));
        assert!(!found.secure);
        // The secure service of the same printer makes it secure.
        services
            .announce("Acme [A1]", "_ipps._tcp", 0, &["ty=Acme Laser 9000"])
            .await?;
        let Discovered::Added(found) = next(&mut discovery).await? else {
            return Err("not updated".into());
        };
        assert!(found.secure);
        assert_eq!(found.uri, "ipps://Acme%20%5BA1%5D._ipps._tcp.local/");
        // One on the network that can't be resolved shows by its name.
        services
            .announce("Bare One", "_ipp._tcp", 0, &["unresolvable"])
            .await?;
        let Discovered::Added(bare) = next(&mut discovery).await? else {
            return Err("the unresolved printer isn't shown".into());
        };
        assert_eq!((bare.title(), bare.model.as_str()), ("Bare One", ""));
        services.withdraw("Bare One", "_ipp._tcp").await?;
        assert_eq!(
            next(&mut discovery).await?,
            Discovered::Removed("Bare One".to_owned())
        );
        services.withdraw("Acme [A1]", "_ipp._tcp").await?;
        services.withdraw("Acme [A1]", "_ipps._tcp").await?;
        assert_eq!(
            next(&mut discovery).await?,
            Discovered::Removed("Acme [A1]".to_owned())
        );
        Ok(())
    })
}
