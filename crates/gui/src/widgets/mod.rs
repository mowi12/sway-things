// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The building blocks of the app.

mod bluetooth;
mod clamp;
mod cursor;
mod device_selector;
mod displays;
mod generated;
mod group_row;
mod keyboard;
mod layout_list;
mod list;
mod live;
mod look;
mod network;
mod notice;
mod page_header;
mod page_view;
mod panes;
mod placeholder;
mod row_layout;
mod scroll_line;
mod search_button;
mod setting_row;
mod shortcuts;
mod sidebar;
mod sound;
mod test_area;
mod wallpaper;
mod welcome;
mod workspaces;
mod xkb_options;

pub use bluetooth::{BluetoothPage, Bus as BluetoothBus};
pub use clamp::Clamp;
pub use device_selector::DeviceSelector;
pub use displays::{Arrangement, Canvas, OutputPicker};
pub use generated::GeneratedWindow;
pub use group_row::GroupRow;
pub use layout_list::LayoutList;
pub use network::{Bus as NetworkBus, NetworkPage};
pub use page_header::PageHeader;
pub use page_view::{MAXIMUM_WIDTH, PageView};
pub use panes::Panes;
pub use placeholder::Placeholder;
pub use row_layout::RowLayout;
pub use scroll_line::{FULL_OFFSET, ScrollLine, scroller_in};
pub use setting_row::SettingRow;
pub use shortcuts::{Captured, Editor as ShortcutEditor, GdkKeymap, ShortcutList, capture};
pub use sidebar::Sidebar;
pub use sound::{Server as SoundServer, SoundPage};
pub use test_area::TestArea;
pub use wallpaper::page as wallpaper_page;
pub use welcome::Welcome;
pub use workspaces::WorkspaceMap;
pub use xkb_options::OptionBrowser;
