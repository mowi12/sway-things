// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What Sway Things keeps per sway session, shared by its pages, hooks and commands:
//!
//! - [`process`]: the processes it started (the clipboard history's watcher, a recorder, the
//!   night light), found in `/proc` again.
//! - [`lock`]: the locks that keep two of its commands, hooks or the app from doing one thing at
//!   once.

pub mod lock;
pub mod process;

pub use lock::{Lock, LockError};
