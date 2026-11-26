// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The architecture test: the layers and cycles of the module graph of `core`, `cli` and `gui`
//! match the baseline in `baseline.toml`.
//!
//! Rewrite the baseline with `SWAY_THINGS_UPDATE_BASELINE=1 cargo test -p sway-things-architecture`,
//! then give every new entry its reason; see `wiki/Testing.md`, "Architecture tests".

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use architecture::baseline::{self, Entry};
use architecture::graph::Graph;
use architecture::rules::{self, Violation};
use architecture::workspace;

const UPDATE: &str = "SWAY_THINGS_UPDATE_BASELINE";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn graph() -> Result<Graph, architecture::Error> {
    Graph::build(&workspace::units(&root()))
}

#[test]
fn every_top_level_module_of_the_core_has_a_layer() {
    let unplaced = rules::unplaced(&graph().unwrap());
    assert!(
        unplaced.is_empty(),
        "these top-level modules of the core have no layer: {}. Place each in CORE_LAYERS in \
         crates/architecture/src/rules.rs, in the lowest layer whose modules it may use, and in the layer table of \
         wiki/Architecture.md",
        unplaced.join(", ")
    );
}

#[test]
fn the_layers_and_cycles_match_the_baseline() {
    let graph = graph().unwrap();
    let mut violations: Vec<Violation> = rules::layers(&graph);
    violations.extend(rules::module_cycles(&graph));
    violations.extend(rules::file_cycles(&graph));

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("baseline.toml");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let entries = match baseline::parse(&text) {
        Ok(entries) => entries,
        Err(error) => panic!("{error}"),
    };
    let entries: Vec<Entry> = if std::env::var_os(UPDATE).is_some_and(|value| value == "1") {
        let updated = baseline::update(&entries, &violations);
        fs::write(&path, baseline::render(&updated)).unwrap();
        updated
    } else {
        entries
    };

    let comparison = baseline::compare(&entries, &violations);
    if comparison.is_clean() {
        return;
    }
    let mut report = String::new();
    for violation in &comparison.new {
        let _ = writeln!(
            report,
            "{}: {}: {} → {}\n  {}.\n  If it can't be repaired now, add it to the baseline with a reason: \
             {UPDATE}=1 cargo test -p sway-things-architecture, then write its `why`.\n",
            violation.rule, violation.location, violation.from, violation.to, violation.message
        );
    }
    for entry in &comparison.stale {
        let _ = writeln!(
            report,
            "{}: {} → {} no longer occurs: remove it from crates/architecture/baseline.toml (or rewrite the file \
             with {UPDATE}=1).\n",
            entry.rule, entry.from, entry.to
        );
    }
    for entry in &comparison.unexplained {
        let _ = writeln!(
            report,
            "{}: {} → {} has no reason: write in its `why` in crates/architecture/baseline.toml why it is \
             accepted and how it will be repaired.\n",
            entry.rule, entry.from, entry.to
        );
    }
    panic!("the module graph doesn't match the baseline:\n\n{report}");
}
