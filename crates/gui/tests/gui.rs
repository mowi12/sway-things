// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! GUI tests: they need a display, so a plain `cargo test` skips them. scripts/check-integration.sh
//! runs them with `-- --ignored` against the headless sway of each CI container.
//!
//! - Contrast: the window is drawn with every page open, in GTK's own theme, light and dark, and
//!   every text in it must reach WCAG AA against what is drawn under it.
//! - Keyboard: the sidebar of the window is navigated and opened as the keyboard does it.
//! - Deep links: a link opens its page, and with a setting, focuses and tints its row (expanding
//!   its group first).
//! - Narrow mode: the four real pages fit next to the sidebar at the threshold with every row's
//!   controls beside its text, every page fits a third of a laptop screen and scrolls sideways
//!   when narrower, a narrow row puts its controls below its text; below the threshold the
//!   window shows one pane at a time, keeps the page and the pane with the focus when it switches,
//!   and Esc, the back button, search and deep links move between the panes.
//! - Search: results replace the pages, a result opens its page and points to its row, pages
//!   without settings are found by name, and Esc goes back and ends the search.
//! - Rows: the pages built from the registry have a row per setting, a change reaches the
//!   settings, a locked setting can't be changed, a failed change shows its error, and the line
//!   of the sway config shows while its preference is on.
//! - Groups: the settings of one directive are rows below a group row, which shows their values
//!   and the line once, and expands and collapses with `Enter` and `Space`.
//! - Safety net: a risky change shows the Keep / Revert banner and is stored only when kept.
//! - Sections: a header collapses and expands its section, which is remembered in `ui.toml`
//!   while the preference is on, and a deep link opens a collapsed section without storing it.
//! - About: About & Maintenance shows the system details (versions of Sway Things, the
//!   compositor, GTK, …, and the delegated tools), copies them as text, and the project's links.
//! - Generated config: the footers and About & Maintenance open the window at their file, it
//!   follows changes, and a file changed outside has its banner, its content on disk and an
//!   Overwrite that asks first and only works with the include line.
//! - Reload: a change sway only reads from its config shows the reload banner, which goes once
//!   sway reloaded; a setting sway only reads at login has its badge.
//! - Indicators: reset, this machine only, the badges of a conflict with the user's config and
//!   its ⋮ menu, the badge of a requirement, and the reset of a whole page.
//! - Devices: the device selectors list the devices of their type, a row changes the value of
//!   the device picked, a type without devices shows a hint, and devices come and go live.
//! - Keyboard page: the layout list moves, adds and removes layouts with their variants through
//!   the safety net, keeps the last layout, the XKB options change through their drop-downs,
//!   switch and browser, the model is a drop-down, and the active layout switches.
//!
//! The window works on settings files in a temporary directory and a sway config in memory,
//! without a session or in the session of the stand-in for sway. It never saves by itself, and
//! the files of a reload are validated with `true`, so nothing runs sway.
//!
//! GTK runs on one thread, so this is a plain program (`harness = false`), not `#[test]`s. With
//! `SWAY_THINGS_SCREENSHOTS=<dir>` it also saves the window with each page as a PNG there.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{glib, gsk};
use sway_things_core::apply::Applier;
use sway_things_core::bluetooth::stand_in::StandIn;
use sway_things_core::command::input::InputType;
use sway_things_core::generate::{Locations, SwayValidator};
use sway_things_core::include::IncludeStatus;
use sway_things_core::ipc::Compositor;
use sway_things_core::ipc::stand_in::{
    FakeSway, SWAY_1_10, obedient_with_inputs, with_inputs, with_outputs,
};
use sway_things_core::network::stand_in::{self as nm_stand_in, StandIn as NmStandIn};
use sway_things_core::registry::{ConfigFile, Link, Page, Registry, Value};
use sway_things_core::state::Layer;
use sway_things_core::state::{Paths, State};
use sway_things_core::sway_config::MemoryFiles;
use sway_things_core::version::{SwayFxVersion, SwayVersion};
use sway_things_core::view::Source;
use sway_things_gui::contrast::{self, TextSample};
use sway_things_gui::model::Model;
use sway_things_gui::style;
use sway_things_gui::theme;
use sway_things_gui::widgets::{
    Arrangement, BluetoothBus, BluetoothPage, DeviceSelector, FULL_OFFSET, GroupRow, LayoutList,
    NetworkBus, NetworkPage, OptionBrowser, OutputPicker, PageView, SettingRow, TestArea,
};
use sway_things_gui::window::{NARROW_WIDTH, Window};

/// The theme the window is checked in: the one GTK ships, so results don't depend on the machine.
const THEME: &str = "Default";

type Outcome = Result<(), String>;

/// SwayFX, so that no setting is locked.
const SWAYFX: Compositor = Compositor::SwayFx {
    version: SwayFxVersion::OLDEST_SUPPORTED,
    base: SwayVersion::new(1, 11, 0),
};
const SWAY: Compositor = Compositor::Sway {
    version: SwayVersion::new(1, 10, 1),
};
/// Values stored in the settings file, so rows show stored values too.
const STORED: &str = "version = 1\n\n[input.touchpad]\npointer_accel = 0.3\n\n\
                      [windows.focused]\nchild_border = \"#3b6e4f\"\n";
/// The preference that shows the line of the sway config in every row, so the contrast checks
/// cover it.
const DIRECTIVES: &str = "\n[app]\nshow_directives = true\n";
/// Lines of the user's config before the include line, so rows show where a value comes from.
const USERS: &str = "gaps inner 6\nfocus_follows_mouse no\n";

/// A model on settings files in a temporary directory, which goes away with it.
struct Fixture {
    _dir: tempfile::TempDir,
    model: Model,
    /// Where the collapsed sections are saved.
    ui: PathBuf,
    main: PathBuf,
    locations: Locations,
}

impl Fixture {
    /// The include line of the generated files.
    fn include_line(&self) -> String {
        let file = self.locations.path(ConfigFile::Input);
        let directory = file.parent().unwrap_or(Path::new("/"));
        format!("include {}/*.conf\n", directory.display())
    }

    /// The include status of a sway config with `users` and then, if `include`, the include line
    /// of the generated files.
    fn status(&self, users: &str, include: bool) -> IncludeStatus {
        let mut text = users.to_owned();
        if include {
            text.push_str(&self.include_line());
        }
        self.status_of(&text)
    }

    /// The include status of a sway config with `text`.
    fn status_of(&self, text: &str) -> IncludeStatus {
        let files = MemoryFiles::new().file(self.main.to_string_lossy().as_ref(), text);
        IncludeStatus::detect(Some(&self.main), &files, &self.locations)
    }
}

/// A model with `stored` in the settings file, for a sway config with `users` and then, if
/// `include`, the include line of the generated files.
fn fixture(
    compositor: Compositor,
    stored: &str,
    users: &str,
    include: bool,
) -> Result<Fixture, String> {
    fixture_in(compositor, stored, users, include, None)
}

/// A model like [`fixture`] with the include line, in the session of the stand-in `sway`, once
/// it has read the devices.
fn live_fixture(sway: &FakeSway, stored: &str) -> Result<Fixture, String> {
    let fixture = fixture_in(SWAY, stored, USERS, true, Some(sway))?;
    let known = run_until(Duration::from_secs(5), || fixture.model.devices_known());
    ensure(known, "  the devices weren't read within 5 s")?;
    Ok(fixture)
}

const KEYBOARD: &str = r#"{"identifier": "1:1:AT_Translated_Set_2_keyboard",
    "name": "AT Translated Set 2 keyboard", "type": "keyboard",
    "libinput": {"send_events": "enabled"}}"#;
const MOUSE_ID: &str = "1133:16511:Logitech_MX_Master_3";
const MOUSE: &str = r#"{"identifier": "1133:16511:Logitech_MX_Master_3", "name": "MX Master 3",
    "type": "pointer", "libinput": {"send_events": "enabled"}}"#;
const TOUCHPAD: &str = r#"{"identifier": "1739:52619:Synaptics_TM3276-022",
    "name": "Synaptics TM3276-022", "type": "touchpad",
    "libinput": {"send_events": "enabled"}}"#;

/// A laptop's sway with an external mouse: a keyboard, a mouse and a touchpad, and every
/// command succeeds. Switching off all mice leaves the touchpad.
fn laptop() -> FakeSway {
    obedient_with_inputs(SWAY_1_10, format!("[{KEYBOARD}, {MOUSE}, {TOUCHPAD}]"))
}

fn fixture_in(
    compositor: Compositor,
    stored: &str,
    users: &str,
    include: bool,
    sway: Option<&FakeSway>,
) -> Result<Fixture, String> {
    fixture_with_ui(compositor, stored, users, include, sway, None)
}

/// A model like [`fixture_in`] that starts with `ui` as `ui.toml`, the collapsed sections.
fn fixture_with_ui(
    compositor: Compositor,
    stored: &str,
    users: &str,
    include: bool,
    sway: Option<&FakeSway>,
    ui: Option<&str>,
) -> Result<Fixture, String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let config = dir.path().join("config");
    std::fs::create_dir_all(&config).map_err(|error| error.to_string())?;
    std::fs::write(config.join("settings.toml"), stored).map_err(|error| error.to_string())?;
    if let Some(ui) = ui {
        let state = dir.path().join("state");
        std::fs::create_dir_all(&state).map_err(|error| error.to_string())?;
        std::fs::write(state.join("ui.toml"), ui).map_err(|error| error.to_string())?;
    }
    let paths = Paths::new(&config, &dir.path().join("state"), "host")
        .map_err(|error| error.to_string())?;
    let state = State::load(Registry::builtin(), paths);
    let generated = dir.path().join("sway/sway-things");
    let locations = Locations::new(&generated, &dir.path().join("state"));
    let mut fixture = Fixture {
        model: Model::default(),
        ui: dir.path().join("state/ui.toml"),
        main: dir.path().join("sway/config"),
        locations: locations.clone(),
        _dir: dir,
    };
    let status = fixture.status(users, include);
    // `true` accepts every file, so a reload of sway saves without running sway.
    let validator = SwayValidator::with_program("true");
    let applier = match sway {
        Some(sway) => Applier::with_session(state, locations, validator, sway.connect()),
        None => Applier::without_session(state, locations, validator, compositor),
    };
    fixture.model = Model::with_applier(applier, status);
    Ok(fixture)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--list") {
        return ExitCode::SUCCESS;
    }
    if !args
        .iter()
        .any(|arg| arg == "--ignored" || arg == "--include-ignored")
    {
        println!("GUI tests skipped: they need a display (run with `-- --ignored`)");
        return ExitCode::SUCCESS;
    }
    if let Err(error) = gtk::init() {
        eprintln!("error: can't initialize GTK: {error}");
        return ExitCode::FAILURE;
    }
    style::load();
    sway_things_gui::icons::register();
    let Some(settings) = gtk::Settings::default() else {
        eprintln!("error: no GTK settings");
        return ExitCode::FAILURE;
    };
    settings.set_gtk_theme_name(Some(THEME));
    // Every screen is checked as it ends up: a banner sliding in needs a new layout each frame,
    // and GTK draws nothing of a window while its layout is due.
    settings.set_gtk_enable_animations(false);
    let screenshots = std::env::var_os("SWAY_THINGS_SCREENSHOTS").map(PathBuf::from);

    let mut results: Vec<(String, Outcome)> = Vec::new();
    // The Bluetooth page shows the stand-in's devices; without python-dbusmock (screenshots on
    // a desktop) it shows Bluetooth as unavailable, never the machine's own.
    let bluetooth = bluetooth_stand_in();
    // Likewise the Network page and NetworkManager.
    let network = network_stand_in().map(|(stand_in, _)| stand_in);
    let fixture = match fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true) {
        Ok(fixture) => fixture,
        Err(error) => {
            eprintln!("error: can't set up the settings: {error}");
            return ExitCode::FAILURE;
        }
    };
    for dark in [false, true] {
        set_dark(&settings, dark);
        let scheme = if dark { "dark" } else { "light" };
        let window = Window::with_model(&fixture.model);
        use_stand_in(&window, bluetooth.as_ref().ok());
        if let Ok(network) = &network {
            window.use_network_bus(NetworkBus::Address(network.address().to_owned()));
        }
        window.present();
        for page in Page::ALL {
            let outcome = check_page(&window, page, screenshots.as_deref(), scheme);
            results.push((format!("contrast: {} ({scheme})", page.id()), outcome));
        }
        window.destroy();
        results.extend(check_screens(screenshots.as_deref(), scheme));
    }
    set_dark(&settings, false);
    results.push(("keyboard: sidebar".to_owned(), check_sidebar_keyboard()));
    results.push(("keyboard: Esc goes back".to_owned(), check_escape()));
    results.extend(check_links());
    results.extend(check_searches());
    results.extend(check_narrow());
    results.extend(check_setting_rows());
    results.extend(check_safety_net());
    results.extend(check_reload());
    results.extend(check_generated());
    results.push((
        "about: the versions and the project's links".to_owned(),
        check_about(),
    ));
    results.extend(check_sections());
    results.push((
        "theme: the accent follows sway's focus color".to_owned(),
        check_accent(),
    ));
    results.extend(check_devices());
    results.extend(check_displays());
    results.extend(check_bluetooth_pages());
    results.extend(check_network_pages());
    results.extend(check_keyboard());
    results.extend(check_indicators());
    results.extend(check_welcomes(&settings));

    let mut failed = 0;
    for (name, outcome) in &results {
        match outcome {
            Ok(()) => println!("test {name} ... ok"),
            Err(message) => {
                failed += 1;
                println!("test {name} ... FAILED\n{message}");
            }
        }
    }
    println!(
        "\nGUI test result: {} passed; {failed} failed",
        results.len() - failed
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The screens beyond the pages, checked for contrast in the color `scheme`.
fn check_screens(screenshots: Option<&Path>, scheme: &str) -> Vec<(String, Outcome)> {
    let mut results = Vec::new();
    let (welcome, banner) = check_read_only_screens(screenshots, scheme);
    results.push((format!("contrast: welcome screen ({scheme})"), welcome));
    results.push((format!("contrast: read-only banner ({scheme})"), banner));
    results.push((
        format!("contrast: safety-net banner ({scheme})"),
        check_safety_net_screen(screenshots, scheme),
    ));
    results.push((
        format!("contrast: reload banner ({scheme})"),
        check_reload_screen(screenshots, scheme),
    ));
    results.push((
        format!("contrast: generated config ({scheme})"),
        check_generated_screen(screenshots, scheme),
    ));
    results.push((
        format!("contrast: indicators ({scheme})"),
        check_indicators_screen(screenshots, scheme),
    ));
    results.push((
        format!("contrast: a type without devices ({scheme})"),
        check_no_devices_screen(screenshots, scheme),
    ));
    results.push((
        format!("contrast: search results ({scheme})"),
        check_search_screens(screenshots, scheme),
    ));
    results
}

/// The checks of the rows built from the registry.
fn check_setting_rows() -> Vec<(String, Outcome)> {
    let mut results = vec![
        ("rows: one per setting".to_owned(), check_rows()),
        (
            "rows: a change reaches the settings".to_owned(),
            check_change(),
        ),
        (
            "rows: a locked setting can't change".to_owned(),
            check_locked(),
        ),
        ("rows: a failed change shows why".to_owned(), check_failed()),
        (
            "rows: a dragged slider snaps to the default, keys don't".to_owned(),
            check_slider_snap(),
        ),
        (
            "rows: a control keeps its place when the reset buttons appear".to_owned(),
            check_control_stays(),
        ),
        (
            "rows: a setting waits for the one it depends on".to_owned(),
            check_dependency(),
        ),
        (
            "rows: the directive line follows the preference".to_owned(),
            check_directives(),
        ),
        (
            "groups: one row with the values and the line".to_owned(),
            check_group_row(),
        ),
        (
            "groups: Enter and Space expand and collapse".to_owned(),
            check_group_keyboard(),
        ),
    ];
    results.extend(check_editor());
    results
}

/// The checks of the safety net for risky changes.
fn check_safety_net() -> Vec<(String, Outcome)> {
    vec![
        ("safety net: keep".to_owned(), check_keep()),
        ("safety net: revert".to_owned(), check_revert()),
        (
            "safety net: the countdown reverts".to_owned(),
            check_countdown(),
        ),
        ("safety net: lockout protection".to_owned(), check_lockout()),
        (
            "safety net: closing the window reverts".to_owned(),
            check_close_reverts(),
        ),
    ]
}

/// The checks of the reload banner and the badges of settings that don't apply live.
fn check_reload() -> Vec<(String, Outcome)> {
    vec![
        (
            "reload: the banner counts the waiting changes and goes after the reload".to_owned(),
            check_reload_banner(),
        ),
        (
            "reload: a setting for the next login has its badge".to_owned(),
            check_relogin_badge(),
        ),
    ]
}

const WORKSPACE_LAYOUT: &str = "behavior.workspace_layout";
const XWAYLAND: &str = "behavior.xwayland";

/// A window on the Window Behavior page in the session of `sway`, with the default layout of new
/// workspaces changed, which waits for a reload of sway.
fn reload_waits(sway: &FakeSway) -> Result<(Fixture, Window), String> {
    let fixture = live_fixture(sway, STORED)?;
    let window = open(&fixture, Page::WindowBehavior)?;
    ensure(
        window.reload_banner().is_none(),
        "the reload banner shows before a change",
    )?;
    fixture
        .model
        .set(WORKSPACE_LAYOUT, None, Value::Choice("tabbed".to_owned()));
    wait_for_model(&fixture.model)?;
    settle(&window)?;
    Ok((fixture, window))
}

/// A change for the config only shows the banner; a change for the next login doesn't count;
/// the reload saves, asks sway and hides the banner.
fn check_reload_banner() -> Outcome {
    let sway = laptop();
    let (fixture, window) = reload_waits(&sway)?;
    let expected = Some("1 change applies after sway reloads its config".to_owned());
    ensure(
        window.reload_banner() == expected,
        &format!("  the reload banner says {:?}", window.reload_banner()),
    )?;
    ensure(
        window.reload_button().is_sensitive(),
        "Reload sway… can't be pressed",
    )?;
    fixture
        .model
        .set(XWAYLAND, None, Value::Choice("force".to_owned()));
    wait_for_model(&fixture.model)?;
    ensure(
        window.reload_banner() == expected,
        &format!(
            "  after a change for the next login the banner says {:?}",
            window.reload_banner()
        ),
    )?;
    fixture.model.reload_sway();
    wait_for_model(&fixture.model)?;
    ensure(
        sway.commands().iter().any(|command| command == "reload"),
        "sway wasn't asked to reload",
    )?;
    ensure(
        window.reload_banner().is_none(),
        "the reload banner stays after the reload",
    )?;
    ensure(
        window.problem().is_none(),
        &format!("  problem: {:?}", window.problem()),
    )?;
    let generated = std::fs::read_to_string(fixture.locations.path(ConfigFile::Behavior))
        .map_err(|error| format!("  {error}"))?;
    ensure(
        generated.contains("workspace_layout tabbed"),
        "the change wasn't saved before the reload",
    )?;
    window.destroy();
    Ok(())
}

/// Xwayland, which sway only reads at login, says so in a badge.
fn check_relogin_badge() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::WindowBehavior)?;
    let xwayland = row(&window, XWAYLAND)?;
    ensure(
        xwayland.badges() == ["Applies after re-login"],
        &format!("  Xwayland has the badges {:?}", xwayland.badges()),
    )?;
    let layout = row(&window, WORKSPACE_LAYOUT)?;
    ensure(
        layout.badges() == ["Applies after reload"],
        &format!("  the default layout has the badges {:?}", layout.badges()),
    )?;
    window.destroy();
    Ok(())
}

/// The Window Behavior page with the reload banner, checked like the pages.
fn check_reload_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let sway = laptop();
    let (_fixture, window) = reload_waits(&sway)?;
    ensure(window.reload_banner().is_some(), "no reload banner")?;
    let directory = screenshots.map(|dir| dir.join("reload"));
    let outcome = check_page(&window, Page::WindowBehavior, directory.as_deref(), scheme);
    window.destroy();
    outcome
}

/// The checks of the collapsible sections.
fn check_sections() -> Vec<(String, Outcome)> {
    vec![
        (
            "sections: a header collapses and expands, and it is remembered".to_owned(),
            check_section_collapse(),
        ),
        (
            "sections: collapsed ones come back, unless the preference is off".to_owned(),
            check_sections_restored(),
        ),
        (
            "sections: a deep link opens a collapsed section without storing it".to_owned(),
            check_section_link(),
        ),
    ]
}

/// The id, header and expansion of the section `id` on the open page of `window`.
fn section(window: &Window, id: &str) -> Result<(gtk::Button, bool), String> {
    page_view(window)
        .and_then(|view| {
            view.sections()
                .into_iter()
                .find(|(section, _, _)| section == id)
        })
        .map(|(_, header, expanded)| (header, expanded))
        .ok_or_else(|| format!("  no section {id}"))
}

/// The text of `ui.toml` of `fixture`, if it was written.
fn ui_file(fixture: &Fixture) -> Option<String> {
    std::fs::read_to_string(&fixture.ui).ok()
}

/// Every section starts expanded; a header collapses its section and is saved, and expands it
/// again.
fn check_section_collapse() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let view = page_view(&window).ok_or_else(|| "  no page".to_owned())?;
    ensure(
        view.sections().iter().all(|(_, _, expanded)| *expanded),
        "a section starts collapsed",
    )?;
    let ids: Vec<String> = view.sections().into_iter().map(|(id, _, _)| id).collect();
    ensure(
        ids.first().map(String::as_str) == Some("mouse")
            && ids.last().map(String::as_str) == Some("test-area"),
        &format!("  the sections are {ids:?}"),
    )?;
    let (header, _) = section(&window, "mouse")?;
    let speed = row(&window, "input.pointer.pointer_accel")?;
    // `activate` is what Enter and Space do on the focused header.
    header.activate();
    // A button clicks a moment after it is activated.
    let collapsed = run_until(Duration::from_secs(2), || {
        section(&window, "mouse").is_ok_and(|(_, expanded)| !expanded)
    });
    ensure(collapsed, "Enter on the header didn't collapse the section")?;
    ensure(
        !speed.is_mapped() || !speed.is_drawable(),
        "the rows stay shown",
    )?;
    ensure(
        ui_file(&fixture).is_some_and(|text| text.contains("mouse-touchpad = [\"mouse\"]")),
        &format!("  ui.toml holds {:?}", ui_file(&fixture)),
    )?;
    header.emit_clicked();
    ensure(
        section(&window, "mouse")?.1,
        "a click didn't expand the section",
    )?;
    ensure(
        ui_file(&fixture).is_some_and(|text| !text.contains("mouse-touchpad")),
        &format!("  after expanding, ui.toml holds {:?}", ui_file(&fixture)),
    )?;
    window.destroy();
    Ok(())
}

