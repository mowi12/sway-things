// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scanners of Printers & Scanners against scanimage with SANE's test backend, and Avahi's
//! stand-in on a private bus.

use super::*;

/// The configuration of SANE's test backend: two devices that scan a color pattern.
const TEST_CONF: &str = "number_of_devices 2\ntest-picture \"Color pattern\"\n";

/// A SANE configuration in a directory of its own that has the backends `backends` only, and an
/// empty installation to look at for the hints: no sane-airscan, no ipp-usb.
pub(crate) struct SaneFixture {
    /// SANE's configuration.
    pub config: tempfile::TempDir,
    /// The installation the hints look at.
    pub root: tempfile::TempDir,
}

impl SaneFixture {
    /// With the backends `backends` (`test`, or none).
    pub(crate) fn new(backends: &str) -> Result<Self, String> {
        let config = tempfile::tempdir().map_err(|error| error.to_string())?;
        std::fs::write(config.path().join("dll.conf"), backends)
            .map_err(|error| error.to_string())?;
        // Pages with something on them, rather than solid black.
        std::fs::write(config.path().join("test.conf"), TEST_CONF)
            .map_err(|error| error.to_string())?;
        let root = tempfile::tempdir().map_err(|error| error.to_string())?;
        Ok(Self { config, root })
    }

    /// Where the page finds this SANE, with scanimage at `program` and Avahi on `bus`.
    pub(crate) fn service(&self, program: &Path, bus: Option<&str>) -> ScanService {
        ScanService::At {
            program: program.to_owned(),
            config: Some(self.config.path().to_owned()),
            bus: bus.map(ToOwned::to_owned),
            root: Some(self.root.path().to_owned()),
        }
    }
}

/// Points `window` at `sane` with the installed scanimage, if both are there.
pub(crate) fn use_sane(window: &Window, sane: Option<&SaneFixture>) {
    if let (Some(sane), Ok(program)) = (sane, scanimage()) {
        window.use_scan_service(sane.service(&program, None));
    }
}

/// The installed scanimage.
pub(crate) fn scanimage() -> Result<PathBuf, String> {
    sway_things_core::scanners::Sane::find()
        .map(|sane| sane.program().to_owned())
        .ok_or_else(|| "  scanimage isn't installed".to_owned())
}

/// `page` in a window of its own, once its search of the scanners has ended.
pub(crate) fn scanners_window(page: &PrintersPage) -> Result<gtk::Window, String> {
    let window = gtk::Window::new();
    window.set_default_size(720, 900);
    window.set_child(Some(page));
    window.present();
    settle(&window)?;
    ensure(
        run_until(Duration::from_secs(30), || {
            !page.searching_scanners() && !page.scanner_hints().is_empty()
                || !page.scanner_rows().is_empty() && !page.searching_scanners()
        }),
        &format!(
            "  the search didn't end: {:?} {:?}",
            page.scanner_rows(),
            page.scanner_hints()
        ),
    )?;
    Ok(window)
}

pub(crate) fn check_scanners_pages() -> Vec<(String, Outcome)> {
    vec![
        (
            "scanners: SANE's devices, one Avahi finds that SANE doesn't, and the hint".to_owned(),
            check_scanners_listed(),
        ),
        ("scanners: none found".to_owned(), check_scanners_none()),
        (
            "scanners: scanimage missing".to_owned(),
            check_scanners_without_scanimage(),
        ),
        (
            "scanners: preview, a PNG, a PDF of the feeder and the glass, cancel".to_owned(),
            check_scan_dialog(),
        ),
        (
            "scanners: a scanner that is gone".to_owned(),
            check_scan_gone(),
        ),
    ]
}

/// The test backend's two devices, and a driverless scanner on the network that SANE doesn't
/// list, which says sane-airscan is missing.
pub(crate) fn check_scanners_listed() -> Outcome {
    let sane = SaneFixture::new("test\n")?;
    let services = async_io::block_on(sway_things_core::printers::stand_in::Services::start(
        false, false,
    ))
    .map_err(|error| format!("  Avahi's stand-in: {error}"))?;
    async_io::block_on(services.announce(
        "Hall Scanner",
        "_uscan._tcp",
        0,
        &["ty=Brother ADS-1700W"],
    ))
    .map_err(|error| format!("  announcing: {error}"))?;
    let page = PrintersPage::with_scanners(
        &PrintService::Off,
        &sane.service(&scanimage()?, Some(services.address())),
    );
    let window = scanners_window(&page)?;
    let rows = page.scanner_rows();
    let row =
        |title: &str, subtitle: &str, listed: bool| (title.to_owned(), subtitle.to_owned(), listed);
    ensure(
        rows == [
            row("Noname frontend-tester", "Virtual device", true),
            row("Noname frontend-tester", "Virtual device", true),
            row(
                "Brother ADS-1700W",
                "Network, driverless \u{b7} SANE doesn't see it yet",
                false,
            ),
        ],
        &format!("  rows: {rows:?}"),
    )?;
    let hints = page.scanner_hints();
    ensure(
        hints.len() == 1
            && hints[0].starts_with("Brother ADS-1700W on the network can't be used yet")
            && hints[0].contains("sane-airscan"),
        &format!("  hints: {hints:?}"),
    )?;
    // The printers' placeholder doesn't hide the scanners.
    ensure(page.placeholder().is_some(), "  no placeholder for CUPS")?;
    window.destroy();
    drop(services);
    Ok(())
}

