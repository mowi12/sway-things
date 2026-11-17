// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which workspace goes on which display, per combination of displays (opt-in).
//!
//! A placement maps workspace names (`1` … `10`) to displays by their name in the settings
//! ([`super::Output::identifier`]). [`commands`] turns it into sway commands: `workspace N
//! output X` for workspaces that don't exist yet, and for each one that does but stands on
//! another display, a move there. sway's `workspace N output X` only adds to a list that is never
//! cleared, so a workspace that is open already has to be moved; the focus goes back to where it
//! was at the end.

use std::collections::BTreeMap;

use super::{Output, Outputs};
use crate::command::{SwayCmd, SwayText, TextError};

/// The workspaces a placement covers: `1` to `10`, as sway's default config binds them.
pub const NAMES: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];

/// A placement to start from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Round the displays from left to right: 1 on the first, 2 on the second, …
    LeftToRight,
    /// A block of workspaces per display, from left to right: 1 to 3 on the first, …
    Blocks,
}

impl Preset {
    /// Every preset, in the order a page offers them.
    pub const ALL: [Self; 2] = [Self::LeftToRight, Self::Blocks];

    /// The placement of [`NAMES`] on the connected displays of `outputs` that are on.
    pub fn place(self, outputs: &Outputs) -> BTreeMap<String, String> {
        let displays = left_to_right(outputs);
        let mut placement = BTreeMap::new();
        if displays.is_empty() {
            return placement;
        }
        let per_display = NAMES.len().div_ceil(displays.len());
        for (index, name) in NAMES.iter().enumerate() {
            let display = match self {
                Self::LeftToRight => index % displays.len(),
                Self::Blocks => (index / per_display).min(displays.len() - 1),
            };
            if let Some(display) = displays.get(display) {
                placement.insert((*name).to_owned(), display.identifier().to_owned());
            }
        }
        placement
    }
}

/// The connected displays of `outputs` that are on, from left to right (then top to bottom).
pub fn left_to_right(outputs: &Outputs) -> Vec<&Output> {
    let mut displays: Vec<&Output> = outputs
        .iter()
        .filter(|output| output.is_connected() && output.is_enabled())
        .collect();
    displays.sort_by_key(|output| output.rect().map(|rect| (rect.x, rect.y)));
    displays
}

/// The placement as the workspaces stand now: each open workspace of [`NAMES`] on its display.
pub fn current(
    outputs: &Outputs,
    workspaces: &[crate::ipc::types::Workspace],
) -> BTreeMap<String, String> {
    workspaces
        .iter()
        .filter(|workspace| NAMES.contains(&workspace.name.as_str()))
        .filter_map(|workspace| {
            let output = outputs.find(&workspace.output)?;
            Some((workspace.name.clone(), output.identifier().to_owned()))
        })
        .collect()
}

/// The sway commands that put the workspaces where `placement` says, on the displays of
/// `outputs`, with the workspaces as they are now in `open`. A display of the placement that
/// isn't connected, or a name that can't be a sway argument, leaves its workspace alone.
///
/// # Errors
///
/// [`TextError`] if a name can't be put into a command.
pub fn commands(
    placement: &BTreeMap<String, String>,
    outputs: &Outputs,
    open: &[crate::ipc::types::Workspace],
) -> Result<Vec<SwayCmd>, TextError> {
    let mut declared = Vec::new();
    let mut moves = Vec::new();
    for (workspace, display) in placement {
        let Some(output) = outputs.find(display).filter(|output| output.is_enabled()) else {
            continue;
        };
        let name = output.name();
        declared.push(
            SwayCmd::new("workspace")
                .text(SwayText::new(workspace)?)
                .keyword("output")
                .text(SwayText::new(name)?),
        );
        let elsewhere = open
            .iter()
            .any(|open| open.name == *workspace && open.output != name);
        if elsewhere {
            moves.push(
                SwayCmd::new("workspace")
                    .keyword("--no-auto-back-and-forth")
                    .text(SwayText::new(workspace)?),
            );
            moves.push(
                SwayCmd::new("move")
                    .keyword("workspace")
                    .keyword("to")
                    .keyword("output")
                    .text(SwayText::new(name)?),
            );
        }
    }
    if !moves.is_empty() {
        // Back to the workspace that had the focus, and the one each display showed.
        for visible in open.iter().filter(|open| open.visible && !open.focused) {
            moves.push(
                SwayCmd::new("workspace")
                    .keyword("--no-auto-back-and-forth")
                    .text(SwayText::new(&visible.name)?),
            );
        }
        if let Some(focused) = open.iter().find(|open| open.focused) {
            moves.push(
                SwayCmd::new("workspace")
                    .keyword("--no-auto-back-and-forth")
                    .text(SwayText::new(&focused.name)?),
            );
        }
    }
    declared.extend(moves);
    Ok(declared)
}