/// A section collapsed in `ui.toml` starts collapsed; with the preference off, it doesn't and
/// nothing is saved.
fn check_sections_restored() -> Outcome {
    let ui = "version = 1\n[collapsed]\nwindows = [\"gaps\", \"no-such-section\"]\n";
    let fixture = fixture_with_ui(SWAYFX, STORED, USERS, true, None, Some(ui))?;
    let window = open(&fixture, Page::Windows)?;
    ensure(
        !section(&window, "gaps")?.1,
        "the stored section isn't collapsed",
    )?;
    ensure(
        section(&window, "borders")?.1,
        "another section is collapsed",
    )?;
    window.destroy();

    let off = format!("{STORED}\n[app]\nremember_collapsed_sections = false\n");
    let fixture = fixture_with_ui(SWAYFX, &off, USERS, true, None, Some(ui))?;
    let window = open(&fixture, Page::Windows)?;
    ensure(
        section(&window, "gaps")?.1,
        "with the preference off, a section starts collapsed",
    )?;
    let (header, _) = section(&window, "borders")?;
    header.emit_clicked();
    ensure(
        !section(&window, "borders")?.1,
        "the header didn't collapse the section",
    )?;
    ensure(
        ui_file(&fixture).as_deref() == Some(ui),
        &format!(
            "  with the preference off, ui.toml became {:?}",
            ui_file(&fixture)
        ),
    )?;
    window.destroy();
    Ok(())
}

/// A link to a setting in a collapsed section expands it, and the file still has it collapsed.
fn check_section_link() -> Outcome {
    let ui = "version = 1\n[collapsed]\nwindows = [\"gaps\"]\n";
    let fixture = fixture_with_ui(SWAYFX, STORED, USERS, true, None, Some(ui))?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let link = Link::parse("windows.gaps.inner", Registry::builtin())
        .map_err(|error| error.to_string())?;
    window.open_link(&link);
    settle(&window)?;
    ensure(
        section(&window, "gaps")?.1,
        "the link didn't expand the section",
    )?;
    ensure(
        ui_file(&fixture).as_deref() == Some(ui),
        &format!("  the link changed ui.toml to {:?}", ui_file(&fixture)),
    )?;
    window.destroy();
    Ok(())
}

/// The checks of the window of "Show generated config".
fn check_generated() -> Vec<(String, Outcome)> {
    vec![
        (
            "generated: a footer opens its file and follows changes".to_owned(),
            check_generated_footer(),
        ),
        (
            "generated: About & Maintenance opens it".to_owned(),
            check_generated_about(),
        ),
        (
            "generated: a file changed outside can be seen and overwritten".to_owned(),
            check_generated_edited(),
        ),
        (
            "generated: a foreign file and a read-only one aren't overwritten".to_owned(),
            check_generated_not_overwritten(),
        ),
    ]
}

/// The buttons labeled `label` in the open page of `window`.
fn buttons_labeled(window: &Window, label: &str) -> Vec<gtk::Button> {
    window
        .content()
        .map(|content| all::<gtk::Button>(&content))
        .unwrap_or_default()
        .into_iter()
        .filter(|button| button.label().as_deref() == Some(label))
        .collect()
}

/// The text the generated window shows for `file`, and its banner.
fn generated_state(
    window: &Window,
    file: ConfigFile,
) -> Result<(String, Option<String>, bool), String> {
    window
        .generated()
        .and_then(|generated| generated.file_state(file))
        .ok_or_else(|| format!("  no tab for {}", file.file_name()))
}

/// The Show button of the Windows page footer opens 30-appearance.conf with the stored colors;
/// a change shows at once.
fn check_generated_footer() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::Windows)?;
    let shows = buttons_labeled(&window, "Show");
    ensure(
        shows.len() == 2,
        &format!(
            "  the Windows footer has {} Show buttons, expected 2",
            shows.len()
        ),
    )?;
    shows[0].emit_clicked();
    let generated = window
        .generated()
        .ok_or_else(|| "  Show opened no window".to_owned())?;
    settle(&generated)?;
    ensure(
        generated.shown_file() == Some(ConfigFile::Appearance),
        &format!("  it opened at {:?}", generated.shown_file()),
    )?;
    let (text, banner, _) = generated_state(&window, ConfigFile::Appearance)?;
    ensure(
        text.contains("client.focused ") && text.contains("#3b6e4f"),
        &format!("  30-appearance.conf shows {text:?}"),
    )?;
    ensure(banner.is_none(), "a file that isn't written has a banner")?;
    // Copy contents copies the text, not the path.
    let copy = all::<gtk::Button>(generated.upcast_ref())
        .into_iter()
        .find(|button| {
            button.is_mapped() && button.tooltip_text().as_deref() == Some("Copy contents")
        })
        .ok_or_else(|| "  no copy button".to_owned())?;
    copy.emit_clicked();
    let copied = std::rc::Rc::new(std::cell::RefCell::new(None));
    generated
        .clipboard()
        .read_text_async(gtk::gio::Cancellable::NONE, {
            let copied = copied.clone();
            move |text| {
                copied.replace(Some(text.ok().flatten().map(|text| text.to_string())));
            }
        });
    run_until(Duration::from_secs(5), || copied.borrow().is_some());
    let copied = copied.borrow().clone().flatten();
    ensure(
        copied.as_deref() == Some(text.as_str()),
        &format!("  Copy contents copied {copied:?}"),
    )?;
    // Nothing was written to show it.
    ensure(
        !fixture.locations.path(ConfigFile::Appearance).exists(),
        "showing wrote the file",
    )?;
    fixture
        .model
        .set("windows.gaps.inner", None, Value::Integer(9));
    wait_for_model(&fixture.model)?;
    let (text, _, _) = generated_state(&window, ConfigFile::Appearance)?;
    ensure(
        text.contains("gaps inner 9"),
        &format!("  after a change it shows {text:?}"),
    )?;
    // The other button brings the same window to 90-effects.conf.
    shows[1].emit_clicked();
    ensure(
        window.generated().as_ref() == Some(&generated)
            && generated.shown_file() == Some(ConfigFile::Effects),
        "the second Show didn't switch the open window to 90-effects.conf",
    )?;
    generated.close();
    run_until(Duration::from_millis(100), || false);
    ensure(
        window.generated().is_none(),
        "the window stays after closing",
    )?;
    window.destroy();
    Ok(())
}

/// About & Maintenance starts with the system details, a button that copies them, and the
/// project's links.
fn check_about() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::About)?;
    let content = window
        .content()
        .ok_or_else(|| "  About & Maintenance has no content".to_owned())?;
    let copy = all::<gtk::Button>(&content)
        .into_iter()
        .find(|button| button.label().as_deref() == Some("Copy System Details"))
        .ok_or_else(|| "  no Copy System Details button".to_owned())?;
    ensure(
        run_until(Duration::from_secs(5), || copy.is_sensitive()),
        "  the system details weren't read within 5 s",
    )?;
    let texts: Vec<String> = all::<gtk::Label>(&content)
        .iter()
        .map(|label| label.label().to_string())
        .collect();
    let gtk_version = sway_things_gui::app::gtk_version();
    for expected in [
        "Sway Things",
        env!("CARGO_PKG_VERSION"),
        "Compositor",
        &SWAYFX.to_string(),
        "GTK",
        &gtk_version,
        "Distribution",
        "NetworkManager",
        // The window in tests talks to no bus.
        "Not running",
        "Kernel",
        "nm-connection-editor",
        "blueman-manager",
        "Website",
        "Source Code (GitHub)",
        "Report a Bug",
    ] {
        ensure(
            texts.iter().any(|text| text == expected),
            &format!("  no {expected:?} among {texts:?}"),
        )?;
    }
    let links: Vec<String> = all::<gtk::Button>(&content)
        .iter()
        .filter_map(|button| button.tooltip_text().map(|text| text.to_string()))
        .collect();
    let theme = gtk::IconTheme::for_display(&WidgetExt::display(&window));
    for icon in [
        sway_things_gui::icons::WEBSITE,
        sway_things_gui::icons::SOURCE_CODE,
        sway_things_gui::icons::BUG,
    ] {
        ensure(
            theme.has_icon(icon),
            &format!("  the icon theme has no {icon}"),
        )?;
    }
    let repository = env!("CARGO_PKG_REPOSITORY");
    for uri in [
        env!("CARGO_PKG_HOMEPAGE").to_owned(),
        repository.to_owned(),
        format!("{repository}/issues"),
    ] {
        ensure(
            links.contains(&uri),
            &format!("  no link to {uri} among {links:?}"),
        )?;
    }
    // The copied block has the same lines, in the order of the bug form.
    copy.emit_clicked();
    let copied = std::rc::Rc::new(std::cell::RefCell::new(None));
    copy.clipboard()
        .read_text_async(gtk::gio::Cancellable::NONE, {
            let copied = copied.clone();
            move |text| {
                copied.replace(Some(text.ok().flatten().map(|text| text.to_string())));
            }
        });
    run_until(Duration::from_secs(5), || copied.borrow().is_some());
    ensure(
        copy.label().as_deref() == Some("Copied"),
        &format!("  after a click the button says {:?}", copy.label()),
    )?;
    ensure(
        run_until(Duration::from_secs(5), || {
            copy.label().as_deref() == Some("Copy System Details")
        }),
        "  the button doesn't go back to Copy System Details",
    )?;
    let copied = copied.borrow().clone().flatten().unwrap_or_default();
    let start = format!(
        "Sway Things: {}\nCompositor: {SWAYFX}\nGTK: {gtk_version}\nDistribution: ",
        env!("CARGO_PKG_VERSION")
    );
    ensure(
        copied.starts_with(&start)
            && copied.contains("\nNetworkManager: not running\nKernel: ")
            && copied.contains("\nnm-connection-editor: ")
            && copied.ends_with("installed\n"),
        &format!("  Copy System Details copied {copied:?}"),
    )?;
    window.destroy();
    Ok(())
}

/// About & Maintenance has a section with a button that opens the window.
fn check_generated_about() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::About)?;
    let shows = buttons_labeled(&window, "Show\u{2026}");
    let show = shows
        .first()
        .ok_or_else(|| "  About & Maintenance has no Show… button".to_owned())?;
    show.emit_clicked();
    let generated = window
        .generated()
        .ok_or_else(|| "  Show… opened no window".to_owned())?;
    settle(&generated)?;
    ensure(
        generated.shown_file() == Some(ConfigFile::Input),
        &format!("  it opened at {:?}", generated.shown_file()),
    )?;
    window.destroy();
    run_until(Duration::from_millis(100), || false);
    let open = gtk::Window::list_toplevels()
        .iter()
        .any(|toplevel| toplevel == generated.upcast_ref::<gtk::Widget>());
    ensure(
        !open,
        &format!(
            "  the window stays after the main window is gone (visible: {}, mapped: {})",
            generated.is_visible(),
            generated.is_mapped()
        ),
    )?;
    Ok(())
}

/// A generated file with the header but other content was changed outside: banner, the content
/// on disk on request, and Overwrite writes what Sway Things generates.
fn check_generated_edited() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let path = fixture.locations.path(ConfigFile::Appearance);
    let edited = format!("{}gaps inner 30\n", sway_things_core::generate::HEADER);
    write_file(&path, &edited)?;
    let window = open(&fixture, Page::Windows)?;
    window.show_generated(Some(ConfigFile::Appearance));
    let generated = window
        .generated()
        .ok_or_else(|| "  no generated window".to_owned())?;
    settle(&generated)?;
    let (text, banner, can_overwrite) = generated_state(&window, ConfigFile::Appearance)?;
    ensure(
        banner
            .as_deref()
            .is_some_and(|banner| banner.starts_with("Changed outside")),
        &format!("  the banner says {banner:?}"),
    )?;
    ensure(can_overwrite, "Overwrite can't be pressed")?;
    ensure(
        !text.contains("gaps inner 30"),
        "it shows the file on disk at first",
    )?;
    let on_disk = generated
        .on_disk_button(ConfigFile::Appearance)
        .ok_or_else(|| "  no Show file on disk button".to_owned())?;
    on_disk.set_active(true);
    let (text, _, _) = generated_state(&window, ConfigFile::Appearance)?;
    ensure(text == edited, &format!("  on disk it shows {text:?}"))?;
    on_disk.set_active(false);

    fixture.model.overwrite(ConfigFile::Appearance);
    wait_for_model(&fixture.model)?;
    let written = std::fs::read_to_string(&path).map_err(|error| format!("  {error}"))?;
    ensure(
        written.contains("client.focused ") && !written.contains("gaps inner 30"),
        &format!("  the file holds {written:?} after Overwrite"),
    )?;
    let (_, banner, _) = generated_state(&window, ConfigFile::Appearance)?;
    ensure(banner.is_none(), "the banner stays after Overwrite")?;
    window.destroy();
    Ok(())
}

/// A file without the header isn't Sway Things', so it is never overwritten; without the
/// include line an edited file can't be overwritten either.
fn check_generated_not_overwritten() -> Outcome {
    let foreign = fixture(SWAYFX, STORED, USERS, true)?;
    write_file(
        &foreign.locations.path(ConfigFile::Behavior),
        "focus_follows_mouse no\n",
    )?;
    let window = open(&foreign, Page::WindowBehavior)?;
    window.show_generated(Some(ConfigFile::Behavior));
    let (_, banner, can_overwrite) = generated_state(&window, ConfigFile::Behavior)?;
    ensure(
        banner
            .as_deref()
            .is_some_and(|banner| banner.starts_with("Not made by Sway Things")),
        &format!("  the banner of a foreign file says {banner:?}"),
    )?;
    ensure(!can_overwrite, "a foreign file can be overwritten")?;
    window.destroy();

    let read_only = fixture(SWAYFX, STORED, USERS, false)?;
    write_file(
        &read_only.locations.path(ConfigFile::Input),
        &format!(
            "{}input * tap enabled\n",
            sway_things_core::generate::HEADER
        ),
    )?;
    let window = open(&read_only, Page::MouseTouchpad)?;
    window.look_around();
    window.show_generated(Some(ConfigFile::Input));
    let (_, banner, can_overwrite) = generated_state(&window, ConfigFile::Input)?;
    ensure(
        banner.is_some(),
        "an edited file has no banner while read-only",
    )?;
    ensure(!can_overwrite, "Overwrite can be pressed while read-only")?;
    window.destroy();
    Ok(())
}

/// Writes `text` to `path`, with the directories it needs.
fn write_file(path: &Path, text: &str) -> Outcome {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("  {error}"))?;
    }
    std::fs::write(path, text).map_err(|error| format!("  {error}"))
}

/// The generated window with a file changed outside, checked for contrast.
fn check_generated_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    write_file(
        &fixture.locations.path(ConfigFile::Appearance),
        &format!("{}gaps inner 30\n", sway_things_core::generate::HEADER),
    )?;
    let window = open(&fixture, Page::Windows)?;
    window.show_generated(Some(ConfigFile::Appearance));
    let generated = window
        .generated()
        .ok_or_else(|| "  no generated window".to_owned())?;
    settle(&generated)?;
    wait_for_paint(&generated)?;
    let path = screenshots.map(|dir| dir.join(format!("generated--{scheme}.png")));
    let problems = check_screen(&generated, path)?;
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Switches GTK between its light and dark style, as the desktop's preference would.
fn set_dark(settings: &gtk::Settings, dark: bool) {
    // GTK 4.20 replaced gtk-application-prefer-dark-theme with an interface color scheme.
    let scheme = settings
        .find_property("gtk-interface-color-scheme")
        .and_then(|property| glib::EnumClass::with_type(property.value_type()))
        .and_then(|class| class.to_value_by_nick(if dark { "dark" } else { "light" }));
    if let Some(scheme) = scheme {
        settings.set_property_from_value("gtk-interface-color-scheme", &scheme);
    } else {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
}

/// Runs the main loop until `done` holds or `timeout` has passed, and tells whether `done` holds.
/// It never blocks in the main loop, so it ends in time even when no event comes.
fn run_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + timeout;
    loop {
        while context.pending() {
            context.iteration(false);
        }
        if done() {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs the main loop until `window` is mapped and laid out, and nothing is pending.
fn settle(window: &impl IsA<gtk::Widget>) -> Outcome {
    if !run_until(Duration::from_secs(10), || {
        window.is_mapped() && window.width() > 0
    }) {
        return Err("  the window wasn't shown within 10 s".to_owned());
    }
    // A few frames for layout and style changes to land.
    run_until(Duration::from_millis(200), || false);
    Ok(())
}

/// Runs the main loop until `window` has painted a frame since now.
fn wait_for_paint(window: &impl IsA<gtk::Widget>) -> Outcome {
    let clock = window
        .frame_clock()
        .ok_or_else(|| "  the window has no frame clock".to_owned())?;
    let painted = std::rc::Rc::new(std::cell::Cell::new(false));
    let handler = clock.connect_after_paint({
        let painted = painted.clone();
        move |_| painted.set(true)
    });
    window.queue_draw();
    run_until(Duration::from_secs(10), || painted.get());
    clock.disconnect(handler);
    run_until(Duration::ZERO, || false);
    if painted.get() {
        Ok(())
    } else {
        Err("  the window didn't paint within 10 s".to_owned())
    }
}

fn render(window: &gtk::Window) -> Result<gsk::RenderNode, String> {
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(
        &snapshot,
        f64::from(window.width()),
        f64::from(window.height()),
    );
    snapshot
        .to_node()
        .ok_or_else(|| "nothing was drawn".to_owned())
}

/// Opens `page`, checks every text drawn in the window and saves a screenshot if asked. A page
/// longer than the window is scrolled through, a window height at a time.
fn check_page(window: &Window, page: Page, screenshots: Option<&Path>, scheme: &str) -> Outcome {
    window.show_page(page);
    // Expanded, so the rows of the members are checked too.
    for group in page_view(window).iter().flat_map(PageView::groups) {
        group.set_expanded(true);
    }
    settle(window)?;
    let scroller = window
        .content()
        .and_then(|content| find::<gtk::ScrolledWindow>(&content))
        .ok_or_else(|| "  the page has no scrolled window".to_owned())?;
    let adjustment = scroller.vadjustment();
    adjustment.set_value(0.0);
    let mut problems = String::new();
    let mut screen = 0;
    loop {
        wait_for_paint(window)?;
        let name = match screen {
            0 => format!("{}--{scheme}.png", page.id()),
            _ => format!("{}--{scheme}-{screen}.png", page.id()),
        };
        problems.push_str(&check_screen(
            window,
            screenshots.map(|dir| dir.join(&name)),
        )?);
        let bottom = adjustment.upper() - adjustment.page_size();
        if adjustment.value() >= bottom {
            break;
        }
        // A little overlap, so text cut at the edge is seen whole on the next screen.
        adjustment.set_value((adjustment.value() + adjustment.page_size() * 0.8).min(bottom));
        settle(window)?;
        screen += 1;
    }
    adjustment.set_value(0.0);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Checks every text drawn in the window as it is now, and saves it to `screenshot` if given.
/// Returns the problems, one per line.
fn check_screen(
    window: &impl IsA<gtk::Window>,
    screenshot: Option<PathBuf>,
) -> Result<String, String> {
    let node = render_drawn(window)?;
    let window = window.upcast_ref::<gtk::Window>();
    // The viewport starts at the window's origin, so pixels and node coordinates line up.
    #[expect(
        clippy::cast_precision_loss,
        reason = "window sizes are far below 2^24, where f32 starts to round"
    )]
    let (width, height) = (window.width() as f32, window.height() as f32);
    let texture = window
        .renderer()
        .ok_or_else(|| "  the window has no renderer".to_owned())?
        .render_texture(
            &node,
            Some(&gtk::graphene::Rect::new(0.0, 0.0, width, height)),
        );
    if let Some(path) = screenshot {
        save_png(&texture, &path)?;
    }
    let pixels = contrast::Pixels::new(&texture, width);
    let samples = contrast::text_samples(&node, &pixels);
    if samples.is_empty() {
        return Err("  no text was drawn".to_owned());
    }
    // WCAG exempts inactive controls (1.4.3), like those of a read-only page: GTK draws their
    // text faint on purpose.
    let inactive = insensitive_areas(window.upcast_ref(), window.upcast_ref());
    let mut problems = String::new();
    for sample in samples.iter().filter(|sample| {
        let center = gtk::graphene::Point::new(
            sample.bounds.x() + sample.bounds.width() / 2.0,
            sample.bounds.y() + sample.bounds.height() / 2.0,
        );
        !inactive.iter().any(|area| area.contains_point(&center))
    }) {
        if let Some(problem) = problem(sample) {
            let _ = writeln!(problems, "  {problem}");
        }
    }
    Ok(problems)
}

/// The first widget of type `T` in `widget` and below it.
fn find<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Option<T> {
    if let Ok(found) = widget.clone().downcast::<T>() {
        return Some(found);
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(found) = find(&widget) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

fn problem(sample: &TextSample) -> Option<String> {
    let place = format!(
        "text at ({:.0}, {:.0}) {:.0}×{:.0}, {}",
        sample.bounds.x(),
        sample.bounds.y(),
        sample.bounds.width(),
        sample.bounds.height(),
        sample.font
    );
    match &sample.colors {
        Ok(colors) if colors.ratio() < sample.required() => Some(format!(
            "{place}: {} on {} is {:.2}:1, needs {}:1",
            colors.foreground,
            colors.background,
            colors.ratio(),
            sample.required()
        )),
        Ok(_) => None,
        Err(unknown) => Some(format!("{place}: background unknown: {unknown:?}")),
    }
}

fn save_png(texture: &gtk::gdk::Texture, path: &Path) -> Outcome {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    texture
        .save_to_png(path)
        .map_err(|error| format!("can't save {}: {error}", path.display()))
}

fn ensure(condition: bool, message: &str) -> Outcome {
    if condition {
        Ok(())
    } else {
        Err(format!("  {message}"))
    }
}

/// Arrow keys move the focus through the sidebar without opening pages; Enter opens one.
fn check_sidebar_keyboard() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let sidebar = window.sidebar();
    let list = sidebar.list();

    ensure(
        window.page() == Page::ALL.first().copied(),
        "the first page isn't open",
    )?;
    ensure(sidebar.focus_selected(), "the sidebar can't take the focus")?;

    list.emit_move_cursor(gtk::MovementStep::DisplayLines, 1, false, false);
    let focused = list.focus_child().and_downcast::<gtk::ListBoxRow>();
    ensure(
        focused.as_ref().map(gtk::ListBoxRow::index) == Some(1),
        "Down doesn't move the focus to the second page",
    )?;
    ensure(
        window.page() == Page::ALL.first().copied(),
        "moving the focus opened a page",
    )?;

    list.emit_activate_cursor_row();
    ensure(
        window.page() == Page::ALL.get(1).copied(),
        "Enter doesn't open the focused page",
    )?;
    ensure(
        sidebar.selected() == Page::ALL.get(1).copied(),
        "the opened page isn't marked in the sidebar",
    )?;

    // Down from the last page of a group lands on the first page of the next, past the line and
    // the heading; only the groups after the first start below a line.
    let boundary = Page::ALL
        .windows(2)
        .position(|pair| pair[0].group() != pair[1].group())
        .and_then(|index| i32::try_from(index).ok())
        .ok_or_else(|| "the sidebar has only one group".to_owned())?;
    ensure(
        list.row_at_index(boundary)
            .is_some_and(|row| row.grab_focus()),
        "the last page of the first group can't take the focus",
    )?;
    list.emit_move_cursor(gtk::MovementStep::DisplayLines, 1, false, false);
    let focused = list.focus_child().and_downcast::<gtk::ListBoxRow>();
    ensure(
        focused.as_ref().map(gtk::ListBoxRow::index) == Some(boundary + 1),
        "Down doesn't skip the group separator and heading",
    )?;
    let has_separator = |index: i32| {
        list.row_at_index(index)
            .and_then(|row| row.header())
            .and_then(|header| header.first_child())
            .is_some_and(|child| child.is::<gtk::Separator>())
    };
    ensure(!has_separator(0), "the first group starts below a line")?;
    ensure(
        has_separator(boundary + 1),
        "the second group doesn't start below a line",
    )?;
    ensure(
        !has_separator(1),
        "a line separates two pages of the same group",
    )?;

    let mut unnamed = Vec::new();
    let mut index = 0;
    while let Some(row) = list.row_at_index(index) {
        if !row.is_focusable() {
            unnamed.push(format!("row {index} can't take the focus"));
        }
        index += 1;
    }
    ensure(
        usize::try_from(index).ok() == Some(Page::ALL.len()),
        "the sidebar doesn't list every page",
    )?;
    ensure(unnamed.is_empty(), &unnamed.join(", "))?;
    window.destroy();
    Ok(())
}

/// Esc in the content pane moves the focus back to the open page in the sidebar.
fn check_escape() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    window.show_page(Page::Keyboard);
    ensure(
        !window.back_to_sidebar(),
        "Esc acted with the focus in the sidebar",
    )?;

    // Unbuilt pages have nothing to focus, so a stand-in takes the focus in the content pane.
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    content.set_focusable(true);
    ensure(
        content.grab_focus(),
        "the content pane can't take the focus",
    )?;
    ensure(
        window.back_to_sidebar(),
        "Esc didn't go back to the sidebar",
    )?;
    let focused = gtk::prelude::GtkWindowExt::focus(&window);
    let row = focused.and_downcast::<gtk::ListBoxRow>();
    ensure(
        row.map(|row| row.index())
            == Page::ALL
                .iter()
                .position(|p| *p == Page::Keyboard)
                .and_then(|i| i32::try_from(i).ok()),
        "the focus isn't on the open page's row",
    )?;
    window.destroy();
    Ok(())
}

/// The checks of the deep links.
fn check_links() -> Vec<(String, Outcome)> {
    vec![
        ("deep links: page and setting".to_owned(), check_open_link()),
        (
            "deep links: a hidden row only opens the page".to_owned(),
            check_open_hidden_link(),
        ),
        (
            "deep links: a member of a group expands it".to_owned(),
            check_open_group_link(),
        ),
    ]
}

/// A link to a page opens it; a link to a setting opens its page, tints its row and gives its
/// control the focus, and the tint goes again.
fn check_open_link() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let registry = Registry::builtin();

    let link = Link::parse("windows", registry).map_err(|error| error.to_string())?;
    window.open_link(&link);
    ensure(
        window.page() == Some(Page::Windows),
        "  the page didn't open",
    )?;

    let key = "input.keyboard.repeat_rate";
    let link = Link::parse(key, registry).map_err(|error| error.to_string())?;
    window.open_link(&link);
    ensure(
        window.page() == Some(Page::Keyboard),
        "  the setting's page didn't open",
    )?;
    let row = row(&window, key)?;
    let tinted = || {
        row.parent()
            .is_some_and(|item| item.has_css_class(style::HIGHLIGHT))
    };
    ensure(tinted(), "  the row isn't tinted")?;
    let focused = run_until(Duration::from_secs(5), || {
        gtk::prelude::GtkWindowExt::focus(&window).is_some_and(|focus| focus.is_ancestor(&row))
    });
    ensure(focused, "  the row's control didn't get the focus")?;
    ensure(
        run_until(Duration::from_secs(5), || !tinted()),
        "  the tint didn't go",
    )?;
    window.destroy();
    Ok(())
}

/// A link to a setting of an input type without devices opens the page and points to nothing.
fn check_open_hidden_link() -> Outcome {
    let sway = obedient_with_inputs(SWAY_1_10, format!("[{KEYBOARD}, {MOUSE}]"));
    let fixture = live_fixture(&sway, "version = 1\n")?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let link = Link::parse("input.touchpad.tap", Registry::builtin())
        .map_err(|error| error.to_string())?;
    window.open_link(&link);
    ensure(
        window.page() == Some(Page::MouseTouchpad),
        "  the page didn't open",
    )?;
    let row = row(&window, "input.touchpad.tap")?;
    ensure(
        row.parent()
            .is_none_or(|item| !item.has_css_class(style::HIGHLIGHT)),
        "  a hidden row is tinted",
    )?;
    window.destroy();
    Ok(())
}

/// The checks of the search in the sidebar.
fn check_searches() -> Vec<(String, Outcome)> {
    vec![
        (
            "search: a result opens its setting".to_owned(),
            check_search(),
        ),
        (
            "search: pages are found by name".to_owned(),
            check_search_pages(),
        ),
        (
            "search: a member of a group".to_owned(),
            check_search_group(),
        ),
        (
            "search: the sidebar keeps its width".to_owned(),
            check_search_width(),
        ),
    ]
}

/// A member of a group is found by the group's title, its result says where it is, and opening
/// it expands the group and tints the member's row.
fn check_search_group() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let sidebar = window.sidebar();
    sidebar.search("focused window border");
    let first = sidebar
        .results()
        .row_at_index(0)
        .ok_or("  \"focused window border\" finds nothing")?;
    let texts: Vec<String> = all::<gtk::Label>(first.upcast_ref())
        .iter()
        .map(|label| label.label().to_string())
        .collect();
    ensure(
        texts == ["Title bar border", "Windows › Focused window"],
        &format!("  the first result reads {texts:?}"),
    )?;
    first.activate();
    ensure(
        window.page() == Some(Page::Windows),
        "  the result's page didn't open",
    )?;
    ensure(
        group(&window, "windows.focused")?.is_expanded(),
        "  the result didn't expand its group",
    )?;
    let row = row(&window, "windows.focused.border")?;
    ensure(
        row.parent()
            .is_some_and(|item| item.is_visible() && item.has_css_class(style::HIGHLIGHT)),
        "  the member's row isn't shown and tinted",
    )?;
    window.destroy();
    Ok(())
}

