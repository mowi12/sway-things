// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scanners backend against the distribution's scanimage with SANE's `test` backend, a real
//! frontend-tester device that scans pictures of its own, and a stand-in for Avahi on a private
//! bus.
//!
//! These tests need scanimage with the test backend, `dbus-daemon` and python-dbusmock, so a plain
//! `cargo test` skips them. `scripts/check-integration.sh` runs them with `--ignored` in the
//! containers, which have all of them.

use std::error::Error;
use std::fs;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::future;
use sway_things_core::printers::stand_in::Services;
use sway_things_core::printers::{Discovered, Discovery};
use sway_things_core::scanners::{
    Error as ScanError, Format, ModeKind, Problem, Progress, Request, Sane, SaveAs, Scan, ScanDir,
    SourceKind, save,
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// How long a scan of the test backend may take: one takes well under a second, and scanimage
/// gets 10 s to end after the batch ended.
const STALLED: Duration = Duration::from_secs(30);

/// Runs a test, which fails instead of hanging.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(120)).await;
        Err("the test didn't finish within 120 seconds".into())
    }))
}

/// Runs `step` of a test, which fails with what it was doing instead of hanging.
async fn step<T>(what: &str, step: impl Future<Output = Result<T>>) -> Result<T> {
    future::or(step, async {
        Timer::after(Duration::from_secs(60)).await;
        Err(format!("{what} didn't finish within 60 seconds").into())
    })
    .await
}

/// The pages of a scan with `request` into a new batch of `dir`, which fails with `what` it was
/// doing instead of hanging. scanimage can stall for good between two pages of the test backend:
/// its reader thread, canceled asynchronously while it ends, can leave the dynamic loader's lock
/// taken. A scan that stalls is killed and tried once more; a second stall fails.
async fn scan(what: &str, sane: &Sane, request: &Request, dir: &ScanDir) -> Result<Vec<PathBuf>> {
    for attempt in 1..=2 {
        let scan = sane.scan(request, &dir.batch()?)?;
        // Dropping the scan kills scanimage.
        let finished = future::or(Box::pin(async { Some(scan.finish(|_| {}).await) }), async {
            Timer::after(STALLED).await;
            None
        })
        .await;
        match finished {
            Some(files) => return Ok(files?),
            None => eprintln!("{what} stalled (attempt {attempt})"),
        }
    }
    Err(format!(
        "{what} didn't finish within {} seconds, twice",
        STALLED.as_secs()
    )
    .into())
}

/// Starts a scan with the fake scanimage `sane` into a new batch of `dir`. A program just
/// written can't be run while a process another test forks meanwhile still holds it open for
/// writing ("Text file busy"), until that process runs its own program: tried again then.
async fn start(sane: &Sane, request: &Request, dir: &ScanDir) -> Result<Scan> {
    for _ in 0..100 {
        match sane.scan(request, &dir.batch()?) {
            Err(ScanError::Io(error)) if error.contains("os error 26") => {
                Timer::after(Duration::from_millis(20)).await;
            }
            started => return Ok(started?),
        }
    }
    Err("the fake scanimage stays busy".into())
}

/// scanimage with a SANE configuration in `dir` that has the test backend only.
fn sane(dir: &Path) -> Result<Sane> {
    fs::write(dir.join("dll.conf"), "test\n")?;
    let sane = Sane::find().ok_or("scanimage isn't installed")?;
    Ok(sane.with_env("SANE_CONFIG_DIR", dir))
}

