// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every binding sway has, for the list of the Keyboard Shortcuts page: the user's (from the
//! config parser, with file and line) and those of Sway Things, each with what became of it and
//! the bindings it clashes with.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::command::{Category, describe};
use super::keys::{Key, Modifiers};
use super::line::{Identity, Keymap, Kind, Line};
use super::ours::{self, FileLine};
use super::table::{Bound, DEFAULT_MODE, Entry, Fate, Origin, Source, Table, user_lines};
use crate::generate::HEADER;
use crate::i18n::Text;
use crate::include::IncludeStatus;
use crate::registry::ConfigFile;
use crate::state::State;
use crate::sway_config::Directive;

/// What a row of the list is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    /// A binding of the user's.
    Hand,
    /// A binding Sway Things adds: the item of `shortcuts.bindings` at this index.
    Added {
        /// The index of the item.
        item: usize,
    },
    /// A binding of the user's that Sway Things changed: the change item at this index unbinds
    /// it and binds what is shown instead.
    Changed {
        /// The index of the change item.
        item: usize,
        /// The user's binding, as their config has it.
        original: Line,
    },
    /// A binding of the user's that Sway Things removes: the item at this index. Without a
    /// line of the user's (`source` is in the generated file), the binding it removed is gone
    /// from their config.
    Removed {
        /// The index of the item.
        item: usize,
    },
    /// A binding of a group of media keys, by the key of its switch.
    Group {
        /// The key of the switch.
        key: &'static str,
    },
    /// A binding of a key of another page, by the key of its setting.
    Feature {
        /// The key of the setting with the keys.
        key: &'static str,
    },
    /// The binding of the terminal or the launcher of the Apps & Autostart page, by the key of
    /// its command's setting.
    App {
        /// The key of the setting.
        key: &'static str,
    },
}

/// How a binding clashes with another one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClashKind {
    /// This binding took the place of the other one, which came earlier on the same keys.
    Replaces,
    /// The other one came later on the same keys and took the place of this one.
    ReplacedBy,
    /// An unbind of the user's removed this binding.
    RemovedBy,
    /// The other one, a keycode binding, is on the same key and wins: sway looks at keycode
    /// bindings first.
    Shadowed,
    /// This keycode binding is on the same key as the other one, a keysym binding, and wins.
    Shadows,
    /// This binding has `--locked` and the other one hasn't: sway runs the other one unless the
    /// screen is locked.
    LockScreenOnly,
}

/// Another binding a binding clashes with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clash {
    /// How.
    pub kind: ClashKind,
    /// Where the other one is.
    pub source: Source,
    /// The other one's command; `None` for an unbind.
    pub command: Option<String>,
}

/// A row of the list of bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The mode the binding is in.
    pub mode: String,
    /// The binding as it is now: for a changed one, the new keys and command.
    pub line: Line,
    /// Where it is: the user's line, or the line of the generated file.
    pub source: Source,
    /// What it is.
    pub kind: RowKind,
    /// Whether sway has it.
    pub in_effect: bool,
    /// The bindings it clashes with.
    pub clashes: Vec<Clash>,
}

impl Row {
    /// What kind of thing it does.
    pub fn category(&self) -> Category {
        self.line
            .command
            .as_deref()
            .map_or(Category::Other, Category::of)
    }

    /// What it does in words, if it is a common command.
    pub fn description(&self) -> Option<Text> {
        self.line.command.as_deref().and_then(describe)
    }

    /// Whether Sway Things can change or remove it: a binding of the user's in the default
    /// mode, read before the generated files, whose keys can be written without quotes.
    pub fn editable(&self) -> bool {
        match self.kind {
            RowKind::Hand => {
                self.mode == DEFAULT_MODE
                    && !self.source.after_ours
                    && super::keys::is_plain(&self.line.keys)
                    && self
                        .line
                        .unbinding()
                        .config(Modifiers::SUPER, false)
                        .is_ok()
            }
            RowKind::Added { .. } | RowKind::Changed { .. } => true,
            RowKind::Removed { .. }
            | RowKind::Group { .. }
            | RowKind::Feature { .. }
            | RowKind::App { .. } => false,
        }
    }

