// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Printers page against CUPS's stand-in.

use super::*;

/// Two printers CUPS knows from before: "Front Desk", paused and low on toner, and "Office", the
/// default.
pub(crate) const PRINTERS_CONF: &str = "\
<DefaultPrinter Office>
Info Office
DeviceURI ipp://127.0.0.1:9/ipp/print
State Idle
Type 4
Accepting Yes
Shared No
</DefaultPrinter>
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

/// A CUPS of its own with [`PRINTERS_CONF`]; the user may change printers if `admin`. A test page
/// waits on "Front Desk".
pub(crate) fn printers_stand_in(admin: bool) -> Result<CupsStandIn, String> {
    let stand_in = CupsStandIn::start_with(admin, PRINTERS_CONF)
        .map_err(|error| format!("  CUPS's stand-in: {error}"))?;
    async_io::block_on(stand_in.cups().print_test_page("Seeded"))
        .map_err(|error| format!("  a test page on the stand-in: {error}"))?;
    Ok(stand_in)
}

/// Where the Printers page finds `stand_in`.
pub(crate) fn print_service(stand_in: &CupsStandIn) -> PrintService {
    PrintService::At {
        socket: stand_in.socket().to_owned(),
        bus: None,
    }
}

/// `page` in a window of its own, once it shows `rows` printers.
pub(crate) fn printers_window(page: &PrintersPage, rows: usize) -> Result<gtk::Window, String> {
    let window = gtk::Window::new();
    window.set_default_size(720, 900);
    window.set_child(Some(page));
    window.present();
    settle(&window)?;
    ensure(
        run_until(Duration::from_secs(10), || page.rows().len() == rows),
        &format!(
            "  the printers didn't show: {:?} {:?}",
            page.rows(),
            page.placeholder()
        ),
    )?;
    Ok(window)
}

/// The checks of Printers & Scanners: the printers', then the scanners'.
pub(crate) fn check_printers_pages() -> Vec<(String, Outcome)> {
    let mut checks = vec![
        (
            "printers: states, supplies, the default and the queue".to_owned(),
            check_printers(),
        ),
        (
            "printers: resume, pause, test page and cancel".to_owned(),
            check_printers_actions(),
        ),
        (
            "printers: a change that needs an administrator".to_owned(),
            check_printers_not_allowed(),
        ),
        (
            "printers: adding a printer by its address".to_owned(),
            check_printers_add(),
        ),
        (
            "printers: without CUPS".to_owned(),
            check_printers_missing(),
        ),
        (
            "printers: the dialogs close every way and open again".to_owned(),
            check_printers_dialogs(),
        ),
        (
            "printers: the page, the details and Add Printer, light and dark".to_owned(),
            check_printers_screens(),
        ),
    ];
    checks.extend(check_scanners_pages());
    checks
}

/// [`shoot_printers`] in light and dark: each screen's contrast is checked, and saved as a
/// screenshot with `SWAY_THINGS_SCREENSHOTS`.
pub(crate) fn check_printers_screens() -> Outcome {
    let settings = gtk::Settings::default().ok_or_else(|| "  no GTK settings".to_owned())?;
    for dark in [false, true] {
        set_dark(&settings, dark);
        let result = shoot_printers(if dark { "dark" } else { "light" });
        if result.is_err() {
            set_dark(&settings, false);
            return result;
        }
    }
    set_dark(&settings, false);
    Ok(())
}

/// Saves `window` as `<name>.png` with `SWAY_THINGS_SCREENSHOTS`, else checks its contrast.
pub(crate) fn shoot_checked(window: &gtk::Window, name: &str) -> Outcome {
    if std::env::var_os("SWAY_THINGS_SCREENSHOTS").is_some() {
        return shoot(window, name);
    }
    settle(window)?;
    check_screen(window, None).map(|_| ())
}

