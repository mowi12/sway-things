// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The string literals of the modules: in code, in attributes other than doc comments, and in
//! macro bodies (scanned token by token). The items the module graph leaves out (`#[cfg(test)]`,
//! `test-support`, `tests.rs`) are left out here too.

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::{self, Visit};
use syn::{Attribute, ImplItem, Item, Meta, TraitItem};

use crate::graph::{self, Graph};

/// One string literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Literal {
    /// The module it is written in, an index into [`Graph::modules`].
    pub module: usize,
    /// Its line.
    pub line: usize,
    /// Its value, escapes resolved.
    pub value: String,
}

/// The string literals of every module of the graph, in the order of the modules.
pub fn literals(graph: &Graph) -> Vec<Literal> {
    let mut collector = Collector {
        module: 0,
        found: Vec::new(),
    };
    for (index, module) in graph.modules().iter().enumerate() {
        collector.module = index;
        for item in module.items() {
            collector.visit_item(item);
        }
    }
    collector.found
}

struct Collector {
    module: usize,
    found: Vec<Literal>,
}

impl Collector {
    fn tokens(&mut self, stream: TokenStream) {
        for token in stream {
            match token {
                TokenTree::Group(group) => self.tokens(group.stream()),
                TokenTree::Literal(literal) => {
                    if let syn::Lit::Str(text) = syn::Lit::new(literal.clone()) {
                        self.found.push(Literal {
                            module: self.module,
                            line: graph::line(literal.span()),
                            value: text.value(),
                        });
                    }
                }
                TokenTree::Ident(_) | TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Collector {
    fn visit_item(&mut self, item: &'ast Item) {
        if !graph::test_only(graph::item_attrs(item)) && !matches!(item, Item::Mod(_)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if !graph::test_only(graph::impl_item_attrs(item)) {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        if !graph::test_only(graph::trait_item_attrs(item)) {
            visit::visit_trait_item(self, item);
        }
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.found.push(Literal {
            module: self.module,
            line: graph::line(literal.span()),
            value: literal.value(),
        });
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.tokens(mac.tokens.clone());
    }

    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        if attr.path().is_ident("doc") {
            return;
        }
        match &attr.meta {
            Meta::List(list) => self.tokens(list.tokens.clone()),
            Meta::NameValue(pair) => self.visit_expr(&pair.value),
            Meta::Path(_) => {}
        }
    }
}
