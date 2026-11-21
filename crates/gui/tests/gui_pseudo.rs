// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window in the pseudo-locale made from `po/sway-things.pot`, where every translation swaps
//! each ASCII letter for an accented one: every page, the welcome screen, the read-only banner,
//! and Bluetooth and Network against their stand-ins (if python-dbusmock is there). Each text a
//! visible widget shows (labels, buttons, tooltips, placeholders and the items of drop-downs)
//! must have no English word left, unless it is data: a value, a device or network name, a name
//! from the XKB rules, a product's name, a line of sway config (shown in monospace).
//! Accessible labels can't be read back from GTK, so they aren't checked.
//!
//! It needs a display like `gui.rs`, so a plain `cargo test` skips it and
//! scripts/check-integration.sh runs it with `-- --ignored`. The catalogs are loaded once at
//! startup from the environment, which a program can't change for itself without `unsafe`: it
//! writes the catalog and runs itself again with the environment that picks it.

use std::fmt::Write as _;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::apply::Applier;
use sway_things_core::bluetooth::stand_in::StandIn;
use sway_things_core::generate::{Locations, SwayValidator};
use sway_things_core::i18n::{self, DOMAIN};
use sway_things_core::include::IncludeStatus;
use sway_things_core::ipc::Compositor;
use sway_things_core::look;
use sway_things_core::network::stand_in::{self as nm_stand_in, StandIn as NmStandIn};
use sway_things_core::registry::{Kind, Page, Registry};
use sway_things_core::sound::stand_in::StandIn as SoundStandIn;
use sway_things_core::state::{Paths, State};
use sway_things_core::sway_config::{Disk, MemoryFiles};
use sway_things_core::system::Details;
use sway_things_core::version::{SwayFxVersion, SwayVersion};
use sway_things_gui::model::Model;
use sway_things_gui::widgets::{BluetoothBus, NetworkBus, SoundServer};
use sway_things_gui::window::Window;
use sway_things_gui::{icons, style, xkb};

type Outcome = Result<(), String>;

/// Set in the run that checks, with the catalogs of the pseudo-locale.
const CHILD: &str = "SWAY_THINGS_PSEUDO_CHILD";

/// SwayFX, so that no setting is locked.
const SWAYFX: Compositor = Compositor::SwayFx {
    version: SwayFxVersion::OLDEST_SUPPORTED,
    base: SwayVersion::new(1, 11, 0),
};

/// Values stored in the settings file, with the line of the sway config shown in every row.
const STORED: &str = "version = 1\n\n[input.touchpad]\npointer_accel = 0.3\n\n\
                      [app]\nshow_directives = true\n";

/// Lines of the user's config before the include line, so rows say where a value comes from.
const USERS: &str = "gaps inner 6\nfocus_follows_mouse no\n";

/// Names of products, standards and units, which no translation changes.
const PRODUCTS: &[&str] = &[
    "Sway Things",
    "SwayFX",
    "sway",
    "GTK",
    "NetworkManager",
    "nm-connection-editor",
    "blueman-manager",
    "pwvucontrol",
    "pavucontrol",
    "PipeWire",
    "PulseAudio",
    "Blueman",
    "BlueZ",
    "XKB",
    "WPA2",
    "WPA3",
    "WEP",
    "VPN",
    "IPv4",
    "IPv6",
    "DNS",
    "Mb/s",
    "GHz",
    // GTK's own text for an empty drop-down, which GTK's catalogs translate.
    "(None)",
];

/// What the stand-ins call their devices, adapters and networks.
const STAND_IN_NAMES: &[&str] = &[
    "Headset",
    "Speaker",
    "laptop",
    "dongle",
    "hci0",
    "hci1",
    "Home",
    "Office",
    "Cafe",
    "wlan0",
    "Speakers",
    "HDMI",
    "Microphone",
    "Music",
    "Recorder",
    "pacat",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--list") {
        return ExitCode::SUCCESS;
    }
    if !args
        .iter()
        .any(|arg| arg == "--ignored" || arg == "--include-ignored")
    {
        println!("GUI pseudo-locale test skipped: it needs a display (run with `-- --ignored`)");
        return ExitCode::SUCCESS;
    }
    if std::env::var_os(CHILD).is_none() {
        return run_in_pseudo_locale(&args);
    }
    i18n::init();
    match check() {
        Ok(()) => {
            println!("test pseudo-locale: no English text ... ok");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            println!("test pseudo-locale: no English text ... FAILED\n{problems}");
            ExitCode::FAILURE
        }
    }
}

