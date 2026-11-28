// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Changes to the items of `shortcuts.bindings`, as the Keyboard Shortcuts page makes them: add a
//! binding, change or remove one of the user's or of Sway Things, and undo a change. Each returns
//! the new list of items; the page stores it, and the applier sends the difference to sway.
//!
//! A binding of the user's is never edited in their file: it is removed with an `unbindsym` item,
//! and changed with a change item (`unbindsym … => bindsym …`). A new binding on keys a binding
//! of the user's has takes its place the same way, so sway never warns about a binding that
//! replaces another.

use super::listing::{Listing, Row, RowKind};
use super::ours::{Item, item_text, resolve};
use super::table::DEFAULT_MODE;
use crate::sway_config::bindings::{Keymap, Line};

/// `items` without the items at `indexes`.
fn without(items: &[String], indexes: &[usize]) -> Vec<String> {
    items
        .iter()
        .enumerate()
        .filter(|(index, _)| !indexes.contains(index))
        .map(|(_, item)| item.clone())
        .collect()
}

/// What a new binding `line` takes the place of, other than the row at `skip`.
#[derive(Default)]
struct Displaced {
    /// Unbinds of the user's bindings on the same keys.
    unbinds: Vec<Line>,
    /// Items of Sway Things' bindings on the same keys, which go.
    added: Vec<usize>,
    /// Change items whose new binding is on the same keys: they keep only their unbind.
    changed: Vec<(usize, Line)>,
}

impl Displaced {
    fn of(
        listing: &Listing,
        line: &Line,
        keymap: Option<&dyn Keymap>,
        skip: Option<usize>,
    ) -> Self {
        let identity = resolve(line, listing.modifier()).identity(DEFAULT_MODE, keymap);
        let mut displaced = Self::default();
        for (index, row) in listing.rows().iter().enumerate() {
            if !row.in_effect
                || Some(index) == skip
                || row.line.identity(&row.mode, keymap) != identity
            {
                continue;
            }
            match &row.kind {
                RowKind::Hand if row.editable() => displaced.unbinds.push(row.line.unbinding()),
                RowKind::Added { item } => displaced.added.push(*item),
                RowKind::Changed { item, original } => {
                    displaced.changed.push((*item, original.unbinding()));
                }
                _ => {}
            }
        }
        displaced
    }

    /// `items` with the bindings of Sway Things it takes the place of gone, and the items at
    /// `also` (by the same indexes) too.
    fn clear(&self, items: &[String], also: Option<usize>) -> Vec<String> {
        let mut items = items.to_vec();
        for (item, unbind) in &self.changed {
            if let Some(slot) = items.get_mut(*item) {
                *slot = item_text(unbind);
            }
        }
        let mut gone = self.added.clone();
        gone.extend(also);
        without(&items, &gone)
    }
}

/// The items with a new binding `line` added. A binding of the user's on the same keys is
/// changed to it (or, for several, the others removed); one of Sway Things on the same keys is
/// replaced.
pub fn add(
    listing: &Listing,
    items: &[String],
    line: &Line,
    keymap: Option<&dyn Keymap>,
) -> Vec<String> {
    let mut displaced = Displaced::of(listing, line, keymap, None);
    let mut items = displaced.clear(items, None);
    let last = displaced.unbinds.pop();
    let unbinds = displaced.unbinds;
    items.extend(unbinds.iter().map(item_text));
    items.push(match last {
        Some(from) => Item::Change {
            from,
            to: line.clone(),
        }
        .text(),
        None => item_text(line),
    });
    items
}

/// The items with the binding of the row at `index` changed to `line`. A binding of the user's
/// gets a change item; changed back to exactly what the user's config binds, the change goes.
pub fn change(
    listing: &Listing,
    items: &[String],
    index: usize,
    line: &Line,
    keymap: Option<&dyn Keymap>,
) -> Vec<String> {
    let Some(row) = listing.rows().get(index) else {
        return items.to_vec();
    };
    let displaced = Displaced::of(listing, line, keymap, Some(index));
    let mut gone = None;
    let items = match &row.kind {
        RowKind::Hand => {
            let mut items = items.to_vec();
            items.push(
                Item::Change {
                    from: row.line.unbinding(),
                    to: line.clone(),
                }
                .text(),
            );
            items
        }
        RowKind::Added { item } => {
            let mut items = items.to_vec();
            if let Some(slot) = items.get_mut(*item) {
                *slot = item_text(line);
            }
            items
        }
        RowKind::Changed { item, original } => {
            if same_binding(original, line) {
                gone = Some(*item);
                items.to_vec()
            } else {
                let mut items = items.to_vec();
                if let Some(slot) = items.get_mut(*item) {
                    *slot = Item::Change {
                        from: original.unbinding(),
                        to: line.clone(),
                    }
                    .text();
                }
                items
            }
        }
        RowKind::Removed { .. }
        | RowKind::Group { .. }
        | RowKind::Feature { .. }
        | RowKind::App { .. } => {
            return items.to_vec();
        }
    };
    // The bindings it takes the place of: Sway Things' go, the user's are removed. Their unbinds
    // are plain items, not changes.
    let mut items = displaced.clear(&items, gone);
    items.extend(displaced.unbinds.iter().map(item_text));
    items
}

/// Whether `a` and `b` bind the same keys with the same options to the same command.
fn same_binding(a: &Line, b: &Line) -> bool {
    a.identity(DEFAULT_MODE, None) == b.identity(DEFAULT_MODE, None)
        && a.command == b.command
        && a.flags == b.flags
}

/// The items with the binding of the row at `index` removed: one of the user's gets an unbind
/// item, one of Sway Things goes, a changed one keeps only its unbind.
pub fn remove(listing: &Listing, items: &[String], index: usize) -> Vec<String> {
    let Some(row) = listing.rows().get(index) else {
        return items.to_vec();
    };
    let mut items = items.to_vec();
    match &row.kind {
        RowKind::Hand => items.push(item_text(&row.line.unbinding())),
        RowKind::Added { item } => items = without(&items, &[*item]),
        RowKind::Changed { item, original } => {
            if let Some(slot) = items.get_mut(*item) {
                *slot = item_text(&original.unbinding());
            }
        }
        RowKind::Removed { .. }
        | RowKind::Group { .. }
        | RowKind::Feature { .. }
        | RowKind::App { .. } => {}
    }
    items
}

/// The items without the change or removal of the user's binding in the row at `index`: their
/// config's binding is back. Only that item goes.
pub fn restore(listing: &Listing, items: &[String], index: usize) -> Vec<String> {
    match listing.rows().get(index).map(|row| &row.kind) {
        Some(RowKind::Changed { item, .. } | RowKind::Removed { item }) => without(items, &[*item]),
        _ => items.to_vec(),
    }
}

/// Whether the row can be restored: a binding of the user's that Sway Things changed or removes.
pub fn can_restore(row: &Row) -> bool {
    matches!(row.kind, RowKind::Changed { .. } | RowKind::Removed { .. })
}