/// The lines of the printers: by title, with their state in words and the jobs waiting, the
/// default's badge and the ink or toner; a click on the line or on the jobs opens the details
/// with the job.
pub(crate) fn check_printers() -> Outcome {
    let stand_in = printers_stand_in(true)?;
    let page = PrintersPage::new(&print_service(&stand_in));
    let window = printers_window(&page, 2)?;
    let rows = page.rows();
    let row = |name: &str, title: &str, status: &str, default: bool| {
        (
            name.to_owned(),
            title.to_owned(),
            status.to_owned(),
            default,
        )
    };
    ensure(
        rows == [
            row("Seeded", "Front Desk", "Paused \u{b7} Toner low", false),
            row("Office", "Office", "Ready", true),
        ],
        &format!("  rows: {rows:?}"),
    )?;
    let supplies = page.supplies("Seeded");
    ensure(
        supplies.as_deref() == Some("Black Toner: 7 %, low\nColor Toner: 55 %"),
        &format!("  supplies: {supplies:?}"),
    )?;
    ensure(
        page.resume_button("Seeded").is_some_and(|b| b.is_visible())
            && page
                .resume_button("Office")
                .is_some_and(|b| !b.is_visible()),
        "  Resume shows on the wrong printers",
    )?;
    let jobs = page.jobs_button("Seeded");
    ensure(
        jobs.as_ref()
            .is_some_and(|b| b.is_visible() && b.label().as_deref() == Some("1 job"))
            && page.jobs_button("Office").is_some_and(|b| !b.is_visible()),
        "  the jobs show on the wrong printers",
    )?;
    if let Some(jobs) = jobs {
        jobs.emit_clicked();
    }
    let details = page
        .details_dialog()
        .ok_or("  the jobs opened no details")?;
    ensure(
        details.window.is_visible(),
        "  the details of the jobs aren't shown",
    )?;
    details.close();
    page.activate_row("Seeded");
    let details = page.details_dialog().ok_or("  no details")?;
    let jobs = details.job_rows();
    ensure(
        jobs.len() == 1 && jobs[0].1 == "Test Page",
        &format!("  jobs: {jobs:?}"),
    )?;
    let said = labels_in_window(&details.window);
    ensure(
        said.iter().any(|text| text == "Black Toner: 7 %, low")
            && said.iter().any(|text| text == "Local Raw Printer")
            && said.iter().any(|text| text == "Paused \u{b7} Toner low"),
        &format!("  the details say {said:?}"),
    )?;
    let shown = |label: &str| {
        button_in(details.window.upcast_ref(), label).map(|button| button.is_visible())
    };
    ensure(
        shown("Set as Default") == Some(true)
            && shown("Resume") == Some(true)
            && shown("Pause") == Some(false),
        &format!(
            "  the buttons of Front Desk: Set as Default {:?}, Resume {:?}, Pause {:?}",
            shown("Set as Default"),
            shown("Resume"),
            shown("Pause")
        ),
    )?;
    details.close();
    page.activate_row("Office");
    let details = page.details_dialog().ok_or("  no details of Office")?;
    ensure(
        button_in(details.window.upcast_ref(), "Set as Default")
            .is_some_and(|button| !button.is_visible()),
        "  Set as Default on the default printer",
    )?;
    details.close();
    window.destroy();
    Ok(())
}

/// The texts of the labels in `window`.
pub(crate) fn labels_in_window(window: &gtk::Window) -> Vec<String> {
    all::<gtk::Label>(window.upcast_ref())
        .into_iter()
        .filter(WidgetExt::is_visible)
        .map(|label| label.text().to_string())
        .collect()
}

