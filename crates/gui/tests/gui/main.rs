// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! GUI tests: they need a display, so a plain `cargo test` skips them. scripts/check-integration.sh
//! runs them against the headless sway of the sway containers; scripts/gui-screenshots.sh runs
//! them on a private Broadway display.
//!
//! One module per area, each a group of checks that `main` runs in order:
//!
//! - `contrast_check`: every page, the welcome screen and the banners drawn in GTK's own theme,
//!   light and dark, reach WCAG AA, read from what GTK draws.
//! - `shell`: the sidebar from the keyboard, deep links, search, narrow mode, the scroll lines.
//! - `rows`, `safety_net`, `reload`, `sections`, `indicators`, `devices`, `editor`: the rows built
//!   from the registry and what they do.
//! - `generated`, `about`, `welcome`: the Generated config window, About & Maintenance, the welcome
//!   screen and the read-only banner.
//! - `keyboard`, `shortcuts`, `displays` (with the Workspaces map), `look`, `bar`, `bluetooth`,
//!   `sound`, `network`, `printers`, `region`, `environment`: the pages with parts of their own,
//!   the live ones against their stand-ins.
//! - `common`: the fixtures and helpers they share.
//! - `measured`: GTK's warnings that a layout gave a widget less than its minimum size; each group
//!   of checks fails on the ones it caused.
//!
//! The window works on settings files in a temporary directory and a sway config in memory,
//! without a session or in the session of the stand-in for sway. It never saves by itself, and
//! the files of a reload are validated with `true`, so nothing runs sway.
//!
//! GTK runs on one thread, so this is a plain program (`harness = false`), not `#[test]`s. With
//! `SWAY_THINGS_SCREENSHOTS=<dir>` it also saves the window with each page as a PNG there.

#![allow(
    clippy::wildcard_imports,
    reason = "the modules of this test program share its fixtures, helpers and imports, as one file did"
)]

mod about;
mod apps_autostart;
mod bar;
mod bluetooth;
mod common;
mod contrast_check;
mod devices;
mod displays;
mod editor;
mod environment;
mod generated;
mod indicators;
mod keyboard;
mod look;
mod measured;
mod network;
mod notifications;
mod place;
mod printers;
mod region;
mod reload;
mod rows;
mod safety_net;
mod scanners;
mod sections;
mod shell;
mod shortcuts;
mod sound;
mod swayosd;
mod wallpaper;
mod welcome;

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
use sway_things_core::gsettings::{GSettingsError, GSettingsStore, INTERFACE, MemoryGSettings};
use sway_things_core::include::IncludeStatus;
use sway_things_core::ipc::Compositor;
use sway_things_core::ipc::stand_in::{
    FakeSway, SWAY_1_10, obedient_with_inputs, with_inputs, with_outputs,
};
use sway_things_core::network::stand_in::{self as nm_stand_in, StandIn as NmStandIn};
use sway_things_core::printers::stand_in::{FakePrinter, StandIn as CupsStandIn};
use sway_things_core::region::Bus as RegionBus;
use sway_things_core::region::stand_in::StandIn as RegionStandIn;
use sway_things_core::registry::{ConfigFile, GSettingsKey, Link, Page, Registry, Value};
use sway_things_core::sound::stand_in::StandIn as SoundStandIn;
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
    Arrangement, BluetoothBus, BluetoothPage, DeviceSelector, FULL_OFFSET, GdkKeymap, GroupRow,
    LayoutList, NetworkBus, NetworkPage, OptionBrowser, OutputPicker, PageView, PrintService,
    PrintersPage, RegionTimePage, RowLayout, ScanDialog, ScanService, SettingRow, ShortcutEditor,
    SoundPage, SoundServer, TestArea,
};
use sway_things_gui::window::{NARROW_WIDTH, Window};

