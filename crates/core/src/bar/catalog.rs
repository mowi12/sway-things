// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The common waybar modules: what each one shows, whether it works on this machine, and the
//! options the page's form offers for it.
//!
//! Options and their types follow waybar's man pages (`waybar-<module>(5)`, 0.16). A module's
//! options that aren't listed here stay as they are in its definition and are shown as JSON. The
//! format strings get a preview ([`preview`]) with sample values for the placeholders a module
//! fills in.

use std::fmt::Write as _;
use std::path::Path;

use crate::i18n::Msg;
use crate::msg;

/// The type of an option's value, as waybar reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// A format string with `{placeholders}` and Pango markup.
    Format,
    /// Any text: a device, a path, a time zone.
    Text,
    /// A command waybar runs with `sh -c` on a click or to get the module's text. The user's own
    /// data: Sway Things stores and shows it, never runs it.
    Command,
    /// A whole number within a range.
    Integer {
        /// The smallest value.
        min: i64,
        /// The largest value.
        max: i64,
    },
    /// A decimal number within a range.
    Decimal {
        /// The smallest value.
        min: f64,
        /// The largest value.
        max: f64,
    },
    /// On or off.
    Bool,
    /// One of some words.
    Choice(&'static [&'static str]),
}

/// An option of a module the form offers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opt {
    /// The key in the module's definition, e.g. `format-alt`.
    pub key: &'static str,
    /// The label of its field.
    pub title: Msg,
    /// The type of its value.
    pub kind: Kind,
}

const fn opt(key: &'static str, title: Msg, kind: Kind) -> Opt {
    Opt { key, title, kind }
}

/// What a module needs to work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Nothing beyond waybar.
    Nothing,
    /// A battery (`/sys/class/power_supply/*/type` is `Battery`).
    Battery,
    /// A backlight that can be dimmed (`/sys/class/backlight/*`).
    Backlight,
    /// A Bluetooth adapter (`/sys/class/bluetooth/*`).
    Bluetooth,
    /// A thermal zone (`/sys/class/thermal/thermal_zone*`).
    Thermal,
    /// A program in `$PATH`.
    Program(&'static str),
    /// A sound server that speaks the PulseAudio protocol (pipewire-pulse or PulseAudio).
    Pulse,
    /// power-profiles-daemon or tuned-ppd on the system bus.
    PowerProfiles,
}

/// Whether a module works here, as far as can be told without running it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Works {
    /// What it needs is there.
    Yes,
    /// What it needs isn't there: it would stay empty or hidden.
    No(Need),
}

impl Need {
    /// Whether what is needed is there, looking at the system's files and `$PATH`.
    pub fn check(self) -> Works {
        self.check_in(Path::new("/"), std::env::var_os("PATH").as_deref())
    }

    /// [`Need::check`] with the system's files under `root` and `$PATH` given, for tests.
    ///
    /// # Blocking
    ///
    /// Reads the directories under `root` and `$PATH`.
    pub fn check_in(self, root: &Path, path: Option<&std::ffi::OsStr>) -> Works {
        let any_in = |dir: &str, test: &dyn Fn(&Path) -> bool| {
            std::fs::read_dir(root.join(dir)).is_ok_and(|entries| {
                entries
                    .filter_map(Result::ok)
                    .any(|entry| test(&entry.path()))
            })
        };
        let there = match self {
            Self::Nothing => true,
            Self::Battery => any_in("sys/class/power_supply", &|entry| {
                std::fs::read_to_string(entry.join("type"))
                    .is_ok_and(|kind| kind.trim() == "Battery")
            }),
            Self::Backlight => any_in("sys/class/backlight", &|_| true),
            Self::Bluetooth => any_in("sys/class/bluetooth", &|_| true),
            Self::Thermal => any_in("sys/class/thermal", &|entry| {
                entry
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("thermal_zone"))
            }),
            Self::Program(name) => crate::delegate::find_program(name, path).is_some(),
            Self::Pulse => ["pipewire-pulse", "pulseaudio"]
                .iter()
                .any(|name| crate::delegate::find_program(name, path).is_some()),
            Self::PowerProfiles => [
                "usr/share/dbus-1/system-services/net.hadess.PowerProfiles.service",
                "usr/share/dbus-1/system-services/org.freedesktop.UPower.PowerProfiles.service",
            ]
            .iter()
            .any(|file| root.join(file).exists()),
        };
        if there { Works::Yes } else { Works::No(self) }
    }

    /// What is missing, to show.
    pub fn missing(self) -> Msg {
        match self {
            Self::Nothing => msg!("Nothing is missing"),
            Self::Battery => msg!("No battery here"),
            Self::Backlight => msg!("No backlight here that can be dimmed"),
            Self::Bluetooth => msg!("No Bluetooth adapter here"),
            Self::Thermal => msg!("No temperature sensor here"),
            Self::Program(_) => msg!("Needs a program that isn't installed"),
            Self::Pulse => msg!("Needs PipeWire's pipewire-pulse or PulseAudio"),
            Self::PowerProfiles => msg!("Needs power-profiles-daemon or tuned-ppd"),
        }
    }
}