/// Resume, Pause, Print Test Page, Cancel and Set as Default, on the line and in the details,
/// against CUPS; and a change of the description, saved when the details close.
pub(crate) fn check_printers_actions() -> Outcome {
    let stand_in = printers_stand_in(true)?;
    let page = PrintersPage::new(&print_service(&stand_in));
    let window = printers_window(&page, 2)?;
    let status = |name: &str| {
        page.rows()
            .into_iter()
            .find(|row| row.0 == name)
            .map(|row| row.2)
            .unwrap_or_default()
    };
    // Its queue is canceled first, so resuming doesn't send it to a printer that isn't there.
    page.activate_row("Seeded");
    let details = page.details_dialog().ok_or("  no details")?;
    let id = details
        .job_rows()
        .first()
        .map(|job| job.0)
        .ok_or("  no job")?;
    details
        .cancel_button(id)
        .ok_or("  no Cancel")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(10), || details.job_rows().is_empty()),
        &format!("  the job wasn't canceled: {}", page.error_text()),
    )?;
    details.close();
    page.resume_button("Seeded")
        .ok_or("  no Resume")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(10), || {
            status("Seeded") == "Ready \u{b7} Toner low"
        }),
        &format!(
            "  Front Desk didn't resume: {}; error: {}",
            status("Seeded"),
            page.error_text()
        ),
    )?;
    page.activate_row("Office");
    let details = page.details_dialog().ok_or("  no details of Office")?;
    button_in(details.window.upcast_ref(), "Pause")
        .ok_or("  no Pause")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(10), || {
            status("Office") == "Paused" && details.status_text() == "Paused"
        }),
        &format!(
            "  Office didn't pause: {} / {}",
            status("Office"),
            details.status_text()
        ),
    )?;
    button_in(details.window.upcast_ref(), "Print Test Page")
        .ok_or("  no Print Test Page")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(10), || details.job_rows().len() == 1),
        &format!(
            "  no test page: {:?} {}",
            details.job_rows(),
            page.error_text()
        ),
    )?;
    let (info, _, _) = details.fields();
    info.set_text("Office Laser");
    details.close();
    ensure(
        run_until(Duration::from_secs(10), || {
            page.rows()
                .iter()
                .any(|row| row.0 == "Office" && row.1 == "Office Laser")
        }),
        &format!("  the description wasn't saved: {:?}", page.rows()),
    )?;
    page.activate_row("Seeded");
    let details = page.details_dialog().ok_or("  no details of Front Desk")?;
    button_in(details.window.upcast_ref(), "Set as Default")
        .ok_or("  no Set as Default")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(10), || {
            page.rows().iter().any(|row| row.0 == "Seeded" && row.3)
        }),
        &format!("  Front Desk isn't the default: {:?}", page.rows()),
    )?;
    details.close();
    ensure(page.error_text().is_empty(), &page.error_text())?;
    window.destroy();
    Ok(())
}

/// A change CUPS refuses, without cups-pk-helper: the error says what to install or join.
pub(crate) fn check_printers_not_allowed() -> Outcome {
    let stand_in = printers_stand_in(false)?;
    let page = PrintersPage::new(&print_service(&stand_in));
    let window = printers_window(&page, 2)?;
    page.resume_button("Seeded")
        .ok_or("  no Resume")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(10), || !page.error_text().is_empty()),
        "  no error",
    )?;
    let error = page.error_text();
    ensure(
        error
            .starts_with("Couldn't resume “Front Desk”: Changing printers needs an administrator.")
            && error.contains("cups-pk-helper")
            && error.contains("group “"),
        &format!("  error: {error}"),
    )?;
    window.destroy();
    Ok(())
}

/// _Add Printer…_ with an address: the printer is added as CUPS makes it, named by its host.
pub(crate) fn check_printers_add() -> Outcome {
    let stand_in = printers_stand_in(true)?;
    let fake = FakePrinter::start("Acme Color 500").map_err(|error| format!("  {error}"))?;
    let page = PrintersPage::new(&print_service(&stand_in));
    let window = printers_window(&page, 2)?;
    page.add_button().emit_clicked();
    let dialog = page.add_dialog().ok_or("  no dialog")?;
    ensure(
        !dialog.add_address.is_sensitive(),
        "  Add without an address",
    )?;
    dialog.address.set_text("not a printer");
    ensure(
        !dialog.add_address.is_sensitive(),
        "  Add with a bad address",
    )?;
    dialog.address.set_text(fake.uri());
    ensure(
        dialog.add_address.is_sensitive(),
        "  no Add for a good address",
    )?;
    dialog.add_address.emit_clicked();
    ensure(
        run_until(Duration::from_secs(30), || page.add_dialog().is_none()),
        &format!("  the dialog stayed: {}", dialog.status.text()),
    )?;
    ensure(
        run_until(Duration::from_secs(10), || {
            page.rows()
                .iter()
                .any(|row| row.0 == "127_0_0_1" && row.1 == "127.0.0.1")
        }),
        &format!("  not added: {:?}", page.rows()),
    )?;
    window.destroy();
    Ok(())
}

/// Without CUPS, the page says so instead of an empty list.
pub(crate) fn check_printers_missing() -> Outcome {
    let page = PrintersPage::new(&PrintService::Off);
    let window = gtk::Window::new();
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let texts = page.placeholder().ok_or("  no placeholder")?;
    ensure(
        texts.first().map(String::as_str) == Some("CUPS isn't running")
            && texts
                .get(1)
                .is_some_and(|text| text.contains("cups.socket")),
        &format!("  placeholder: {texts:?}"),
    )?;
    window.destroy();
    Ok(())
}

