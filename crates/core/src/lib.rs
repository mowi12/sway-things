// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core of Sway Things: settings registry, state, generators, sway config parser, IPC and backends.
//!
//! This crate has no GTK dependency. Both the `sway-things` CLI and the `sway-things-gui` app build on it.

#![warn(missing_docs)]
#![deny(clippy::print_stdout)]

pub mod command;
pub mod registry;
pub mod state;
pub mod version;
pub mod watch;
