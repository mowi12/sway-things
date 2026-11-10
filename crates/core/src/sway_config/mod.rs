// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the user's sway config. Sway Things only ever reads it.

mod expand;
mod files;

pub use expand::{ExpandError, expand};
pub use files::{Disk, Files, MemoryFiles};
