// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use super::*;
use crate::sway_config::MemoryFiles;

fn names(themes: &[Theme]) -> Vec<&str> {
    themes.iter().map(|theme| theme.name.as_str()).collect()
}

#[test]
fn theme_dirs_follow_xdg_with_the_dot_directory_second() {
    let files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .var("XDG_DATA_DIRS", "/opt/share:relative:/usr/share");
    assert_eq!(
        theme_dirs(&files),
        [
            "/home/me/.local/share/themes",
            "/home/me/.themes",
            "/opt/share/themes",
            "/usr/share/themes",
        ]
        .map(PathBuf::from)
    );
    let files = files.var("XDG_DATA_HOME", "/data");
    assert_eq!(
        icon_dirs(&files)[..2],
        ["/data/icons", "/home/me/.icons"].map(PathBuf::from)
    );
}

#[test]
fn gtk_themes_need_a_stylesheet_and_include_the_built_in_ones() {
    let files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .file("/home/me/.themes/Nordic/gtk-4.0/gtk.css", "")
        .file(
            "/usr/share/themes/Adwaita-dark/index.theme",
            "[X-GNOME-Metatheme]\nName[de]=Dunkel\nName=Adwaita-dark\n",
        )
        .file("/usr/share/themes/Adwaita-dark/gtk-3.0/gtk.css", "")
        .file("/usr/share/themes/Adwaita/gtk-3.0/gtk.css", "")
        .file("/usr/share/themes/Default/gtk-3.0/gtk-keys.css", "")
        .file("/usr/share/themes/Metacity-only/metacity-1/theme.xml", "")
        .file("/usr/share/themes/Nordic/gtk-3.0/gtk.css", "");
    let themes = gtk_themes(&files);
    assert_eq!(
        names(&themes),
        ["Adwaita", "Adwaita-dark", "HighContrast", "Nordic"]
    );
    // The user's copy wins.
    assert_eq!(
        themes[3].dir.as_deref(),
        Some(std::path::Path::new("/home/me/.themes/Nordic"))
    );
    assert_eq!(themes[1].title, "Adwaita-dark");
    assert!(themes[2].dir.is_none());
}

#[test]
fn icon_themes_need_icon_directories() {
    let files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .file(
            "/usr/share/icons/Adwaita/index.theme",
            "[Icon Theme]\nName=Adwaita\nHidden=true\nDirectories=16x16/places,scalable/apps\n",
        )
        .file(
            "/usr/share/icons/hicolor/index.theme",
            "[Icon Theme]\nName=Hicolor\nDirectories=16x16/apps\n",
        )
        .file(
            "/usr/share/icons/breeze_cursors/index.theme",
            "[Icon Theme]\nName=Breeze Cursors\nInherits=breeze\n",
        )
        .file(
            "/usr/share/icons/empty/index.theme",
            "[Icon Theme]\nDirectories= , \n",
        )
        .file(
            "/usr/share/icons/papirus/index.theme",
            "[Icon Theme]\nName=Papirus\nDirectories=16x16/apps\n",
        )
        .file("/usr/share/icons/broken/index.theme", [0xff, 0xfe, b'[']);
    let themes = icon_themes(&files);
    assert_eq!(names(&themes), ["Adwaita", "papirus"]);
    assert_eq!(themes[1].title, "Papirus");
}

#[test]
fn themes_with_the_same_title_are_told_apart() {
    let index = "[Icon Theme]\nName=Papirus\nName[de]=Papyrus\nDirectories=16x16/apps\n";
    let files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .file("/home/me/.icons/papirus-mine/index.theme", index)
        .file("/usr/share/icons/Papirus/index.theme", index);
    let titles: Vec<String> = icon_themes(&files)
        .into_iter()
        .map(|theme| theme.title)
        .collect();
    assert_eq!(titles, ["Papirus (Papirus)", "Papirus (papirus-mine)"]);
}

#[test]
fn a_directory_that_cant_be_read_is_skipped() {
    let files = MemoryFiles::new().var("HOME", "/home/me");
    assert_eq!(names(&gtk_themes(&files)), BUILTIN_GTK_THEMES);
    assert_eq!(icon_themes(&files), Vec::<Theme>::new());
}
