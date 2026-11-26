// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which functions block: a call graph over the functions of the workspace, seeded with the
//! primitives that wait for something.
//!
//! **Nodes** are the functions with a body: free functions, associated functions and methods
//! (named after the type, `core::session::lock::Lock::wait`), and the default methods of traits.
//! Functions nested in a body and the items the module graph leaves out (`#[cfg(test)]`,
//! `test-support`) are none.
//!
//! **Primitives** ([`Primitive`]):
//! - `std::thread::sleep`;
//! - `std::fs::read_dir`, and every method call `.read_dir()` (`Path::read_dir`);
//! - `block_on` of an executor (`async_io::block_on`, `futures_lite::future::block_on`, …);
//! - synchronous `std::process`: a method call `.status()`, `.output()`, `.wait()` or
//!   `.wait_with_output()` in a function that names `std::process::Command` or
//!   `std::process::Child` (also imported). Methods can't be resolved without types, so this is a
//!   heuristic: a function that only gets a `Command` from another one and runs it is missed, and
//!   one that names a `std::process::Command` and calls `.output()` on something else counts;
//! - `core::session::lock::Lock::wait`, which waits for another process to give up a lock.
//!
//! Whole-file reads and writes with `std::fs` are no primitives: they are short and local. The
//! functions given to [`Analysis::build`] as exempt (fast, bounded scans) never block.
//!
//! **Calls** are paths that name a function node: `a::b::f(..)`, `Type::f(..)`, `Self::f(..)`,
//! `f(..)` after `use a::b::f`, and such a path passed as a value (`.map(Self::read)`). Method calls
//! (`x.f()`) are not followed: without types their receiver is unknown. Calls inside macro bodies
//! that parse as expressions (`format!`, `glib::clone!`) count.
//!
//! A function **blocks** when its body uses a primitive or calls a function that blocks. What runs
//! in the closure (or the function) given to `gio::spawn_blocking` or `std::thread::spawn` runs on
//! another thread and doesn't count; closures given to anything else run as part of the body.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::Span;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ImplItem, Item, TraitItem};

use crate::graph::{self, Graph, Named};

/// Something that waits, the start of every chain of blocking calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Primitive {
    /// `std::thread::sleep`.
    Sleep,
    /// `std::fs::read_dir` or `Path::read_dir`.
    ReadDir,
    /// `block_on` of an executor.
    BlockOn,
    /// `status`, `output`, `wait` or `wait_with_output` of `std::process`.
    Process,
    /// `core::session::lock::Lock::wait`.
    LockWait,
}

impl Primitive {
    /// How a violation names it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Sleep => "std::thread::sleep",
            Self::ReadDir => "read_dir",
            Self::BlockOn => "block_on",
            Self::Process => "std::process (synchronous)",
            Self::LockWait => "core::session::lock::Lock::wait",
        }
    }
}

/// The functions that are primitives themselves, by their node's name.
const PRIMITIVE_FUNCTIONS: &[(&str, Primitive)] =
    &[("core::session::lock::Lock::wait", Primitive::LockWait)];

/// The crates whose `block_on` runs a future to its end on the calling thread.
const EXECUTORS: &[&str] = &[
    "async_io",
    "futures_lite",
    "futures",
    "futures_executor",
    "pollster",
    "glib",
    "gio",
    "gtk",
    "blocking",
];

/// What a use in a body targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A primitive.
    Primitive(Primitive),
    /// A function: its module and path inside it, the key of [`Analysis::by_key`].
    Function(usize, Vec<String>),
}

/// One use of a primitive or a function in a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    /// What it uses.
    pub target: Target,
    /// Its line.
    pub line: usize,
    /// Whether it is inside what is given to `spawn_blocking` or `thread::spawn`.
    pub off_thread: bool,
}

/// A function node.
#[derive(Debug, Clone)]
pub struct Function {
    /// Its module, an index into the graph's modules.
    pub module: usize,
    /// The module that defines its type or itself, and its path there, e.g. `["Lock", "wait"]`.
    pub key: (usize, Vec<String>),
    /// Its full name, e.g. `core::session::lock::Lock::wait`.
    pub name: String,
    /// The file and line of its signature.
    pub location: String,
    /// Whether it is an `async fn`.
    pub is_async: bool,
    /// Whether it implements a trait method (`impl Trait for Type`).
    pub trait_impl: bool,
    /// Whether its doc comment has a `# Blocking` section.
    pub documented: bool,
    /// The primitives and functions its body uses.
    pub uses: Vec<Use>,
}

