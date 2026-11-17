// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Translation of the strings the app shows: [`tr`] gives the text of a [`Msg`] in the user's
//! language, once `main` has called [`sway_things_core::i18n::init`]; [`trf`] and [`ntrf`] fill
//! in named values, the latter in the plural form of a count. Strings are marked with
//! [`msg!`](sway_things_core::msg) and [`msg_plural!`](sway_things_core::msg_plural).
//!
//! [`Msg`]: sway_things_core::i18n::Msg

pub use sway_things_core::i18n::{Msg, MsgPlural, Text, ntr, ntrf, tr, trf};
pub use sway_things_core::{msg, msg_plural};
