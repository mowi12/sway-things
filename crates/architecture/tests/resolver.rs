// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The resolver on small fixture crates: which paths become which edges.

use std::collections::BTreeMap;

use architecture::baseline::{self, Entry, PLACEHOLDER};
use architecture::graph::{Files, Graph, Unit};
use architecture::rules;

fn unit(name: &str, extern_name: Option<&str>, files: &[(&str, &str)]) -> Unit {
    Unit {
        name: name.to_owned(),
        extern_name: extern_name.map(str::to_owned),
        dir: format!("crates/{name}/src"),
        root: "lib.rs".to_owned(),
        files: Files::Memory(
            files
                .iter()
                .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
                .collect(),
        ),
    }
}

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn graph(files: &[(&str, &str)]) -> Result<Graph> {
    Ok(Graph::build(&[unit(
        "core",
        Some("sway_things_core"),
        files,
    )])?)
}

fn edges(graph: &Graph) -> Vec<(String, String)> {
    graph.edge_names().into_iter().collect()
}

fn edge(from: &str, to: &str) -> (String, String) {
    (from.to_owned(), to.to_owned())
}

#[test]
fn use_trees_with_groups_renames_and_self_resolve_to_the_defining_modules() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod a; mod b; mod c;"),
        ("a.rs", "pub struct A; pub fn f() {}"),
        ("b.rs", "pub mod inner { pub const X: u8 = 1; }"),
        (
            "c.rs",
            "use crate::{a::{self, A as Renamed}, b::inner::X};\nfn g() { a::f(); }",
        ),
    ])?;
    assert_eq!(
        edges(&graph),
        [
            edge("core::c", "core::a"),
            edge("core::c", "core::b::inner")
        ]
    );
    Ok(())
}

#[test]
fn importing_a_module_is_no_edge_but_a_path_through_it_is() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod a; mod b; mod c;"),
        ("a.rs", "pub fn f() {}"),
        ("b.rs", "pub fn f() {}"),
        ("c.rs", "use crate::a;\nuse crate::b;\nfn g() { b::f(); }"),
    ])?;
    assert_eq!(edges(&graph), [edge("core::c", "core::b")]);
    Ok(())
}

#[test]
fn super_and_nested_files_resolve() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod a;"),
        ("a/mod.rs", "mod x; mod y; pub struct Shared;"),
        ("a/x.rs", "use super::Shared;\nfn f() { super::y::g(); }"),
        ("a/y.rs", "pub fn g() {}"),
    ])?;
    assert_eq!(
        edges(&graph),
        [
            edge("core::a::x", "core::a"),
            edge("core::a::x", "core::a::y")
        ]
    );
    Ok(())
}

#[test]
fn inline_modules_are_nodes_and_find_their_files() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod a; mod b;"),
        (
            "a.rs",
            "mod inline { pub fn f() { crate::b::g(); } }\nmod nested { mod deep; }",
        ),
        ("a/nested/deep.rs", "fn h() { super::super::inline::f(); }"),
        ("b.rs", "pub fn g() {}"),
    ])?;
    assert_eq!(
        edges(&graph),
        [
            edge("core::a::inline", "core::b"),
            edge("core::a::nested::deep", "core::a::inline")
        ]
    );
    Ok(())
}

#[test]
fn re_exports_resolve_to_the_defining_module_also_when_chained() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod low; mod mid; mod top; mod user;"),
        ("low.rs", "pub const MODE: &str = \"mode\";"),
        ("mid.rs", "pub use crate::low::MODE;"),
        ("top.rs", "pub use crate::mid::MODE as TOP_MODE;"),
        (
            "user.rs",
            "use crate::top::TOP_MODE;\nfn f() -> &'static str { crate::mid::MODE }",
        ),
    ])?;
    assert_eq!(
        edges(&graph),
        [
            edge("core::mid", "core::low"),
            edge("core::top", "core::low"),
            edge("core::user", "core::low")
        ]
    );
    Ok(())
}

