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

fn bar(subcommand: &'static str) -> SwayCmd {
    SwayCmd::new("bar").keyword(ID).keyword(subcommand)
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
