// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pages of the app and the sidebar groups they belong to.

use super::setting::Msg;
use crate::msg;

/// A group of pages in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    /// Input devices, displays, sound and connections
    Devices,
    /// How windows, the desktop and apps look
    Appearance,
    /// How the desktop behaves
    Desktop,
    /// Power, time, language and maintenance
    System,
}

impl Group {
    /// Every group, in sidebar order.
    pub const ALL: [Self; 4] = [Self::Devices, Self::Appearance, Self::Desktop, Self::System];

    /// The heading of the group in the sidebar.
    pub const fn title(self) -> Msg {
        match self {
            Self::Devices => msg!("Devices"),
            Self::Appearance => msg!("Appearance"),
            Self::Desktop => msg!("Desktop"),
            Self::System => msg!("System"),
        }
    }

    /// The pages of the group, in sidebar order.
    pub fn pages(self) -> impl Iterator<Item = Page> {
        Page::ALL
            .into_iter()
            .filter(move |page| page.group() == self)
    }
}

/// A page of the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    /// Mouse & Touchpad
    MouseTouchpad,
    /// Keyboard
    Keyboard,
    /// Cursor & Tablet
    CursorTablet,
    /// Displays
    Displays,
    /// Sound
    Sound,
    /// Bluetooth & Network
    BluetoothNetwork,
    /// Windows, including the SwayFX effects
    Windows,
    /// Wallpaper
    Wallpaper,
    /// Apps Look & Feel
    AppsLookFeel,
    /// Bar
    Bar,
    /// Window Behavior
    WindowBehavior,
    /// Workspaces
    Workspaces,
    /// Keyboard Shortcuts
    KeyboardShortcuts,
    /// Notifications
    Notifications,
    /// Apps & Autostart
    AppsAutostart,
    /// Screenshots & Clipboard
    ScreenshotsClipboard,
    /// Power & Lock
    PowerLock,
    /// Brightness & Night Light
    BrightnessNightLight,
    /// Region & Time
    RegionTime,
    /// Accessibility
    Accessibility,
    /// Environment
    Environment,
    /// About & Maintenance, which holds the preferences of Sway Things itself
    About,
}

impl Page {
    /// Every page, in sidebar order: grouped as in [`Group::ALL`].
    pub const ALL: [Self; 22] = [
        Self::MouseTouchpad,
        Self::Keyboard,
        Self::CursorTablet,
        Self::Displays,
        Self::Sound,
        Self::BluetoothNetwork,
        Self::Windows,
        Self::Wallpaper,
        Self::AppsLookFeel,
        Self::Bar,
        Self::WindowBehavior,
        Self::Workspaces,
        Self::KeyboardShortcuts,
        Self::Notifications,
        Self::AppsAutostart,
        Self::ScreenshotsClipboard,
        Self::PowerLock,
        Self::BrightnessNightLight,
        Self::RegionTime,
        Self::Accessibility,
        Self::Environment,
        Self::About,
    ];

