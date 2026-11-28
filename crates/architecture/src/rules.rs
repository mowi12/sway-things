// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rules of the module graph.
//!
//! - [`LAYERS`] (R1): every top-level module of the core belongs to one layer and may use only its
//!   own layer and lower ones.
//! - [`MODULE_CYCLES`] (R2): no cycle between the top-level modules of the core.
//! - [`FILE_CYCLES`] (R2): no cycle between the modules of a crate, except inside one
//!   [cycle unit](cycle_unit): a top-level module, or in the GUI one `widgets::<x>`
//!   (`network::agent` and `network::nm::snapshot` may cycle, `widgets::keyboard` and
//!   `widgets::xkb_options` may not).
//!
//! - [`PARTICIPANTS`] (R7): `apply`, `generate` and `doctor` reach a feature only through the
//!   participant list (`core::participant`), not by naming the feature's module.
//!
//! - [`KEY_LITERALS`] (R4): no registry key as a string literal outside `core::registry`.
//! - [`HOOK_LITERALS`] (R4): no `sway-things hook` in a string literal outside `core::command`.
//! - [`STAND_INS`] (R5): a module `stand_in` is declared behind
//!   `#[cfg(any(test, feature = "test-support"))]`.
//! - [`BLOCKING_DOCS`] (R5): a function of the core has a `# Blocking` section in its docs exactly
//!   when it [blocks](crate::blocking).
//! - [`BLOCKING_CALLS`] (R5): nothing blocks the main context of the GUI or the executor of an
//!   `async fn` of the core.
//!
//! The cycle rules leave out the edges R1 reports: an edge to a higher layer is R1's, and every
//! cycle through it ends when it is repaired.

use std::collections::{BTreeMap, BTreeSet};

use crate::blocking::{Analysis, Target};
use crate::graph::{Edge, Graph, Module};
use crate::literals;

/// The rule of the layers of the core (R1).
pub const LAYERS: &str = "R1-layers";
/// The rule against cycles between the top-level modules of the core (R2).
pub const MODULE_CYCLES: &str = "R2-module-cycles";
/// The rule against cycles between the modules of a crate across its top-level modules (R2).
pub const FILE_CYCLES: &str = "R2-file-cycles";

/// The rule that the use cases and the assembly reach features through the participants (R7).
pub const PARTICIPANTS: &str = "R7-participants";

/// The top-level modules of the core that may name a feature only through `participant` (R7).
pub const THROUGH_PARTICIPANTS: &[&str] = &["apply", "generate", "doctor"];

/// The layer of the features, which [`THROUGH_PARTICIPANTS`] doesn't name.
const FEATURES: &str = "features";

/// The rule against registry keys spelled as string literals (R4).
pub const KEY_LITERALS: &str = "R4-key-literals";
/// The rule against hooks spelled out in string literals (R4).
pub const HOOK_LITERALS: &str = "R4-hook-literals";
/// The rule that stand-ins are built only for tests (R5).
pub const STAND_INS: &str = "R5-stand-ins";
/// The rule that the docs of a core function say whether it blocks (R5).
pub const BLOCKING_DOCS: &str = "R5-blocking-docs";
/// The rule against blocking calls on the main context and in async functions (R5).
pub const BLOCKING_CALLS: &str = "R5-blocking-calls";

/// The words a hand-written hook starts with.
pub const HOOK: &str = "sway-things hook";

/// The functions that read a directory but don't count as blocking (R5): fast scans with a small,
/// bounded result, each with the reason. They are no source of [blocking](crate::blocking).
pub const NOT_BLOCKING: &[(&str, &str)] = &[
    (
        "core::session::process::scan",
        "reads /proc, which the kernel serves from memory, for the processes of the session",
    ),
    (
        "core::outputs::session_processes",
        "reads /proc, which the kernel serves from memory, for the processes of the session",
    ),
    (
        "core::scanners::look::runs",
        "reads /proc, which the kernel serves from memory, for a running process",
    ),
    (
        "core::state::history::list",
        "reads the app's own state directory, which holds a few snapshots",
    ),
];

/// The names of [`NOT_BLOCKING`], for [`Analysis::build`].
pub fn not_blocking() -> Vec<&'static str> {
    NOT_BLOCKING.iter().map(|(name, _)| *name).collect()
}

