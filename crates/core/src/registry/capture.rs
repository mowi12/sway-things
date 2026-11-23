// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Screenshots & Clipboard page: the screenshot keys and where screenshots go,
//! the clipboard history, and screen recording.
//!
//! None of them has a sway directive: the keys go to `50-bindings.conf`
//! ([`crate::capture::shortcuts`]) and run the command line, which reads the others when a key
//! is pressed; the clipboard history is started by `sway-things hook clipboard` from
//! `80-hooks.conf`.
//!
//! The keys, the defaults and the checks are defined here and re-exported by [`crate::capture`],
//! so the registry stays the bottom layer: the check of the keys reads them with the binding
//! reader (`bindings::keys`), which imports neither the registry nor the state.

use gio::glib;

use super::{Choice, Page, Setting, ValueError, When};
use crate::bindings::keys;
use crate::msg;

/// The key of the switch of the screenshot keys.
pub const SCREENSHOTS: &str = "screenshots.enabled";
/// The key of the keys for a screenshot of a region.
pub const KEY_REGION: &str = "screenshots.key.region";
/// The key of the keys for a screenshot of the focused window.
pub const KEY_WINDOW: &str = "screenshots.key.window";
/// The key of the keys for a screenshot of the focused display.
pub const KEY_DISPLAY: &str = "screenshots.key.display";
/// The key of the keys for a screenshot of every display.
pub const KEY_ALL: &str = "screenshots.key.all";
/// The key of where a screenshot goes: the clipboard, the folder, or both.
pub const DESTINATION: &str = "screenshots.destination";
/// The key of the folder of screenshots and recordings.
pub const FOLDER: &str = "screenshots.folder";
/// The key of the file name pattern of screenshots.
pub const FILE_NAME: &str = "screenshots.file_name";
/// The key of what happens after a screenshot.
pub const AFTERWARDS: &str = "screenshots.afterwards";
/// The key of the switch of the clipboard history.
pub const HISTORY: &str = "clipboard.enabled";
/// The key of the keys that open the clipboard history.
pub const HISTORY_KEY: &str = "clipboard.key";
/// The key of how many entries the history keeps.
pub const MAX_ENTRIES: &str = "clipboard.max_entries";
/// The key of the switch of the recording key.
pub const RECORDING: &str = "recording.enabled";
/// The key of the keys that start and stop a recording.
pub const RECORDING_KEY: &str = "recording.key";
/// The key of what a recording captures: a region or the focused display.
pub const RECORDING_AREA: &str = "recording.area";
/// The key of the file name pattern of recordings.
pub const RECORDING_FILE_NAME: &str = "recording.file_name";
/// The key of whether a recording has sound.
pub const RECORDING_AUDIO: &str = "recording.audio";

/// The default file name pattern of screenshots, as GNOME names them.
pub const DEFAULT_FILE_NAME: &str = "Screenshot from %Y-%m-%d %H-%M-%S.png";
/// The default file name pattern of recordings.
pub const DEFAULT_RECORDING_FILE_NAME: &str = "Recording from %Y-%m-%d %H-%M-%S.mp4";

/// The check of a setting with keys: empty, or keys that need no quotes (`$mod` for the main
/// modifier) and leave typing alone.
///
/// # Errors
///
/// [`ValueError::NotAKey`] for keys that can't be bound, [`ValueError::TypingKey`] for a key
/// that types without Super, Ctrl or Alt.
pub fn check_keys(text: &str) -> Result<(), ValueError> {
    if text.is_empty() {
        return Ok(());
    }
    if !keys::is_plain(text) {
        return Err(ValueError::NotAKey);
    }
    let combo = keys::parse(&keys::with_mod(text, keys::Modifiers::SUPER), false, false)
        .map_err(|_| ValueError::NotAKey)?;
    if combo.keys.is_empty() || combo.device == keys::Device::Mouse {
        return Err(ValueError::NotAKey);
    }
    if !keys::leaves_typing(&combo) {
        return Err(ValueError::TypingKey);
    }
    Ok(())
}

/// The check of the folder setting: empty, or a path that starts with `/` or `~/`, without
/// control characters.
///
/// # Errors
///
/// [`ValueError::NotAFolder`].
pub fn check_folder(text: &str) -> Result<(), ValueError> {
    let rooted = text.is_empty() || text.starts_with('/') || text == "~" || text.starts_with("~/");
    if rooted && !text.chars().any(char::is_control) {
        Ok(())
    } else {
        Err(ValueError::NotAFolder)
    }
}

