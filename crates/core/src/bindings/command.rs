// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command of a binding: what sway runs when the keys are pressed.
//!
//! sway keeps the rest of a `bindsym` line after the keys as one string (`join_args`) and runs
//! it like an IPC message when the keys are pressed (`execute_command`): split at `;` and `,`
//! outside quotes, `$variables` replaced once more, quotes removed except for `exec`. A
//! [`BindCommand`] is that string. It can be written into a config line and sent over IPC so
//! that sway keeps exactly this string, which needs a few things it can't contain (see
//! [`BindCommand::new`]).
//!
//! [`Action`] is what the editor of the Keyboard Shortcuts page offers instead of free text:
//! run a command, focus or move a window, go to a workspace. [`describe`] and [`Category`] say
//! what a command does, for the list of all bindings.

use std::fmt;

use crate::command::SwayText;
use crate::i18n::{Msg, Text};
use crate::msg;
use crate::sway_config::line::{split_args, strip_quotes};

/// The command of a binding as sway keeps it: one or more sway commands.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BindCommand(String);

/// Why a text can't be the command of a binding Sway Things writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The command is empty.
    Empty,
    /// The command contains a control character, which would end the line.
    ControlCharacter,
    /// The command contains a backslash: sway reads escapes in a config line and again when it
    /// runs the command, so it can't be written back exactly.
    Backslash,
    /// The command ends with `{`, which would open a block in a config file.
    OpensBlock,
    /// A quote or a `[` isn't closed.
    Unbalanced,
    /// The first word isn't a command sway knows.
    UnknownCommand(String),
    /// A command to run is empty or holds a character it can't (see [`SwayText`]).
    NotAProgram,
    /// A workspace number of 0, or above what sway takes.
    InvalidWorkspace,
}

impl CommandError {
    /// What is wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Empty => msg!("the command is empty").into(),
            Self::ControlCharacter => msg!("the command can't contain a line break or tab").into(),
            Self::Backslash => msg!("the command can't contain a backslash").into(),
            Self::OpensBlock => msg!("the command can't end with a brace").into(),
            Self::Unbalanced => msg!("a quote or [ in the command isn't closed").into(),
            Self::UnknownCommand(name) => {
                Text::new(msg!("sway has no command {name}")).arg("name", name)
            }
            Self::NotAProgram => {
                msg!("the program to run is empty or contains a backslash or line break").into()
            }
            Self::InvalidWorkspace => msg!("workspace numbers start at 1").into(),
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for CommandError {}

/// The commands sway 1.12 runs at runtime (`handlers` and `command_handlers` in
/// `sway/commands.c`), which a binding can run, and those SwayFX adds.
const SWAY_COMMANDS: &[&str] = &[
    "allow_tearing",
    "assign",
    "bar",
    "bindcode",
    "bindgesture",
    "bindswitch",
    "bindsym",
    "border",
    "client.focused",
    "client.focused_inactive",
    "client.focused_tab_title",
    "client.unfocused",
    "client.urgent",
    "create_output",
    "default_border",
    "default_floating_border",
    "exec",
    "exec_always",
    "exit",
    "floating",
    "floating_maximum_size",
    "floating_minimum_size",
    "floating_modifier",
    "focus",
    "focus_follows_mouse",
    "focus_on_window_activation",
    "focus_wrapping",
    "font",
    "for_window",
    "force_display_urgency_hint",
    "force_focus_wrapping",
    "fullscreen",
    "gaps",
    "hide_edge_borders",
    "inhibit_idle",
    "input",
    "kill",
    "layout",
    "mark",
    "max_render_time",
    "mode",
    "mouse_warping",
    "move",
    "new_float",
    "new_window",
    "no_focus",
    "nop",
    "opacity",
    "output",
    "popup_during_fullscreen",
    "reload",
    "rename",
    "resize",
    "scratchpad",
    "seat",
    "set",
    "shortcuts_inhibitor",
    "show_marks",
    "smart_borders",
    "smart_gaps",
    "split",
    "splith",
    "splitt",
    "splitv",
    "sticky",
    "swap",
    "tiling_drag",
    "tiling_drag_threshold",
    "title_align",
    "title_format",
    "titlebar_border_thickness",
    "titlebar_padding",
    "unbindcode",
    "unbindgesture",
    "unbindswitch",
    "unbindsym",
    "unmark",
    "urgent",
    "workspace",
    "workspace_auto_back_and_forth",
    // SwayFX 0.5 and 0.6.
    "animation_duration_ms",
    "blur",
    "blur_brightness",
    "blur_contrast",
    "blur_noise",
    "blur_passes",
    "blur_radius",
    "blur_saturation",
    "blur_xray",
    "corner_radius",
    "default_dim_inactive",
    "dim_inactive",
    "dim_inactive_colors.unfocused",
    "dim_inactive_colors.urgent",
    "layer_effects",
    "saturation",
    "scratchpad_minimize",
    "shadow_blur_radius",
    "shadow_color",
    "shadow_inactive_color",
    "shadow_offset",
    "shadows",
    "shadows_on_csd",
    "smart_corner_radius",
    "titlebar_separator",
];

impl BindCommand {
    /// The command `text`, with blanks between its words as sway keeps them: one space, except
    /// inside quotes.
    ///
    /// # Errors
    ///
    /// A [`CommandError`] if `text` is empty, contains a control character or a backslash, or
    /// ends with `{`. Such a command can't be written into a config line so that sway reads it
    /// back unchanged.
    pub fn new(text: &str) -> Result<Self, CommandError> {
        if text.chars().any(char::is_control) {
            return Err(CommandError::ControlCharacter);
        }
        if text.contains('\\') {
            return Err(CommandError::Backslash);
        }
        if !balanced(text) {
            return Err(CommandError::Unbalanced);
        }
        let words = split_args(text.as_bytes());
        let joined = String::from_utf8_lossy(&words.join(&b' ')).into_owned();
        if joined.is_empty() {
            return Err(CommandError::Empty);
        }
        if joined.ends_with('{') || joined.ends_with('}') {
            return Err(CommandError::OpensBlock);
        }
        Ok(Self(joined))
    }