#[test]
#[ignore = "needs scanimage with SANE's test backend; run scripts/check-integration.sh"]
fn the_test_devices_are_listed_and_read() -> Result<()> {
    run(async {
        let config = tempfile::tempdir()?;
        let sane = sane(config.path())?;
        let scanners = sane.list().await?;
        let devices: Vec<&str> = scanners.iter().map(|s| s.device.as_str()).collect();
        assert_eq!(devices, ["test:0", "test:1"]);
        assert_eq!(scanners[0].title(), "Noname frontend-tester");
        let options = sane.options("test:0").await?;
        assert_eq!(options.mode_of(ModeKind::Color), Some("Color"));
        assert_eq!(options.mode_of(ModeKind::Gray), Some("Gray"));
        assert_eq!(
            options
                .sources
                .iter()
                .map(|source| source.kind)
                .collect::<Vec<_>>(),
            [SourceKind::Flatbed, SourceKind::Feeder]
        );
        assert_eq!(options.document_resolution(), Some(300));
        assert_eq!(options.area, Some((200.0, 200.0)));
        let formats = sane.formats().await;
        assert!(formats.contains(&Format::Png) && formats.contains(&Format::Jpeg));
        // A device that isn't there.
        let gone = sane.options("nosuch:0").await;
        assert!(
            matches!(
                gone,
                Err(ScanError::Failed {
                    problem: Problem::Gone,
                    ..
                })
            ),
            "{gone:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs scanimage with SANE's test backend; run scripts/check-integration.sh"]
fn without_scanimage_nothing_is_found() -> Result<()> {
    run(async {
        let sane = Sane::at("/nonexistent/scanimage");
        assert_eq!(sane.list().await, Err(ScanError::Missing));
        Ok(())
    })
}

#[test]
#[ignore = "needs scanimage with SANE's test backend; run scripts/check-integration.sh"]
fn a_page_is_scanned_and_saved_as_png() -> Result<()> {
    run(async {
        let config = tempfile::tempdir()?;
        let sane = sane(config.path())?;
        let cache = tempfile::tempdir()?;
        let dir = ScanDir::new_in(cache.path())?;
        let request = Request {
            device: "test:0".to_owned(),
            mode: Some("Color".to_owned()),
            resolution: Some(75),
            area: Some((148.0, 200.0)),
            format: Format::Png,
            ..Request::default()
        };
        let scan = sane.scan(&request, &dir.batch()?)?;
        let mut percent = 0.0;
        let mut pages = Vec::new();
        let files = scan
            .finish(|progress| match progress {
                Progress::Percent(value) => percent = value,
                Progress::Page(page) => pages.push(page),
            })
            .await?;
        assert_eq!(files.len(), 1);
        assert_eq!(pages, [1]);
        assert!(percent > 99.0, "{percent}");
        let out = tempfile::tempdir()?;
        let png = out.path().join("Scan.png");
        let pages: Vec<_> = files.into_iter().map(|file| (file, 75)).collect();
        save(&pages, SaveAs::Image, &png)?;
        assert!(fs::read(&png)?.starts_with(b"\x89PNG"));
        Ok(())
    })
}

#[test]
#[ignore = "needs scanimage with SANE's test backend; run scripts/check-integration.sh"]
fn the_feeder_is_scanned_into_one_pdf() -> Result<()> {
    run(async {
        let config = tempfile::tempdir()?;
        let sane = sane(config.path())?;
        let cache = tempfile::tempdir()?;
        let dir = ScanDir::new_in(cache.path())?;
        // The test backend's feeder holds ten sheets.
        let request = Request {
            device: "test:0".to_owned(),
            source: Some("Automatic Document Feeder".to_owned()),
            mode: Some("Gray".to_owned()),
            resolution: Some(50),
            format: Format::Jpeg,
            all_pages: true,
            ..Request::default()
        };
        let files = scan("scanning the feeder", &sane, &request, &dir).await?;
        assert_eq!(files.len(), 10);
        // A second page from the flatbed, added to the same PDF.
        let flatbed = Request {
            source: Some("Flatbed".to_owned()),
            all_pages: false,
            ..request
        };
        let more = scan("scanning the flatbed", &sane, &flatbed, &dir).await?;
        let all: Vec<_> = files.into_iter().chain(more).collect();
        let out = tempfile::tempdir()?;
        let pdf = out.path().join("Scan.pdf");
        let all: Vec<_> = all.into_iter().map(|file| (file, 50)).collect();
        save(&all, SaveAs::Pdf, &pdf)?;
        let text = String::from_utf8_lossy(&fs::read(&pdf)?).into_owned();
        assert!(text.starts_with("%PDF-1.4") && text.contains("/Count 11"));
        Ok(())
    })
}

#[test]
#[ignore = "needs scanimage with SANE's test backend; run scripts/check-integration.sh"]
fn a_scanimage_that_hangs_after_the_batch_is_ended() -> Result<()> {
    run(async {
        // scanimage can hang for good in `sane_exit` after the batch ended, its pages written:
        // a fake one that does, after a page or after a jam.
        let bin = tempfile::tempdir()?;
        let fake = |name: &str, error: &str| -> Result<Sane> {
            let path = bin.path().join(name);
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\n\
                     for arg; do case $arg in --batch=*) pattern=${{arg#--batch=}};; esac; done\n\
                     echo 'Scanning page 1' >&2\n\
                     : > \"$(printf \"$pattern\" 1)\"\n\
                     {error}\
                     echo 'Batch terminated, 1 page scanned' >&2\n\
                     exec sleep 600\n"
                ),
            )?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
            Ok(Sane::at(path))
        };
        let cache = tempfile::tempdir()?;
        let dir = ScanDir::new_in(cache.path())?;
        let request = Request {
            device: "test:0".to_owned(),
            ..Request::default()
        };
        let sane = fake("scanimage", "")?;
        let jams = fake(
            "jammed",
            "echo 'scanimage: sane_read: Document feeder jammed' >&2\n",
        )?;
        // Both at once: each takes the grace scanimage has to end.
        let (files, jammed) = future::zip(
            Box::pin(step("the scan that hangs after a page", async {
                Ok(start(&sane, &request, &dir).await?.finish(|_| {}).await?)
            })),
            Box::pin(step("the scan that hangs after a jam", async {
                Ok(start(&jams, &request, &dir).await?.finish(|_| {}).await)
            })),
        )
        .await;
        assert_eq!(files?.len(), 1);
        let jammed = jammed?;
        assert!(
            matches!(
                jammed,
                Err(ScanError::Failed {
                    problem: Problem::Jammed,
                    ..
                })
            ),
            "{jammed:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs scanimage with SANE's test backend; run scripts/check-integration.sh"]
fn a_scan_is_canceled() -> Result<()> {
    run(async {
        let config = tempfile::tempdir()?;
        let sane = sane(config.path())?;
        let cache = tempfile::tempdir()?;
        let dir = ScanDir::new_in(cache.path())?;
        // Large enough to take a while.
        let request = Request {
            device: "test:0".to_owned(),
            mode: Some("Color".to_owned()),
            resolution: Some(1200),
            format: Format::Png,
            ..Request::default()
        };
        let scan = sane.scan(&request, &dir.batch()?)?;
        let canceler = scan.canceler();
        let result = scan
            .finish(|progress| {
                if matches!(progress, Progress::Percent(_)) {
                    canceler.cancel();
                }
            })
            .await;
        assert_eq!(result, Err(ScanError::Canceled));
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon and python-dbusmock; run scripts/check-integration.sh"]
fn avahi_finds_driverless_scanners() -> Result<()> {
    run(async {
        let services = Services::start(false, false).await?;
        services
            .announce("Office MFP", "_ipp._tcp", 0, &["ty=Office MFP"])
            .await?;
        services
            .announce(
                "Office MFP",
                "_uscan._tcp",
                0,
                &["ty=Office MFP", "rs=eSCL"],
            )
            .await?;
        // ipp-usb announces a USB device on this computer.
        services
            .announce("Desk MFP", "_uscan._tcp", 8, &["ty=Desk MFP"])
            .await?;
        let mut discovery = Discovery::scanners(&services.bus()).await?;
        let mut found = Vec::new();
        while found.len() < 2 {
            let event = future::or(async { Ok(discovery.next().await) }, async {
                Timer::after(Duration::from_secs(10)).await;
                Err::<Option<Discovered>, Box<dyn Error + Send + Sync>>("nothing more found".into())
            })
            .await?;
            if let Some(Discovered::Added(scanner)) = event {
                found.push((scanner.title().to_owned(), scanner.local));
            }
        }
        found.sort();
        assert_eq!(
            found,
            [
                ("Desk MFP".to_owned(), true),
                ("Office MFP".to_owned(), false)
            ]
        );
        Ok(())
    })
}
