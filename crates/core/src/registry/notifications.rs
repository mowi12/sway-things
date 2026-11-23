// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Notifications page: what the notification daemon looks like and does. None of
//! them is a sway directive except the blur behind notifications on SwayFX (`layer_effects`):
//! the others go into the files Sway Things writes for the daemon
//! ([`crate::notifications`]), each in the daemon's own format, and only where they are stored.
//!
//! The defaults are the daemon's own: mako's from `mako(5)` of mako 1.11, swaync's from the
//! `config.json` swaync 0.12.6 ships (what a fresh install runs with) and its packaged style
//! sheet. Settings both share (the position, the colors) start from what both do.

use super::read::{self, ReadError};
use super::{Apply, Choice, ConfigFile, Directive, Font, Page, Setting, Unit, Value, ValueError};
use super::{SettingBuilder, When};
use crate::command::Color;
use crate::command::effects::{self, Layer};
use crate::msg;
use crate::version::SwayFxVersion;

/// Which daemon Sway Things starts from its include, if any.
pub const START: &str = "notifications.start";
/// Clicking a notification brings its app's window to the front.
pub const CLICK_FOCUS: &str = "notifications.click_focus";
/// Where notifications show.
pub const POSITION: &str = "notifications.position";
/// The display they show on, by its name; empty for the focused one.
pub const OUTPUT: &str = "notifications.output";
/// Where the colors come from.
pub const COLORS: &str = "notifications.colors";
/// The background, with `notifications.colors = "custom"`.
pub const BACKGROUND: &str = "notifications.background";
/// The text, with `notifications.colors = "custom"`.
pub const TEXT: &str = "notifications.text";
/// The border, with `notifications.colors = "custom"`.
pub const BORDER: &str = "notifications.border";
/// Blur behind notifications, on SwayFX.
pub const BLUR: &str = "notifications.blur";

/// mako's settings.
pub mod mako {
    /// `width`
    pub const WIDTH: &str = "notifications.mako.width";
    /// `height`, the most a notification grows to.
    pub const HEIGHT: &str = "notifications.mako.height";
    /// `max-visible`
    pub const MAX_VISIBLE: &str = "notifications.mako.max_visible";
    /// `default-timeout`, in seconds here.
    pub const TIMEOUT: &str = "notifications.mako.timeout";
    /// `default-timeout` of `[urgency=critical]`, in seconds here.
    pub const CRITICAL_TIMEOUT: &str = "notifications.mako.critical_timeout";
    /// `group-by=app-name`
    pub const GROUP: &str = "notifications.mako.group";
    /// `icons`
    pub const ICONS: &str = "notifications.mako.icons";
    /// `font`
    pub const FONT: &str = "notifications.mako.font";
    /// `border-size`
    pub const BORDER_SIZE: &str = "notifications.mako.border_size";
    /// `border-radius`
    pub const BORDER_RADIUS: &str = "notifications.mako.border_radius";
}

/// swaync's settings.
pub mod swaync {
    /// `notification-window-width`
    pub const WIDTH: &str = "notifications.swaync.width";
    /// `timeout`
    pub const TIMEOUT: &str = "notifications.swaync.timeout";
    /// `timeout-low`
    pub const TIMEOUT_LOW: &str = "notifications.swaync.timeout_low";
    /// `timeout-critical`
    pub const TIMEOUT_CRITICAL: &str = "notifications.swaync.timeout_critical";
    /// `notification-grouping`
    pub const GROUPING: &str = "notifications.swaync.grouping";
    /// `control-center-width`
    pub const PANEL_WIDTH: &str = "notifications.swaync.panel_width";
    /// `control-center-height`
    pub const PANEL_HEIGHT: &str = "notifications.swaync.panel_height";
    /// `widgets`: the widgets of the panel, in order.
    pub const WIDGETS: &str = "notifications.swaync.widgets";
    /// The buttons of the `buttons-grid` widget, by preset.
    pub const BUTTONS: &str = "notifications.swaync.buttons";
    /// The style: swaync's own, or the frosted preset.
    pub const STYLE: &str = "notifications.swaync.style";
    /// `--border-radius` of the style.
    pub const CORNER_RADIUS: &str = "notifications.swaync.corner_radius";
    /// Options of swaync's config that have no setting here, as a JSON object: what an import
    /// found, passed through as it is.
    pub const OTHER: &str = "notifications.swaync.other";
}

