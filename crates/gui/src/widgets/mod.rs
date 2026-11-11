// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The building blocks of the app.

mod clamp;
mod page_header;
mod page_view;
mod placeholder;
mod sidebar;

pub use clamp::Clamp;
pub use page_header::PageHeader;
pub use page_view::{MAXIMUM_WIDTH, PageView};
pub use placeholder::Placeholder;
pub use sidebar::Sidebar;
