// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Wallpaper page: the preview of the displays, the pictures of the wallpaper folder, the
//! way back to the default, and the notice about wallpaper tools that the user's config starts.

use super::*;

pub(crate) fn check_wallpaper() -> Vec<(String, Outcome)> {
    vec![
        (
            "wallpaper: a picture for one display".to_owned(),
            check_wallpaper_picker(),
        ),
        (
            "wallpaper: the config's PNG comes back by its tile, Use Default and Reset page"
                .to_owned(),
            check_wallpaper_default("png"),
        ),
        (
            "wallpaper: the config's WebP comes back, and says it can't be read".to_owned(),
            check_wallpaper_default("webp"),
        ),
        (
            "wallpaper: a tool of the config is managed externally".to_owned(),
            check_wallpaper_notice(),
        ),
    ]
}

/// The screen of a laptop, without a serial, so it is stored by its name.
pub(crate) const LAPTOP_SCREEN: &str = r#"[{"name": "eDP-1", "make": "BOE", "model": "0x0BCA",
  "serial": "Unknown", "active": true, "primary": false,
  "modes": [{"width": 1920, "height": 1080, "refresh": 60000}],
  "current_mode": {"width": 1920, "height": 1080, "refresh": 60000},
  "rect": {"x": 0, "y": 0, "width": 1920, "height": 1080}}]"#;

/// A wallpaper folder with two pictures, a file that isn't one, and a subfolder with a picture.
fn wallpaper_folder() -> Result<tempfile::TempDir, String> {
    let pictures = tempfile::tempdir().map_err(|error| error.to_string())?;
    for name in ["a.png", "b.jpg", "notes.txt"] {
        std::fs::write(pictures.path().join(name), b"").map_err(|error| error.to_string())?;
    }
    std::fs::create_dir(pictures.path().join("Holidays")).map_err(|error| error.to_string())?;
    std::fs::write(pictures.path().join("Holidays/c.webp"), b"")
        .map_err(|error| error.to_string())?;
    Ok(pictures)
}

/// The preview shows the screen of the session; switched to one display at a time, the tile of
/// the display is the one edited. The pictures of the wallpaper folder and its subfolders are
/// offered, and picking one sets it for that display and selects it.
fn check_wallpaper_picker() -> Outcome {
    let pictures = wallpaper_folder()?;
    let sway = with_outputs(&[LAPTOP_SCREEN]);
    let stored = format!(
        "{STORED}\n[app]\nwallpaper_folder = {:?}\n",
        pictures.path().display().to_string()
    );
    let fixture = displays_fixture(&sway, &stored)?;
    let window = open(&fixture, Page::Wallpaper)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let same = all::<gtk::Switch>(&content)
        .into_iter()
        .next()
        .ok_or_else(|| "  no switch for all displays".to_owned())?;
    ensure(
        same.is_active(),
        "  the displays start out edited one by one",
    )?;
    let tiles = || -> Vec<gtk::Button> {
        all::<gtk::Button>(&content)
            .into_iter()
            .filter(|button| button.has_css_class("st-wallpaper-tile"))
            .collect()
    };
    let shown = run_until(Duration::from_secs(5), || tiles().len() == 1);
    let names: Vec<String> = tiles()
        .iter()
        .filter_map(|tile| tile.tooltip_text().map(Into::into))
        .collect();
    ensure(
        shown && names == ["BOE 0x0BCA · eDP-1"],
        &format!("  the preview shows {names:?}"),
    )?;
    let pictures_shown = || pictures_of(&content);
    let loaded = run_until(Duration::from_secs(5), || pictures_shown().len() == 3);
    ensure(
        loaded,
        &format!("  {} pictures are offered", pictures_shown().len()),
    )?;
    let chips: Vec<String> = all::<gtk::ToggleButton>(&content)
        .into_iter()
        .filter(|chip| chip.has_css_class("st-wallpaper-chip"))
        .filter_map(|chip| chip.label().map(Into::into))
        .collect();
    ensure(
        chips == ["All", "Holidays"],
        &format!("  the chips are {chips:?}"),
    )?;

    same.set_active(false);
    settle(&window)?;
    ensure(
        tiles().iter().any(|tile| tile.has_css_class("st-editing")),
        "  the display isn't marked as edited",
    )?;
    let second = pictures_shown()
        .into_iter()
        .nth(1)
        .ok_or_else(|| "  no second picture".to_owned())?;
    second.emit_activate();
    wait_for_model(&fixture.model)?;
    let path = pictures.path().join("b.jpg");
    let expected = format!("output \"eDP-1\" bg \"'{}'\" fill #000000", path.display());
    ensure(
        sway.commands().last() == Some(&expected),
        &format!("  sent {:?}", sway.commands()),
    )?;
    let image = Registry::builtin()
        .get("wallpaper.image")
        .ok_or_else(|| "  no wallpaper image".to_owned())?;
    let view = fixture
        .model
        .view(image, Some("eDP-1"))
        .ok_or_else(|| "  no settings".to_owned())?;
    ensure(
        view.source
            == Source::Stored {
                layer: Layer::Host,
                for_type: false,
            },
        &format!("  the display's image comes from {:?}", view.source),
    )?;
    settle(&window)?;
    let selected = pictures_shown().into_iter().nth(1);
    ensure(
        selected.is_some_and(|picture| picture.is_selected()),
        "the picture in use isn't selected",
    )?;
    let title = all::<gtk::Label>(&content)
        .into_iter()
        .any(|label| label.has_css_class("st-wallpaper-title") && label.text() == "b.jpg");
    ensure(title, "  the picture in use isn't named")?;
    window.destroy();
    Ok(())
}