/// The name of the core's unit.
pub const CORE: &str = "core";

/// One layer of the core.
#[derive(Debug)]
pub struct Layer {
    /// What the layer holds.
    pub name: &'static str,
    /// Its top-level modules.
    pub modules: &'static [&'static str],
}

/// The layers of the core, from the bottom up. A module may use its own layer and the ones before
/// it. A new top-level module must be added here.
pub const CORE_LAYERS: &[Layer] = &[
    Layer {
        name: "foundation",
        modules: &[
            "i18n", "atomic", "delegate", "dbus", "systemd", "version", "watch", "session",
            "manifest",
        ],
    },
    Layer {
        name: "sway",
        modules: &["command", "sway_config", "ipc"],
    },
    Layer {
        name: "registry",
        modules: &["registry"],
    },
    Layer {
        name: "state",
        modules: &["paths", "state", "include", "ui_state"],
    },
    Layer {
        name: "features",
        modules: &[
            "apps",
            "bar",
            "bindings",
            "bluetooth",
            "brightness",
            "capture",
            "conflict",
            "cursor",
            "devices",
            "editor",
            "environment",
            "gsettings",
            "look",
            "network",
            "night_light",
            "notifications",
            "outputs",
            "packages",
            "printers",
            "region",
            "scanners",
            "sound",
            "swayosd",
            "system",
            "theme",
            "wallpaper",
            "xkb",
        ],
    },
    Layer {
        name: "assembly",
        modules: &["participant", "generate", "view"],
    },
    Layer {
        name: "use cases",
        modules: &["apply", "doctor"],
    },
];

/// The layer of a top-level module of the core: its index (0 is the bottom) and name.
pub fn layer_of(module: &str) -> Option<(usize, &'static str)> {
    CORE_LAYERS
        .iter()
        .enumerate()
        .find(|(_, layer)| layer.modules.contains(&module))
        .map(|(index, layer)| (index, layer.name))
}

/// A broken rule: `from` uses `to` where it may not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The rule's id, e.g. [`LAYERS`].
    pub rule: &'static str,
    /// The using module, e.g. `core::registry::value`.
    pub from: String,
    /// The used module.
    pub to: String,
    /// Where it is used: `file:line`.
    pub location: String,
    /// What is wrong and what to do.
    pub message: String,
}

/// The top-level modules of the core that [`CORE_LAYERS`] doesn't place.
pub fn unplaced(graph: &Graph) -> Vec<String> {
    let core = core_unit(graph);
    graph
        .modules()
        .iter()
        .filter(|module| Some(module.unit) == core && module.path.len() == 1)
        .filter_map(|module| module.top())
        .filter(|top| layer_of(top).is_none())
        .map(str::to_owned)
        .collect()
}

fn core_unit(graph: &Graph) -> Option<usize> {
    graph
        .modules()
        .iter()
        .find(|module| module.path.is_empty() && module.name == CORE)
        .map(|module| module.unit)
}

fn location(graph: &Graph, edge: &Edge) -> String {
    let file = &graph.modules()[edge.from].file;
    match edge.lines.first() {
        Some(&line) if line > 0 => format!("{file}:{line}"),
        _ => file.clone(),
    }
}

/// The layers of an edge between two modules of the core, both placed: `(from, to)`.
fn edge_layers(graph: &Graph, core: Option<usize>, edge: &Edge) -> Option<(usize, usize)> {
    let modules = graph.modules();
    let (from, to) = (&modules[edge.from], &modules[edge.to]);
    if Some(from.unit) != core || from.unit != to.unit {
        return None;
    }
    Some((layer_of(from.top()?)?.0, layer_of(to.top()?)?.0))
}

/// Whether an edge points to a higher layer of the core, which [`layers`] reports. The cycle rules
/// leave these edges out: every cycle through one ends when it is repaired.
fn upward(graph: &Graph, core: Option<usize>, edge: &Edge) -> bool {
    edge_layers(graph, core, edge).is_some_and(|(from, to)| to > from)
}

