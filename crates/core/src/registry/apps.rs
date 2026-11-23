// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Apps & Autostart page: the terminal and launcher the main keys open, the
//! default apps, and the autostart entries, with their keys and the checks of their values.
//!
//! None of them has a sway directive of its own. The terminal and launcher become bindings in
//! `50-bindings.conf` with those of the Keyboard Shortcuts page ([`crate::apps::launchers`]), the
//! default apps Sway Things' own `sway-mimeapps.list` ([`crate::apps::mime`]), the autostart
//! entries and the runner `exec` lines in `70-autostart.conf`, and the switched-off entries
//! overrides with `Hidden=true` ([`crate::apps::autostart`]). Those modules re-export what is
//! defined here; the checks read binding lines with the binding reader (`bindings::keys`,
//! `line`, `command`), which imports neither the registry nor the state, so the registry stays
//! the bottom layer.

use super::{Choice, Msg, Page, Setting, ValueError};
use crate::bindings::keys::{self, Modifiers};
use crate::bindings::{Action, Line, LineError};
use crate::command::{SwayCmd, SwayText};
use crate::msg;

/// The key of the terminal's command.
pub const TERMINAL: &str = "apps.terminal";
/// The key of the keys that open the terminal.
pub const TERMINAL_KEYS: &str = "apps.terminal_keys";
/// The key of the launcher's command.
pub const LAUNCHER: &str = "apps.launcher";
/// The key of the keys that open the launcher.
pub const LAUNCHER_KEYS: &str = "apps.launcher_keys";

/// The keys of the settings whose change changes the bindings.
pub const KEYS: [&str; 4] = [TERMINAL, TERMINAL_KEYS, LAUNCHER, LAUNCHER_KEYS];

/// The key of Sway Things' autostart entries.
pub const ENTRIES: &str = "autostart.entries";
/// The key of the XDG autostart entries Sway Things switches off.
pub const HIDDEN: &str = "autostart.hidden";
/// The key of the runner Sway Things starts.
pub const RUNNER: &str = "autostart.runner";

/// What starts the XDG autostart entries from Sway Things' file, by the id of its choice.
pub const RUNNERS: &[Choice] = &[
    Choice::new("none", msg!("Your session or config")),
    Choice::new("systemd", msg!("systemd")),
    Choice::new("dex", msg!("dex")),
];

/// A kind of default app, with a row on the page and a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Web browser.
    Browser,
    /// File manager.
    FileManager,
    /// Mail.
    Mail,
    /// Image viewer.
    ImageViewer,
    /// PDF viewer.
    PdfViewer,
    /// Video player.
    VideoPlayer,
    /// Music player.
    MusicPlayer,
    /// Text editor.
    TextEditor,
    /// Terminal.
    Terminal,
}

impl Kind {
    /// Every kind, in the order of the rows.
    pub const ALL: [Self; 9] = [
        Self::Browser,
        Self::FileManager,
        Self::Mail,
        Self::ImageViewer,
        Self::PdfViewer,
        Self::VideoPlayer,
        Self::MusicPlayer,
        Self::TextEditor,
        Self::Terminal,
    ];

    /// The key of its setting.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Browser => "apps.default.browser",
            Self::FileManager => "apps.default.file_manager",
            Self::Mail => "apps.default.mail",
            Self::ImageViewer => "apps.default.image_viewer",
            Self::PdfViewer => "apps.default.pdf_viewer",
            Self::VideoPlayer => "apps.default.video_player",
            Self::MusicPlayer => "apps.default.music_player",
            Self::TextEditor => "apps.default.text_editor",
            Self::Terminal => "apps.default.terminal",
        }
    }

    /// The kind whose setting has the key `key`.
    pub fn of(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }

    /// What its row is called.
    pub const fn title(self) -> Msg {
        match self {
            Self::Browser => msg!("Web browser"),
            Self::FileManager => msg!("File manager"),
            Self::Mail => msg!("Mail"),
            Self::ImageViewer => msg!("Image viewer"),
            Self::PdfViewer => msg!("PDF viewer"),
            Self::VideoPlayer => msg!("Video player"),
            Self::MusicPlayer => msg!("Music player"),
            Self::TextEditor => msg!("Text editor"),
            Self::Terminal => msg!("Terminal for apps"),
        }
    }
}