/// The file name `pattern` makes at `time`, or `None` if it can't make one: empty, a slash or a
/// control character, an unknown conversion, or a name of only dots.
pub fn file_name(pattern: &str, time: &glib::DateTime) -> Option<String> {
    if pattern.is_empty() || pattern.contains('/') || pattern.chars().any(char::is_control) {
        return None;
    }
    let name = time.format(pattern).ok()?.to_string();
    let usable = !name.is_empty()
        && !name.contains('/')
        && !name.chars().any(char::is_control)
        && !name.chars().all(|c| c == '.');
    usable.then_some(name)
}

/// The check of a file name pattern setting: it must make a file name now.
///
/// # Errors
///
/// [`ValueError::NotAFileName`].
pub fn check_file_name(pattern: &str) -> Result<(), ValueError> {
    let now = glib::DateTime::now_local().map_err(|_| ValueError::NotAFileName)?;
    file_name(pattern, &now)
        .map(|_| ())
        .ok_or(ValueError::NotAFileName)
}

const DESTINATIONS: &[Choice] = &[
    Choice::new("both", msg!("Copy and save")),
    Choice::new("clipboard", msg!("Copy to the clipboard")),
    Choice::new("folder", msg!("Save to the folder")),
];

const AFTER: &[Choice] = &[
    Choice::new("notify", msg!("Show a notification")),
    Choice::new("edit", msg!("Open the editor")),
    Choice::new("nothing", msg!("Nothing")),
];

const AREAS: &[Choice] = &[
    Choice::new("region", msg!("A region you select")),
    Choice::new("display", msg!("The focused display")),
];

pub(super) fn settings() -> Vec<Setting> {
    let mut settings = screenshots();
    settings.extend(clipboard());
    settings.extend(recording());
    settings
}

/// A setting with the keys of a shortcut, bound while `switch` is on.
fn keys(key: &'static str, default: &'static str, switch: &'static str) -> super::SettingBuilder {
    Setting::text(key, Page::ScreenshotsClipboard, default)
        .check_text(check_keys)
        .placeholder(msg!("No key"))
        .depends_on(switch, When::On)
}

fn screenshots() -> Vec<Setting> {
    let section = msg!("Screenshots");
    vec![
        Setting::toggle(SCREENSHOTS, Page::ScreenshotsClipboard, false)
            .section(section)
            .title(msg!("Screenshot keys"))
            .subtitle(msg!(
                "Take screenshots with the keys below, with grim and slurp"
            ))
            .requires_tools(&["grim", "slurp"])
            .keywords(&[
                msg!("screenshot"),
                msg!("print screen"),
                msg!("capture"),
                msg!("grim"),
                msg!("slurp"),
            ])
            .build(),
        keys(KEY_REGION, "Print", SCREENSHOTS)
            .section(section)
            .title(msg!("Region"))
            .subtitle(msg!("Select a region, or click a window"))
            .keywords(&[msg!("area"), msg!("selection")])
            .build(),
        keys(KEY_WINDOW, "Alt+Print", SCREENSHOTS)
            .section(section)
            .title(msg!("Window"))
            .subtitle(msg!("The focused window"))
            .build(),
        keys(KEY_DISPLAY, "Shift+Print", SCREENSHOTS)
            .section(section)
            .title(msg!("Display"))
            .subtitle(msg!("The display with the focused window"))
            .keywords(&[msg!("screen"), msg!("monitor"), msg!("output")])
            .build(),
        keys(KEY_ALL, "Ctrl+Print", SCREENSHOTS)
            .section(section)
            .title(msg!("All displays"))
            .keywords(&[msg!("screen"), msg!("monitor"), msg!("full")])
            .build(),
        Setting::choice(
            DESTINATION,
            Page::ScreenshotsClipboard,
            "both",
            DESTINATIONS,
        )
        .section(section)
        .title(msg!("Where screenshots go"))
        .keywords(&[msg!("clipboard"), msg!("save"), msg!("copy")])
        .build(),
        Setting::text(FOLDER, Page::ScreenshotsClipboard, "")
            .section(section)
            .title(msg!("Folder"))
            .subtitle(msg!(
                "Where screenshots and recordings are saved; made when the first one goes there"
            ))
            .placeholder(msg!("Screenshots in your Pictures folder"))
            .check_text(check_folder)
            .keywords(&[msg!("directory"), msg!("path"), msg!("location")])
            .build(),
        Setting::text(FILE_NAME, Page::ScreenshotsClipboard, DEFAULT_FILE_NAME)
            .section(section)
            .title(msg!("File name"))
            .subtitle(msg!(
                "%Y, %m, %d, %H, %M and %S are the year, month, day, hour, minute and second; \
                 .png, .jpg or .ppm at the end decides the format"
            ))
            .check_text(check_file_name)
            .keywords(&[msg!("pattern"), msg!("name"), msg!("date")])
            .build(),
        Setting::choice(AFTERWARDS, Page::ScreenshotsClipboard, "notify", AFTER)
            .section(section)
            .title(msg!("Afterwards"))
            .subtitle(msg!(
                "A notification with the picture, with Open and Edit, or the editor right away \
                 (swappy or satty)"
            ))
            .keywords(&[
                msg!("notification"),
                msg!("editor"),
                msg!("annotate"),
                msg!("swappy"),
                msg!("satty"),
            ])
            .build(),
    ]
}

