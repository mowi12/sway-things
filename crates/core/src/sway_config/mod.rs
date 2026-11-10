// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the user's sway config. Sway Things only ever reads it.
//!
//! [`parse`] reads the main config and every file it includes, the way sway does: comments,
//! continued lines, blocks, `set` variables and `include` paths. It never runs anything; an
//! include that would need a shell command is a [`Diagnostic`]. The result is the list of
//! [`Directive`]s in the order sway runs them, each with its file, line, block and resolved
//! arguments. The parser doesn't know which commands exist, so it is best-effort by design.
//!
//! ```
//! use std::path::Path;
//! use sway_things_core::sway_config::{MemoryFiles, parse};
//!
//! let files = MemoryFiles::new()
//!     .var("HOME", "/home/me")
//!     .file(
//!         "/home/me/.config/sway/config",
//!         "set $gap 8\ninclude ~/.config/sway/sway-things/*.conf\ngaps inner $gap\n",
//!     )
//!     .file(
//!         "/home/me/.config/sway/sway-things/10-input.conf",
//!         "input type:touchpad tap enabled\n",
//!     );
//! let config = parse(Path::new("/home/me/.config/sway/config"), &files);
//!
//! let tap = &config.directives[1];
//! assert!(tap.is(&["input", "type:touchpad"], "tap"));
//! assert_eq!(tap.args, ["enabled"]);
//! assert_eq!((tap.file.file_name().unwrap().to_str(), tap.line), (Some("10-input.conf"), 1));
//!
//! let gaps = &config.directives[2];
//! assert_eq!((gaps.line, gaps.args.as_slice()), (3, &["inner".to_owned(), "8".to_owned()][..]));
//! assert!(config.diagnostics.is_empty());
//! ```

mod expand;
mod files;
mod line;
mod parse;
#[cfg(test)]
mod tests;

pub use expand::{ExpandError, expand};
pub use files::{Disk, Files, MemoryFiles};
pub use parse::{Diagnostic, DiagnosticKind, Directive, Include, SwayConfig, default_path, parse};
