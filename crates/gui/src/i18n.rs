// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Translation of the strings the app shows.

use sway_things_core::registry::Msg;

/// The text of `msg` in the user's language. Until gettext is wired up, that is the English source
/// string.
pub fn tr(msg: Msg) -> &'static str {
    msg.msgid()
}