    /// [`BindCommand::new`], and checks that each command starts with a command sway or SwayFX
    /// knows, so a typo doesn't make a binding that does nothing. Every command Sway Things
    /// writes is checked so; commands of the user's config are read as they are.
    ///
    /// # Errors
    ///
    /// Those of [`BindCommand::new`], and [`CommandError::UnknownCommand`].
    pub fn checked(text: &str) -> Result<Self, CommandError> {
        let command = Self::new(text)?;
        for part in command.parts() {
            let Some(name) = part.first() else { continue };
            if !SWAY_COMMANDS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(name))
            {
                return Err(CommandError::UnknownCommand(name.clone()));
            }
        }
        Ok(command)
    }

    /// The command as sway keeps it.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The command in a config line: sway replaces the variables of the line before it keeps
    /// the command, so every `$` is doubled.
    pub(crate) fn config_text(&self) -> String {
        self.0.replace('$', "$$")
    }

    /// The command as one argument of an IPC message: quoted, so that sway neither splits it
    /// at `;` nor replaces its variables before it keeps it.
    pub(crate) fn ipc_text(&self) -> String {
        // A command has no control characters and no backslashes, so it is valid text.
        SwayText::new(self.0.clone()).map_or_else(|_| self.0.clone(), |text| text.render_ipc())
    }

    /// The commands of the binding, each as its words without quotes, leading criteria left
    /// out: sway splits the command at `;` and `,` outside quotes.
    fn parts(&self) -> Vec<Vec<String>> {
        split_commands(&self.0)
            .into_iter()
            .map(|command| {
                let command = skip_criteria(command);
                split_args(command.as_bytes())
                    .iter()
                    .map(|word| {
                        let word = if matches!(word.first(), Some(b'"' | b'\'')) {
                            strip_quotes(word)
                        } else {
                            word.clone()
                        };
                        String::from_utf8_lossy(&word).into_owned()
                    })
                    .collect()
            })
            .filter(|words: &Vec<String>| !words.is_empty())
            .collect()
    }
}

impl fmt::Display for BindCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether every quote and `[` of `text` is closed, as sway's `split_args` reads them.
fn balanced(text: &str) -> bool {
    let (mut in_string, mut in_char, mut in_brackets) = (false, false, false);
    for character in text.chars() {
        match character {
            '"' if !in_char => in_string = !in_string,
            '\'' if !in_string => in_char = !in_char,
            '[' if !in_string && !in_char => in_brackets = true,
            ']' if !in_string && !in_char => in_brackets = false,
            _ => {}
        }
    }
    !in_string && !in_char && !in_brackets
}

