// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The binary message catalogs (`.mo` files) that `msgfmt` writes.
//!
//! A catalog starts with a magic number that also tells the byte order, then two tables of
//! (length, offset) pairs: the msgids and their translations, in the same order. Strings end with
//! a NUL byte, which the lengths don't count. Only the singular form is read: a plural entry
//! (`msgid\0msgid_plural`) is found by its singular msgid and gives the first translation.

use std::collections::HashMap;
use std::fmt;

/// The magic number at the start of a catalog, as read in the byte order it was written in.
const MAGIC: u32 = 0x9504_12de;

/// Why a catalog can't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoError {
    /// The file doesn't start with the magic number of a catalog.
    NotACatalog,
    /// A major revision other than 0 or 1.
    UnsupportedRevision(u32),
    /// A table or a string lies outside the file.
    Truncated,
}

impl fmt::Display for MoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotACatalog => f.write_str("not a gettext message catalog"),
            Self::UnsupportedRevision(revision) => {
                write!(f, "unsupported message catalog revision {revision}")
            }
            Self::Truncated => f.write_str("the message catalog is truncated"),
        }
    }
}

impl std::error::Error for MoError {}

/// The translations of one catalog, by msgid.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    messages: HashMap<String, String>,
}

impl Catalog {
    /// Reads the catalog in `bytes`. Entries that aren't UTF-8, the header entry and empty
    /// translations are left out.
    ///
    /// # Errors
    ///
    /// [`MoError`] if `bytes` aren't a catalog, or one of a revision this reader doesn't know.
    pub fn parse(bytes: &[u8]) -> Result<Self, MoError> {
        let reader = Reader::new(bytes)?;
        let revision = reader.word(4)?;
        if revision >> 16 > 1 {
            return Err(MoError::UnsupportedRevision(revision >> 16));
        }
        let count = reader.word(8)?;
        let originals = reader.word(12)?;
        let translations = reader.word(16)?;
        let mut messages = HashMap::new();
        for index in 0..count {
            let entry = index.checked_mul(8).ok_or(MoError::Truncated)?;
            let original =
                reader.string(originals.checked_add(entry).ok_or(MoError::Truncated)?)?;
            let translation =
                reader.string(translations.checked_add(entry).ok_or(MoError::Truncated)?)?;
            let (Ok(original), Ok(translation)) = (
                std::str::from_utf8(original),
                std::str::from_utf8(translation),
            ) else {
                continue;
            };
            let msgid = singular(original);
            let text = singular(translation);
            if !msgid.is_empty() && !text.is_empty() {
                messages.insert(msgid.to_owned(), text.to_owned());
            }
        }
        Ok(Self { messages })
    }

    /// The translation of `msgid`, or `None` if the catalog has none.
    pub fn get(&self, msgid: &str) -> Option<&str> {
        self.messages.get(msgid).map(String::as_str)
    }
}

/// The singular part of a msgid or translation: up to the first NUL byte.
fn singular(text: &str) -> &str {
    text.split('\0').next().unwrap_or(text)
}

/// Reads the words and strings of a catalog in its byte order.
struct Reader<'a> {
    bytes: &'a [u8],
    little_endian: bool,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, MoError> {
        let magic: [u8; 4] = bytes
            .get(..4)
            .and_then(|start| start.try_into().ok())
            .ok_or(MoError::NotACatalog)?;
        let little_endian = if u32::from_le_bytes(magic) == MAGIC {
            true
        } else if u32::from_be_bytes(magic) == MAGIC {
            false
        } else {
            return Err(MoError::NotACatalog);
        };
        Ok(Self {
            bytes,
            little_endian,
        })
    }

    /// The 32-bit word at `offset`.
    fn word(&self, offset: u32) -> Result<u32, MoError> {
        let start = usize::try_from(offset).map_err(|_| MoError::Truncated)?;
        let word: [u8; 4] = start
            .checked_add(4)
            .and_then(|end| self.bytes.get(start..end))
            .and_then(|word| word.try_into().ok())
            .ok_or(MoError::Truncated)?;
        Ok(if self.little_endian {
            u32::from_le_bytes(word)
        } else {
            u32::from_be_bytes(word)
        })
    }

    /// The string that the (length, offset) pair at `entry` points to.
    fn string(&self, entry: u32) -> Result<&'a [u8], MoError> {
        let length = self.word(entry)?;
        let offset = self.word(entry.checked_add(4).ok_or(MoError::Truncated)?)?;
        let start = usize::try_from(offset).map_err(|_| MoError::Truncated)?;
        let length = usize::try_from(length).map_err(|_| MoError::Truncated)?;
        start
            .checked_add(length)
            .and_then(|end| self.bytes.get(start..end))
            .ok_or(MoError::Truncated)
    }
}