/// Searching shows results instead of the pages; a result opens its page and tints and focuses
/// its row. Esc goes from the page back to the result, then ends the search.
fn check_search() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let sidebar = window.sidebar();
    let results = sidebar.results();

    ensure(!sidebar.is_searching(), "  the sidebar starts searching")?;
    sidebar.search("DPI");
    ensure(sidebar.is_searching(), "  the results aren't shown")?;
    ensure(
        results.row_at_index(1).is_some(),
        "  \"DPI\" doesn't find the pointer speed of mice and touchpads",
    )?;

    ensure(
        sidebar.focus_selected(),
        "  the results can't take the focus",
    )?;
    let focused = results.focus_child().and_downcast::<gtk::ListBoxRow>();
    ensure(
        focused.as_ref().map(gtk::ListBoxRow::index) == Some(0),
        "  the first result doesn't get the focus",
    )?;
    results.emit_activate_cursor_row();
    ensure(
        window.page() == Some(Page::MouseTouchpad),
        "  the result's page didn't open",
    )?;
    ensure(
        sidebar.is_searching(),
        "  opening a result ended the search",
    )?;
    let key = "input.pointer.pointer_accel";
    let row = row(&window, key)?;
    ensure(
        row.parent()
            .is_some_and(|item| item.has_css_class(style::HIGHLIGHT)),
        "  the setting's row isn't tinted",
    )?;
    let focused = run_until(Duration::from_secs(5), || {
        gtk::prelude::GtkWindowExt::focus(&window).is_some_and(|focus| focus.is_ancestor(&row))
    });
    ensure(focused, "  the setting's control didn't get the focus")?;

    ensure(
        window.back_to_sidebar(),
        "  Esc didn't go back to the results",
    )?;
    let focus = gtk::prelude::GtkWindowExt::focus(&window);
    ensure(
        focus
            .and_downcast::<gtk::ListBoxRow>()
            .is_some_and(|row| row.index() == 0 && row.is_ancestor(results)),
        "  the focus isn't on the opened result",
    )?;
    ensure(sidebar.stop_search(), "  Esc didn't end the search")?;
    ensure(!sidebar.is_searching(), "  the pages aren't shown again")?;
    ensure(
        sidebar.search_entry().text().is_empty(),
        "  the search field isn't cleared",
    )?;
    ensure(
        gtk::prelude::GtkWindowExt::focus(&window)
            .is_some_and(|focus| focus.is_ancestor(sidebar.search_entry())),
        "  the search field doesn't have the focus",
    )?;
    ensure(
        !sidebar.stop_search(),
        "  Esc ended a search that wasn't there",
    )?;

    sidebar.search("no such setting anywhere");
    ensure(
        sidebar.is_searching() && results.row_at_index(0).is_none(),
        "  a query that matches nothing shows results",
    )?;
    window.destroy();
    Ok(())
}

/// A page without settings is found by its name, and Enter in the search field opens the best
/// result.
fn check_search_pages() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let sidebar = window.sidebar();
    sidebar.search("bluetooth");
    ensure(
        sidebar.results().row_at_index(0).is_some(),
        "  \"bluetooth\" finds nothing",
    )?;
    sidebar.search_entry().emit_activate();
    ensure(
        window.page() == Some(Page::Bluetooth),
        "  Enter didn't open the page",
    )?;
    window.destroy();
    Ok(())
}

/// The sidebar is as wide with long results, and with none, as with the pages, and doesn't ask
/// for more, which a wider window would give it. Large text makes the results longer than the
/// sidebar is wide.
fn check_search_width() -> Outcome {
    let settings = gtk::Settings::default().ok_or("  no GTK settings")?;
    settings.set_gtk_font_name(Some("Sans 14"));
    let outcome = search_widths();
    settings.reset_property("gtk-font-name");
    outcome
}

/// The problems of [`check_search_width`].
fn search_widths() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let sidebar = window.sidebar();
    let pages = sidebar.width();
    let mut problems = String::new();
    for query in ["", "trackpoint", "", "no such setting anywhere"] {
        sidebar.search(query);
        settle(&window)?;
        let width = sidebar.width();
        let (_, natural, _, _) = sidebar.measure(gtk::Orientation::Horizontal, -1);
        if width != pages || natural != pages {
            let _ = writeln!(
                problems,
                "    the sidebar is {width} px wide and asks for {natural} px after searching \
                 \"{query}\", {pages} px with the pages"
            );
        }
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The sidebar with search results, and with none, checked like the pages.
fn check_search_screens(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    let mut problems = String::new();
    for (query, name) in [("scroll", "search"), ("no such setting", "search-nothing")] {
        window.sidebar().search(query);
        let directory = screenshots.map(|dir| dir.join(name));
        if let Err(problem) = check_page(&window, Page::MouseTouchpad, directory.as_deref(), scheme)
        {
            problems.push_str(&problem);
        }
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// A link to a member of a collapsed group expands the group, then tints the member's row and
/// gives its control the focus.
fn check_open_group_link() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    window.show_page(Page::Windows);
    let group = group(&window, "windows.focused")?;
    ensure(!group.is_expanded(), "  the group starts expanded")?;

    let key = "windows.focused.indicator";
    let link = Link::parse(key, Registry::builtin()).map_err(|error| error.to_string())?;
    window.open_link(&link);
    ensure(group.is_expanded(), "  the link didn't expand the group")?;
    let row = row(&window, key)?;
    ensure(
        row.parent()
            .is_some_and(|item| item.is_visible() && item.has_css_class(style::HIGHLIGHT)),
        "  the member's row isn't shown and tinted",
    )?;
    let focused = run_until(Duration::from_secs(5), || {
        gtk::prelude::GtkWindowExt::focus(&window).is_some_and(|focus| focus.is_ancestor(&row))
    });
    ensure(focused, "  the member's control didn't get the focus")?;
    window.destroy();
    Ok(())
}

/// The checks of narrow mode.
fn check_narrow() -> Vec<(String, Outcome)> {
    vec![
        (
            "narrow: the real pages fit next to the sidebar at the threshold".to_owned(),
            check_narrow_threshold(),
        ),
        (
            "narrow: every page fits a third of a laptop screen".to_owned(),
            check_tile_width(),
        ),
        (
            "narrow: a narrow row puts its controls below its text".to_owned(),
            check_row_wrap(),
        ),
        (
            "narrow: one pane at a time, Esc and the back button".to_owned(),
            check_narrow_panes(),
        ),
        (
            "narrow: switching keeps the page and the focus".to_owned(),
            check_narrow_switch(),
        ),
        (
            "narrow: search and deep links".to_owned(),
            check_narrow_search(),
        ),
        (
            "narrow: the lines below the search field and the back bar".to_owned(),
            check_scroll_lines(),
        ),
    ]
}

/// The pages of v0.1, whose widths decide the threshold.
const REAL_PAGES: [Page; 4] = [
    Page::MouseTouchpad,
    Page::Keyboard,
    Page::Windows,
    Page::WindowBehavior,
];

/// Makes the panes of `window` narrow or wide, as resizing the window across the threshold
/// does, and waits for the new layout.
fn set_narrow(window: &Window, narrow: bool) -> Outcome {
    let panes = window.panes().ok_or("  the window has no panes")?;
    panes.set_threshold(if narrow { i32::MAX } else { 0 });
    let switched = run_until(Duration::from_secs(5), || {
        panes.is_narrow() == narrow && window.back_button().is_visible() == narrow
    });
    ensure(
        switched,
        if narrow {
            "  the window didn't get narrow"
        } else {
            "  the window didn't get wide"
        },
    )?;
    settle(window)
}

/// Which panes of `window` are shown (mapped): the sidebar, and the content pane.
fn shown_panes(window: &Window) -> (bool, bool) {
    let content = window.back_button().parent().and_then(|bar| bar.parent());
    (
        window.sidebar().is_mapped(),
        content.is_some_and(|content| content.is_mapped()),
    )
}

/// The widget with the keyboard focus in `window`.
fn focus(window: &Window) -> Option<gtk::Widget> {
    gtk::prelude::GtkWindowExt::focus(window)
}

/// A third of a laptop screen 1646 px wide (2880 px at 175 %), less the borders of a tiled window:
/// every page fits in it without scrolling sideways.
const TILE_WIDTH: i32 = 544;

/// Every real page, with its groups expanded, fits next to the sidebar in a window as wide as
/// the threshold with the controls of every row beside their text, and the window can get
/// narrower than that.
///
/// The width a page needs is measured, not tried: in a tiled window the compositor sets the size.
/// A row needs [`SettingRow::beside_width`], plus what the page puts around it (its padding, the
/// indent of a group's member), plus the page's margins.
fn check_narrow_threshold() -> Outcome {
    let fixture = fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let (_, sidebar, _, _) = window.sidebar().measure(gtk::Orientation::Horizontal, -1);
    let mut problems = String::new();
    for page in REAL_PAGES {
        window.show_page(page);
        for group in page_view(&window).iter().flat_map(PageView::groups) {
            group.set_expanded(true);
        }
        settle(&window)?;
        let view = page_view(&window).ok_or("  no page view")?;
        // The rows of a page shown just now get their widths with the next layout.
        run_until(Duration::from_secs(5), || {
            rows(view.upcast_ref())
                .iter()
                .any(|row| row.is_mapped() && row.width() > 0)
        });
        let content = view.content();
        let Some((needed, row)) = rows(view.upcast_ref())
            .into_iter()
            .filter(|row| row.is_mapped() && row.width() > 0)
            .map(|row| {
                let around = content.width() - row.width();
                (row.beside_width() + around, row)
            })
            .max_by_key(|(needed, _)| *needed)
        else {
            let _ = writeln!(problems, "    {} shows no rows", page.id());
            continue;
        };
        let width = view.width_for(needed);
        // The separator between the panes is 1 px wide.
        if sidebar + 1 + width > NARROW_WIDTH {
            let _ = writeln!(
                problems,
                "    {} needs {width} px next to the {sidebar} px sidebar for the controls of {} \
                 beside its text, more than the {NARROW_WIDTH} px of the threshold",
                page.id(),
                row.setting().map_or("a row", |setting| setting.key())
            );
        }
    }
    let panes = window.panes().ok_or("  the window has no panes")?;
    let (minimum, _, _, _) = panes.measure(gtk::Orientation::Horizontal, -1);
    if minimum >= NARROW_WIDTH {
        let _ = writeln!(
            problems,
            "    the window can't get narrower than {minimum} px, not below the threshold"
        );
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Every page, with its groups expanded, fits in a window a third of a laptop screen wide
/// without scrolling sideways; narrower still, it scrolls sideways instead of being cut off.
fn check_tile_width() -> Outcome {
    let fixture = fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let mut problems = String::new();
    for page in Page::ALL {
        window.show_page(page);
        let view = page_view(&window).ok_or("  no page view")?;
        for group in view.groups() {
            group.set_expanded(true);
        }
        settle(&window)?;
        if !view.fits(TILE_WIDTH) {
            let (content, _, _, _) = view.content().measure(gtk::Orientation::Horizontal, -1);
            let _ = writeln!(
                problems,
                "    {} needs {} px, more than {TILE_WIDTH} px",
                page.id(),
                view.width_for(content)
            );
        }
        let scroller = find::<gtk::ScrolledWindow>(view.upcast_ref()).ok_or("  no scroller")?;
        if scroller.hscrollbar_policy() != gtk::PolicyType::Automatic {
            let _ = writeln!(problems, "    {} can't scroll sideways", page.id());
        }
    }
    let (minimum, _, _, _) = window
        .panes()
        .ok_or("  the window has no panes")?
        .measure(gtk::Orientation::Horizontal, -1);
    if minimum > TILE_WIDTH / 2 {
        let _ = writeln!(
            problems,
            "    the window can't get narrower than {minimum} px, so a narrower tile cuts it off"
        );
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// A slider row has its controls beside its text down to its [`SettingRow::beside_width`], with
/// the slider shrinking first; narrower, the controls are below the text, so the row gets
/// higher, and it can get narrower than that.
fn check_row_wrap() -> Outcome {
    let fixture = fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    window.show_page(Page::MouseTouchpad);
    settle(&window)?;
    let view = page_view(&window).ok_or("  no page view")?;
    let row = rows(view.upcast_ref())
        .into_iter()
        .find(|row| row.controls().iter().any(ObjectExt::is::<gtk::Scale>))
        .ok_or("  no slider row")?;
    let beside = row.beside_width();
    let (minimum, natural, _, _) = row.measure(gtk::Orientation::Horizontal, -1);
    let height = |width: i32| row.measure(gtk::Orientation::Vertical, width).0;
    let mut problems = String::new();
    if natural <= beside {
        let _ = writeln!(
            problems,
            "    the slider doesn't shrink before the controls move: natural {natural} px, beside \
             down to {beside} px"
        );
    }
    if height(beside - 1) <= height(beside) {
        let _ = writeln!(
            problems,
            "    at {} px the row is {} px high, not higher than {} px at {beside} px: the \
             controls didn't move below the text",
            beside - 1,
            height(beside - 1),
            height(beside)
        );
    }
    if minimum >= beside {
        let _ = writeln!(
            problems,
            "    the row can't get narrower than {minimum} px, the width it needs beside"
        );
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Narrow, the window shows the sidebar, then the page with the back button; Esc and the back
/// button go back to the sidebar, at the open page.
fn check_narrow_panes() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, true) && !window.back_button().is_visible(),
        "  the window doesn't start with both panes and no back button",
    )?;
    let sidebar = window.sidebar();
    ensure(
        sidebar.focus_selected(),
        "  the sidebar can't take the focus",
    )?;
    set_narrow(&window, true)?;
    ensure(
        shown_panes(&window) == (true, false),
        "  with the focus in the sidebar, narrow doesn't show only the sidebar",
    )?;

    let list = sidebar.list();
    list.emit_move_cursor(gtk::MovementStep::DisplayLines, 1, false, false);
    list.emit_activate_cursor_row();
    settle(&window)?;
    ensure(
        window.page() == Page::ALL.get(1).copied(),
        "  Enter doesn't open the page",
    )?;
    ensure(
        shown_panes(&window) == (false, true),
        "  the page doesn't replace the sidebar",
    )?;
    let content = window.content().ok_or("  no content pane")?;
    let back = window.back_button();
    ensure(
        focus(&window).is_some_and(|focus| {
            focus.is_ancestor(&content) || focus == *back.upcast_ref::<gtk::Widget>()
        }),
        "  the focus didn't move to the page",
    )?;

    ensure(window.back_to_sidebar(), "  Esc didn't go back")?;
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, false),
        "  Esc doesn't show the sidebar instead of the page",
    )?;
    let focused = focus(&window)
        .and_downcast::<gtk::ListBoxRow>()
        .map(|row| row.index());
    ensure(
        focused == Some(1),
        "  the focus isn't on the open page's row",
    )?;
    ensure(
        !window.back_to_sidebar(),
        "  Esc went back from the sidebar",
    )?;

    window.show_page(Page::Keyboard);
    settle(&window)?;
    ensure(
        shown_panes(&window) == (false, true),
        "  opening a page doesn't show it",
    )?;
    back.emit_clicked();
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, false) && sidebar.selected() == Some(Page::Keyboard),
        "  the back button doesn't show the sidebar at the open page",
    )?;

    set_narrow(&window, false)?;
    ensure(
        shown_panes(&window) == (true, true) && window.page() == Some(Page::Keyboard),
        "  wide again, the window doesn't show both panes with the open page",
    )?;
    window.destroy();
    Ok(())
}

/// A window of `fixture` with `page` open, shown and laid out.
fn window_at(fixture: &Fixture, page: Page) -> Result<Window, String> {
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    window.show_page(page);
    settle(&window)?;
    Ok(window)
}

/// Switching between narrow and wide keeps the open page, the pane with the focus and the focus.
fn check_narrow_switch() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = window_at(&fixture, Page::Keyboard)?;
    let row = row(&window, "input.keyboard.repeat_rate")?;
    ensure(
        row.child_focus(gtk::DirectionType::TabForward),
        "  the row's control can't take the focus",
    )?;
    let focused = focus(&window).ok_or("  nothing has the focus")?;

    set_narrow(&window, true)?;
    ensure(
        shown_panes(&window) == (false, true) && window.page() == Some(Page::Keyboard),
        "  with the focus on the page, narrow doesn't show the page",
    )?;
    ensure(
        focus(&window).as_ref() == Some(&focused),
        "  getting narrow moved the focus",
    )?;
    set_narrow(&window, false)?;
    ensure(
        shown_panes(&window) == (true, true) && window.page() == Some(Page::Keyboard),
        "  wide again, the window doesn't show both panes with the open page",
    )?;
    ensure(
        focus(&window).as_ref() == Some(&focused),
        "  getting wide moved the focus",
    )?;
    window.destroy();
    Ok(())
}

