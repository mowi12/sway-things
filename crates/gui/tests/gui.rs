// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! GUI tests: they need a display, so a plain `cargo test` skips them. scripts/check-integration.sh
//! runs them with `-- --ignored` against the headless sway of each CI container.
//!
//! - Contrast: the window is drawn with every page open, in GTK's own theme, light and dark, and
//!   every text in it must reach WCAG AA against what is drawn under it.
//! - Keyboard: the sidebar of the window is navigated and opened as the keyboard does it.
//! - Deep links: a link opens its page, and with a setting, focuses and tints its row.
//! - Rows: the pages built from the registry have a row per setting, a change reaches the
//!   settings, a locked setting can't be changed, a failed change shows its error, and the line
//!   of the sway config shows while its preference is on.
//! - Safety net: a risky change shows the Keep / Revert banner and is stored only when kept.
//! - Indicators: reset, this machine only, the badges of a conflict with the user's config and
//!   its ⋮ menu, the badge of a requirement, and the reset of a whole page.
//! - Devices: the device selectors list the devices of their type, a row changes the value of
//!   the device picked, a type without devices shows a hint, and devices come and go live.
//!
//! The window works on settings files in a temporary directory and a sway config in memory,
//! without a session or in the session of the stand-in for sway, and never saves, so nothing
//! runs sway.
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
use sway_things_core::command::input::InputType;
use sway_things_core::generate::{Locations, SwayValidator};
use sway_things_core::include::IncludeStatus;
use sway_things_core::ipc::Compositor;
use sway_things_core::ipc::stand_in::{FakeSway, SWAY_1_10, obedient_with_inputs, with_inputs};
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
use sway_things_gui::widgets::{DeviceSelector, PageView, SettingRow, TestArea};
use sway_things_gui::window::Window;

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
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let config = dir.path().join("config");
    std::fs::create_dir_all(&config).map_err(|error| error.to_string())?;
    std::fs::write(config.join("settings.toml"), stored).map_err(|error| error.to_string())?;
    let paths = Paths::new(&config, &dir.path().join("state"), "host")
        .map_err(|error| error.to_string())?;
    let state = State::load(Registry::builtin(), paths);
    let generated = dir.path().join("sway/sway-things");
    let locations = Locations::new(&generated, &dir.path().join("state"));
    let mut fixture = Fixture {
        model: Model::default(),
        main: dir.path().join("sway/config"),
        locations: locations.clone(),
        _dir: dir,
    };
    let status = fixture.status(users, include);
    let applier = match sway {
        Some(sway) => Applier::with_session(state, locations, SwayValidator::new(), sway.connect()),
        None => Applier::without_session(state, locations, SwayValidator::new(), compositor),
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
    results.push(("deep links: page and setting".to_owned(), check_open_link()));
    results.push((
        "deep links: a hidden row only opens the page".to_owned(),
        check_open_hidden_link(),
    ));
    results.extend(check_setting_rows());
    results.extend(check_safety_net());
    results.push((
        "theme: the accent follows sway's focus color".to_owned(),
        check_accent(),
    ));
    results.extend(check_devices());
    results.extend(check_indicators());
    results.push((
        "welcome: the spinner turns after coming back".to_owned(),
        check_spinner(&settings),
    ));
    results.push(("welcome: look around and back".to_owned(), check_welcome()));
    results.push((
        "welcome: closes once the line is there".to_owned(),
        check_welcome_closes(),
    ));
    results.push((
        "welcome: a line that isn't last".to_owned(),
        check_not_last(),
    ));

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
        format!("contrast: indicators ({scheme})"),
        check_indicators_screen(screenshots, scheme),
    ));
    results.push((
        format!("contrast: a type without devices ({scheme})"),
        check_no_devices_screen(screenshots, scheme),
    ));
    results
}

/// The checks of the rows built from the registry.
fn check_setting_rows() -> Vec<(String, Outcome)> {
    vec![
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
            "rows: the directive line follows the preference".to_owned(),
            check_directives(),
        ),
    ]
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
fn check_screen(window: &Window, screenshot: Option<PathBuf>) -> Result<String, String> {
    let node = render_drawn(window)?;
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
        text.contains("1 of your config lines come after them"),
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
fn render_drawn(window: &Window) -> Result<gsk::RenderNode, String> {
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
