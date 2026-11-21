// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The fixtures and helpers every check uses: windows on settings in a temporary directory, waiting
//! for GTK, finding widgets.

use super::*;
/// The theme the window is checked in: the one GTK ships, so results don't depend on the machine.
pub(crate) const THEME: &str = "Default";

pub(crate) type Outcome = Result<(), String>;

/// SwayFX, so that no setting is locked.
pub(crate) const SWAYFX: Compositor = Compositor::SwayFx {
    version: SwayFxVersion::OLDEST_SUPPORTED,
    base: SwayVersion::new(1, 11, 0),
};
pub(crate) const SWAY: Compositor = Compositor::Sway {
    version: SwayVersion::new(1, 10, 1),
};
/// Values stored in the settings file, so rows show stored values too.
pub(crate) const STORED: &str = "version = 1\n\n[input.touchpad]\npointer_accel = 0.3\n\n\
                      [windows.focused]\nchild_border = \"#3b6e4f\"\n";
/// The preference that shows the line of the sway config in every row, so the contrast checks
/// cover it.
pub(crate) const DIRECTIVES: &str = "\n[app]\nshow_directives = true\n";
/// Lines of the user's config before the include line, so rows show where a value comes from.
pub(crate) const USERS: &str = "gaps inner 6\nfocus_follows_mouse no\n";

/// A model on settings files in a temporary directory, which goes away with it.
pub(crate) struct Fixture {
    pub(crate) _dir: tempfile::TempDir,
    pub(crate) model: Model,
    /// Where the collapsed sections are saved.
    pub(crate) ui: PathBuf,
    pub(crate) main: PathBuf,
    pub(crate) locations: Locations,
}

impl Fixture {
    /// The include line of the generated files.
    pub(crate) fn include_line(&self) -> String {
        let file = self.locations.path(ConfigFile::Input);
        let directory = file.parent().unwrap_or(Path::new("/"));
        format!("include {}/*.conf\n", directory.display())
    }

    /// The include status of a sway config with `users` and then, if `include`, the include line
    /// of the generated files.
    pub(crate) fn status(&self, users: &str, include: bool) -> IncludeStatus {
        let mut text = users.to_owned();
        if include {
            text.push_str(&self.include_line());
        }
        self.status_of(&text)
    }

    /// The include status of a sway config with `text`.
    pub(crate) fn status_of(&self, text: &str) -> IncludeStatus {
        let files = MemoryFiles::new().file(self.main.to_string_lossy().as_ref(), text);
        IncludeStatus::detect(Some(&self.main), &files, &self.locations)
    }
}

/// A model with `stored` in the settings file, for a sway config with `users` and then, if
/// `include`, the include line of the generated files.
pub(crate) fn fixture(
    compositor: Compositor,
    stored: &str,
    users: &str,
    include: bool,
) -> Result<Fixture, String> {
    fixture_in(compositor, stored, users, include, None)
}

/// A model like [`fixture`] with the include line, in the session of the stand-in `sway`, once
/// it has read the devices.
pub(crate) fn live_fixture(sway: &FakeSway, stored: &str) -> Result<Fixture, String> {
    let fixture = fixture_in(SWAY, stored, USERS, true, Some(sway))?;
    let known = run_until(Duration::from_secs(5), || fixture.model.devices_known());
    ensure(known, "  the devices weren't read within 5 s")?;
    Ok(fixture)
}

pub(crate) const KEYBOARD: &str = r#"{"identifier": "1:1:AT_Translated_Set_2_keyboard",
    "name": "AT Translated Set 2 keyboard", "type": "keyboard",
    "libinput": {"send_events": "enabled"}}"#;
pub(crate) const MOUSE_ID: &str = "1133:16511:Logitech_MX_Master_3";
pub(crate) const MOUSE: &str = r#"{"identifier": "1133:16511:Logitech_MX_Master_3", "name": "MX Master 3",
    "type": "pointer", "libinput": {"send_events": "enabled"}}"#;
pub(crate) const TOUCHPAD: &str = r#"{"identifier": "1739:52619:Synaptics_TM3276-022",
    "name": "Synaptics TM3276-022", "type": "touchpad",
    "libinput": {"send_events": "enabled"}}"#;

/// A laptop's sway with an external mouse: a keyboard, a mouse and a touchpad, and every
/// command succeeds. Switching off all mice leaves the touchpad.
pub(crate) fn laptop() -> FakeSway {
    obedient_with_inputs(SWAY_1_10, format!("[{KEYBOARD}, {MOUSE}, {TOUCHPAD}]"))
}