/// A module of the catalog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Module {
    /// Its name in the module lists, e.g. `clock`; `custom` and `group` stand for
    /// `custom/<name>` and `group/<name>`.
    pub name: &'static str,
    /// Its name to show.
    pub title: Msg,
    /// What it shows, in a few words.
    pub description: Msg,
    /// What it needs.
    pub need: Need,
    /// The options of its form, in order.
    pub options: &'static [Opt],
    /// The placeholders its format strings fill in, with sample values for the preview.
    pub samples: &'static [(&'static str, &'static str)],
}

const FORMAT: Opt = opt("format", msg!("Format"), Kind::Format);
const FORMAT_ALT: Opt = opt("format-alt", msg!("Format after a click"), Kind::Format);
const TOOLTIP: Opt = opt("tooltip", msg!("Tooltip"), Kind::Bool);
const TOOLTIP_FORMAT: Opt = opt("tooltip-format", msg!("Tooltip format"), Kind::Format);
const INTERVAL: Opt = opt(
    "interval",
    msg!("Update every (seconds)"),
    Kind::Integer {
        min: 1,
        max: 86_400,
    },
);
const MAX_LENGTH: Opt = opt(
    "max-length",
    msg!("Longest text (characters)"),
    Kind::Integer { min: 0, max: 1000 },
);
const ON_CLICK: Opt = opt("on-click", msg!("On click"), Kind::Command);
const ON_CLICK_RIGHT: Opt = opt("on-click-right", msg!("On right click"), Kind::Command);
const ON_CLICK_MIDDLE: Opt = opt("on-click-middle", msg!("On middle click"), Kind::Command);
const ON_SCROLL_UP: Opt = opt("on-scroll-up", msg!("On scroll up"), Kind::Command);
const ON_SCROLL_DOWN: Opt = opt("on-scroll-down", msg!("On scroll down"), Kind::Command);