    /// Whether `words` (in lowercase) are all in its keys, labels, command or description.
    pub fn matches(&self, words: &[String], labels: &[String]) -> bool {
        let mut haystack = vec![
            self.line.keys.to_lowercase(),
            self.line.command.clone().unwrap_or_default().to_lowercase(),
            self.mode.to_lowercase(),
        ];
        haystack.extend(labels.iter().map(|label| label.to_lowercase()));
        if let Some(description) = self.description() {
            haystack.push(description.translated().to_lowercase());
            haystack.push(description.english().to_lowercase());
        }
        words
            .iter()
            .all(|word| haystack.iter().any(|text| text.contains(word.as_str())))
    }
}

/// Every binding of the user's config and of Sway Things.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    rows: Vec<Row>,
    table: Table,
    modifier: Modifiers,
}

/// The lines of the generated file as sway reads them, at `path`.
fn our_bound(lines: &[FileLine], path: &Arc<Path>) -> Vec<Bound> {
    let header = HEADER.lines().count();
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, file_line)| {
            let line = file_line.line.as_ref().ok()?;
            let text = file_line
                .command()
                .ok()?
                .render(crate::command::Target::Config)
                .ok()?;
            Some(Bound {
                mode: DEFAULT_MODE.to_owned(),
                line: line.clone(),
                source: Source {
                    file: Arc::clone(path),
                    line: header + index + 1,
                    text,
                    origin: file_line.origin,
                    after_ours: false,
                },
            })
        })
        .collect()
}

/// The binding lines sway reads, in order: the user's before the generated files, ours, the
/// user's after them. Without the generated files included, ours aren't read.
pub fn sequence(status: &IncludeStatus, state: &State, path: &Path) -> Vec<Bound> {
    sequence_of(
        &status.config().directives,
        status.position(ConfigFile::Bindings),
        state,
        path,
    )
}

/// [`sequence`] for the user's `directives`, of which sway reads those from `position` on after
/// the generated file at `path`, which holds the bindings of `state`.
pub fn sequence_of(
    directives: &[Directive],
    position: Option<usize>,
    state: &State,
    path: &Path,
) -> Vec<Bound> {
    let user = user_lines(directives, position);
    if position.is_none() {
        return user;
    }
    let path: Arc<Path> = path.into();
    let split = user
        .iter()
        .position(|bound| bound.source.after_ours)
        .unwrap_or(user.len());
    let mut lines = user;
    let after = lines.split_off(split);
    lines.extend(our_bound(&ours::file_lines(state), &path));
    lines.extend(after);
    lines
}

impl Listing {
    /// The bindings of the user's config in `status` and of the settings in `state`, whose
    /// generated file is at `path`. `keymap` compares keysyms with keycodes.
    pub fn new(
        status: &IncludeStatus,
        state: &State,
        path: &Path,
        keymap: Option<&dyn Keymap>,
    ) -> Self {
        let table = Table::read(sequence(status, state, path), keymap);
        let items: Vec<Option<ours::Item>> = ours::items(state)
            .iter()
            .map(|item| ours::parse(item).ok())
            .collect();
        let modifier = ours::modifier(state);
        let mut listing = Self {
            rows: Vec::new(),
            table,
            modifier,
        };
        listing.build(&items, keymap);
        listing
    }

    /// The rows, in the order sway reads the bindings.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The main modifier of the bindings of Sway Things.
    pub fn modifier(&self) -> Modifiers {
        self.modifier
    }

    /// The modes other than the default one, in the order they first appear.
    pub fn modes(&self) -> Vec<String> {
        let mut modes: Vec<String> = Vec::new();
        for row in &self.rows {
            if row.mode != DEFAULT_MODE && !modes.contains(&row.mode) {
                modes.push(row.mode.clone());
            }
        }
        modes
    }

