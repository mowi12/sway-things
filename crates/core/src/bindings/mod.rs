// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Key bindings: every `bindsym` and `bindcode` of the user's config and of Sway Things, read the
//! way sway reads them, and the bindings Sway Things writes.
//!
//! - [`keys`]: key combinations as sway compares them (`Mod4` and `Super` are one modifier, the
//!   order of modifiers and keys doesn't matter, keysyms ignore case).
//! - [`Line`]: one binding line with its options, keys and command, and its [`Identity`]: what
//!   makes a later line replace it or an unbind remove it.
//! - [`Table`]: the bindings of a config in the order sway reads them, and what became of each.
//! - [`Listing`]: every binding for the Keyboard Shortcuts page, with the clashes between them;
//!   also what the Workspaces page needs to tell whether its keys are taken.
//! - [`ours`]: the bindings Sway Things writes to `50-bindings.conf` from `shortcuts.*`.
//! - [`live`]: the IPC commands that apply a change without a reload.
//! - [`edit`]: the changes the page makes to the items of Sway Things.
//!
//! ```
//! use sway_things_core::bindings::{Line, keys::Modifiers};
//!
//! // The arguments as sway's handler gets them, `$mod` already replaced.
//! let args = ["--to-code", "Mod4+Shift+e", "exec", "swaynag"].map(String::from);
//! let line = Line::read("bindsym", &args)?;
//! assert!(line.flags.to_code);
//! assert_eq!(line.command.as_deref(), Some("exec swaynag"));
//!
//! // `Super` and `Mod4`, the order and the case of the keys don't matter to sway.
//! let a = Line::read("bindsym", &["Super+Shift+E", "kill"].map(String::from))?;
//! let b = Line::read("bindsym", &["shift+mod4+e", "exit"].map(String::from))?;
//! assert_eq!(a.identity("default", None), b.identity("default", None));
//! assert_eq!(a.combo.modifiers, Modifiers::SUPER.union(Modifiers::SHIFT));
//! # Ok::<(), sway_things_core::bindings::LineError>(())
//! ```

mod command;
pub mod edit;
pub(crate) mod item;
pub mod keys;
mod line;
mod listing;
pub mod live;
pub mod ours;
mod table;
#[cfg(test)]
mod tests;

pub use command::{
    Action, BindCommand, Category, CommandError, Direction, MAX_WORKSPACE, describe,
};
pub use keys::{Combo, Device, Key, KeyError, Modifiers};
pub use line::{Flags, Identity, Keymap, Kind, Line, LineError, LineErrorKind};
pub use listing::{Clash, ClashKind, Listing, Row, RowKind, sequence, sequence_of};
pub use table::{
    Bound, DEFAULT_MODE, Entry, Fate, Origin, Source, Table, binding_parts, user_lines,
};
