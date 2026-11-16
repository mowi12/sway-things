// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The building blocks of the app.

mod bluetooth;
mod clamp;
mod device_selector;
mod generated;
mod group_row;
mod keyboard;
mod layout_list;
mod network;
mod page_header;
mod page_view;
mod panes;
mod placeholder;
mod scroll_line;
mod search_button;
mod setting_row;
mod sidebar;
mod test_area;
mod welcome;
mod xkb_options;

pub use bluetooth::{BluetoothPage, Bus as BluetoothBus};
pub use clamp::Clamp;
pub use device_selector::DeviceSelector;
pub use generated::GeneratedWindow;
pub use group_row::GroupRow;
pub use layout_list::LayoutList;
pub use network::{Bus as NetworkBus, NetworkPage};
pub use page_header::PageHeader;
pub use page_view::{MAXIMUM_WIDTH, PageView};
pub use panes::Panes;
pub use placeholder::Placeholder;
pub use scroll_line::{FULL_OFFSET, ScrollLine, scroller_in};
pub use setting_row::SettingRow;
pub use sidebar::Sidebar;
pub use test_area::TestArea;
pub use welcome::Welcome;
pub use xkb_options::OptionBrowser;