/// Narrow, a search typed on the page shows the sidebar with the results, a result opens its
/// page and points to its row, Esc goes back to the result, the search field shows the sidebar,
/// and a deep link opens its page from the sidebar.
fn check_narrow_search() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = window_at(&fixture, Page::Keyboard)?;
    set_narrow(&window, true)?;
    window.show_page(Page::Keyboard);
    settle(&window)?;
    ensure(
        shown_panes(&window) == (false, true),
        "  opening a page doesn't show it",
    )?;
    let sidebar = window.sidebar();
    sidebar.search("DPI");
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, false) && sidebar.is_searching(),
        "  a search doesn't show the sidebar with the results",
    )?;
    ensure(
        focus(&window).is_some_and(|focus| focus.is_ancestor(sidebar.search_entry())),
        "  the search field doesn't have the focus",
    )?;
    let results = sidebar.results();
    ensure(
        sidebar.focus_selected(),
        "  the results can't take the focus",
    )?;
    results.emit_activate_cursor_row();
    settle(&window)?;
    ensure(
        window.page() == Some(Page::MouseTouchpad) && shown_panes(&window) == (false, true),
        "  the result doesn't show its page",
    )?;
    let accel = row(&window, "input.pointer.pointer_accel")?;
    let pointed = run_until(Duration::from_secs(5), || {
        focus(&window).is_some_and(|focus| focus.is_ancestor(&accel))
    });
    ensure(pointed, "  the setting's control didn't get the focus")?;
    ensure(window.back_to_sidebar(), "  Esc didn't go back")?;
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, false)
            && focus(&window)
                .and_downcast::<gtk::ListBoxRow>()
                .is_some_and(|row| row.index() == 0 && row.is_ancestor(results)),
        "  Esc doesn't show the results with the focus on the opened one",
    )?;
    ensure(sidebar.stop_search(), "  Esc didn't end the search")?;

    // Ctrl+F on the page.
    window.show_page(Page::Keyboard);
    settle(&window)?;
    ensure(
        sidebar.focus_search(),
        "  the search field can't take the focus",
    )?;
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, false),
        "  the search field with the focus doesn't show the sidebar",
    )?;

    let key = "windows.focused.indicator";
    let link = Link::parse(key, Registry::builtin()).map_err(|error| error.to_string())?;
    window.open_link(&link);
    settle(&window)?;
    ensure(
        window.page() == Some(Page::Windows) && shown_panes(&window) == (false, true),
        "  a deep link doesn't show its page",
    )?;
    let row = row(&window, key)?;
    let pointed = run_until(Duration::from_secs(5), || {
        focus(&window).is_some_and(|focus| focus.is_ancestor(&row))
    });
    ensure(pointed, "  the deep link's row didn't get the focus")?;
    window.destroy();
    Ok(())
}

/// The page view open in `window`.
fn page_view(window: &Window) -> Option<PageView> {
    window
        .content()
        .and_then(|content| find::<PageView>(&content))
}

/// The row of the group `id` on the open page of `window`.
fn group(window: &Window, id: &str) -> Result<GroupRow, String> {
    page_view(window)
        .and_then(|view| view.group(id))
        .ok_or_else(|| format!("  no group {id}"))
}

/// The five colors of `client.focused` are rows below one group row: collapsed, it shows a
/// swatch per color and, while the preference is on, the line once; the members never show it.
fn check_group_row() -> Outcome {
    let fixture = fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true)?;
    let window = open(&fixture, Page::Windows)?;
    let group = group(&window, "windows.focused")?;
    let keys: Vec<&str> = group
        .members()
        .iter()
        .filter_map(|member| {
            member
                .setting()
                .map(sway_things_core::registry::Setting::key)
        })
        .collect();
    ensure(
        keys == [
            "windows.focused.border",
            "windows.focused.background",
            "windows.focused.text",
            "windows.focused.indicator",
            "windows.focused.child_border",
        ],
        &format!("  the group has the members {keys:?}"),
    )?;
    ensure(!group.is_expanded(), "the group starts expanded")?;
    ensure(
        group
            .members()
            .iter()
            .all(|member| member.parent().is_some_and(|item| !item.is_visible())),
        "a collapsed group shows its members",
    )?;
    let expected = fixture
        .model
        .view(setting("windows.focused.border")?, None)
        .and_then(|view| view.line)
        .ok_or_else(|| "  client.focused has no line".to_owned())?;
    ensure(
        group.line().as_deref() == Some(expected.as_str()),
        &format!(
            "  the group shows {:?}, expected {expected:?}",
            group.line()
        ),
    )?;
    ensure(
        group.members().iter().all(|member| member.line().is_none()),
        "a member repeats the line",
    )?;
    let swatches: Vec<String> = group
        .swatches()
        .iter()
        .map(gtk::gdk::RGBA::to_str)
        .map(|color| color.to_string())
        .collect();
    ensure(
        swatches.len() == 5 && swatches.last().map(String::as_str) == Some("rgb(59,110,79)"),
        &format!("  the swatches are {swatches:?}, the last one the stored #3b6e4f"),
    )?;

    // A change shows in the summary and the line.
    fixture.model.set(
        "windows.focused.border",
        None,
        Value::Color("#102030".parse().map_err(|_| "  not a color".to_owned())?),
    );
    wait_for_model(&fixture.model)?;
    ensure(
        group
            .swatches()
            .first()
            .map(|color| color.to_str().to_string())
            == Some("rgb(16,32,48)".to_owned()),
        "  the swatch didn't follow the change",
    )?;
    ensure(
        group
            .line()
            .is_some_and(|line| line.starts_with("client.focused #102030 ")),
        &format!("  the line didn't follow the change: {:?}", group.line()),
    )?;
    fixture
        .model
        .set("app.show_directives", None, Value::Bool(false));
    wait_for_model(&fixture.model)?;
    ensure(
        group.line().is_none(),
        "the group keeps its line after the preference went off",
    )?;
    window.destroy();
    Ok(())
}

/// The group row takes the focus, and `Enter` and `Space` expand and collapse it; its accessible
/// state follows.
fn check_group_keyboard() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::Windows)?;
    let group = group(&window, "windows.focused")?;
    let item = group.item();
    ensure(
        item.is_focusable() && item.grab_focus(),
        "the group row can't take the focus",
    )?;
    let keys = item
        .observe_controllers()
        .into_iter()
        .filter_map(Result::ok)
        .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
        .ok_or_else(|| "  the group row doesn't listen to keys".to_owned())?;
    let press = |key: gtk::gdk::Key| -> bool {
        use gtk::glib::translate::IntoGlib as _;
        keys.emit_by_name::<bool>(
            "key-pressed",
            &[&key.into_glib(), &0u32, &gtk::gdk::ModifierType::empty()],
        )
    };
    let members_shown = || {
        group
            .members()
            .iter()
            .all(|member| member.parent().is_some_and(|item| item.is_visible()))
    };
    ensure(
        press(gtk::gdk::Key::Return) && group.is_expanded() && members_shown(),
        "Enter doesn't expand the group",
    )?;
    ensure(
        press(gtk::gdk::Key::space) && !group.is_expanded() && !members_shown(),
        "Space doesn't collapse the group",
    )?;
    ensure(
        press(gtk::gdk::Key::KP_Enter) && group.is_expanded(),
        "Enter on the keypad doesn't expand the group",
    )?;
    ensure(
        !press(gtk::gdk::Key::a) && group.is_expanded(),
        "another key toggles the group",
    )?;
    window.destroy();
    Ok(())
}

/// Every widget of type `T` in `widget` and below it.
fn all<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Vec<T> {
    let mut found = Vec::new();
    if let Ok(widget) = widget.clone().downcast::<T>() {
        found.push(widget);
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        found.extend(all::<T>(&widget));
        child = widget.next_sibling();
    }
    found
}

/// Every setting row below `widget`.
fn rows(widget: &gtk::Widget) -> Vec<SettingRow> {
    let mut found = Vec::new();
    if let Some(row) = widget.downcast_ref::<SettingRow>() {
        found.push(row.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        found.extend(rows(&widget));
        child = widget.next_sibling();
    }
    found
}

/// The row of `key` on the open page of `window`.
fn row(window: &Window, key: &str) -> Result<SettingRow, String> {
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    rows(&content)
        .into_iter()
        .find(|row| row.setting().is_some_and(|setting| setting.key() == key))
        .ok_or_else(|| format!("  no row for {key}"))
}

/// A window for `fixture` with `page` open.
fn open(fixture: &Fixture, page: Page) -> Result<Window, String> {
    let window = Window::with_model(&fixture.model);
    window.present();
    window.show_page(page);
    settle(&window)?;
    Ok(window)
}

/// Waits until the model has sent every change.
fn wait_for_model(model: &Model) -> Outcome {
    if run_until(Duration::from_secs(10), || !model.is_busy()) {
        Ok(())
    } else {
        Err("  the change didn't finish within 10 s".to_owned())
    }
}

/// Every page with settings shows a row for each of them, in the order of the registry.
fn check_rows() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    for page in Page::ALL {
        let window = open(&fixture, page)?;
        let expected: Vec<&str> = Registry::builtin()
            .iter()
            .filter(|setting| setting.page() == page)
            .map(sway_things_core::registry::Setting::key)
            .collect();
        let content = window
            .content()
            .ok_or_else(|| "  no content pane".to_owned())?;
        let shown: Vec<&str> = rows(&content)
            .iter()
            .filter_map(|row| row.setting().map(sway_things_core::registry::Setting::key))
            .collect();
        ensure(
            shown == expected,
            &format!("{}: rows {shown:?}, expected {expected:?}", page.id()),
        )?;
        let unfocusable: Vec<String> = rows(&content)
            .iter()
            .flat_map(SettingRow::controls)
            .filter(|control| {
                !control.is_focusable()
                    && control
                        .first_child()
                        .is_none_or(|child| !child.is_focusable())
            })
            .map(|control| control.type_().name().to_owned())
            .collect();
        ensure(
            unfocusable.is_empty(),
            &format!("{}: controls without focus: {unfocusable:?}", page.id()),
        )?;
        window.destroy();
    }
    Ok(())
}

/// Pointer speed (default 0, slider -1 to 1, stored 0.3): a drag near 0 lands exactly on 0, one
/// outside the zone and a key step land where they are.
fn check_slider_snap() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let speed = row(&window, "input.touchpad.pointer_accel")?;
    let scale = speed
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Scale>().ok())
        .ok_or_else(|| "  the pointer speed has no slider".to_owned())?;
    let stored = |fixture: &Fixture| -> Result<Value, String> {
        let setting = Registry::builtin()
            .get("input.touchpad.pointer_accel")
            .ok_or_else(|| "  no pointer speed setting".to_owned())?;
        fixture
            .model
            .view(setting, None)
            .map(|view| view.value)
            .ok_or_else(|| "  no settings".to_owned())
    };
    // How GTK reports a pointer drag, and an arrow key.
    let mv = |scroll: gtk::ScrollType, value: f64| {
        scale.emit_by_name::<bool>("change-value", &[&scroll, &value]);
    };
    for (scroll, to, expected) in [
        (gtk::ScrollType::Jump, 0.03, 0.0),
        (gtk::ScrollType::Jump, -0.04, 0.0),
        (gtk::ScrollType::Jump, 0.05, 0.05),
        (gtk::ScrollType::StepLeft, 0.03, 0.03),
    ] {
        mv(scroll, to);
        wait_for_model(&fixture.model)?;
        ensure(
            (scale.value() - expected).abs() < 1e-9,
            &format!("  {scroll:?} to {to} left the slider at {}", scale.value()),
        )?;
        ensure(
            stored(&fixture)? == Value::Decimal(expected),
            &format!("  {scroll:?} to {to} stored {:?}", stored(&fixture)?),
        )?;
    }
    window.destroy();
    Ok(())
}

/// The slider of a setting without a stored value stays where it is once a change stores one
/// and the reset and machine buttons appear, so a drag doesn't jump.
fn check_control_stays() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let key = "input.touchpad.scroll_factor";
    let speed = row(&window, key)?;
    let scale = speed
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Scale>().ok())
        .ok_or_else(|| "  the scroll speed has no slider".to_owned())?;
    let bounds = |scale: &gtk::Scale| {
        scale
            .compute_bounds(&window)
            .map(|rect| (rect.x(), rect.width()))
    };
    ensure(
        speed.reset_button().is_none(),
        "scroll speed is stored already",
    )?;
    let before = bounds(&scale);
    fixture.model.set(key, None, Value::Decimal(1.5));
    wait_for_model(&fixture.model)?;
    settle(&window)?;
    ensure(
        speed.reset_button().is_some(),
        "the reset button didn't appear",
    )?;
    let after = bounds(&scale);
    ensure(
        before.is_some() && before == after,
        &format!("  the slider moved from {before:?} to {after:?}"),
    )?;
    window.destroy();
    Ok(())
}

/// Turning on the tap switch stores the value.
fn check_change() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let tap = row(&window, "input.touchpad.tap")?;
    let switch = tap
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Switch>().ok())
        .ok_or_else(|| "  the tap row has no switch".to_owned())?;
    ensure(!switch.is_active(), "tap is on before the change")?;
    switch.set_active(true);
    wait_for_model(&fixture.model)?;
    let setting = Registry::builtin()
        .get("input.touchpad.tap")
        .ok_or_else(|| "  no tap setting".to_owned())?;
    let view = fixture
        .model
        .view(setting, None)
        .ok_or_else(|| "  no settings".to_owned())?;
    ensure(
        view.value == Value::Bool(true),
        "the change didn't reach the settings",
    )?;
    ensure(
        view.line.as_deref() == Some("tap enabled"),
        "the row's line didn't follow",
    )?;
    ensure(tap.error().is_none(), "the change shows an error")?;
    window.destroy();
    Ok(())
}

/// The line of the sway config shows below the subtitle only while the preference is on, in
/// every row, and follows the preference at once.
fn check_directives() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let accel = row(&window, "input.touchpad.pointer_accel")?;
    let expected = fixture
        .model
        .view(setting("input.touchpad.pointer_accel")?, None)
        .and_then(|view| view.line)
        .ok_or_else(|| "  the pointer acceleration has no line".to_owned())?;
    ensure(
        accel.line().is_none(),
        "a row shows its line with the preference off",
    )?;
    fixture
        .model
        .set("app.show_directives", None, Value::Bool(true));
    wait_for_model(&fixture.model)?;
    ensure(
        accel.line().as_deref() == Some(expected.as_str()),
        &format!("  the row shows {:?}, expected {expected:?}", accel.line()),
    )?;
    window.show_page(Page::Windows);
    settle(&window)?;
    ensure(
        row(&window, "windows.gaps.inner")?.line().is_some(),
        "a row on another page doesn't show its line",
    )?;
    fixture
        .model
        .set("app.show_directives", None, Value::Bool(false));
    wait_for_model(&fixture.model)?;
    ensure(
        row(&window, "windows.gaps.inner")?.line().is_none(),
        "a row keeps its line after the preference went off",
    )?;
    window.destroy();
    Ok(())
}

/// On vanilla sway, the SwayFX corner radius can't be changed, and says why. A setting that
/// applies after a reload says so.
fn check_locked() -> Outcome {
    let fixture = fixture(SWAY, STORED, USERS, true)?;
    let window = open(&fixture, Page::Windows)?;
    let radius = row(&window, "effects.corner_radius")?;
    for control in radius.controls() {
        ensure(
            !control.is_sensitive(),
            "a control of the corner radius can be used",
        )?;
        ensure(
            control.tooltip_text().as_deref() == Some("Requires SwayFX 0.5"),
            "the corner radius doesn't say why it is locked",
        )?;
    }
    ensure(
        radius.badges() == ["Requires SwayFX 0.5"],
        &format!("  the corner radius has the badges {:?}", radius.badges()),
    )?;
    let gaps = row(&window, "windows.gaps.inner")?;
    ensure(
        gaps.controls()
            .iter()
            .all(gtk::prelude::WidgetExt::is_sensitive),
        "the gaps are locked",
    )?;
    window.destroy();
    let window = open(&fixture, Page::Keyboard)?;
    let numlock = row(&window, "input.keyboard.xkb_numlock")?;
    ensure(
        numlock.badges() == ["Applies after reload"],
        &format!(
            "  Num Lock at startup has the badges {:?}",
            numlock.badges()
        ),
    )?;
    window.destroy();
    Ok(())
}

/// Blur passes can't be changed while blur is off, and say so; turning blur on unlocks them.
fn check_dependency() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::Windows)?;
    let passes = row(&window, "effects.blur_passes")?;
    ensure(
        passes
            .controls()
            .iter()
            .all(|control| !control.is_sensitive()),
        "blur passes can be changed with blur off",
    )?;
    ensure(
        passes.note().as_deref() == Some("Turn on Blur first"),
        &format!("  blur passes say {:?}", passes.note()),
    )?;
    let (_, blur) = switch(&window, "effects.blur")?;
    blur.set_active(true);
    wait_for_model(&fixture.model)?;
    ensure(
        passes
            .controls()
            .iter()
            .all(gtk::prelude::WidgetExt::is_sensitive),
        "blur passes stay locked with blur on",
    )?;
    ensure(passes.note().is_none(), "blur passes still say why")?;
    window.destroy();
    Ok(())
}

/// Without the include line nothing can be saved: a change fails and the row says why.
fn check_failed() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, false)?;
    let window = open(&fixture, Page::Windows)?;
    let gaps = row(&window, "windows.gaps.inner")?;
    ensure(
        gaps.controls()
            .iter()
            .all(|control| !control.is_sensitive()),
        "the gaps can be changed without the include line",
    )?;
    // The model refuses it too, should a change get through.
    fixture
        .model
        .set("windows.gaps.inner", None, Value::Integer(9));
    wait_for_model(&fixture.model)?;
    ensure(gaps.error().is_some(), "the refused change shows no error")?;
    window.destroy();
    Ok(())
}

const POINTER_EVENTS: &str = "input.pointer.events";

/// The switch of the row of `key` on the open page of `window`.
fn switch(window: &Window, key: &str) -> Result<(SettingRow, gtk::Switch), String> {
    let row = row(window, key)?;
    let switch = row
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Switch>().ok())
        .ok_or_else(|| format!("  the row of {key} has no switch"))?;
    Ok((row, switch))
}

/// A window on the Mouse & Touchpad page in the session of `sway`, after pointers were switched
/// off with the safety net: the banner is up.
fn tried(sway: &FakeSway, stored: &str) -> Result<(Fixture, Window, gtk::Switch), String> {
    let fixture = live_fixture(sway, stored)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let (row, switch) = switch(&window, POINTER_EVENTS)?;
    ensure(switch.is_active(), "pointers are off before the change")?;
    switch.set_active(false);
    wait_for_model(&fixture.model)?;
    ensure(row.error().is_none(), "the tried change shows an error")?;
    let banner = window.safety_banner().unwrap_or_default();
    ensure(
        banner.starts_with("Keep the change to Mouse › Enabled? Reverting in "),
        &format!("the banner says {banner:?}"),
    )?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command.ends_with("events disabled")),
        "the tried value wasn't sent",
    )?;
    Ok((fixture, window, switch))
}

/// The value of `key` the model shows, and whether it is stored.
fn shown(model: &Model, key: &str) -> Result<(Value, bool), String> {
    let setting = Registry::builtin()
        .get(key)
        .ok_or_else(|| format!("  no setting {key}"))?;
    let view = model
        .view(setting, None)
        .ok_or_else(|| "  no settings".to_owned())?;
    Ok((view.value, matches!(view.source, Source::Stored { .. })))
}

/// A risky change is tried, shown with the banner (Revert has the focus), and stored on Keep.
fn check_keep() -> Outcome {
    let sway = laptop();
    let (fixture, window, switch) = tried(&sway, STORED)?;
    ensure(
        GtkWindowExt::focus(&window).as_ref() == Some(window.revert_button().upcast_ref()),
        "Revert doesn't have the focus",
    )?;
    ensure(!switch.is_active(), "the row doesn't show the tried value")?;
    ensure(
        shown(&fixture.model, POINTER_EVENTS)? == (Value::Bool(false), false),
        "the tried value isn't shown or is already stored",
    )?;
    window.keep_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        window.safety_banner().is_none(),
        "the banner stays after Keep",
    )?;
    ensure(
        shown(&fixture.model, POINTER_EVENTS)? == (Value::Bool(false), true),
        "the kept value isn't stored",
    )?;
    ensure(!switch.is_active(), "the row lost the kept value")?;
    window.destroy();
    Ok(())
}

/// Revert puts the old value back, live and in the row, and stores nothing.
fn check_revert() -> Outcome {
    let sway = laptop();
    let (fixture, window, switch) = tried(&sway, STORED)?;
    window.revert_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        window.safety_banner().is_none(),
        "the banner stays after Revert",
    )?;
    ensure(switch.is_active(), "the row doesn't show the old value")?;
    ensure(
        shown(&fixture.model, POINTER_EVENTS)? == (Value::Bool(true), false),
        "the reverted value is stored",
    )?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command.ends_with("events enabled")),
        "the old value wasn't sent",
    )?;
    window.destroy();
    Ok(())
}

/// Without an answer, the change is reverted when the countdown ends.
fn check_countdown() -> Outcome {
    let sway = laptop();
    let stored = "version = 1\n\n[app]\nsafety_net_timeout = 5\n";
    let (fixture, window, switch) = tried(&sway, stored)?;
    let reverted = run_until(Duration::from_secs(8), || {
        window.safety_banner().is_none() && !fixture.model.is_busy()
    });
    ensure(reverted, "the change wasn't reverted within 8 s")?;
    ensure(switch.is_active(), "the row doesn't show the old value")?;
    ensure(
        shown(&fixture.model, POINTER_EVENTS)? == (Value::Bool(true), false),
        "the reverted value is stored",
    )?;
    window.destroy();
    Ok(())
}