/// Presses Escape in `window`: runs the action of its shortcut for Escape.
pub(crate) fn press_escape(window: &gtk::Window) -> Outcome {
    let controllers = window.observe_controllers();
    for i in 0..controllers.n_items() {
        let Some(controller) = controllers
            .item(i)
            .and_downcast::<gtk::ShortcutController>()
        else {
            continue;
        };
        for j in 0..controller.n_items() {
            let Some(shortcut) = controller.item(j).and_downcast::<gtk::Shortcut>() else {
                continue;
            };
            let escape = shortcut
                .trigger()
                .is_some_and(|trigger| trigger.to_str() == "Escape");
            if let (true, Some(action)) = (escape, shortcut.action()) {
                action.activate(gtk::ShortcutActionFlags::empty(), window, None);
                return Ok(());
            }
        }
    }
    Err("  the dialog has no shortcut for Escape".to_owned())
}

/// The button labeled `label` in `widget`.
pub(crate) fn button_in(widget: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    all::<gtk::Button>(widget)
        .into_iter()
        .find(|button| button.label().is_some_and(|text| text == label))
}

/// Closes `dialog` the way `how` says: its button `button`, Escape, or the window's close
/// request (the compositor's close, `swaymsg kill`).
pub(crate) fn close_dialog(dialog: &gtk::Window, how: &str, button: &str) -> Outcome {
    match how {
        "button" => button_in(dialog.upcast_ref(), button)
            .ok_or_else(|| format!("  no {button} button"))?
            .emit_clicked(),
        "escape" => press_escape(dialog)?,
        _ => dialog.close(),
    }
    Ok(())
}

/// _Add Printer…_ and the details open their dialog with its content, every way of closing one
/// closes it, and it opens again with its content: never an empty window that can't be closed.
pub(crate) fn check_printers_dialogs() -> Outcome {
    let stand_in = printers_stand_in(true)?;
    let page = PrintersPage::new(&print_service(&stand_in));
    let window = printers_window(&page, 2)?;
    for how in ["button", "escape", "close-request", "button"] {
        page.add_button().emit_clicked();
        let dialog = page.add_dialog().ok_or("  no dialog")?;
        ensure(
            run_until(Duration::from_secs(5), || dialog.window.is_visible())
                && button_in(dialog.window.upcast_ref(), "Close").is_some(),
            &format!("  Add Printer opened without its content (before closing by {how})"),
        )?;
        close_dialog(&dialog.window, how, "Close")?;
        ensure(
            run_until(Duration::from_secs(5), || {
                page.add_dialog().is_none() && !dialog.window.is_visible()
            }),
            &format!("  Add Printer didn't close by {how}"),
        )?;
    }
    for how in ["button", "escape", "close-request", "button"] {
        page.activate_row("Office");
        let dialog = page.details_dialog().ok_or("  no details dialog")?;
        let dialog = dialog.window;
        ensure(
            run_until(Duration::from_secs(5), || dialog.is_visible())
                && button_in(dialog.upcast_ref(), "Close").is_some(),
            &format!("  Details opened without its content (before closing by {how})"),
        )?;
        close_dialog(&dialog, how, "Close")?;
        ensure(
            run_until(Duration::from_secs(5), || {
                page.details_dialog().is_none() && !dialog.is_visible()
            }),
            &format!("  Details didn't close by {how}"),
        )?;
    }
    window.destroy();
    Ok(())
}

/// Printers as an office has them: the default with its toner, one paused and low on toner, one
/// out of paper with jobs waiting, and one that is off.
pub(crate) const OFFICE_CONF: &str = "\
<DefaultPrinter Office>
Info Office
Location Second floor
DeviceURI ipp://127.0.0.1:9/ipp/print
State Idle
Type 4
Accepting Yes
Shared No
Attribute marker-colors \\#000000
Attribute marker-levels 64
Attribute marker-low-levels 10
Attribute marker-high-levels 100
Attribute marker-names Black Toner
Attribute marker-types toner
</DefaultPrinter>
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
<Printer Lab>
Info Lab Color
Location Lab
DeviceURI ipp://127.0.0.1:9/ipp/print
State Idle
Reason media-empty-error
Type 4
Accepting Yes
Shared Yes
Attribute marker-colors \\#00FFFF,\\#FF00FF,\\#FFFF00,\\#000000
Attribute marker-levels 80,45,12,90
Attribute marker-low-levels 10,10,10,10
Attribute marker-high-levels 100,100,100,100
Attribute marker-names Cyan,Magenta,Yellow,Black
Attribute marker-types ink,ink,ink,ink
</Printer>
<Printer Basement>
Info Basement Laser
DeviceURI ipp://127.0.0.1:9/ipp/print
State Idle
Reason offline-report
Type 4
Accepting Yes
Shared No
</Printer>
";

