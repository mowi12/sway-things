// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The module graph: one node per module (a file or an inline `mod x { … }`), one edge per module
//! that a module uses.
//!
//! Edges come from `use` items (groups, globs, renames, `self`, `super`, `crate`), from paths of two
//! or more segments in expressions, types, patterns and attributes, and from `sway_things_core::…`
//! and the other crates' names. A path resolves to the module that *defines* the item: a
//! `pub use crate::…` re-export is followed, transitively, so reaching an item through a module
//! that re-exports it is an edge to the defining module. Importing a module (`use crate::state;`)
//! is no edge by itself; the paths through it (`state::load`) are.
//!
//! Skipped: items behind `#[cfg(test)]`, `#[cfg(doc)]` or the `test-support` feature, `tests.rs`
//! files, and doc comments. Macro bodies are token streams, not syntax: they are scanned for
//! `a::b` sequences that resolve to a module of the workspace, best effort (a path built by the
//! macro itself is missed).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, ImplItem, Item, Meta, TraitItem, UseTree};

use crate::Error;

/// Where the source files of a [`Unit`] come from.
#[derive(Debug, Clone)]
pub enum Files {
    /// Read from this directory.
    Disk(PathBuf),
    /// In memory, by path relative to the source directory (fixtures of the resolver's tests).
    Memory(BTreeMap<String, String>),
}

impl Files {
    fn read(&self, path: &str) -> Option<String> {
        match self {
            Self::Disk(dir) => fs::read_to_string(dir.join(path)).ok(),
            Self::Memory(files) => files.get(path).cloned(),
        }
    }
}

/// One crate root: a library or a binary.
#[derive(Debug, Clone)]
pub struct Unit {
    /// The short name that starts the names of its modules, e.g. `core` for `core::state::file`.
    pub name: String,
    /// The name other crates use for it (`sway_things_core`); `None` for a binary.
    pub extern_name: Option<String>,
    /// The source directory relative to the workspace, for messages, e.g. `crates/core/src`.
    pub dir: String,
    /// The root file relative to the source directory, e.g. `lib.rs`.
    pub root: String,
    /// The source files.
    pub files: Files,
}

/// A module of the graph.
pub struct Module {
    /// The full name: the unit's name and the module path, e.g. `core::state::file`.
    pub name: String,
    /// Index of the unit in the list given to [`Graph::build`].
    pub unit: usize,
    /// The module path below the crate root; empty for the root.
    pub path: Vec<String>,
    /// The file that holds it, relative to the workspace.
    pub file: String,
    /// The parent module; `None` for a crate root.
    pub parent: Option<usize>,
    children: BTreeMap<String, usize>,
    defined: BTreeSet<String>,
    imports: BTreeMap<String, Vec<String>>,
    globs: Vec<Vec<String>>,
    items: Vec<Item>,
}

impl Module {
    /// The top-level module this module belongs to (its first path segment); `None` for the root.
    pub fn top(&self) -> Option<&str> {
        self.path.first().map(String::as_str)
    }
}

/// An edge: `from` uses something defined in `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// The using module.
    pub from: usize,
    /// The module that defines what is used.
    pub to: usize,
    /// The lines in `from`'s file where it is used, sorted; 0 where unknown.
    pub lines: Vec<usize>,
}

/// The module graph of a set of units.
pub struct Graph {
    modules: Vec<Module>,
    roots: Vec<usize>,
    externs: BTreeMap<String, usize>,
    edges: BTreeMap<(usize, usize), BTreeSet<usize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Module(usize),
    /// An item (function, type, constant, …) defined in this module.
    Item(usize),
}

impl Target {
    fn module(self) -> usize {
        match self {
            Self::Module(module) | Self::Item(module) => module,
        }
    }
}

/// The `(module, name)` lookups a resolution has made, so an import or glob loop ends.
type Seen = BTreeSet<(usize, String)>;

