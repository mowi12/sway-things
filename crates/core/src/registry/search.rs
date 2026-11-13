// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Search over the pages and settings of the registry.
//!
//! A setting is found by its title, subtitle, keywords (which hold its synonyms) and its sway
//! directive, a page by its title and description. Texts are matched case-insensitively, in the
//! translation and in English. Every word of the query has to be found, in any of the texts.
//!
//! ```
//! use sway_things_core::registry::{Field, Registry, SearchIndex, Setting};
//!
//! let index = SearchIndex::new(Registry::builtin(), |msg| msg.msgid());
//! let hits = index.search("dpi");
//! let first = hits.first().ok_or("nothing found")?;
//! assert_eq!(first.setting().map(Setting::key), Some("input.pointer.pointer_accel"));
//! assert_eq!(first.field(), Field::Keyword);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::{Link, Msg, Page, Registry, Setting};

/// Where a query was found, from the strongest match to the weakest. Hits are ranked by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    /// The title of the setting or the page.
    Title,
    /// A keyword or synonym of the setting, or the title of its group.
    Keyword,
    /// The subtitle of the setting, or the description of the page.
    Subtitle,
    /// The sway directive of the setting, e.g. `pointer_accel` or `gaps inner`.
    Directive,
}

/// A page or a setting that matches a query.
#[derive(Debug, Clone, Copy)]
pub struct Hit<'a> {
    link: Link<'a>,
    field: Field,
}

impl<'a> Hit<'a> {
    /// What to open: the page, or the setting on its page.
    pub fn link(&self) -> Link<'a> {
        self.link
    }

    /// The page that was found, or the page of the setting that was found.
    pub fn page(&self) -> Page {
        self.link.page()
    }

    /// The setting that was found, or `None` if the hit is a page.
    pub fn setting(&self) -> Option<&'a Setting> {
        self.link.setting()
    }

    /// The weakest field one of the query's words was found in.
    pub fn field(&self) -> Field {
        self.field
    }
}

/// A page or a setting with its texts, in lowercase.
#[derive(Debug, Clone)]
struct Entry<'a> {
    link: Link<'a>,
    texts: Vec<(Field, String)>,
}

/// The texts of every page and setting, prepared for searching. Build it once and keep it: the
/// translations don't change while the app runs.
#[derive(Debug, Clone)]
pub struct SearchIndex<'a> {
    entries: Vec<Entry<'a>>,
}

impl<'a> SearchIndex<'a> {
    /// An index of every page and of the settings of `registry`, with their texts in English and
    /// as `translate` gives them.
    pub fn new<S: AsRef<str>>(registry: &'a Registry, translate: impl Fn(Msg) -> S) -> Self {
        let mut entries = Vec::new();
        for page in Page::ALL {
            let mut texts = Vec::new();
            add(&mut texts, Field::Title, page.title(), &translate);
            add(&mut texts, Field::Subtitle, page.description(), &translate);
            entries.push(Entry {
                link: Link::to_page(page),
                texts,
            });
        }
        for setting in registry.iter() {
            let mut texts = Vec::new();
            add(&mut texts, Field::Title, setting.title(), &translate);
            for keyword in setting.keywords() {
                add(&mut texts, Field::Keyword, keyword, &translate);
            }
            // The members of a group have short titles ("Indicator"); the group's title names them.
            if let Some(group) = setting.group() {
                add(&mut texts, Field::Keyword, group.title(), &translate);
            }
            if let Some(subtitle) = setting.subtitle() {
                add(&mut texts, Field::Subtitle, subtitle, &translate);
            }
            if let Some(mapping) = setting.sway() {
                let directive = mapping.directive();
                let mut text = directive.name.to_lowercase();
                if let Some(argument) = directive.argument {
                    text.push(' ');
                    text.push_str(&argument.to_lowercase());
                }
                texts.push((Field::Directive, text));
            }
            entries.push(Entry {
                link: Link::to_setting(setting),
                texts,
            });
        }
        Self { entries }
    }