/// `text` split at `;` and `,` outside quotes, like sway's `argsep`.
fn split_commands(text: &str) -> Vec<&str> {
    let mut commands = Vec::new();
    let (mut in_string, mut in_char) = (false, false);
    let mut start = 0;
    for (index, character) in text.char_indices() {
        match character {
            '"' if !in_char => in_string = !in_string,
            '\'' if !in_string => in_char = !in_char,
            ';' | ',' if !in_string && !in_char => {
                commands.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    commands.push(text[start..].trim());
    commands
}

/// `command` without the criteria it starts with (`[app_id="foot"] focus`).
fn skip_criteria(command: &str) -> &str {
    if !command.starts_with('[') {
        return command;
    }
    let mut in_string = false;
    for (index, character) in command.char_indices() {
        match character {
            '"' => in_string = !in_string,
            ']' if !in_string => return command[index + 1..].trim_start(),
            _ => {}
        }
    }
    ""
}

/// A direction to focus or move a window in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// `left`
    Left,
    /// `right`
    Right,
    /// `up`
    Up,
    /// `down`
    Down,
}

impl Direction {
    /// Every direction.
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Up, Self::Down];

    /// The word sway takes.
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }

    fn from_keyword(word: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|direction| direction.keyword().eq_ignore_ascii_case(word))
    }
}

/// The largest workspace number the editor offers; sway reads numbers up to `INT_MAX`.
pub const MAX_WORKSPACE: u32 = 999;

/// What a binding does, as the editor offers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// `exec <command line>`: runs a program (an app, a script) through `sh -c`.
    Run(String),
    /// `focus <direction>`
    Focus(Direction),
    /// `move <direction>`
    Move(Direction),
    /// `workspace number <n>`
    Workspace(u32),
    /// `move container to workspace number <n>`
    MoveToWorkspace(u32),
    /// `kill`: closes the focused window.
    Close,
    /// `fullscreen toggle`
    Fullscreen,
    /// `floating toggle`
    Floating,
    /// Any other sway command, as typed.
    Sway(String),
}

impl Action {
    /// The command of a binding that does this.
    ///
    /// # Errors
    ///
    /// [`CommandError::NotAProgram`] for a command line that is empty or has a backslash or a
    /// control character, [`CommandError::InvalidWorkspace`] for a workspace number of 0 or
    /// above [`MAX_WORKSPACE`], and those of [`BindCommand::checked`] for a sway command.
    pub fn command(&self) -> Result<BindCommand, CommandError> {
        let workspace = |number: u32| {
            if (1..=MAX_WORKSPACE).contains(&number) {
                Ok(number)
            } else {
                Err(CommandError::InvalidWorkspace)
            }
        };
        let text = match self {
            Self::Run(line) => {
                let line = SwayText::new(line.trim()).map_err(|_| CommandError::NotAProgram)?;
                // sway runs a single quoted argument of `exec` as it is, without the quotes.
                format!("exec {}", line.render_ipc())
            }
            Self::Focus(direction) => format!("focus {}", direction.keyword()),
            Self::Move(direction) => format!("move {}", direction.keyword()),
            Self::Workspace(number) => format!("workspace number {}", workspace(*number)?),
            Self::MoveToWorkspace(number) => {
                format!("move container to workspace number {}", workspace(*number)?)
            }
            Self::Close => "kill".to_owned(),
            Self::Fullscreen => "fullscreen toggle".to_owned(),
            Self::Floating => "floating toggle".to_owned(),
            Self::Sway(text) => return BindCommand::checked(text),
        };
        BindCommand::new(&text)
    }

    /// What `command` does, as the editor shows it: one of the actions it offers, else
    /// [`Action::Sway`] with the command.
    pub fn of(command: &BindCommand) -> Self {
        let parts = command.parts();
        let sway = || Self::Sway(command.as_str().to_owned());
        let ([words], false) = (parts.as_slice(), command.as_str().starts_with('[')) else {
            return sway();
        };
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let lower: Vec<String> = words.iter().map(|word| word.to_ascii_lowercase()).collect();
        let lower: Vec<&str> = lower.iter().map(String::as_str).collect();
        match lower.as_slice() {
            ["exec", ..] if words.len() > 1 => Self::Run(exec_line(command.as_str())),
            ["focus", direction] => {
                Direction::from_keyword(direction).map_or_else(sway, Self::Focus)
            }
            ["move", direction] => Direction::from_keyword(direction).map_or_else(sway, Self::Move),
            ["workspace", "number", number] => number
                .parse()
                .ok()
                .filter(|n| (1..=MAX_WORKSPACE).contains(n))
                .map_or_else(sway, Self::Workspace),
            [
                "move",
                "container" | "window",
                "to",
                "workspace",
                "number",
                number,
            ]
            | ["move", "to", "workspace", "number", number] => number
                .parse()
                .ok()
                .filter(|n| (1..=MAX_WORKSPACE).contains(n))
                .map_or_else(sway, Self::MoveToWorkspace),
            ["kill"] => Self::Close,
            ["fullscreen", "toggle"] | ["fullscreen"] => Self::Fullscreen,
            ["floating", "toggle"] => Self::Floating,
            _ => sway(),
        }
    }
}

