// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A PDF of scanned pages, written without a library: a page per image, as large as the paper
//! the image was scanned from.
//!
//! - A page scanned in color or gray comes as a JPEG file from scanimage, which goes into the
//!   PDF as it is (`/DCTDecode`): nothing is decoded or compressed again.
//! - A page in black and white comes as a PNM file (`P4`, one bit a pixel), and goes in run-length
//!   encoded (`/RunLengthDecode`), which keeps the white of a page small. Gray and color PNM
//!   (`P5`, `P6`), which some backends give for black and white, go in the same way.
//!
//! The files come from scanimage, and so from a scanner's backend: they are parsed with bounds
//! checked everywhere, and a file that isn't what it should be is an error, never a panic.

use std::fmt::Write as _;

/// Why a page can't go into a PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageError {
    /// It is neither a JPEG nor a PNM file.
    UnknownFormat,
    /// It is cut short or its header doesn't hold together.
    Damaged,
    /// It is a kind of image the PDF can't take as it is (16 bits, CMYK, progressive).
    Unsupported,
}

impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnknownFormat => "not a JPEG or PNM image",
            Self::Damaged => "a damaged image",
            Self::Unsupported => "an image of a kind a PDF can't take as it is",
        })
    }
}

/// A scanned page, ready to go into a PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    width: u32,
    height: u32,
    /// The color space: `/DeviceGray` or `/DeviceRGB`.
    space: &'static str,
    /// Bits a color component.
    bits: u8,
    /// The filter its data is encoded with: `/DCTDecode` or `/RunLengthDecode`.
    filter: &'static str,
    /// Whether 1 is black, as in PNM's bitmaps, where it is white in a PDF's gray.
    inverted: bool,
    data: Vec<u8>,
}

impl Page {
    /// The page in `bytes`, a JPEG or PNM file.
    ///
    /// # Errors
    ///
    /// A [`PageError`] if it isn't one, or can't go into a PDF as it is.
    pub fn read(bytes: &[u8]) -> Result<Self, PageError> {
        match bytes {
            [0xff, 0xd8, ..] => jpeg(bytes),
            [b'P', b'4' | b'5' | b'6', ..] => pnm(bytes),
            _ => Err(PageError::UnknownFormat),
        }
    }

    /// Its width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// The page of a JPEG file: its size and components from its frame header (`SOF`).
fn jpeg(bytes: &[u8]) -> Result<Page, PageError> {
    let mut at = 2;
    loop {
        // Markers may be padded with 0xff.
        while bytes.get(at) == Some(&0xff) && bytes.get(at + 1) == Some(&0xff) {
            at += 1;
        }
        let (Some(&0xff), Some(&marker)) = (bytes.get(at), bytes.get(at + 1)) else {
            return Err(PageError::Damaged);
        };
        at += 2;
        // Markers without a length.
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }
        if marker == 0xd9 || marker == 0xda {
            // The end, or the image data, before any frame header.
            return Err(PageError::Damaged);
        }
        let length = usize::from(u16::from_be_bytes([
            *bytes.get(at).ok_or(PageError::Damaged)?,
            *bytes.get(at + 1).ok_or(PageError::Damaged)?,
        ]));
        if length < 2 {
            return Err(PageError::Damaged);
        }
        let segment = bytes.get(at + 2..at + length).ok_or(PageError::Damaged)?;
        match marker {
            // Baseline and extended sequential frames; a PDF reader takes progressive ones too.
            0xc0..=0xc2 => {
                let [precision, h1, h0, w1, w0, components, ..] = *segment else {
                    return Err(PageError::Damaged);
                };
                let space = match components {
                    1 => "/DeviceGray",
                    3 => "/DeviceRGB",
                    _ => return Err(PageError::Unsupported),
                };
                if precision != 8 {
                    return Err(PageError::Unsupported);
                }
                let height = u32::from(u16::from_be_bytes([h1, h0]));
                let width = u32::from(u16::from_be_bytes([w1, w0]));
                if width == 0 || height == 0 {
                    return Err(PageError::Damaged);
                }
                return Ok(Page {
                    width,
                    height,
                    space,
                    bits: 8,
                    filter: "/DCTDecode",
                    inverted: false,
                    data: bytes.to_vec(),
                });
            }
            // Lossless, hierarchical and arithmetic-coded frames.
            0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => {
                return Err(PageError::Unsupported);
            }
            _ => at += length,
        }
    }
}