    /// The page with this [id](Page::id).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.id() == id)
    }

    /// The id of the page, as used in deep links (`sway-things open <id>`).
    pub const fn id(self) -> &'static str {
        match self {
            Self::MouseTouchpad => "mouse-touchpad",
            Self::Keyboard => "keyboard",
            Self::CursorTablet => "cursor-tablet",
            Self::Displays => "displays",
            Self::Sound => "sound",
            Self::BluetoothNetwork => "bluetooth-network",
            Self::Windows => "windows",
            Self::Wallpaper => "wallpaper",
            Self::AppsLookFeel => "apps-look-feel",
            Self::Bar => "bar",
            Self::WindowBehavior => "window-behavior",
            Self::Workspaces => "workspaces",
            Self::KeyboardShortcuts => "keyboard-shortcuts",
            Self::Notifications => "notifications",
            Self::AppsAutostart => "apps-autostart",
            Self::ScreenshotsClipboard => "screenshots-clipboard",
            Self::PowerLock => "power-lock",
            Self::BrightnessNightLight => "brightness-night-light",
            Self::RegionTime => "region-time",
            Self::Accessibility => "accessibility",
            Self::Environment => "environment",
            Self::About => "about",
        }
    }

    /// The sidebar group of the page.
    pub const fn group(self) -> Group {
        match self {
            Self::MouseTouchpad
            | Self::Keyboard
            | Self::CursorTablet
            | Self::Displays
            | Self::Sound
            | Self::BluetoothNetwork => Group::Devices,
            Self::Windows | Self::Wallpaper | Self::AppsLookFeel | Self::Bar => Group::Appearance,
            Self::WindowBehavior
            | Self::Workspaces
            | Self::KeyboardShortcuts
            | Self::Notifications
            | Self::AppsAutostart
            | Self::ScreenshotsClipboard => Group::Desktop,
            Self::PowerLock
            | Self::BrightnessNightLight
            | Self::RegionTime
            | Self::Accessibility
            | Self::Environment
            | Self::About => Group::System,
        }
    }

    /// The title of the page, in the sidebar and the page header.
    pub const fn title(self) -> Msg {
        match self {
            Self::MouseTouchpad => msg!("Mouse & Touchpad"),
            Self::Keyboard => msg!("Keyboard"),
            Self::CursorTablet => msg!("Cursor & Tablet"),
            Self::Displays => msg!("Displays"),
            Self::Sound => msg!("Sound"),
            Self::BluetoothNetwork => msg!("Bluetooth & Network"),
            Self::Windows => msg!("Windows"),
            Self::Wallpaper => msg!("Wallpaper"),
            Self::AppsLookFeel => msg!("Apps Look & Feel"),
            Self::Bar => msg!("Bar"),
            Self::WindowBehavior => msg!("Window Behavior"),
            Self::Workspaces => msg!("Workspaces"),
            Self::KeyboardShortcuts => msg!("Keyboard Shortcuts"),
            Self::Notifications => msg!("Notifications"),
            Self::AppsAutostart => msg!("Apps & Autostart"),
            Self::ScreenshotsClipboard => msg!("Screenshots & Clipboard"),
            Self::PowerLock => msg!("Power & Lock"),
            Self::BrightnessNightLight => msg!("Brightness & Night Light"),
            Self::RegionTime => msg!("Region & Time"),
            Self::Accessibility => msg!("Accessibility"),
            Self::Environment => msg!("Environment"),
            Self::About => msg!("About & Maintenance"),
        }
    }

    /// The one-line description under the title in the page header.
    pub const fn description(self) -> Msg {
        match self {
            Self::MouseTouchpad => msg!("Pointer speed, scrolling and touchpad"),
            Self::Keyboard => msg!("Layouts, switching shortcut, key repeat and XKB options"),
            Self::CursorTablet => msg!("Cursor theme, size and visibility"),
            Self::Displays => msg!("Arrange outputs, resolution, scale and profiles"),
            Self::Sound => msg!("Devices, volume and per-application levels"),
            Self::BluetoothNetwork => msg!("Connect devices and Wi-Fi networks"),
            Self::Windows => msg!("Borders, colors, gaps and title bars of tiled windows"),
            Self::Wallpaper => msg!("Background image or color per output"),
            Self::AppsLookFeel => msg!("Theme, icons, fonts and dark mode for applications"),
            Self::Bar => msg!("Position, modules and style of the bar"),
            Self::WindowBehavior => msg!("How focus, floating and tiling behave"),
            Self::Workspaces => msg!("Names, icons and the number of workspaces"),
            Self::KeyboardShortcuts => msg!("All bindings, gestures and media keys"),
            Self::Notifications => msg!("Notification daemon settings"),
            Self::AppsAutostart => {
                msg!("Default applications and everything that starts with sway")
            }
            Self::ScreenshotsClipboard => msg!("Capturing the screen and clipboard history"),
            Self::PowerLock => msg!("Idle timeouts, lock screen and power profile"),
            Self::BrightnessNightLight => msg!("Screen brightness and color temperature"),
            Self::RegionTime => msg!("Time zone, language and formats"),
            Self::Accessibility => msg!("Larger cursor, larger text and key repeat"),
            Self::Environment => msg!("Environment variables and Wayland compatibility"),
            Self::About => msg!("Versions, config check, history and Sway Things preferences"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Group, Page};
    use crate::registry::Registry;

    #[test]
    fn ids_are_unique_kebab_case_and_round_trip() {
        let mut seen = HashSet::new();
        for page in Page::ALL {
            let id = page.id();
            assert!(seen.insert(id), "duplicate id {id}");
            assert!(
                !id.is_empty()
                    && !id.starts_with('-')
                    && !id.ends_with('-')
                    && !id.contains("--")
                    && id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'),
                "{id} isn't kebab-case"
            );
            assert_eq!(Page::from_id(id), Some(page));
        }
        assert_eq!(Page::from_id("no-such-page"), None);
    }

    #[test]
    fn pages_are_listed_group_by_group() {
        let listed: Vec<Page> = Group::ALL.into_iter().flat_map(Group::pages).collect();
        assert_eq!(listed, Page::ALL);
        for group in Group::ALL {
            assert!(group.pages().next().is_some(), "{group:?} is empty");
        }
    }

    #[test]
    fn texts_are_set() {
        for page in Page::ALL {
            assert!(!page.title().msgid().is_empty(), "{page:?}");
            assert!(!page.description().msgid().is_empty(), "{page:?}");
            assert!(!page.description().msgid().ends_with('.'), "{page:?}");
        }
    }

    #[test]
    fn settings_follow_the_page_order() {
        let position = |page| Page::ALL.iter().position(|p| *p == page);
        let positions: Vec<_> = Registry::builtin()
            .iter()
            .map(|setting| position(setting.page()))
            .collect();
        assert!(positions.is_sorted(), "settings aren't in page order");
    }
}