/// The modules of the catalog, in the order the page lists them.
pub const MODULES: &[Module] = &[
    Module {
        name: "clock",
        title: msg!("Clock"),
        description: msg!("The time and date, with a calendar as tooltip"),
        need: Need::Nothing,
        options: &[
            FORMAT,
            FORMAT_ALT,
            TOOLTIP_FORMAT,
            opt("timezone", msg!("Time zone"), Kind::Text),
            opt("locale", msg!("Locale"), Kind::Text),
            INTERVAL,
            ON_CLICK,
        ],
        samples: &[("calendar", "▦")],
    },
    Module {
        name: "battery",
        title: msg!("Battery"),
        description: msg!("Charge, time left and whether it charges"),
        need: Need::Battery,
        options: &[
            opt("bat", msg!("Battery"), Kind::Text),
            FORMAT,
            opt(
                "format-charging",
                msg!("Format while charging"),
                Kind::Format,
            ),
            opt(
                "format-plugged",
                msg!("Format while plugged in"),
                Kind::Format,
            ),
            opt("format-full", msg!("Format when full"), Kind::Format),
            FORMAT_ALT,
            TOOLTIP_FORMAT,
            INTERVAL,
            ON_CLICK,
        ],
        samples: &[
            ("capacity", "76"),
            ("icon", ""),
            ("time", "3 h 12 min"),
            ("timeTo", "3 h 12 min until empty"),
            ("power", "8.2"),
        ],
    },
    Module {
        name: "network",
        title: msg!("Network"),
        description: msg!("Wi-Fi or cable, address and speed"),
        need: Need::Nothing,
        options: &[
            opt("interface", msg!("Network interface"), Kind::Text),
            opt("format-wifi", msg!("Format on Wi-Fi"), Kind::Format),
            opt("format-ethernet", msg!("Format on a cable"), Kind::Format),
            opt(
                "format-disconnected",
                msg!("Format when offline"),
                Kind::Format,
            ),
            FORMAT_ALT,
            TOOLTIP_FORMAT,
            INTERVAL,
            ON_CLICK,
            ON_CLICK_RIGHT,
        ],
        samples: &[
            ("essid", "Home"),
            ("signalStrength", "71"),
            ("ifname", "wlan0"),
            ("ipaddr", "192.168.1.20"),
            ("cidr", "24"),
            ("gwaddr", "192.168.1.1"),
            ("bandwidthDownBits", "1.2 Mb/s"),
            ("bandwidthUpBits", "80 kb/s"),
            ("icon", ""),
        ],
    },
    Module {
        name: "pulseaudio",
        title: msg!("Volume (PulseAudio)"),
        description: msg!("Volume and mute of the default output, and the microphone"),
        need: Need::Pulse,
        options: &[
            FORMAT,
            opt("format-muted", msg!("Format when muted"), Kind::Format),
            opt(
                "format-bluetooth",
                msg!("Format on Bluetooth"),
                Kind::Format,
            ),
            opt("format-source", msg!("Microphone format"), Kind::Format),
            opt(
                "format-source-muted",
                msg!("Microphone format when muted"),
                Kind::Format,
            ),
            opt(
                "scroll-step",
                msg!("Scroll step (percent)"),
                Kind::Decimal {
                    min: 0.0,
                    max: 100.0,
                },
            ),
            TOOLTIP_FORMAT,
            ON_CLICK,
            ON_CLICK_RIGHT,
        ],
        samples: &[
            ("volume", "64"),
            ("icon", ""),
            // The microphone, which is quiet in the sample.
            ("format_source", ""),
            ("desc", "Speakers"),
        ],
    },
    Module {
        name: "wireplumber",
        title: msg!("Volume (WirePlumber)"),
        description: msg!("Volume and mute of the default output, from WirePlumber"),
        need: Need::Program("wireplumber"),
        options: &[
            FORMAT,
            opt("format-muted", msg!("Format when muted"), Kind::Format),
            opt(
                "scroll-step",
                msg!("Scroll step (percent)"),
                Kind::Decimal {
                    min: 0.0,
                    max: 100.0,
                },
            ),
            TOOLTIP_FORMAT,
            ON_CLICK,
            ON_CLICK_RIGHT,
        ],
        samples: &[("volume", "64"), ("icon", ""), ("node_name", "Speakers")],
    },
    Module {
        name: "tray",
        title: msg!("Tray"),
        description: msg!("The icons of running apps (StatusNotifierItem)"),
        need: Need::Nothing,
        options: &[
            opt(
                "icon-size",
                msg!("Icon size"),
                Kind::Integer { min: 8, max: 128 },
            ),
            opt(
                "spacing",
                msg!("Space between icons"),
                Kind::Integer { min: 0, max: 64 },
            ),
        ],
        samples: &[],
    },
    Module {
        name: "cpu",
        title: msg!("CPU"),
        description: msg!("How busy the processor is"),
        need: Need::Nothing,
        options: &[FORMAT, TOOLTIP, INTERVAL, ON_CLICK],
        samples: &[("usage", "12"), ("load", "0.42"), ("avg_frequency", "2.1")],
    },
    Module {
        name: "memory",
        title: msg!("Memory"),
        description: msg!("How much memory is in use"),
        need: Need::Nothing,
        options: &[FORMAT, TOOLTIP_FORMAT, INTERVAL, ON_CLICK],
        samples: &[
            ("", "38"),
            ("percentage", "38"),
            ("used", "6.1"),
            ("total", "16"),
        ],
    },
    Module {
        name: "disk",
        title: msg!("Disk"),
        description: msg!("Free space of a file system"),
        need: Need::Nothing,
        options: &[
            opt("path", msg!("Mount point"), Kind::Text),
            FORMAT,
            TOOLTIP_FORMAT,
            INTERVAL,
            ON_CLICK,
        ],
        samples: &[("percentage_used", "54"), ("free", "210 GB"), ("path", "/")],
    },
    Module {
        name: "temperature",
        title: msg!("Temperature"),
        description: msg!("A temperature sensor, such as the processor's"),
        need: Need::Thermal,
        options: &[
            opt(
                "thermal-zone",
                msg!("Thermal zone"),
                Kind::Integer { min: 0, max: 64 },
            ),
            opt("hwmon-path", msg!("hwmon file"), Kind::Text),
            opt(
                "critical-threshold",
                msg!("Critical from (°C)"),
                Kind::Integer { min: 0, max: 200 },
            ),
            FORMAT,
            opt(
                "format-critical",
                msg!("Format when critical"),
                Kind::Format,
            ),
            INTERVAL,
        ],
        samples: &[
            ("temperatureC", "48"),
            ("temperatureF", "118"),
            ("icon", ""),
        ],
    },
    Module {
        name: "backlight",
        title: msg!("Backlight"),
        description: msg!("The brightness of the built-in display"),
        need: Need::Backlight,
        options: &[
            opt("device", msg!("Device"), Kind::Text),
            FORMAT,
            TOOLTIP_FORMAT,
            opt(
                "scroll-step",
                msg!("Scroll step (percent)"),
                Kind::Decimal {
                    min: 0.0,
                    max: 100.0,
                },
            ),
            ON_SCROLL_UP,
            ON_SCROLL_DOWN,
        ],
        samples: &[("percent", "60"), ("icon", "")],
    },
    Module {
        name: "bluetooth",
        title: msg!("Bluetooth"),
        description: msg!("Whether Bluetooth is on and what is connected"),
        need: Need::Bluetooth,
        options: &[
            FORMAT,
            opt("format-on", msg!("Format when on"), Kind::Format),
            opt("format-off", msg!("Format when off"), Kind::Format),
            opt(
                "format-connected",
                msg!("Format when connected"),
                Kind::Format,
            ),
            TOOLTIP_FORMAT,
            ON_CLICK,
        ],
        samples: &[
            ("status", "on"),
            ("num_connections", "1"),
            ("device_alias", "Headphones"),
            ("controller_alias", "laptop"),
        ],
    },
    Module {
        name: "idle_inhibitor",
        title: msg!("Keep awake"),
        description: msg!("A switch that keeps the screen from blanking"),
        need: Need::Nothing,
        options: &[
            FORMAT,
            opt("start-activated", msg!("On at start"), Kind::Bool),
            opt(
                "timeout",
                msg!("Turn off after (minutes)"),
                Kind::Decimal {
                    min: 0.0,
                    max: 1440.0,
                },
            ),
            TOOLTIP_FORMAT,
        ],
        samples: &[("icon", ""), ("status", "deactivated")],
    },
    Module {
        name: "power-profiles-daemon",
        title: msg!("Power profile"),
        description: msg!("The power profile, switched with a click"),
        need: Need::PowerProfiles,
        options: &[FORMAT, TOOLTIP_FORMAT],
        samples: &[("icon", ""), ("profile", "balanced")],
    },
    Module {
        name: "privacy",
        title: msg!("Privacy"),
        description: msg!("Shows while the screen is shared or the microphone records"),
        need: Need::Program("pipewire"),
        options: &[
            opt(
                "icon-size",
                msg!("Icon size"),
                Kind::Integer { min: 8, max: 128 },
            ),
            opt(
                "icon-spacing",
                msg!("Space between icons"),
                Kind::Integer { min: 0, max: 64 },
            ),
        ],
        samples: &[],
    },
    Module {
        name: "mpris",
        title: msg!("Media player"),
        description: msg!("What a media player plays, with play and pause"),
        need: Need::Nothing,
        options: &[
            FORMAT,
            opt("format-paused", msg!("Format when paused"), Kind::Format),
            opt("player", msg!("Player"), Kind::Text),
            opt(
                "dynamic-len",
                msg!("Longest title (characters)"),
                Kind::Integer { min: 0, max: 1000 },
            ),
            ON_CLICK,
            ON_CLICK_RIGHT,
        ],
        samples: &[
            ("player_icon", "▶"),
            ("status_icon", "⏸"),
            ("dynamic", "Song – Artist"),
            ("title", "Song"),
            ("artist", "Artist"),
        ],
    },
    Module {
        name: "mpd",
        title: msg!("Music Player Daemon"),
        description: msg!("What mpd plays"),
        need: Need::Program("mpd"),
        options: &[
            opt("server", msg!("Server"), Kind::Text),
            opt(
                "port",
                msg!("Port"),
                Kind::Integer {
                    min: 1,
                    max: 65_535,
                },
            ),
            FORMAT,
            opt("format-stopped", msg!("Format when stopped"), Kind::Format),
            INTERVAL,
        ],
        samples: &[("artist", "Artist"), ("title", "Song"), ("album", "Album")],
    },
    Module {
        name: "keyboard-state",
        title: msg!("Lock keys"),
        description: msg!("Whether Caps Lock and Num Lock are on"),
        need: Need::Nothing,
        options: &[
            FORMAT,
            opt("capslock", msg!("Show Caps Lock"), Kind::Bool),
            opt("numlock", msg!("Show Num Lock"), Kind::Bool),
            opt("scrolllock", msg!("Show Scroll Lock"), Kind::Bool),
        ],
        samples: &[("name", "Caps"), ("icon", "")],
    },
    Module {
        name: "sway/workspaces",
        title: msg!("Workspaces"),
        description: msg!("sway's workspaces, switched with a click"),
        need: Need::Nothing,
        options: &[
            FORMAT,
            opt(
                "all-outputs",
                msg!("Workspaces of every display"),
                Kind::Bool,
            ),
            opt(
                "disable-scroll",
                msg!("No switching by scrolling"),
                Kind::Bool,
            ),
            opt("active-only", msg!("Only the active workspace"), Kind::Bool),
        ],
        samples: &[("name", "1"), ("index", "1"), ("icon", "")],
    },
    Module {
        name: "sway/mode",
        title: msg!("Binding mode"),
        description: msg!("sway's binding mode, such as resize, while one is on"),
        need: Need::Nothing,
        options: &[FORMAT, MAX_LENGTH],
        samples: &[("", "resize")],
    },
    Module {
        name: "sway/window",
        title: msg!("Window title"),
        description: msg!("The title of the focused window"),
        need: Need::Nothing,
        options: &[
            FORMAT,
            MAX_LENGTH,
            opt("icon", msg!("App icon"), Kind::Bool),
            TOOLTIP,
        ],
        samples: &[
            ("", "Sway Things"),
            ("title", "Sway Things"),
            ("app_id", "foot"),
        ],
    },
    Module {
        name: "sway/language",
        title: msg!("Keyboard layout"),
        description: msg!("The keyboard layout in use"),
        need: Need::Nothing,
        options: &[FORMAT, TOOLTIP_FORMAT, ON_CLICK],
        samples: &[("short", "us"), ("long", "English (US)"), ("flag", "🇺🇸")],
    },
    Module {
        name: "sway/scratchpad",
        title: msg!("Scratchpad"),
        description: msg!("How many windows are in the scratchpad"),
        need: Need::Nothing,
        options: &[
            FORMAT,
            opt("show-empty", msg!("Show when empty"), Kind::Bool),
            TOOLTIP_FORMAT,
        ],
        samples: &[("count", "2"), ("icon", "")],
    },
    Module {
        name: "custom",
        title: msg!("Custom"),
        description: msg!("The output of a command or script of yours"),
        need: Need::Nothing,
        options: &[
            opt("exec", msg!("Command"), Kind::Command),
            opt("return-type", msg!("Output"), Kind::Choice(&["", "json"])),
            INTERVAL,
            opt(
                "signal",
                msg!("Update on signal (RTMIN+n)"),
                Kind::Integer { min: 1, max: 30 },
            ),
            FORMAT,
            TOOLTIP,
            ON_CLICK,
            ON_CLICK_RIGHT,
            ON_CLICK_MIDDLE,
            ON_SCROLL_UP,
            ON_SCROLL_DOWN,
        ],
        samples: &[("", "text"), ("text", "text"), ("icon", "")],
    },
    Module {
        name: "group",
        title: msg!("Group"),
        description: msg!("Several modules as one, optionally in a drawer that opens on hover"),
        need: Need::Nothing,
        options: &[opt(
            "orientation",
            msg!("Orientation"),
            Kind::Choice(&["inherit", "horizontal", "vertical", "orthogonal"]),
        )],
        samples: &[],
    },
    Module {
        name: "image",
        title: msg!("Image"),
        description: msg!("A picture from a file or a command"),
        need: Need::Nothing,
        options: &[
            opt("path", msg!("File"), Kind::Text),
            opt("exec", msg!("Command that prints the file"), Kind::Command),
            opt("size", msg!("Size"), Kind::Integer { min: 1, max: 512 }),
            INTERVAL,
        ],
        samples: &[],
    },
];

