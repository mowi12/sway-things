// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builders for swaybar's `bar` commands, written on one line with the bar's ID:
//! `bar sway-things position top`.
//!
//! sway's `cmd_bar` takes a first argument that isn't a subcommand as the ID of a bar, selects the
//! bar with that ID or makes it, and runs the rest on it (`sway/commands/bar.c`, 1.10 and 1.12).
//! While the config is read, a bar made that way stays the current one for the lines after it,
//! and a `bar {` block of the user's resets that. Over IPC the same commands change the running
//! bar, or make it and start swaybar for it.
//!
//! ```
//! use sway_things_core::command::{bar, Target};
//!
//! assert_eq!(bar::position(bar::Position::Top).render(Target::Config)?, "bar sway-things position top");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::{Color, SwayCmd, SwayText, TextError};

/// The ID of the bar Sway Things sets up.
pub const ID: &str = "sway-things";

/// The ID of the bar config waybar follows over IPC while it shows only while a key is held
/// (`"ipc": true` and this `"id"` in its config). The bar config runs no swaybar of its own
/// ([`waybar_hold`]).
pub const WAYBAR_ID: &str = "sway-things-waybar";

fn bar(subcommand: &'static str) -> SwayCmd {
    bar_of(ID, subcommand)
}

fn bar_of(id: &'static str, subcommand: &'static str) -> SwayCmd {
    SwayCmd::new("bar").keyword(id).keyword(subcommand)
}

/// The key that shows a bar while it is held, as sway's `modifier` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    /// `Mod4`: the Super (logo) key, sway's default.
    Super,
    /// `Mod1`: Alt.
    Alt,
    /// `Control`.
    Control,
    /// `Shift`.
    Shift,
}

impl Modifier {
    /// The modifier with sway's name `name` (`Mod4`, `Mod1`, `Control`, `Shift`).
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Mod4" => Some(Self::Super),
            "Mod1" => Some(Self::Alt),
            "Control" => Some(Self::Control),
            "Shift" => Some(Self::Shift),
            _ => None,
        }
    }

    fn keyword(self) -> &'static str {
        match self {
            Self::Super => "Mod4",
            Self::Alt => "Mod1",
            Self::Control => "Control",
            Self::Shift => "Shift",
        }
    }
}

/// `bar sway-things mode hide`: shown only while the modifier is held (or a workspace is
/// urgent), lying over the windows then.
pub fn mode_hide() -> SwayCmd {
    bar("mode").keyword("hide")
}

/// `bar sway-things modifier <key>`: the key that shows a hidden bar while held.
pub fn modifier(key: Modifier) -> SwayCmd {
    bar("modifier").keyword(key.keyword())
}

/// The config lines of the bar waybar follows while it shows only while `key` is held: a bar
/// config with the ID [`WAYBAR_ID`] whose `swaybar_command` is `true`, so sway starts no
/// swaybar for it, in `hide` mode with the modifier. Only for the config: `swaybar_command` is
/// config-only, and over IPC the first line would make the bar and start a swaybar.
pub fn waybar_hold(key: Modifier) -> [SwayCmd; 3] {
    [
        bar_of(WAYBAR_ID, "swaybar_command").keyword("true"),
        bar_of(WAYBAR_ID, "mode").keyword("hide"),
        bar_of(WAYBAR_ID, "modifier").keyword(key.keyword()),
    ]
}

/// `bar <id> swaybar_command sway-things`: the user's bar `id` runs Sway Things' waybar (sway
/// runs `sway-things -b <id>`). Only for the config: `swaybar_command` is config-only.
///
/// # Errors
///
/// [`TextError`] for an ID that can't be sway text.
pub fn take_over(id: &str) -> Result<SwayCmd, TextError> {
    Ok(SwayCmd::new("bar")
        .text(SwayText::new(id)?)
        .keyword("swaybar_command")
        .keyword(crate::bar::takeover::LAUNCHER)
        .config_only())
}

/// `bar <id> swaybar_command true`: the user's bar `id` shows nothing, while another one of theirs
/// shows Sway Things' waybar.
///
/// # Errors
///
/// [`TextError`] for an ID that can't be sway text.
pub fn silence(id: &str) -> Result<SwayCmd, TextError> {
    Ok(SwayCmd::new("bar")
        .text(SwayText::new(id)?)
        .keyword("swaybar_command")
        .keyword("true")
        .config_only())
}

/// Where swaybar stands: it takes only the top or the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// `top`
    Top,
    /// `bottom`
    Bottom,
}

