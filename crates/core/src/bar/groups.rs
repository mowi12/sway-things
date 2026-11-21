// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Groups of items: several modules shown as one, waybar's `group/<name>`, optionally as a
//! drawer whose first member always shows and whose others slide out on hover.
//!
//! A group is an item of a lane like any other; its members live in its definition in
//! `bar.jsonc` (`"modules": [...]`), with the drawer's options under `"drawer"`. The looks draw
//! each group of a lane as one pill with its members inside ([`styled`]).
//!
//! waybar builds a group as a box named after it (`group/audio` is `#audio`) with the `widget` of
//! each member; a drawer puts all but the first member in a `revealer` with the class `drawer`,
//! before the first member when it opens toward the left (`transition-left-to-right: false`),
//! after it otherwise, and gives them the class of `children-class` (`drawer-child`). An open
//! drawer marks the group as hovered. swaybar has no groups.

use super::modules::Modules;
use super::preview::css_name;

/// Whether `name` is a group's.
pub fn is_group(name: &str) -> bool {
    super::kind_of(name).starts_with("group/")
}

/// A group the stylesheet styles: its CSS id and whether it is in another group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Styled {
    /// The id waybar gives its box (`audio` for `group/audio`).
    pub id: String,
    /// Whether it is a member of another group: then it is part of that group's pill.
    pub nested: bool,
}

/// The groups the bar of `lists` shows, each once: those in the lanes first, then the nested
/// ones. A group both in a lane and in another group counts as in the lane.
pub fn styled(lists: &[Vec<String>], modules: &Modules) -> Vec<Styled> {
    let top: Vec<&String> = lists
        .iter()
        .flatten()
        .filter(|name| is_group(name))
        .collect();
    let mut out: Vec<Styled> = Vec::new();
    let mut push = |id: String, nested: bool| {
        if super::is_module_name(&id)
            && id
                .chars()
                .all(|char| char.is_alphanumeric() || matches!(char, '-' | '_'))
            && !out.iter().any(|styled| styled.id == id)
        {
            out.push(Styled { id, nested });
        }
    };
    for name in &top {
        push(css_name(name).0, false);
    }
    for name in modules.shown(lists) {
        if is_group(&name) && !top.contains(&&name) {
            push(css_name(&name).0, true);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn styled_groups_are_those_shown_with_their_nesting() {
        let mut modules = Modules::default();
        modules.set(
            "group/ai",
            json!({"modules": ["group/ai-head", "custom/x"]})
                .as_object()
                .unwrap()
                .clone(),
        );
        modules.set(
            "group/ai-head",
            json!({"modules": ["custom/y"]})
                .as_object()
                .unwrap()
                .clone(),
        );
        modules.set(
            "group/bad id",
            json!({"modules": []}).as_object().unwrap().clone(),
        );
        let lists = vec![vec!["group/ai".to_owned(), "clock".to_owned()]];
        assert_eq!(
            styled(&lists, &modules),
            [
                Styled {
                    id: "ai".into(),
                    nested: false
                },
                Styled {
                    id: "ai-head".into(),
                    nested: true
                }
            ]
        );
    }
}