const STARTS: &[Choice] = &[
    // TRANSLATORS: Sway Things starts no notification daemon itself.
    Choice::new("none", msg!("None")),
    Choice::new("mako", msg!("mako")),
    Choice::new("swaync", msg!("SwayNotificationCenter")),
    Choice::new("dunst", msg!("dunst")),
];

/// The positions both mako (`anchor`) and swaync (`positionX`, `positionY`) can take.
pub const POSITIONS: &[Choice] = &[
    Choice::new("top-right", msg!("Top right")),
    Choice::new("top-center", msg!("Top center")),
    Choice::new("top-left", msg!("Top left")),
    Choice::new("bottom-right", msg!("Bottom right")),
    Choice::new("bottom-center", msg!("Bottom center")),
    Choice::new("bottom-left", msg!("Bottom left")),
    Choice::new("center", msg!("Center")),
];

const COLOR_SOURCES: &[Choice] = &[
    // TRANSLATORS: The notification daemon's own colors.
    Choice::new("daemon", msg!("The daemon\u{2019}s own")),
    Choice::new("windows", msg!("Like the focused window")),
    Choice::new("custom", msg!("Custom")),
];

const STYLES: &[Choice] = &[
    Choice::new("default", msg!("swaync\u{2019}s own")),
    // TRANSLATORS: A look: translucent cards that let a blurred background through.
    Choice::new("frosted", msg!("Frosted glass")),
];

/// The widgets of swaync's panel the page offers, by the names swaync knows them.
pub const WIDGETS: &[&str] = &[
    "title",
    "dnd",
    "notifications",
    "mpris",
    "volume",
    "backlight",
    "buttons-grid",
    "inhibitors",
];

/// The buttons of the `buttons-grid` widget the page offers.
pub const BUTTONS: &[&str] = &[
    "dnd",
    "wifi",
    "bluetooth",
    "sound",
    "microphone",
    "settings",
];

/// mako's colors, which are sway's own of the focused window.
const MAKO_BACKGROUND: Color = Color::rgb(0x28, 0x55, 0x77);
const MAKO_TEXT: Color = Color::rgb(0xff, 0xff, 0xff);
const MAKO_BORDER: Color = Color::rgb(0x4c, 0x78, 0x99);

/// A widget name swaync takes: `name` or `name#id`, of letters, digits, `_` and `-`
/// (`configSchema.json`).
///
/// # Errors
///
/// [`ValueError::NotAWidget`] for another text.
pub fn check_widget(name: &str) -> Result<(), ValueError> {
    let part = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    let mut parts = name.splitn(2, '#');
    let valid = parts.next().is_some_and(part) && parts.next().is_none_or(part);
    if valid {
        Ok(())
    } else {
        Err(ValueError::NotAWidget)
    }
}

/// A button preset of [`BUTTONS`].
fn check_button(name: &str) -> Result<(), ValueError> {
    if BUTTONS.contains(&name) {
        Ok(())
    } else {
        Err(ValueError::NotAWidget)
    }
}

/// A JSON object, or empty for none.
///
/// # Errors
///
/// [`ValueError::NotAJsonObject`] for another text.
pub fn check_json_object(text: &str) -> Result<(), ValueError> {
    if text.trim().is_empty()
        || serde_json::from_str::<serde_json::Value>(text).is_ok_and(|value| value.is_object())
    {
        Ok(())
    } else {
        Err(ValueError::NotAJsonObject)
    }
}

/// A line of a daemon's config: at most 255 characters, no control characters. An output name
/// or a font.
fn check_line(text: &str) -> Result<(), ValueError> {
    if text.chars().count() > 255 || text.chars().any(char::is_control) {
        return Err(ValueError::NotDaemonText);
    }
    Ok(())
}

/// A font description: like [`check_line`], and not empty.
fn check_font(text: &str) -> Result<(), ValueError> {
    if text.trim().is_empty() {
        return Err(ValueError::NotAFont);
    }
    check_line(text)
}