/// The groups of the item picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// The clock, the tray, the session's buttons.
    Basics,
    /// What sway shows: workspaces, the window title, the binding mode, the keyboard layout.
    Sway,
    /// The machine: battery, processor, memory, disks, temperature, brightness, power.
    System,
    /// Network, Bluetooth and sound.
    NetworkAudio,
    /// What plays.
    Media,
    /// Camera, microphone and screen sharing.
    Privacy,
    /// Your own commands, pictures and groups.
    Custom,
}

impl Category {
    /// Every category, in the order the picker shows them.
    pub const ALL: [Self; 7] = [
        Self::Basics,
        Self::Sway,
        Self::System,
        Self::NetworkAudio,
        Self::Media,
        Self::Privacy,
        Self::Custom,
    ];

    /// Its name to show.
    pub fn title(self) -> Msg {
        match self {
            Self::Basics => msg!("Basics"),
            Self::Sway => msg!("Sway"),
            Self::System => msg!("System"),
            Self::NetworkAudio => msg!("Network & Audio"),
            Self::Media => msg!("Media"),
            Self::Privacy => msg!("Privacy"),
            Self::Custom => msg!("Custom"),
        }
    }
}

/// The picker's category of module `name`.
pub fn category(name: &str) -> Category {
    match find(name).map_or("", |module| module.name) {
        "clock" | "tray" | "idle_inhibitor" => Category::Basics,
        "sway/workspaces" | "sway/mode" | "sway/window" | "sway/language" | "sway/scratchpad"
        | "keyboard-state" => Category::Sway,
        "battery"
        | "cpu"
        | "memory"
        | "disk"
        | "temperature"
        | "backlight"
        | "power-profiles-daemon" => Category::System,
        "network" | "pulseaudio" | "wireplumber" | "bluetooth" => Category::NetworkAudio,
        "mpris" | "mpd" => Category::Media,
        "privacy" => Category::Privacy,
        _ => Category::Custom,
    }
}