/// The edges between modules of the core that point to a higher layer (R1).
pub fn layers(graph: &Graph) -> Vec<Violation> {
    let modules = graph.modules();
    let core = core_unit(graph);
    graph
        .edges()
        .iter()
        .filter_map(|edge| {
            let (from_layer, to_layer) = edge_layers(graph, core, edge)?;
            if to_layer <= from_layer {
                return None;
            }
            let (from, to) = (&modules[edge.from], &modules[edge.to]);
            let name = |layer: usize| CORE_LAYERS.get(layer).map_or("", |layer| layer.name);
            Some(Violation {
                rule: LAYERS,
                from: from.name.clone(),
                to: to.name.clone(),
                location: location(graph, edge),
                message: format!(
                    "{} (layer {from_layer}, {}) uses {} (layer {to_layer}, {}); a module may use only its own \
                     layer and lower ones. Move what it uses down to layer {from_layer} or lower, or move the user up",
                    from.name,
                    name(from_layer),
                    to.name,
                    name(to_layer)
                ),
            })
        })
        .collect()
}

/// The edges from `apply`, `generate` or `doctor` to a module of the features layer (R7): they
/// reach a feature through the participant list instead, so a new feature adds a participant
/// rather than an arm in each of them.
pub fn participants(graph: &Graph) -> Vec<Violation> {
    let modules = graph.modules();
    let core = core_unit(graph);
    graph
        .edges()
        .iter()
        .filter_map(|edge| {
            let (from, to) = (&modules[edge.from], &modules[edge.to]);
            if Some(from.unit) != core || from.unit != to.unit {
                return None;
            }
            let from_top = from.top()?;
            if !THROUGH_PARTICIPANTS.contains(&from_top) {
                return None;
            }
            let (_, to_layer) = layer_of(to.top()?)?;
            if to_layer != FEATURES {
                return None;
            }
            Some(Violation {
                rule: PARTICIPANTS,
                from: from.name.clone(),
                to: to.name.clone(),
                location: location(graph, edge),
                message: format!(
                    "{} uses the feature module {}; `apply`, `generate` and `doctor` reach a feature only \
                     through the participant list. Add what it needs to the feature's participant \
                     (core::participant) and use it from there",
                    from.name, to.name
                ),
            })
        })
        .collect()
}

/// The edges between top-level modules of the core that lie on a cycle (R2), leaving out the
/// edges [`layers`] reports.
pub fn module_cycles(graph: &Graph) -> Vec<Violation> {
    let modules = graph.modules();
    let core = core_unit(graph);
    // The top-level modules and, per pair, the first edge between modules in them.
    let mut tops: BTreeMap<String, usize> = BTreeMap::new();
    let mut names = Vec::new();
    let mut pairs: BTreeMap<(usize, usize), Edge> = BTreeMap::new();
    for edge in graph.edges() {
        let (from, to) = (&modules[edge.from], &modules[edge.to]);
        if Some(from.unit) != core || from.unit != to.unit || upward(graph, core, &edge) {
            continue;
        }
        let (Some(a), Some(b)) = (from.top(), to.top()) else {
            continue;
        };
        if a == b {
            continue;
        }
        let mut node = |top: &str| {
            let next = tops.len();
            *tops.entry(top.to_owned()).or_insert_with(|| {
                names.push(format!("{CORE}::{top}"));
                next
            })
        };
        let (a, b) = (node(a), node(b));
        pairs.entry((a, b)).or_insert(edge);
    }
    let adjacency = adjacency(names.len(), pairs.keys().copied());
    let mut out = Vec::new();
    for component in components(&adjacency) {
        let members: BTreeSet<usize> = component.iter().copied().collect();
        let mut cycle: Vec<&str> = members.iter().map(|&node| names[node].as_str()).collect();
        cycle.sort_unstable();
        for (&(a, b), edge) in &pairs {
            if members.contains(&a) && members.contains(&b) {
                out.push(Violation {
                    rule: MODULE_CYCLES,
                    from: names[a].clone(),
                    to: names[b].clone(),
                    location: location(graph, edge),
                    message: format!(
                        "{} uses {} (first: {} → {}), and both lie on a cycle among {}. Break the cycle: move \
                         what both need into a lower module, or invert the dependency",
                        names[a],
                        names[b],
                        modules[edge.from].name,
                        modules[edge.to].name,
                        cycle.join(", ")
                    ),
                });
            }
        }
    }
    out
}