/// The command line `exec` runs, from the command of a binding that starts with `exec`: a
/// single quoted argument without its quotes, as sway runs it, else the words after `exec`.
fn exec_line(command: &str) -> String {
    let words = split_args(command.as_bytes());
    let rest: Vec<&Vec<u8>> = words
        .iter()
        .skip(1)
        .skip_while(|word| word.as_slice() == b"--no-startup-id")
        .collect();
    let line = match rest.as_slice() {
        [single] if matches!(single.first(), Some(b'"' | b'\'')) => strip_quotes(single),
        _ => rest
            .iter()
            .map(|word| word.as_slice())
            .collect::<Vec<_>>()
            .join(&b' '),
    };
    // sway replaces the variables once more when it runs the command.
    String::from_utf8_lossy(&line).replace("$$", "$")
}

/// What kind of thing a binding does, for grouping the list of bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    /// Focus, move, close, resize, lay out windows.
    Windows,
    /// Go to or move to workspaces.
    Workspaces,
    /// Run programs.
    Apps,
    /// Volume, brightness, media players.
    Media,
    /// Reload, exit, lock, modes, outputs.
    System,
    /// Everything else.
    Other,
}

impl Category {
    /// Every category, in the order of the list.
    pub const ALL: [Self; 6] = [
        Self::Windows,
        Self::Workspaces,
        Self::Apps,
        Self::Media,
        Self::System,
        Self::Other,
    ];

    /// The heading.
    pub const fn title(self) -> Msg {
        match self {
            Self::Windows => msg!("Windows"),
            Self::Workspaces => msg!("Workspaces"),
            Self::Apps => msg!("Apps"),
            Self::Media => msg!("Media"),
            Self::System => msg!("System"),
            Self::Other => msg!("Other"),
        }
    }

    /// The category of `command`, by its first command.
    pub fn of(command: &str) -> Self {
        let Ok(command) = BindCommand::new(command) else {
            return Self::Other;
        };
        let parts = command.parts();
        let Some(words) = parts.first() else {
            return Self::Other;
        };
        let lower: Vec<String> = words.iter().map(|word| word.to_ascii_lowercase()).collect();
        let first = lower.first().map_or("", String::as_str);
        match first {
            "exec" | "exec_always" => exec_category(&exec_line(command.as_str())),
            "workspace" | "rename" => Self::Workspaces,
            "move" if lower.iter().any(|word| word == "workspace") => Self::Workspaces,
            "focus" if lower.get(1).is_some_and(|word| word == "output") => Self::Workspaces,
            "kill" | "focus" | "move" | "floating" | "fullscreen" | "layout" | "split"
            | "splith" | "splitv" | "splitt" | "resize" | "sticky" | "border" | "scratchpad"
            | "swap" | "mark" | "unmark" | "opacity" | "title_format" | "inhibit_idle"
            | "urgent" => Self::Windows,
            "reload"
            | "exit"
            | "mode"
            | "output"
            | "input"
            | "seat"
            | "bar"
            | "shortcuts_inhibitor"
            | "create_output" => Self::System,
            _ => Self::Other,
        }
    }
}

/// The category of a binding that runs `line`.
fn exec_category(line: &str) -> Category {
    let program = line
        .split_whitespace()
        .find(|word| !word.contains('='))
        .unwrap_or("");
    let program = program.rsplit('/').next().unwrap_or(program);
    match program {
        "wpctl" | "pactl" | "pamixer" | "amixer" | "brightnessctl" | "light" | "playerctl"
        | "swayosd-client" | "volumectl" | "lightctl" => Category::Media,
        "swaylock" | "swaynag" | "systemctl" | "loginctl" | "wlogout" | "swaymsg" | "nwg-bar"
        | "gtklock" | "waylock" => Category::System,
        _ => Category::Apps,
    }
}