use self::about::*;
use self::apps_autostart::*;
use self::bar::*;
use self::bluetooth::*;
use self::common::*;
use self::contrast_check::*;
use self::devices::*;
use self::displays::*;
use self::editor::*;
use self::environment::*;
use self::generated::*;
use self::indicators::*;
use self::keyboard::*;
use self::look::*;
use self::network::*;
use self::notifications::*;
use self::place::*;
use self::printers::*;
use self::region::*;
use self::reload::*;
use self::rows::*;
use self::safety_net::*;
use self::scanners::*;
use self::sections::*;
use self::shell::*;
use self::shortcuts::*;
use self::sound::*;
use self::swayosd::*;
use self::wallpaper::*;
use self::welcome::*;

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
    measured::watch();
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
    // Likewise the Network page and NetworkManager, and the Sound page and the sound server.
    let network = network_stand_in().map(|(stand_in, _)| stand_in);
    let sound = sound_stand_in();
    // Likewise Printers & Scanners with CUPS, and SANE with its test backend.
    let printers = printers_stand_in(true);
    let sane = SaneFixture::new("test\n");
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
        if let Ok(sound) = &sound {
            window.use_sound_server(SoundServer::Address(sound.server()));
        }
        if let Ok(printers) = &printers {
            window.use_print_service(print_service(printers));
        }
        use_sane(&window, sane.as_ref().ok());
        window.present();
        let mut pages = Vec::new();
        for page in Page::ALL {
            let outcome = check_page(&window, page, screenshots.as_deref(), scheme);
            pages.push((format!("contrast: {} ({scheme})", page.id()), outcome));
        }
        window.destroy();
        results.extend(measured::checked(&format!("contrast ({scheme})"), pages));
        results.extend(measured::checked(
            &format!("screens ({scheme})"),
            check_screens(screenshots.as_deref(), scheme),
        ));
    }
    set_dark(&settings, false);
    results.extend(check_the_rest(&settings));
    report(&results)
}

/// The checks after the contrast of every page, group by group.
fn check_the_rest(settings: &gtk::Settings) -> Vec<(String, Outcome)> {
    let mut results: Vec<(String, Outcome)> = Vec::new();
    results.extend(measured::checked(
        "keyboard navigation",
        vec![
            ("keyboard: sidebar".to_owned(), check_sidebar_keyboard()),
            ("keyboard: Esc goes back".to_owned(), check_escape()),
        ],
    ));
    results.extend(measured::checked("links", check_links()));
    results.extend(measured::checked("searches", check_searches()));
    results.extend(measured::checked("narrow", check_narrow()));
    results.extend(measured::checked("setting rows", check_setting_rows()));
    results.extend(measured::checked("safety net", check_safety_net()));
    results.extend(measured::checked("reload", check_reload()));
    results.extend(measured::checked("generated", check_generated()));
    results.extend(measured::checked(
        "about",
        vec![(
            "about: the versions and the project's links".to_owned(),
            check_about(),
        )],
    ));
    results.extend(measured::checked("sections", check_sections()));
    results.extend(measured::checked(
        "theme",
        vec![(
            "theme: the accent follows sway's focus color".to_owned(),
            check_accent(),
        )],
    ));
    results.extend(measured::checked("devices", check_devices()));
    results.extend(measured::checked("displays", check_displays()));
    results.extend(measured::checked("wallpaper", check_wallpaper()));
    results.extend(measured::checked("bluetooth", check_bluetooth_pages()));
    results.extend(measured::checked("sound", check_sound_pages()));
    results.extend(measured::checked("network", check_network_pages()));
    results.extend(measured::checked("printers", check_printers_pages()));
    results.extend(measured::checked("region", check_region_pages()));
    results.extend(measured::checked("keyboard", check_keyboard()));
    results.extend(measured::checked("indicators", check_indicators()));
    results.extend(measured::checked("look", check_look()));
    results.extend(measured::checked("shortcuts", check_shortcuts()));
    results.extend(measured::checked("places", check_places()));
    results.extend(measured::checked("swayosd", check_swayosd()));
    results.extend(measured::checked("apps", check_apps()));
    results.extend(measured::checked("environment", check_environment()));
    results.extend(measured::checked("notifications", check_notifications()));
    results.extend(measured::checked("bar", check_bar_pages()));
    results.extend(measured::checked("welcome", check_welcomes(settings)));
    results
}

/// Prints each result as the test harness does and the summary; fails if one failed.
fn report(results: &[(String, Outcome)]) -> ExitCode {
    let mut failed = 0;
    for (name, outcome) in results {
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
