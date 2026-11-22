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
use std::path::Path;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::future;
use sway_things_core::printers::stand_in::Services;
use sway_things_core::printers::{Discovered, Discovery};
use sway_things_core::scanners::{
    Error as ScanError, Format, ModeKind, Problem, Progress, Request, Sane, SaveAs, ScanDir,
    SourceKind, save,
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Runs a test, which fails instead of hanging.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(120)).await;
        Err("the test didn't finish within 120 seconds".into())
    }))
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
        let files = sane.scan(&request, &dir.batch()?)?.finish(|_| {}).await?;
        assert_eq!(files.len(), 10);
        // A second page from the flatbed, added to the same PDF.
        let flatbed = Request {
            source: Some("Flatbed".to_owned()),
            all_pages: false,
            ..request
        };
        let more = sane.scan(&flatbed, &dir.batch()?)?.finish(|_| {}).await?;
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