/// Writes the catalog of the pseudo-locale and runs this program again with it.
fn run_in_pseudo_locale(args: &[String]) -> ExitCode {
    let result = (|| -> Result<std::process::ExitStatus, String> {
        let locales = tempfile::tempdir().map_err(|error| error.to_string())?;
        let messages = locales.path().join("xx/LC_MESSAGES");
        std::fs::create_dir_all(&messages).map_err(|error| error.to_string())?;
        let pot = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../po/sway-things.pot"
        ))
        .map_err(|error| error.to_string())?;
        std::fs::write(
            messages.join(format!("{DOMAIN}.mo")),
            i18n::pseudo::catalog(&pot),
        )
        .map_err(|error| error.to_string())?;
        let program = std::env::current_exe().map_err(|error| error.to_string())?;
        Command::new(program)
            .args(args.iter().skip(1))
            .env(CHILD, "1")
            .env(i18n::LOCALEDIR_VAR, locales.path())
            .env("LANGUAGE", "")
            .env("LC_ALL", "xx_XX.UTF-8")
            .status()
            .map_err(|error| error.to_string())
    })();
    match result {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: can't run in the pseudo-locale: {error}");
            ExitCode::FAILURE
        }
    }
}

fn check() -> Outcome {
    gtk::init().map_err(|error| format!("can't initialize GTK: {error}"))?;
    style::load();
    icons::register();
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_enable_animations(false);
    }
    let mut data = data();
    let mut problems = String::new();

    // Without the include line: the welcome screen, then the pages with the banner.
    let read_only = Fixture::new(false)?;
    let window = Window::with_model(&read_only.model);
    window.present();
    settle(&window);
    problems.push_str(&english_in("welcome screen", window.upcast_ref(), &data));
    window.look_around();
    window.show_page(Page::MouseTouchpad);
    settle(&window);
    problems.push_str(&english_in("read-only banner", window.upcast_ref(), &data));
    window.destroy();

    // With it: every page, Bluetooth, Network and Sound against their stand-ins.
    let bluetooth = bluetooth_stand_in();
    let network = network_stand_in();
    let sound = sound_stand_in();
    let fixture = Fixture::new(true)?;
    let window = Window::with_model(&fixture.model);
    match &bluetooth {
        Ok(stand_in) => {
            window.use_bluetooth_bus(BluetoothBus::Address(stand_in.address().to_owned()));
        }
        Err(error) => println!("note: Bluetooth without its stand-in: {error}"),
    }
    match &network {
        Ok(stand_in) => window.use_network_bus(NetworkBus::Address(stand_in.address().to_owned())),
        Err(error) => println!("note: Network without its stand-in: {error}"),
    }
    match &sound {
        Ok(stand_in) => {
            window.use_sound_server(SoundServer::Address(stand_in.server()));
            // The server's name, which About shows, is data.
            let info = stand_in.pactl(&["info"]).unwrap_or_default();
            data.extend(
                info.lines()
                    .filter_map(|line| line.strip_prefix("Server Name: "))
                    .map(str::to_owned),
            );
            data.sort_by_key(|text| std::cmp::Reverse(text.len()));
        }
        Err(error) => println!("note: Sound without its stand-in: {error}"),
    }
    window.present();
    for page in Page::ALL {
        window.show_page(page);
        settle(&window);
        // The pages that ask D-Bus need a moment longer.
        if matches!(
            page,
            Page::Sound | Page::Bluetooth | Page::Network | Page::About
        ) {
            run_for(Duration::from_secs(2));
        }
        problems.push_str(&english_in(page.id(), window.upcast_ref(), &data));
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// A model on settings files in a temporary directory, with or without the include line.
struct Fixture {
    _dir: tempfile::TempDir,
    model: Model,
}

impl Fixture {
    fn new(include: bool) -> Result<Self, String> {
        let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
        let config = dir.path().join("config");
        std::fs::create_dir_all(&config).map_err(|error| error.to_string())?;
        std::fs::write(config.join("settings.toml"), STORED).map_err(|error| error.to_string())?;
        let paths = Paths::new(&config, &dir.path().join("state"), "host")
            .map_err(|error| error.to_string())?;
        let state = State::load(Registry::builtin(), paths);
        let generated = dir.path().join("sway/sway-things");
        let locations = Locations::new(&generated, &dir.path().join("state"));
        let main = dir.path().join("sway/config");
        let mut text = USERS.to_owned();
        if include {
            let _ = writeln!(text, "include {}/*.conf", generated.display());
        }
        let files = MemoryFiles::new().file(main.to_string_lossy().as_ref(), text.as_str());
        let status = IncludeStatus::detect(Some(&main), &files, &locations);
        let validator = SwayValidator::with_program("true");
        let applier = Applier::without_session(state, locations, validator, SWAYFX);
        Ok(Self {
            model: Model::with_applier(applier, status),
            _dir: dir,
        })
    }
}

/// The data a text may show untranslated: products, the stand-ins' names, the names of the
/// XKB rules in English and as shown, the themes and fonts of Apps Look & Feel, longest first.
fn data() -> Vec<String> {
    let rules = xkb::rules();
    let items = rules
        .models
        .iter()
        .chain(rules.layouts.iter().map(|layout| &layout.item))
        .chain(rules.layouts.iter().flat_map(|layout| &layout.variants))
        .chain(rules.option_groups.iter().map(|group| &group.item))
        .chain(rules.option_groups.iter().flat_map(|group| &group.options));
    let mut data: Vec<String> = PRODUCTS
        .iter()
        .chain(STAND_IN_NAMES)
        .map(|text| (*text).to_owned())
        .collect();
    for item in items {
        data.push(item.label().to_owned());
        data.push(item.shown().to_owned());
        data.extend(item.vendor.clone());
    }
    // The installed GTK and icon themes, and the default theme names and fonts of Apps Look &
    // Feel: values, not text.
    for theme in look::gtk_themes(&Disk)
        .into_iter()
        .chain(look::icon_themes(&Disk))
    {
        data.push(theme.name);
        data.push(theme.title);
    }
    for setting in Registry::builtin()
        .iter()
        .filter(|setting| setting.page() == Page::AppsLookFeel)
    {
        if let Kind::Text { default } = setting.kind() {
            data.push((*default).to_owned());
        }
    }
    // The names of waybar's modules on the Bar page: its default layout, with the names of
    // custom modules and instances (`custom/media`, `battery#bat2`) on their own too.
    for name in sway_things_core::registry::bar::DEFAULT_LEFT
        .iter()
        .chain(sway_things_core::registry::bar::DEFAULT_CENTER)
        .chain(sway_things_core::registry::bar::DEFAULT_RIGHT)
    {
        data.push((*name).to_owned());
        data.extend(name.strip_prefix("custom/").map(ToOwned::to_owned));
        data.extend(
            name.split_once('#')
                .map(|(_, instance)| instance.to_owned()),
        );
    }
    // The system details About shows: the distribution and the kernel of this machine.
    let system = async_io::block_on(Details::read(None, None, None, None));
    data.extend(system.distribution);
    data.extend(system.kernel);
    data.sort_by_key(|text| std::cmp::Reverse(text.len()));
    data.dedup();
    data
}

/// The texts of the visible widgets in `root` with an English word left once `data` is taken
/// out, one line each, under `screen`.
fn english_in(screen: &str, root: &gtk::Widget, data: &[String]) -> String {
    let mut texts = Vec::new();
    collect(root, &mut texts);
    let mut problems = String::new();
    for text in texts {
        let words: Vec<&str> = text
            .split_whitespace()
            .filter(|word| !is_data(word))
            .collect();
        let rest = data.iter().fold(words.join(" "), |rest, item| {
            rest.replace(item.as_str(), " ")
        });
        let english = rest
            .split(|c: char| !c.is_ascii_alphabetic())
            .any(|word| word.len() > 1);
        if english {
            let _ = writeln!(problems, "  {screen}: {text:?}");
        }
    }
    problems
}

/// Whether `word` is data by its form: a path, a link, a color, a generated file, or a
/// placeholder a text names on purpose (`{file}`).
fn is_data(word: &str) -> bool {
    let word = word.trim_matches(|c: char| ",.:;()⟦⟧`“”".contains(c));
    word.starts_with('/')
        || word.starts_with('~')
        || word.starts_with("http")
        || word.starts_with('#')
        || std::path::Path::new(word)
            .extension()
            .is_some_and(|extension| extension == "conf")
        || (word.starts_with('{') && word.ends_with('}'))
}

/// The texts `widget` and its visible children show.
fn collect(widget: &gtk::Widget, texts: &mut Vec<String>) {
    if !widget.is_visible() || !widget.is_child_visible() {
        return;
    }
    // A line of sway config, a path or a command: code, not text.
    if widget.has_css_class("monospace") {
        return;
    }
    let mut push = |text: Option<glib::GString>| {
        if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
            texts.push(text.to_string());
        }
    };
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        push(Some(label.text()));
    }
    push(widget.tooltip_text());
    if widget.find_property("placeholder-text").is_some() {
        push(widget.property::<Option<glib::GString>>("placeholder-text"));
    }
    if let Some(dropdown) = widget.downcast_ref::<gtk::DropDown>() {
        if let Some(list) = dropdown.model().and_downcast::<gtk::StringList>() {
            for index in 0..list.n_items() {
                push(list.string(index));
            }
        }
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        collect(&current, texts);
        child = current.next_sibling();
    }
}

