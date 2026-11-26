// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Architecture tests of Sway Things: the module graph of the crates `core`, `cli` and `gui`, the
//! rules it must follow (layers of the core, no cycles, no spelled-out keys and hooks, stand-ins only
//! for tests) and the baseline of the violations that are accepted until they are repaired.
//!
//! The checks themselves are the integration tests in `tests/`; this library builds the graph,
//! evaluates the rules and reads and writes the baseline. It is a dev-only crate: never built by the
//! distributions, never shipped.

pub mod baseline;
pub mod blocking;
pub mod graph;
pub mod literals;
pub mod rules;
pub mod workspace;

use std::fmt;

/// An error while reading the sources or the baseline.
#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}