/// Screenshots of the page in `scheme` with [`OFFICE_CONF`], two jobs waiting on Front Desk and a
/// driverless printer on the network that isn't added: the list, the details of a printer and
/// _Add Printer…_.
pub(crate) fn shoot_printers(scheme: &str) -> Outcome {
    let stand_in = CupsStandIn::start_with(true, OFFICE_CONF)
        .map_err(|error| format!("  CUPS's stand-in: {error}"))?;
    for _ in 0..2 {
        async_io::block_on(stand_in.cups().print_test_page("Seeded"))
            .map_err(|error| format!("  a test page: {error}"))?;
    }
    let services = async_io::block_on(sway_things_core::printers::stand_in::Services::start(
        false, false,
    ))
    .map_err(|error| format!("  Avahi's stand-in: {error}"))?;
    async_io::block_on(services.announce(
        "Brother HL-L2350DW series",
        "_ipps._tcp",
        0,
        &[
            "ty=Brother HL-L2350DW series",
            "note=Reception",
            "rp=ipp/print",
        ],
    ))
    .map_err(|error| format!("  announcing: {error}"))?;
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.set_default_size(1100, 820);
    window.use_print_service(PrintService::At {
        socket: stand_in.socket().to_owned(),
        bus: Some(services.address().to_owned()),
    });
    let sane = SaneFixture::new("test\n")?;
    if let Ok(program) = scanimage() {
        window.use_scan_service(sane.service(&program, Some(services.address())));
    }
    async_io::block_on(services.announce(
        "Hall Scanner",
        "_uscan._tcp",
        0,
        &["ty=Brother ADS-1700W"],
    ))
    .map_err(|error| format!("  announcing: {error}"))?;
    window.present();
    window.show_page(Page::PrintersScanners);
    settle(&window)?;
    let page = widget::<PrintersPage>(&window)?;
    ensure(
        run_until(Duration::from_secs(30), || {
            page.rows().len() == 4 && !page.searching_scanners() && !page.scanner_hints().is_empty()
        }),
        &format!("  the printers didn't show: {:?}", page.rows()),
    )?;
    shoot_checked(window.upcast_ref(), &format!("print-page-{scheme}"))?;
    if scanimage().is_ok() {
        let dialog = scan_dialog(&page, "test:0")?;
        let [preview, ..] = dialog.buttons();
        preview.emit_clicked();
        run_until(Duration::from_secs(30), || !dialog.is_scanning());
        shoot_checked(&dialog.window, &format!("scan-dialog-{scheme}"))?;
        // Two pages of a PDF, waiting to be saved; then saved.
        dialog.choose(2, "75 dpi");
        let [_, primary, another, ..] = dialog.buttons();
        primary.emit_clicked();
        run_until(Duration::from_secs(30), || !dialog.is_scanning());
        another.emit_clicked();
        run_until(Duration::from_secs(30), || !dialog.is_scanning());
        shoot_checked(&dialog.window, &format!("scan-pages-{scheme}"))?;
        let out = tempfile::tempdir().map_err(|error| error.to_string())?;
        dialog.save_to(&out.path().join("Scan 2026-11-22 09.31.pdf"));
        run_until(Duration::from_secs(30), || !dialog.is_saving());
        shoot_checked(&dialog.window, &format!("scan-saved-{scheme}"))?;
        dialog.close();
    }
    page.activate_row("Seeded");
    let details = page.details_dialog().ok_or("  no details")?;
    settle(&details.window)?;
    shoot_checked(&details.window, &format!("print-details-{scheme}"))?;
    details.close();
    page.add_button().emit_clicked();
    if let Some(add) = page.add_dialog() {
        run_until(Duration::from_secs(3), || add.found.first_child().is_some());
        shoot_checked(&add.window, &format!("print-add-{scheme}"))?;
        add.window.close();
    }
    window.destroy();
    drop(services);
    Ok(())
}
