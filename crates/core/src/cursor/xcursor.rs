// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A reader for Xcursor files (`man 3 Xcursor`), the format of the files in a cursor theme's
//! `cursors/` directory: the sizes a cursor comes in and the image of one size.
//!
//! The files come from the user's or the system's theme directories, so they are read as data:
//! every offset and size is checked against the file before use, and images larger than
//! libXcursor allows are refused.

use std::fmt;

/// The magic number at the start of every Xcursor file.
const MAGIC: &[u8; 4] = b"Xcur";
/// The chunk type of an image.
const IMAGE_TYPE: u32 = 0xfffd_0002;
/// The size of the file header and of a table of contents entry, in bytes.
const FILE_HEADER: usize = 16;
const TOC_ENTRY: usize = 12;
/// The size of an image chunk's header, in bytes.
const IMAGE_HEADER: usize = 36;
/// libXcursor's limits: at most this many table entries, and images of at most 0x7fff pixels on
/// each side.
const MAX_ENTRIES: u32 = 0x10000;
const MAX_SIDE: u32 = 0x7fff;

/// One image of a cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// The size the image is made for, which the theme's sizes are named by.
    pub nominal_size: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels, row by row, four bytes each: blue, green, red and alpha, with the colors
    /// premultiplied by alpha (Xcursor's ARGB words in little-endian byte order).
    pub pixels: Vec<u8>,
}

/// Why a file isn't a readable Xcursor file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XcursorError {
    /// It doesn't start with `Xcur`.
    NotXcursor,
    /// An offset or a size points past the end of the file.
    Truncated,
    /// The table of contents or an image is larger than libXcursor allows.
    TooLarge,
    /// The file has no image.
    NoImage,
}

impl fmt::Display for XcursorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotXcursor => "not an Xcursor file",
            Self::Truncated => "the file ends too early",
            Self::TooLarge => "an image or the table of contents is too large",
            Self::NoImage => "the file has no image",
        })
    }
}

impl std::error::Error for XcursorError {}

/// The little-endian 32-bit word at `offset`.
fn word(bytes: &[u8], offset: usize) -> Result<u32, XcursorError> {
    let end = offset.checked_add(4).ok_or(XcursorError::Truncated)?;
    let slice = bytes.get(offset..end).ok_or(XcursorError::Truncated)?;
    let mut array = [0; 4];
    array.copy_from_slice(slice);
    Ok(u32::from_le_bytes(array))
}

fn offset(value: u32) -> Result<usize, XcursorError> {
    usize::try_from(value).map_err(|_| XcursorError::Truncated)
}

/// The image entries of the table of contents: the nominal size and where the chunk starts, in
/// the order of the file.
fn image_entries(bytes: &[u8]) -> Result<Vec<(u32, usize)>, XcursorError> {
    if bytes.get(..4) != Some(MAGIC.as_slice()) {
        return Err(XcursorError::NotXcursor);
    }
    let header = offset(word(bytes, 4)?)?;
    if header < FILE_HEADER {
        return Err(XcursorError::Truncated);
    }
    let count = word(bytes, 12)?;
    if count > MAX_ENTRIES {
        return Err(XcursorError::TooLarge);
    }
    let mut entries = Vec::new();
    for index in 0..offset(count)? {
        let at = header
            .checked_add(
                index
                    .checked_mul(TOC_ENTRY)
                    .ok_or(XcursorError::Truncated)?,
            )
            .ok_or(XcursorError::Truncated)?;
        if word(bytes, at)? == IMAGE_TYPE {
            entries.push((word(bytes, at + 4)?, offset(word(bytes, at + 8)?)?));
        }
    }
    if entries.is_empty() {
        return Err(XcursorError::NoImage);
    }
    Ok(entries)
}

/// The nominal sizes of the images in the file, smallest first, each once.
///
/// # Errors
///
/// An [`XcursorError`] if the file can't be read.
pub fn sizes(bytes: &[u8]) -> Result<Vec<u32>, XcursorError> {
    let mut sizes: Vec<u32> = image_entries(bytes)?
        .into_iter()
        .map(|(size, _)| size)
        .collect();
    sizes.sort_unstable();
    sizes.dedup();
    Ok(sizes)
}

/// The first image of the size closest to `size`, as libXcursor picks it (the smaller one of two
/// equally close).
///
/// # Errors
///
/// An [`XcursorError`] if the file or the image can't be read.
pub fn image(bytes: &[u8], size: u32) -> Result<Image, XcursorError> {
    let entries = image_entries(bytes)?;
    let best = entries
        .iter()
        .map(|(nominal, _)| *nominal)
        .min_by_key(|nominal| (nominal.abs_diff(size), *nominal))
        .ok_or(XcursorError::NoImage)?;
    let (_, at) = entries
        .into_iter()
        .find(|(nominal, _)| *nominal == best)
        .ok_or(XcursorError::NoImage)?;
    read_image(bytes, at)
}

fn read_image(bytes: &[u8], at: usize) -> Result<Image, XcursorError> {
    let header = offset(word(bytes, at)?)?;
    if header < IMAGE_HEADER || word(bytes, at + 4)? != IMAGE_TYPE {
        return Err(XcursorError::Truncated);
    }
    let nominal_size = word(bytes, at + 8)?;
    let width = word(bytes, at + 16)?;
    let height = word(bytes, at + 20)?;
    if width == 0 || height == 0 {
        return Err(XcursorError::NoImage);
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(XcursorError::TooLarge);
    }
    let length = offset(width)?
        .checked_mul(offset(height)?)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(XcursorError::TooLarge)?;
    let start = at.checked_add(header).ok_or(XcursorError::Truncated)?;
    let end = start.checked_add(length).ok_or(XcursorError::Truncated)?;
    let pixels = bytes.get(start..end).ok_or(XcursorError::Truncated)?;
    Ok(Image {
        nominal_size,
        width,
        height,
        pixels: pixels.to_vec(),
    })
}