/// The symbolic icon of module `name`, from the icon theme.
pub fn icon(name: &str) -> &'static str {
    match find(name).map_or("", |module| module.name) {
        "clock" => "preferences-system-time-symbolic",
        "battery" => "battery-good-symbolic",
        "network" => "network-wireless-symbolic",
        "pulseaudio" | "wireplumber" => "audio-volume-high-symbolic",
        "tray" => "view-app-grid-symbolic",
        "cpu" | "memory" | "temperature" => "utilities-system-monitor-symbolic",
        "disk" => "drive-harddisk-symbolic",
        "backlight" => "display-brightness-symbolic",
        "bluetooth" => "bluetooth-active-symbolic",
        "idle_inhibitor" => "weather-clear-night-symbolic",
        "power-profiles-daemon" => "power-profile-balanced-symbolic",
        "privacy" => "camera-web-symbolic",
        "mpris" | "mpd" => "audio-x-generic-symbolic",
        "keyboard-state" | "sway/language" => "input-keyboard-symbolic",
        "sway/workspaces" => "view-grid-symbolic",
        "sway/mode" => "preferences-desktop-keyboard-shortcuts-symbolic",
        "sway/window" => "focus-windows-symbolic",
        "sway/scratchpad" => "view-restore-symbolic",
        "group" => "view-list-symbolic",
        "image" => "image-x-generic-symbolic",
        _ => "utilities-terminal-symbolic",
    }
}