/// The call graph and which of its functions block.
pub struct Analysis {
    /// The function nodes.
    pub functions: Vec<Function>,
    /// The nodes per key; several for one key when the same name is defined twice (trait impls).
    pub by_key: BTreeMap<(usize, Vec<String>), Vec<usize>>,
    /// Per node, why it blocks: the index of the use in its `uses` that does; `None` when it doesn't.
    pub blocking: Vec<Option<usize>>,
    /// Per node, its uses by other nodes: the using node and the index into its `uses`.
    pub callers: Vec<Vec<(usize, usize)>>,
}

impl Analysis {
    /// Builds the call graph of every module of the graph and finds the functions that block.
    /// The functions named in `exempt` (full names, e.g. `core::state::history::list`) never
    /// block: what they wait for is short and bounded.
    pub fn build(graph: &Graph, exempt: &[&str]) -> Self {
        let mut functions = Vec::new();
        for (index, module) in graph.modules().iter().enumerate() {
            collect(graph, index, module.items(), &mut functions);
        }
        let mut by_key: BTreeMap<(usize, Vec<String>), Vec<usize>> = BTreeMap::new();
        for (index, function) in functions.iter().enumerate() {
            by_key.entry(function.key.clone()).or_default().push(index);
        }
        // Drop the uses that name no node: a type's constructor, a variant, a constant.
        for function in &mut functions {
            function.uses.retain(|used| match &used.target {
                Target::Primitive(_) => true,
                Target::Function(module, path) => by_key.contains_key(&(*module, path.clone())),
            });
        }
        let mut callers = vec![Vec::new(); functions.len()];
        for (caller, function) in functions.iter().enumerate() {
            for (index, used) in function.uses.iter().enumerate() {
                if let Target::Function(module, path) = &used.target {
                    for &callee in by_key.get(&(*module, path.clone())).into_iter().flatten() {
                        callers[callee].push((caller, index));
                    }
                }
            }
        }
        let mut analysis = Self {
            blocking: vec![None; functions.len()],
            functions,
            by_key,
            callers,
        };
        analysis.propagate(exempt);
        analysis
    }

    fn propagate(&mut self, exempt: &[&str]) {
        let exempt: Vec<bool> = self
            .functions
            .iter()
            .map(|function| exempt.contains(&function.name.as_str()))
            .collect();
        for (index, function) in self.functions.iter().enumerate() {
            if PRIMITIVE_FUNCTIONS
                .iter()
                .any(|(name, _)| *name == function.name)
            {
                // Its own uses explain it, if it has one; else it is the primitive.
                self.blocking[index] = Some(usize::MAX);
            }
        }
        loop {
            let mut changed = false;
            for (index, &exempt) in exempt.iter().enumerate() {
                if self.blocking[index].is_some() || exempt {
                    continue;
                }
                let found = self.functions[index]
                    .uses
                    .iter()
                    .position(|used| !used.off_thread && self.target_blocks(&used.target));
                if found.is_some() {
                    self.blocking[index] = found;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Whether a target is a primitive or a function that blocks.
    pub fn target_blocks(&self, target: &Target) -> bool {
        match target {
            Target::Primitive(_) => true,
            Target::Function(module, path) => self
                .by_key
                .get(&(*module, path.clone()))
                .is_some_and(|nodes| nodes.iter().any(|&node| self.blocking[node].is_some())),
        }
    }

    /// Whether a node blocks.
    pub fn blocks(&self, node: usize) -> bool {
        self.blocking[node].is_some()
    }

    /// How a target is named in a violation: the primitive, or the function's name.
    pub fn target_name(&self, graph: &Graph, target: &Target) -> String {
        match target {
            Target::Primitive(primitive) => primitive.name().to_owned(),
            Target::Function(module, path) => {
                format!("{}::{}", graph.modules()[*module].name, path.join("::"))
            }
        }
    }

    /// Why a target blocks, as the chain down to a primitive: `a → b → std::thread::sleep`.
    pub fn chain(&self, graph: &Graph, target: &Target) -> String {
        let mut parts = vec![self.target_name(graph, target)];
        let mut seen = BTreeSet::new();
        let mut current = target.clone();
        while let Target::Function(module, path) = &current {
            let Some(node) = self
                .by_key
                .get(&(*module, path.clone()))
                .and_then(|nodes| nodes.iter().copied().find(|&node| self.blocks(node)))
            else {
                break;
            };
            if !seen.insert(node) {
                break;
            }
            let Some(next) = self.blocking[node]
                .and_then(|index| self.functions[node].uses.get(index))
                .map(|used| used.target.clone())
            else {
                if let Some((_, primitive)) = PRIMITIVE_FUNCTIONS
                    .iter()
                    .find(|(name, _)| *name == self.functions[node].name)
                {
                    parts.push(primitive.name().to_owned());
                }
                break;
            };
            parts.push(self.target_name(graph, &next));
            current = next;
        }
        parts.dedup();
        parts.join(" → ")
    }
}

/// Whether doc attributes have a `# Blocking` heading.
fn documented(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let syn::Meta::NameValue(pair) = &attr.meta else {
            return false;
        };
        let Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(text),
            ..
        }) = &pair.value
        else {
            return false;
        };
        attr.path().is_ident("doc") && text.value().lines().any(|line| line.trim() == "# Blocking")
    })
}

