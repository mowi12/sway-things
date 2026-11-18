// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use super::xcursor::{self, XcursorError};
use super::{search_path, themes};
use crate::sway_config::MemoryFiles;

/// An Xcursor file with one square image per size, each pixel holding its size as blue.
pub(crate) fn xcursor_file(sizes: &[u32]) -> Vec<u8> {
    let count = u32::try_from(sizes.len()).unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"Xcur");
    for value in [16, 0x1_0000, count] {
        bytes.extend_from_slice(&u32::to_le_bytes(value));
    }
    let mut position = 16 + 12 * count;
    for &size in sizes {
        for value in [0xfffd_0002, size, position] {
            bytes.extend_from_slice(&u32::to_le_bytes(value));
        }
        position += 36 + 4 * size * size;
    }
    for &size in sizes {
        for value in [36, 0xfffd_0002, size, 1, size, size, 0, 0, 0] {
            bytes.extend_from_slice(&u32::to_le_bytes(value));
        }
        for _ in 0..size * size {
            bytes.extend_from_slice(&[u8::try_from(size).unwrap(), 0, 0, 255]);
        }
    }
    bytes
}

#[test]
fn reads_the_sizes_and_the_closest_image() {
    let file = xcursor_file(&[48, 24, 32]);
    assert_eq!(xcursor::sizes(&file).unwrap(), [24, 32, 48]);
    let image = xcursor::image(&file, 30).unwrap();
    assert_eq!(
        (image.nominal_size, image.width, image.height),
        (32, 32, 32)
    );
    assert_eq!(image.pixels.len(), 32 * 32 * 4);
    assert_eq!(&image.pixels[..4], &[32, 0, 0, 255]);
    // Equally close: the smaller one, like libXcursor.
    assert_eq!(xcursor::image(&file, 40).unwrap().nominal_size, 32);
    assert_eq!(xcursor::image(&file, 100).unwrap().nominal_size, 48);
}

#[test]
fn refuses_broken_files() {
    assert_eq!(xcursor::sizes(b"PNG"), Err(XcursorError::NotXcursor));
    assert_eq!(xcursor::sizes(b"Xcur"), Err(XcursorError::Truncated));
    let file = xcursor_file(&[24]);
    // Cut into the pixels.
    assert_eq!(
        xcursor::image(&file[..file.len() - 1], 24),
        Err(XcursorError::Truncated)
    );
    // No images at all.
    assert_eq!(
        xcursor::sizes(&xcursor_file(&[])),
        Err(XcursorError::NoImage)
    );
    // A table of contents larger than libXcursor's limit.
    let mut huge = file.clone();
    huge[12..16].copy_from_slice(&u32::to_le_bytes(0x10001));
    assert_eq!(xcursor::sizes(&huge), Err(XcursorError::TooLarge));
    // An image wider than libXcursor's limit.
    let mut wide = file.clone();
    let width = 16 + 12 + 16;
    wide[width..width + 4].copy_from_slice(&u32::to_le_bytes(0x8000));
    assert_eq!(xcursor::image(&wide, 24), Err(XcursorError::TooLarge));
    // An image position past the end.
    let mut far = file;
    far[24..28].copy_from_slice(&u32::to_le_bytes(u32::MAX));
    assert_eq!(xcursor::image(&far, 24), Err(XcursorError::Truncated));
}

#[test]
fn every_prefix_of_a_file_is_refused_or_read() {
    let file = xcursor_file(&[8, 16]);
    for end in 0..file.len() {
        let _ = xcursor::sizes(&file[..end]);
        let _ = xcursor::image(&file[..end], 16);
    }
}

#[test]
fn search_path_follows_libxcursor() {
    let files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .var("XDG_DATA_DIRS", "/usr/share:relative:/opt/share");
    assert_eq!(
        search_path(&files),
        [
            "/home/me/.local/share/icons",
            "/home/me/.icons",
            "/usr/share/icons",
            "/opt/share/icons",
            "/usr/share/pixmaps",
        ]
        .map(PathBuf::from)
    );
    let files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .var("XCURSOR_PATH", "~/cursors::/usr/share/icons");
    assert_eq!(
        search_path(&files),
        ["/home/me/cursors", "/usr/share/icons"].map(PathBuf::from)
    );
}

#[test]
fn themes_are_found_once_with_their_titles() {
    let files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .var("XDG_DATA_DIRS", "/usr/share")
        .file(
            "/home/me/.icons/Bibata/index.theme",
            "[Icon Theme]\nName[de]=Bibata DE\nName = Bibata Modern\n",
        )
        .file(
            "/home/me/.icons/Bibata/cursors/left_ptr",
            xcursor_file(&[24, 32]),
        )
        .file(
            "/usr/share/icons/Bibata/cursors/left_ptr",
            xcursor_file(&[64]),
        )
        .file(
            "/usr/share/icons/Adwaita/index.theme",
            "[Other]\nName=Wrong\n[Icon Theme]\nName=Adwaita\n",
        )
        .file(
            "/usr/share/icons/Adwaita/cursors/default",
            xcursor_file(&[24]),
        )
        .file(
            "/usr/share/icons/hicolor/index.theme",
            "[Icon Theme]\nName=Hicolor\n",
        )
        .file("/usr/share/icons/plain/cursors/arrow", xcursor_file(&[16]));
    let found = themes(&files);
    let summary: Vec<(&str, &str, &[u32])> = found
        .iter()
        .map(|theme| {
            (
                theme.name.as_str(),
                theme.title.as_str(),
                theme.sizes.as_slice(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("Adwaita", "Adwaita", [24].as_slice()),
            ("Bibata", "Bibata Modern", [24, 32].as_slice()),
            ("plain", "plain", [16].as_slice()),
        ]
    );
    assert_eq!(found[1].dir, PathBuf::from("/home/me/.icons/Bibata"));
    let preview = found[1].preview(&files, 32).unwrap();
    assert_eq!(preview.width, 32);
}