impl Graph {
    /// Reads and parses the units and resolves every path.
    ///
    /// # Errors
    ///
    /// When a root file is missing or a file doesn't parse.
    pub fn build(units: &[Unit]) -> Result<Self, Error> {
        let mut graph = Self {
            modules: Vec::new(),
            roots: Vec::new(),
            externs: BTreeMap::new(),
            edges: BTreeMap::new(),
        };
        for (index, unit) in units.iter().enumerate() {
            let text = unit
                .files
                .read(&unit.root)
                .ok_or_else(|| Error::new(format!("{}/{} is missing", unit.dir, unit.root)))?;
            let file = parse(unit, &unit.root, &text)?;
            let root = graph.add_module(unit, index, &[], &unit.root, None, file.items, "")?;
            graph.roots.push(root);
            if let Some(name) = &unit.extern_name {
                graph.externs.insert(name.clone(), root);
            }
        }
        for module in 0..graph.modules.len() {
            let mut collector = Edges {
                graph: &graph,
                from: module,
                found: Vec::new(),
            };
            for item in &graph.modules[module].items {
                collector.visit_item(item);
            }
            let found = collector.found;
            for (to, line) in found {
                if to != module {
                    graph.edges.entry((module, to)).or_default().insert(line);
                }
            }
        }
        Ok(graph)
    }

    /// All modules; an edge's `from` and `to` index into it.
    pub fn modules(&self) -> &[Module] {
        &self.modules
    }

    /// The module with this full name.
    pub fn module(&self, name: &str) -> Option<usize> {
        self.modules.iter().position(|module| module.name == name)
    }

    /// All edges, sorted by `from` and `to`.
    pub fn edges(&self) -> Vec<Edge> {
        self.edges
            .iter()
            .map(|(&(from, to), lines)| Edge {
                from,
                to,
                lines: lines.iter().copied().collect(),
            })
            .collect()
    }