/// Switching off the only pointer is refused at the row, and nothing is tried.
fn check_lockout() -> Outcome {
    let sway = obedient_with_inputs(SWAY_1_10, format!("[{KEYBOARD}, {MOUSE}]"));
    let fixture = live_fixture(&sway, STORED)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let (row, switch) = switch(&window, POINTER_EVENTS)?;
    switch.set_active(false);
    wait_for_model(&fixture.model)?;
    ensure(
        window.safety_banner().is_none(),
        "the refused change shows the banner",
    )?;
    let error = row.error().unwrap_or_default();
    ensure(
        error.contains("wasn't changed"),
        &format!("the row says {error:?}"),
    )?;
    ensure(switch.is_active(), "the row doesn't show the old value")?;
    ensure(
        sway.commands().is_empty(),
        "a command was sent for the refused change",
    )?;
    window.destroy();
    Ok(())
}

/// Closing the window while a change waits reverts it first.
fn check_close_reverts() -> Outcome {
    let sway = laptop();
    let (fixture, window, _) = tried(&sway, STORED)?;
    window.close();
    let closed = run_until(Duration::from_secs(5), || !window.is_visible());
    ensure(closed, "the window didn't close within 5 s")?;
    ensure(
        fixture.model.pending_change().is_none(),
        "the change still waits",
    )?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command.ends_with("events enabled")),
        "the old value wasn't sent before closing",
    )?;
    Ok(())
}

/// The page with the safety-net banner, checked like the pages.
fn check_safety_net_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let sway = laptop();
    let (_fixture, window, _) = tried(&sway, STORED)?;
    let directory = screenshots.map(|dir| dir.join("safety-net"));
    let outcome = check_page(&window, Page::MouseTouchpad, directory.as_deref(), scheme);
    window.destroy();
    outcome
}

/// The welcome screen and a page with the read-only banner, checked like the pages.
fn check_read_only_screens(screenshots: Option<&Path>, scheme: &str) -> (Outcome, Outcome) {
    let fixture = match fixture(SWAYFX, STORED, USERS, false) {
        Ok(fixture) => fixture,
        Err(error) => return (Err(error.clone()), Err(error)),
    };
    let window = Window::with_model(&fixture.model);
    window.present();
    let welcome = settle(&window)
        .and_then(|()| wait_for_paint(&window))
        .and_then(|()| {
            let path = screenshots.map(|dir| dir.join(format!("welcome--{scheme}.png")));
            let problems = check_screen(&window, path)?;
            ensure(problems.is_empty(), problems.trim_start())
        });
    window.look_around();
    let directory = screenshots.map(|dir| dir.join("read-only"));
    let banner = check_page(&window, Page::MouseTouchpad, directory.as_deref(), scheme);
    window.destroy();
    (welcome, banner)
}

/// Without the include line the window opens on the welcome screen; "Look around first" and
/// opening a page lead to the pages with the banner, whose button leads back.
fn check_welcome() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, false)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(window.is_welcome_shown(), "the welcome screen isn't shown")?;
    ensure(window.welcome().is_waiting(), "the spinner doesn't spin")?;
    ensure(
        window.welcome().line().starts_with("include ")
            && window.welcome().line().ends_with("/*.conf"),
        &format!("the welcome screen asks for {:?}", window.welcome().line()),
    )?;
    ensure(
        window.sidebar().selected().is_none(),
        "a page is marked in the sidebar",
    )?;
    ensure(
        window.status_banner().is_none(),
        "the banner shows with the welcome screen",
    )?;
    let [_, _, look_around] = window.welcome().buttons();
    ensure(
        look_around.is_focusable(),
        "\"Look around first\" can't take the focus",
    )?;
    look_around.emit_clicked();
    settle(&window)?;
    ensure(
        !window.is_welcome_shown(),
        "\"Look around first\" didn't leave the welcome screen",
    )?;
    ensure(
        !window.welcome().is_waiting(),
        "the spinner spins while hidden",
    )?;
    ensure(
        window.page() == Page::ALL.first().copied(),
        "no page is open",
    )?;
    ensure(
        window
            .status_banner()
            .is_some_and(|text| text.starts_with("Not enabled yet")),
        "the read-only banner isn't shown",
    )?;
    window.status_button().emit_clicked();
    settle(&window)?;
    ensure(
        window.is_welcome_shown(),
        "\"Show instructions\" didn't bring back the welcome screen",
    )?;
    ensure(
        window.welcome().is_waiting(),
        "the spinner doesn't spin after coming back",
    )?;
    // Picking a page from the sidebar counts as looking around.
    window.show_page(Page::Keyboard);
    settle(&window)?;
    ensure(
        !window.is_welcome_shown() && window.page() == Some(Page::Keyboard),
        "opening a page didn't leave the welcome screen",
    )?;
    ensure(
        window.status_banner().is_some(),
        "the banner is gone after opening a page",
    )?;
    // A change from the model, like any row's, doesn't bring the welcome screen back.
    fixture.model.set_status(fixture.status(USERS, false));
    settle(&window)?;
    ensure(
        !window.is_welcome_shown(),
        "the welcome screen came back by itself",
    )?;
    window.destroy();
    Ok(())
}

/// Once the user adds the line, the welcome screen and the banner go away by themselves.
fn check_welcome_closes() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, false)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(window.is_welcome_shown(), "the welcome screen isn't shown")?;
    fixture.model.set_status(fixture.status(USERS, true));
    settle(&window)?;
    ensure(
        !window.is_welcome_shown(),
        "the welcome screen stayed after the line was added",
    )?;
    ensure(
        window.page() == Page::ALL.first().copied(),
        "no page is open",
    )?;
    ensure(
        window.status_banner().is_none(),
        "a banner stayed after the line was added",
    )?;
    let tap = row(&window, "input.touchpad.tap")?;
    ensure(
        tap.controls()
            .iter()
            .all(gtk::prelude::WidgetExt::is_sensitive),
        "the rows stayed locked after the line was added",
    )?;
    window.destroy();
    Ok(())
}

/// With lines of the user's after the include, a banner explains that they win, once.
fn check_not_last() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let after = fixture.status_of(&format!("{}gaps inner 3\n", fixture.include_line()));
    fixture.model.set_status(after);
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(
        !window.is_welcome_shown(),
        "the welcome screen shows with the line there",
    )?;
    let text = window
        .status_banner()
        .ok_or_else(|| "  no banner for the lines after the include".to_owned())?;
    ensure(
        text.contains("1 of your config lines comes after them"),
        &format!("the banner says {text:?}"),
    )?;
    window.status_button().emit_clicked();
    settle(&window)?;
    ensure(
        window.status_banner().is_none(),
        "the banner can't be dismissed",
    )?;
    window.destroy();
    Ok(())
}

/// Where the insensitive widgets below `widget` are, in the coordinates of `window`: the topmost
/// of each insensitive part of the tree.
fn insensitive_areas(widget: &gtk::Widget, window: &gtk::Widget) -> Vec<gtk::graphene::Rect> {
    if !widget.is_sensitive() {
        return widget.compute_bounds(window).into_iter().collect();
    }
    let mut areas = Vec::new();
    let mut child = widget.first_child();
    while let Some(next) = child {
        if next.is_visible() {
            areas.extend(insensitive_areas(&next, window));
        }
        child = next.next_sibling();
    }
    areas
}

/// The pixels of `widget` as drawn in `window` now.
fn pixels_of(window: &Window, widget: &gtk::Widget) -> Result<Vec<u8>, String> {
    wait_for_paint(window)?;
    let node = render_drawn(window)?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "window sizes are far below 2^24, where f32 starts to round"
    )]
    let (width, height) = (window.width() as f32, window.height() as f32);
    let texture = window
        .renderer()
        .ok_or_else(|| "  the window has no renderer".to_owned())?
        .render_texture(
            &node,
            Some(&gtk::graphene::Rect::new(0.0, 0.0, width, height)),
        );
    let bounds = widget
        .compute_bounds(window)
        .ok_or_else(|| "  the widget has no bounds".to_owned())?;
    let mut downloader = gtk::gdk::TextureDownloader::new(&texture);
    downloader.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    let mut pixels = Vec::new();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the bounds are within the window"
    )]
    let (left, top, right, bottom) = (
        bounds.x() as usize,
        bounds.y() as usize,
        (bounds.x() + bounds.width()) as usize,
        (bounds.y() + bounds.height()) as usize,
    );
    for y in top..bottom {
        let start = y * stride + left * 4;
        pixels.extend_from_slice(bytes.get(start..y * stride + right * 4).unwrap_or_default());
    }
    Ok(pixels)
}

/// Whether the spinner of the welcome screen looks different within a few seconds. A slow machine
/// can take a while to lay out a new spinner and to draw the next frame of its animation, so this
/// keeps looking instead of comparing two fixed moments.
fn spinner_turns(window: &Window) -> Result<bool, String> {
    let spinner = window
        .welcome()
        .spinner()
        .ok_or_else(|| "  no spinner".to_owned())?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut first: Option<Vec<u8>> = None;
    while Instant::now() < deadline {
        let pixels = pixels_of(window, spinner.upcast_ref())?;
        if !pixels.is_empty() {
            match &first {
                None => first = Some(pixels),
                Some(first) if *first != pixels => return Ok(true),
                Some(_) => {}
            }
        }
        run_until(Duration::from_millis(100), || false);
    }
    Ok(false)
}

/// The spinner of the welcome screen turns when it opens, and again after looking around.
fn check_spinner(settings: &gtk::Settings) -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, false)?;
    settings.set_gtk_enable_animations(true);
    let window = Window::with_model(&fixture.model);
    window.present();
    let outcome = (|| {
        settle(&window)?;
        ensure(spinner_turns(&window)?, "the spinner doesn't turn")?;
        window.look_around();
        settle(&window)?;
        window.status_button().emit_clicked();
        settle(&window)?;
        ensure(
            window.is_welcome_shown(),
            "the welcome screen didn't come back",
        )?;
        ensure(
            spinner_turns(&window)?,
            "the spinner doesn't turn after coming back",
        )
    })();
    window.destroy();
    settings.set_gtk_enable_animations(false);
    outcome
}

/// What `window` draws. Right after the content changed, a frame can pass with the layout still
/// due, and GTK draws nothing of the window then: this waits for the next one, a few times.
fn render_drawn(window: &impl IsA<gtk::Window>) -> Result<gsk::RenderNode, String> {
    let window = window.upcast_ref::<gtk::Window>();
    let mut node = render(window.upcast_ref());
    for _ in 0..5 {
        if node.is_ok() {
            break;
        }
        wait_for_paint(window)?;
        node = render(window.upcast_ref());
    }
    node
}

/// The checks of the device selectors and the device list.
fn check_devices() -> Vec<(String, Outcome)> {
    vec![
        (
            "devices: a row for one device".to_owned(),
            check_device_rows(),
        ),
        (
            "devices: a type without devices shows a hint".to_owned(),
            check_no_devices(),
        ),
        ("devices: plugged in".to_owned(), check_plugged_in()),
        ("devices: unplugged".to_owned(), check_unplugged()),
        ("devices: the test area".to_owned(), check_test_area()),
    ]
}

/// The device selector of `kind` on the open page of `window`.
fn selector(window: &Window, kind: InputType) -> Result<DeviceSelector, String> {
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    all::<DeviceSelector>(&content)
        .into_iter()
        .find(|selector| selector.kind() == Some(kind))
        .ok_or_else(|| format!("  no device selector for {kind:?}"))
}

/// Whether the rows of `key` on the open page of `window` are shown, and whether the hint that
/// starts with `hint` is.
fn shown_or_hint(window: &Window, key: &str, hint: &str) -> Result<(bool, bool), String> {
    let row = row(window, key)?;
    let list = row
        .ancestor(gtk::ListBox::static_type())
        .ok_or_else(|| format!("  the row of {key} isn't in a list"))?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let hint = all::<gtk::Label>(&content)
        .into_iter()
        .find(|label| label.has_css_class("st-section-hint") && label.text().starts_with(hint))
        .ok_or_else(|| format!("  no hint {hint:?}"))?;
    Ok((list.is_visible(), hint.is_visible()))
}

/// The mouse selector lists the mice, also one that is only stored; picking one points the rows
/// at it, and a change is stored for it on this machine.
fn check_device_rows() -> Outcome {
    let sway = laptop();
    let stored = format!(
        "{STORED}\n[device.\"1133:16500:Logitech_G305\".input.pointer]\nnatural_scroll = true\n"
    );
    let fixture = live_fixture(&sway, &stored)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let mice = selector(&window, InputType::Pointer)?;
    let labels = mice.labels();
    ensure(
        labels == ["All mice", "MX Master 3", "Logitech_G305 (not connected)"],
        &format!("  the mouse selector offers {labels:?}"),
    )?;
    let touchpads = selector(&window, InputType::Touchpad)?.labels();
    ensure(
        touchpads == ["All touchpads", "Synaptics TM3276-022"],
        &format!("  the touchpad selector offers {touchpads:?}"),
    )?;

    let accel = row(&window, "input.pointer.pointer_accel")?;
    ensure(accel.note().is_none(), "a row for all mice has a note")?;
    mice.select_device(Some(MOUSE_ID));
    settle(&window)?;
    ensure(
        accel.device().as_deref() == Some(MOUSE_ID),
        "the row isn't for the mouse picked",
    )?;
    ensure(
        row(&window, "input.touchpad.tap")?.device().is_none(),
        "a touchpad row follows the mouse selector",
    )?;
    let note = accel.note().unwrap_or_default();
    ensure(
        note == "Same as all mice",
        &format!("  the row of one mouse says {note:?}"),
    )?;
    let spin = accel
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::SpinButton>().ok())
        .ok_or_else(|| "  the speed row has no number field".to_owned())?;
    spin.set_value(0.5);
    wait_for_model(&fixture.model)?;
    let expected = format!("input \"{MOUSE_ID}\" pointer_accel 0.5");
    ensure(
        sway.commands().last() == Some(&expected),
        &format!("  sent {:?}", sway.commands()),
    )?;
    let setting = Registry::builtin()
        .get("input.pointer.pointer_accel")
        .ok_or_else(|| "  no pointer speed".to_owned())?;
    let view = fixture
        .model
        .view(setting, Some(MOUSE_ID))
        .ok_or_else(|| "  no settings".to_owned())?;
    ensure(
        view.source
            == Source::Stored {
                layer: Layer::Host,
                for_type: false,
            },
        &format!("  the mouse's value comes from {:?}", view.source),
    )?;
    ensure(
        accel.note().is_none(),
        "the row still says it is the same as all mice",
    )?;
    mice.select_device(None);
    settle(&window)?;
    ensure(accel.device().is_none(), "the row isn't back to all mice")?;
    window.destroy();
    Ok(())
}

/// Without a touchpad and nothing stored for touchpads, the touchpad section is a hint; a value
/// stored for all touchpads brings it back.
fn check_no_devices() -> Outcome {
    let sway = obedient_with_inputs(SWAY_1_10, format!("[{KEYBOARD}, {MOUSE}]"));
    let fixture = live_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    ensure(
        shown_or_hint(&window, "input.touchpad.tap", "No touchpad")? == (false, true),
        "the touchpad section shows without a touchpad",
    )?;
    ensure(
        shown_or_hint(&window, "input.pointer.pointer_accel", "No mouse")? == (true, false),
        "the mouse section is hidden with a mouse",
    )?;
    window.destroy();

    let fixture = live_fixture(&sway, STORED)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    ensure(
        shown_or_hint(&window, "input.touchpad.tap", "No touchpad")? == (true, false),
        "the touchpad section is hidden with a value stored for touchpads",
    )?;
    window.destroy();
    Ok(())
}

/// A touchpad that is plugged in shows up in its selector and brings its section.
fn check_plugged_in() -> Outcome {
    let sway = with_inputs(&[
        r#"[{"identifier": "1133:16511:Logitech_MX_Master_3", "name": "MX Master 3",
             "type": "pointer"}]"#,
        r#"[{"identifier": "1133:16511:Logitech_MX_Master_3", "name": "MX Master 3",
             "type": "pointer"},
            {"identifier": "1739:52619:Synaptics_TM3276-022", "name": "Synaptics TM3276-022",
             "type": "touchpad"}]"#,
    ]);
    let fixture = live_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let touchpads = selector(&window, InputType::Touchpad)?;
    let plugged = run_until(Duration::from_secs(5), || touchpads.labels().len() == 2);
    ensure(
        plugged,
        &format!("  the touchpad selector offers {:?}", touchpads.labels()),
    )?;
    ensure(
        shown_or_hint(&window, "input.touchpad.tap", "No touchpad")? == (true, false),
        "the touchpad section didn't come with the touchpad",
    )?;
    window.destroy();
    Ok(())
}

/// A mouse that is unplugged leaves its selector; the device picked goes back to all mice.
fn check_unplugged() -> Outcome {
    let sway = with_inputs(&[
        r#"[{"identifier": "1133:16511:Logitech_MX_Master_3", "name": "MX Master 3",
             "type": "pointer"},
            {"identifier": "1739:52619:Synaptics_TM3276-022", "name": "Synaptics TM3276-022",
             "type": "touchpad"}]"#,
        r#"[{"identifier": "1739:52619:Synaptics_TM3276-022", "name": "Synaptics TM3276-022",
             "type": "touchpad"}]"#,
    ]);
    let fixture = fixture_in(SWAY, "version = 1\n", USERS, true, Some(&sway))?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let gone = run_until(Duration::from_secs(5), || {
        selector(&window, InputType::Pointer).is_ok_and(|mice| mice.labels() == ["All mice"])
    });
    ensure(gone, "the unplugged mouse stays in its selector")?;
    ensure(
        shown_or_hint(&window, "input.pointer.pointer_accel", "No mouse")? == (false, true),
        "the mouse section stays without a mouse",
    )?;
    ensure(
        row(&window, "input.pointer.pointer_accel")?
            .device()
            .is_none(),
        "the rows aren't for all mice",
    )?;
    window.destroy();
    Ok(())
}

/// The Mouse & Touchpad page ends with the test area: the pad names the click, the strip
/// scrolls.
fn check_test_area() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let area = find::<TestArea>(&content).ok_or_else(|| "  no test area".to_owned())?;
    area.show_click(3, 1);
    ensure(
        area.last_click() == "Last: Right click",
        &format!("  the pad says {:?}", area.last_click()),
    )?;
    let lines = area.strip().vadjustment();
    ensure(
        lines.upper() > lines.page_size(),
        "the strip has nothing to scroll",
    )?;
    window.destroy();
    Ok(())
}

/// The Mouse & Touchpad page without a touchpad, checked like the pages.
fn check_no_devices_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let sway = obedient_with_inputs(SWAY_1_10, format!("[{KEYBOARD}, {MOUSE}]"));
    let fixture = live_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let directory = screenshots.map(|dir| dir.join("no-touchpad"));
    let outcome = check_page(&window, Page::MouseTouchpad, directory.as_deref(), scheme);
    window.destroy();
    outcome
}

/// The checks of the indicators of the rows and of the page reset.
fn check_indicators() -> Vec<(String, Outcome)> {
    vec![
        (
            "indicators: reset and this machine only".to_owned(),
            check_reset_and_machine(),
        ),
        (
            "indicators: conflicts with the user's config".to_owned(),
            check_conflicts(),
        ),
        (
            "indicators: a risky reset goes through the safety net".to_owned(),
            check_risky_reset(),
        ),
        (
            "indicators: a device's own value is reset".to_owned(),
            check_device_reset(),
        ),
        ("indicators: reset a page".to_owned(), check_page_reset()),
    ]
}

const TOUCHPAD_ACCEL: &str = "input.touchpad.pointer_accel";

/// The setting `key`.
fn setting(key: &str) -> Result<&'static sway_things_core::registry::Setting, String> {
    Registry::builtin()
        .get(key)
        .ok_or_else(|| format!("  no setting {key}"))
}

/// A stored value has a reset button and a "This machine only" toggle; the toggle moves it to
/// the host overlay, the reset removes it. A value that isn't stored has neither.
fn check_reset_and_machine() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let accel = row(&window, TOUCHPAD_ACCEL)?;
    let tap = row(&window, "input.touchpad.tap")?;
    ensure(
        tap.reset_button().is_none() && tap.machine_toggle().is_none(),
        "a value that isn't stored has indicators",
    )?;
    let machine = accel
        .machine_toggle()
        .ok_or_else(|| "  the stored speed has no machine toggle".to_owned())?;
    ensure(
        !machine.is_active(),
        "a shared value is for this machine only",
    )?;
    machine.set_active(true);
    wait_for_model(&fixture.model)?;
    ensure(
        fixture.model.stored_in(setting(TOUCHPAD_ACCEL)?, None) == Some(Layer::Host),
        "the value didn't move to the host overlay",
    )?;
    let reset = accel
        .reset_button()
        .ok_or_else(|| "  the stored speed has no reset button".to_owned())?;
    reset.emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        fixture
            .model
            .stored_in(setting(TOUCHPAD_ACCEL)?, None)
            .is_none(),
        "the reset value is still stored",
    )?;
    ensure(
        accel.reset_button().is_none() && accel.machine_toggle().is_none(),
        "the reset row still has its indicators",
    )?;
    window.destroy();
    Ok(())
}

/// A line of the user's config before the include line is overridden, one after it wins; both
/// show a badge and the ⋮ menu with the line. Letting the config win removes the stored value.
fn check_conflicts() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let before = format!(
        "input type:touchpad pointer_accel 0.1\n{}",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&before));
    let window = open(&fixture, Page::MouseTouchpad)?;
    let accel = row(&window, TOUCHPAD_ACCEL)?;
    ensure(
        accel.badges() == ["Overridden"],
        &format!("  a line before the include shows {:?}", accel.badges()),
    )?;
    let (title, place, line) = accel
        .conflict_details()
        .ok_or_else(|| "  no ⋮ menu for the overridden line".to_owned())?;
    ensure(
        title == "Overridden in your config"
            && place.ends_with("config:1")
            && line == "input type:touchpad pointer_accel 0.1",
        &format!("  the menu shows {title:?}, {place:?}, {line:?}"),
    )?;
    ensure(
        row(&window, "input.touchpad.tap")?
            .conflict_button()
            .is_none(),
        "a row without a conflict has a ⋮ menu",
    )?;

    let after = format!(
        "{}input type:touchpad pointer_accel 0.5\n",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&after));
    settle(&window)?;
    ensure(
        accel.badges() == ["Your config wins"],
        &format!("  a line after the include shows {:?}", accel.badges()),
    )?;
    accel.let_config_win();
    wait_for_model(&fixture.model)?;
    let view = fixture
        .model
        .view(setting(TOUCHPAD_ACCEL)?, None)
        .ok_or_else(|| "  no settings".to_owned())?;
    ensure(
        view.value == Value::Decimal(0.5) && matches!(view.source, Source::Config { .. }),
        &format!("  after letting the config win: {view:?}"),
    )?;
    ensure(
        accel.badges().is_empty() && accel.conflict_button().is_none(),
        "the row still shows the conflict",
    )?;
    window.destroy();
    Ok(())
}