/// The page of a binary PNM file: `P4` (bitmap), `P5` (gray) or `P6` (color), 8 bits at most.
fn pnm(bytes: &[u8]) -> Result<Page, PageError> {
    let kind = bytes.get(1).copied().ok_or(PageError::Damaged)?;
    let mut at = 2;
    let fields = if kind == b'4' { 2 } else { 3 };
    let mut numbers = Vec::new();
    while numbers.len() < fields {
        match bytes.get(at) {
            Some(b'#') => {
                while bytes.get(at).is_some_and(|byte| *byte != b'\n') {
                    at += 1;
                }
            }
            Some(byte) if byte.is_ascii_whitespace() => at += 1,
            Some(byte) if byte.is_ascii_digit() => {
                let start = at;
                while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                    at += 1;
                }
                let digits =
                    std::str::from_utf8(&bytes[start..at]).map_err(|_| PageError::Damaged)?;
                numbers.push(digits.parse::<u32>().map_err(|_| PageError::Damaged)?);
            }
            _ => return Err(PageError::Damaged),
        }
    }
    // One whitespace character ends the header.
    if !bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        return Err(PageError::Damaged);
    }
    at += 1;
    let (width, height) = (numbers[0], numbers[1]);
    if width == 0 || height == 0 {
        return Err(PageError::Damaged);
    }
    let (space, bits, components) = match kind {
        b'4' => ("/DeviceGray", 1, 1),
        b'5' => ("/DeviceGray", 8, 1),
        _ => ("/DeviceRGB", 8, 3),
    };
    if kind != b'4' && numbers[2] != 255 {
        return Err(PageError::Unsupported);
    }
    let row = if bits == 1 {
        u64::from(width).div_ceil(8)
    } else {
        u64::from(width) * components
    };
    let size = row
        .checked_mul(u64::from(height))
        .and_then(|size| usize::try_from(size).ok())
        .ok_or(PageError::Damaged)?;
    let end = at.checked_add(size).ok_or(PageError::Damaged)?;
    let data = bytes.get(at..end).ok_or(PageError::Damaged)?;
    Ok(Page {
        width,
        height,
        space,
        bits,
        filter: "/RunLengthDecode",
        inverted: kind == b'4',
        data: run_length(data),
    })
}

/// `data` run-length encoded as PDF's `/RunLengthDecode` reads it: a length byte and up to 128
/// bytes as they are, or a byte repeated up to 128 times; 128 ends it.
fn run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 4 + 16);
    let mut literal: Vec<u8> = Vec::with_capacity(128);
    let flush = |literal: &mut Vec<u8>, out: &mut Vec<u8>| {
        if let Some(length) = literal.len().checked_sub(1) {
            out.push(u8::try_from(length).unwrap_or(127));
            out.append(literal);
        }
    };
    let mut i = 0;
    while let Some(&byte) = data.get(i) {
        let run = data[i..]
            .iter()
            .take(128)
            .take_while(|other| **other == byte)
            .count();
        if run >= 3 {
            flush(&mut literal, &mut out);
            // 257 - run: 254 for a run of 3, 129 for 128.
            out.push(u8::try_from(257 - run).unwrap_or(129));
            out.push(byte);
            i += run;
        } else {
            literal.push(byte);
            if literal.len() == 128 {
                flush(&mut literal, &mut out);
            }
            i += 1;
        }
    }
    flush(&mut literal, &mut out);
    out.push(128);
    out
}

