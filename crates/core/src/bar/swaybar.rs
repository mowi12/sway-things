// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! swaybar as the bar: `bar sway-things …` lines in `75-bar.conf`, made by the command builders
//! of [`crate::command::bar`] from the same settings waybar's config is made from, where swaybar
//! has them: the position (top or bottom), the height, the displays, the font, the colors and
//! the status command.

use super::style::Colors;
use super::value;
use crate::command::Color;
use crate::command::SwayCmd;
use crate::command::bar::{self, Position, Single, Triple};
use crate::registry::{Value, bar as keys};
use crate::state::State;
use crate::sway_config::SwayConfig;

/// One line for swaybar: a command, or why a setting has none (a comment in the file).
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// A command, with the setting it comes from.
    Command(&'static str, SwayCmd),
    /// A setting swaybar can't take as it is, with the reason.
    Skipped(&'static str, String),
}

/// The lines that set up swaybar from `state`; `sway` is the user's sway config, for the colors
/// of the windows when the style follows them.
pub fn lines(state: &State, sway: Option<&SwayConfig>) -> Vec<Line> {
    let mut lines = vec![Line::Command(keys::BACKEND, bar::mode(true))];
    match value(state, keys::POSITION) {
        Value::Choice(id) if id == "top" => {
            lines.push(Line::Command(keys::POSITION, bar::position(Position::Top)));
        }
        Value::Choice(id) if id == "bottom" => {
            lines.push(Line::Command(
                keys::POSITION,
                bar::position(Position::Bottom),
            ));
        }
        _ => lines.push(Line::Skipped(
            keys::POSITION,
            "swaybar stands only at the top or the bottom".to_owned(),
        )),
    }
    if let Value::Integer(height) = value(state, keys::HEIGHT) {
        lines.push(Line::Command(
            keys::HEIGHT,
            bar::height(u16::try_from(height).unwrap_or(u16::MAX)),
        ));
    }
    // `output *` first: the list sway keeps only grows, and `*` clears it, so a display taken
    // out of the list leaves the running bar too.
    lines.push(Line::Command(keys::OUTPUTS, bar::output_all()));
    if let Value::List(outputs) = value(state, keys::OUTPUTS) {
        for output in outputs {
            match bar::output(&output) {
                Ok(command) => lines.push(Line::Command(keys::OUTPUTS, command)),
                Err(error) => lines.push(Line::Skipped(keys::OUTPUTS, error.to_string())),
            }
        }
    }
    // The font has no value that means "sway's own": a font taken away applies after a reload.
    if let Value::Text(font) = value(state, keys::FONT) {
        if !font.is_empty() {
            match bar::font(&font) {
                Ok(command) => lines.push(Line::Command(keys::FONT, command)),
                Err(error) => lines.push(Line::Skipped(keys::FONT, error.to_string())),
            }
        }
    }
    colors(state, sway, &mut lines);
    // `-` is none, so a command taken away stops in the running bar too.
    let command = match value(state, keys::STATUS_COMMAND) {
        Value::Text(command) if !command.is_empty() => command,
        _ => "-".to_owned(),
    };
    match bar::status_command(&command) {
        Ok(command) => lines.push(Line::Command(keys::STATUS_COMMAND, command)),
        Err(error) => lines.push(Line::Skipped(keys::STATUS_COMMAND, error.to_string())),
    }
    lines
}

/// The colors of swaybar: the style's, or swaybar's own for waybar's look and none.
fn colors(state: &State, sway: Option<&SwayConfig>, lines: &mut Vec<Line>) {
    let theme = match value(state, keys::THEME) {
        Value::Choice(id) => id,
        _ => String::new(),
    };
    let key = keys::THEME;
    if theme == "waybar" || theme == "plain" {
        // swaybar's own colors (`default_bar_config` in sway's `config/bar.c`), sent too, so a
        // look taken back reaches the running bar.
        let color = |red, green, blue| Color::rgb(red, green, blue);
        lines.push(Line::Command(
            key,
            bar::color(Single::Background, color(0, 0, 0)),
        ));
        lines.push(Line::Command(
            key,
            bar::color(Single::Statusline, color(0xff, 0xff, 0xff)),
        ));
        let white = color(0xff, 0xff, 0xff);
        for (class, border, background, text) in [
            (
                Triple::Focused,
                color(0x4c, 0x78, 0x99),
                color(0x28, 0x55, 0x77),
                white,
            ),
            (
                Triple::Active,
                color(0x33, 0x33, 0x33),
                color(0x5f, 0x67, 0x6a),
                white,
            ),
            (
                Triple::Inactive,
                color(0x33, 0x33, 0x33),
                color(0x22, 0x22, 0x22),
                color(0x88, 0x88, 0x88),
            ),
            (
                Triple::Urgent,
                color(0x2f, 0x34, 0x3a),
                color(0x90, 0, 0),
                white,
            ),
            (
                Triple::BindingMode,
                color(0x2f, 0x34, 0x3a),
                color(0x90, 0, 0),
                white,
            ),
        ] {
            lines.push(Line::Command(
                key,
                bar::colors(class, border, background, text),
            ));
        }
    } else {
        let colors = if theme == "sway" {
            Colors::sway(state, sway)
        } else {
            Colors::custom(state)
        };
        lines.push(Line::Command(
            key,
            bar::color(Single::Background, colors.background),
        ));
        lines.push(Line::Command(
            key,
            bar::color(Single::Statusline, colors.text),
        ));
        let workspace = |class, background| {
            Line::Command(key, bar::colors(class, background, background, colors.text))
        };
        lines.push(workspace(Triple::Focused, colors.focused));
        lines.push(workspace(Triple::Active, colors.hover));
        lines.push(workspace(Triple::Inactive, colors.background));
        lines.push(workspace(Triple::Urgent, colors.urgent));
        lines.push(workspace(Triple::BindingMode, colors.focused));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Target;
    use crate::registry::Registry;
    use crate::state::Paths;

    #[test]
    fn swaybar_gets_the_settings_it_has() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = State::load(
            Registry::builtin(),
            Paths::new(&dir.path().join("c"), &dir.path().join("s"), "h").unwrap(),
        );
        let set = |state: &mut State, key, value| state.set(key, None, value, None).unwrap();
        set(&mut state, keys::BACKEND, Value::Choice("swaybar".into()));
        set(&mut state, keys::POSITION, Value::Choice("left".into()));
        set(
            &mut state,
            keys::STATUS_COMMAND,
            Value::Text("i3status".into()),
        );
        let rendered: Vec<String> = lines(&state, None)
            .into_iter()
            .map(|line| match line {
                Line::Command(_, command) => command.render(Target::Config).unwrap(),
                Line::Skipped(key, reason) => format!("# {key}: {reason}"),
            })
            .collect();
        assert_eq!(
            rendered,
            [
                "bar sway-things mode dock",
                "# bar.position: swaybar stands only at the top or the bottom",
                "bar sway-things height 30",
                "bar sway-things output *",
                "bar sway-things colors background #000000",
                "bar sway-things colors statusline #ffffff",
                "bar sway-things colors focused_workspace #4c7899 #285577 #ffffff",
                "bar sway-things colors active_workspace #333333 #5f676a #ffffff",
                "bar sway-things colors inactive_workspace #333333 #222222 #888888",
                "bar sway-things colors urgent_workspace #2f343a #900000 #ffffff",
                "bar sway-things colors binding_mode #2f343a #900000 #ffffff",
                "bar sway-things status_command \"i3status\"",
            ]
        );
        // Taken away, the status command is none in the running bar too.
        state.reset(keys::STATUS_COMMAND, None).unwrap();
        let last = lines(&state, None).pop().unwrap();
        assert!(matches!(
            last,
            Line::Command(_, command) if command.render(Target::Ipc).unwrap()
                == "bar sway-things status_command \"-\""
        ));
        set(&mut state, keys::THEME, Value::Choice("sway".into()));
        let colored = lines(&state, None);
        assert!(colored.iter().any(|line| matches!(
            line,
            Line::Command(_, command) if command.render(Target::Config).unwrap()
                == "bar sway-things colors focused_workspace #285577 #285577 #ffffff"
        )));
    }
}