    /// The edges as pairs of module names, for tests.
    pub fn edge_names(&self) -> BTreeSet<(String, String)> {
        self.edges
            .keys()
            .map(|&(from, to)| {
                (
                    self.modules[from].name.clone(),
                    self.modules[to].name.clone(),
                )
            })
            .collect()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the position of a module in the tree has these parts"
    )]
    fn add_module(
        &mut self,
        unit: &Unit,
        unit_index: usize,
        path: &[String],
        file: &str,
        parent: Option<usize>,
        items: Vec<Item>,
        child_dir: &str,
    ) -> Result<usize, Error> {
        let id = self.modules.len();
        let name = if path.is_empty() {
            unit.name.clone()
        } else {
            format!("{}::{}", unit.name, path.join("::"))
        };
        self.modules.push(Module {
            name,
            unit: unit_index,
            path: path.to_vec(),
            file: format!("{}/{file}", unit.dir),
            parent,
            children: BTreeMap::new(),
            defined: BTreeSet::new(),
            imports: BTreeMap::new(),
            globs: Vec::new(),
            items: Vec::new(),
        });
        let mut kept = Vec::new();
        for item in items {
            if test_only(item_attrs(&item)) {
                continue;
            }
            let Item::Mod(module) = item else {
                if let Some(defined) = item_name(&item) {
                    self.modules[id].defined.insert(defined);
                }
                kept.push(item);
                continue;
            };
            let child_name = module.ident.to_string();
            let mut child_path = path.to_vec();
            child_path.push(child_name.clone());
            let dir = join(child_dir, &child_name);
            let child = if let Some((_, inner)) = module.content {
                Some(self.add_module(unit, unit_index, &child_path, file, Some(id), inner, &dir)?)
            } else {
                let flat = format!("{dir}.rs");
                let nested = format!("{dir}/mod.rs");
                let found = [flat, nested]
                    .into_iter()
                    .find_map(|path| unit.files.read(&path).map(|text| (path, text)));
                match found {
                    // A file of tests only, kept out like the `#[cfg(test)]` items.
                    Some((path, _)) if path.ends_with("/tests.rs") || path == "tests.rs" => None,
                    Some((child_file, text)) => {
                        let parsed = parse(unit, &child_file, &text)?;
                        Some(self.add_module(
                            unit,
                            unit_index,
                            &child_path,
                            &child_file,
                            Some(id),
                            parsed.items,
                            &dir,
                        )?)
                    }
                    // Generated or included from elsewhere: no source to read.
                    None => None,
                }
            };
            if let Some(child) = child {
                self.modules[id].children.insert(child_name, child);
            }
        }
        let mut uses = Uses::default();
        for item in &kept {
            uses.visit_item(item);
        }
        let module = &mut self.modules[id];
        for (local, segments) in uses.named {
            module.imports.entry(local).or_insert(segments);
        }
        module.globs = uses.globs;
        module.items = kept;
        Ok(id)
    }

    /// Resolves a path as written in module `ctx`.
    fn resolve(&self, ctx: usize, segments: &[String], seen: &mut Seen) -> Option<Target> {
        let (first, mut rest) = segments.split_first()?;
        let unit = self.modules[ctx].unit;
        let mut current = match first.as_str() {
            "::" => {
                let (name, after) = rest.split_first()?;
                rest = after;
                Target::Module(*self.externs.get(name)?)
            }
            "crate" => Target::Module(*self.roots.get(unit)?),
            "self" => Target::Module(ctx),
            "super" => Target::Module(self.modules[ctx].parent?),
            name => match self.lookup(ctx, name, seen) {
                Some(target) => target,
                None => Target::Module(*self.externs.get(name)?),
            },
        };
        for segment in rest {
            current = match current {
                Target::Module(module) => match segment.as_str() {
                    "self" => current,
                    "super" => Target::Module(self.modules[module].parent?),
                    name => self
                        .lookup(module, name, seen)
                        .unwrap_or(Target::Item(module)),
                },
                // An associated item or a variant: still defined where the type is.
                Target::Item(_) => current,
            };
        }
        Some(current)
    }

    /// Finds what `name` means in `module`: a child module, an import, an item defined there, or a
    /// name a glob import brings in.
    fn lookup(&self, module: usize, name: &str, seen: &mut Seen) -> Option<Target> {
        if !seen.insert((module, name.to_owned())) {
            return None;
        }
        let data = &self.modules[module];
        if let Some(&child) = data.children.get(name) {
            return Some(Target::Module(child));
        }
        if let Some(segments) = data.imports.get(name) {
            return self.resolve(module, segments, seen);
        }
        if data.defined.contains(name) {
            return Some(Target::Item(module));
        }
        data.globs
            .iter()
            .find_map(|glob| match self.resolve(module, glob, seen) {
                Some(Target::Module(source)) if source != module => self.lookup(source, name, seen),
                _ => None,
            })
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

fn parse(unit: &Unit, file: &str, text: &str) -> Result<syn::File, Error> {
    syn::parse_file(text).map_err(|error| {
        let start = error.span().start();
        Error::new(format!(
            "{}/{file}:{}:{}: {error}",
            unit.dir,
            start.line,
            start.column + 1
        ))
    })
}

fn line(span: Span) -> usize {
    span.start().line
}

/// One leaf of a `use` tree.
enum Leaf {
    /// `use a::b::C;` or `use a::b::C as D;`: the full path and the local name, if any (`as _`).
    Named {
        segments: Vec<String>,
        local: Option<String>,
        line: usize,
    },
    /// `use a::b::*;`: the path of the module.
    Glob { segments: Vec<String>, line: usize },
}

fn leaves(tree: &UseTree, prefix: &mut Vec<String>, line: usize, out: &mut Vec<Leaf>) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            leaves(&path.tree, prefix, self::line(path.ident.span()), out);
            prefix.pop();
        }
        UseTree::Name(name) => {
            let ident = name.ident.to_string();
            let line = self::line(name.ident.span());
            if ident == "self" {
                let local = prefix.last().cloned();
                out.push(Leaf::Named {
                    segments: prefix.clone(),
                    local,
                    line,
                });
            } else {
                let mut segments = prefix.clone();
                segments.push(ident.clone());
                out.push(Leaf::Named {
                    segments,
                    local: Some(ident),
                    line,
                });
            }
        }
        UseTree::Rename(rename) => {
            let mut segments = prefix.clone();
            if rename.ident != "self" {
                segments.push(rename.ident.to_string());
            }
            let local = Some(rename.rename.to_string()).filter(|local| local != "_");
            out.push(Leaf::Named {
                segments,
                local,
                line: self::line(rename.ident.span()),
            });
        }
        UseTree::Glob(_) => out.push(Leaf::Glob {
            segments: prefix.clone(),
            line,
        }),
        UseTree::Group(group) => {
            for tree in &group.items {
                leaves(tree, prefix, line, out);
            }
        }
    }
}