/// The unit a module belongs to for [`file_cycles`]: the modules of one unit may depend on each
/// other in a cycle, modules of different units may not. It is the top-level module (`network`
/// for `network::nm::snapshot`), except in the GUI, where every `widgets::<x>` is a unit of its
/// own, one per page or building block (`widgets::apps` for `widgets::apps::dialogs`; `widgets`
/// itself is one too). `None` for a crate root.
pub fn cycle_unit(module: &Module) -> Option<&[String]> {
    let depth = if module.name.starts_with("gui::") && module.top() == Some("widgets") {
        2
    } else {
        1
    };
    module
        .path
        .get(..depth.min(module.path.len()))
        .filter(|unit| !unit.is_empty())
}

/// The edges on a cycle between modules of one crate that belong to different
/// [cycle units](cycle_unit) (R2), in every crate. A cycle inside one unit, between siblings
/// like `network::agent` and `network::nm` (with its children), is allowed. Crate roots are
/// left out, like the edges [`layers`] reports.
pub fn file_cycles(graph: &Graph) -> Vec<Violation> {
    let modules = graph.modules();
    let core = core_unit(graph);
    let edges: Vec<Edge> = graph
        .edges()
        .into_iter()
        .filter(|edge| {
            let (from, to) = (&modules[edge.from], &modules[edge.to]);
            from.unit == to.unit
                && from.parent.is_some()
                && to.parent.is_some()
                && !upward(graph, core, edge)
        })
        .collect();
    let adjacency = adjacency(modules.len(), edges.iter().map(|edge| (edge.from, edge.to)));
    let mut out = Vec::new();
    for component in components(&adjacency) {
        let members: BTreeSet<usize> = component.iter().copied().collect();
        let mut cycle: Vec<&str> = members
            .iter()
            .map(|&node| modules[node].name.as_str())
            .collect();
        cycle.sort_unstable();
        for edge in &edges {
            let (from, to) = (&modules[edge.from], &modules[edge.to]);
            if !members.contains(&edge.from)
                || !members.contains(&edge.to)
                || cycle_unit(from) == cycle_unit(to)
            {
                continue;
            }
            out.push(Violation {
                rule: FILE_CYCLES,
                from: from.name.clone(),
                to: to.name.clone(),
                location: location(graph, edge),
                message: format!(
                    "{} uses {}, and both lie on a cycle among {}, which crosses units (top-level \
                     modules; in the GUI each `widgets::<x>`). Break the cycle: move what both need into a module both may use, or invert the dependency",
                    from.name,
                    to.name,
                    cycle.join(", ")
                ),
            });
        }
    }
    out
}

/// The crate a module belongs to: `core`, `cli` or `gui` (a binary counts as its crate).
fn crate_of(module: &Module) -> &str {
    let unit = module.name.split("::").next().unwrap_or_default();
    unit.strip_suffix("-bin").unwrap_or(unit)
}

/// Whether a module is `parent` or below it, e.g. `core::registry::app` for `core::registry`.
fn within(module: &Module, parent: &str) -> bool {
    module.name == parent || module.name.starts_with(&format!("{parent}::"))
}

/// The string literals equal to a registry key outside `core::registry` (R4). The constants of the
/// registry name them instead.
pub fn key_literals(graph: &Graph, keys: &BTreeSet<String>) -> Vec<Violation> {
    let modules = graph.modules();
    literals::literals(graph)
        .into_iter()
        .filter(|literal| keys.contains(&literal.value))
        .filter_map(|literal| {
            let module = &modules[literal.module];
            if within(module, "core::registry") {
                return None;
            }
            Some(Violation {
                rule: KEY_LITERALS,
                from: module.name.clone(),
                to: literal.value.clone(),
                location: format!("{}:{}", module.file, literal.line),
                message: format!(
                    "{} spells the registry key \"{}\" as a literal. Use the constant of the registry module that \
                     defines the setting (export one there if there is none), so a renamed key can't miss a reader",
                    module.name, literal.value
                ),
            })
        })
        .collect()
}

