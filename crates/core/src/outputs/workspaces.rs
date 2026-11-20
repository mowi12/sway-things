// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which workspace goes on which display: by a rule worked out for the displays connected, or
//! fixed per combination of displays.
//!
//! The rule is the setting `workspaces.placement` ([`Rule`]), and `workspaces.count` says how
//! many workspaces, from `1`, are placed ([`names`]). A fixed placement maps workspace names to
//! displays by their name in the settings ([`super::Output::identifier`]) and is remembered per
//! combination in `displays.toml` ([`super::profiles::Profile::workspaces`]).
//!
//! [`place`] gives each workspace its display, and [`with_fallbacks`] the other displays after it,
//! from left to right: [`commands`] sends `workspace N output A B C`, so a workspace whose display
//! goes away moves to the next one, and comes back when its display returns (sway moves a
//! workspace to an output higher on its list once that is there). sway's `workspace N output`
//! only adds to a list that is never cleared over IPC, so a workspace that is open already is
//! moved too; the focus goes back to where it was at the end.

use std::collections::BTreeMap;

use super::{Output, Outputs};
use crate::command::{SwayCmd, SwayText, TextError};
use crate::registry::{Value, workspaces as keys};
use crate::state::State;

/// How the workspaces are placed (`workspaces.placement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Where sway puts them: on the display with the focus. Nothing is placed.
    Sway,
    /// Round the displays from left to right: 1 on the first, 2 on the second, …
    RoundRobin,
    /// A block of workspaces per display, from left to right: 1 to 5 on the first, …
    Blocks,
    /// All on the built-in display, or on the leftmost one without a built-in display.
    OneDisplay,
    /// As the user arranged them for the combination connected, remembered per combination.
    /// A combination without one is placed round robin.
    Fixed,
}

impl Rule {
    /// The rule of the choice `id`; sway's own for an unknown one.
    pub fn from_id(id: &str) -> Self {
        match id {
            "round_robin" => Self::RoundRobin,
            "blocks" => Self::Blocks,
            "one_display" => Self::OneDisplay,
            "fixed" => Self::Fixed,
            _ => Self::Sway,
        }
    }

    /// The id of the rule's choice.
    pub fn id(self) -> &'static str {
        match self {
            Self::Sway => "sway",
            Self::RoundRobin => "round_robin",
            Self::Blocks => "blocks",
            Self::OneDisplay => "one_display",
            Self::Fixed => "fixed",
        }
    }

    /// The rule stored in `state`, or sway's own.
    pub fn of(state: &State) -> Self {
        match state.get(keys::PLACEMENT, None) {
            Some(Value::Choice(id)) => Self::from_id(id),
            _ => Self::Sway,
        }
    }

    /// Whether the rule places anything.
    pub fn places(self) -> bool {
        self != Self::Sway
    }
}

/// How many workspaces are placed, as stored in `state`.
pub fn count(state: &State) -> usize {
    match state.get(keys::COUNT, None) {
        Some(Value::Integer(count)) => {
            usize::try_from((*count).clamp(1, keys::MAX_COUNT)).unwrap_or(10)
        }
        _ => 10,
    }
}

/// The names of the workspaces placed: `1` to `count`.
pub fn names(count: usize) -> Vec<String> {
    (1..=count).map(|number| number.to_string()).collect()
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

/// The displays workspaces go on: those of [`left_to_right`] except the ones in `mirrors` (by
/// name), which show another display and take no workspaces.
pub fn targets<'a>(outputs: &'a Outputs, mirrors: &[String]) -> Vec<&'a Output> {
    left_to_right(outputs)
        .into_iter()
        .filter(|output| !mirrors.iter().any(|name| name == output.identifier()))
        .collect()
}

/// Whether `output` is a laptop's or tablet's own panel, by its connector.
fn is_built_in(output: &Output) -> bool {
    ["eDP-", "LVDS-", "DSI-"]
        .iter()
        .any(|prefix| output.name().starts_with(prefix))
}

