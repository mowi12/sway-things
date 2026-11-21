// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Apps & Autostart page: the terminal and launcher the main keys open, the
//! default apps, and the autostart entries.
//!
//! None of them has a sway directive of its own. The terminal and launcher become bindings in
//! `50-bindings.conf` with those of the Keyboard Shortcuts page ([`crate::apps::launchers`]), the
//! default apps Sway Things' own `sway-mimeapps.list` ([`crate::apps::mime`]), the autostart
//! entries and the runner `exec` lines in `70-autostart.conf`, and the switched-off entries
//! overrides with `Hidden=true` ([`crate::apps::autostart`]).

use super::{Msg, Page, Setting};
use crate::apps::autostart::{self, ENTRIES, HIDDEN, RUNNER, RUNNERS};
use crate::apps::launchers::{self, LAUNCHER, LAUNCHER_KEYS, TERMINAL, TERMINAL_KEYS};
use crate::apps::mime::{self, Kind};
use crate::msg;

/// The keywords of each kind of default app.
const fn keywords(kind: Kind) -> &'static [Msg] {
    match kind {
        Kind::Browser => &[
            msg!("default browser"),
            msg!("web"),
            msg!("links"),
            msg!("firefox"),
            msg!("chromium"),
        ],
        Kind::FileManager => &[msg!("folders"), msg!("files"), msg!("directory")],
        Kind::Mail => &[msg!("email"), msg!("mailto")],
        Kind::ImageViewer => &[msg!("pictures"), msg!("photos"), msg!("images")],
        Kind::PdfViewer => &[msg!("documents"), msg!("pdf")],
        Kind::VideoPlayer => &[msg!("movies"), msg!("videos")],
        Kind::MusicPlayer => &[msg!("audio"), msg!("songs"), msg!("music")],
        Kind::TextEditor => &[msg!("text files"), msg!("notes")],
        Kind::Terminal => &[msg!("xdg-terminal-exec"), msg!("console")],
    }
}

pub(super) fn settings() -> Vec<Setting> {
    let mut settings = terminal_and_launcher();
    for kind in Kind::ALL {
        settings.push(
            Setting::text(kind.key(), Page::AppsAutostart, "")
                .section(msg!("Default apps"))
                .title(kind.title())
                .placeholder(msg!("Not chosen here"))
                .check_text(mime::check_id)
                .keywords(keywords(kind))
                .build(),
        );
    }
    settings.extend(autostart());
    settings
}

fn terminal_and_launcher() -> Vec<Setting> {
    vec![
        Setting::text(TERMINAL, Page::AppsAutostart, "")
            .section(msg!("Terminal and launcher"))
            .title(msg!("Terminal"))
            .subtitle(msg!(
                "What its keys open. Your config's $term only reaches its own lines, so Sway \
                 Things binds the keys itself once you choose"
            ))
            .placeholder(msg!("As your config binds it"))
            .check_text(launchers::check_command)
            .keywords(&[msg!("$term"), msg!("terminal emulator"), msg!("shell")])
            .build(),
        Setting::text(TERMINAL_KEYS, Page::AppsAutostart, "$mod+Return")
            .section(msg!("Terminal and launcher"))
            .title(msg!("Keys of the terminal"))
            .subtitle(msg!("$mod is the main modifier of Keyboard Shortcuts"))
            .check_text(launchers::check_keys)
            .keywords(&[msg!("shortcut"), msg!("bindsym")])
            .build(),
        Setting::text(LAUNCHER, Page::AppsAutostart, "")
            .section(msg!("Terminal and launcher"))
            .title(msg!("Launcher"))
            .subtitle(msg!(
                "The app menu its keys open. Your config's $menu only reaches its own lines"
            ))
            .placeholder(msg!("As your config binds it"))
            .check_text(launchers::check_command)
            .keywords(&[
                msg!("$menu"),
                msg!("app menu"),
                msg!("run dialog"),
                msg!("fuzzel"),
                msg!("wofi"),
                msg!("rofi"),
            ])
            .build(),
        Setting::text(LAUNCHER_KEYS, Page::AppsAutostart, "$mod+d")
            .section(msg!("Terminal and launcher"))
            .title(msg!("Keys of the launcher"))
            .subtitle(msg!("$mod is the main modifier of Keyboard Shortcuts"))
            .check_text(launchers::check_keys)
            .keywords(&[msg!("shortcut"), msg!("bindsym")])
            .build(),
    ]
}

fn autostart() -> [Setting; 3] {
    [
        Setting::choice(RUNNER, Page::AppsAutostart, "none", RUNNERS)
            .section(msg!("Autostart"))
            .title(msg!("Start XDG autostart entries with"))
            .subtitle(msg!(
                "sway doesn't start the entries of ~/.config/autostart and /etc/xdg/autostart \
                 itself. Takes effect at the next login"
            ))
            .keywords(&[
                msg!("dex"),
                msg!("xdg-desktop-autostart.target"),
                msg!("startup"),
                msg!("login"),
            ])
            .build(),
        Setting::list(ENTRIES, Page::AppsAutostart, &[])
            .section(msg!("Autostart"))
            .title(msg!("Autostart entries"))
            .subtitle(msg!(
                "What Sway Things starts at login: exec and a command line, or app and a desktop \
                 file ID; # in front switches one off"
            ))
            .check_text(autostart::check_entry)
            .keywords(&[
                msg!("exec"),
                msg!("startup"),
                msg!("login"),
                msg!("start automatically"),
            ])
            .build(),
        Setting::list(HIDDEN, Page::AppsAutostart, &[])
            .section(msg!("Autostart"))
            .title(msg!("Switched-off XDG autostart entries"))
            .subtitle(msg!(
                "System autostart entries Sway Things switches off with its own file in \
                 ~/.config/autostart"
            ))
            .check_text(autostart::check_hidden)
            .keywords(&[msg!("Hidden"), msg!("disable autostart")])
            .build(),
    ]
}