#[test]
fn globs_import_names_and_re_export_them() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod a; mod b; mod c; mod d;"),
        ("a.rs", "pub struct A;"),
        ("b.rs", "pub use crate::a::*;\npub fn own() {}"),
        ("c.rs", "fn f() { let _ = crate::b::A; crate::b::own(); }"),
        ("d.rs", "use crate::b::*;"),
    ])?;
    assert_eq!(
        edges(&graph),
        [
            edge("core::b", "core::a"),
            edge("core::c", "core::a"),
            edge("core::c", "core::b"),
            edge("core::d", "core::b")
        ]
    );
    Ok(())
}

#[test]
fn import_loops_end() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod a; mod b; mod c;"),
        ("a.rs", "pub use crate::b::*;"),
        ("b.rs", "pub use crate::a::*;"),
        ("c.rs", "fn f() { crate::a::Missing::new(); }"),
    ])?;
    assert!(edges(&graph).contains(&edge("core::c", "core::a")));
    Ok(())
}

#[test]
fn test_only_items_files_doc_comments_and_visibilities_are_skipped() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod a; mod b; mod stand_in_user; mod visibility;"),
        ("a.rs", "pub fn f() {}"),
        (
            "b.rs",
            "/// Calls [`crate::a::f`].\n\
             pub fn g() {}\n\
             #[cfg(test)]\nuse crate::a::f;\n\
             #[cfg(test)]\nmod tests;\n\
             #[cfg(any(test, feature = \"test-support\"))]\npub mod stand_in { pub fn s() { crate::a::f(); } }\n\
             #[cfg(doc)]\nuse crate::a::f as for_docs;\n\
             impl Foo { #[cfg(test)] fn t() { crate::a::f(); } }\n\
             struct Foo;",
        ),
        ("b/tests.rs", "use crate::a::f;"),
        (
            "stand_in_user.rs",
            "#[cfg(not(test))]\nfn f() { crate::a::f(); }",
        ),
        // A visibility limits who may use an item; it uses nothing.
        ("visibility.rs", "pub(in crate::a) fn v() {}"),
    ])?;
    assert_eq!(edges(&graph), [edge("core::stand_in_user", "core::a")]);
    assert!(graph.module("core::b::tests").is_none());
    assert!(graph.module("core::b::stand_in").is_none());
    Ok(())
}

#[test]
fn paths_in_macros_and_attributes_count() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod a; mod b; mod c; mod d;"),
        ("a.rs", "pub fn f() -> u8 { 1 }"),
        ("b.rs", "pub fn g() -> u8 { 2 }"),
        (
            "c.rs",
            "use crate::b;\n\
             fn h() { println!(\"{} {}\", crate::a::f(), b::g()); }\n\
             macro_rules! m { ($name:ident) => { $name::x(); $crate::d::y(); } }",
        ),
        (
            "d.rs",
            "#[serde(with = \"x\")]\n#[my_attr(crate::a::f)]\npub fn y() {}",
        ),
    ])?;
    assert_eq!(
        edges(&graph),
        [
            edge("core::c", "core::a"),
            edge("core::c", "core::b"),
            edge("core::c", "core::d"),
            edge("core::d", "core::a")
        ]
    );
    Ok(())
}

#[test]
fn front_ends_reach_the_core_by_its_crate_name() -> Result {
    let core = unit(
        "core",
        Some("sway_things_core"),
        &[
            ("lib.rs", "pub mod state; pub mod outputs;"),
            ("state.rs", "pub fn load() {}"),
            ("outputs.rs", "pub use crate::state::load;"),
        ],
    );
    let gui = unit(
        "gui",
        Some("sway_things_gui"),
        &[
            ("lib.rs", "mod window;"),
            (
                "window.rs",
                "use sway_things_core::outputs::load;\nfn f() { ::sway_things_core::state::load(); }",
            ),
        ],
    );
    let graph = Graph::build(&[core, gui])?;
    assert_eq!(
        edges(&graph),
        [
            edge("core::outputs", "core::state"),
            edge("gui::window", "core::state")
        ]
    );
    Ok(())
}

