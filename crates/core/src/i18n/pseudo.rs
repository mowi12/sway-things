// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A pseudo-translation of every string in the template, for tests that look for text that isn't
//! translated. Each translation is its msgid with every ASCII letter swapped for an accented one
//! (`Keyboard` → `Ķéýƀóàŕđ`), between [`OPEN`] and [`CLOSE`]. Placeholders (`{count}`), markup
//! tags (`<b>`) and entities (`&amp;`) stay as they are, so they still work. A plain ASCII word
//! in what is shown didn't go through a catalog, unless it is a value filled in: data.

use super::mo;

/// Where a pseudo-translation starts.
pub const OPEN: char = '⟦';

/// Where a pseudo-translation ends.
pub const CLOSE: char = '⟧';

/// The accented letters that stand for `a` to `z` and `A` to `Z`.
const LOWER: [char; 26] = [
    'à', 'ƀ', 'ç', 'đ', 'é', 'ƒ', 'ĝ', 'ĥ', 'í', 'ĵ', 'ķ', 'ĺ', 'ɱ', 'ñ', 'ó', 'ƥ', 'ʠ', 'ŕ', 'š',
    'ť', 'ú', 'ʋ', 'ŵ', 'ẋ', 'ý', 'ž',
];
const UPPER: [char; 26] = [
    'À', 'Ɓ', 'Ç', 'Đ', 'É', 'Ƒ', 'Ĝ', 'Ĥ', 'Í', 'Ĵ', 'Ķ', 'Ĺ', 'Ṁ', 'Ñ', 'Ó', 'Ƥ', 'Ǫ', 'Ŕ', 'Š',
    'Ť', 'Ú', 'Ṽ', 'Ŵ', 'Ẋ', 'Ý', 'Ž',
];

/// The pseudo-translation of `msgid`.
pub fn pseudo(msgid: &str) -> String {
    let mut text = String::with_capacity(msgid.len() * 2 + 6);
    text.push(OPEN);
    // The character that ends the part kept as it is, if in one.
    let mut kept: Option<char> = None;
    for c in msgid.chars() {
        if let Some(end) = kept {
            text.push(c);
            if c == end {
                kept = None;
            }
            continue;
        }
        kept = match c {
            '{' => Some('}'),
            '<' => Some('>'),
            '&' => Some(';'),
            _ => None,
        };
        text.push(accented(c));
    }
    text.push(CLOSE);
    text
}

/// The accented letter that stands for an ASCII letter; any other character as it is.
fn accented(c: char) -> char {
    let index = |base: u8| usize::from(u8::try_from(c).unwrap_or(0).wrapping_sub(base));
    match c {
        'a'..='z' => LOWER.get(index(b'a')).copied().unwrap_or(c),
        'A'..='Z' => UPPER.get(index(b'A')).copied().unwrap_or(c),
        _ => c,
    }
}

/// A catalog (`.mo` bytes) with the pseudo-translation of every message of `pot`, the text of a
/// gettext template such as `po/sway-things.pot`. Plural entries get it for both forms.
pub fn catalog(pot: &str) -> Vec<u8> {
    let messages: Vec<(String, String)> = entries(pot)
        .into_iter()
        .filter(|(msgid, _)| !msgid.is_empty())
        .map(|(msgid, plural)| {
            if let Some(plural) = plural {
                (
                    format!("{msgid}\0{plural}"),
                    format!("{}\0{}", pseudo(&msgid), pseudo(&plural)),
                )
            } else {
                let text = pseudo(&msgid);
                (msgid, text)
            }
        })
        .collect();
    let mut pairs: Vec<(&str, &str)> = messages
        .iter()
        .map(|(msgid, text)| (msgid.as_str(), text.as_str()))
        .collect();
    pairs.push(("", "Plural-Forms: nplurals=2; plural=(n != 1);\n"));
    mo::write(&pairs)
}

