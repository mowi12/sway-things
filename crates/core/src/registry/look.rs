// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the Apps Look & Feel page: what GTK apps, libadwaita apps and the browsers read
//! from GSettings (`org.gnome.desktop.interface`) instead of asking sway. None of them has a sway
//! directive: each is copied to its GSettings key ([`crate::gsettings`]), and a row without a
//! stored value shows what GSettings has now.
//!
//! The defaults are those of the schema in `gsettings-desktop-schemas` 48 (Debian 13), shown
//! only where GSettings can't be read.

use super::{Choice, Font, Page, Setting, ValueError, When};
use crate::msg;

/// The schema of the keys GTK apps read for their look: cursor, theme, icons, fonts, style.
pub const INTERFACE: &str = "org.gnome.desktop.interface";

/// The longest theme name or font description taken; GSettings itself has no limit.
const MAX_LENGTH: usize = 255;

const STYLE: &str = "look.style";
const ANTIALIASING: &str = "look.antialiasing";

const STYLES: &[Choice] = &[
    // TRANSLATORS: No light or dark style asked for: apps choose, most of them light.
    Choice::new("default", msg!("No preference")),
    Choice::new("prefer-light", msg!("Light")),
    Choice::new("prefer-dark", msg!("Dark")),
];

const ANTIALIASING_MODES: &[Choice] = &[
    Choice::new("grayscale", msg!("Standard (grayscale)")),
    Choice::new("rgba", msg!("Subpixel (for LCD screens)")),
    Choice::new("none", msg!("None")),
];

const HINTING_MODES: &[Choice] = &[
    Choice::new("slight", msg!("Slight")),
    Choice::new("medium", msg!("Medium")),
    Choice::new("full", msg!("Full")),
    Choice::new("none", msg!("None")),
];

const SUBPIXEL_ORDERS: &[Choice] = &[
    Choice::new("rgb", msg!("RGB")),
    Choice::new("bgr", msg!("BGR")),
    Choice::new("vrgb", msg!("Vertical RGB")),
    Choice::new("vbgr", msg!("Vertical BGR")),
];

/// A theme name GSettings can take and GTK looks up as a directory name.
fn check_theme(name: &str) -> Result<(), ValueError> {
    if name.is_empty()
        || name.len() > MAX_LENGTH
        || name.contains('/')
        || name.chars().any(char::is_control)
    {
        return Err(ValueError::NotAThemeName);
    }
    Ok(())
}