    /// The index of the entry that the placeholder of the unbind item `item` replaced: the
    /// user's binding the item removes.
    fn removed_by_item(&self) -> HashMap<usize, usize> {
        let mut removed = HashMap::new();
        for (index, entry) in self.table.entries.iter().enumerate() {
            if let Fate::Replaced(by) = entry.fate {
                if let Some(Origin::Placeholder(item)) = self
                    .table
                    .entries
                    .get(by)
                    .map(|entry| entry.bound.source.origin)
                {
                    removed.insert(item, index);
                }
            }
        }
        removed
    }

    fn build(&mut self, items: &[Option<ours::Item>], keymap: Option<&dyn Keymap>) {
        let removed = self.removed_by_item();
        let is_change =
            |item: usize| matches!(items.get(item), Some(Some(ours::Item::Change { .. })));
        let entry_of = |origin: Origin| {
            self.table
                .entries
                .iter()
                .position(|entry| entry.bound.source.origin == origin)
        };
        let mut rows = Vec::new();
        for (index, entry) in self.table.entries.iter().enumerate() {
            let source = &entry.bound.source;
            let row = match source.origin {
                Origin::Placeholder(_) => continue,
                // A change of a binding of the user's shows as the user's row.
                Origin::Item(item) if is_change(item) && removed.contains_key(&item) => continue,
                Origin::Item(item) => self.row(index, RowKind::Added { item }),
                Origin::Group(key) => self.row(index, RowKind::Group { key }),
                Origin::Feature(key) => self.row(index, RowKind::Feature { key }),
                Origin::App(key) => self.row(index, RowKind::App { key }),
                Origin::User => {
                    let unbind = removed
                        .iter()
                        .find(|(_, user)| **user == index)
                        .map(|(item, _)| *item);
                    match unbind {
                        Some(item) if is_change(item) => {
                            let Some(new) = entry_of(Origin::Item(item)) else {
                                continue;
                            };
                            let kind = RowKind::Changed {
                                item,
                                original: entry.bound.line.clone(),
                            };
                            self.row(new, kind).map(|row| Row {
                                source: source.clone(),
                                ..row
                            })
                        }
                        Some(item) => self.row(index, RowKind::Removed { item }).map(|row| Row {
                            in_effect: false,
                            clashes: Vec::new(),
                            ..row
                        }),
                        None => self.row(index, RowKind::Hand),
                    }
                }
            };
            rows.extend(row);
        }
        // Unbinds whose binding of the user's is gone from their config: their placeholder took
        // the place of nothing.
        for (item, entry) in items.iter().enumerate() {
            let Some(ours::Item::Line(line)) = entry else {
                continue;
            };
            if !line.unbind || removed.contains_key(&item) {
                continue;
            }
            if let Some(placeholder) =
                entry_of(Origin::Placeholder(item)).and_then(|index| self.table.entries.get(index))
            {
                rows.push(Row {
                    mode: DEFAULT_MODE.to_owned(),
                    line: placeholder.bound.line.unbinding(),
                    source: Source {
                        origin: Origin::Item(item),
                        ..placeholder.bound.source.clone()
                    },
                    kind: RowKind::Removed { item },
                    in_effect: false,
                    clashes: Vec::new(),
                });
            }
        }
        self.rows = rows;
        self.add_key_clashes(keymap);
    }

    /// The row of the entry at `index`, with the clashes of what became of it.
    fn row(&self, index: usize, kind: RowKind) -> Option<Row> {
        let entry = self.table.entries.get(index)?;
        let mut clashes = Vec::new();
        match &entry.fate {
            Fate::InEffect => {}
            Fate::Replaced(by) => {
                if let Some(other) = self.table.entries.get(*by) {
                    clashes.push(clash(ClashKind::ReplacedBy, other));
                }
            }
            Fate::Removed(unbind) => clashes.push(Clash {
                kind: ClashKind::RemovedBy,
                source: unbind.source.clone(),
                command: None,
            }),
        }
        for replaced in &entry.replaces {
            if let Some(other) = self.table.entries.get(*replaced) {
                clashes.push(clash(ClashKind::Replaces, other));
            }
        }
        Some(Row {
            mode: entry.bound.mode.clone(),
            line: entry.bound.line.clone(),
            source: entry.bound.source.clone(),
            kind,
            in_effect: entry.fate == Fate::InEffect,
            clashes,
        })
    }

