// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The documents of a test page, best first: CUPS's own test page (`testprint`), the test page
//! of cups-filters, and a plain page of our own for a CUPS without either.

use std::fmt::Write as _;
use std::path::Path;

/// A document to print as the test page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Document {
    /// Its MIME type; `application/octet-stream` lets CUPS find it out.
    pub format: &'static str,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

/// Where CUPS's data files are on the distributions.
const DATA_DIRS: [&str; 2] = ["/usr/share/cups/data", "/usr/local/share/cups/data"];

/// The test pages there are, best first; the last one is always there.
pub(super) fn documents() -> Vec<Document> {
    let mut documents = Vec::new();
    for dir in DATA_DIRS {
        let dir = Path::new(dir);
        if let Ok(bytes) = std::fs::read(dir.join("testprint")) {
            // A CUPS banner, which a filter of CUPS or cups-filters turns into the page.
            documents.push(Document {
                format: "application/octet-stream",
                bytes,
            });
        }
        if let Ok(bytes) = std::fs::read(dir.join("default-testpage.pdf")) {
            documents.push(Document {
                format: "application/pdf",
                bytes,
            });
        }
    }
    documents.push(Document {
        format: "application/pdf",
        bytes: plain_page(),
    });
    documents
}

/// A one-page PDF that says it is a test page, on a page that fits A4 and Letter.
pub(super) fn plain_page() -> Vec<u8> {
    let content = "BT /F1 24 Tf 72 720 Td (Sway Things test page) Tj ET\n\
                   BT /F1 12 Tf 72 690 Td (If you can read this, the printer works.) Tj ET\n\
                   0 0 1 rg 72 600 120 60 re f 1 0 1 rg 202 600 120 60 re f \
                   1 1 0 rg 332 600 120 60 re f 0 0 0 rg 462 600 60 60 re f\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        let _ = write!(pdf, "{} 0 obj\n{object}\nendobj\n", i + 1);
    }
    let xref = pdf.len();
    let _ = write!(pdf, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = writeln!(pdf, "{offset:010} 00000 n ");
    }
    let _ = write!(
        pdf,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    pdf.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plain_page_is_a_pdf_with_a_valid_cross_reference_table() {
        let pdf = plain_page();
        let text = String::from_utf8(pdf.clone()).unwrap();
        assert!(text.starts_with("%PDF-1.4\n"));
        assert!(text.ends_with("%%EOF\n"));
        let xref: usize = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(text[xref..].starts_with("xref\n"));
        for (i, line) in text[xref..].lines().skip(3).take(5).enumerate() {
            let offset: usize = line[..10].parse().unwrap();
            assert!(text[offset..].starts_with(&format!("{} 0 obj", i + 1)));
        }
        assert_eq!(documents().last().unwrap().bytes, pdf);
    }
}