/// No scanner: the page says so, and that sane-airscan finds most scanners.
pub(crate) fn check_scanners_none() -> Outcome {
    let sane = SaneFixture::new("")?;
    let page = PrintersPage::with_scanners(&PrintService::Off, &sane.service(&scanimage()?, None));
    let window = scanners_window(&page)?;
    let hints = page.scanner_hints();
    ensure(
        page.scanner_rows().is_empty()
            && hints.len() == 2
            && hints[0].starts_with("No scanner found.")
            && hints[1].starts_with("Most scanners today are driverless"),
        &format!("  hints: {hints:?}"),
    )?;
    window.destroy();
    Ok(())
}

/// Without scanimage, the hint says which package has it.
pub(crate) fn check_scanners_without_scanimage() -> Outcome {
    let sane = SaneFixture::new("test\n")?;
    std::fs::create_dir_all(sane.root.path().join("etc")).map_err(|error| error.to_string())?;
    std::fs::write(sane.root.path().join("etc/os-release"), "ID=debian\n")
        .map_err(|error| error.to_string())?;
    let page = PrintersPage::with_scanners(
        &PrintService::Off,
        &sane.service(Path::new("/nonexistent/scanimage"), None),
    );
    let window = scanners_window(&page)?;
    shoot_checked(&window, "scan-missing-light")?;
    let hints = page.scanner_hints();
    ensure(
        page.scanner_rows().is_empty()
            && hints
                == [
                    "SANE's scanimage isn't installed, so no scanner can be found or used. \
                     Install the package sane-utils.",
                ],
        &format!("  hints: {hints:?}"),
    )?;
    window.destroy();
    Ok(())
}

/// The files in `dir`, sorted.
fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The scan dialog of `page` for `device`, once it has read the scanner's options.
pub(crate) fn scan_dialog(page: &PrintersPage, device: &str) -> Result<ScanDialog, String> {
    page.open_scan_dialog(device);
    let dialog = page.scan_dialog().ok_or("  no scan dialog")?;
    ensure(
        run_until(Duration::from_secs(30), || dialog.is_ready()),
        &format!("  the options weren't read: {:?}", dialog.error_text()),
    )?;
    Ok(dialog)
}

/// Waits until the scan of `dialog` has ended.
fn scanned(dialog: &ScanDialog) -> Outcome {
    ensure(
        run_until(Duration::from_secs(60), || !dialog.is_scanning()),
        "  the scan didn't end",
    )
}