/// Whether `id` can be a desktop file ID Sway Things writes or passes on: a name ending in
/// `.desktop` of ASCII letters, digits, `-`, `_` and `.`, not starting with `-` or `.`. That is
/// what desktop file IDs consist of in practice, and none of these characters means anything to
/// a shell or to sway.
pub fn is_desktop_id(id: &str) -> bool {
    id.len() > ".desktop".len()
        && id.len() <= 255
        && id.ends_with(".desktop")
        && !id.starts_with(['-', '.'])
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// The check of a default app: empty (none chosen) or a desktop file ID.
///
/// # Errors
///
/// [`ValueError::NotADesktopId`].
pub fn check_id(text: &str) -> Result<(), ValueError> {
    if text.is_empty() || is_desktop_id(text) {
        Ok(())
    } else {
        Err(ValueError::NotADesktopId)
    }
}

/// `bindsym <keys> exec <command>`, `$mod` kept in `keys`.
///
/// # Errors
///
/// A [`LineError`] if the keys aren't a plain key combination or the command can't be run by a
/// binding.
pub fn line(keys_text: &str, command: &str) -> Result<Line, LineError> {
    if !keys::is_plain(keys_text) {
        return Err(LineError::NotPlain);
    }
    let resolved = keys::with_mod(keys_text, Modifiers::SUPER);
    let combo = keys::parse(&resolved, false, false).map_err(LineError::Keys)?;
    let command = Action::Run(command.to_owned())
        .command()
        .map_err(LineError::Command)?;
    let line = Line {
        unbind: false,
        code: false,
        flags: crate::bindings::Flags::default(),
        input: None,
        keys: keys_text.to_owned(),
        combo,
        command: Some(command.as_str().to_owned()),
    };
    line.config(Modifiers::SUPER, true)?;
    Ok(line)
}

/// The check of a terminal's or launcher's command, for the registry: empty (none chosen) or a
/// command line a binding can run.
///
/// # Errors
///
/// [`ValueError::NotABinding`] with the reason.
pub fn check_command(text: &str) -> Result<(), ValueError> {
    if text.trim().is_empty() {
        return Ok(());
    }
    line("$mod+Return", text)
        .map(|_| ())
        .map_err(|error| ValueError::NotABinding(error.kind()))
}

/// The check of the keys of a terminal or launcher: a key combination without quotes that
/// leaves typing alone (a key that types needs Super, Ctrl or Alt).
///
/// # Errors
///
/// [`ValueError::NotABinding`] with the reason, [`ValueError::TypingKeys`] for keys that type.
pub fn check_keys(text: &str) -> Result<(), ValueError> {
    let line = line(text.trim(), "true").map_err(|error| ValueError::NotABinding(error.kind()))?;
    if !keys::leaves_typing(&line.combo) {
        return Err(ValueError::TypingKeys);
    }
    Ok(())
}

/// An entry of Sway Things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A command line, run with `sh -c` by sway.
    Exec(String),
    /// An app by its desktop file ID.
    App(String),
}

/// An item of `autostart.entries` read: the entry and whether it is switched on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Whether it starts.
    pub on: bool,
    /// What starts.
    pub item: Item,
}

impl Entry {
    /// Reads an item of `autostart.entries`.
    ///
    /// # Errors
    ///
    /// [`ValueError::NotAnAutostartEntry`] for anything else, [`ValueError::NotADesktopId`] for an
    /// app that isn't a desktop file ID, and the error of [`SwayText`] for a command that
    /// can't be written.
    pub fn parse(text: &str) -> Result<Self, ValueError> {
        let (on, rest) = match text.strip_prefix("# ") {
            Some(rest) => (false, rest),
            None => (true, text),
        };
        let (verb, argument) = rest
            .split_once(' ')
            .ok_or(ValueError::NotAnAutostartEntry)?;
        let argument = argument.trim();
        let item = match verb {
            "exec" => {
                SwayText::new(argument).map_err(ValueError::NotSwayText)?;
                // The line goes to the shell as it is: an option of sway's `exec`
                // (`--no-startup-id`) would be the shell's command.
                if argument.starts_with('-') {
                    return Err(ValueError::NotAnAutostartEntry);
                }
                Item::Exec(argument.to_owned())
            }
            "app" => {
                if !is_desktop_id(argument) {
                    return Err(ValueError::NotADesktopId);
                }
                Item::App(argument.to_owned())
            }
            _ => return Err(ValueError::NotAnAutostartEntry),
        };
        Ok(Self { on, item })
    }