fn use_leaves(item: &syn::ItemUse) -> Vec<Leaf> {
    let mut prefix = Vec::new();
    if item.leading_colon.is_some() {
        prefix.push("::".to_owned());
    }
    let mut out = Vec::new();
    leaves(&item.tree, &mut prefix, line(item.use_token.span), &mut out);
    out
}

/// Collects the `use` items of a module, also those inside function bodies.
#[derive(Default)]
struct Uses {
    named: Vec<(String, Vec<String>)>,
    globs: Vec<Vec<String>>,
}

impl<'ast> Visit<'ast> for Uses {
    fn visit_item(&mut self, item: &'ast Item) {
        if !test_only(item_attrs(item)) && !matches!(item, Item::Mod(_)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if !test_only(impl_item_attrs(item)) {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        if !test_only(trait_item_attrs(item)) {
            visit::visit_trait_item(self, item);
        }
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        for leaf in use_leaves(item) {
            match leaf {
                Leaf::Named {
                    segments,
                    local: Some(local),
                    ..
                } => self.named.push((local, segments)),
                Leaf::Named { local: None, .. } => {}
                Leaf::Glob { segments, .. } => self.globs.push(segments),
            }
        }
    }
}

/// Collects the edges of one module.
struct Edges<'g> {
    graph: &'g Graph,
    from: usize,
    found: Vec<(usize, usize)>,
}

impl Edges<'_> {
    fn path(&mut self, segments: &[String], line: usize) {
        if let Some(target) = self.graph.resolve(self.from, segments, &mut Seen::new()) {
            self.found.push((target.module(), line));
        }
    }