/// The msgids of `pot` with their plural, if they have one.
fn entries(pot: &str) -> Vec<(String, Option<String>)> {
    /// The string a continuation line (`"…"`) belongs to.
    #[derive(Clone, Copy)]
    enum Part {
        Msgid,
        Plural,
        Other,
    }
    let mut entries: Vec<(String, Option<String>)> = Vec::new();
    let mut part = Part::Other;
    for line in pot.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("msgid ") {
            entries.push((unquote(rest), None));
            part = Part::Msgid;
        } else if let Some(rest) = line.strip_prefix("msgid_plural ") {
            if let Some(entry) = entries.last_mut() {
                entry.1 = Some(unquote(rest));
            }
            part = Part::Plural;
        } else if line.starts_with('"') {
            let text = unquote(line);
            match (part, entries.last_mut()) {
                (Part::Msgid, Some(entry)) => entry.0.push_str(&text),
                (Part::Plural, Some((_, Some(plural)))) => plural.push_str(&text),
                _ => {}
            }
        } else {
            part = Part::Other;
        }
    }
    entries
}

/// The text of a quoted PO string, with its escapes undone.
fn unquote(quoted: &str) -> String {
    let inner = quoted
        .trim()
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default();
    let mut text = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => text.push('\n'),
            Some('t') => text.push('\t'),
            Some(other) => text.push(other),
            None => {}
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{catalog, entries, pseudo};
    use crate::i18n::Catalog;

    const POT: &str = r#"# A template.
msgid ""
msgstr ""
"Project-Id-Version: sway-things\n"
"Content-Type: text/plain; charset=UTF-8\n"

#: crates/cli/src/doctor.rs
msgid "Checking the compositor…"
msgstr ""

#. TRANSLATORS: A note.
#: crates/cli/src/lib.rs
#, rust-format
msgid ""
"{setting} can lock you out, so it is only changed after you keep it in a "
"terminal"
msgstr ""

msgid "a \"quoted\" word\tand\\n"
msgstr ""

msgid "{count} problem"
msgid_plural "{count} problems"
msgstr[0] ""
msgstr[1] ""
"#;

    #[test]
    fn reads_the_messages_of_a_template() {
        assert_eq!(
            entries(POT),
            [
                (String::new(), None),
                ("Checking the compositor…".to_owned(), None),
                (
                    "{setting} can lock you out, so it is only changed after you keep it in a \
                     terminal"
                        .to_owned(),
                    None
                ),
                ("a \"quoted\" word\tand\\n".to_owned(), None),
                (
                    "{count} problem".to_owned(),
                    Some("{count} problems".to_owned())
                ),
            ]
        );
    }

    #[test]
    fn pseudo_translates_every_message() {
        let catalog = Catalog::parse(&catalog(POT)).unwrap();
        assert_eq!(
            catalog.get("Checking the compositor…"),
            Some("⟦Çĥéçķíñĝ ťĥé çóɱƥóšíťóŕ…⟧")
        );
        assert_eq!(
            catalog.get_plural("{count} problem", 1),
            Some("⟦{count} ƥŕóƀĺéɱ⟧")
        );
        assert_eq!(
            catalog.get_plural("{count} problem", 2),
            Some("⟦{count} ƥŕóƀĺéɱš⟧")
        );
        assert_eq!(catalog.get(""), None);
    }

    #[test]
    fn the_template_of_the_project_is_read() {
        let pot = include_str!("../../../../po/sway-things.pot");
        let catalog = Catalog::parse(&catalog(pot)).unwrap();
        assert_eq!(catalog.get("Keyboard"), Some("⟦Ķéýƀóàŕđ⟧"));
    }

    #[test]
    fn placeholders_markup_and_entities_stay() {
        assert_eq!(
            pseudo("Run `{command}` in <b>Mouse &amp; Touchpad</b>, {{not}} {"),
            "⟦Ŕúñ `{command}` íñ <b>Ṁóúšé &amp; Ťóúçĥƥàđ</b>, {{not}} {⟧"
        );
    }
}