/// The display of each of the first `count` workspaces by `rule`, on the displays of `outputs`
/// that aren't in `mirrors`, with `fixed` the user's placement for this combination. Nothing for
/// [`Rule::Sway`] or without a display. A fixed placement that names a display that isn't one of
/// them, or misses a workspace, has it placed round robin.
pub fn place(
    rule: Rule,
    outputs: &Outputs,
    mirrors: &[String],
    count: usize,
    fixed: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let displays = targets(outputs, mirrors);
    let mut placement = BTreeMap::new();
    if displays.is_empty() || rule == Rule::Sway {
        return placement;
    }
    let per_display = count.div_ceil(displays.len()).max(1);
    let built_in = displays
        .iter()
        .position(|output| is_built_in(output))
        .unwrap_or(0);
    for (index, name) in names(count).into_iter().enumerate() {
        let round = index % displays.len();
        let display = match rule {
            Rule::Sway | Rule::RoundRobin => round,
            Rule::Blocks => (index / per_display).min(displays.len() - 1),
            Rule::OneDisplay => built_in,
            Rule::Fixed => fixed
                .get(&name)
                .and_then(|wanted| {
                    displays
                        .iter()
                        .position(|output| output.identifier() == wanted)
                })
                .unwrap_or(round),
        };
        if let Some(display) = displays.get(display) {
            placement.insert(name, display.identifier().to_owned());
        }
    }
    placement
}

/// `placement` with the other displays of `outputs` that aren't in `mirrors` after each
/// workspace's own, from left to right: where it goes while its own display is gone.
pub fn with_fallbacks(
    placement: &BTreeMap<String, String>,
    outputs: &Outputs,
    mirrors: &[String],
) -> BTreeMap<String, Vec<String>> {
    let displays = targets(outputs, mirrors);
    placement
        .iter()
        .map(|(name, display)| {
            let mut list = vec![display.clone()];
            list.extend(
                displays
                    .iter()
                    .map(|output| output.identifier().to_owned())
                    .filter(|other| other != display),
            );
            (name.clone(), list)
        })
        .collect()
}

/// The sway commands that put each workspace of `placement` on its display, on the displays of
/// `outputs`, with the workspaces as they are now in `open`: what [`placing`] sends, without
/// other displays to go to.
///
/// # Errors
///
/// [`TextError`] if a name can't be put into a command.
pub fn commands(
    placement: &BTreeMap<String, String>,
    outputs: &Outputs,
    open: &[crate::ipc::types::Workspace],
) -> Result<Vec<SwayCmd>, TextError> {
    let lists: BTreeMap<String, Vec<String>> = placement
        .iter()
        .map(|(name, display)| (name.clone(), vec![display.clone()]))
        .collect();
    placing(&lists, outputs, open)
}