/// `bar sway-things position top|bottom`.
pub fn position(position: Position) -> SwayCmd {
    bar("position").keyword(match position {
        Position::Top => "top",
        Position::Bottom => "bottom",
    })
}

/// `bar sway-things mode dock|invisible`: shown, or hidden (the bar that is handed back keeps
/// running until sway reloads, invisible).
pub fn mode(visible: bool) -> SwayCmd {
    bar("mode").keyword(if visible { "dock" } else { "invisible" })
}

/// `bar sway-things height <px>`; 0 fits the bar to its font. sway reads it with `atoi`.
pub fn height(px: u16) -> SwayCmd {
    bar("height").int(i64::from(px))
}

/// `bar sway-things output <name>`: one display that shows the bar; each line adds one.
///
/// # Errors
///
/// [`TextError`] for a name that can't be sway text.
pub fn output(name: &str) -> Result<SwayCmd, TextError> {
    Ok(bar("output").text(SwayText::new(name)?))
}

/// `bar sway-things output *`: every display. sway's list of displays only grows; `*` clears it,
/// and a display named after it takes the place of `*`.
pub fn output_all() -> SwayCmd {
    bar("output").keyword("*")
}

/// `bar sway-things font "pango:<description>"`.
///
/// # Errors
///
/// [`TextError`] for a description that can't be sway text.
pub fn font(description: &str) -> Result<SwayCmd, TextError> {
    Ok(bar("font").text(SwayText::new(format!("pango:{description}"))?))
}

/// `bar sway-things status_command "<command>"`: the command whose output swaybar shows. sway
/// runs it with `sh -c`; it is the user's own text, quoted as one argument.
///
/// # Errors
///
/// [`TextError`] for a command that can't be sway text.
pub fn status_command(command: &str) -> Result<SwayCmd, TextError> {
    Ok(bar("status_command").text(SwayText::new(command)?))
}

/// A color of swaybar that takes one color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Single {
    /// `background`
    Background,
    /// `statusline`
    Statusline,
    /// `separator`
    Separator,
}

/// `bar sway-things colors <class> <color>`.
pub fn color(class: Single, color: Color) -> SwayCmd {
    bar("colors")
        .keyword(match class {
            Single::Background => "background",
            Single::Statusline => "statusline",
            Single::Separator => "separator",
        })
        .color(color)
}

/// A color of swaybar that takes a border, a background and a text color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Triple {
    /// `focused_workspace`
    Focused,
    /// `active_workspace`
    Active,
    /// `inactive_workspace`
    Inactive,
    /// `urgent_workspace`
    Urgent,
    /// `binding_mode`
    BindingMode,
}

/// `bar sway-things colors <class> <border> <background> <text>`.
pub fn colors(class: Triple, border: Color, background: Color, text: Color) -> SwayCmd {
    bar("colors")
        .keyword(match class {
            Triple::Focused => "focused_workspace",
            Triple::Active => "active_workspace",
            Triple::Inactive => "inactive_workspace",
            Triple::Urgent => "urgent_workspace",
            Triple::BindingMode => "binding_mode",
        })
        .color(border)
        .color(background)
        .color(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Target;

    #[test]
    fn commands_render_with_the_bar_s_id() {
        let render = |command: SwayCmd| command.render(Target::Config).unwrap();
        assert_eq!(render(height(30)), "bar sway-things height 30");
        assert_eq!(render(mode(false)), "bar sway-things mode invisible");
        assert_eq!(render(mode_hide()), "bar sway-things mode hide");
        assert_eq!(
            render(modifier(Modifier::Alt)),
            "bar sway-things modifier Mod1"
        );
        assert_eq!(
            waybar_hold(Modifier::Super).map(render),
            [
                "bar sway-things-waybar swaybar_command true",
                "bar sway-things-waybar mode hide",
                "bar sway-things-waybar modifier Mod4",
            ]
        );
        assert_eq!(
            render(status_command("while date +'%X'; do sleep 1; done").unwrap()),
            "bar sway-things status_command \"while date +'%X'; do sleep 1; done\""
        );
        assert_eq!(
            render(font("Noto Sans 11").unwrap()),
            "bar sway-things font \"pango:Noto Sans 11\""
        );
        assert_eq!(
            render(colors(
                Triple::Focused,
                Color::rgb(1, 2, 3),
                Color::rgba(4, 5, 6, 7),
                Color::rgb(8, 9, 10)
            )),
            "bar sway-things colors focused_workspace #010203 #04050607 #08090a"
        );
        assert!(output("a\nb").is_err());
        assert_eq!(
            output("$x").unwrap().render(Target::Ipc).unwrap(),
            "bar sway-things output \"$$x\""
        );
    }
}
