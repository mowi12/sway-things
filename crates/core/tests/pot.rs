// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every string of the registry and of the doctor's report is in the gettext template, so it was
//! marked with `msg!` and `po/sway-things.pot` is up to date, and so is every string marked in
//! the sources: xgettext skips some, such as those in the attributes of `glib::clone!`. `scripts/check-pot.sh` checks the
//! other direction: the template holds nothing that is no longer marked.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    reason = "helpers of the tests: a step that fails fails the test"
)]

use std::collections::BTreeSet;

use sway_things_core::doctor::Section;
use sway_things_core::i18n::Msg;
use sway_things_core::registry::{Group, Kind, Page, Registry};

/// The msgids of a `.pot` file: `msgid "…"` (and `msgid_plural "…"`) and the `"…"` lines that
/// continue it.
fn msgids(pot: &str) -> BTreeSet<String> {
    let mut msgids = BTreeSet::new();
    let mut current: Option<String> = None;
    for line in pot.lines() {
        let start = line
            .strip_prefix("msgid ")
            .or_else(|| line.strip_prefix("msgid_plural "));
        if let Some(rest) = start {
            msgids.extend(current.take());
            current = Some(unquote(rest));
        } else if line.starts_with('"') {
            if let Some(msgid) = &mut current {
                msgid.push_str(&unquote(line));
            }
        } else {
            msgids.extend(current.take());
        }
    }
    msgids.extend(current);
    msgids
}

/// The text of a quoted, escaped `.pot` string.
fn unquote(quoted: &str) -> String {
    let inner = quoted
        .trim()
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("not a quoted string: {quoted}"));
    let mut text = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => text.push('\n'),
                Some('t') => text.push('\t'),
                Some(other) => text.push(other),
                None => panic!("a lone backslash in {quoted}"),
            }
        } else {
            text.push(c);
        }
    }
    text
}

/// Every message of the registry and of the doctor's headings.
fn messages() -> Vec<(String, Msg)> {
    let mut messages = Vec::new();
    for group in Group::ALL {
        messages.push((format!("{group:?}"), group.title()));
    }
    for page in Page::ALL {
        messages.push((page.id().to_owned(), page.title()));
        messages.push((page.id().to_owned(), page.description()));
    }
    for section in Section::ALL {
        messages.push((section.id().to_owned(), section.title()));
    }
    for setting in Registry::builtin().iter() {
        let key = setting.key().to_owned();
        messages.push((key.clone(), setting.title()));
        messages.push((key.clone(), setting.section()));
        messages.extend(setting.subtitle().map(|msg| (key.clone(), msg)));
        messages.extend(setting.placeholder().map(|msg| (key.clone(), msg)));
        messages.extend(setting.group().map(|group| (key.clone(), group.title())));
        messages.extend(setting.keywords().map(|msg| (key.clone(), msg)));
        if let Kind::Choice { choices, .. } = setting.kind() {
            messages.extend(choices.iter().map(|choice| (key.clone(), choice.label)));
        }
    }
    messages
}

#[test]
fn the_template_holds_every_message() {
    let pot = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../po/sway-things.pot"
    ))
    .unwrap();
    let msgids = msgids(&pot);
    let missing: Vec<String> = messages()
        .into_iter()
        .filter(|(_, msg)| !msgids.contains(msg.msgid()))
        .map(|(owner, msg)| format!("{owner}: {:?}", msg.msgid()))
        .collect();
    assert!(
        missing.is_empty(),
        "not in po/sway-things.pot (mark them with msg! and run scripts/update-pot.sh):\n{}",
        missing.join("\n")
    );
}

/// The string literals marked with `msg!` or `msg_plural!` in `source`, Rust code without its
/// comments (the examples in the documentation are left out).
fn marked(source: &str) -> Vec<String> {
    let mut code = String::new();
    for line in source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
    {
        code.push_str(line);
        code.push('\n');
    }
    let mut found = Vec::new();
    let mut rest = code.as_str();
    while let Some(start) = ["msg!(", "msg_plural!("]
        .iter()
        .filter_map(|macro_call| rest.find(macro_call))
        .min()
    {
        // Both macros end at the first `)` after their literals; a `)` in a literal is skipped.
        let mut chars = rest[start..]
            .char_indices()
            .skip_while(|(_, c)| *c != '(')
            .skip(1);
        let mut end = rest.len() - start;
        while let Some((index, c)) = chars.next() {
            match c {
                '"' => found.push(literal(&mut chars.by_ref().map(|(_, c)| c))),
                ')' => {
                    end = index;
                    break;
                }
                _ => {}
            }
        }
        rest = &rest[start + end..];
    }
    found
}

/// The text of a Rust string literal up to its closing quote, read from `chars`.
fn literal(chars: &mut impl Iterator<Item = char>) -> String {
    let mut text = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => text.push('\n'),
                Some('t') => text.push('\t'),
                Some('u') => {
                    let code: String = chars.by_ref().skip(1).take_while(|c| *c != '}').collect();
                    let code = u32::from_str_radix(&code, 16).unwrap_or_default();
                    text.extend(char::from_u32(code));
                }
                // A line continuation: the line break and the blanks after it are left out.
                Some('\n') => {
                    let mut next = chars.next();
                    while next.is_some_and(char::is_whitespace) {
                        next = chars.next();
                    }
                    match next {
                        Some('"') => break,
                        Some('\\') => text.push_str(&literal_escape(chars)),
                        Some(other) => text.push(other),
                        None => {}
                    }
                }
                Some(other) => text.push(other),
                None => {}
            },
            other => text.push(other),
        }
    }
    text
}

/// An escape right after a line continuation, which is rare: only `\"` and `\\` are expected.
fn literal_escape(chars: &mut impl Iterator<Item = char>) -> String {
    chars.next().map(String::from).unwrap_or_default()
}

/// The Rust sources of every crate.
fn sources(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn the_template_holds_every_marked_string() {
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let msgids = msgids(&std::fs::read_to_string(root.join("po/sway-things.pot")).unwrap());
    let mut files = Vec::new();
    for crate_dir in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        let src = crate_dir.path().join("src");
        if src.is_dir() {
            sources(&src, &mut files);
        }
    }
    let mut missing = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file).unwrap();
        for text in marked(&source) {
            if !msgids.contains(&text) {
                missing.push(format!("{}: {text:?}", file.display()));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "marked, but not in po/sway-things.pot (run scripts/update-pot.sh; a string xgettext \
         skips, such as one in the attributes of glib::clone!, goes into a constant outside):\n{}",
        missing.join("\n")
    );
}

#[test]
fn marked_strings_are_found_in_code() {
    let code = "let a = tr(msg!(\"Keyboard\"));\n\
                let b = msg_plural!(\"{count} file\", \"{count} files\");\n\
                let c = msg!(\n    \"Sway Things\\u{2019} value, \\\n     \\\"quoted\\\"\"\n);";
    assert_eq!(
        marked(code),
        [
            "Keyboard",
            "{count} file",
            "{count} files",
            "Sway Things\u{2019} value, \"quoted\"",
        ]
    );
}

#[test]
fn messages_are_not_empty() {
    for (owner, msg) in messages() {
        assert!(
            !msg.msgid().trim().is_empty(),
            "an empty message of {owner}"
        );
    }
}

#[test]
fn the_parser_reads_continued_and_escaped_msgids() {
    let pot = "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain\\n\"\n\n\
               msgid \"Say \\\"hi\\\" \"\n\"twice\"\nmsgstr \"\"\n";
    assert_eq!(
        msgids(pot),
        BTreeSet::from([String::new(), "Say \"hi\" twice".to_owned()])
    );
}