/// What `command` does, in words, for the commands people bind most; `None` for others, which
/// show as they are.
pub fn describe(command: &str) -> Option<Text> {
    let command = BindCommand::new(command).ok()?;
    let parts = command.parts();
    if parts.len() != 1 || command.as_str().starts_with('[') {
        return None;
    }
    if let Action::Run(line) = Action::of(&command) {
        return Some(describe_run(&line).map_or_else(
            || Text::new(msg!("Run {command}")).arg("command", line),
            Text::from,
        ));
    }
    let words: Vec<String> = parts
        .first()?
        .iter()
        .map(|word| word.to_ascii_lowercase())
        .collect();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let text = match words.as_slice() {
        ["kill"] => msg!("Close the window").into(),
        ["fullscreen"] | ["fullscreen", "toggle"] => msg!("Toggle fullscreen").into(),
        ["floating", "toggle"] => msg!("Toggle floating").into(),
        ["focus", "mode_toggle"] => msg!("Switch the focus between tiling and floating").into(),
        ["focus", "parent"] => msg!("Focus the parent container").into(),
        ["focus", "child"] => msg!("Focus the child container").into(),
        ["move", "scratchpad"] | ["move", "container" | "window", "to", "scratchpad"] => {
            msg!("Move the window to the scratchpad").into()
        }
        ["focus", direction] => direction_text(direction, true)?,
        ["move", direction] => direction_text(direction, false)?,
        ["workspace", "number", name] | ["workspace", name]
            if !matches!(*name, "next" | "prev" | "back_and_forth" | "next_on_output")
                && *name != "prev_on_output" =>
        {
            Text::new(msg!("Go to workspace {name}")).arg("name", parts.first()?.last()?)
        }
        ["workspace", "next" | "next_on_output"] => msg!("Go to the next workspace").into(),
        ["workspace", "prev" | "prev_on_output"] => msg!("Go to the previous workspace").into(),
        ["workspace", "back_and_forth"] => msg!("Go back to the last workspace").into(),
        ["move", "container" | "window", "to", "workspace", .., _]
        | ["move", "to", "workspace", .., _] => {
            Text::new(msg!("Move the window to workspace {name}"))
                .arg("name", parts.first()?.last()?)
        }
        ["scratchpad", "show"] => msg!("Show the scratchpad").into(),
        ["layout", "stacking"] => msg!("Stacked layout").into(),
        ["layout", "tabbed"] => msg!("Tabbed layout").into(),
        ["layout", "toggle", "split"] => msg!("Toggle the split layout").into(),
        ["splith"] | ["split", "h" | "horizontal"] => msg!("Split horizontally").into(),
        ["splitv"] | ["split", "v" | "vertical"] => msg!("Split vertically").into(),
        ["resize", "shrink", "width", ..] => msg!("Make the window narrower").into(),
        ["resize", "grow", "width", ..] => msg!("Make the window wider").into(),
        ["resize", "shrink", "height", ..] => msg!("Make the window shorter").into(),
        ["resize", "grow", "height", ..] => msg!("Make the window taller").into(),
        ["reload"] => msg!("Reload the sway config").into(),
        ["exit"] => msg!("Exit sway").into(),
        ["mode", "default"] => msg!("Leave the mode").into(),
        ["mode", ..] => {
            Text::new(msg!("Enter the mode {mode}")).arg("mode", parts.first()?.last()?)
        }
        _ => return None,
    };
    Some(text)
}

