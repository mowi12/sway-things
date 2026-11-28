// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Who wrote a binding line.

/// Who wrote a binding line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// The user, in their config or a file it includes.
    User,
    /// Sway Things, in its generated file: the item of `shortcuts.bindings` at this index.
    Item(usize),
    /// Sway Things: the binding that makes the unbind of the item of `shortcuts.bindings` at
    /// this index always succeed, which the unbind removes right after.
    Placeholder(usize),
    /// Sway Things: a binding of a group of media keys, by the key of its switch.
    Group(&'static str),
    /// Sway Things: a binding of a key of another page, by the key of its setting (the keys of
    /// the Screenshots & Clipboard page, [`crate::capture::shortcuts`]).
    Feature(&'static str),
    /// Sway Things: the binding of the terminal or the launcher of the Apps & Autostart page, by
    /// the key of its command's setting.
    App(&'static str),
}

impl Origin {
    /// Whether Sway Things wrote it.
    pub fn is_ours(self) -> bool {
        self != Self::User
    }
}