/// The pictures of the grid, after the _Default_ tile: their tiles hold a box with the picture
/// and its name.
fn pictures_of(content: &gtk::Widget) -> Vec<gtk::FlowBoxChild> {
    all::<gtk::FlowBoxChild>(content)
        .into_iter()
        .filter(|child| {
            child
                .child()
                .and_downcast::<gtk::Box>()
                .is_some_and(|tile| tile.widget_name() != DEFAULT_TILE)
        })
        .collect()
}

/// The widget name of the _Default_ tile.
const DEFAULT_TILE: &str = "st-wallpaper-default";

/// A picture of 16×10 pixels as PNG at `path`.
fn write_png(path: &Path) -> Outcome {
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 16, 10)
        .ok_or_else(|| "  no pixbuf".to_owned())?;
    pixbuf.fill(0x2855_77ff);
    pixbuf
        .savev(path, "png", &[])
        .map_err(|error| format!("  the PNG wasn't written: {error}"))
}

/// The user's config sets a picture outside the wallpaper folder (`extension`: a PNG that can be
/// read, or a WebP that can't). It shows as the default, selected, with where it comes from;
/// after another picture is picked, the _Default_ tile, _Use Default_ and _Reset page…_ each
/// bring it back and send the user's line.
fn check_wallpaper_default(extension: &str) -> Outcome {
    let pictures = wallpaper_folder()?;
    let backgrounds = tempfile::tempdir().map_err(|error| error.to_string())?;
    let original = backgrounds.path().join(format!("original.{extension}"));
    if extension == "png" {
        write_png(&original)?;
    } else {
        // No loader reads an empty file, like swaybg without the WebP loader.
        std::fs::write(&original, b"").map_err(|error| error.to_string())?;
    }
    let sway = with_outputs(&[LAPTOP_SCREEN]);
    let stored = format!(
        "{STORED}\n[app]\nwallpaper_folder = {:?}\n",
        pictures.path().display().to_string()
    );
    let users = format!("{USERS}output * bg {} fill #102030\n", original.display());
    let fixture = fixture_in(SWAY, &stored, &users, true, Some(&sway))?;
    let known = run_until(Duration::from_secs(5), || fixture.model.outputs_known());
    ensure(known, "  the outputs weren't read within 5 s")?;
    let window = open(&fixture, Page::Wallpaper)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let loaded = run_until(Duration::from_secs(5), || pictures_of(&content).len() == 3);
    ensure(loaded, "  the pictures weren't listed within 5 s")?;
    let page = DefaultPage {
        use_default: button_labeled(&content, "Use Default")
            .ok_or_else(|| "  no Use Default button".to_owned())?,
        reset_page: page_view(&window)
            .and_then(|view| view.reset_button().cloned())
            .ok_or_else(|| "  no Reset page button".to_owned())?,
        name: format!("original.{extension}"),
        users_line: format!("output * bg \"'{}'\" fill #102030", original.display()),
        window: &window,
        content,
        fixture: &fixture,
        sway: &sway,
    };

    page.shows_default("at first")?;
    // The warning that the picture can't be read comes once it was tried.
    let warned = run_until(Duration::from_secs(5), || {
        page.labeled("warning")
            .iter()
            .any(|text| text.starts_with(&format!("{} can’t be read", page.name)))
            == (extension == "webp")
    });
    ensure(
        warned,
        &format!("  the warning shows {:?}", page.labeled("warning")),
    )?;

    page.pick_another("tile")?;
    page.default_tile()
        .ok_or_else(|| "  no Default tile".to_owned())?
        .emit_activate();
    page.back("the Default tile")?;

    page.pick_another("button")?;
    page.use_default.emit_clicked();
    page.back("Use Default")?;

    page.pick_another("page")?;
    fixture.model.reset_page(Page::Wallpaper);
    page.back("Reset page")?;
    window.destroy();
    Ok(())
}