/// Runs the main loop until `window` is shown, then a little longer for layout and the work
/// started on the way.
fn settle(window: &Window) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !(window.is_mapped() && window.width() > 0) && Instant::now() < deadline {
        run_for(Duration::from_millis(5));
    }
    run_for(Duration::from_millis(300));
}

/// Runs the main loop for `duration`, never blocking in it.
fn run_for(duration: Duration) {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// BlueZ's stand-in with an adapter, a paired headset and a speaker nearby.
fn bluetooth_stand_in() -> Result<StandIn, String> {
    async_io::block_on(async {
        let stand_in = StandIn::start().await?;
        stand_in.add_adapter("hci0", "laptop").await?;
        stand_in.add_adapter("hci1", "dongle").await?;
        stand_in
            .add_device("hci0", "00:11:22:33:44:55", "Headset")
            .await?;
        stand_in.pair_device("hci0", "00:11:22:33:44:55").await?;
        stand_in
            .add_device("hci0", "66:77:88:99:AA:BB", "Speaker")
            .await?;
        Ok::<_, sway_things_core::bluetooth::stand_in::Error>(stand_in)
    })
    .map_err(|error| format!("BlueZ's stand-in: {error}"))
}

/// A sound server with two outputs, a microphone, an app that plays and one that records.
fn sound_stand_in() -> Result<SoundStandIn, String> {
    let start = || {
        let mut stand_in = SoundStandIn::start()?;
        stand_in.add_output("speakers", "Speakers")?;
        stand_in.add_output("hdmi", "HDMI")?;
        stand_in.add_input("mic", "Microphone")?;
        stand_in.pactl(&["set-default-sink", "speakers"])?;
        stand_in.pactl(&["set-default-source", "mic"])?;
        stand_in.play("Music")?;
        stand_in.record("Recorder")?;
        Ok::<_, sway_things_core::sound::stand_in::Error>(stand_in)
    };
    start().map_err(|error| format!("the sound server's stand-in: {error}"))
}

/// NetworkManager's stand-in with a Wi-Fi adapter, networks in range and a saved one.
fn network_stand_in() -> Result<NmStandIn, String> {
    async_io::block_on(async {
        let stand_in = NmStandIn::start().await?;
        let wlan = stand_in.add_wifi_device("wlan0", "wlan0").await?;
        for (name, ssid, strength, security) in [
            ("home", "Home", 90, nm_stand_in::WPA2),
            ("office", "Office", 70, nm_stand_in::WPA2),
            ("cafe", "Cafe", 40, nm_stand_in::OPEN),
        ] {
            stand_in
                .add_access_point(&wlan, name, ssid, "00:11:22:33:44:55", strength, security)
                .await?;
        }
        stand_in.add_wifi_connection(&wlan, "home", "Home").await?;
        Ok::<_, nm_stand_in::Error>(stand_in)
    })
    .map_err(|error| format!("NetworkManager's stand-in: {error}"))
}