pub(crate) fn fixture_in(
    compositor: Compositor,
    stored: &str,
    users: &str,
    include: bool,
    sway: Option<&FakeSway>,
) -> Result<Fixture, String> {
    fixture_with_ui(compositor, stored, users, include, sway, None)
}

/// A model like [`fixture_in`] that starts with `ui` as `ui.toml`, the collapsed sections.
pub(crate) fn fixture_with_ui(
    compositor: Compositor,
    stored: &str,
    users: &str,
    include: bool,
    sway: Option<&FakeSway>,
    ui: Option<&str>,
) -> Result<Fixture, String> {
    fixture_with_gsettings(compositor, stored, users, include, sway, ui, None)
}

/// A model like [`fixture_with_ui`] that copies settings to `gsettings` and reads them there.
pub(crate) fn fixture_with_gsettings(
    compositor: Compositor,
    stored: &str,
    users: &str,
    include: bool,
    sway: Option<&FakeSway>,
    ui: Option<&str>,
    gsettings: Option<Box<dyn GSettingsStore>>,
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
    let mut applier = match sway {
        Some(sway) => Applier::with_session(state, locations, validator, sway.connect()),
        None => Applier::without_session(state, locations, validator, compositor),
    };
    if let Some(gsettings) = gsettings {
        applier.set_gsettings(gsettings);
    }
    fixture.model = Model::with_applier(applier, status);
    Ok(fixture)
}

/// Switches GTK between its light and dark style, as the desktop's preference would.
pub(crate) fn set_dark(settings: &gtk::Settings, dark: bool) {
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
pub(crate) fn run_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
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
pub(crate) fn settle(window: &impl IsA<gtk::Widget>) -> Outcome {
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
pub(crate) fn wait_for_paint(window: &impl IsA<gtk::Widget>) -> Outcome {
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

pub(crate) fn render(window: &gtk::Window) -> Result<gsk::RenderNode, String> {
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

/// The first widget of type `T` in `widget` and below it.
pub(crate) fn find<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Option<T> {
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

pub(crate) fn ensure(condition: bool, message: &str) -> Outcome {
    if condition {
        Ok(())
    } else {
        Err(format!("  {message}"))
    }
}

/// The page view open in `window`.
pub(crate) fn page_view(window: &Window) -> Option<PageView> {
    window
        .content()
        .and_then(|content| find::<PageView>(&content))
}

/// The row of the group `id` on the open page of `window`.
pub(crate) fn group(window: &Window, id: &str) -> Result<GroupRow, String> {
    page_view(window)
        .and_then(|view| view.group(id))
        .ok_or_else(|| format!("  no group {id}"))
}

/// Every widget of type `T` in `widget` and below it.
pub(crate) fn all<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Vec<T> {
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
pub(crate) fn rows(widget: &gtk::Widget) -> Vec<SettingRow> {
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
pub(crate) fn row(window: &Window, key: &str) -> Result<SettingRow, String> {
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    rows(&content)
        .into_iter()
        .find(|row| row.setting().is_some_and(|setting| setting.key() == key))
        .ok_or_else(|| format!("  no row for {key}"))
}

/// A window for `fixture` with `page` open.
pub(crate) fn open(fixture: &Fixture, page: Page) -> Result<Window, String> {
    let window = Window::with_model(&fixture.model);
    window.present();
    window.show_page(page);
    settle(&window)?;
    Ok(window)
}

/// Waits until the model has sent every change.
pub(crate) fn wait_for_model(model: &Model) -> Outcome {
    if run_until(Duration::from_secs(10), || !model.is_busy()) {
        Ok(())
    } else {
        Err("  the change didn't finish within 10 s".to_owned())
    }
}

/// The only widget of type `T` on the open page of `window`.
pub(crate) fn widget<T: IsA<gtk::Widget>>(window: &Window) -> Result<T, String> {
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    all::<T>(&content)
        .into_iter()
        .next()
        .ok_or_else(|| format!("  no {} on the page", T::static_type().name()))
}

/// Strings for a list value.
pub(crate) fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// The first button labeled `label` in `widget`.
pub(crate) fn button_labeled(widget: &gtk::Widget, label: &str) -> Option<gtk::Button> {
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

/// Saves `window` as `<name>.png` in `$SWAY_THINGS_SCREENSHOTS`, if set, once it is drawn, and
/// checks its contrast then.
pub(crate) fn shoot(window: &gtk::Window, name: &str) -> Outcome {
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
