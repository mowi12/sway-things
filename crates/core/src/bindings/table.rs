// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bindings of a config as sway ends up with them: every binding line in the order sway
//! reads it, and what became of it.
//!
//! sway keeps a list of bindings per mode and kind of keys (`binding_upsert` and
//! `binding_remove` in `sway/commands/bind.c`): a binding with the [`Identity`] of one in the list
//! takes its place, an unbind removes the one with its identity (and fails if there is none).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::sway_config::Directive;
use crate::sway_config::bindings::{Identity, Keymap, Line, Origin};
use crate::sway_config::line::strip_quotes;

/// The mode outside `mode` blocks.
pub const DEFAULT_MODE: &str = "default";

/// Where a binding line is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The file.
    pub file: Arc<Path>,
    /// The number of its first line, counted from 1.
    pub line: usize,
    /// The line as written.
    pub text: String,
    /// Who wrote it.
    pub origin: Origin,
    /// Whether sway reads it after the generated files, so that Sway Things can't change it.
    pub after_ours: bool,
}

/// A binding line with its mode and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    /// The mode it is in, [`DEFAULT_MODE`] outside `mode` blocks.
    pub mode: String,
    /// The binding.
    pub line: Line,
    /// Where it is.
    pub source: Source,
}

/// What became of a binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fate {
    /// sway has it.
    InEffect,
    /// A later line with the same keys took its place: the entry at this index.
    Replaced(usize),
    /// An unbind removed it.
    Removed(Box<Bound>),
}

/// A binding and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The binding.
    pub bound: Bound,
    /// What became of it.
    pub fate: Fate,
    /// The entries it took the place of.
    pub replaces: Vec<usize>,
}

/// The bindings of a config, in the order sway reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    /// Every binding (not the unbinds), in order.
    pub entries: Vec<Entry>,
    /// Unbinds that found no binding to remove; sway reports an error for each.
    pub failed: Vec<Bound>,
}

/// The mode, command name and arguments of `directive` if it can be a binding line: at the top,
/// in a `mode` block, or a `mode <name> bindsym …` line.
pub fn binding_parts(directive: &Directive) -> Option<(String, &str, &[String])> {
    let mode_name = |words: &[String]| -> Option<String> {
        let words = match words.split_first() {
            Some((first, rest)) if first == "--pango_markup" => rest,
            _ => words,
        };
        words
            .first()
            .map(|name| String::from_utf8_lossy(&strip_quotes(name.as_bytes())).into_owned())
    };
    match directive.block.as_slice() {
        [] if directive.name.eq_ignore_ascii_case("mode") => {
            let args = match directive.args.split_first() {
                Some((first, rest)) if first == "--pango_markup" => rest,
                _ => directive.args.as_slice(),
            };
            let (name, rest) = args.split_first()?;
            let (verb, args) = rest.split_first()?;
            Some((mode_name(std::slice::from_ref(name))?, verb.as_str(), args))
        }
        [] => Some((
            DEFAULT_MODE.to_owned(),
            directive.name.as_str(),
            &directive.args,
        )),
        [mode, words @ ..] if mode.eq_ignore_ascii_case("mode") => {
            Some((mode_name(words)?, directive.name.as_str(), &directive.args))
        }
        _ => None,
    }
}

impl Table {
    /// The bindings of the lines in `bound`, in the order sway reads them. `keymap` translates
    /// `--to-code` bindings to keycodes.
    pub fn read(bound: impl IntoIterator<Item = Bound>, keymap: Option<&dyn Keymap>) -> Self {
        let mut table = Self::default();
        let mut active: HashMap<Identity, usize> = HashMap::new();
        for bound in bound {
            let identity = bound.line.identity(&bound.mode, keymap);
            if bound.line.unbind {
                match active.remove(&identity) {
                    Some(index) => {
                        if let Some(entry) = table.entries.get_mut(index) {
                            entry.fate = Fate::Removed(Box::new(bound));
                        }
                    }
                    None => table.failed.push(bound),
                }
                continue;
            }
            let index = table.entries.len();
            let mut replaces = Vec::new();
            if let Some(old) = active.insert(identity, index) {
                if let Some(entry) = table.entries.get_mut(old) {
                    entry.fate = Fate::Replaced(index);
                }
                replaces.push(old);
            }
            table.entries.push(Entry {
                bound,
                fate: Fate::InEffect,
                replaces,
            });
        }
        table
    }

    /// The bindings sway has in the end, with their indexes.
    pub fn in_effect(&self) -> impl Iterator<Item = (usize, &Entry)> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.fate == Fate::InEffect)
    }
}

/// The binding lines of `directives`, as written by the user. `after` is the index of the first
/// one sway reads after the generated files.
pub fn user_lines(directives: &[Directive], after: Option<usize>) -> Vec<Bound> {
    directives
        .iter()
        .enumerate()
        .filter_map(|(index, directive)| {
            let (mode, name, args) = binding_parts(directive)?;
            let line = Line::read(name, args).ok()?;
            Some(Bound {
                mode,
                line,
                source: Source {
                    file: Arc::clone(&directive.file),
                    line: directive.line,
                    text: directive.text.clone(),
                    origin: Origin::User,
                    after_ours: after.is_some_and(|after| index >= after),
                },
            })
        })
        .collect()
}