/// `layer_effects notifications blur …`: whether the blur is on, from the effects after the
/// namespace (the arguments after it) (`blur enable`, also among others of a block joined by `;`).
fn read_blur(args: &[String]) -> Result<Value, ReadError> {
    let words: Vec<&str> = args
        .iter()
        .flat_map(|arg| arg.split([' ', ';']))
        .filter(|word| !word.is_empty())
        .collect();
    let at = words
        .iter()
        .position(|word| *word == "blur")
        .ok_or_else(|| ReadError::Unsupported(msg!("another effect than blur").into()))?;
    let value = words
        .get(at + 1)
        .ok_or_else(|| ReadError::Rejected(msg!("blur without enable or disable").into()))?;
    Ok(Value::Bool(read::boolean(value, true)))
}

fn mako_setting(builder: SettingBuilder) -> SettingBuilder {
    builder
        .section(msg!("mako"))
        .apply(Apply::ToolReload)
        .requires_tools(&["mako"])
}

fn swaync_setting(builder: SettingBuilder, section: super::Msg) -> SettingBuilder {
    builder
        .section(section)
        .apply(Apply::ToolReload)
        .requires_tools(&["swaync"])
}

fn custom_color(builder: SettingBuilder) -> SettingBuilder {
    builder
        .section(msg!("Colors"))
        .apply(Apply::ToolReload)
        .depends_on(COLORS, When::OneOf(&["custom"]))
}

pub(super) fn settings() -> Vec<Setting> {
    let mut settings = common();
    settings.extend(mako_settings());
    settings.extend(swaync_settings());
    settings
}

fn common() -> Vec<Setting> {
    vec![
        Setting::choice(START, Page::Notifications, "none", STARTS)
            .section(msg!("Daemon"))
            .title(msg!("Start with sway"))
            .subtitle(msg!(
                "Started from Sway Things\u{2019} include, while your config starts none"
            ))
            .keywords(&[
                msg!("daemon"),
                msg!("mako"),
                msg!("swaync"),
                msg!("dunst"),
                msg!("autostart"),
            ])
            .apply(Apply::ToolReload)
            .build(),
        Setting::toggle(CLICK_FOCUS, Page::Notifications, false)
            .section(msg!("Daemon"))
            .title(msg!("Clicking a notification shows its app"))
            .subtitle(msg!(
                "Brings the app\u{2019}s window to the front; the app still opens the chat or \
                 mail itself"
            ))
            .keywords(&[msg!("click"), msg!("focus"), msg!("jump to app")])
            .apply(Apply::ToolReload)
            .build(),
        Setting::choice(POSITION, Page::Notifications, "top-right", POSITIONS)
            .section(msg!("Position"))
            .title(msg!("Position"))
            .subtitle(msg!("Where on the display notifications show"))
            .keywords(&[msg!("corner"), msg!("anchor"), msg!("top"), msg!("bottom")])
            .apply(Apply::ToolReload)
            .build(),
        Setting::text(OUTPUT, Page::Notifications, "")
            .section(msg!("Position"))
            .title(msg!("Display"))
            .subtitle(msg!(
                "The name of the display, like DP-1; empty for the focused one"
            ))
            .placeholder(msg!("The focused display"))
            .keywords(&[msg!("monitor"), msg!("screen"), msg!("output")])
            .check_text(check_line)
            .apply(Apply::ToolReload)
            .build(),
        Setting::choice(COLORS, Page::Notifications, "daemon", COLOR_SOURCES)
            .section(msg!("Colors"))
            .title(msg!("Colors"))
            .subtitle(msg!(
                "Like the focused window follows the Windows page, with its border as the accent"
            ))
            .keywords(&[msg!("theme"), msg!("accent"), msg!("look")])
            .apply(Apply::ToolReload)
            .build(),
        custom_color(Setting::color(
            BACKGROUND,
            Page::Notifications,
            MAKO_BACKGROUND,
        ))
        .title(msg!("Background"))
        .build(),
        custom_color(Setting::color(TEXT, Page::Notifications, MAKO_TEXT))
            .title(msg!("Text"))
            .build(),
        custom_color(Setting::color(BORDER, Page::Notifications, MAKO_BORDER))
            .title(msg!("Border"))
            .build(),
        Setting::toggle(BLUR, Page::Notifications, false)
            .section(msg!("Colors"))
            .title(msg!("Blur behind notifications"))
            .subtitle(msg!(
                "Shows through a blurred desktop where the background is see-through"
            ))
            .keywords(&[
                msg!("frosted glass"),
                msg!("transparency"),
                msg!("layer effects"),
            ])
            .swayfx_only(SwayFxVersion::OLDEST_SUPPORTED)
            .sway(
                Directive::top("layer_effects").arguments(&["notifications"]),
                ConfigFile::Effects,
                |ctx| {
                    let on = ctx.bool(BLUR)?;
                    Ok(Layer::NOTIFICATIONS
                        .iter()
                        .map(|layer| effects::layer_blur(layer.namespace(), on))
                        .collect())
                },
                read_blur,
            )
            .build(),
    ]
}