#[test]
fn edges_know_their_lines() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod a; mod b;"),
        ("a.rs", "pub fn f() {}"),
        ("b.rs", "\n\nfn g() {\n    crate::a::f();\n}"),
    ])?;
    let edge = graph.edges().into_iter().next().unwrap();
    assert_eq!(edge.lines, [4]);
    Ok(())
}

#[test]
fn a_file_that_doesnt_parse_is_an_error_with_its_place() {
    let error = Graph::build(&[unit(
        "core",
        None,
        &[("lib.rs", "mod a;"), ("a.rs", "fn (")],
    )])
    .err()
    .unwrap();
    assert!(
        error.to_string().starts_with("crates/core/src/a.rs:1:"),
        "{error}"
    );
}

fn layered(files: &[(&str, &str)]) -> Result<Vec<(String, String, String)>> {
    let graph = graph(files)?;
    let mut found: Vec<_> = rules::layers(&graph)
        .into_iter()
        .chain(rules::module_cycles(&graph))
        .chain(rules::file_cycles(&graph))
        .map(|violation| (violation.rule.to_owned(), violation.from, violation.to))
        .collect();
    found.sort();
    Ok(found)
}

fn found(rule: &str, from: &str, to: &str) -> (String, String, String) {
    (rule.to_owned(), from.to_owned(), to.to_owned())
}

#[test]
fn an_edge_to_a_higher_layer_breaks_the_layers_but_no_cycle_rule() -> Result {
    let violations = layered(&[
        ("lib.rs", "mod registry; mod outputs;"),
        (
            "registry.rs",
            "pub const MODE: u8 = 0;\nfn f() { crate::outputs::g(); }",
        ),
        ("outputs.rs", "pub fn g() -> u8 { crate::registry::MODE }"),
    ])?;
    assert_eq!(
        violations,
        [found(rules::LAYERS, "core::registry", "core::outputs")]
    );
    Ok(())
}

#[test]
fn a_cycle_between_top_level_modules_is_reported_but_not_one_between_siblings() -> Result {
    let violations = layered(&[
        ("lib.rs", "mod apps; mod bindings; mod network;"),
        ("apps.rs", "pub fn a() { crate::bindings::b(); }"),
        ("bindings.rs", "pub fn b() { crate::apps::a(); }"),
        ("network/mod.rs", "mod agent; mod nm;"),
        (
            "network/agent.rs",
            "pub fn a() { super::nm::snapshot::s(); }",
        ),
        ("network/nm/mod.rs", "pub mod snapshot;"),
        (
            "network/nm/snapshot.rs",
            "pub fn s() { crate::network::agent::a(); }",
        ),
    ])?;
    assert_eq!(
        violations,
        [
            found(rules::FILE_CYCLES, "core::apps", "core::bindings"),
            found(rules::FILE_CYCLES, "core::bindings", "core::apps"),
            found(rules::MODULE_CYCLES, "core::apps", "core::bindings"),
            found(rules::MODULE_CYCLES, "core::bindings", "core::apps"),
        ]
    );
    Ok(())
}

#[test]
fn a_module_cycle_needs_no_cycle_between_files() -> Result {
    let violations = layered(&[
        ("lib.rs", "mod apps; mod bindings;"),
        ("apps/mod.rs", "mod x; mod y;"),
        ("apps/x.rs", "pub fn x() { crate::bindings::z::z(); }"),
        ("apps/y.rs", "pub fn y() {}"),
        ("bindings/mod.rs", "pub mod z; mod w;"),
        ("bindings/z.rs", "pub fn z() {}"),
        ("bindings/w.rs", "fn w() { crate::apps::y::y(); }"),
    ])?;
    assert_eq!(
        violations,
        [
            found(rules::MODULE_CYCLES, "core::apps", "core::bindings"),
            found(rules::MODULE_CYCLES, "core::bindings", "core::apps"),
        ]
    );
    Ok(())
}