    /// Scans a token stream for `a::b…` sequences (macro bodies, attribute arguments).
    fn tokens(&mut self, stream: TokenStream) {
        let tokens: Vec<TokenTree> = stream.into_iter().collect();
        let mut index = 0;
        while let Some(token) = tokens.get(index) {
            match token {
                TokenTree::Group(group) => {
                    self.tokens(group.stream());
                    index += 1;
                }
                TokenTree::Ident(ident) => {
                    let mut segments = vec![ident.to_string()];
                    let mut next = index + 1;
                    while let (
                        Some(TokenTree::Punct(a)),
                        Some(TokenTree::Punct(b)),
                        Some(TokenTree::Ident(part)),
                    ) = (tokens.get(next), tokens.get(next + 1), tokens.get(next + 2))
                    {
                        if a.as_char() != ':' || b.as_char() != ':' {
                            break;
                        }
                        segments.push(part.to_string());
                        next += 3;
                    }
                    // A `$name::…` of a macro_rules body is a metavariable, not a path; `$crate` is.
                    let metavariable = index > 0
                        && matches!(tokens.get(index - 1), Some(TokenTree::Punct(p)) if p.as_char() == '$')
                        && segments[0] != "crate";
                    if segments.len() >= 2 && !metavariable {
                        self.path(&segments, line(ident.span()));
                    }
                    index = next;
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => index += 1,
            }
        }
    }
}

impl<'ast> Visit<'ast> for Edges<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if !test_only(item_attrs(item)) && !matches!(item, Item::Mod(_)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if !test_only(impl_item_attrs(item)) {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        if !test_only(trait_item_attrs(item)) {
            visit::visit_trait_item(self, item);
        }
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let public = !matches!(item.vis, syn::Visibility::Inherited);
        for leaf in use_leaves(item) {
            match leaf {
                Leaf::Named { segments, line, .. } => {
                    match self.graph.resolve(self.from, &segments, &mut Seen::new()) {
                        // Importing a module is no edge by itself; the paths through it are. A
                        // public re-export of a module is part of this module's API, so it counts.
                        Some(Target::Module(module)) if public => self.found.push((module, line)),
                        Some(Target::Module(_)) | None => {}
                        Some(Target::Item(module)) => self.found.push((module, line)),
                    }
                }
                Leaf::Glob { segments, line } => self.path(&segments, line),
            }
        }
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path.segments.len() >= 2 || path.leading_colon.is_some() {
            let mut segments = Vec::new();
            if path.leading_colon.is_some() {
                segments.push("::".to_owned());
            }
            segments.extend(
                path.segments
                    .iter()
                    .map(|segment| segment.ident.to_string()),
            );
            let line = path
                .segments
                .first()
                .map_or(0, |segment| line(segment.ident.span()));
            self.path(&segments, line);
        }
        visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.tokens(mac.tokens.clone());
        visit::visit_macro(self, mac);
    }

    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        if attr.path().is_ident("doc") {
            return;
        }
        if let Meta::List(list) = &attr.meta {
            self.tokens(list.tokens.clone());
        }
        visit::visit_attribute(self, attr);
    }

    // `pub(in crate::widgets)` limits who may use an item; it uses nothing.
    fn visit_visibility(&mut self, _: &'ast syn::Visibility) {}
}

/// Whether the attributes keep an item out of the normal build: `#[cfg(test)]`, `#[cfg(doc)]`,
/// `#[cfg(feature = "test-support")]` and `any`/`all` combinations of them.
fn test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<Meta>()
                .is_ok_and(|meta| test_only_predicate(&meta))
    })
}

fn test_only_predicate(meta: &Meta) -> bool {
    match meta {
        Meta::Path(path) => path.is_ident("test") || path.is_ident("doc"),
        Meta::NameValue(pair) => {
            pair.path.is_ident("feature")
                && matches!(&pair.value, syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(value), .. }) if value.value() == "test-support")
        }
        Meta::List(list) => {
            let Ok(args) =
                list.parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
            else {
                return false;
            };
            if list.path.is_ident("any") {
                !args.is_empty() && args.iter().all(test_only_predicate)
            } else if list.path.is_ident("all") {
                args.iter().any(test_only_predicate)
            } else {
                false
            }
        }
    }
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn impl_item_attrs(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Const(item) => &item.attrs,
        ImplItem::Fn(item) => &item.attrs,
        ImplItem::Type(item) => &item.attrs,
        ImplItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn trait_item_attrs(item: &TraitItem) -> &[Attribute] {
    match item {
        TraitItem::Const(item) => &item.attrs,
        TraitItem::Fn(item) => &item.attrs,
        TraitItem::Type(item) => &item.attrs,
        TraitItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn item_name(item: &Item) -> Option<String> {
    let ident = match item {
        Item::Const(item) => &item.ident,
        Item::Enum(item) => &item.ident,
        Item::Fn(item) => &item.sig.ident,
        Item::Macro(item) => item.ident.as_ref()?,
        Item::Static(item) => &item.ident,
        Item::Struct(item) => &item.ident,
        Item::Trait(item) => &item.ident,
        Item::TraitAlias(item) => &item.ident,
        Item::Type(item) => &item.ident,
        Item::Union(item) => &item.ident,
        _ => return None,
    };
    Some(ident.to_string())
}