/// The checks of the editor command preference.
fn check_editor() -> Vec<(String, Outcome)> {
    vec![
        (
            "editor: the conflict menu opens the file at the line".to_owned(),
            check_editor_conflict(),
        ),
        (
            "editor: the welcome screen opens the config".to_owned(),
            check_editor_welcome(),
        ),
        (
            "editor: a command that can't be split shows why".to_owned(),
            check_editor_command_error(),
        ),
        (
            "editor: a failed open shows why until one works".to_owned(),
            check_editor_failure_clears(),
        ),
    ]
}

/// Waits until `path` exists, as a program the app started creates it.
fn wait_for_file(path: &Path) -> Outcome {
    ensure(
        run_until(Duration::from_secs(10), || path.exists()),
        &format!("  {} wasn't created", path.display()),
    )
}

/// The directory of the user's sway config of `fixture`, which only exists in memory.
fn config_dir(fixture: &Fixture) -> Outcome {
    let dir = fixture.main.parent().unwrap_or(Path::new("/"));
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())
}

/// _Open config in editor_ in the ⋮ menu runs the editor command with the file and the line of
/// the user's config filled in, without a shell.
fn check_editor_conflict() -> Outcome {
    let stored = format!("{STORED}\n[app]\neditor_command = \"touch '{{file}}.at {{line}}'\"\n");
    let fixture = fixture(SWAYFX, &stored, USERS, true)?;
    config_dir(&fixture)?;
    let before = format!(
        "\ninput type:touchpad pointer_accel 0.1\n{}",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&before));
    let window = open(&fixture, Page::MouseTouchpad)?;
    let accel = row(&window, TOUCHPAD_ACCEL)?;
    ensure(accel.conflict_button().is_some(), "no ⋮ menu for the line")?;
    accel.open_config();
    let mut opened = fixture.main.clone().into_os_string();
    opened.push(".at 2");
    wait_for_file(Path::new(&opened))?;
    settle(&window)?;
    ensure(accel.error().is_none(), "opening the editor shows an error")?;
    window.destroy();
    Ok(())
}

/// _Open config in editor_ on the welcome screen runs the editor command with the config.
fn check_editor_welcome() -> Outcome {
    let stored = "version = 1\n\n[app]\neditor_command = \"touch {file}.opened\"\n";
    let fixture = fixture(SWAYFX, stored, USERS, false)?;
    config_dir(&fixture)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(window.is_welcome_shown(), "the welcome screen isn't shown")?;
    let [open, _, _] = window.welcome().buttons();
    open.emit_clicked();
    let mut opened = fixture.main.clone().into_os_string();
    opened.push(".opened");
    wait_for_file(Path::new(&opened))?;
    window.destroy();
    Ok(())
}

/// Waits until `error` gives what `expected` says: an error or none.
fn wait_for_error(error: impl Fn() -> Option<String>, expected: bool, what: &str) -> Outcome {
    ensure(
        run_until(Duration::from_secs(10), || error().is_some() == expected),
        what,
    )
}

/// An editor that ends with an error shows it, under the row and on the welcome screen; the
/// next open that works takes it away.
fn check_editor_failure_clears() -> Outcome {
    let stored = format!("{STORED}\n[app]\neditor_command = \"false\"\n");
    let fixture = fixture(SWAYFX, &stored, USERS, true)?;
    config_dir(&fixture)?;
    let before = format!(
        "\ninput type:touchpad pointer_accel 0.1\n{}",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&before));
    let window = open(&fixture, Page::MouseTouchpad)?;
    let accel = row(&window, TOUCHPAD_ACCEL)?;
    accel.open_config();
    wait_for_error(|| accel.error(), true, "the failed editor shows no error")?;
    let error = accel.error().unwrap_or_default();
    ensure(
        error.starts_with("false ended with"),
        &format!("  the row shows {error:?}"),
    )?;
    fixture
        .model
        .set("app.editor_command", None, Value::Text("true".to_owned()));
    wait_for_model(&fixture.model)?;
    accel.open_config();
    wait_for_error(
        || accel.error(),
        false,
        "the error stays after an open that works",
    )?;
    window.destroy();
    editor_failure_clears_welcome()
}

/// The welcome screen's part of [`check_editor_failure_clears`].
fn editor_failure_clears_welcome() -> Outcome {
    // Without the include line, the window opens on the welcome screen, and the preference
    // can't change: the command fails until the file it tests for is there.
    let stored = "version = 1\n\n[app]\neditor_command = \"test -e {file}.ok\"\n";
    let fixture = fixture(SWAYFX, stored, USERS, false)?;
    config_dir(&fixture)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(window.is_welcome_shown(), "the welcome screen isn't shown")?;
    let [open, _, _] = window.welcome().buttons();
    let welcome = window.welcome().clone();
    open.emit_clicked();
    wait_for_error(
        || welcome.error(),
        true,
        "the failed editor shows no error on the welcome screen",
    )?;
    let mut ok = fixture.main.clone().into_os_string();
    ok.push(".ok");
    write_file(Path::new(&ok), "")?;
    open.emit_clicked();
    wait_for_error(
        || welcome.error(),
        false,
        "the welcome screen keeps the error after an open that works",
    )?;
    window.destroy();
    Ok(())
}

/// An editor command with a quote that isn't closed isn't stored, and its row says why; one
/// that splits is stored. The entry hints at what an empty command means.
fn check_editor_command_error() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::About)?;
    let editor = row(&window, "app.editor_command")?;
    let entry = editor
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Entry>().ok())
        .ok_or_else(|| "  the editor row has no entry".to_owned())?;
    ensure(
        entry
            .placeholder_text()
            .is_some_and(|hint| hint.contains("$EDITOR")),
        "the empty entry has no hint",
    )?;
    entry.set_text("'/opt/My Editor/bin/edit");
    entry.emit_activate();
    wait_for_model(&fixture.model)?;
    let error = editor.error().unwrap_or_default();
    ensure(
        error.contains("quote that isn't closed"),
        &format!("  the row shows {error:?}"),
    )?;
    let setting = setting("app.editor_command")?;
    ensure(
        fixture.model.stored_in(setting, None).is_none(),
        "the command was stored",
    )?;
    entry.set_text("'/opt/My Editor/bin/edit' --line {line}");
    entry.emit_activate();
    wait_for_model(&fixture.model)?;
    ensure(editor.error().is_none(), "the good command shows an error")?;
    ensure(
        fixture.model.stored_in(setting, None).is_some(),
        "the good command wasn't stored",
    )?;
    window.destroy();
    Ok(())
}

/// Resetting a risky value tries it with the safety net, and removes it on Keep.
fn check_risky_reset() -> Outcome {
    let sway = laptop();
    let stored = "version = 1\n\n[input.pointer]\nevents = false\n";
    let fixture = live_fixture(&sway, stored)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let enabled = row(&window, POINTER_EVENTS)?;
    let reset = enabled
        .reset_button()
        .ok_or_else(|| "  the stored value has no reset button".to_owned())?;
    reset.emit_clicked();
    wait_for_model(&fixture.model)?;
    let banner = window.safety_banner().unwrap_or_default();
    ensure(
        banner.starts_with("Keep the change to Mouse › Enabled?"),
        &format!("  the banner says {banner:?}"),
    )?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command.ends_with("events enabled")),
        "the default wasn't tried",
    )?;
    window.keep_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        fixture
            .model
            .stored_in(setting(POINTER_EVENTS)?, None)
            .is_none(),
        "the kept reset left the value stored",
    )?;
    window.destroy();
    Ok(())
}

/// The reset of a device's own value goes back to the value of all mice.
fn check_device_reset() -> Outcome {
    let sway = laptop();
    let stored =
        format!("version = 1\n\n[device.\"{MOUSE_ID}\".input.pointer]\npointer_accel = 0.5\n");
    let fixture = live_fixture(&sway, &stored)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    selector(&window, InputType::Pointer)?.select_device(Some(MOUSE_ID));
    settle(&window)?;
    let accel = row(&window, "input.pointer.pointer_accel")?;
    let reset = accel
        .reset_button()
        .ok_or_else(|| "  the mouse's own value has no reset button".to_owned())?;
    ensure(
        reset.tooltip_text().as_deref() == Some("Use the value of all mice"),
        &format!("  the reset button says {:?}", reset.tooltip_text()),
    )?;
    reset.emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        accel.note().as_deref() == Some("Same as all mice"),
        &format!("  after the reset the row says {:?}", accel.note()),
    )?;
    window.destroy();
    Ok(())
}

/// A page reset removes every stored value of the page but those that can lock the user out.
fn check_page_reset() -> Outcome {
    let stored = "version = 1\n\n[input.pointer]\nevents = false\n\n\
                  [input.touchpad]\npointer_accel = 0.3\ntap = true\n";
    let fixture = fixture(SWAYFX, stored, USERS, true)?;
    let window = open(&fixture, Page::MouseTouchpad)?;
    let (removed, kept) = fixture.model.page_reset(Page::MouseTouchpad);
    let kept: Vec<&str> = kept.iter().map(|setting| setting.key()).collect();
    ensure(
        removed == 2 && kept == [POINTER_EVENTS],
        &format!("  the page reset removes {removed} and keeps {kept:?}"),
    )?;
    let content = window
        .content()
        .and_downcast::<PageView>()
        .ok_or_else(|| "  the page isn't a page view".to_owned())?;
    let button = content
        .reset_button()
        .ok_or_else(|| "  no Reset page button".to_owned())?;
    ensure(button.is_sensitive(), "Reset page can't be pressed")?;
    fixture.model.reset_page(Page::MouseTouchpad);
    wait_for_model(&fixture.model)?;
    ensure(
        fixture.model.page_reset(Page::MouseTouchpad).0 == 0,
        "values are left after the page reset",
    )?;
    ensure(
        fixture
            .model
            .stored_in(setting(POINTER_EVENTS)?, None)
            .is_some(),
        "the page reset removed a risky value",
    )?;
    ensure(
        !button.is_sensitive(),
        "Reset page can be pressed with only risky values left",
    )?;
    window.destroy();
    Ok(())
}

/// The Mouse & Touchpad page with stored values, both kinds of conflict and the machine toggle,
/// checked like the pages.
fn check_indicators_screen(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let stored = STORED.replace("pointer_accel = 0.3\n", "pointer_accel = 0.3\ntap = true\n");
    let fixture = fixture(SWAYFX, &format!("{stored}{DIRECTIVES}"), USERS, true)?;
    let after = format!(
        "input type:touchpad tap disabled\n{}input type:touchpad pointer_accel 0.5\n",
        fixture.include_line()
    );
    fixture.model.set_status(fixture.status_of(&after));
    let window = Window::with_model(&fixture.model);
    window.present();
    let directory = screenshots.map(|dir| dir.join("indicators"));
    let outcome = check_page(&window, Page::MouseTouchpad, directory.as_deref(), scheme);
    window.destroy();
    outcome
}

/// The accent is the stored focus color, follows a change of it, and gives way to the theme's own
/// with the preference.
fn check_accent() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = open(&fixture, Page::About)?;
    let css = theme::accent_css();
    ensure(
        css.contains("@define-color accent_bg_color #3b6e4f;"),
        &format!("  the accent isn't the stored focus color:\n{css}"),
    )?;
    fixture.model.set(
        "windows.focused.child_border",
        None,
        Value::Color(sway_things_core::command::Color::rgb(0xe0, 0x1b, 0x24)),
    );
    wait_for_model(&fixture.model)?;
    ensure(
        theme::accent_css().contains("@define-color accent_bg_color #e01b24;"),
        "the accent didn't follow the focus color",
    )?;
    fixture
        .model
        .set("app.accent_source", None, Value::Choice("theme".to_owned()));
    wait_for_model(&fixture.model)?;
    ensure(
        theme::accent_css().is_empty(),
        "the accent stays with the theme's accent chosen",
    )?;
    window.destroy();
    Ok(())
}

/// The checks of the Keyboard page's own widgets.
fn check_keyboard() -> Vec<(String, Outcome)> {
    vec![
        (
            "keyboard page: layouts move with their variants".to_owned(),
            check_layout_move(),
        ),
        (
            "keyboard page: layouts are added and the last one stays".to_owned(),
            check_layout_add_remove(),
        ),
        ("keyboard page: XKB options".to_owned(), check_xkb_options()),
        (
            "keyboard page: model and active layout".to_owned(),
            check_model_and_active_layout(),
        ),
    ]
}

const LAYOUTS: &str = "input.keyboard.xkb_layout";
const VARIANTS: &str = "input.keyboard.xkb_variant";
const XKB_OPTIONS: &str = "input.keyboard.xkb_options";

/// A keyboard with two layouts, the first one in use.
const KEYBOARD_WITH_LAYOUTS: &str = r#"{"identifier": "1:1:AT_Translated_Set_2_keyboard",
    "name": "AT Translated Set 2 keyboard", "type": "keyboard",
    "xkb_layout_names": ["English (US)", "German (no dead keys)"],
    "xkb_active_layout_index": 0, "libinput": {"send_events": "enabled"}}"#;

/// The only widget of type `T` on the open page of `window`.
fn widget<T: IsA<gtk::Widget>>(window: &Window) -> Result<T, String> {
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    all::<T>(&content)
        .into_iter()
        .next()
        .ok_or_else(|| format!("  no {} on the page", T::static_type().name()))
}

/// Strings for a list value.
fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// Moving a layout moves its variant too, as one change with one banner; Keep stores both.
fn check_layout_move() -> Outcome {
    let sway = laptop();
    let stored = "version = 1\n\n[input.keyboard]\nxkb_layout = [\"us\", \"de\"]\n\
                  xkb_variant = [\"\", \"nodeadkeys\"]\n";
    let fixture = live_fixture(&sway, stored)?;
    let window = open(&fixture, Page::Keyboard)?;
    let list = widget::<LayoutList>(&window)?;
    ensure(
        list.shown() == (strings(&["us", "de"]), strings(&["", "nodeadkeys"])),
        &format!("  the list shows {:?}", list.shown()),
    )?;
    ensure(list.rows().len() == 2, "  the list has no row per layout")?;
    list.move_layout(1, 0);
    wait_for_model(&fixture.model)?;
    let banner = window.safety_banner().unwrap_or_default();
    ensure(
        banner.starts_with("Keep the change to Layouts? Reverting in "),
        &format!("  the banner says {banner:?}"),
    )?;
    ensure(
        list.shown() == (strings(&["de", "us"]), strings(&["nodeadkeys", ""])),
        &format!("  the list shows {:?} while the change waits", list.shown()),
    )?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command.ends_with(r#"xkb_variant "nodeadkeys,""#)),
        &format!("  sway got {:?}", sway.commands()),
    )?;
    window.keep_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        shown(&fixture.model, LAYOUTS)? == (Value::List(strings(&["de", "us"])), true)
            && shown(&fixture.model, VARIANTS)? == (Value::List(strings(&["nodeadkeys"])), true),
        "  the kept layouts and variants aren't stored",
    )?;
    window.destroy();
    Ok(())
}

/// A layout added with the safety net is gone after Revert; the last layout can't be removed.
fn check_layout_add_remove() -> Outcome {
    let sway = laptop();
    let fixture = live_fixture(&sway, STORED)?;
    let window = open(&fixture, Page::Keyboard)?;
    let list = widget::<LayoutList>(&window)?;
    ensure(
        list.shown() == (strings(&["us"]), strings(&[""])),
        &format!("  the list shows {:?}", list.shown()),
    )?;
    let removes: Vec<gtk::Button> = list
        .rows()
        .iter()
        .flat_map(|row| all::<gtk::Button>(row.upcast_ref()))
        .filter(|button| !button.is::<gtk::ToggleButton>())
        .collect();
    ensure(
        removes.len() == 1 && removes.iter().all(|button| !button.is_sensitive()),
        "  the last layout can be removed",
    )?;
    list.remove(0);
    wait_for_model(&fixture.model)?;
    ensure(
        window.safety_banner().is_none() && sway.commands().is_empty(),
        "  removing the last layout was tried",
    )?;
    list.add("de", "nodeadkeys");
    wait_for_model(&fixture.model)?;
    ensure(
        list.shown() == (strings(&["us", "de"]), strings(&["", "nodeadkeys"])),
        &format!("  the list shows {:?} after adding", list.shown()),
    )?;
    ensure(
        window.safety_banner().is_some(),
        "  adding a layout shows no banner",
    )?;
    window.revert_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        list.shown() == (strings(&["us"]), strings(&[""])),
        &format!("  the list shows {:?} after Revert", list.shown()),
    )?;
    ensure(
        !shown(&fixture.model, LAYOUTS)?.1 && !shown(&fixture.model, VARIANTS)?.1,
        "  the reverted layouts are stored",
    )?;
    window.destroy();
    Ok(())
}

/// The swap switch and an option of the browser try the options through the safety net.
fn check_xkb_options() -> Outcome {
    let sway = laptop();
    let fixture = live_fixture(&sway, STORED)?;
    let window = open(&fixture, Page::Keyboard)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let swap_switch = all::<gtk::Switch>(&content)
        .into_iter()
        .find(|switch| {
            switch.accessible_role() == gtk::AccessibleRole::Switch
                && switch.tooltip_text().is_none()
                && switch.ancestor(SettingRow::static_type()).is_none()
        })
        .ok_or_else(|| "  no swap switch".to_owned())?;
    swap_switch.set_active(true);
    wait_for_model(&fixture.model)?;
    ensure(
        shown(&fixture.model, XKB_OPTIONS)?
            == (Value::List(strings(&["ctrl:swap_lalt_lctl"])), false),
        "  the swap switch doesn't try its option",
    )?;
    window.revert_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        !swap_switch.is_active(),
        "  the switch doesn't show the old value",
    )?;

    let browser = widget::<OptionBrowser>(&window)?;
    browser.expander().set_expanded(true);
    if let Some(check) = browser.check("compose:ralt") {
        check.set_active(true);
        wait_for_model(&fixture.model)?;
        ensure(
            shown(&fixture.model, XKB_OPTIONS)? == (Value::List(strings(&["compose:ralt"])), false),
            "  the browser doesn't try its option",
        )?;
        window.keep_button().emit_clicked();
        wait_for_model(&fixture.model)?;
        ensure(
            shown(&fixture.model, XKB_OPTIONS)? == (Value::List(strings(&["compose:ralt"])), true),
            "  the kept option isn't stored",
        )?;
    }
    window.destroy();
    Ok(())
}

/// The model is a drop-down, and _Active layout_ lists sway's layouts and switches them.
fn check_model_and_active_layout() -> Outcome {
    let sway = obedient_with_inputs(
        SWAY_1_10,
        format!("[{KEYBOARD_WITH_LAYOUTS}, {MOUSE}, {TOUCHPAD}]"),
    );
    let fixture = live_fixture(&sway, STORED)?;
    let window = open(&fixture, Page::Keyboard)?;
    let model_row = row(&window, "input.keyboard.xkb_model")?;
    ensure(
        model_row
            .controls()
            .iter()
            .any(ObjectExt::is::<gtk::DropDown>),
        "  the keyboard model isn't a drop-down",
    )?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let active = all::<gtk::DropDown>(&content)
        .into_iter()
        .find(|dropdown| {
            dropdown
                .model()
                .and_downcast::<gtk::StringList>()
                .and_then(|labels| labels.string(1))
                .is_some_and(|label| label == "German (no dead keys)")
        })
        .ok_or_else(|| "  no drop-down with the layouts sway reports".to_owned())?;
    ensure(
        active.is_sensitive(),
        "  the active layout can't be switched",
    )?;
    active.set_selected(1);
    wait_for_model(&fixture.model)?;
    ensure(
        sway.commands()
            .last()
            .is_some_and(|command| command == "input type:keyboard xkb_switch_layout 1"),
        &format!("  sway got {:?}", sway.commands()),
    )?;
    window.destroy();
    Ok(())
}

/// The address of the headset of [`bluetooth_stand_in`], paired, with a battery.
const HEADSET: &str = "AA:BB:CC:DD:EE:FF";
/// The address of its speaker, not paired.
const SPEAKER: &str = "11:22:33:44:55:66";

/// A stand-in for BlueZ with two adapters, a paired headset and a speaker nearby.
fn bluetooth_stand_in() -> Result<StandIn, String> {
    async_io::block_on(async {
        let stand_in = StandIn::start().await?;
        stand_in.add_adapter("hci0", "laptop").await?;
        stand_in.add_adapter("hci1", "dongle").await?;
        let headset = stand_in.add_device("hci0", HEADSET, "Headset").await?;
        stand_in.pair_device("hci0", HEADSET).await?;
        stand_in
            .add_properties(
                &headset,
                "org.bluez.Battery1",
                &[("Percentage", zbus::zvariant::Value::from(80u8))],
            )
            .await?;
        stand_in.add_device("hci0", SPEAKER, "Speaker").await?;
        Ok::<_, sway_things_core::bluetooth::stand_in::Error>(stand_in)
    })
    .map_err(|error| format!("  BlueZ's stand-in: {error}"))
}

/// Points the Bluetooth page of `window` at `stand_in`, if there is one.
fn use_stand_in(window: &Window, stand_in: Option<&StandIn>) {
    if let Some(stand_in) = stand_in {
        window.use_bluetooth_bus(BluetoothBus::Address(stand_in.address().to_owned()));
    }
}