fn path_segments(path: &syn::Path) -> Vec<String> {
    let mut segments = Vec::new();
    if path.leading_colon.is_some() {
        segments.push("::".to_owned());
    }
    segments.extend(
        path.segments
            .iter()
            .map(|segment| segment.ident.to_string()),
    );
    segments
}

/// The functions of one module's items.
fn collect(graph: &Graph, module: usize, items: &[Item], out: &mut Vec<Function>) {
    let file = &graph.modules()[module].file;
    let name_of = |key: &(usize, Vec<String>)| {
        format!("{}::{}", graph.modules()[key.0].name, key.1.join("::"))
    };
    for item in items {
        if graph::test_only(graph::item_attrs(item)) {
            continue;
        }
        match item {
            Item::Fn(function) => {
                let key = (module, vec![function.sig.ident.to_string()]);
                let uses = body_uses(graph, module, None, &function.sig, &function.block);
                out.push(Function {
                    module,
                    name: name_of(&key),
                    key,
                    location: format!("{file}:{}", graph::line(function.sig.ident.span())),
                    is_async: function.sig.asyncness.is_some(),
                    trait_impl: false,
                    documented: documented(&function.attrs),
                    uses,
                });
            }
            Item::Impl(implementation) => {
                let Some(owner) = self_type(graph, module, &implementation.self_ty) else {
                    continue;
                };
                for item in &implementation.items {
                    let ImplItem::Fn(function) = item else {
                        continue;
                    };
                    if graph::test_only(&function.attrs) {
                        continue;
                    }
                    let mut path = owner.1.clone();
                    path.push(function.sig.ident.to_string());
                    let key = (owner.0, path);
                    let uses =
                        body_uses(graph, module, Some(&owner), &function.sig, &function.block);
                    out.push(Function {
                        module,
                        name: name_of(&key),
                        key,
                        location: format!("{file}:{}", graph::line(function.sig.ident.span())),
                        is_async: function.sig.asyncness.is_some(),
                        trait_impl: implementation.trait_.is_some(),
                        documented: documented(&function.attrs),
                        uses,
                    });
                }
            }
            Item::Trait(definition) => {
                let owner = (module, vec![definition.ident.to_string()]);
                for item in &definition.items {
                    let TraitItem::Fn(function) = item else {
                        continue;
                    };
                    let Some(block) = &function.default else {
                        continue;
                    };
                    if graph::test_only(&function.attrs) {
                        continue;
                    }
                    let mut path = owner.1.clone();
                    path.push(function.sig.ident.to_string());
                    let key = (owner.0, path);
                    let uses = body_uses(graph, module, Some(&owner), &function.sig, block);
                    out.push(Function {
                        module,
                        name: name_of(&key),
                        key,
                        location: format!("{file}:{}", graph::line(function.sig.ident.span())),
                        is_async: function.sig.asyncness.is_some(),
                        trait_impl: false,
                        documented: documented(&function.attrs),
                        uses,
                    });
                }
            }
            _ => {}
        }
    }
}

/// The module and path that define the type of an `impl`.
fn self_type(graph: &Graph, module: usize, ty: &syn::Type) -> Option<(usize, Vec<String>)> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let segments = path_segments(&path.path);
    match graph.resolve_named(module, &segments) {
        Some(Named::Item(owner, path)) => Some((owner, path)),
        _ => Some((module, vec![segments.last()?.clone()])),
    }
}

