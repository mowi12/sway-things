// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! GUI tests: they need a display, so a plain `cargo test` skips them. scripts/check-integration.sh
//! runs them with `-- --ignored` against the headless sway of each CI container.
//!
//! - Contrast: the window is drawn with every page open, in GTK's own theme, light and dark, and
//!   every text in it must reach WCAG AA against what is drawn under it.
//! - Keyboard: the sidebar of the window is navigated and opened as the keyboard does it.
//! - Rows: the pages built from the registry have a row per setting, a change reaches the
//!   settings, a locked setting can't be changed, and a failed change shows its error.
//!
//! The window works on settings files in a temporary directory and a sway config in memory,
//! without a session, and never saves, so nothing runs sway.
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
use sway_things_core::generate::{Locations, SwayValidator};
use sway_things_core::include::IncludeStatus;
use sway_things_core::ipc::Compositor;
use sway_things_core::registry::{Kind, Page, Registry, Value};
use sway_things_core::state::{Paths, State};
use sway_things_core::sway_config::MemoryFiles;
use sway_things_core::version::{SwayFxVersion, SwayVersion};
use sway_things_gui::contrast::{self, TextSample};
use sway_things_gui::model::Model;
use sway_things_gui::style;
use sway_things_gui::widgets::SettingRow;
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
/// Lines of the user's config before the include line, so rows show where a value comes from.
const USERS: &str = "gaps inner 6\nfocus_follows_mouse no\n";

/// A model on settings files in a temporary directory, which goes away with it.
struct Fixture {
    _dir: tempfile::TempDir,
    model: Model,
}

/// A model with `stored` in the settings file, for a sway config with `users` and then, if
/// `include`, the include line of the generated files.
fn fixture(
    compositor: Compositor,
    stored: &str,
    users: &str,
    include: bool,
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
    let main = dir.path().join("sway/config");
    let mut text = users.to_owned();
    if include {
        let _ = writeln!(text, "include {}/*.conf", generated.display());
    }
    let files = MemoryFiles::new().file(main.to_string_lossy().as_ref(), text);
    let status = IncludeStatus::detect(Some(&main), &files, &locations);
    let applier = Applier::without_session(state, locations, SwayValidator::new(), compositor);
    Ok(Fixture {
        _dir: dir,
        model: Model::with_applier(applier, status),
    })
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
    let screenshots = std::env::var_os("SWAY_THINGS_SCREENSHOTS").map(PathBuf::from);

    let mut results: Vec<(String, Outcome)> = Vec::new();
    let fixture = match fixture(SWAYFX, STORED, USERS, true) {
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
    }
    set_dark(&settings, false);
    results.push(("keyboard: sidebar".to_owned(), check_sidebar_keyboard()));
    results.push(("keyboard: Esc goes back".to_owned(), check_escape()));
    results.push(("rows: one per setting".to_owned(), check_rows()));
    results.push((
        "rows: a change reaches the settings".to_owned(),
        check_change(),
    ));
    results.push((
        "rows: a locked setting can't change".to_owned(),
        check_locked(),
    ));
    results.push(("rows: a failed change shows why".to_owned(), check_failed()));

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
    let node = render(window.upcast_ref())?;
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
    let mut problems = String::new();
    for sample in &samples {
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
            .filter(|setting| {
                setting.page() == page && !matches!(setting.kind(), Kind::List { .. })
            })
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

/// On vanilla sway, the SwayFX corner radius can't be changed, and says why.
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
    let gaps = row(&window, "windows.gaps.inner")?;
    ensure(
        gaps.controls()
            .iter()
            .all(gtk::prelude::WidgetExt::is_sensitive),
        "the gaps are locked",
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
