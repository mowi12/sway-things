// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core of Sway Things: settings registry, state, generators, sway config parser, IPC and backends.
//!
//! This crate has no GTK dependency; it uses GIO only to write GSettings. Both the `sway-things` CLI and the `sway-things-gui` app build on it.

#![warn(missing_docs)]
#![deny(clippy::print_stdout)]

pub mod apply;
mod atomic;
pub mod bluetooth;
pub mod command;
pub mod conflict;
pub mod cursor;
pub mod delegate;
pub mod devices;
pub mod doctor;
pub mod editor;
pub mod generate;
pub mod gsettings;
pub mod i18n;
pub mod include;
pub mod ipc;
pub mod network;
pub mod outputs;
pub mod registry;
pub mod state;
pub mod sway_config;
pub mod system;
pub mod theme;
pub mod ui_state;
pub mod version;
pub mod view;
pub mod watch;
pub mod xkb;
