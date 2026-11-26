// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The baseline: the violations that are accepted until they are repaired, one entry per rule and
//! edge, each with the reason and the planned repair.
//!
//! The architecture test fails on a violation that isn't in the baseline and on an entry that no
//! longer occurs, so the list only shrinks. Rewriting it (`SWAY_THINGS_UPDATE_BASELINE=1`) keeps
//! the reason of the entries that stay and gives new ones [`PLACEHOLDER`], which the test refuses
//! until a person replaces it with the reason.

use std::collections::BTreeMap;

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

use crate::Error;
use crate::rules::Violation;

/// The reason a rewritten baseline gives a new entry; the test refuses it.
pub const PLACEHOLDER: &str = "TODO: why this is accepted and how it will be repaired";

const HEADER: &str = "\
# The violations of the architecture rules that are accepted until they are repaired: one entry per rule and edge, with
# the reason and the planned repair in `why`. The test fails on a violation that isn't listed and on an entry that no
# longer occurs. Rewrite it with `SWAY_THINGS_UPDATE_BASELINE=1 cargo test -p sway-things-architecture`, then replace
# every placeholder `why` with the reason; see wiki/Testing.md, \"Architecture tests\".
";

/// One accepted violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The rule's id.
    pub rule: String,
    /// The using module.
    pub from: String,
    /// The used module.
    pub to: String,
    /// Why it is accepted and how it will be repaired.
    pub why: String,
}

impl Entry {
    fn key(&self) -> (&str, &str, &str) {
        (&self.rule, &self.from, &self.to)
    }

    /// Whether `why` still needs a person to write it.
    pub fn unexplained(&self) -> bool {
        let why = self.why.trim();
        why.is_empty() || why == PLACEHOLDER || why.starts_with("TODO")
    }
}

fn key(violation: &Violation) -> (&str, &str, &str) {
    (violation.rule, &violation.from, &violation.to)
}

/// Reads a baseline.
///
/// # Errors
///
/// When the text isn't TOML or an entry lacks a field.
pub fn parse(text: &str) -> Result<Vec<Entry>, Error> {
    let document: DocumentMut = text
        .parse()
        .map_err(|error| Error::new(format!("baseline.toml: {error}")))?;
    let Some(item) = document.get("violation") else {
        return Ok(Vec::new());
    };
    let tables = item.as_array_of_tables().ok_or_else(|| {
        Error::new("baseline.toml: `violation` must be an array of tables ([[violation]])")
    })?;
    tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            let field = |name: &str| {
                table
                    .get(name)
                    .and_then(Item::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        Error::new(format!(
                            "baseline.toml: entry {} has no string `{name}`",
                            index + 1
                        ))
                    })
            };
            Ok(Entry {
                rule: field("rule")?,
                from: field("from")?,
                to: field("to")?,
                why: field("why")?,
            })
        })
        .collect()
}

/// Writes a baseline, sorted by rule, `from` and `to`.
pub fn render(entries: &[Entry]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.key().cmp(&b.key()));
    sorted.dedup_by(|a, b| a.key() == b.key());
    let mut tables = ArrayOfTables::new();
    for entry in &sorted {
        let mut table = Table::new();
        table.insert("rule", value(entry.rule.as_str()));
        table.insert("from", value(entry.from.as_str()));
        table.insert("to", value(entry.to.as_str()));
        table.insert("why", value(entry.why.as_str()));
        tables.push(table);
    }
    let mut document = DocumentMut::new();
    document.insert("violation", Item::ArrayOfTables(tables));
    format!("{HEADER}\n{document}")
}

/// What the violations found and the baseline disagree on.
#[derive(Debug, Default)]
pub struct Comparison<'a> {
    /// Violations that aren't in the baseline.
    pub new: Vec<&'a Violation>,
    /// Entries that no longer occur.
    pub stale: Vec<&'a Entry>,
    /// Entries without a reason.
    pub unexplained: Vec<&'a Entry>,
}

impl Comparison<'_> {
    /// Whether the violations match the baseline and every entry has a reason.
    pub fn is_clean(&self) -> bool {
        self.new.is_empty() && self.stale.is_empty() && self.unexplained.is_empty()
    }
}

/// Compares the violations found with the baseline.
pub fn compare<'a>(entries: &'a [Entry], violations: &'a [Violation]) -> Comparison<'a> {
    let known: BTreeMap<_, _> = entries.iter().map(|entry| (entry.key(), entry)).collect();
    let found: BTreeMap<_, _> = violations
        .iter()
        .map(|violation| (key(violation), violation))
        .collect();
    Comparison {
        new: found
            .iter()
            .filter(|(key, _)| !known.contains_key(*key))
            .map(|(_, &violation)| violation)
            .collect(),
        stale: known
            .iter()
            .filter(|(key, _)| !found.contains_key(*key))
            .map(|(_, &entry)| entry)
            .collect(),
        unexplained: entries.iter().filter(|entry| entry.unexplained()).collect(),
    }
}

/// The baseline for these violations: the entries that still occur keep their reason, new ones get
/// [`PLACEHOLDER`].
pub fn update(entries: &[Entry], violations: &[Violation]) -> Vec<Entry> {
    let known: BTreeMap<_, _> = entries.iter().map(|entry| (entry.key(), entry)).collect();
    violations
        .iter()
        .map(|violation| Entry {
            rule: violation.rule.to_owned(),
            from: violation.from.clone(),
            to: violation.to.clone(),
            why: known
                .get(&key(violation))
                .map_or_else(|| PLACEHOLDER.to_owned(), |entry| entry.why.clone()),
        })
        .collect()
}
