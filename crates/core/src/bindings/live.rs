// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bringing a running sway to changed bindings without a reload.
//!
//! The bindings of the whole config are worked out twice, with the bindings of Sway Things
//! before and after the change, and only what differs in the end is sent: a binding that is gone
//! is unbound (after a placeholder of the same keys, so the unbind can't fail), a new or changed
//! one is bound with `--no-warn`. So undoing a change of a binding of the user's binds theirs
//! again, with its command from their config.

use std::collections::HashMap;
use std::path::Path;

use super::keys::Modifiers;
use super::line::{Identity, Line, LineError};
use super::listing::sequence_of;
use super::table::{DEFAULT_MODE, Table};
use crate::command::SwayCmd;
use crate::state::State;
use crate::sway_config::Directive;

/// The bindings in effect in the default mode, in the order sway read them.
fn in_effect(table: &Table) -> Vec<(Identity, &Line)> {
    table
        .in_effect()
        .filter(|(_, entry)| entry.bound.mode == DEFAULT_MODE)
        .map(|(_, entry)| {
            (
                entry.bound.line.identity(DEFAULT_MODE, None),
                &entry.bound.line,
            )
        })
        .collect()
}

/// The IPC commands that change the bindings of a running sway from those of `before` to those
/// of `now`, with the user's `directives`, of which sway reads those from `position` on after
/// the generated files. Bindings in other modes are left alone: Sway Things only binds in the
/// default mode.
///
/// # Errors
///
/// A [`LineError`] if a binding that changes can't be sent exactly (a binding of the user's
/// with a backslash in its command, keys that need quotes): then a reload of sway is the way.
pub fn commands(
    directives: &[Directive],
    position: Option<usize>,
    before: &State,
    now: &State,
) -> Result<Vec<SwayCmd>, LineError> {
    // Where the file is doesn't matter for what sway ends up with.
    let path = Path::new("50-bindings.conf");
    let old_table = Table::read(sequence_of(directives, position, before, path), None);
    let new_table = Table::read(sequence_of(directives, position, now, path), None);
    let old = in_effect(&old_table);
    let new = in_effect(&new_table);
    let old_by: HashMap<&Identity, &Line> = old.iter().map(|(id, line)| (id, *line)).collect();
    let new_by: HashMap<&Identity, &Line> = new.iter().map(|(id, line)| (id, *line)).collect();
    let mut commands = Vec::new();
    for (identity, line) in &old {
        // A binding that is gone, or one that sway keeps in another list now (`--to-code`
        // toggled, which a keymap would tell apart), is unbound first.
        let moved = new_by
            .get(identity)
            .is_some_and(|new| new.flags.to_code != line.flags.to_code);
        if new_by.contains_key(identity) && !moved {
            continue;
        }
        unbind(&mut commands, line)?;
    }
    for (identity, line) in &new {
        // Every option counts here, except `--no-warn`, which changes nothing in sway's list.
        let same = old_by.get(identity).is_some_and(|old| {
            let flags = |line: &Line| super::Flags {
                no_warn: false,
                ..line.flags
            };
            old.command == line.command && flags(old) == flags(line) && old.input == line.input
        });
        if !same {
            commands.push(SwayCmd::binding(line, Modifiers::NONE, true)?);
        }
    }
    Ok(commands)
}

/// Unbinds `line` after a placeholder of the same keys, so the unbind can't fail.
fn unbind(commands: &mut Vec<SwayCmd>, line: &Line) -> Result<(), LineError> {
    let unbind = line.unbinding();
    let placeholder = Line {
        unbind: false,
        command: Some("nop".to_owned()),
        ..unbind.clone()
    };
    commands.push(SwayCmd::binding(&placeholder, Modifiers::NONE, true)?);
    commands.push(SwayCmd::binding(&unbind, Modifiers::NONE, false)?);
    Ok(())
}