/// The Wallpaper page of [`check_wallpaper_default`], with the user's picture.
struct DefaultPage<'a> {
    window: &'a Window,
    content: gtk::Widget,
    fixture: &'a Fixture,
    sway: &'a FakeSway,
    use_default: gtk::Button,
    reset_page: gtk::Button,
    /// The file name of the user's picture.
    name: String,
    /// The command of the user's `bg` line.
    users_line: String,
}

impl DefaultPage<'_> {
    /// The _Default_ tile of the grid.
    fn default_tile(&self) -> Option<gtk::FlowBoxChild> {
        all::<gtk::FlowBoxChild>(&self.content)
            .into_iter()
            .find(|child| {
                child
                    .child()
                    .is_some_and(|tile| tile.widget_name() == DEFAULT_TILE)
            })
    }

    /// The texts of the labels shown with `class`.
    fn labeled(&self, class: &str) -> Vec<String> {
        all::<gtk::Label>(&self.content)
            .into_iter()
            .filter(|label| label.has_css_class(class) && label.is_drawable())
            .map(|label| label.text().into())
            .collect()
    }

    /// The user's picture shows, from their config, with the _Default_ tile selected.
    fn shows_default(&self, step: &str) -> Outcome {
        settle(self.window)?;
        let image = Registry::builtin()
            .get("wallpaper.image")
            .ok_or_else(|| "  no wallpaper image".to_owned())?;
        let source = self.fixture.model.view(image, None).map(|view| view.source);
        ensure(
            matches!(source, Some(Source::Config { .. })),
            &format!("  {step}: the image comes from {source:?}"),
        )?;
        let title = self.labeled("st-wallpaper-title");
        ensure(
            title == [self.name.as_str()],
            &format!("  {step}: the title is {title:?}"),
        )?;
        let origin = self
            .labeled("st-dim")
            .iter()
            .any(|text| text.starts_with("Default · from your config, "));
        ensure(
            origin,
            &format!("  {step}: nothing says where it comes from"),
        )?;
        ensure(
            self.default_tile().is_some_and(|tile| tile.is_selected())
                && !self.use_default.is_drawable(),
            &format!("  {step}: the Default tile isn't selected, or Use Default shows"),
        )?;
        ensure(
            !self.reset_page.is_sensitive(),
            &format!("  {step}: Reset page can be pressed with nothing stored"),
        )
    }

    /// Picks the second picture of the folder, b.jpg.
    fn pick_another(&self, step: &str) -> Outcome {
        let second = pictures_of(&self.content)
            .into_iter()
            .nth(1)
            .ok_or_else(|| "  no second picture".to_owned())?;
        second.emit_activate();
        wait_for_model(&self.fixture.model)?;
        settle(self.window)?;
        let title = self.labeled("st-wallpaper-title");
        ensure(
            title == ["b.jpg"] && self.use_default.is_drawable() && self.reset_page.is_sensitive(),
            &format!("  {step}: after picking b.jpg the title is {title:?}"),
        )?;
        ensure(
            !self.default_tile().is_some_and(|tile| tile.is_selected()),
            &format!("  {step}: the Default tile stays selected"),
        )
    }

    /// After `step`, the user's picture is back, and their line was sent.
    fn back(&self, step: &str) -> Outcome {
        wait_for_model(&self.fixture.model)?;
        self.shows_default(&format!("after {step}"))?;
        let last = self.sway.commands().last().cloned();
        ensure(
            last.as_ref() == Some(&self.users_line),
            &format!("  {step}: sent {last:?}"),
        )
    }
}

/// A wallpaper tool that the user's config starts is named with its line.
fn check_wallpaper_notice() -> Outcome {
    let users = format!("{USERS}exec swww-daemon\n");
    let fixture = fixture(SWAY, STORED, &users, true)?;
    let window = open(&fixture, Page::Wallpaper)?;
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    let notice = all::<gtk::Label>(&content)
        .into_iter()
        .find(|label| label.text().starts_with("Your config starts swww-daemon ("));
    ensure(
        notice.is_some_and(|label| label.is_drawable() && label.text().contains(":3)")),
        "  no notice about swww-daemon at line 3",
    )?;
    window.destroy();
    Ok(())
}