/// The first button labeled `label` in `widget`.
fn button_labeled(widget: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    if let Some(button) = widget.downcast_ref::<gtk::Button>() {
        if button.label().as_deref() == Some(label) {
            return Some(button.clone());
        }
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = button_labeled(&current, label) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

/// The Bluetooth page against the stand-in: the devices in their lists, discovery while it is
/// shown, connecting, pairing through the agent's dialog, and switching the radio off.
fn check_bluetooth() -> Outcome {
    let stand_in = bluetooth_stand_in()?;
    let page = BluetoothPage::new(&BluetoothBus::Address(stand_in.address().to_owned()));
    let window = gtk::Window::new();
    window.set_default_size(720, 700);
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let shown = run_until(Duration::from_secs(10), || {
        page.device_rows(true).len() == 1 && page.device_rows(false).len() == 1
    });
    let rows = |mine| page.device_rows(mine);
    ensure(
        shown,
        &format!(
            "  the devices didn't show: {:?} / {:?}",
            rows(true),
            rows(false)
        ),
    )?;
    let row = |name: &str, status: &str, button: &str| {
        (name.to_owned(), status.to_owned(), button.to_owned())
    };
    ensure(
        rows(true)
            == [row(
                "Headset",
                "Not connected \u{b7} Battery 80 %",
                "Connect",
            )],
        &format!("  my devices: {:?}", rows(true)),
    )?;
    ensure(
        rows(false) == [row("Speaker", "Not paired", "Connect")],
        &format!("  other devices: {:?}", rows(false)),
    )?;
    ensure(
        page.adapter_choice().is_visible(),
        "  two adapters but no adapter choice",
    )?;
    let calls = |path: &str, method: &str| {
        async_io::block_on(stand_in.calls(path, method)).map_or(0, |calls| calls.len())
    };
    ensure(
        run_until(Duration::from_secs(5), || {
            calls("/org/bluez/hci0", "StartDiscovery") > 0
        }),
        "  the page didn't look for devices while shown",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || page.is_searching()),
        "  no turning spinner while looking for devices",
    )?;
    ensure(
        !page.search_button().is_visible(),
        "  a Search button while the page searches by itself",
    )?;

    page.device_button("Headset")
        .ok_or("  no button for the headset")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || {
            rows(true).first().is_some_and(|(_, status, button)| {
                status.starts_with("Connected") && button == "Disconnect"
            })
        }),
        &format!("  the headset didn't connect: {:?}", rows(true)),
    )?;

    check_bluetooth_pairing(&page, &stand_in)?;
    check_bluetooth_adapter(&page, &stand_in)?;

    page.power_switch().set_active(false);
    ensure(
        run_until(Duration::from_secs(5), || !page.power_switch().state()),
        "  Bluetooth didn't turn off",
    )?;
    ensure(
        !page.visible_switch().is_sensitive() && !page.adapter_rename_button().is_sensitive(),
        "  visibility and renaming stayed available with the radio off",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || {
            calls("/org/bluez/hci0", "StopDiscovery") > 0
        }),
        "  the page didn't stop looking for devices when the radio went off",
    )?;
    check_bluetooth_off(&page)?;
    window.destroy();
    Ok(())
}

/// The placeholder of `page` while its radio is off: it replaces the devices, and turning the
/// radio back on with the switch brings the devices back.
fn check_bluetooth_off(page: &BluetoothPage) -> Outcome {
    let texts = page
        .off_placeholder()
        .ok_or("  no placeholder while Bluetooth is off")?;
    ensure(
        texts.first().map(String::as_str) == Some("Bluetooth is off"),
        &format!("  placeholder: {texts:?}"),
    )?;
    ensure(
        !page
            .device_button("Headset")
            .is_some_and(|button| button.is_drawable()),
        "  the devices still show while Bluetooth is off",
    )?;
    page.power_switch().set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || {
            page.power_switch().state() && page.off_placeholder().is_none()
        }),
        "  turning Bluetooth on didn't remove the placeholder",
    )?;
    Ok(())
}

/// Renaming the selected adapter of [`bluetooth_stand_in`] on `page` through its dialog, and
/// making it visible; the switch follows BlueZ when the time is up.
fn check_bluetooth_adapter(page: &BluetoothPage, stand_in: &StandIn) -> Outcome {
    let bluez = async_io::block_on(stand_in.bluez()).map_err(|error| format!("  {error}"))?;
    let name = || {
        async_io::block_on(bluez.snapshot())
            .ok()
            .and_then(|snapshot| Some(snapshot.adapter("/org/bluez/hci0")?.name.clone()))
    };
    page.adapter_rename_button().emit_clicked();
    let dialog = || {
        gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::Window>().ok())
            .find(|window| {
                window.is_visible() && button_labeled(window.upcast_ref(), "Rename").is_some()
            })
    };
    ensure(
        run_until(Duration::from_secs(5), || dialog().is_some()),
        "  Rename… didn't ask for a name",
    )?;
    let dialog = dialog().ok_or("  no dialog")?;
    find::<gtk::Entry>(dialog.upcast_ref())
        .ok_or("  the dialog has no field")?
        .set_text("Work laptop");
    button_labeled(dialog.upcast_ref(), "Rename")
        .ok_or("  the dialog has no Rename button")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || {
            name().as_deref() == Some("Work laptop")
        }),
        &format!("  the adapter wasn't renamed: {:?}", name()),
    )?;

    ensure(
        page.visible_switch().is_sensitive(),
        "  the visibility switch isn't usable with the radio on",
    )?;
    // The stand-in makes the adapter visible when discovery starts: hide it first.
    let hide = || {
        async_io::block_on(stand_in.update(
            "/org/bluez/hci0",
            "org.bluez.Adapter1",
            &[("Discoverable", zbus::zvariant::Value::from(false))],
        ))
        .map_err(|error| format!("  {error}"))
    };
    hide()?;
    ensure(
        run_until(Duration::from_secs(5), || !page.visible_switch().state()),
        "  the switch didn't follow BlueZ hiding the adapter",
    )?;
    page.visible_switch().set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || page.visible_switch().state()),
        &format!("  the adapter didn't become visible: {}", page.error_text()),
    )?;
    ensure(
        async_io::block_on(bluez.snapshot())
            .ok()
            .and_then(|s| Some(s.adapter("/org/bluez/hci0")?.discoverable))
            == Some(true),
        "  BlueZ didn't make the adapter visible",
    )?;
    hide()?;
    ensure(
        run_until(Duration::from_secs(5), || !page.visible_switch().state()),
        "  the switch didn't follow BlueZ when the time was up",
    )?;
    Ok(())
}

/// The Bluetooth page as it searches by itself and as it searches only on request.
fn check_bluetooth_pages() -> Vec<(String, Outcome)> {
    vec![
        (
            "bluetooth: devices, pairing and power".to_owned(),
            check_bluetooth(),
        ),
        (
            "bluetooth: searching only with the Search button".to_owned(),
            check_bluetooth_manual(false),
        ),
        (
            "bluetooth: searching only with the Search button (dark)".to_owned(),
            check_bluetooth_manual(true),
        ),
    ]
}

/// Saves `window` as `<name>.png` in `$SWAY_THINGS_SCREENSHOTS`, if set, once it is drawn, and
/// checks its contrast then.
fn shoot(window: &gtk::Window, name: &str) -> Outcome {
    if let Some(directory) = std::env::var_os("SWAY_THINGS_SCREENSHOTS") {
        // A widget shown just now, such as a new spinner, is drawn with the next frames.
        settle(window)?;
        check_screen(
            window,
            Some(PathBuf::from(directory).join(format!("{name}.png"))),
        )?;
    }
    Ok(())
}

/// The Bluetooth page with searches only on request: no discovery until _Search_, one after it,
/// and _Stop_ ends it.
fn check_bluetooth_manual(dark: bool) -> Outcome {
    let style = if dark { "dark" } else { "light" };
    let settings = gtk::Settings::default().ok_or("  no GTK settings")?;
    set_dark(&settings, dark);
    let stand_in = bluetooth_stand_in()?;
    let page = BluetoothPage::new(&BluetoothBus::Address(stand_in.address().to_owned()));
    page.set_manual_search(true);
    let window = gtk::Window::new();
    window.set_default_size(720, 700);
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let calls = |method: &str| {
        async_io::block_on(stand_in.calls("/org/bluez/hci0", method)).map_or(0, |calls| calls.len())
    };
    ensure(
        run_until(Duration::from_secs(10), || {
            page.device_rows(false).len() == 1
        }),
        "  the devices didn't show",
    )?;
    ensure(
        !run_until(Duration::from_secs(2), || calls("StartDiscovery") > 0),
        "  the page looked for devices without Search",
    )?;
    ensure(!page.is_searching(), "  a spinner without a search")?;
    ensure(page.search_button().is_visible(), "  no Search button")?;
    shoot(&window, &format!("bluetooth-manual-idle-{style}"))?;
    page.search_button().emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || calls("StartDiscovery") == 1),
        "  Search didn't look for devices",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || page.is_searching()),
        "  no turning spinner while searching",
    )?;
    ensure(
        page.search_text() == "Stop",
        "  the button doesn't read Stop while searching",
    )?;
    window.set_focus_visible(true);
    page.search_button().grab_focus();
    shoot(&window, &format!("bluetooth-manual-running-{style}"))?;
    page.search_button().emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || calls("StopDiscovery") > 0),
        "  Stop didn't stop looking for devices",
    )?;
    ensure(
        page.search_text() == "Search",
        "  the button doesn't read Search again",
    )?;
    // Switched to automatic, the button goes and the page looks for devices by itself.
    let started = calls("StartDiscovery");
    page.set_manual_search(false);
    ensure(
        !page.search_button().is_visible(),
        "  the Search button stayed in automatic mode",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || calls("StartDiscovery") > started),
        "  automatic mode didn't look for devices",
    )?;
    window.destroy();
    set_dark(&settings, false);
    Ok(())
}

/// Pairing the speaker of [`bluetooth_stand_in`] on `page`: it asks to confirm a code, and
/// confirming pairs and connects it.
fn check_bluetooth_pairing(page: &BluetoothPage, stand_in: &StandIn) -> Outcome {
    let rows = |mine| page.device_rows(mine);
    let speaker = format!("/org/bluez/hci0/dev_{}", SPEAKER.replace(':', "_"));
    let name = page.bus_name().ok_or("  the page has no bus name")?;
    async_io::block_on(stand_in.pair_asks(
        &speaker,
        &name,
        "agent.RequestConfirmation(device, dbus.UInt32(12345))",
        "True",
    ))
    .map_err(|error| format!("  {error}"))?;
    page.device_button("Speaker")
        .ok_or("  no button for the speaker")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || page.pairing_dialog().is_some()),
        "  pairing didn't ask to confirm the code",
    )?;
    let dialog = page.pairing_dialog().ok_or("  no dialog")?;
    let code = find::<gtk::Label>(dialog.upcast_ref());
    ensure(code.is_some(), "  the dialog has no text")?;
    button_labeled(dialog.upcast_ref(), "Pair")
        .ok_or("  the dialog has no Pair button")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || {
            rows(true)
                .iter()
                .any(|(name, status, _)| name == "Speaker" && status.starts_with("Connected"))
        }),
        &format!(
            "  the speaker wasn't paired and connected: {:?} / {:?}; error: {}",
            rows(true),
            rows(false),
            page.error_text()
        ),
    )?;
    ensure(page.pairing_dialog().is_none(), "  the dialog stayed open")?;
    Ok(())
}

/// The NetworkManager manager object.
const NM_MANAGER: &str = "/org/freedesktop/NetworkManager";

/// A stand-in for NetworkManager with a Wi-Fi adapter that sees a saved network ("Home", WPA2),
/// a new one ("Office", WPA2), an open one ("Cafe") and an 802.1X one ("eduroam"), a saved
/// network that isn't around ("Attic"), a wired device with a saved connection and a VPN; the
/// path of the Wi-Fi device too.
fn network_stand_in() -> Result<(NmStandIn, String), String> {
    async_io::block_on(async {
        let stand_in = NmStandIn::start().await?;
        let wlan = stand_in.add_wifi_device("wlan0", "wlan0").await?;
        for (name, ssid, strength, security) in [
            ("home", "Home", 90, nm_stand_in::WPA2),
            ("office", "Office", 70, nm_stand_in::WPA2),
            ("cafe", "Cafe", 40, nm_stand_in::OPEN),
            ("eduroam", "eduroam", 60, 0x200),
        ] {
            stand_in
                .add_access_point(&wlan, name, ssid, "00:11:22:33:44:55", strength, security)
                .await?;
        }
        stand_in.add_wifi_connection(&wlan, "home", "Home").await?;
        let value = |text: &str| zbus::zvariant::Value::from(text.to_owned());
        let settings = |id: &str, kind: &str| {
            std::collections::HashMap::from([(
                "connection",
                std::collections::HashMap::from([("id", value(id)), ("type", value(kind))]),
            )])
        };
        let mut attic = settings("Attic", "802-11-wireless");
        attic.insert(
            "802-11-wireless",
            std::collections::HashMap::from([(
                "ssid",
                zbus::zvariant::Value::from(b"Attic".to_vec()),
            )]),
        );
        stand_in.add_connection(&attic).await?;
        let eth = stand_in
            .add_ethernet_device("eth0", "eth0", nm_stand_in::DISCONNECTED)
            .await?;
        let wired = stand_in
            .add_connection(&settings("Wired", "802-3-ethernet"))
            .await?;
        stand_in.make_available(&eth, &wired).await?;
        let mut vpn = settings("Office VPN", "vpn");
        vpn.insert(
            "vpn",
            std::collections::HashMap::from([(
                "service-type",
                value("org.freedesktop.NetworkManager.openvpn"),
            )]),
        );
        stand_in.add_connection(&vpn).await?;
        Ok::<_, nm_stand_in::Error>((stand_in, wlan))
    })
    .map_err(|error| format!("  NetworkManager's stand-in: {error}"))
}

/// The Network page against the stand-in: the networks in their lists, scans and the agent while
/// it is shown, connecting to a saved network and to a new one with its password, the agent's
/// dialog for a wrong password, the wired device and the VPN, the captive portal note, and
/// switching Wi-Fi off.
fn check_network() -> Outcome {
    let (stand_in, wlan) = network_stand_in()?;
    let page = NetworkPage::new(&NetworkBus::Address(stand_in.address().to_owned()));
    let window = gtk::Window::new();
    window.set_default_size(720, 900);
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let rows = |list: &str| page.rows(list);
    let row = |name: &str, status: &str, control: &str| {
        (name.to_owned(), status.to_owned(), control.to_owned())
    };
    ensure(
        run_until(Duration::from_secs(10), || {
            rows("known").len() == 2 && rows("others").len() == 3
        }),
        &format!(
            "  the networks didn't show: {:?} / {:?}",
            rows("known"),
            rows("others")
        ),
    )?;
    ensure(
        rows("known")
            == [
                row("Home", "Saved \u{b7} WPA2", "Connect"),
                row("Attic", "Not in range", ""),
            ],
        &format!("  known networks: {:?}", rows("known")),
    )?;
    let editor = sway_things_core::delegate::NM_CONNECTION_EDITOR
        .find()
        .is_some();
    let eduroam = if editor {
        row("eduroam", "Enterprise (802.1X)", "Set Up…")
    } else {
        row(
            "eduroam",
            "Enterprise (802.1X) \u{b7} needs nm-connection-editor",
            "",
        )
    };
    ensure(
        rows("others")
            == [
                eduroam,
                row("Office", "WPA2", "Connect"),
                row("Cafe", "Open, not encrypted", "Connect"),
            ],
        &format!("  other networks: {:?}", rows("others")),
    )?;
    ensure(
        rows("wired") == [row("Wired", "Off", "off")],
        &format!("  wired: {:?}", rows("wired")),
    )?;
    ensure(
        rows("vpns") == [row("Office VPN", "OpenVPN \u{b7} Off", "off")],
        &format!("  VPN: {:?}", rows("vpns")),
    )?;
    check_network_while_shown(&page, &stand_in, &wlan)?;

    page.row_button("Home")
        .ok_or("  no button for Home")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || {
            rows("known").first().is_some_and(|(_, status, button)| {
                status.starts_with("Connected") && button == "Disconnect"
            })
        }),
        &format!(
            "  Home didn't connect: {:?}; error: {}",
            rows("known"),
            page.error_text()
        ),
    )?;

    check_network_password(&page, &stand_in)?;
    check_network_agent(&page, &stand_in)?;

    check_network_wired_vpn(&page, &stand_in)?;

    page.wifi_switch().set_active(false);
    ensure(
        run_until(Duration::from_secs(5), || !page.wifi_switch().state()),
        "  Wi-Fi didn't turn off",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || !page.is_searching()),
        "  the page kept scanning with Wi-Fi off",
    )?;
    check_network_wifi_off(&page, &stand_in)?;
    window.destroy();
    Ok(())
}

/// The placeholder of `page` while Wi-Fi is off: it replaces the known and other networks, the
/// wired and VPN sections stay, a hardware block says so, and turning Wi-Fi back on with the
/// switch brings the networks back.
fn check_network_wifi_off(page: &NetworkPage, stand_in: &NmStandIn) -> Outcome {
    let texts = page
        .off_placeholder()
        .ok_or("  no placeholder while Wi-Fi is off")?;
    ensure(
        texts.first().map(String::as_str) == Some("Wi-Fi is off"),
        &format!("  placeholder: {texts:?}"),
    )?;
    ensure(
        !page
            .row_button("Home")
            .is_some_and(|button| button.is_drawable()),
        "  the known networks still show while Wi-Fi is off",
    )?;
    ensure(
        page.rows("wired").len() == 1 && page.rows("vpns").len() == 1,
        "  the wired and VPN sections went with Wi-Fi",
    )?;

    let hardware = |enabled: bool| {
        async_io::block_on(stand_in.update(
            NM_MANAGER,
            "org.freedesktop.NetworkManager",
            &[(
                "WirelessHardwareEnabled",
                zbus::zvariant::Value::from(enabled),
            )],
        ))
        .map_err(|error| format!("  {error}"))
    };
    hardware(false)?;
    ensure(
        run_until(Duration::from_secs(5), || {
            page.off_placeholder()
                .is_some_and(|texts| texts.iter().any(|text| text.contains("hardware switch")))
        }),
        &format!("  hardware block: {:?}", page.off_placeholder()),
    )?;
    hardware(true)?;
    ensure(
        run_until(Duration::from_secs(5), || page.wifi_switch().is_sensitive()),
        "  the switch stayed locked once the hardware block went",
    )?;
    page.wifi_switch().set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || {
            page.wifi_switch().state() && page.off_placeholder().is_none()
        }),
        "  turning Wi-Fi on didn't remove the placeholder",
    )?;
    Ok(())
}

/// The Network page as it scans by itself and as it scans only on request.
fn check_network_pages() -> Vec<(String, Outcome)> {
    vec![
        (
            "network: networks, passwords, wired, VPN and Wi-Fi".to_owned(),
            check_network(),
        ),
        (
            "network: scanning only with the Search button".to_owned(),
            check_network_manual(false),
        ),
        (
            "network: scanning only with the Search button (dark)".to_owned(),
            check_network_manual(true),
        ),
    ]
}

/// The Network page with scans only on request: no scan until _Search_, one after it, and the
/// search ends once NetworkManager has the new list.
fn check_network_manual(dark: bool) -> Outcome {
    let style = if dark { "dark" } else { "light" };
    let settings = gtk::Settings::default().ok_or("  no GTK settings")?;
    set_dark(&settings, dark);
    let (stand_in, wlan) = network_stand_in()?;
    let page = NetworkPage::new(&NetworkBus::Address(stand_in.address().to_owned()));
    page.set_manual_search(true);
    let window = gtk::Window::new();
    window.set_default_size(720, 900);
    window.set_child(Some(&page));
    window.present();
    settle(&window)?;
    let scans =
        || async_io::block_on(stand_in.calls(&wlan, "RequestScan")).map_or(0, |calls| calls.len());
    ensure(
        run_until(Duration::from_secs(10), || page.rows("others").len() == 3),
        "  the networks didn't show",
    )?;
    ensure(
        !run_until(Duration::from_secs(2), || scans() > 0),
        "  the page asked for a scan without Search",
    )?;
    ensure(!page.is_searching(), "  a spinner without a search")?;
    ensure(page.search_button().is_visible(), "  no Search button")?;
    shoot(&window, &format!("network-manual-idle-{style}"))?;
    page.search_button().emit_clicked();
    ensure(
        page.search_text() == "Stop" && page.is_searching(),
        "  no spinner and Stop while searching",
    )?;
    shoot(&window, &format!("network-manual-running-{style}"))?;
    ensure(
        run_until(Duration::from_secs(5), || scans() == 1),
        "  Search didn't ask for a scan",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || {
            page.search_text() == "Search" && !page.is_searching()
        }),
        "  the search didn't end with the new list",
    )?;
    ensure(scans() == 1, "  more than one scan for one Search")?;
    // Switched to automatic, the button goes and the page scans by itself.
    page.set_manual_search(false);
    ensure(
        !page.search_button().is_visible(),
        "  the Search button stayed in automatic mode",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || scans() > 1),
        "  automatic mode didn't scan",
    )?;
    window.destroy();
    set_dark(&settings, false);
    Ok(())
}

/// What `page` does while it is shown: it asks the Wi-Fi device `wlan` for scans, with the
/// spinner, and registers the agent.
fn check_network_while_shown(page: &NetworkPage, stand_in: &NmStandIn, wlan: &str) -> Outcome {
    let calls = |path: &str, method: &str| {
        async_io::block_on(stand_in.calls(path, method)).map_or(0, |calls| calls.len())
    };
    ensure(
        run_until(Duration::from_secs(5), || calls(wlan, "RequestScan") > 0),
        "  the page didn't ask for a scan while shown",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || page.is_searching()),
        "  no turning spinner while scanning",
    )?;
    ensure(
        !page.search_button().is_visible(),
        "  a Search button while the page scans by itself",
    )?;
    ensure(
        run_until(Duration::from_secs(5), || {
            calls(stand_in.agent_manager(), "Register") > 0
        }),
        "  the agent wasn't registered while the page is shown",
    )?;
    Ok(())
}

/// The wired device and the VPN on `page`, switched on, and the captive portal note.
fn check_network_wired_vpn(page: &NetworkPage, stand_in: &NmStandIn) -> Outcome {
    page.row_switch("Wired")
        .ok_or("  no switch for the wired device")?
        .set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || {
            page.rows("wired")
                .first()
                .is_some_and(|(_, status, on)| status.starts_with("Connected") && on == "on")
        }),
        &format!("  the cable didn't connect: {:?}", page.rows("wired")),
    )?;
    page.row_switch("Office VPN")
        .ok_or("  no switch for the VPN")?
        .set_active(true);
    ensure(
        run_until(Duration::from_secs(5), || {
            page.rows("vpns")
                .first()
                .is_some_and(|(_, status, on)| status == "OpenVPN \u{b7} Connected" && on == "on")
        }),
        &format!("  the VPN didn't connect: {:?}", page.rows("vpns")),
    )?;

    // Unplugged: nothing to switch, the row only says so.
    async_io::block_on(stand_in.update(
        "/org/freedesktop/NetworkManager/Devices/eth0",
        "org.freedesktop.NetworkManager.Device",
        &[("State", zbus::zvariant::Value::from(20u32))],
    ))
    .map_err(|error| format!("  {error}"))?;
    ensure(
        run_until(Duration::from_secs(5), || {
            page.rows("wired")
                == [(
                    "Wired".to_owned(),
                    "Cable unplugged".to_owned(),
                    String::new(),
                )]
        }),
        &format!("  the unplugged cable: {:?}", page.rows("wired")),
    )?;

    ensure(
        page.portal_note() == (false, false),
        "  a portal note without a portal",
    )?;
    async_io::block_on(async {
        stand_in
            .add_properties(
                NM_MANAGER,
                "org.freedesktop.NetworkManager",
                &[(
                    "ConnectivityCheckUri",
                    zbus::zvariant::Value::from("http://check.invalid/"),
                )],
            )
            .await?;
        stand_in
            .update(
                NM_MANAGER,
                "org.freedesktop.NetworkManager",
                &[("Connectivity", zbus::zvariant::Value::from(2u32))],
            )
            .await
    })
    .map_err(|error| format!("  {error}"))?;
    ensure(
        run_until(Duration::from_secs(5), || {
            page.portal_note() == (true, true)
        }),
        "  no note and login button behind a captive portal",
    )?;
    Ok(())
}

