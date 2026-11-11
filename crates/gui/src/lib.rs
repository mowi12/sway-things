// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `sway-things-gui`: the GTK4 settings app of Sway Things.
//!
//! The app is plain gtk4-rs. Every building block is a widget in [`widgets`]; the [`window`] puts
//! them together.

#![warn(missing_docs)]

pub mod app;
pub mod i18n;
pub mod style;
pub mod widgets;
pub mod window;
