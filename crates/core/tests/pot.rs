// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every string of the registry and of the doctor's report is in the gettext template, so it was
//! marked with `msg!` and `po/sway-things.pot` is up to date. `scripts/check-pot.sh` checks the
//! other direction: the template holds nothing that is no longer marked.

#![allow(
    clippy::panic,
    reason = "helpers of the tests: a step that fails fails the test"
)]

use std::collections::BTreeSet;

use sway_things_core::doctor::Section;
use sway_things_core::i18n::Msg;
use sway_things_core::registry::{Group, Kind, Page, Registry};

/// The msgids of a `.pot` file: `msgid "…"` and the `"…"` lines that continue it.
fn msgids(pot: &str) -> BTreeSet<String> {
    let mut msgids = BTreeSet::new();
    let mut current: Option<String> = None;
    for line in pot.lines() {
        if let Some(rest) = line.strip_prefix("msgid ") {
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