fn clipboard() -> Vec<Setting> {
    let section = msg!("Clipboard history");
    vec![
        Setting::toggle(HISTORY, Page::ScreenshotsClipboard, false)
            .section(section)
            .title(msg!("Clipboard history"))
            .subtitle(msg!(
                "Keep what you copy, text and images, with cliphist, and pick it again with a key"
            ))
            .requires_tools(&["cliphist", "wl-paste", "wl-copy"])
            .keywords(&[
                msg!("clipboard"),
                msg!("copy"),
                msg!("paste"),
                msg!("cliphist"),
            ])
            .build(),
        keys(HISTORY_KEY, "$mod+Shift+v", HISTORY)
            .section(section)
            .title(msg!("Open the history"))
            .subtitle(msg!(
                "In the launcher that is installed; the entry you pick is copied again"
            ))
            .keywords(&[msg!("launcher"), msg!("pick")])
            .build(),
        Setting::integer(MAX_ENTRIES, Page::ScreenshotsClipboard, 750, 10, 10000)
            .section(section)
            .title(msg!("Entries kept"))
            .subtitle(msg!("The oldest entries go when there are more"))
            .slider(10.0, 2000.0, 10.0)
            .depends_on(HISTORY, When::On)
            .keywords(&[msg!("limit"), msg!("size"), msg!("history")])
            .build(),
    ]
}

fn recording() -> Vec<Setting> {
    let section = msg!("Screen recording");
    vec![
        Setting::toggle(RECORDING, Page::ScreenshotsClipboard, false)
            .section(section)
            .title(msg!("Recording key"))
            .subtitle(msg!(
                "Start a recording of the screen with the key below, and stop it with the same \
                 key, with wf-recorder or wl-screenrec"
            ))
            .keywords(&[
                msg!("record"),
                msg!("video"),
                msg!("screencast"),
                msg!("wf-recorder"),
                msg!("wl-screenrec"),
            ])
            .build(),
        keys(RECORDING_KEY, "Ctrl+Alt+Shift+r", RECORDING)
            .section(section)
            .title(msg!("Start and stop"))
            .build(),
        Setting::choice(RECORDING_AREA, Page::ScreenshotsClipboard, "region", AREAS)
            .section(section)
            .title(msg!("What to record"))
            .keywords(&[msg!("area"), msg!("display"), msg!("region")])
            .build(),
        Setting::text(
            RECORDING_FILE_NAME,
            Page::ScreenshotsClipboard,
            DEFAULT_RECORDING_FILE_NAME,
        )
        .section(section)
        .title(msg!("File name"))
        .subtitle(msg!(
            "In the folder of screenshots; .mp4 or .mkv at the end decides the format"
        ))
        .check_text(check_file_name)
        .keywords(&[msg!("pattern"), msg!("name"), msg!("video")])
        .build(),
        Setting::toggle(RECORDING_AUDIO, Page::ScreenshotsClipboard, false)
            .section(section)
            .title(msg!("Record sound"))
            .subtitle(msg!("What the default sound input hears"))
            .keywords(&[msg!("audio"), msg!("microphone"), msg!("sound")])
            .build(),
    ]
}
