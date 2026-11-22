// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Groups of items: several modules shown as one, waybar's `group/<name>`, optionally as a
//! drawer whose first member always shows and whose others slide out on hover.
//!
//! A group is an item of a lane like any other; its members live in its definition in
//! `bar.jsonc` (`"modules": [...]`), with the drawer's options under `"drawer"`. The changes here
//! work on the three lists of the lanes and on [`Modules`] together, so a member is always in
//! exactly one place: a lane or a group. The page stores both (the lists as settings, the
//! definitions in `bar.jsonc`).
//!
//! waybar builds a group as a box named after it (`group/audio` is `#audio`) with the `widget` of
//! each member; a drawer puts all but the first member in a `revealer` with the class `drawer`,
//! before the first member when it opens toward the left (`transition-left-to-right: false`),
//! after it otherwise, and gives them the class of `children-class` (`drawer-child`). An open
//! drawer marks the group as hovered. swaybar has no groups.

use serde_json::{Map, Value};

use super::modules::{Modules, members};
use super::preview::css_name;

/// How a drawer opens, as waybar takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drawer {
    /// How long it takes to slide out, in milliseconds (waybar's default: 500).
    pub duration: u32,
    /// Whether the others slide out to the left of the first member (`transition-left-to-right`
    /// false); else to its right, waybar's default.
    pub to_left: bool,
    /// Whether it opens on a click instead of on hover (`click-to-reveal`).
    pub on_click: bool,
}

impl Default for Drawer {
    /// What a new drawer of the page gets: a quarter second, toward the left, on hover, so the
    /// first member stays where it is at the right edge of its pill on the right of the bar.
    fn default() -> Self {
        Self {
            duration: 250,
            to_left: true,
            on_click: false,
        }
    }
}

/// The longest slide of a drawer the page offers, in milliseconds.
pub const MAX_DURATION: u32 = 2000;

/// The class waybar gives the members a drawer hides, unless a group names another.
pub const CHILDREN_CLASS: &str = "drawer-child";

/// Whether `name` is a group's.
pub fn is_group(name: &str) -> bool {
    super::kind_of(name).starts_with("group/")
}

