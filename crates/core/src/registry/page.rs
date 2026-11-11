// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pages of the app and the sidebar groups they belong to.

use super::setting::Msg;

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
            Self::Devices => Msg::new("Devices"),
            Self::Appearance => Msg::new("Appearance"),
            Self::Desktop => Msg::new("Desktop"),
            Self::System => Msg::new("System"),
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
        Msg::new(match self {
            Self::MouseTouchpad => "Mouse & Touchpad",
            Self::Keyboard => "Keyboard",
            Self::CursorTablet => "Cursor & Tablet",
            Self::Displays => "Displays",
            Self::Sound => "Sound",
            Self::BluetoothNetwork => "Bluetooth & Network",
            Self::Windows => "Windows",
            Self::Wallpaper => "Wallpaper",
            Self::AppsLookFeel => "Apps Look & Feel",
            Self::Bar => "Bar",
            Self::WindowBehavior => "Window Behavior",
            Self::Workspaces => "Workspaces",
            Self::KeyboardShortcuts => "Keyboard Shortcuts",
            Self::Notifications => "Notifications",
            Self::AppsAutostart => "Apps & Autostart",
            Self::ScreenshotsClipboard => "Screenshots & Clipboard",
            Self::PowerLock => "Power & Lock",
            Self::BrightnessNightLight => "Brightness & Night Light",
            Self::RegionTime => "Region & Time",
            Self::Accessibility => "Accessibility",
            Self::Environment => "Environment",
            Self::About => "About & Maintenance",
        })
    }

    /// The one-line description under the title in the page header.
    pub const fn description(self) -> Msg {
        Msg::new(match self {
            Self::MouseTouchpad => "Pointer speed, scrolling and touchpad",
            Self::Keyboard => "Layouts, switching shortcut, key repeat and XKB options",
            Self::CursorTablet => "Cursor theme, size and visibility",
            Self::Displays => "Arrange outputs, resolution, scale and profiles",
            Self::Sound => "Devices, volume and per-application levels",
            Self::BluetoothNetwork => "Connect devices and Wi-Fi networks",
            Self::Windows => "Borders, colors, gaps and title bars of tiled windows",
            Self::Wallpaper => "Background image or color per output",
            Self::AppsLookFeel => "Theme, icons, fonts and dark mode for applications",
            Self::Bar => "Position, modules and style of the bar",
            Self::WindowBehavior => "How focus, floating and tiling behave",
            Self::Workspaces => "Names, icons and the number of workspaces",
            Self::KeyboardShortcuts => "All bindings, gestures and media keys",
            Self::Notifications => "Notification daemon settings",
            Self::AppsAutostart => "Default applications and everything that starts with sway",
            Self::ScreenshotsClipboard => "Capturing the screen and clipboard history",
            Self::PowerLock => "Idle timeouts, lock screen and power profile",
            Self::BrightnessNightLight => "Screen brightness and color temperature",
            Self::RegionTime => "Time zone, language and formats",
            Self::Accessibility => "Larger cursor, larger text and key repeat",
            Self::Environment => "Environment variables and Wayland compatibility",
            Self::About => "Versions, config check, history and Sway Things preferences",
        })
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