/// The string literals that spell out a hook (`sway-things hook …`) outside `core::command` (R4).
/// A violation names the hook, so one per module and hook.
pub fn hook_literals(graph: &Graph) -> Vec<Violation> {
    let modules = graph.modules();
    literals::literals(graph)
        .into_iter()
        .filter_map(|literal| {
            let module = &modules[literal.module];
            let start = literal.value.find(HOOK)?;
            if within(module, "core::command") {
                return None;
            }
            let rest = &literal.value[start + HOOK.len()..];
            let name: String = rest
                .trim_start_matches(' ')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            let hook = if name.is_empty() {
                HOOK.to_owned()
            } else {
                format!("{HOOK} {name}")
            };
            Some(Violation {
                rule: HOOK_LITERALS,
                from: module.name.clone(),
                message: format!(
                    "{} spells out \"{hook}\" in a literal. Build it from `core::command::Hook` \
                     (`command::hook`, `Hook::command_line`), which the command line maps from, so a renamed hook \
                     can't miss a caller",
                    module.name
                ),
                to: hook,
                location: format!("{}:{}", module.file, literal.line),
            })
        })
        .collect()
}

/// The modules `stand_in` that aren't declared behind `#[cfg(any(test, feature = "test-support"))]`
/// (R5): a stand-in replaces a service or sway for tests, and the normal build must not have one.
pub fn stand_ins(graph: &Graph) -> Vec<Violation> {
    graph
        .modules()
        .iter()
        .flat_map(|module| {
            module
                .declarations()
                .iter()
                .filter(|declaration| declaration.name == "stand_in")
                .filter(|declaration| !declaration.test_support)
                .map(move |declaration| Violation {
                    rule: STAND_INS,
                    from: module.name.clone(),
                    to: format!("{}::stand_in", module.name),
                    location: format!("{}:{}", module.file, declaration.line),
                    message: format!(
                        "{}::stand_in isn't declared behind #[cfg(any(test, feature = \"test-support\"))]. Put that \
                         attribute on its `mod` item, so only tests build it",
                        module.name
                    ),
                })
        })
        .collect()
}

/// The functions of the core whose docs don't say right whether they block (R5): one that blocks
/// needs a `# Blocking` section, one that doesn't may not have one. Methods of trait impls are left
/// out; their docs are the trait's.
pub fn blocking_docs(graph: &Graph, analysis: &Analysis) -> Vec<Violation> {
    let modules = graph.modules();
    analysis
        .functions
        .iter()
        .enumerate()
        .filter(|(_, function)| crate_of(&modules[function.module]) == CORE && !function.trait_impl)
        .filter_map(|(node, function)| {
            let blocks = analysis.blocks(node);
            if blocks == function.documented {
                return None;
            }
            let (to, message) = if blocks {
                let why = analysis.blocking[node]
                    .and_then(|index| function.uses.get(index))
                    .map_or_else(
                        || "it is a primitive".to_owned(),
                        |used| format!("line {}: {}", used.line, analysis.chain(graph, &used.target)),
                    );
                (
                    "missing # Blocking",
                    format!(
                        "{} blocks ({why}), but its docs have no `# Blocking` section. Add one that says what it \
                         waits for, so callers on the main context know to use gio::spawn_blocking",
                        function.name
                    ),
                )
            } else {
                (
                    "needless # Blocking",
                    format!(
                        "{} has a `# Blocking` section, but nothing it calls blocks. Remove the section (or, if \
                         it does block through a method call the test can't follow, say so in the code)",
                        function.name
                    ),
                )
            };
            Some(Violation {
                rule: BLOCKING_DOCS,
                from: function.name.clone(),
                to: to.to_owned(),
                location: function.location.clone(),
                message,
            })
        })
        .collect()
}

/// The GUI functions that run only off the main context: every function that calls them by path
/// does so inside `gio::spawn_blocking` (or `std::thread::spawn`), or is such a function itself.
/// A function no other one calls by path is none: a signal handler, a method or a page's setup
/// runs on the main context.
fn gui_workers(graph: &Graph, analysis: &Analysis) -> BTreeSet<usize> {
    let modules = graph.modules();
    let mut workers: BTreeSet<usize> = analysis
        .functions
        .iter()
        .enumerate()
        .filter(|&(node, function)| {
            crate_of(&modules[function.module]) == "gui" && !analysis.callers[node].is_empty()
        })
        .map(|(node, _)| node)
        .collect();
    loop {
        let before = workers.len();
        let kept: BTreeSet<usize> = workers
            .iter()
            .copied()
            .filter(|&node| {
                analysis.callers[node].iter().all(|&(caller, index)| {
                    analysis.functions[caller]
                        .uses
                        .get(index)
                        .is_some_and(|used| used.off_thread)
                        || workers.contains(&caller)
                })
            })
            .collect();
        workers = kept;
        if workers.len() == before {
            return workers;
        }
    }
}