fn body_uses(
    graph: &Graph,
    module: usize,
    owner: Option<&(usize, Vec<String>)>,
    signature: &syn::Signature,
    block: &syn::Block,
) -> Vec<Use> {
    let mut body = Body {
        graph,
        module,
        owner,
        off_thread: false,
        uses: Vec::new(),
        names_process: false,
        process_calls: Vec::new(),
    };
    body.visit_signature(signature);
    body.visit_block(block);
    let mut uses = body.uses;
    if body.names_process {
        uses.extend(
            body.process_calls
                .into_iter()
                .map(|(line, off_thread)| Use {
                    target: Target::Primitive(Primitive::Process),
                    line,
                    off_thread,
                }),
        );
    }
    uses.sort_by_key(|used| used.line);
    uses
}

/// Collects the uses of one body.
struct Body<'g> {
    graph: &'g Graph,
    module: usize,
    owner: Option<&'g (usize, Vec<String>)>,
    off_thread: bool,
    uses: Vec<Use>,
    names_process: bool,
    process_calls: Vec<(usize, bool)>,
}

impl Body<'_> {
    fn push(&mut self, target: Target, span: Span) {
        self.uses.push(Use {
            target,
            line: graph::line(span),
            off_thread: self.off_thread,
        });
    }

    /// A path in an expression: a primitive, a function, or nothing to record.
    fn expr_path(&mut self, path: &syn::Path) {
        let segments = path_segments(path);
        let span = path
            .segments
            .first()
            .map_or_else(Span::call_site, |segment| segment.ident.span());
        let expanded = self.graph.expand(self.module, &segments);
        if let Some(primitive) = primitive(&expanded) {
            self.push(Target::Primitive(primitive), span);
            return;
        }
        let named = if segments.first().is_some_and(|first| first == "Self") {
            self.owner.map(|(owner, path)| {
                let mut path = path.clone();
                path.extend(segments.iter().skip(1).cloned());
                Named::Item(*owner, path)
            })
        } else {
            self.graph.resolve_named(self.module, &segments)
        };
        if let Some(Named::Item(module, path)) = named {
            self.push(Target::Function(module, path), span);
        }
    }

    /// Whether a called path runs what it is given on another thread.
    fn spawns(&self, path: &syn::Path) -> bool {
        let expanded = self.graph.expand(self.module, &path_segments(path));
        let tail: Vec<&str> = expanded.iter().map(String::as_str).collect();
        tail.ends_with(&["gio", "spawn_blocking"])
            || tail == ["std", "thread", "spawn"]
            || tail == ["thread", "spawn"]
    }

    fn macro_body(&mut self, mac: &syn::Macro) {
        if let Ok(exprs) = mac.parse_body_with(Punctuated::<Expr, syn::Token![,]>::parse_terminated)
        {
            for expr in &exprs {
                self.visit_expr(expr);
            }
        } else if let Ok(statements) = mac.parse_body_with(syn::Block::parse_within) {
            for statement in &statements {
                self.visit_stmt(statement);
            }
        }
    }
}

/// The primitive a path outside the workspace names, if any.
fn primitive(path: &[String]) -> Option<Primitive> {
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    match path.as_slice() {
        ["std", "thread", "sleep"] => Some(Primitive::Sleep),
        ["std", "fs", "read_dir"] => Some(Primitive::ReadDir),
        [first, .., "block_on"] if EXECUTORS.contains(first) => Some(Primitive::BlockOn),
        _ => None,
    }
}

fn names_process(path: &[String]) -> bool {
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    matches!(path.as_slice(), ["std", "process", "Command" | "Child", ..])
}

impl<'ast> Visit<'ast> for Body<'_> {
    // A nested function, impl or module is no part of the body.
    fn visit_item(&mut self, _: &'ast Item) {}

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        let spawns = matches!(&*call.func, Expr::Path(path) if self.spawns(&path.path));
        match &*call.func {
            Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => {
                self.expr_path(&path.path);
            }
            func => self.visit_expr(func),
        }
        let outer = self.off_thread;
        if spawns {
            self.off_thread = true;
        }
        for arg in &call.args {
            self.visit_expr(arg);
        }
        self.off_thread = outer;
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        // A single name is a function only when called: `x` alone is mostly a local variable,
        // which may share the name of a function.
        if path.qself.is_none()
            && (path.path.segments.len() > 1 || path.path.leading_colon.is_some())
        {
            self.expr_path(&path.path);
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        let line = graph::line(call.method.span());
        match method.as_str() {
            "read_dir" => self.push(Target::Primitive(Primitive::ReadDir), call.method.span()),
            "status" | "output" | "wait" | "wait_with_output" => {
                self.process_calls.push((line, self.off_thread));
            }
            _ => {}
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if names_process(&self.graph.expand(self.module, &path_segments(path))) {
            self.names_process = true;
        }
        visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.macro_body(mac);
        visit::visit_macro(self, mac);
    }
}
