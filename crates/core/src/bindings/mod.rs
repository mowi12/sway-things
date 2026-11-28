// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Key bindings: every `bindsym` and `bindcode` of the user's config and of Sway Things, read the
//! way sway reads them, and the bindings Sway Things writes. A single binding line, its keys and
//! its command are read in [`crate::sway_config::bindings`].
//!
//! - [`Table`]: the bindings of a config in the order sway reads them, and what became of each.
//! - [`Listing`]: every binding for the Keyboard Shortcuts page, with the clashes between them;
//!   also what the Workspaces page needs to tell whether its keys are taken.
//! - [`ours`]: the bindings Sway Things writes to `50-bindings.conf` from `shortcuts.*`.
//! - [`live`]: the IPC commands that apply a change without a reload.
//! - [`edit`]: the changes the page makes to the items of Sway Things.

pub mod edit;
mod listing;
pub mod live;
pub mod ours;
mod table;
#[cfg(test)]
mod tests;

pub use listing::{Clash, ClashKind, Listing, Row, RowKind, sequence, sequence_of};
pub use table::{Bound, DEFAULT_MODE, Entry, Fate, Source, Table, binding_parts, user_lines};