/// The calls that block where nothing may (R5):
/// - in the GUI, on the main context: a use of a primitive or of a blocking function of the core,
///   outside `gio::spawn_blocking`, in a GUI function that may run on the main context (see
///   [`gui_workers`]). A call from one GUI function to another isn't reported: the place where the
///   GUI reaches the primitive or the core is;
/// - in the body of an `async fn` of the core, which blocks the executor that polls it.
///
/// The command line is left out: it has no main loop to block.
pub fn blocking_calls(graph: &Graph, analysis: &Analysis) -> Vec<Violation> {
    let modules = graph.modules();
    let workers = gui_workers(graph, analysis);
    let mut out = Vec::new();
    for (node, function) in analysis.functions.iter().enumerate() {
        let krate = crate_of(&modules[function.module]);
        let gui = krate == "gui" && !workers.contains(&node);
        let core_async = krate == CORE && function.is_async;
        if !gui && !core_async {
            continue;
        }
        for used in &function.uses {
            if used.off_thread || !analysis.target_blocks(&used.target) {
                continue;
            }
            if let Target::Function(module, _) = &used.target {
                if gui && crate_of(&modules[*module]) == "gui" {
                    continue;
                }
            }
            let to = analysis.target_name(graph, &used.target);
            let chain = analysis.chain(graph, &used.target);
            let file = function.location.split(':').next().unwrap_or_default();
            let message = if gui {
                format!(
                    "{} calls {to} on the main context, and it blocks ({chain}). Run it in \
                     gio::spawn_blocking and hand the result back to the main context",
                    function.name
                )
            } else {
                format!(
                    "the async fn {} calls {to}, which blocks ({chain}) the executor that polls it. Use an \
                     async counterpart, or run it in a thread",
                    function.name
                )
            };
            out.push(Violation {
                rule: BLOCKING_CALLS,
                from: function.name.clone(),
                to,
                location: format!("{file}:{}", used.line),
                message,
            });
        }
    }
    out
}

fn adjacency(nodes: usize, edges: impl Iterator<Item = (usize, usize)>) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); nodes];
    for (from, to) in edges {
        if let Some(list) = adjacency.get_mut(from) {
            list.push(to);
        }
    }
    adjacency
}

/// The strongly connected components with more than one node (Tarjan's algorithm).
fn components(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct State<'a> {
        adjacency: &'a [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        out: Vec<Vec<usize>>,
    }

    fn visit(state: &mut State<'_>, node: usize) {
        state.index[node] = Some(state.next);
        state.low[node] = state.next;
        state.next += 1;
        state.stack.push(node);
        state.on_stack[node] = true;
        let adjacency = state.adjacency;
        for &next in &adjacency[node] {
            match state.index[next] {
                None => {
                    visit(state, next);
                    state.low[node] = state.low[node].min(state.low[next]);
                }
                Some(index) if state.on_stack[next] => state.low[node] = state.low[node].min(index),
                Some(_) => {}
            }
        }
        if Some(state.low[node]) == state.index[node] {
            let mut component = Vec::new();
            while let Some(member) = state.stack.pop() {
                state.on_stack[member] = false;
                component.push(member);
                if member == node {
                    break;
                }
            }
            if component.len() > 1 {
                component.sort_unstable();
                state.out.push(component);
            }
        }
    }

    let nodes = adjacency.len();
    let mut state = State {
        adjacency,
        index: vec![None; nodes],
        low: vec![0; nodes],
        on_stack: vec![false; nodes],
        stack: Vec::new(),
        next: 0,
        out: Vec::new(),
    };
    for node in 0..nodes {
        if state.index[node].is_none() {
            visit(&mut state, node);
        }
    }
    state.out.sort();
    state.out
}
