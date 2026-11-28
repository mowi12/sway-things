// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The binding lines as sway reads them, below the features that list and write them
//! ([`crate::bindings`]):
//!
//! - [`keys`]: key combinations as sway compares them (`Mod4` and `Super` are one modifier, the
//!   order of modifiers and keys doesn't matter, keysyms ignore case).
//! - [`Line`]: one binding line with its options, keys and command, and its [`Identity`]: what
//!   makes a later line replace it or an unbind remove it.
//! - [`BindCommand`]: the command of a binding, read into the actions the pages know.
//! - [`item`]: the items of `shortcuts.bindings` as text.
//! - [`Origin`] and [`FileLine`]: who wrote a binding line, and a line of Sway Things' own file.
//!
//! They use only the sway layer and `i18n`, so the command builders and the registry's checks
//! read them without depending on the features.
//!
//! ```
//! use sway_things_core::sway_config::bindings::{Line, Modifiers};
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
//! # Ok::<(), sway_things_core::sway_config::bindings::LineError>(())
//! ```

mod command;
mod file_line;
pub mod item;
pub mod keys;
mod line;
mod origin;

pub use command::{
    Action, BindCommand, Category, CommandError, Direction, MAX_WORKSPACE, describe,
};
pub use file_line::FileLine;
pub use keys::{Combo, Device, Key, KeyError, Modifiers};
pub use line::{Flags, Identity, Keymap, Kind, Line, LineError, LineErrorKind};
pub use origin::Origin;

#[cfg(test)]
pub(crate) use line::keys_index;