fn mako_settings() -> Vec<Setting> {
    use mako::{
        BORDER_RADIUS, BORDER_SIZE, CRITICAL_TIMEOUT, FONT, GROUP, HEIGHT, ICONS, MAX_VISIBLE,
        TIMEOUT, WIDTH,
    };
    vec![
        mako_setting(Setting::integer(WIDTH, Page::Notifications, 300, 1, 4000))
            .title(msg!("Width"))
            .slider(150.0, 1000.0, 10.0)
            .unit(Unit::Pixels)
            .build(),
        mako_setting(Setting::integer(HEIGHT, Page::Notifications, 100, 1, 4000))
            .title(msg!("Maximum height"))
            .subtitle(msg!("Shorter notifications shrink to fit"))
            .slider(50.0, 800.0, 10.0)
            .unit(Unit::Pixels)
            .build(),
        mako_setting(Setting::integer(
            MAX_VISIBLE,
            Page::Notifications,
            5,
            -1,
            50,
        ))
        .title(msg!("Visible at once"))
        .subtitle(msg!("Older ones wait; -1 shows all"))
        .slider(-1.0, 20.0, 1.0)
        .build(),
        mako_setting(Setting::integer(TIMEOUT, Page::Notifications, 0, 0, 3600))
            .title(msg!("Hide after"))
            .subtitle(msg!(
                "For notifications that don\u{2019}t say; 0 keeps them"
            ))
            .slider(0.0, 60.0, 1.0)
            .unit(Unit::Seconds)
            .keywords(&[msg!("timeout"), msg!("duration")])
            .build(),
        mako_setting(Setting::integer(
            CRITICAL_TIMEOUT,
            Page::Notifications,
            0,
            0,
            3600,
        ))
        .title(msg!("Hide critical ones after"))
        .subtitle(msg!("0 keeps them until you close them"))
        .slider(0.0, 60.0, 1.0)
        .unit(Unit::Seconds)
        .keywords(&[msg!("timeout"), msg!("urgent")])
        .build(),
        mako_setting(Setting::toggle(GROUP, Page::Notifications, false))
            .title(msg!("Group by app"))
            .keywords(&[msg!("grouping"), msg!("stack")])
            .build(),
        mako_setting(Setting::toggle(ICONS, Page::Notifications, true))
            .title(msg!("Icons"))
            .build(),
        mako_setting(Setting::text(FONT, Page::Notifications, "monospace 10"))
            .title(msg!("Font"))
            .font(Font::Any)
            .check_text(check_font)
            .build(),
        mako_setting(Setting::integer(BORDER_SIZE, Page::Notifications, 2, 0, 20))
            .title(msg!("Border width"))
            .unit(Unit::Pixels)
            .build(),
        mako_setting(Setting::integer(
            BORDER_RADIUS,
            Page::Notifications,
            0,
            0,
            50,
        ))
        .title(msg!("Rounded corners"))
        .unit(Unit::Pixels)
        .keywords(&[msg!("corner radius")])
        .build(),
    ]
}

fn swaync_settings() -> Vec<Setting> {
    let mut settings = swaync_popups();
    settings.extend(swaync_panel());
    settings
}