    /// Adds the clashes between bindings in effect that aren't the same binding for sway but
    /// still fight over a key: a keysym and a keycode binding of the same key, and a `--locked`
    /// binding next to one without it.
    fn add_key_clashes(&mut self, keymap: Option<&dyn Keymap>) {
        let in_effect: Vec<(usize, Identity, Source, Option<String>)> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.in_effect)
            .map(|(index, row)| {
                (
                    index,
                    row.line.identity(&row.mode, keymap),
                    row.source.clone(),
                    row.line.command.clone(),
                )
            })
            .collect();
        let mut added: Vec<(usize, Clash)> = Vec::new();
        for (a, identity_a, _, command_a) in &in_effect {
            for (b, identity_b, source_b, command_b) in &in_effect {
                if a == b {
                    continue;
                }
                if identity_a.kind == Kind::Sym
                    && identity_b.kind == Kind::Code
                    && same_key(identity_a, identity_b, keymap)
                {
                    added.push((*a, other(ClashKind::Shadowed, source_b, command_b.as_ref())));
                    if let Some((_, _, source_a, _)) =
                        in_effect.iter().find(|(index, ..)| index == a)
                    {
                        added.push((*b, other(ClashKind::Shadows, source_a, command_a.as_ref())));
                    }
                }
                if identity_a.locked
                    && identity_a.differs_in_lock_only(identity_b)
                    && command_a != command_b
                {
                    added.push((
                        *a,
                        other(ClashKind::LockScreenOnly, source_b, command_b.as_ref()),
                    ));
                }
            }
        }
        for (index, clash) in added {
            if let Some(row) = self.rows.get_mut(index) {
                row.clashes.push(clash);
            }
        }
    }

    /// The rows in effect that `line` (in `mode`) would clash with: the same binding for sway,
    /// a keysym and a keycode binding of the same key, or one that differs only in `--locked`.
    /// `skip` is the row being edited, which the new binding replaces.
    pub fn users_of(
        &self,
        line: &Line,
        mode: &str,
        keymap: Option<&dyn Keymap>,
        skip: Option<usize>,
    ) -> Vec<&Row> {
        let line = ours::resolve(line, self.modifier);
        let identity = line.identity(mode, keymap);
        self.rows
            .iter()
            .enumerate()
            .filter(|(index, row)| row.in_effect && Some(*index) != skip)
            .filter(|(_, row)| {
                let other = row.line.identity(&row.mode, keymap);
                other == identity
                    || identity.differs_in_lock_only(&other)
                    || (identity.kind != other.kind && same_key(&identity, &other, keymap))
            })
            .map(|(_, row)| row)
            .collect()
    }
}

/// Whether the keysym binding `a` and the keycode binding `b` (either way round) are on the same
/// keys, by `keymap`, with everything else the same.
fn same_key(a: &Identity, b: &Identity, keymap: Option<&dyn Keymap>) -> bool {
    let Some(keymap) = keymap else {
        return false;
    };
    let (sym, code) = match (a.kind, b.kind) {
        (Kind::Sym, Kind::Code) => (a, b),
        (Kind::Code, Kind::Sym) => (b, a),
        _ => return false,
    };
    let codes: Option<Vec<Key>> = sym
        .keys
        .iter()
        .map(|key| match key {
            Key::Sym(name) => keymap.keycode(name).map(Key::Code),
            other => Some(other.clone()),
        })
        .collect();
    let Some(mut codes) = codes else {
        return false;
    };
    codes.sort();
    Identity {
        kind: Kind::Code,
        keys: codes,
        ..sym.clone()
    } == *code
}

fn clash(kind: ClashKind, other: &Entry) -> Clash {
    Clash {
        kind,
        source: other.bound.source.clone(),
        command: other.bound.line.command.clone(),
    }
}

fn other(kind: ClashKind, source: &Source, command: Option<&String>) -> Clash {
    Clash {
        kind,
        source: source.clone(),
        command: command.cloned(),
    }
}