/// The options of `definition` that hold module `name`'s text, first the one it shows most of
/// the time.
fn format_keys(name: &str) -> &'static [&'static str] {
    match find(name).map_or("", |module| module.name) {
        "network" => &["format-wifi", "format", "format-ethernet"],
        "bluetooth" => &["format-connected", "format", "format-on"],
        _ => &["format"],
    }
}

/// What module `name` shows when its definition sets no format: waybar's built-in format, or a
/// sample where it shows no text of its own (the workspaces, the tray).
fn built_in(name: &str) -> &'static str {
    match find(name).map_or("", |module| module.name) {
        "clock" => "{:%H:%M}",
        "battery" => "{capacity}%",
        "network" => "{essid}",
        "pulseaudio" | "wireplumber" => "{volume}%",
        "cpu" => "{usage}%",
        "memory" => "{percentage}%",
        "disk" => "{percentage_used}%",
        "temperature" => "{temperatureC}°C",
        "backlight" => "{percent}%",
        "bluetooth" | "idle_inhibitor" => "{status}",
        "sway/mode" | "sway/window" => "{}",
        "sway/language" => "{short}",
        "sway/scratchpad" => "{icon} {count}",
        "sway/workspaces" | "keyboard-state" => "{name}",
        "power-profiles-daemon" => "{profile}",
        "mpris" => "{dynamic}",
        "mpd" => "{title}",
        _ => "",
    }
}

/// What module `name` with `definition` shows in the preview: its format with sample values,
/// the time `now` for a clock; `None` where it shows nothing of its own text (the tray, a
/// custom command, an image).
pub fn sample(
    name: &str,
    definition: Option<&serde_json::Map<String, serde_json::Value>>,
    now: (u32, u32, u32),
) -> Option<String> {
    let kind = find(name).map_or("", |module| module.name);
    if matches!(kind, "tray" | "image" | "group" | "privacy") {
        return None;
    }
    let format = definition
        .and_then(|definition| {
            format_keys(name)
                .iter()
                .find_map(|key| definition.get(*key)?.as_str())
        })
        .unwrap_or_else(|| built_in(name));
    if format.is_empty() {
        return None;
    }
    // Placeholders without a sample (`{stateIcon}`) show as nothing, as waybar shows an empty
    // value.
    let mut shown = String::new();
    let mut rest = preview(name, format, now);
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}') else {
            break;
        };
        shown.push_str(&rest[..start]);
        rest = rest[start + end + 1..].to_owned();
    }
    shown.push_str(&rest);
    let shown = shown.split_whitespace().collect::<Vec<_>>().join(" ");
    (!shown.is_empty()).then_some(shown)
}

/// The catalog entry for `name` (`battery#bat2` is a `battery`, `custom/media` a `custom`), if
/// the catalog has one.
pub fn find(name: &str) -> Option<&'static Module> {
    let kind = super::kind_of(name);
    let kind = if kind.starts_with("custom/") {
        "custom"
    } else if kind.starts_with("group/") {
        "group"
    } else {
        kind
    };
    MODULES.iter().find(|module| module.name == kind)
}

