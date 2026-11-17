// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A message that is translated when it is shown, not when it is made.
//!
//! The core makes messages that people read and scripts and logs keep: a finding of the doctor,
//! why a row is locked, an error. A [`Text`] holds the marked message and its values, so the
//! same message gives the English text for logs and `--json` ([`Text::english`]) and the text in
//! the user's language for display ([`Text::translated`]). A value is data, shown as it is (a
//! file name, a number), or a text of its own, translated with the message (a setting's title,
//! the error that caused this one).

use std::fmt;

use super::{Msg, MsgPlural, TRANSLATIONS, Translations, fill};

/// A message with its values, in English or in the user's language when it is shown.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Text {
    message: Message,
    args: Vec<(&'static str, Value)>,
}

/// What a [`Text`] says.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Message {
    /// A marked message.
    One(Msg),
    /// A marked message about a count.
    Count(MsgPlural, usize),
    /// Data, never translated: a line of a config file, a message from another program.
    Data(String),
}

/// A value of a placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Value {
    /// Shown as it is.
    Data(String),
    /// Translated with the message it is in.
    Text(Text),
}

impl Text {
    /// The message `msg`, with values to come from [`Text::arg`] and [`Text::text`].
    pub fn new(msg: Msg) -> Self {
        Self {
            message: Message::One(msg),
            args: Vec::new(),
        }
    }

    /// The message about the count `n`, in the form `n` takes. The count isn't filled in by
    /// itself: pass it with [`Text::arg`] too.
    pub fn count(msg: MsgPlural, n: usize) -> Self {
        Self {
            message: Message::Count(msg, n),
            args: Vec::new(),
        }
    }

    /// Text that isn't translated, such as a line of the user's config or what another program
    /// said.
    pub fn data(text: impl Into<String>) -> Self {
        Self {
            message: Message::Data(text.into()),
            args: Vec::new(),
        }
    }

    /// Fills the placeholder `{name}` with `value`, shown as it is.
    #[must_use]
    pub fn arg(mut self, name: &'static str, value: impl fmt::Display) -> Self {
        self.args.push((name, Value::Data(value.to_string())));
        self
    }

    /// Fills the placeholder `{name}` with `text`, translated with this message.
    #[must_use]
    pub fn text(mut self, name: &'static str, text: impl Into<Self>) -> Self {
        self.args.push((name, Value::Text(text.into())));
        self
    }

    /// The English text: for logs, `--json` and ids.
    pub fn english(&self) -> String {
        self.render(None)
    }

    /// The text in the user's language: for display. English if there is no translation or
    /// [`init`](super::init) wasn't called.
    pub fn translated(&self) -> String {
        self.render(TRANSLATIONS.get())
    }

    /// The text in the languages of `translations`, English where they have none.
    pub fn translated_with(&self, translations: &Translations) -> String {
        self.render(Some(translations))
    }

    fn render(&self, translations: Option<&Translations>) -> String {
        let template = match &self.message {
            Message::One(msg) => translations
                .and_then(|translations| translations.get(msg.msgid()))
                .unwrap_or(msg.msgid()),
            Message::Count(msg, n) => translations
                .and_then(|translations| {
                    translations.get_plural(msg.singular(), u64::try_from(*n).unwrap_or(u64::MAX))
                })
                .unwrap_or(msg.english(*n)),
            Message::Data(text) => return text.clone(),
        };
        let values: Vec<(&str, String)> = self
            .args
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    Value::Data(data) => data.clone(),
                    Value::Text(text) => text.render(translations),
                };
                (*name, value)
            })
            .collect();
        let args: Vec<(&str, &dyn fmt::Display)> = values
            .iter()
            .map(|(name, value)| (*name, value as &dyn fmt::Display))
            .collect();
        fill(template, &args)
    }
}

impl From<Msg> for Text {
    fn from(msg: Msg) -> Self {
        Self::new(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::Text;
    use crate::i18n::{DOMAIN, Msg, MsgPlural, Translations, write_mo};

    #[test]
    fn english_fills_in_data_and_nested_texts() {
        let cause = Text::new(Msg::new("{file} is missing")).arg("file", "30-appearance.conf");
        let text = Text::new(Msg::new("{setting} can't be changed: {cause}"))
            .text("setting", Msg::new("Inner gaps"))
            .text("cause", cause);
        assert_eq!(
            text.english(),
            "Inner gaps can't be changed: 30-appearance.conf is missing"
        );
        // Without catalogs, the translation is the English text.
        assert_eq!(text.translated(), text.english());
    }

    #[test]
    fn counts_pick_their_form() {
        let lines = MsgPlural::new("{count} line comes after it", "{count} lines come after it");
        assert_eq!(
            Text::count(lines, 1).arg("count", 1).english(),
            "1 line comes after it"
        );
        assert_eq!(
            Text::count(lines, 3).arg("count", 3).english(),
            "3 lines come after it"
        );
    }

    #[test]
    fn translations_reach_nested_texts_but_not_data() {
        let dir = tempfile::tempdir().unwrap();
        let messages = dir.path().join("de/LC_MESSAGES");
        std::fs::create_dir_all(&messages).unwrap();
        std::fs::write(
            messages.join(format!("{DOMAIN}.mo")),
            write_mo(&[
                (
                    "{setting} can't be changed: {cause}",
                    "{setting} lässt sich nicht ändern: {cause}",
                ),
                ("Inner gaps", "Innenabstand"),
                (
                    "{count} line\0{count} lines",
                    "{count} Zeile\0{count} Zeilen",
                ),
            ]),
        )
        .unwrap();
        let translations = Translations::load(dir.path(), DOMAIN, &["de".to_owned()]);
        let text = Text::new(Msg::new("{setting} can't be changed: {cause}"))
            .text("setting", Msg::new("Inner gaps"))
            .text("cause", Text::data("sway said no"));
        assert_eq!(
            text.translated_with(&translations),
            "Innenabstand lässt sich nicht ändern: sway said no"
        );
        let lines = Text::count(MsgPlural::new("{count} line", "{count} lines"), 2).arg("count", 2);
        assert_eq!(lines.translated_with(&translations), "2 Zeilen");
        assert_eq!(lines.english(), "2 lines");
    }

    #[test]
    fn data_is_never_filled_in() {
        let line = Text::data("bindsym {key} exec foo").arg("key", "x");
        assert_eq!(line.english(), "bindsym {key} exec foo");
        assert_eq!(line.translated(), "bindsym {key} exec foo");
    }
}