    /// The pages and settings that match `query`, best first. A query without words finds
    /// nothing.
    ///
    /// Every word of the query has to be in one of the texts. Hits where every word was found at
    /// the start of a word come first ("tap" in "Tap to click" before "metaphor"), then they rank
    /// by the weakest [`Field`] a word was found in (a title before a keyword). Equal hits keep
    /// the order of the pages and then the settings in the registry.
    pub fn search(&self, query: &str) -> Vec<Hit<'a>> {
        let query = query.to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        if words.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(Rank, Hit<'a>)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let rank = words
                    .iter()
                    .map(|word| best_match(&entry.texts, word))
                    .try_fold(Rank::BEST, |worst, rank| Some(worst.max(rank?)))?;
                Some((
                    rank,
                    Hit {
                        link: entry.link,
                        field: rank.field,
                    },
                ))
            })
            .collect();
        hits.sort_by_key(|(rank, _)| *rank);
        hits.into_iter().map(|(_, hit)| hit).collect()
    }
}

/// How well a word matched; smaller is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    /// Whether the word was found only inside a word, not at its start.
    inside: bool,
    field: Field,
}

impl Rank {
    const BEST: Self = Self {
        inside: false,
        field: Field::Title,
    };
}

/// Adds `msg` to `texts` in lowercase, translated and in English (once if they are the same).
fn add<S: AsRef<str>>(
    texts: &mut Vec<(Field, String)>,
    field: Field,
    msg: Msg,
    translate: &impl Fn(Msg) -> S,
) {
    let english = msg.msgid().to_lowercase();
    let translated = translate(msg).as_ref().to_lowercase();
    if translated != english {
        texts.push((field, translated));
    }
    texts.push((field, english));
}

