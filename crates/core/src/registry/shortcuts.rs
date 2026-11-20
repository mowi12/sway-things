// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Keyboard Shortcuts page: the bindings Sway Things adds, removes or changes,
//! the main modifier they use, and the groups of media keys.
//!
//! None of them has a sway directive of its own: they all go to `50-bindings.conf` as binding
//! lines ([`crate::bindings::ours`]), which the applier sends to a running sway as the
//! difference to what it has ([`crate::bindings::live`]).

use super::{Apply, Choice, Page, Setting, ValueError};
use crate::msg;

/// The key of the main modifier of the bindings Sway Things writes.
pub const MODIFIER: &str = "shortcuts.modifier";
/// The key of the bindings Sway Things writes and the user's it removes or changes.
pub const BINDINGS: &str = "shortcuts.bindings";
/// The key of the switch of the volume keys.
pub const VOLUME: &str = "shortcuts.media.volume";
/// The key of the switch of the brightness keys.
pub const BRIGHTNESS: &str = "shortcuts.media.brightness";
/// The key of the switch of the media player keys.
pub const PLAYER: &str = "shortcuts.media.player";

/// The main modifiers, by the id of the choice.
pub const MODIFIERS: &[Choice] = &[
    Choice::new("super", msg!("Super (the logo key)")),
    Choice::new("alt", msg!("Alt")),
];

/// The keys of every setting of the page.
pub const KEYS: [&str; 5] = [BINDINGS, VOLUME, BRIGHTNESS, PLAYER, MODIFIER];

/// The check of the items of `shortcuts.bindings`: what can be read at all. A command sway
/// doesn't know passes here, so one typo in `settings.toml` doesn't drop every binding; its item
/// becomes a comment of the generated file, which `doctor` reports, and `sway-things set` and the
/// app refuse it ([`parse_strict`](crate::bindings::ours::parse_strict)).
///
/// The items are read by `bindings::item`, which imports neither the registry nor the
/// state, so the registry stays the bottom layer.
///
/// # Errors
///
/// [`ValueError::NotABinding`] with the reason of [`parse`](crate::bindings::ours::parse).
pub fn check_item(text: &str) -> Result<(), ValueError> {
    crate::bindings::item::parse(text)
        .map(|_| ())
        .map_err(|error| ValueError::NotABinding(error.kind()))
}

pub(super) fn settings() -> Vec<Setting> {
    vec![
        Setting::list(BINDINGS, Page::KeyboardShortcuts, &[])
            .section(msg!("Shortcuts"))
            .title(msg!("Shortcuts"))
            .subtitle(msg!(
                "The bindings Sway Things adds, and the bindings of your config it removes or \
                 changes, as sway lines: bindsym $mod+Return exec foot, unbindsym Mod4+d"
            ))
            .check_text(check_item)
            .apply(Apply::Reload)
            .keywords(&[
                msg!("bindsym"),
                msg!("key binding"),
                msg!("hotkey"),
                msg!("keyboard shortcut"),
            ])
            .build(),
        Setting::toggle(VOLUME, Page::KeyboardShortcuts, false)
            .section(msg!("Media keys"))
            .title(msg!("Volume keys"))
            .subtitle(msg!(
                "Volume up and down, mute and microphone mute with wpctl, also on the lock screen"
            ))
            .apply(Apply::Reload)
            .requires_tools(&["wpctl"])
            .keywords(&[msg!("volume"), msg!("mute"), msg!("speaker"), msg!("audio")])
            .build(),
        Setting::toggle(BRIGHTNESS, Page::KeyboardShortcuts, false)
            .section(msg!("Media keys"))
            .title(msg!("Brightness keys"))
            .subtitle(msg!(
                "Screen brightness up and down with brightnessctl, also on the lock screen"
            ))
            .apply(Apply::Reload)
            .requires_tools(&["brightnessctl"])
            .keywords(&[msg!("brightness"), msg!("backlight"), msg!("screen")])
            .build(),
        Setting::toggle(PLAYER, Page::KeyboardShortcuts, false)
            .section(msg!("Media keys"))
            .title(msg!("Media player keys"))
            .subtitle(msg!(
                "Play, pause, next and previous track with playerctl, also on the lock screen"
            ))
            .apply(Apply::Reload)
            .requires_tools(&["playerctl"])
            .keywords(&[msg!("play"), msg!("pause"), msg!("music"), msg!("track")])
            .build(),
        Setting::choice(MODIFIER, Page::KeyboardShortcuts, "super", MODIFIERS)
            .section(msg!("Main modifier"))
            .title(msg!("Main modifier ($mod)"))
            .subtitle(msg!(
                "The key the shortcuts Sway Things adds start with. Your config's own lines keep \
                 the $mod they were read with: sway replaces a variable when it reads a line"
            ))
            .apply(Apply::Reload)
            .keywords(&[
                msg!("mod key"),
                msg!("super"),
                msg!("windows key"),
                msg!("logo key"),
            ])
            .build(),
    ]
}