/// The scan dialog against the test backend, as a user goes through it: what it offers, a
/// preview, a page into PNG, the feeder and a page of the glass into one PDF, a cancel, and the
/// files written: only those picked.
pub(crate) fn check_scan_dialog() -> Outcome {
    let sane = SaneFixture::new("test\n")?;
    let page = PrintersPage::with_scanners(&PrintService::Off, &sane.service(&scanimage()?, None));
    let window = scanners_window(&page)?;
    let dialog = scan_dialog(&page, "test:0")?;
    let [sources, modes, resolutions, papers, formats] = dialog.offered();
    ensure(
        sources == ["Glass (flatbed)", "Document feeder"]
            && modes == ["Color", "Gray"]
            && resolutions.first().map(String::as_str) == Some("75 dpi")
            && resolutions.iter().any(|dpi| dpi == "300 dpi")
            && papers == ["A6", "Whole area"]
            && formats.first().map(String::as_str) == Some("PDF")
            && formats.iter().any(|format| format == "PNG"),
        &format!("  offered: {:?}", dialog.offered()),
    )?;
    let [preview, primary, another, discard, cancel] = dialog.buttons();
    preview.emit_clicked();
    scanned(&dialog)?;
    ensure(
        dialog.has_picture() && dialog.page_count() == 0,
        &format!("  no preview: {:?}", dialog.error_text()),
    )?;

    let out = tempfile::tempdir().map_err(|error| error.to_string())?;
    ensure(dialog.choose(4, "PNG"), "  no PNG")?;
    ensure(primary.label().as_deref() == Some("Scan"), "  no Scan")?;
    primary.emit_clicked();
    scanned(&dialog)?;
    ensure(
        dialog.page_count() == 1
            && primary.label().as_deref() == Some("Save…")
            && another.label().as_deref() == Some("Scan Again")
            && discard.is_visible(),
        &format!(
            "  after a PNG: {} pages, {:?}, {:?}",
            dialog.page_count(),
            primary.label(),
            dialog.error_text()
        ),
    )?;
    dialog.save_to(&out.path().join("Scan.png"));
    let png = std::fs::read(out.path().join("Scan.png")).unwrap_or_default();
    ensure(
        png.starts_with(b"\x89PNG") && dialog.page_count() == 0,
        &format!("  the PNG wasn't saved: {:?}", dialog.error_text()),
    )?;

    ensure(dialog.choose(4, "PDF"), "  no PDF")?;
    ensure(dialog.choose(0, "Document feeder"), "  no feeder")?;
    ensure(!preview.is_sensitive(), "  a preview from the feeder")?;
    primary.emit_clicked();
    scanned(&dialog)?;
    ensure(
        dialog.page_count() == 10 && another.label().as_deref() == Some("Scan Another Page"),
        &format!("  the feeder: {} pages", dialog.page_count()),
    )?;
    ensure(dialog.choose(0, "Glass (flatbed)"), "  no glass")?;
    another.emit_clicked();
    scanned(&dialog)?;
    ensure(
        dialog.page_count() == 11 && dialog.status_text().starts_with("11 pages scanned"),
        &format!("  {} pages: {}", dialog.page_count(), dialog.status_text()),
    )?;
    dialog.save_to(&out.path().join("Scan.pdf"));
    let pdf = std::fs::read(out.path().join("Scan.pdf")).unwrap_or_default();
    let text = String::from_utf8_lossy(&pdf);
    ensure(
        text.starts_with("%PDF-1.4") && text.contains("/Count 11"),
        "  the PDF doesn't hold 11 pages",
    )?;
    ensure(
        files_in(out.path()) == ["Scan.pdf", "Scan.png"],
        &format!("  files written: {:?}", files_in(out.path())),
    )?;

    // A large scan, canceled while it runs.
    ensure(dialog.choose(2, "1200 dpi"), "  no 1200 dpi")?;
    ensure(dialog.choose(3, "Whole area"), "  no whole area")?;
    primary.emit_clicked();
    ensure(
        dialog.is_scanning() && cancel.is_visible(),
        "  the scan didn't start",
    )?;
    cancel.emit_clicked();
    scanned(&dialog)?;
    ensure(
        dialog.page_count() == 0 && dialog.status_text() == "Canceled.",
        &format!("  after canceling: {}", dialog.status_text()),
    )?;
    dialog.close();
    ensure(page.scan_dialog().is_none(), "  the dialog stayed")?;
    window.destroy();
    Ok(())
}

/// A scanner that is gone by the time its dialog opens: the error says so in words, with what
/// scanimage said under Details, and a scan fails the same way.
pub(crate) fn check_scan_gone() -> Outcome {
    let sane = SaneFixture::new("test\n")?;
    let page = PrintersPage::with_scanners(&PrintService::Off, &sane.service(&scanimage()?, None));
    let window = scanners_window(&page)?;
    // The test backend has one device now: test:1 is gone.
    std::fs::write(
        sane.config.path().join("test.conf"),
        TEST_CONF.replace("number_of_devices 2", "number_of_devices 1"),
    )
    .map_err(|error| error.to_string())?;
    let dialog = scan_dialog(&page, "test:1")?;
    let (error, details) = dialog.error_text();
    ensure(
        error.starts_with("The scanner can't be reached.") && details.contains("open of device"),
        &format!("  error: {error:?} {details:?}"),
    )?;
    shoot_checked(&dialog.window, "scan-error-light")?;
    let [_, primary, ..] = dialog.buttons();
    primary.emit_clicked();
    scanned(&dialog)?;
    ensure(
        dialog.page_count() == 0
            && dialog
                .error_text()
                .0
                .starts_with("The scanner can't be reached."),
        &format!("  after a scan: {:?}", dialog.error_text()),
    )?;
    dialog.close();
    window.destroy();
    Ok(())
}