/// The best match of `word` in `texts`, or `None` if no text contains it.
fn best_match(texts: &[(Field, String)], word: &str) -> Option<Rank> {
    texts
        .iter()
        .filter_map(|(field, text)| {
            let mut found = text.match_indices(word).map(|(at, _)| at).peekable();
            found.peek()?;
            let at_start = found.any(|at| {
                text[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|before| !before.is_alphanumeric())
            });
            Some(Rank {
                field: *field,
                inside: !at_start,
            })
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::{Field, SearchIndex};
    use crate::registry::{Msg, Page, Registry, Setting};

    /// The keys of the settings `query` finds in the built-in registry, best first.
    fn keys(query: &str) -> Vec<&'static str> {
        SearchIndex::new(Registry::builtin(), Msg::msgid)
            .search(query)
            .iter()
            .filter_map(|hit| hit.setting().map(Setting::key))
            .collect()
    }

    /// A registry of a few settings, for ranking.
    fn sample() -> Registry {
        Registry {
            settings: vec![
                Setting::toggle("sample.inside", Page::Workspaces, false)
                    .title("Metaphor")
                    .build(),
                Setting::toggle("sample.keyword", Page::Workspaces, false)
                    .title("Something else")
                    .keywords(&["tap"])
                    .build(),
                Setting::toggle("sample.subtitle", Page::Workspaces, false)
                    .title("Another thing")
                    .subtitle("Tap it")
                    .build(),
                Setting::toggle("sample.title", Page::Workspaces, false)
                    .title("Tap to click")
                    .build(),
            ],
        }
    }

    fn sample_keys<'a>(index: &SearchIndex<'a>, query: &str) -> Vec<&'a str> {
        index
            .search(query)
            .iter()
            .filter_map(|hit| hit.setting().map(Setting::key))
            .collect()
    }

    #[test]
    fn synonyms_find_the_pointer_speed() {
        for query in ["dpi", "sensitivity", "DPI", "Sensitivity"] {
            let found = keys(query);
            assert!(
                found.contains(&"input.pointer.pointer_accel"),
                "{query}: {found:?}"
            );
            assert!(
                found.contains(&"input.touchpad.pointer_accel"),
                "{query}: {found:?}"
            );
        }
    }

    #[test]
    fn matching_ignores_case() {
        assert_eq!(keys("POINTER SPEED"), keys("pointer speed"));
        assert!(keys("Pointer Speed").contains(&"input.pointer.pointer_accel"));
    }

    #[test]
    fn every_word_has_to_be_found_somewhere() {
        // "touchpad" is a keyword, "tap" in the title.
        let found = keys("touchpad tap");
        assert!(found.contains(&"input.touchpad.tap"), "{found:?}");
        assert!(!found.contains(&"input.pointer.pointer_accel"), "{found:?}");
        assert_eq!(keys("tap nothing-like-this"), [] as [&str; 0]);
    }

    #[test]
    fn the_directive_is_searched() {
        assert_eq!(
            keys("tap_button_map"),
            ["input.touchpad.tap_button_map"],
            "an option of input blocks"
        );
        assert!(keys("gaps inner").contains(&"windows.gaps.inner"));
        assert!(keys("client.focused").contains(&"windows.focused.child_border"));
    }

    #[test]
    fn the_members_of_a_group_are_found_by_its_title() {
        assert_eq!(
            keys("focused window border").first(),
            Some(&"windows.focused.border")
        );
        assert!(keys("focused window").contains(&"windows.focused.indicator"));
    }

    #[test]
    fn word_starts_rank_first_then_titles_keywords_and_subtitles() {
        let registry = sample();
        let index = SearchIndex::new(&registry, Msg::msgid);
        assert_eq!(
            sample_keys(&index, "tap"),
            [
                "sample.title",
                "sample.keyword",
                "sample.subtitle",
                "sample.inside"
            ]
        );
        let fields: Vec<Field> = index
            .search("tap")
            .iter()
            .filter(|hit| hit.setting().is_some())
            .map(super::Hit::field)
            .collect();
        assert_eq!(
            fields,
            [Field::Title, Field::Keyword, Field::Subtitle, Field::Title]
        );
    }

    #[test]
    fn a_hit_ranks_by_its_weakest_word() {
        let registry = sample();
        let index = SearchIndex::new(&registry, Msg::msgid);
        // "click" only in the title of one, "it" only in the subtitle of the other.
        assert_eq!(sample_keys(&index, "tap click"), ["sample.title"]);
        let hits = index.search("tap it");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].field(), Field::Subtitle);
    }

    #[test]
    fn pages_are_found_by_name_with_or_without_settings() {
        let index = SearchIndex::new(Registry::builtin(), Msg::msgid);
        for page in Page::ALL {
            let hits = index.search(page.title().msgid());
            let first = hits.first().unwrap();
            assert_eq!(first.page(), page, "{page:?}");
            assert!(first.setting().is_none(), "{page:?}");
            assert_eq!(first.field(), Field::Title);
        }
        // Displays has no settings yet.
        assert!(
            Registry::builtin()
                .iter()
                .all(|s| s.page() != Page::Displays)
        );
        let hits = index.search("display");
        assert!(
            hits.iter()
                .any(|hit| hit.page() == Page::Displays && hit.setting().is_none())
        );
        // The description counts too, after the titles.
        let hits = index.search("wi-fi");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page(), Page::BluetoothNetwork);
        assert_eq!(hits[0].field(), Field::Subtitle);
    }

    #[test]
    fn a_hit_carries_page_and_setting() {
        let index = SearchIndex::new(Registry::builtin(), Msg::msgid);
        let hits = index.search("tap to click");
        let hit = hits.first().unwrap();
        assert_eq!(hit.page(), Page::MouseTouchpad);
        assert_eq!(hit.setting().map(Setting::key), Some("input.touchpad.tap"));
        assert_eq!(hit.link().page(), Page::MouseTouchpad);
        assert_eq!(
            hit.link().setting().map(Setting::key),
            Some("input.touchpad.tap")
        );
    }

    #[test]
    fn a_query_without_words_finds_nothing() {
        let index = SearchIndex::new(Registry::builtin(), Msg::msgid);
        assert!(index.search("").is_empty());
        assert!(index.search("  \t ").is_empty());
    }

    #[test]
    fn the_translation_and_the_english_text_both_match() {
        let registry = sample();
        let german = |msg: Msg| match msg.msgid() {
            "Tap to click" => "Antippen zum Klicken".to_owned(),
            "tap" => "Tippen".to_owned(),
            other => other.to_owned(),
        };
        let index = SearchIndex::new(&registry, german);
        assert_eq!(sample_keys(&index, "ANTIPPEN"), ["sample.title"]);
        assert_eq!(sample_keys(&index, "klicken"), ["sample.title"]);
        // At the start of the keyword's translation before inside the title's.
        assert_eq!(
            sample_keys(&index, "tippen"),
            ["sample.keyword", "sample.title"]
        );
        assert_eq!(sample_keys(&index, "click"), ["sample.title"]);
        // A word of the translation and one of the English text, in one query.
        assert_eq!(sample_keys(&index, "antippen click"), ["sample.title"]);
    }
}
