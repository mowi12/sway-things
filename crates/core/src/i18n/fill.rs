// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Named placeholders in translated strings.
//!
//! `format!` needs its format string as a literal, so it can't take a translation. A message with
//! values in it names them instead (`{count} problems in {file}`), and [`fill`] puts the values in
//! after the lookup. Translators can move the placeholders around, and xgettext flags such a
//! string as `rust-format`.

use std::fmt::{self, Write as _};

/// A value for a placeholder: its name (without the braces) and what to put in.
pub type Arg<'a> = (&'a str, &'a dyn fmt::Display);

/// `text` with every `{name}` replaced by the value of the argument `name`, in one pass: a value
/// is never searched for placeholders itself, so device names and file names stay as they are.
/// `{{` and `}}` are literal braces. A placeholder without an argument is left as it is, and an
/// argument the text doesn't name is left out, so a faulty translation can't fail.
pub fn fill(text: &str, args: &[Arg<'_>]) -> String {
    let mut filled = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(['{', '}']) {
        filled.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(after) = tail.strip_prefix("{{").or_else(|| tail.strip_prefix("}}")) {
            filled.push_str(&tail[..1]);
            rest = after;
            continue;
        }
        let value = tail
            .strip_prefix('{')
            .and_then(|inner| inner.split_once('}'))
            .and_then(|(name, after)| {
                let (_, value) = args.iter().find(|(arg, _)| *arg == name)?;
                Some((*value, after))
            });
        if let Some((value, after)) = value {
            // Writing to a String can't fail.
            let _ = write!(filled, "{value}");
            rest = after;
        } else {
            filled.push_str(&tail[..1]);
            rest = &tail[1..];
        }
    }
    filled.push_str(rest);
    filled
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::fill;

    #[test]
    fn fills_named_placeholders_in_any_order() {
        assert_eq!(
            fill(
                "{count} problems in {file}",
                &[("file", &"config"), ("count", &2)]
            ),
            "2 problems in config"
        );
        assert_eq!(
            fill("in {file}: {count}", &[("count", &2), ("file", &"config")]),
            "in config: 2"
        );
        assert_eq!(fill("{a}{a}", &[("a", &1)]), "11");
    }

    #[test]
    fn doubled_braces_are_literal() {
        assert_eq!(
            fill("{{count}} is {count}", &[("count", &3)]),
            "{count} is 3"
        );
        assert_eq!(fill("}} {{", &[]), "} {");
    }

    #[test]
    fn values_are_not_filled_again() {
        assert_eq!(
            fill(
                "{name} and {other}",
                &[("name", &"{other}"), ("other", &"x")]
            ),
            "{other} and x"
        );
    }

    #[test]
    fn faulty_texts_stay_as_they_are() {
        assert_eq!(fill("{unknown} {count}", &[("count", &1)]), "{unknown} 1");
        assert_eq!(fill("open { brace", &[]), "open { brace");
        assert_eq!(fill("trailing {", &[]), "trailing {");
        assert_eq!(fill("lone } brace", &[]), "lone } brace");
        assert_eq!(fill("no value", &[("count", &1)]), "no value");
    }

    proptest! {
        #[test]
        fn text_without_braces_is_unchanged(text in "[^{}]*") {
            prop_assert_eq!(fill(&text, &[("a", &1)]), text);
        }

        #[test]
        fn any_text_is_filled_without_a_panic(text in ".*", value in ".*") {
            let _ = fill(&text, &[("a", &value), ("b", &1)]);
        }
    }
}