/// The sway commands for `placement`, each workspace with its displays, most wanted first:
/// `workspace N output A B C` with the connected ones that are on, and a move for each open
/// workspace that isn't on the first of them. A workspace none of whose displays is there, or a
/// name that can't be a sway argument, is left alone.
///
/// # Errors
///
/// [`TextError`] if a name can't be put into a command.
pub fn placing(
    placement: &BTreeMap<String, Vec<String>>,
    outputs: &Outputs,
    open: &[crate::ipc::types::Workspace],
) -> Result<Vec<SwayCmd>, TextError> {
    let mut declared = Vec::new();
    let mut moves = Vec::new();
    for (workspace, displays) in placement {
        let names: Vec<&str> = displays
            .iter()
            .filter_map(|display| outputs.find(display))
            .filter(|output| output.is_connected() && output.is_enabled())
            .map(Output::name)
            .collect();
        let Some(first) = names.first().copied() else {
            continue;
        };
        let mut command = SwayCmd::new("workspace")
            .text(SwayText::new(workspace)?)
            .keyword("output");
        for name in &names {
            command = command.text(SwayText::new(*name)?);
        }
        declared.push(command);
        let elsewhere = open
            .iter()
            .any(|open| open.name == *workspace && open.output != first);
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
                    .text(SwayText::new(first)?),
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

/// The displays of the connected combination in `profiles` that show another display's
/// picture, by name: they take no workspaces.
pub fn mirrored(profiles: Option<&super::profiles::Profiles>, outputs: &Outputs) -> Vec<String> {
    let names: Vec<&str> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .map(Output::identifier)
        .collect();
    profiles
        .and_then(|profiles| profiles.of(&names))
        .map(|profile| {
            profile
                .outputs()
                .filter(|(_, setup)| setup.mirror.is_some())
                .map(|(name, _)| name.to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The fixed placement remembered for the connected combination in `profiles`, if any.
pub fn fixed(
    profiles: Option<&super::profiles::Profiles>,
    outputs: &Outputs,
) -> BTreeMap<String, String> {
    let names: Vec<&str> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .map(Output::identifier)
        .collect();
    profiles
        .and_then(|profiles| profiles.of(&names))
        .map(|profile| profile.workspaces().clone())
        .unwrap_or_default()
}

/// Puts the workspaces on the displays connected now as the rule and count stored in `state`
/// say, with the fixed placement of the combination from `profiles`: for `sway-things hook
/// workspaces`, which runs when sway starts or reloads and when kanshi applied a profile, and
/// for the page. Nothing for sway's own placement. Returns how many commands were sent.
///
/// # Errors
///
/// The IPC errors of asking sway and of the commands.
pub async fn apply(
    client: &mut crate::ipc::Client,
    state: &State,
    profiles: Option<&super::profiles::Profiles>,
) -> Result<usize, crate::ipc::IpcError> {
    let rule = Rule::of(state);
    if !rule.places() {
        return Ok(0);
    }
    let outputs = Outputs::new(&client.outputs().await?);
    let mirrors = mirrored(profiles, &outputs);
    let placement = place(
        rule,
        &outputs,
        &mirrors,
        count(state),
        &fixed(profiles, &outputs),
    );
    let lists = with_fallbacks(&placement, &outputs, &mirrors);
    let open = client.workspaces().await?;
    let Ok(commands) = placing(&lists, &outputs, &open) else {
        return Ok(0);
    };
    // One message: nobody else's command comes between a focus and the move after it.
    client.run_together(&commands).await?;
    Ok(commands.len())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Rule, commands, place, placing, with_fallbacks};
    use crate::command::Target;
    use crate::outputs::Outputs;

    /// A laptop with a wide display on its right, and a third display on the right of that.
    const THREE: &str = r#"[
      {"name": "eDP-1", "active": true, "primary": false, "make": "A", "model": "B",
       "serial": "C", "modes": [], "rect": {"x": 0, "y": 0, "width": 1920, "height": 1080}},
      {"name": "DP-1", "active": true, "primary": false, "make": "D", "model": "E",
       "serial": "F", "modes": [], "rect": {"x": 1920, "y": 0, "width": 3440, "height": 1440}},
      {"name": "HDMI-A-1", "active": true, "primary": false, "make": "G", "model": "H",
       "serial": "I", "modes": [], "rect": {"x": 5360, "y": 0, "width": 1920, "height": 1080}}
    ]"#;

    const LAPTOP: &str = "A B C";
    const WIDE: &str = "D E F";
    const THIRD: &str = "G H I";

    fn outputs(json: &str, count: usize) -> Outputs {
        let all: Vec<crate::ipc::types::Output> = serde_json::from_str(json).unwrap();
        Outputs::new(&all[..count])
    }

    fn rendered(commands: &[crate::command::SwayCmd]) -> Vec<String> {
        commands
            .iter()
            .map(|command| command.render(Target::Ipc).unwrap())
            .collect()
    }

    #[test]
    fn rules_round_the_displays_fill_them_in_blocks_or_use_the_built_in_one() {
        let two = outputs(THREE, 2);
        let none = BTreeMap::new();
        let round = place(Rule::RoundRobin, &two, &[], 10, &none);
        assert_eq!(round.len(), 10);
        assert_eq!((round["1"].as_str(), round["2"].as_str()), (LAPTOP, WIDE));
        assert_eq!(round["3"], LAPTOP);
        let blocks = place(Rule::Blocks, &two, &[], 10, &none);
        assert_eq!((blocks["5"].as_str(), blocks["6"].as_str()), (LAPTOP, WIDE));
        assert_eq!(blocks["10"], WIDE);
        // Three displays and seven workspaces: blocks of three, the last one shorter.
        let blocks = place(Rule::Blocks, &outputs(THREE, 3), &[], 7, &none);
        assert_eq!(
            ["1", "3", "4", "6", "7"].map(|name| blocks[name].as_str()),
            [LAPTOP, LAPTOP, WIDE, WIDE, THIRD]
        );
        let one = place(Rule::OneDisplay, &two, &[], 12, &none);
        assert_eq!(one.len(), 12);
        assert!(one.values().all(|display| display == LAPTOP));
        // sway's own placement places nothing.
        assert!(place(Rule::Sway, &two, &[], 10, &none).is_empty());
        // A display that mirrors another takes none.
        let mirrored = place(Rule::RoundRobin, &two, &[WIDE.to_owned()], 4, &none);
        assert!(mirrored.values().all(|display| display == LAPTOP));
    }

    /// Two desk displays without a built-in panel.
    const DESK: &str = r#"[
      {"name": "DP-4", "active": true, "primary": false, "make": "S", "model": "T",
       "serial": "1", "modes": [], "rect": {"x": 2560, "y": 0, "width": 2560, "height": 1440}},
      {"name": "DP-5", "active": true, "primary": false, "make": "S", "model": "T",
       "serial": "2", "modes": [], "rect": {"x": 0, "y": 0, "width": 2560, "height": 1440}}
    ]"#;

    #[test]
    fn one_display_without_a_built_in_one_is_the_leftmost_and_short_counts_fit() {
        let none = BTreeMap::new();
        let one = place(Rule::OneDisplay, &outputs(DESK, 2), &[], 3, &none);
        assert!(one.values().all(|display| display == "S T 2"), "{one:?}");
        // Fewer workspaces than displays: one each, from the left.
        let blocks = place(Rule::Blocks, &outputs(THREE, 3), &[], 2, &none);
        assert_eq!((blocks["1"].as_str(), blocks["2"].as_str()), (LAPTOP, WIDE));
        // The panel shows another display: the leftmost of the others.
        let one = place(
            Rule::OneDisplay,
            &outputs(THREE, 2),
            &[LAPTOP.to_owned()],
            2,
            &none,
        );
        assert!(one.values().all(|display| display == WIDE));
    }

    #[test]
    fn a_fixed_placement_keeps_its_displays_and_fills_the_rest_round_robin() {
        let two = outputs(THREE, 2);
        let fixed: BTreeMap<String, String> = [
            ("1".to_owned(), WIDE.to_owned()),
            ("2".to_owned(), WIDE.to_owned()),
            // Not connected now.
            ("3".to_owned(), THIRD.to_owned()),
        ]
        .into_iter()
        .collect();
        let placed = place(Rule::Fixed, &two, &[], 4, &fixed);
        assert_eq!(
            ["1", "2", "3", "4"].map(|name| placed[name].as_str()),
            [WIDE, WIDE, LAPTOP, WIDE]
        );
    }

    #[test]
    fn each_workspace_falls_back_to_the_other_displays_from_left_to_right() {
        let three = outputs(THREE, 3);
        let placement: BTreeMap<String, String> =
            [("2".to_owned(), THIRD.to_owned())].into_iter().collect();
        let lists = with_fallbacks(&placement, &three, &[]);
        assert_eq!(lists["2"], [THIRD, LAPTOP, WIDE]);
        let sent = rendered(&placing(&lists, &three, &[]).unwrap());
        assert_eq!(sent, [r#"workspace "2" output "HDMI-A-1" "eDP-1" "DP-1""#]);
        // A display that mirrors another isn't one to fall back to.
        let lists = with_fallbacks(&placement, &three, &[WIDE.to_owned()]);
        assert_eq!(lists["2"], [THIRD, LAPTOP]);
    }

    #[test]
    fn an_open_workspace_on_another_display_is_moved_and_the_focus_comes_back() {
        let open: Vec<crate::ipc::types::Workspace> = serde_json::from_str(
            r#"[{"id": 1, "num": 1, "name": "1", "layout": "splith", "visible": true,
                 "focused": true, "urgent": false, "representation": null, "orientation": "horizontal",
                 "output": "eDP-1", "rect": {"x": 0, "y": 0, "width": 0, "height": 0}}]"#,
        )
        .unwrap();
        let placement = [("1".to_owned(), WIDE.to_owned())].into_iter().collect();
        let sent = rendered(&commands(&placement, &outputs(THREE, 2), &open).unwrap());
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

    #[test]
    fn the_rule_comes_from_its_choice() {
        for rule in [
            Rule::Sway,
            Rule::RoundRobin,
            Rule::Blocks,
            Rule::OneDisplay,
            Rule::Fixed,
        ] {
            assert_eq!(Rule::from_id(rule.id()), rule);
        }
        assert_eq!(Rule::from_id("unknown"), Rule::Sway);
        for choice in crate::registry::workspaces::PLACEMENTS {
            assert_eq!(Rule::from_id(choice.id).id(), choice.id);
        }
    }
}
