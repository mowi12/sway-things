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
//! - `keyboard`, `shortcuts`, `displays` (with the Workspaces map), `look`, `bluetooth`, `sound`,
//!   `network`: the pages with parts of their own, the live ones against their stand-ins.
//! - `common`: the fixtures and helpers they share.
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
mod bluetooth;
mod common;
mod contrast_check;
mod devices;
mod displays;
mod editor;
mod generated;
mod indicators;
mod keyboard;
mod look;
mod network;
mod place;
mod reload;
mod rows;
mod safety_net;
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
    LayoutList, NetworkBus, NetworkPage, OptionBrowser, OutputPicker, PageView, SettingRow,
    ShortcutEditor, SoundPage, SoundServer, TestArea,
};
use sway_things_gui::window::{NARROW_WIDTH, Window};

use self::about::*;
use self::apps_autostart::*;
use self::bluetooth::*;
use self::common::*;
use self::contrast_check::*;
use self::devices::*;
use self::displays::*;
use self::editor::*;
use self::generated::*;
use self::indicators::*;
use self::keyboard::*;
use self::look::*;
use self::network::*;
use self::place::*;
use self::reload::*;
use self::rows::*;
use self::safety_net::*;
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
    results.extend(check_wallpaper());
    results.extend(check_bluetooth_pages());
    results.extend(check_sound_pages());
    results.extend(check_network_pages());
    results.extend(check_keyboard());
    results.extend(check_indicators());
    results.extend(check_look());
    results.extend(check_shortcuts());
    results.extend(check_places());
    results.extend(check_swayosd());
    results.extend(check_apps());
    results.extend(check_welcomes(&settings));
    report(&results)
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