/// What the command line `line` of `exec` does, in words, for the programs people bind most to
/// media keys, the lock screen, screenshots and leaving sway; `None` for others.
fn describe_run(line: &str) -> Option<Msg> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let program = words.first()?.rsplit('/').next()?;
    let has = |word: &str| words.contains(&word);
    // The step of a volume or brightness: "5%+" and "+5%" go up, "5%-" and "-5%" down.
    let step = words.last().and_then(|last| {
        let digit_after = |sign: char| {
            last.strip_prefix(sign)
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        };
        if last.ends_with('+') || digit_after('+') {
            Some(true)
        } else if last.ends_with('-') || digit_after('-') {
            Some(false)
        } else {
            None
        }
    });
    let volume = |step: Option<bool>| {
        Some(if step? {
            msg!("Raise the volume")
        } else {
            msg!("Lower the volume")
        })
    };
    Some(match program {
        "wpctl" if has("set-volume") => volume(step)?,
        "pactl" if has("set-sink-volume") => volume(step)?,
        "wpctl" if has("set-mute") && line.contains("SOURCE") => {
            msg!("Mute or unmute the microphone")
        }
        "pactl" if has("set-source-mute") => msg!("Mute or unmute the microphone"),
        "wpctl" if has("set-mute") => msg!("Mute or unmute the sound"),
        "pactl" if has("set-sink-mute") => msg!("Mute or unmute the sound"),
        "brightnessctl" if has("set") => {
            if step? {
                msg!("Brighten the screen")
            } else {
                msg!("Dim the screen")
            }
        }
        "playerctl" => match words.get(1).copied()? {
            "play-pause" | "play" | "pause" => msg!("Play or pause"),
            "next" => msg!("Next track"),
            "previous" => msg!("Previous track"),
            "stop" => msg!("Stop playing"),
            _ => return None,
        },
        "swayosd-client" => describe_osd(&words[1..])?,
        "swaylock" | "gtklock" | "waylock" => msg!("Lock the screen"),
        "swaynag" if line.contains("swaymsg exit") => msg!("Exit sway, after asking"),
        "grim" if line.contains("slurp") => msg!("Take a screenshot of an area"),
        "grim" => msg!("Take a screenshot"),
        _ => return None,
    })
}

/// What `swayosd-client` does with the arguments `args`, in words: the volume, the brightness or
/// the player, with the on-screen display it shows. `--flag value` and `--flag=value` both count;
/// a number is a step (`+5` or `5` up, `-5` down).
fn describe_osd(args: &[&str]) -> Option<Msg> {
    let mut action = None;
    let mut words = args.iter();
    while let Some(word) = words.next() {
        let (flag, value) = match word.split_once('=') {
            Some((flag, value)) => (flag, value),
            None => (*word, *words.next().unwrap_or(&"")),
        };
        if matches!(
            flag,
            "--output-volume" | "--input-volume" | "--brightness" | "--playerctl"
        ) {
            action = Some((flag, value));
        }
    }
    let (flag, value) = action?;
    let up = match value {
        "raise" => Some(true),
        "lower" => Some(false),
        number => number
            .parse::<i32>()
            .ok()
            .filter(|step| *step != 0)
            .map(|step| step > 0),
    };
    Some(match (flag, value, up) {
        ("--output-volume", _, Some(true)) => msg!("Raise the volume, with an on-screen display"),
        ("--output-volume", _, Some(false)) => msg!("Lower the volume, with an on-screen display"),
        ("--output-volume", "mute-toggle" | "mute" | "unmute", _) => {
            msg!("Mute or unmute the sound, with an on-screen display")
        }
        ("--input-volume", _, Some(true)) => {
            msg!("Raise the microphone volume, with an on-screen display")
        }
        ("--input-volume", _, Some(false)) => {
            msg!("Lower the microphone volume, with an on-screen display")
        }
        ("--input-volume", "mute-toggle" | "mute" | "unmute", _) => {
            msg!("Mute or unmute the microphone, with an on-screen display")
        }
        ("--brightness", _, Some(true)) if value.starts_with('+') || value == "raise" => {
            msg!("Brighten the screen, with an on-screen display")
        }
        ("--brightness", _, Some(false)) => msg!("Dim the screen, with an on-screen display"),
        ("--playerctl", "play-pause" | "play" | "pause", _) => {
            msg!("Play or pause, with an on-screen display")
        }
        ("--playerctl", "next", _) => msg!("Next track, with an on-screen display"),
        ("--playerctl", "prev" | "previous", _) => {
            msg!("Previous track, with an on-screen display")
        }
        ("--playerctl", "stop", _) => msg!("Stop playing, with an on-screen display"),
        _ => return None,
    })
}

/// "Focus the window to the left" (`focus` true) or "Move the window left".
fn direction_text(word: &str, focus: bool) -> Option<Text> {
    let direction = Direction::from_keyword(word)?;
    Some(
        match (focus, direction) {
            (true, Direction::Left) => msg!("Focus the window to the left"),
            (true, Direction::Right) => msg!("Focus the window to the right"),
            (true, Direction::Up) => msg!("Focus the window above"),
            (true, Direction::Down) => msg!("Focus the window below"),
            (false, Direction::Left) => msg!("Move the window left"),
            (false, Direction::Right) => msg!("Move the window right"),
            (false, Direction::Up) => msg!("Move the window up"),
            (false, Direction::Down) => msg!("Move the window down"),
        }
        .into(),
    )
}