/// The drawer of the group `definition`, if it is one.
pub fn drawer(definition: &Map<String, Value>) -> Option<Drawer> {
    let drawer = definition.get("drawer")?.as_object()?;
    Some(Drawer {
        duration: drawer
            .get("transition-duration")
            .and_then(Value::as_u64)
            .and_then(|duration| u32::try_from(duration).ok())
            .unwrap_or(500),
        to_left: drawer
            .get("transition-left-to-right")
            .and_then(Value::as_bool)
            .is_some_and(|right| !right),
        on_click: drawer
            .get("click-to-reveal")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// Makes the group `definition` a drawer like `drawer`, or with `None` a plain group. Options of
/// the drawer the page doesn't know are kept.
pub fn set_drawer(definition: &mut Map<String, Value>, drawer: Option<Drawer>) {
    let Some(drawer) = drawer else {
        definition.remove("drawer");
        return;
    };
    let mut options = definition
        .get("drawer")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    options.insert(
        "transition-duration".to_owned(),
        Value::from(drawer.duration.min(MAX_DURATION)),
    );
    options.insert(
        "transition-left-to-right".to_owned(),
        Value::from(!drawer.to_left),
    );
    if drawer.on_click {
        options.insert("click-to-reveal".to_owned(), Value::from(true));
    } else {
        options.remove("click-to-reveal");
    }
    options
        .entry("children-class")
        .or_insert_with(|| Value::from(CHILDREN_CLASS));
    definition.insert("drawer".to_owned(), Value::Object(options));
}

/// The members of group `name` in `modules`.
pub fn members_of(modules: &Modules, name: &str) -> Vec<String> {
    modules
        .definition(name)
        .map(|definition| members(&definition))
        .unwrap_or_default()
}

/// Every name in the lanes `lists` and in the groups they show.
fn taken(lists: &[Vec<String>], modules: &Modules) -> Vec<String> {
    modules.shown(lists)
}

/// A name for a new group that no item has yet: `group/<word>` from `title` (letters and
/// digits, lower case, `-` between words), else `group/group`, with `-2`, `-3`, … if needed. Its
/// CSS id (`#<word>`) must not be a module's either, so `group/clock` is never chosen.
pub fn free_name(lists: &[Vec<String>], modules: &Modules, title: &str) -> String {
    let mut word = String::new();
    for char in title.chars() {
        if char.is_alphanumeric() {
            word.extend(char.to_lowercase());
        } else if !word.is_empty() && !word.ends_with('-') {
            word.push('-');
        }
    }
    let word = word.trim_end_matches('-');
    let word = if word.is_empty() { "group" } else { word };
    let taken = taken(lists, modules);
    let ids: Vec<String> = taken.iter().map(|name| css_name(name).0).collect();
    let free = |candidate: &str| {
        !taken.iter().any(|name| name == candidate)
            && !modules.is_stored(candidate)
            && !ids.contains(&css_name(candidate).0)
    };
    let first = format!("group/{word}");
    if free(&first) {
        return first;
    }
    // One more than there are names: one of them is free.
    (2..=taken.len() + 2)
        .map(|n| format!("group/{word}-{n}"))
        .find(|name| free(name))
        .unwrap_or(first)
}

/// The name a group's title shows: the part after `group/`, as words (`ai-usage` is `ai usage`).
pub fn title(name: &str) -> String {
    let base = super::kind_of(name);
    let base = base.strip_prefix("group/").unwrap_or(base);
    let mut title: String = base
        .chars()
        .map(|char| if matches!(char, '-' | '_') { ' ' } else { char })
        .collect();
    if let Some(first) = title.get(..1) {
        let upper = first.to_uppercase();
        title.replace_range(..1, &upper);
    }
    title
}

/// Makes the items at `indices` of `lane` (an index of `lists`) one group named `name`, at the
/// place of the first of them, in their order; with `drawer`, a drawer. Items that are groups
/// themselves become members as groups. Says whether it did: it needs two items or more, all in
/// the lane, and a free group name.
pub fn group(
    lists: &mut [Vec<String>],
    modules: &mut Modules,
    lane: usize,
    indices: &[usize],
    name: &str,
    drawer: Option<Drawer>,
) -> bool {
    let Some(list) = lists.get_mut(lane) else {
        return false;
    };
    let mut indices: Vec<usize> = indices.to_vec();
    indices.sort_unstable();
    indices.dedup();
    if indices.len() < 2
        || indices.iter().any(|index| *index >= list.len())
        || !is_group(name)
        || !super::is_module_name(name)
        || list.iter().any(|item| item == name)
        || modules.is_stored(name)
    {
        return false;
    }
    let first = indices[0];
    let members: Vec<String> = indices.iter().map(|index| list[*index].clone()).collect();
    for index in indices.iter().rev() {
        list.remove(*index);
    }
    list.insert(first, name.to_owned());
    let mut definition = Map::new();
    definition.insert("orientation".to_owned(), Value::from("inherit"));
    definition.insert("modules".to_owned(), Value::from(members));
    set_drawer(&mut definition, drawer);
    modules.set(name, definition);
    true
}

/// Puts the members of the group at `index` of `lane` back into the lane at its place and
/// removes the group's definition. Says whether it did.
pub fn ungroup(
    lists: &mut [Vec<String>],
    modules: &mut Modules,
    lane: usize,
    index: usize,
) -> bool {
    let Some(list) = lists.get_mut(lane) else {
        return false;
    };
    let Some(name) = list.get(index).cloned() else {
        return false;
    };
    if !is_group(&name) {
        return false;
    }
    let members = members_of(modules, &name);
    list.remove(index);
    for (offset, member) in members.into_iter().enumerate() {
        list.insert(index + offset, member);
    }
    modules.reset(&name);
    true
}

/// Sets the members of group `name` to `members`, keeping its other options.
pub fn set_members(modules: &mut Modules, name: &str, members: &[String]) {
    let mut definition = modules.definition(name).unwrap_or_default();
    definition.insert("modules".to_owned(), Value::from(members.to_vec()));
    modules.set(name, definition);
}

/// Where an item is: in a lane at an index, or a member of the group at an index of a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The item at this index of the lane.
    Lane(usize, usize),
    /// The member at this index of the group at that index of the lane.
    Member(usize, usize, usize),
}

/// The name at `place`.
pub fn at(lists: &[Vec<String>], modules: &Modules, place: Place) -> Option<String> {
    match place {
        Place::Lane(lane, index) => lists.get(lane)?.get(index).cloned(),
        Place::Member(lane, group, member) => {
            let group = lists.get(lane)?.get(group)?;
            members_of(modules, group).get(member).cloned()
        }
    }
}

/// Moves the item at `from` to the place before `to` (`to` counted before the move; an index
/// past the end is the end). An item never goes into a group of its own or into itself. Returns
/// where it went, or `None` if it didn't move.
pub fn move_item(
    lists: &mut [Vec<String>],
    modules: &mut Modules,
    from: Place,
    to: Place,
) -> Option<Place> {
    let name = at(lists, modules, from)?;
    if from == to {
        return None;
    }
    // The target group, as a name, so indices that shift as the item leaves don't matter.
    let target_group = match to {
        Place::Member(lane, group, _) => Some(lists.get(lane)?.get(group)?.clone()),
        Place::Lane(..) => None,
    };
    if let Some(group) = &target_group {
        if *group == name || contains(modules, &name, group, 0) {
            return None;
        }
    }
    // Take it out.
    match from {
        Place::Lane(lane, index) => {
            lists.get_mut(lane)?.remove(index);
        }
        Place::Member(lane, group, member) => {
            let group = lists.get(lane)?.get(group)?.clone();
            let mut list = members_of(modules, &group);
            list.remove(member);
            set_members(modules, &group, &list);
        }
    }
    // Put it in.
    match to {
        Place::Lane(lane, index) => {
            let mut index = index;
            if let Place::Lane(from_lane, from_index) = from {
                if from_lane == lane && from_index < index {
                    index -= 1;
                }
            }
            let list = lists.get_mut(lane)?;
            let index = index.min(list.len());
            list.insert(index, name);
            Some(Place::Lane(lane, index))
        }
        Place::Member(lane, _, member) => {
            let group = target_group?;
            let mut member = member;
            if let Place::Member(from_lane, from_group, from_member) = from {
                let same = lists
                    .get(from_lane)
                    .and_then(|list| list.get(from_group))
                    .is_some_and(|name| *name == group);
                if same && from_member < member {
                    member -= 1;
                }
            }
            let mut list = members_of(modules, &group);
            let member = member.min(list.len());
            list.insert(member, name);
            set_members(modules, &group, &list);
            let index = lists.get(lane)?.iter().position(|item| *item == group)?;
            Some(Place::Member(lane, index, member))
        }
    }
}

/// Whether `group` is `name` or one of its members, at any depth (`name` being a group).
fn contains(modules: &Modules, name: &str, group: &str, depth: u8) -> bool {
    depth < 8
        && members_of(modules, name)
            .iter()
            .any(|member| member == group || contains(modules, member, group, depth + 1))
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

    fn lists() -> Vec<Vec<String>> {
        vec![
            vec!["sway/workspaces".to_owned()],
            vec!["clock".to_owned()],
            vec![
                "pulseaudio".to_owned(),
                "backlight".to_owned(),
                "battery".to_owned(),
                "tray".to_owned(),
            ],
        ]
    }

    #[test]
    fn items_become_a_group_and_back() {
        let mut lists = lists();
        let mut modules = Modules::default();
        let name = free_name(&lists, &modules, "Audio & light");
        assert_eq!(name, "group/audio-light");
        assert!(group(
            &mut lists,
            &mut modules,
            2,
            &[1, 0],
            &name,
            Some(Drawer::default())
        ));
        assert_eq!(lists[2], ["group/audio-light", "battery", "tray"]);
        let definition = modules.definition(&name).unwrap();
        assert_eq!(definition["modules"], json!(["pulseaudio", "backlight"]));
        assert_eq!(
            definition["drawer"],
            json!({"transition-duration": 250, "transition-left-to-right": false,
                   "children-class": "drawer-child"})
        );
        assert_eq!(drawer(&definition), Some(Drawer::default()));
        // One item isn't a group; a name in use isn't free.
        assert!(!group(&mut lists, &mut modules, 2, &[1], "group/x", None));
        assert!(!group(&mut lists, &mut modules, 2, &[1, 2], &name, None));
        assert!(!group(
            &mut lists,
            &mut modules,
            2,
            &[1, 9],
            "group/y",
            None
        ));
        assert!(ungroup(&mut lists, &mut modules, 2, 0));
        assert_eq!(lists, self::lists());
        assert!(!modules.is_stored(&name));
        assert!(!ungroup(&mut lists, &mut modules, 2, 0));
    }

    #[test]
    fn a_group_name_is_free_and_its_id_is_no_module_s() {
        let lists = lists();
        let modules = Modules::default();
        assert_eq!(free_name(&lists, &modules, "Clock"), "group/clock-2");
        assert_eq!(free_name(&lists, &modules, "  "), "group/group");
        assert_eq!(free_name(&lists, &modules, "Système"), "group/système");
        assert_eq!(title("group/ai-usage"), "Ai usage");
        assert_eq!(title("group/audio#x"), "Audio");
    }

    #[test]
    fn items_move_into_out_of_and_within_groups() {
        let mut lists = lists();
        let mut modules = Modules::default();
        assert!(group(&mut lists, &mut modules, 2, &[0, 1], "group/a", None));
        // battery into the group, at its end
        assert_eq!(
            move_item(
                &mut lists,
                &mut modules,
                Place::Lane(2, 1),
                Place::Member(2, 0, 9)
            ),
            Some(Place::Member(2, 0, 2))
        );
        assert_eq!(lists[2], ["group/a", "tray"]);
        assert_eq!(
            members_of(&modules, "group/a"),
            ["pulseaudio", "backlight", "battery"]
        );
        // within: the last to the front
        assert_eq!(
            move_item(
                &mut lists,
                &mut modules,
                Place::Member(2, 0, 2),
                Place::Member(2, 0, 0)
            ),
            Some(Place::Member(2, 0, 0))
        );
        // within: the first after the second
        assert_eq!(
            move_item(
                &mut lists,
                &mut modules,
                Place::Member(2, 0, 0),
                Place::Member(2, 0, 2)
            ),
            Some(Place::Member(2, 0, 1))
        );
        assert_eq!(
            members_of(&modules, "group/a"),
            ["pulseaudio", "battery", "backlight"]
        );
        // out to another lane
        assert_eq!(
            move_item(
                &mut lists,
                &mut modules,
                Place::Member(2, 0, 1),
                Place::Lane(0, 0)
            ),
            Some(Place::Lane(0, 0))
        );
        assert_eq!(lists[0], ["battery", "sway/workspaces"]);
        // within a lane, after itself
        assert_eq!(
            move_item(
                &mut lists,
                &mut modules,
                Place::Lane(0, 0),
                Place::Lane(0, 2)
            ),
            Some(Place::Lane(0, 1))
        );
        // A group never goes into itself.
        assert_eq!(
            move_item(
                &mut lists,
                &mut modules,
                Place::Lane(2, 0),
                Place::Member(2, 0, 0)
            ),
            None
        );
    }

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