/// A little-endian catalog with `messages` as (msgid, translation) pairs, like `msgfmt` writes
/// it. For tests that need translations without the gettext tools.
#[cfg(any(test, feature = "test-support"))]
pub fn write(messages: &[(&str, &str)]) -> Vec<u8> {
    const HEADER: usize = 28;
    let mut messages = messages.to_vec();
    messages.sort_unstable();
    let tables = HEADER + 16 * messages.len();
    let mut strings = Vec::new();
    let mut entries = Vec::new();
    let texts = messages.iter().map(|message| message.0);
    for text in texts.chain(messages.iter().map(|message| message.1)) {
        entries.push((text.len(), tables + strings.len()));
        strings.extend_from_slice(text.as_bytes());
        strings.push(0);
    }
    let word = |number: usize| u32::try_from(number).unwrap_or(u32::MAX).to_le_bytes();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC.to_le_bytes());
    bytes.extend_from_slice(&word(0));
    bytes.extend_from_slice(&word(messages.len()));
    bytes.extend_from_slice(&word(HEADER));
    bytes.extend_from_slice(&word(HEADER + 8 * messages.len()));
    // No hash table: its size and offset are 0.
    bytes.extend_from_slice(&word(0));
    bytes.extend_from_slice(&word(0));
    for (length, offset) in entries {
        bytes.extend_from_slice(&word(length));
        bytes.extend_from_slice(&word(offset));
    }
    bytes.extend_from_slice(&strings);
    bytes
}

#[cfg(test)]
mod tests {
    use super::{Catalog, MoError, write};

    #[test]
    fn reads_what_it_wrote() {
        let catalog = Catalog::parse(&write(&[
            ("Keyboard", "Tastatur"),
            ("Mouse & Touchpad", "Maus & Touchpad"),
        ]))
        .unwrap();
        assert_eq!(catalog.get("Keyboard"), Some("Tastatur"));
        assert_eq!(catalog.get("Mouse & Touchpad"), Some("Maus & Touchpad"));
        assert_eq!(catalog.get("Displays"), None);
    }

    #[test]
    fn leaves_out_the_header_and_empty_translations() {
        let catalog = Catalog::parse(&write(&[
            ("", "Content-Type: text/plain; charset=UTF-8\n"),
            ("Bar", ""),
        ]))
        .unwrap();
        assert_eq!(catalog.get(""), None);
        assert_eq!(catalog.get("Bar"), None);
    }

    #[test]
    fn finds_plural_entries_by_their_singular() {
        let catalog =
            Catalog::parse(&write(&[("one file\0%d files", "eine Datei\0%d Dateien")])).unwrap();
        assert_eq!(catalog.get("one file"), Some("eine Datei"));
    }

    #[test]
    fn reads_big_endian_catalogs() {
        let little = write(&[("Keyboard", "Tastatur")]);
        let mut big = Vec::new();
        // The header and the tables are words; the strings after them are bytes.
        let words = 7 + 4;
        for word in little.chunks(4).take(words) {
            big.extend(word.iter().rev());
        }
        big.extend_from_slice(&little[words * 4..]);
        let catalog = Catalog::parse(&big).unwrap();
        assert_eq!(catalog.get("Keyboard"), Some("Tastatur"));
    }

    #[test]
    fn rejects_broken_files() {
        assert_eq!(Catalog::parse(b"").unwrap_err(), MoError::NotACatalog);
        assert_eq!(
            Catalog::parse(b"# a .po file").unwrap_err(),
            MoError::NotACatalog
        );
        let mut bytes = write(&[("Keyboard", "Tastatur")]);
        bytes.truncate(40);
        assert_eq!(Catalog::parse(&bytes).unwrap_err(), MoError::Truncated);
        let mut bytes = write(&[]);
        bytes[6] = 2;
        assert_eq!(
            Catalog::parse(&bytes).unwrap_err(),
            MoError::UnsupportedRevision(2)
        );
    }

    #[test]
    fn skips_entries_that_are_not_utf8() {
        let mut bytes = write(&[("Keyboard", "Tastatur")]);
        let end = bytes.len();
        // The last byte before the final NUL belongs to the translation.
        bytes[end - 2] = 0xff;
        assert_eq!(Catalog::parse(&bytes).unwrap().get("Keyboard"), None);
    }
}
