// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A line of Sway Things' `50-bindings.conf`.

use super::{Line, LineError, Modifiers, Origin};
use crate::command::SwayCmd;

/// A line of `50-bindings.conf`: a binding of Sway Things, with its keys for the main modifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLine {
    /// Where it comes from.
    pub origin: Origin,
    /// The line; `Err` for an item that can't be written, which becomes a comment.
    pub line: Result<Line, LineError>,
    /// Whether it is written with `--no-warn`: it may replace a binding of the user's, which
    /// the page says.
    pub no_warn: bool,
}

impl FileLine {
    /// The command for the generated file.
    ///
    /// # Errors
    ///
    /// The [`LineError`] of a line that can't be written.
    pub fn command(&self) -> Result<SwayCmd, LineError> {
        let line = self.line.as_ref().map_err(Clone::clone)?;
        line.sway_cmd(Modifiers::NONE, self.no_warn)
    }

    /// The setting of another page or of a group of media keys the line comes from; `None` for
    /// an item of `shortcuts.bindings`.
    pub fn setting(&self) -> Option<&'static str> {
        match self.origin {
            Origin::Group(key) | Origin::Feature(key) | Origin::App(key) => Some(key),
            Origin::User | Origin::Item(_) | Origin::Placeholder(_) => None,
        }
    }
}
