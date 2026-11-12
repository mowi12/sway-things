// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The XKB rules of the system, for the Keyboard page: the keyboard models, layouts, variants and
//! options to pick from. They are read once, when a page first needs them.

use std::sync::OnceLock;

use gtk::glib;
use sway_things_core::sway_config::Disk;
use sway_things_core::xkb::{Item, XkbRules};

/// The XKB rules of the system. A file that can't be read is logged; without the base file the
/// lists are empty, and the page shows the names it gets from the settings.
pub fn rules() -> &'static XkbRules {
    static RULES: OnceLock<XkbRules> = OnceLock::new();
    RULES.get_or_init(|| {
        let (rules, errors) = XkbRules::load(&Disk);
        for error in errors {
            glib::g_warning!("sway-things", "XKB rules: {error}");
        }
        rules
    })
}

/// The name of a layout to show: its description, or its name if the rules don't know it.
pub fn layout_label(layout: &str) -> String {
    rules()
        .layout(layout)
        .map_or_else(|| layout.to_owned(), |known| known.item.label().to_owned())
}

/// The name of a variant of `layout` to show.
pub fn variant_label(layout: &str, variant: &str) -> String {
    rules()
        .variant(layout, variant)
        .map_or_else(|| variant.to_owned(), |known| known.label().to_owned())
}

/// Whether `item` matches the lowercase `query`: its name, description, short description, or
/// one of its languages or countries.
pub fn matches(item: &Item, query: &str) -> bool {
    let contains = |text: &str| text.to_lowercase().contains(query);
    contains(&item.name)
        || item.description.as_deref().is_some_and(contains)
        || item.short_description.as_deref().is_some_and(contains)
        || item.languages.iter().any(|language| contains(language))
        || item.countries.iter().any(|country| contains(country))
}

#[cfg(test)]
mod tests {
    use super::matches;
    use sway_things_core::xkb::Item;

    #[test]
    fn search_matches_names_descriptions_languages_and_countries() {
        let item = Item {
            name: "de".to_owned(),
            description: Some("German".to_owned()),
            languages: vec!["ger".to_owned()],
            countries: vec!["AT".to_owned()],
            ..Item::default()
        };
        for query in ["de", "germ", "ger", "at"] {
            assert!(matches(&item, query), "{query}");
        }
        assert!(!matches(&item, "french"));
    }
}
