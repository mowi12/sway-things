// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Messages of the core in the pseudo-locale made from `po/sway-things.pot`: every part of them,
//! nested ones too, is translated, and only the values filled in stay as they are. That also
//! shows each part is marked and in the template.

#![allow(
    clippy::unwrap_used,
    reason = "helpers of the tests: a step that fails fails the test"
)]

use std::fs;

use sway_things_core::apply::ApplyError;
use sway_things_core::i18n::{self, DOMAIN, Text, Translations};
use sway_things_core::ipc::Unmet;
use sway_things_core::network;
use sway_things_core::registry::ValueError;
use sway_things_core::state::StateError;
use sway_things_core::version::SwayVersion;
use sway_things_core::view::Locked;

/// The catalogs of the pseudo-locale.
fn pseudo_locale() -> (tempfile::TempDir, Translations) {
    let dir = tempfile::tempdir().unwrap();
    let messages = dir.path().join("xx/LC_MESSAGES");
    fs::create_dir_all(&messages).unwrap();
    let pot = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../po/sway-things.pot"
    ))
    .unwrap();
    fs::write(
        messages.join(format!("{DOMAIN}.mo")),
        i18n::pseudo::catalog(&pot),
    )
    .unwrap();
    let translations = Translations::load(dir.path(), DOMAIN, &["xx".to_owned()]);
    (dir, translations)
}

/// The words of two or more ASCII letters in `text` once `data` is taken out: what wasn't
/// translated.
fn english_words(text: &str, data: &[&str]) -> Vec<String> {
    let rest = data
        .iter()
        .fold(text.to_owned(), |text, item| text.replace(item, " "));
    rest.split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| word.len() > 1)
        .map(str::to_owned)
        .collect()
}

fn check(text: &Text, data: &[&str], translations: &Translations) {
    let shown = text.translated_with(translations);
    assert!(shown.contains(i18n::pseudo::OPEN), "{shown}");
    assert_eq!(
        english_words(&shown, data),
        [] as [&str; 0],
        "{shown} (in English: {})",
        text.english()
    );
}

#[test]
fn nested_errors_are_translated_all_the_way_down() {
    let (_dir, translations) = pseudo_locale();
    let error = ApplyError::State(StateError::InvalidValue {
        key: "windows.gaps.inner",
        error: ValueError::IntegerOutOfRange { min: 0, max: 100 },
    });
    check(&error.text(), &["windows.gaps.inner"], &translations);
    let wrong_type = ValueError::WrongType {
        expected: "boolean",
        found: "string",
    };
    check(&wrong_type.text(), &[], &translations);
    // Its English form stays for logs.
    assert_eq!(
        error.to_string(),
        "windows.gaps.inner: must be between 0 and 100"
    );
}

#[test]
fn reasons_from_other_programs_stay_as_they_are() {
    let (_dir, translations) = pseudo_locale();
    check(
        &network::Error::Connect(8).text(),
        &["NetworkManager"],
        &translations,
    );
    let failed = network::Error::Failed(Text::data("Secrets were required (NoSecrets)"));
    assert_eq!(
        failed.text().translated_with(&translations),
        "Secrets were required (NoSecrets)"
    );
}

#[test]
fn locked_rows_say_why_in_the_language() {
    let (_dir, translations) = pseudo_locale();
    let needed = "1.11".parse::<SwayVersion>().unwrap();
    let locked = Locked::Unsupported(Unmet::Sway { needed });
    check(&locked.text(), &["1.11"], &translations);
}