/// `format` as the module `name` would show it with sample values: each `{placeholder}` the
/// module fills in replaced (`{}` and `{:…}` too), `{:%H:%M}` and the like as a clock shows the
/// time `now` (hour, minute, second), and Pango markup left out. Unknown placeholders stay.
pub fn preview(name: &str, format: &str, now: (u32, u32, u32)) -> String {
    let samples = find(name).map_or(&[][..], |module| module.samples);
    let mut out = String::new();
    let mut rest = format;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let inner = &after[..end];
        let (key, spec) = inner.split_once(':').unwrap_or((inner, ""));
        let value = samples
            .iter()
            .find(|(sample, _)| *sample == key)
            .map(|(_, value)| (*value).to_owned())
            .or_else(|| (key.is_empty() && spec.contains('%')).then(|| strftime(spec, now)));
        if let Some(value) = value {
            out.push_str(&value);
        } else {
            out.push('{');
            out.push_str(inner);
            out.push('}');
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    strip_markup(&out)
}

/// The time `now` in `spec`'s `%H`, `%M`, `%S`, `%I`, `%p`, `%R` and `%T`; the date as a fixed
/// sample (`%Y-%m-%d` is 2026-11-21, a Saturday).
fn strftime(spec: &str, (hour, minute, second): (u32, u32, u32)) -> String {
    let mut out = String::new();
    let mut chars = spec.chars();
    while let Some(char) = chars.next() {
        if char != '%' {
            out.push(char);
            continue;
        }
        let twelve = if hour % 12 == 0 { 12 } else { hour % 12 };
        match chars.next() {
            Some('H') => {
                let _ = write!(out, "{hour:02}");
            }
            Some('M') => {
                let _ = write!(out, "{minute:02}");
            }
            Some('S') => {
                let _ = write!(out, "{second:02}");
            }
            Some('I') => {
                let _ = write!(out, "{twelve:02}");
            }
            Some('p') => out.push_str(if hour < 12 { "AM" } else { "PM" }),
            Some('R') => {
                let _ = write!(out, "{hour:02}:{minute:02}");
            }
            Some('T') => {
                let _ = write!(out, "{hour:02}:{minute:02}:{second:02}");
            }
            Some('Y') => out.push_str("2026"),
            Some('y') => out.push_str("26"),
            Some('m') => out.push_str("11"),
            Some('d' | 'e') => out.push_str("21"),
            Some('a') => out.push_str("Sat"),
            Some('A') => out.push_str("Saturday"),
            Some('b') => out.push_str("Nov"),
            Some('B') => out.push_str("November"),
            Some('F') => out.push_str("2026-11-21"),
            Some('%') | None => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
        }
    }
    out
}

/// What workspace `name` shows with the workspaces' `definition`: its `format` (`{name}` by
/// default) with `{name}`, `{value}` and `{index}` filled in, and `{icon}` from `format-icons`
/// (the workspace's own, else `focused` for the focused one, else `default`, else its name), as
/// waybar does; without markup.
pub fn workspace_label(
    definition: Option<&serde_json::Map<String, serde_json::Value>>,
    name: &str,
    focused: bool,
) -> String {
    let format = definition
        .and_then(|definition| definition.get("format")?.as_str())
        .unwrap_or("{name}");
    let icons = definition.and_then(|definition| definition.get("format-icons")?.as_object());
    let icon = icons
        .and_then(|icons| {
            icons
                .get(name)
                .or_else(|| focused.then(|| icons.get("focused")).flatten())
                .or_else(|| icons.get("default"))?
                .as_str()
        })
        .unwrap_or(name);
    let text = format
        .replace("{name}", name)
        .replace("{value}", name)
        .replace("{index}", name)
        .replace("{icon}", icon);
    let text = strip_markup(&text).trim().to_owned();
    if text.is_empty() {
        name.to_owned()
    } else {
        text
    }
}

/// What the item of a command of the user's, `name`, shows in a preview, where its command
/// doesn't run: its `format` with the command's text (`{}`, `{text}`, `{alt}`) as its name and
/// the other placeholders as nothing, without markup; its name where that leaves nothing else.
/// `custom/vpn#1` is named `vpn`.
pub fn custom_sample(
    name: &str,
    definition: Option<&serde_json::Map<String, serde_json::Value>>,
) -> String {
    let short = super::kind_of(name);
    let short = short.strip_prefix("custom/").unwrap_or(short).to_owned();
    let format = definition
        .and_then(|definition| definition.get("format")?.as_str())
        .unwrap_or("{}");
    let mut out = String::new();
    let mut rest = format;
    let mut own = false;
    while let Some(start) = rest.find('{') {
        let before = &rest[..start];
        own |= !strip_markup(before).trim().is_empty();
        out.push_str(before);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            break;
        };
        let key = after[..end].split(':').next().unwrap_or("");
        if matches!(key, "" | "text" | "alt") {
            out.push_str(&short);
        }
        rest = &after[end + 1..];
    }
    own |= !strip_markup(rest).trim().is_empty();
    out.push_str(rest);
    let text = strip_markup(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if own && !text.is_empty() { text } else { short }
}

/// `text` without Pango markup tags, with the entities waybar's formats use resolved.
fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for char in text.chars() {
        match char {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(char),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspaces_show_their_format() {
        let definition = serde_json::json!({
            "format": "<b>{icon}</b>",
            "format-icons": {"1": "one", "focused": "F", "default": "D"}
        });
        let definition = definition.as_object();
        assert_eq!(workspace_label(None, "2", true), "2");
        assert_eq!(workspace_label(definition, "1", true), "one");
        assert_eq!(workspace_label(definition, "2", true), "F");
        assert_eq!(workspace_label(definition, "3", false), "D");
    }

    #[test]
    fn commands_show_their_format_or_their_name() {
        let format = |format: &str| serde_json::json!({ "format": format });
        assert_eq!(custom_sample("custom/claude", None), "claude");
        assert_eq!(custom_sample("custom/vpn#1", None), "vpn");
        assert_eq!(
            custom_sample(
                "custom/notify",
                format("<span font_family='X'>{}</span>").as_object()
            ),
            "notify"
        );
        assert_eq!(
            custom_sample(
                "custom/launcher",
                format("<span size='125%'>\u{f08c7}</span>").as_object()
            ),
            "\u{f08c7}"
        );
        assert_eq!(
            custom_sample("custom/media", format("♪ {text} {icon}").as_object()),
            "♪ media"
        );
    }

    #[test]
    fn modules_are_found_by_kind() {
        assert_eq!(
            find("battery#bat2").map(|module| module.name),
            Some("battery")
        );
        assert_eq!(
            find("custom/claude#detail").map(|module| module.name),
            Some("custom")
        );
        assert_eq!(find("group/net").map(|module| module.name), Some("group"));
        assert_eq!(find("cava"), None);
        let mut names: Vec<&str> = MODULES.iter().map(|module| module.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), MODULES.len(), "names are unique");
        for module in MODULES {
            let mut keys: Vec<&str> = module.options.iter().map(|opt| opt.key).collect();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(keys.len(), module.options.len(), "{}", module.name);
        }
    }

    #[test]
    fn every_module_has_a_category_and_a_sample() {
        let now = (14, 32, 5);
        assert_eq!(category("clock"), Category::Basics);
        assert_eq!(category("battery#bat2"), Category::System);
        assert_eq!(category("custom/media"), Category::Custom);
        assert_eq!(icon("custom/x"), "utilities-terminal-symbolic");
        assert_eq!(sample("clock", None, now).as_deref(), Some("14:32"));
        let definition = serde_json::json!({"format": "{capacity}% {icon}"});
        assert!(
            sample("battery", definition.as_object(), now)
                .is_some_and(|sample| sample.starts_with("76%"))
        );
        let wifi = serde_json::json!({"format-wifi": "{essid} ({signalStrength}%)"});
        assert_eq!(
            sample("network", wifi.as_object(), now).as_deref(),
            Some("Home (71%)")
        );
        assert_eq!(sample("tray", None, now), None);
        let mpd = serde_json::json!({"format": "{stateIcon} {title}  {volume}%"});
        assert_eq!(
            sample("mpd", mpd.as_object(), now).as_deref(),
            Some("Song %")
        );
        assert_eq!(sample("custom/x", None, now), None);
        for module in MODULES {
            assert!(Category::ALL.contains(&category(module.name)));
        }
    }

    #[test]
    fn formats_preview_with_samples() {
        assert_eq!(preview("battery", "{capacity}% {icon}", (9, 5, 0)), "76% ");
        assert_eq!(preview("clock", "{:%H:%M}", (9, 5, 7)), "09:05");
        assert_eq!(
            preview("clock", "<big>{:%Y %B}</big> {unknown}", (0, 0, 0)),
            "2026 November {unknown}"
        );
        assert_eq!(
            preview("sway/mode", "<span style=\"italic\">{}</span>", (0, 0, 0)),
            "resize"
        );
        assert_eq!(preview("custom/x", "a &amp; b {", (0, 0, 0)), "a & b {");
    }

    #[test]
    fn needs_are_checked_against_the_system_files() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            Need::Battery.check_in(root.path(), None),
            Works::No(Need::Battery)
        );
        let bat = root.path().join("sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat).unwrap();
        std::fs::write(bat.join("type"), "Battery\n").unwrap();
        assert_eq!(Need::Battery.check_in(root.path(), None), Works::Yes);
        assert_eq!(Need::Nothing.check_in(root.path(), None), Works::Yes);
        assert_eq!(
            Need::Program("waybar").check_in(root.path(), None),
            Works::No(Need::Program("waybar"))
        );
    }
}