/// A Pango font description GSettings can take; Pango reads anything, so only the form is
/// checked.
fn check_font(font: &str) -> Result<(), ValueError> {
    if font.trim().is_empty() || font.len() > MAX_LENGTH || font.chars().any(char::is_control) {
        return Err(ValueError::NotAFont);
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "one builder per setting, read best as a list"
)]
pub(super) fn settings() -> Vec<Setting> {
    vec![
        Setting::choice(STYLE, Page::AppsLookFeel, "default", STYLES)
            .section(msg!("Style"))
            .title(msg!("Style"))
            .subtitle(msg!("Light or dark, for the apps that follow it"))
            .keywords(&[
                msg!("dark mode"),
                msg!("light mode"),
                msg!("dark theme"),
                msg!("night"),
                msg!("color scheme"),
                msg!("prefer dark"),
            ])
            .gsettings(INTERFACE, "color-scheme")
            .build(),
        Setting::text("look.gtk_theme", Page::AppsLookFeel, "Adwaita")
            .section(msg!("Style"))
            .title(msg!("GTK theme"))
            .subtitle(msg!(
                "For GTK apps without libadwaita, which don't follow the style: pick a dark theme \
                 for them too"
            ))
            .keywords(&[
                msg!("theme"),
                msg!("gtk"),
                msg!("look"),
                msg!("skin"),
                msg!("widgets"),
            ])
            .check_text(check_theme)
            .gsettings(INTERFACE, "gtk-theme")
            .build(),
        Setting::text("look.icon_theme", Page::AppsLookFeel, "Adwaita")
            .section(msg!("Style"))
            .title(msg!("Icons"))
            .subtitle(msg!("The icons of apps, folders and files"))
            .keywords(&[msg!("icon theme"), msg!("icons"), msg!("symbols")])
            .check_text(check_theme)
            .gsettings(INTERFACE, "icon-theme")
            .build(),
        Setting::text("look.font", Page::AppsLookFeel, "Adwaita Sans 11")
            .section(msg!("Fonts"))
            .title(msg!("Interface"))
            .subtitle(msg!("Menus, buttons and labels"))
            .keywords(&[
                msg!("font"),
                msg!("typeface"),
                msg!("text"),
                msg!("interface font"),
            ])
            .font(Font::Any)
            .check_text(check_font)
            .gsettings(INTERFACE, "font-name")
            .build(),
        Setting::text("look.document_font", Page::AppsLookFeel, "Adwaita Sans 12")
            .section(msg!("Fonts"))
            .title(msg!("Documents"))
            .subtitle(msg!("Text you read and write in apps"))
            .keywords(&[
                msg!("font"),
                msg!("typeface"),
                msg!("document font"),
                msg!("reading"),
            ])
            .font(Font::Any)
            .check_text(check_font)
            .gsettings(INTERFACE, "document-font-name")
            .build(),
        Setting::text("look.monospace_font", Page::AppsLookFeel, "Adwaita Mono 11")
            .section(msg!("Fonts"))
            .title(msg!("Monospace"))
            .subtitle(msg!("Terminals and code"))
            .keywords(&[
                msg!("font"),
                msg!("typeface"),
                msg!("monospace font"),
                msg!("fixed width"),
                msg!("terminal"),
                msg!("code"),
            ])
            .font(Font::Monospace)
            .check_text(check_font)
            .gsettings(INTERFACE, "monospace-font-name")
            .build(),
        Setting::decimal("look.text_scale", Page::AppsLookFeel, 1.0, 0.5, 3.0)
            .section(msg!("Fonts"))
            .title(msg!("Text size"))
            .subtitle(msg!("1 is the normal size, 1.25 a quarter larger"))
            .slider(0.5, 3.0, 0.05)
            .keywords(&[
                msg!("larger text"),
                msg!("large text"),
                msg!("text scaling"),
                msg!("font size"),
                msg!("bigger"),
                msg!("zoom"),
                msg!("accessibility"),
            ])
            .gsettings(INTERFACE, "text-scaling-factor")
            .build(),
        Setting::choice(
            ANTIALIASING,
            Page::AppsLookFeel,
            "grayscale",
            ANTIALIASING_MODES,
        )
        .section(msg!("Font rendering"))
        .title(msg!("Smoothing"))
        .subtitle(msg!("How the edges of letters are smoothed"))
        .keywords(&[
            msg!("antialiasing"),
            msg!("font rendering"),
            msg!("subpixel"),
            msg!("cleartype"),
            msg!("blurry"),
        ])
        .gsettings(INTERFACE, "font-antialiasing")
        .build(),
        Setting::choice("look.hinting", Page::AppsLookFeel, "slight", HINTING_MODES)
            .section(msg!("Font rendering"))
            .title(msg!("Hinting"))
            .subtitle(msg!(
                "Fits letters to the pixel grid: sharper, but less true to their shape"
            ))
            .keywords(&[msg!("font rendering"), msg!("sharp"), msg!("crisp")])
            .gsettings(INTERFACE, "font-hinting")
            .build(),
        Setting::choice(
            "look.subpixel_order",
            Page::AppsLookFeel,
            "rgb",
            SUBPIXEL_ORDERS,
        )
        .section(msg!("Font rendering"))
        .title(msg!("Subpixel order"))
        .subtitle(msg!(
            "The order of the color stripes in a pixel of your screen; most are RGB"
        ))
        .keywords(&[
            msg!("font rendering"),
            msg!("rgb"),
            msg!("bgr"),
            msg!("lcd"),
        ])
        .depends_on(ANTIALIASING, When::OneOf(&["rgba"]))
        .gsettings(INTERFACE, "font-rgba-order")
        .build(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_names_are_directory_names() {
        assert_eq!(check_theme("Adwaita-dark"), Ok(()));
        assert_eq!(check_theme("Mint-Y Dark Aqua"), Ok(()));
        for bad in ["", "../etc", "a/b", "line\nbreak", &"x".repeat(256)] {
            assert_eq!(check_theme(bad), Err(ValueError::NotAThemeName), "{bad:?}");
        }
    }

    #[test]
    fn fonts_are_one_line() {
        assert_eq!(check_font("Adwaita Sans 11"), Ok(()));
        assert_eq!(check_font("Noto Sans Bold Italic 10.5"), Ok(()));
        for bad in ["", "  ", "Sans\t11", &"x".repeat(256)] {
            assert_eq!(check_font(bad), Err(ValueError::NotAFont), "{bad:?}");
        }
    }
}