/// Puts the workspaces where the profile of the connected combination says, if `profiles`
/// place them: for `sway-things hook workspaces`, which kanshi runs when it applied a profile,
/// and for the page. Returns how many commands were sent.
///
/// # Errors
///
/// The IPC errors of asking sway and of the commands.
pub async fn apply(
    client: &mut crate::ipc::Client,
    profiles: &super::profiles::Profiles,
) -> Result<usize, crate::ipc::IpcError> {
    if !profiles.places_workspaces() {
        return Ok(0);
    }
    let outputs = Outputs::new(&client.outputs().await?);
    let names: Vec<&str> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .map(Output::identifier)
        .collect();
    let Some(profile) = profiles.of(&names) else {
        return Ok(0);
    };
    let open = client.workspaces().await?;
    let Ok(commands) = commands(profile.workspaces(), &outputs, &open) else {
        return Ok(0);
    };
    for command in &commands {
        client.run(command).await?;
    }
    Ok(commands.len())
}

#[cfg(test)]
mod tests {
    use super::{Preset, commands};
    use crate::command::Target;
    use crate::outputs::Outputs;

    const TWO: &str = r#"[
      {"name": "eDP-1", "active": true, "primary": false, "make": "A", "model": "B",
       "serial": "C", "modes": [], "rect": {"x": 0, "y": 0, "width": 1920, "height": 1080}},
      {"name": "DP-1", "active": true, "primary": false, "make": "D", "model": "E",
       "serial": "F", "modes": [], "rect": {"x": 1920, "y": 0, "width": 3440, "height": 1440}}
    ]"#;

    fn outputs() -> Outputs {
        Outputs::new(&serde_json::from_str::<Vec<_>>(TWO).unwrap())
    }

    #[test]
    fn presets_round_the_displays_or_fill_them_in_blocks() {
        let round = Preset::LeftToRight.place(&outputs());
        assert_eq!(round["1"], "A B C");
        assert_eq!(round["2"], "D E F");
        assert_eq!(round["3"], "A B C");
        let blocks = Preset::Blocks.place(&outputs());
        assert_eq!(blocks["5"], "A B C");
        assert_eq!(blocks["6"], "D E F");
        assert_eq!(blocks["10"], "D E F");
    }

    #[test]
    fn an_open_workspace_on_another_display_is_moved_and_the_focus_comes_back() {
        let open: Vec<crate::ipc::types::Workspace> = serde_json::from_str(
            r#"[{"id": 1, "num": 1, "name": "1", "layout": "splith", "visible": true,
                 "focused": true, "urgent": false, "representation": null, "orientation": "horizontal",
                 "output": "eDP-1", "rect": {"x": 0, "y": 0, "width": 0, "height": 0}}]"#,
        )
        .unwrap();
        let placement = [("1".to_owned(), "D E F".to_owned())].into_iter().collect();
        let sent: Vec<String> = commands(&placement, &outputs(), &open)
            .unwrap()
            .iter()
            .map(|command| command.render(Target::Ipc).unwrap())
            .collect();
        assert_eq!(
            sent,
            [
                r#"workspace "1" output "DP-1""#,
                r#"workspace --no-auto-back-and-forth "1""#,
                r#"move workspace to output "DP-1""#,
                r#"workspace --no-auto-back-and-forth "1""#,
            ]
        );
    }
}
