// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The contrast check: what GTK draws must reach WCAG AA, read from the render nodes and pixels.

use super::*;
/// The screens beyond the pages, checked for contrast in the color `scheme`.
pub(crate) fn check_screens(screenshots: Option<&Path>, scheme: &str) -> Vec<(String, Outcome)> {
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
    results.push((
        format!("contrast: keyboard shortcuts with bindings ({scheme})"),
        check_shortcut_screen(screenshots, scheme),
    ));
    results
}

/// Opens `page`, checks every text drawn in the window and saves a screenshot if asked. A page
/// longer than the window is scrolled through, a window height at a time.
pub(crate) fn check_page(
    window: &Window,
    page: Page,
    screenshots: Option<&Path>,
    scheme: &str,
) -> Outcome {
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
pub(crate) fn check_screen(
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

pub(crate) fn problem(sample: &TextSample) -> Option<String> {
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

pub(crate) fn save_png(texture: &gtk::gdk::Texture, path: &Path) -> Outcome {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    texture
        .save_to_png(path)
        .map_err(|error| format!("can't save {}: {error}", path.display()))
}

/// Where the insensitive widgets below `widget` are, in the coordinates of `window`: the topmost
/// of each insensitive part of the tree.
pub(crate) fn insensitive_areas(
    widget: &gtk::Widget,
    window: &gtk::Widget,
) -> Vec<gtk::graphene::Rect> {
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
pub(crate) fn pixels_of(window: &Window, widget: &gtk::Widget) -> Result<Vec<u8>, String> {
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
pub(crate) fn spinner_turns(window: &Window) -> Result<bool, String> {
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
pub(crate) fn check_spinner(settings: &gtk::Settings) -> Outcome {
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
pub(crate) fn render_drawn(window: &impl IsA<gtk::Window>) -> Result<gsk::RenderNode, String> {
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