/// The PDF of `pages`, each with the resolution it was scanned at, in dots per inch.
pub fn write(pages: &[(Page, u32)]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    // 1: the catalog, 2: the pages, then three objects a page: the page, its contents, its image.
    let page_ids: Vec<usize> = (0..pages.len()).map(|i| 3 + i * 3).collect();
    let object = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        let id = offsets.len();
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    object(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    let mut kids = String::new();
    for id in &page_ids {
        let _ = write!(kids, "{id} 0 R ");
    }
    object(
        &mut out,
        &mut offsets,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.trim_end(),
            pages.len()
        )
        .as_bytes(),
    );
    for ((page, dpi), id) in pages.iter().zip(&page_ids) {
        let dpi = f64::from((*dpi).max(1));
        let width = f64::from(page.width) * 72.0 / dpi;
        let height = f64::from(page.height) * 72.0 / dpi;
        object(
            &mut out,
            &mut offsets,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width:.2} {height:.2}] \
                 /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>",
                id + 2,
                id + 1
            )
            .as_bytes(),
        );
        let contents = format!("q {width:.2} 0 0 {height:.2} 0 0 cm /Im0 Do Q");
        object(
            &mut out,
            &mut offsets,
            format!(
                "<< /Length {} >>\nstream\n{contents}\nendstream",
                contents.len() + 1
            )
            .as_bytes(),
        );
        let decode = if page.inverted { " /Decode [1 0]" } else { "" };
        let mut image = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {} \
             /BitsPerComponent {} /Filter {}{decode} /Length {} >>\nstream\n",
            page.width,
            page.height,
            page.space,
            page.bits,
            page.filter,
            page.data.len()
        )
        .into_bytes();
        image.extend_from_slice(&page.data);
        image.extend_from_slice(b"\nendstream");
        object(&mut out, &mut offsets, &image);
    }
    let xref = out.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for offset in &offsets {
        let _ = writeln!(table, "{offset:010} 00000 n ");
    }
    let _ = write!(
        table,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        offsets.len() + 1
    );
    out.extend_from_slice(table.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest JPEG header with a baseline frame of `width` × `height` and `components`.
    fn jpeg_bytes(width: u16, height: u16, components: u8) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x4a, 0x46];
        bytes.extend_from_slice(&[0xff, 0xc0, 0x00, 0x0b, 8]);
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[components, 1, 0x11, 0]);
        bytes.extend_from_slice(&[0xff, 0xda, 0x00, 0x02, 0xff, 0xd9]);
        bytes
    }

    /// Decodes `/RunLengthDecode` data.
    fn run_length_decode(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(&length) = data.get(i) {
            match length {
                128 => break,
                0..=127 => {
                    let n = usize::from(length) + 1;
                    out.extend_from_slice(&data[i + 1..i + 1 + n]);
                    i += 1 + n;
                }
                _ => {
                    out.extend(std::iter::repeat_n(data[i + 1], 257 - usize::from(length)));
                    i += 2;
                }
            }
        }
        out
    }

    #[test]
    fn a_jpeg_goes_in_as_it_is() {
        let bytes = jpeg_bytes(2480, 3508, 3);
        let page = Page::read(&bytes).unwrap();
        assert_eq!(page.size(), (2480, 3508));
        assert_eq!(page.space, "/DeviceRGB");
        assert_eq!(page.data, bytes);
        assert_eq!(
            Page::read(&jpeg_bytes(10, 10, 4)),
            Err(PageError::Unsupported)
        );
    }

    #[test]
    fn pnm_pages_are_read_and_encoded() {
        let mut bitmap = b"P4\n# made by scanimage\n10 2\n".to_vec();
        bitmap.extend_from_slice(&[0xff, 0xc0, 0x00, 0x00]);
        let page = Page::read(&bitmap).unwrap();
        assert_eq!((page.size(), page.bits, page.inverted), ((10, 2), 1, true));
        assert_eq!(run_length_decode(&page.data), [0xff, 0xc0, 0x00, 0x00]);

        let mut gray = b"P5 3 2 255\n".to_vec();
        gray.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let page = Page::read(&gray).unwrap();
        assert_eq!(run_length_decode(&page.data), [1, 2, 3, 4, 5, 6]);

        assert_eq!(Page::read(b"P6 2 2 65535\n"), Err(PageError::Unsupported));
        assert_eq!(Page::read(b"P5 3 2 255\n\x01"), Err(PageError::Damaged));
        assert_eq!(Page::read(b"P4 0 2\n"), Err(PageError::Damaged));
        assert_eq!(Page::read(b"P4 99999999999 2\n"), Err(PageError::Damaged));
        assert_eq!(
            Page::read(b"P6 4294967295 4294967295 255\n"),
            Err(PageError::Damaged)
        );
        assert_eq!(Page::read(b"GIF89a"), Err(PageError::UnknownFormat));
        assert_eq!(Page::read(&[0xff, 0xd8]), Err(PageError::Damaged));
    }

    #[test]
    fn run_length_round_trips() {
        let cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            vec![7],
            vec![0; 1000],
            (0..=255).collect(),
            [vec![1, 2], vec![9; 130], vec![3, 3], vec![4; 3]].concat(),
        ];
        for case in cases {
            assert_eq!(run_length_decode(&run_length(&case)), case);
        }
        // A white page stays small.
        assert!(run_length(&vec![0; 1_000_000]).len() < 20_000);
    }

    #[test]
    fn a_pdf_holds_every_page() {
        let first = Page::read(&jpeg_bytes(300, 600, 1)).unwrap();
        let mut bitmap = b"P4 8 1\n".to_vec();
        bitmap.push(0x0f);
        let second = Page::read(&bitmap).unwrap();
        let pdf = write(&[(first, 300), (second, 300)]);
        let text = String::from_utf8_lossy(&pdf);
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(text.ends_with("%%EOF\n"));
        assert!(text.contains("/Count 2"));
        assert!(text.contains("/MediaBox [0 0 72.00 144.00]"));
        assert!(text.contains("/Filter /DCTDecode"));
        assert!(text.contains("/Filter /RunLengthDecode /Decode [1 0]"));
        // Each entry of the cross-reference table points at its object.
        let xref = text.rfind("xref\n").unwrap();
        for (id, line) in text[xref..].lines().skip(3).take(8).enumerate() {
            let offset: usize = line[..10].parse().unwrap();
            assert!(
                pdf[offset..].starts_with(format!("{} 0 obj", id + 1).as_bytes()),
                "{line}"
            );
        }
    }
}
