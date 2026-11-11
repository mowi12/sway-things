// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! GUI tests: they need a display, so a plain `cargo test` skips them. scripts/check-integration.sh
//! runs them with `-- --ignored` against the headless sway of each CI container.
//!
//! - Contrast: the window is drawn with every page open, in GTK's own theme, light and dark, and
//!   every text in it must reach WCAG AA against what is drawn under it.
//! - Keyboard: the sidebar of the window is navigated and opened as the keyboard does it.
//!
//! GTK runs on one thread, so this is a plain program (`harness = false`), not `#[test]`s. With
//! `SWAY_THINGS_SCREENSHOTS=<dir>` it also saves the window with each page as a PNG there.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{glib, gsk};
use sway_things_core::registry::Page;
use sway_things_gui::contrast::{self, TextSample};
use sway_things_gui::style;
use sway_things_gui::window::Window;

/// The theme the window is checked in: the one GTK ships, so results don't depend on the machine.
const THEME: &str = "Default";

type Outcome = Result<(), String>;

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
    for dark in [false, true] {
        set_dark(&settings, dark);
        let scheme = if dark { "dark" } else { "light" };
        let window: Window = glib::Object::new();
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

/// Opens `page`, checks every text drawn in the window and saves a screenshot if asked.
fn check_page(window: &Window, page: Page, screenshots: Option<&Path>, scheme: &str) -> Outcome {
    window.show_page(page);
    settle(window)?;
    wait_for_paint(window)?;
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
    if let Some(directory) = screenshots {
        let path = directory.join(format!("{}--{scheme}.png", page.id()));
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
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
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
    let window: Window = glib::Object::new();
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
    let window: Window = glib::Object::new();
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