    /// The text of the item, as [`Entry::parse`] reads it.
    pub fn text(&self) -> String {
        let prefix = if self.on { "" } else { "# " };
        match &self.item {
            Item::Exec(line) => format!("{prefix}exec {line}"),
            Item::App(id) => format!("{prefix}app {id}"),
        }
    }

    /// The command line sway runs for it.
    pub fn command_line(&self) -> String {
        match &self.item {
            Item::Exec(line) => line.clone(),
            Item::App(id) => format!("sway-things hook launch {id}"),
        }
    }

    /// The `exec` command that starts it: written to `70-autostart.conf`, or sent to sway to
    /// start it now.
    ///
    /// # Errors
    ///
    /// [`ValueError::NotSwayText`] for a command line sway can't take.
    pub fn command(&self) -> Result<SwayCmd, ValueError> {
        let line = SwayText::new(self.command_line()).map_err(ValueError::NotSwayText)?;
        Ok(SwayCmd::exec(line, false))
    }
}

/// The check of the items of `autostart.entries`, for the registry.
///
/// # Errors
///
/// Those of [`Entry::parse`].
pub fn check_entry(text: &str) -> Result<(), ValueError> {
    Entry::parse(text).map(|_| ())
}

/// The check of the items of `autostart.hidden`: desktop file IDs.
///
/// # Errors
///
/// [`ValueError::NotADesktopId`].
pub fn check_hidden(text: &str) -> Result<(), ValueError> {
    if is_desktop_id(text) {
        Ok(())
    } else {
        Err(ValueError::NotADesktopId)
    }
}

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
    let mut settings = Vec::new();
    for kind in Kind::ALL {
        let mut setting = Setting::text(kind.key(), Page::AppsAutostart, "")
            .section(msg!("Default apps"))
            .title(kind.title())
            .placeholder(msg!("Not chosen here"))
            .check_text(check_id)
            .keywords(keywords(kind));
        if kind == Kind::Terminal {
            setting = setting.subtitle(msg!(
                "Opens the apps that run in a terminal, where xdg-terminal-exec is installed"
            ));
        }
        settings.push(setting.build());
    }
    settings.extend(terminal_and_launcher());
    settings.extend(autostart());
    settings
}

fn terminal_and_launcher() -> Vec<Setting> {
    vec![
        Setting::text(TERMINAL, Page::AppsAutostart, "")
            .section(msg!("Terminal and launcher"))
            .title(msg!("Terminal"))
            .placeholder(msg!("As your config binds it"))
            .check_text(check_command)
            .keywords(&[msg!("$term"), msg!("terminal emulator"), msg!("shell")])
            .build(),
        Setting::text(TERMINAL_KEYS, Page::AppsAutostart, "$mod+Return")
            .section(msg!("Terminal and launcher"))
            .title(msg!("Terminal keys"))
            .check_text(check_keys)
            .keywords(&[msg!("shortcut"), msg!("bindsym")])
            .build(),
        Setting::text(LAUNCHER, Page::AppsAutostart, "")
            .section(msg!("Terminal and launcher"))
            .title(msg!("App launcher"))
            .placeholder(msg!("As your config binds it"))
            .check_text(check_command)
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
            .title(msg!("Launcher keys"))
            .check_text(check_keys)
            .keywords(&[msg!("shortcut"), msg!("bindsym")])
            .build(),
    ]
}

fn autostart() -> [Setting; 3] {
    [
        Setting::choice(RUNNER, Page::AppsAutostart, "none", RUNNERS)
            .section(msg!("Autostart"))
            .title(msg!("Start autostart entries with"))
            .subtitle(msg!("sway doesn't start them by itself"))
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
            .check_text(check_entry)
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
            .check_text(check_hidden)
            .keywords(&[msg!("Hidden"), msg!("disable autostart")])
            .build(),
    ]
}