/// Connecting to the new network "Office" on `page`: the password dialog takes no short
/// password, and the one given goes to NetworkManager.
fn check_network_password(page: &NetworkPage, stand_in: &NmStandIn) -> Outcome {
    page.row_button("Office")
        .ok_or("  no button for Office")?
        .emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || page.open_dialog().is_some()),
        "  connecting to Office didn't ask for a password",
    )?;
    let dialog = page.open_dialog().ok_or("  no dialog")?;
    let entry = find::<gtk::PasswordEntry>(dialog.upcast_ref()).ok_or("  no password field")?;
    let connect =
        button_labeled(dialog.upcast_ref(), "Connect").ok_or("  the dialog has no Connect")?;
    entry.set_text("short");
    ensure(!connect.is_sensitive(), "  a short password was taken")?;
    entry.set_text("password1");
    ensure(connect.is_sensitive(), "  a good password wasn't taken")?;
    connect.emit_clicked();
    ensure(
        run_until(Duration::from_secs(5), || {
            page.rows("known")
                .iter()
                .any(|(name, status, _)| name == "Office" && status.starts_with("Connected"))
        }),
        &format!(
            "  Office didn't connect: {:?}; error: {}",
            page.rows("known"),
            page.error_text()
        ),
    )?;
    let calls = async_io::block_on(stand_in.calls(NM_MANAGER, "AddAndActivateConnection"))
        .map_err(|error| format!("  {error}"))?;
    let sent = format!("{calls:?}");
    ensure(
        sent.contains("password1"),
        &format!("  the password didn't reach NetworkManager: {sent}"),
    )?;
    ensure(page.open_dialog().is_none(), "  the dialog stayed open")?;
    Ok(())
}

/// The secret agent on `page`: NetworkManager asks again for the password of "Home", and the
/// dialog's answer goes back.
fn check_network_agent(page: &NetworkPage, stand_in: &NmStandIn) -> Outcome {
    let name = page.bus_name().ok_or("  the page has no bus name")?;
    let home = "/org/freedesktop/NetworkManager/Settings/home";
    // Allowed to ask, and the old password failed.
    async_io::block_on(stand_in.ask_secrets(&name, home, 0x1 | 0x2))
        .map_err(|error| format!("  {error}"))?;
    ensure(
        run_until(Duration::from_secs(5), || page.open_dialog().is_some()),
        "  the agent didn't ask for the password again",
    )?;
    let dialog = page.open_dialog().ok_or("  no dialog")?;
    ensure(
        dialog.title().as_deref() == Some("Wrong password for “Home”"),
        &format!("  the dialog is titled {:?}", dialog.title()),
    )?;
    find::<gtk::PasswordEntry>(dialog.upcast_ref())
        .ok_or("  no password field")?
        .set_text("new password");
    button_labeled(dialog.upcast_ref(), "Connect")
        .ok_or("  the dialog has no Connect")?
        .emit_clicked();
    let reply =
        async_io::block_on(stand_in.secrets_reply()).map_err(|error| format!("  {error}"))?;
    ensure(
        reply == Ok("new password".to_owned()),
        &format!("  the agent answered {reply:?}"),
    )?;
    ensure(page.open_dialog().is_none(), "  the dialog stayed open")?;
    Ok(())
}

/// The welcome screen: its spinner, looking around and back, closing once the line is there,
/// and a line that isn't last.
fn check_welcomes(settings: &gtk::Settings) -> Vec<(String, Outcome)> {
    vec![
        (
            "welcome: the spinner turns after coming back".to_owned(),
            check_spinner(settings),
        ),
        ("welcome: look around and back".to_owned(), check_welcome()),
        (
            "welcome: closes once the line is there".to_owned(),
            check_welcome_closes(),
        ),
        (
            "welcome: a line that isn't last".to_owned(),
            check_not_last(),
        ),
    ]
}

/// The lines below the search field and the back bar grow from the center as the list or page
/// below them scrolls, keep their height, and the one below the back bar shows only while
/// narrow.
fn check_scroll_lines() -> Outcome {
    let shoot = |window: &Window, name: &str| -> Outcome {
        if let Some(directory) = std::env::var_os("SWAY_THINGS_SCREENSHOTS") {
            check_screen(
                window,
                Some(PathBuf::from(directory).join(format!("{name}.png"))),
            )?;
        }
        Ok(())
    };
    let fixture = fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.set_default_size(1100, 300);
    window.present();
    settle(&window)?;
    let line = window.sidebar().line();
    let height = line.height();
    ensure(
        line.fraction() == 0.0 && line.line_width() == 0 && height >= 1,
        "  the line below the search field shows at the top, or takes no height",
    )?;
    let scroller = find::<gtk::ScrolledWindow>(window.sidebar().upcast_ref())
        .ok_or("  the sidebar has no scrolled window")?;
    let adjustment = scroller.vadjustment();
    ensure(
        adjustment.upper() - adjustment.page_size() > FULL_OFFSET,
        "  the page list doesn't scroll far enough to test",
    )?;
    shoot(&window, "line-sidebar-top")?;
    adjustment.set_value(FULL_OFFSET / 2.0);
    wait_for_paint(&window)?;
    shoot(&window, "line-sidebar-half")?;
    let half = line.line_width();
    ensure(
        (line.fraction() - 0.5).abs() < 0.01 && (half - line.width() / 2).abs() <= 1,
        &format!("  half way, the line is {half} of {} px wide", line.width()),
    )?;
    adjustment.set_value(FULL_OFFSET * 2.0);
    wait_for_paint(&window)?;
    shoot(&window, "line-sidebar-full")?;
    ensure(
        line.line_width() == line.width() && line.height() == height,
        "  scrolled past the offset, the line doesn't span the width at the same height",
    )?;
    adjustment.set_value(0.0);
    wait_for_paint(&window)?;
    ensure(
        line.line_width() == 0,
        &format!(
            "  back at the top, the line stays: {} px, fraction {}, offset {}",
            line.line_width(),
            line.fraction(),
            adjustment.value()
        ),
    )?;

    let back = window.back_line();
    ensure(
        !back.is_visible(),
        "  the line below the back bar shows while wide",
    )?;
    window.show_page(Page::Keyboard);
    set_narrow(&window, true)?;
    window.show_page(Page::Keyboard);
    wait_for_paint(&window)?;
    let view = page_view(&window).ok_or("  no page view")?;
    let page = find::<gtk::ScrolledWindow>(view.upcast_ref()).ok_or("  no page scroller")?;
    ensure(
        back.is_visible() && back.line_width() == 0 && back.height() >= 1,
        &format!(
            "  narrow at the top, the line below the back bar shows or takes no height: \
             visible {}, {} px wide, {} px high, offset {}",
            back.is_visible(),
            back.line_width(),
            back.height(),
            page.vadjustment().value()
        ),
    )?;
    shoot(&window, "line-back-top")?;
    page.vadjustment().set_value(FULL_OFFSET * 2.0);
    wait_for_paint(&window)?;
    shoot(&window, "line-back-full")?;
    ensure(
        back.line_width() == back.width(),
        "  the line below the back bar doesn't follow the page",
    )?;
    set_narrow(&window, false)?;
    ensure(
        !back.is_visible(),
        "  the line below the back bar stays while wide",
    )?;
    Ok(())
}

/// A desk: two monitors side by side, the left one with the focus, and a laptop panel that is
/// off.
const DESK_OUTPUTS: &str = r#"[
  {"name": "DP-4", "active": true, "primary": false, "make": "Samsung", "model": "LF27T850",
   "serial": "A1", "scale": 1.0, "transform": "normal", "focused": true,
   "modes": [{"width": 2560, "height": 1440, "refresh": 59951},
             {"width": 1920, "height": 1080, "refresh": 60000}],
   "current_mode": {"width": 2560, "height": 1440, "refresh": 59951},
   "rect": {"x": 0, "y": 0, "width": 2560, "height": 1440}},
  {"name": "DP-5", "active": true, "primary": false, "make": "Samsung", "model": "LF27T850",
   "serial": "B2", "scale": 1.0, "transform": "normal",
   "modes": [{"width": 2560, "height": 1440, "refresh": 59951}],
   "current_mode": {"width": 2560, "height": 1440, "refresh": 59951},
   "rect": {"x": 2560, "y": 0, "width": 2560, "height": 1440}},
  {"name": "eDP-1", "active": false, "primary": false, "make": "BOE", "model": "0x0BCA",
   "serial": "C3", "modes": [{"width": 1920, "height": 1080, "refresh": 60000}],
   "rect": {"x": 0, "y": 0, "width": 0, "height": 0}}
]"#;
const LEFT: &str = "Samsung LF27T850 A1";
const RIGHT: &str = "Samsung LF27T850 B2";
const PANEL: &str = "BOE 0x0BCA C3";

/// A model in the session of `sway`, once it has read the outputs.
fn displays_fixture(sway: &FakeSway, stored: &str) -> Result<Fixture, String> {
    let fixture = fixture_in(SWAY, stored, USERS, true, Some(sway))?;
    let known = run_until(Duration::from_secs(5), || fixture.model.outputs_known());
    ensure(known, "  the outputs weren't read within 5 s")?;
    Ok(fixture)
}

/// The arrangement and the output picker of the Displays page of `window`.
fn displays(window: &Window) -> Result<(Arrangement, OutputPicker), String> {
    let view = page_view(window).ok_or_else(|| "  no page".to_owned())?;
    let arrangement = view
        .arrangement()
        .ok_or_else(|| "  no arrangement".to_owned())?;
    let picker = view
        .output_picker()
        .ok_or_else(|| "  no output picker".to_owned())?;
    Ok((arrangement, picker))
}

fn check_displays() -> Vec<(String, Outcome)> {
    vec![
        (
            "displays: the outputs, the one with the focus picked".to_owned(),
            check_display_outputs(),
        ),
        (
            "displays: picking on the canvas points the rows at the output".to_owned(),
            check_display_pick(),
        ),
        (
            "displays: a moved output is settled, stored and undone".to_owned(),
            check_display_move(),
        ),
        (
            "displays: a gap is shown and closed".to_owned(),
            check_display_gap(),
        ),
        ("displays: plugged in".to_owned(), check_display_plugged()),
        (
            "displays: the last output can't be switched off".to_owned(),
            check_display_last(),
        ),
        (
            "displays: without a session".to_owned(),
            check_display_no_session(),
        ),
        (
            "displays: identify shows the numbers on the displays".to_owned(),
            check_display_identify(),
        ),
        (
            "contrast: displays with outputs (light)".to_owned(),
            check_display_contrast(false),
        ),
        (
            "contrast: displays with outputs (dark)".to_owned(),
            check_display_contrast(true),
        ),
    ]
}

/// The Displays page with the canvas, a tile picked and one that is off, checked like the
/// pages.
fn check_display_contrast(dark: bool) -> Outcome {
    let settings = gtk::Settings::default().ok_or_else(|| "  no GTK settings".to_owned())?;
    set_dark(&settings, dark);
    let sway = with_outputs(&[DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let scheme = if dark { "dark" } else { "light" };
    let screenshots = std::env::var_os("SWAY_THINGS_SCREENSHOTS").map(PathBuf::from);
    let outcome = check_page(&window, Page::Displays, screenshots.as_deref(), scheme);
    window.destroy();
    set_dark(&settings, false);
    outcome
}

/// The canvas shows the outputs that are on at their places and the one that is off, the picker
/// lists them all, and the output with the focus is picked for the rows.
fn check_display_outputs() -> Outcome {
    let sway = with_outputs(&[DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, picker) = displays(&window)?;
    let canvas = arrangement.canvas();
    let tiles: Vec<String> = canvas.tiles().into_iter().map(|(id, _)| id).collect();
    ensure(
        tiles == [LEFT, RIGHT, PANEL],
        &format!("  the canvas shows {tiles:?}"),
    )?;
    let layout: Vec<String> = canvas.layout().into_iter().map(|(id, _)| id).collect();
    ensure(
        layout == [LEFT, RIGHT],
        &format!("  the layout is {layout:?}"),
    )?;
    ensure(
        picker.labels()
            == [
                "All displays",
                "1  Samsung LF27T850 \u{b7} DP-4",
                "2  Samsung LF27T850 \u{b7} DP-5",
                "3  BOE 0x0BCA \u{b7} eDP-1 (off)",
            ],
        &format!("  the picker offers {:?}", picker.labels()),
    )?;
    ensure(
        picker.selected().as_deref() == Some(LEFT),
        &format!("  the picker picked {:?}", picker.selected()),
    )?;
    let scale = row(&window, "output.scale")?;
    ensure(
        scale.device().as_deref() == Some(LEFT),
        "the rows aren't for the output with the focus",
    )?;
    ensure(
        scale.note().as_deref() == Some("Chosen by sway"),
        &format!("  the scale row's note is {:?}", scale.note()),
    )?;
    ensure(!arrangement.warns_of_gaps(), "a gap is reported")?;
    window.destroy();
    Ok(())
}

/// A tile clicked on the canvas picks its output in the picker, points the rows at it and
/// offers its modes.
fn check_display_pick() -> Outcome {
    let sway = with_outputs(&[DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, picker) = displays(&window)?;
    let (_, right) = arrangement
        .canvas()
        .tiles()
        .into_iter()
        .find(|(id, _)| id == RIGHT)
        .ok_or_else(|| "  no tile for the right monitor".to_owned())?;
    right.set_active(true);
    settle(&window)?;
    ensure(
        picker.selected().as_deref() == Some(RIGHT),
        &format!("  the picker picked {:?}", picker.selected()),
    )?;
    let mode = row(&window, "output.mode")?;
    ensure(
        mode.device().as_deref() == Some(RIGHT),
        "the mode row isn't for the picked output",
    )?;
    let dropdown = mode
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::DropDown>().ok())
        .ok_or_else(|| "  the mode row has no drop-down".to_owned())?;
    let offered: Vec<String> = dropdown
        .model()
        .map(|model| {
            (0..model.n_items())
                .filter_map(|index| model.item(index).and_downcast::<gtk::StringObject>())
                .map(|item| item.string().to_string())
                .collect()
        })
        .unwrap_or_default();
    ensure(
        offered == ["Automatic", "2560 \u{d7} 1440 \u{b7} 59.95 Hz"],
        &format!("  the mode row offers {offered:?}"),
    )?;
    // The picker back to every output: no tile is pressed.
    picker.dropdown().set_selected(0);
    settle(&window)?;
    ensure(
        arrangement
            .canvas()
            .tiles()
            .iter()
            .all(|(_, tile)| !tile.is_active()),
        "a tile stays pressed for all displays",
    )?;
    ensure(
        row(&window, "output.mode")?.device().is_none(),
        "the rows aren't for all displays",
    )?;
    // Every output stays picked when the model changes.
    fixture.model.emit_by_name::<()>("changed", &[]);
    settle(&window)?;
    ensure(
        picker.selected().is_none(),
        &format!("  after a change the picker picked {:?}", picker.selected()),
    )?;
    window.destroy();
    Ok(())
}

/// The right monitor dropped below the left one, overlapping it, is settled below it; both
/// positions are stored and sent; Ctrl+Z puts them back.
fn check_display_move() -> Outcome {
    let sway = with_outputs(&[DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, _) = displays(&window)?;
    let canvas = arrangement.canvas();
    canvas.move_output(RIGHT, 300, 1000);
    wait_for_model(&fixture.model)?;
    let layout = canvas.layout();
    let place = |id: &str| {
        layout
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, rect)| (rect.x, rect.y))
    };
    ensure(
        place(LEFT) == Some((0, 0)) && place(RIGHT) == Some((300, 1440)),
        &format!("  the layout is {layout:?}"),
    )?;
    let stored = |id: &str| {
        Registry::builtin()
            .get("output.position")
            .and_then(|setting| fixture.model.view(setting, Some(id)))
            .map(|view| view.value)
    };
    ensure(
        stored(RIGHT) == Some(Value::Text("300,1440".to_owned()))
            && stored(LEFT) == Some(Value::Text("0,0".to_owned())),
        &format!("  stored {:?} and {:?}", stored(LEFT), stored(RIGHT)),
    )?;
    let commands = sway.commands();
    ensure(
        commands
            .iter()
            .any(|command| command == r#"output "Samsung LF27T850 B2" pos 300 1440"#),
        &format!("  sent {commands:?}"),
    )?;
    ensure(canvas.undo(), "nothing to undo")?;
    wait_for_model(&fixture.model)?;
    ensure(
        stored(RIGHT) == Some(Value::Text("2560,0".to_owned())),
        &format!("  after undo, stored {:?}", stored(RIGHT)),
    )?;
    ensure(!canvas.undo(), "more to undo than was done")?;
    window.destroy();
    Ok(())
}

/// Two monitors that don't touch are reported, and "Arrange Side by Side" puts them together.
fn check_display_gap() -> Outcome {
    let apart = DESK_OUTPUTS.replace(
        r#""rect": {"x": 2560, "y": 0, "width": 2560, "height": 1440}"#,
        r#""rect": {"x": 3000, "y": 0, "width": 2560, "height": 1440}"#,
    );
    let sway = with_outputs(&[&apart]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, _) = displays(&window)?;
    ensure(arrangement.warns_of_gaps(), "the gap isn't reported")?;
    let button = buttons_labeled(&window, "Arrange Side by Side")
        .into_iter()
        .next()
        .ok_or_else(|| "  no button to arrange".to_owned())?;
    button.emit_clicked();
    wait_for_model(&fixture.model)?;
    let layout = arrangement.canvas().layout();
    let right = layout
        .iter()
        .find(|(id, _)| id == RIGHT)
        .map(|(_, rect)| rect.x);
    ensure(right == Some(2560), &format!("  the layout is {layout:?}"))?;
    window.destroy();
    Ok(())
}

/// A monitor that is plugged in shows up on the canvas and in the picker.
fn check_display_plugged() -> Outcome {
    let one = r#"[{"name": "DP-4", "active": true, "primary": false, "make": "Samsung",
        "model": "LF27T850", "serial": "A1", "scale": 1.0, "focused": true, "modes": [],
        "rect": {"x": 0, "y": 0, "width": 2560, "height": 1440}}]"#;
    let sway = with_outputs(&[one, DESK_OUTPUTS]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, picker) = displays(&window)?;
    let plugged = run_until(Duration::from_secs(5), || {
        arrangement.canvas().tiles().len() == 3 && picker.labels().len() == 4
    });
    ensure(
        plugged,
        &format!("  the picker offers {:?}", picker.labels()),
    )?;
    ensure(
        picker.selected().as_deref() == Some(LEFT),
        "the picked output changed with the hotplug",
    )?;
    window.destroy();
    Ok(())
}

/// Switching off the only output that is on is refused, and the row says why.
fn check_display_last() -> Outcome {
    let single = DESK_OUTPUTS.replace(
        r#""name": "DP-5", "active": true"#,
        r#""name": "DP-5", "active": false"#,
    );
    let sway = with_outputs(&[&single]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let enabled = row(&window, "output.enabled")?;
    let switch = enabled
        .controls()
        .into_iter()
        .find_map(|control| control.downcast::<gtk::Switch>().ok())
        .ok_or_else(|| "  the row has no switch".to_owned())?;
    switch.set_active(false);
    let refused = run_until(Duration::from_secs(5), || {
        enabled
            .error()
            .is_some_and(|error| error.contains("last display"))
    });
    ensure(refused, &format!("  the row says {:?}", enabled.error()))?;
    ensure(
        fixture.model.pending_change().is_none(),
        "a change waits to be kept",
    )?;
    window.destroy();
    Ok(())
}

/// Without a session the canvas gives way to a hint, and the rows are still there.
fn check_display_no_session() -> Outcome {
    let fixture = fixture(SWAY, "version = 1\n", USERS, true)?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, _) = displays(&window)?;
    ensure(
        !arrangement.canvas().is_visible(),
        "the canvas shows without a session",
    )?;
    row(&window, "output.scale")?;
    window.destroy();
    Ok(())
}

/// "Identify" shows the number of an output on the display GDK knows by its name, on the overlay
/// layer, and takes it away again; the tiles are outlined meanwhile. The stand-in names its output
/// like the headless sway's own, so GDK has a monitor for it.
fn check_display_identify() -> Outcome {
    let headless = DESK_OUTPUTS.replacen(r#""name": "DP-4""#, r#""name": "HEADLESS-1""#, 1);
    let sway = with_outputs(&[&headless]);
    let fixture = displays_fixture(&sway, "version = 1\n")?;
    let window = open(&fixture, Page::Displays)?;
    let (arrangement, _) = displays(&window)?;
    ensure(
        arrangement.identify_button().is_sensitive(),
        "Identify can't be pressed on sway",
    )?;
    let numbers = || {
        gtk::Window::list_toplevels()
            .into_iter()
            .filter(|window| window.has_css_class("st-identify") && window.is_visible())
            .count()
    };
    let shown = arrangement.identify();
    ensure(shown == 1, &format!("  the number is on {shown} displays"))?;
    ensure(
        arrangement.identify() == 0 && !arrangement.identify_button().is_sensitive(),
        "Identify can be pressed again while the numbers are shown",
    )?;
    ensure(
        run_until(Duration::from_secs(2), || numbers() == 1),
        "no window with the number",
    )?;
    ensure(
        arrangement.canvas().has_css_class("st-identifying"),
        "the tiles aren't outlined",
    )?;
    ensure(
        run_until(Duration::from_secs(6), || numbers() == 0),
        "the number stays",
    )?;
    ensure(
        !arrangement.canvas().has_css_class("st-identifying"),
        "the tiles stay outlined",
    )?;
    ensure(
        arrangement.identify_button().is_sensitive(),
        "Identify can't be pressed again afterwards",
    )?;
    window.destroy();
    Ok(())
}