#[test]
fn the_baseline_keeps_reasons_and_refuses_placeholders() -> Result {
    let graph = graph(&[
        ("lib.rs", "mod registry; mod outputs; mod apps;"),
        (
            "registry.rs",
            "fn f() { crate::outputs::g(); crate::apps::h(); }",
        ),
        ("outputs.rs", "pub fn g() {}"),
        ("apps.rs", "pub fn h() {}"),
    ])?;
    let violations = rules::layers(&graph);
    let old = vec![
        Entry {
            rule: rules::LAYERS.to_owned(),
            from: "core::registry".to_owned(),
            to: "core::outputs".to_owned(),
            why: "Moves down with the output names.".to_owned(),
        },
        Entry {
            rule: rules::LAYERS.to_owned(),
            from: "core::registry".to_owned(),
            to: "core::gone".to_owned(),
            why: "Repaired already.".to_owned(),
        },
    ];
    let comparison = baseline::compare(&old, &violations);
    assert_eq!(
        comparison
            .new
            .iter()
            .map(|violation| violation.to.as_str())
            .collect::<Vec<_>>(),
        ["core::apps"]
    );
    assert_eq!(
        comparison
            .stale
            .iter()
            .map(|entry| entry.to.as_str())
            .collect::<Vec<_>>(),
        ["core::gone"]
    );

    let updated = baseline::update(&old, &violations);
    let text = baseline::render(&updated);
    let parsed = baseline::parse(&text).unwrap();
    let whys: BTreeMap<_, _> = parsed
        .iter()
        .map(|entry| (entry.to.as_str(), entry.why.as_str()))
        .collect();
    assert_eq!(
        whys,
        BTreeMap::from([
            ("core::apps", PLACEHOLDER),
            ("core::outputs", "Moves down with the output names.")
        ])
    );
    let comparison = baseline::compare(&parsed, &violations);
    assert!(comparison.new.is_empty() && comparison.stale.is_empty());
    assert_eq!(
        comparison
            .unexplained
            .iter()
            .map(|entry| entry.to.as_str())
            .collect::<Vec<_>>(),
        ["core::apps"]
    );
    Ok(())
}

#[test]
fn in_the_gui_each_widget_module_is_a_unit_of_its_own() -> Result {
    let gui = unit(
        "gui",
        Some("sway_things_gui"),
        &[
            ("lib.rs", "mod widgets;"),
            ("widgets/mod.rs", "mod apps; mod keyboard; mod xkb_options;"),
            ("widgets/apps/mod.rs", "mod autostart; mod dialogs;"),
            (
                "widgets/apps/autostart.rs",
                "pub fn a() { super::dialogs::d(); }",
            ),
            (
                "widgets/apps/dialogs.rs",
                "pub fn d() { super::autostart::a(); }",
            ),
            (
                "widgets/keyboard.rs",
                "pub fn k() { super::xkb_options::x(); }",
            ),
            (
                "widgets/xkb_options.rs",
                "pub fn x() { super::keyboard::k(); }",
            ),
        ],
    );
    let graph = Graph::build(&[gui])?;
    let units: Vec<_> = [
        "gui::widgets",
        "gui::widgets::apps::dialogs",
        "gui::widgets::keyboard",
    ]
    .iter()
    .map(|name| {
        graph
            .module(name)
            .and_then(|module| rules::cycle_unit(&graph.modules()[module]))
            .map(|unit| unit.join("::"))
    })
    .collect();
    assert_eq!(
        units,
        [
            Some("widgets".to_owned()),
            Some("widgets::apps".to_owned()),
            Some("widgets::keyboard".to_owned())
        ]
    );
    let found: Vec<_> = rules::file_cycles(&graph)
        .into_iter()
        .map(|violation| (violation.from, violation.to))
        .collect();
    assert_eq!(
        found,
        [
            edge("gui::widgets::keyboard", "gui::widgets::xkb_options"),
            edge("gui::widgets::xkb_options", "gui::widgets::keyboard")
        ]
    );
    Ok(())
}