fn swaync_popups() -> Vec<Setting> {
    use swaync::{CORNER_RADIUS, GROUPING, STYLE, TIMEOUT, TIMEOUT_CRITICAL, TIMEOUT_LOW, WIDTH};
    let popups = msg!("swaync: Notifications");
    vec![
        swaync_setting(
            Setting::integer(WIDTH, Page::Notifications, 500, 100, 4000),
            popups,
        )
        .title(msg!("Width"))
        .slider(200.0, 1000.0, 10.0)
        .unit(Unit::Pixels)
        .build(),
        swaync_setting(
            Setting::integer(TIMEOUT, Page::Notifications, 10, 0, 3600),
            popups,
        )
        .title(msg!("Hide after"))
        .subtitle(msg!("0 keeps them"))
        .slider(0.0, 60.0, 1.0)
        .unit(Unit::Seconds)
        .keywords(&[msg!("timeout"), msg!("duration")])
        .build(),
        swaync_setting(
            Setting::integer(TIMEOUT_LOW, Page::Notifications, 5, 0, 3600),
            popups,
        )
        .title(msg!("Hide unimportant ones after"))
        .slider(0.0, 60.0, 1.0)
        .unit(Unit::Seconds)
        .keywords(&[msg!("timeout"), msg!("low")])
        .build(),
        swaync_setting(
            Setting::integer(TIMEOUT_CRITICAL, Page::Notifications, 0, 0, 3600),
            popups,
        )
        .title(msg!("Hide critical ones after"))
        .subtitle(msg!("0 keeps them until you close them"))
        .slider(0.0, 60.0, 1.0)
        .unit(Unit::Seconds)
        .keywords(&[msg!("timeout"), msg!("urgent")])
        .build(),
        swaync_setting(Setting::toggle(GROUPING, Page::Notifications, true), popups)
            .title(msg!("Group by app"))
            .keywords(&[msg!("grouping"), msg!("stack")])
            .build(),
        swaync_setting(
            Setting::choice(STYLE, Page::Notifications, "default", STYLES),
            popups,
        )
        .title(msg!("Style"))
        .subtitle(msg!(
            "Frosted glass looks best with blur behind notifications"
        ))
        .keywords(&[msg!("theme"), msg!("look"), msg!("css")])
        .build(),
        swaync_setting(
            Setting::integer(CORNER_RADIUS, Page::Notifications, 12, 0, 50),
            popups,
        )
        .title(msg!("Rounded corners"))
        .unit(Unit::Pixels)
        .keywords(&[msg!("corner radius")])
        .build(),
    ]
}

fn swaync_panel() -> Vec<Setting> {
    use swaync::{BUTTONS, OTHER, PANEL_HEIGHT, PANEL_WIDTH, WIDGETS};
    let panel = msg!("swaync: Control center");
    vec![
        swaync_setting(
            Setting::integer(PANEL_WIDTH, Page::Notifications, 500, 300, 4000),
            panel,
        )
        .title(msg!("Width"))
        .slider(300.0, 1000.0, 10.0)
        .unit(Unit::Pixels)
        .keywords(&[msg!("panel"), msg!("notification center")])
        .build(),
        swaync_setting(
            Setting::integer(PANEL_HEIGHT, Page::Notifications, 600, 100, 4000),
            panel,
        )
        .title(msg!("Height"))
        .subtitle(msg!("Unless it fits the screen"))
        .slider(200.0, 1400.0, 10.0)
        .unit(Unit::Pixels)
        .keywords(&[msg!("panel"), msg!("notification center")])
        .build(),
        swaync_setting(
            Setting::list(
                WIDGETS,
                Page::Notifications,
                &["inhibitors", "title", "dnd", "notifications"],
            ),
            panel,
        )
        .title(msg!("Widgets"))
        .subtitle(msg!("What the panel shows, from the top"))
        .keywords(&[
            msg!("panel"),
            msg!("media"),
            msg!("mpris"),
            msg!("volume"),
            msg!("buttons"),
        ])
        .check_text(check_widget)
        .build(),
        swaync_setting(
            Setting::list(BUTTONS, Page::Notifications, &["dnd", "wifi", "bluetooth"]),
            panel,
        )
        .title(msg!("Buttons"))
        .subtitle(msg!("Quick switches in the panel\u{2019}s button grid"))
        .keywords(&[msg!("quick settings"), msg!("toggles"), msg!("wi-fi")])
        .check_text(check_button)
        .build(),
        swaync_setting(Setting::text(OTHER, Page::Notifications, ""), panel)
            .title(msg!("Other options"))
            .subtitle(msg!(
                "Further options of swaync\u{2019}s config.json, as JSON"
            ))
            .placeholder(msg!("None"))
            .keywords(&[msg!("json"), msg!("advanced"), msg!("scripts")])
            .check_text(check_json_object)
            .build(),
    ]
}
