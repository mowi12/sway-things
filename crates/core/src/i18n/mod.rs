// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Translation of the strings Sway Things shows, with gettext message catalogs.
//!
//! A translatable string is a [`Msg`]: its msgid, the American English source string. Mark it
//! with [`msg!`](crate::msg), which `scripts/update-pot.sh` extracts into `po/sway-things.pot`,
//! and show it through [`tr`]. Each binary calls [`init`] once at startup; until then (and in
//! every test that doesn't set it up) [`tr`] gives the English source string.
//!
//! The catalogs are `<dir>/<language>/LC_MESSAGES/sway-things.mo`. The directory is
//! `$SWAY_THINGS_LOCALEDIR` if set, else the one the build set in the same variable, else
//! `/usr/share/locale`. The languages come from `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` and `LANG` as
//! gettext reads them.
//!
//! ```
//! use sway_things_core::{i18n, msg};
//!
//! let title = msg!("Keyboard");
//! assert_eq!(i18n::tr(title), "Keyboard");
//! ```

mod mo;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(any(test, feature = "test-support"))]
pub use mo::write as write_mo;
pub use mo::{Catalog, MoError};

/// The gettext text domain of both binaries: the catalogs are named `sway-things.mo`.
pub const DOMAIN: &str = "sway-things";

/// The environment variable with the directory of the catalogs. Read at run time, and at build
/// time for the default.
pub const LOCALEDIR_VAR: &str = "SWAY_THINGS_LOCALEDIR";

/// Where the catalogs are if neither the environment nor the build says otherwise.
const DEFAULT_LOCALEDIR: &str = match option_env!("SWAY_THINGS_LOCALEDIR") {
    Some(dir) => dir,
    None => "/usr/share/locale",
};

/// A translatable source string (a gettext msgid, in American English). Translate it before
/// showing it; search matches the translation and the English original.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Msg(&'static str);

impl Msg {
    /// A string that is translated, but not extracted into the template. For tests; strings the
    /// app shows are marked with [`msg!`](crate::msg).
    pub const fn new(msgid: &'static str) -> Self {
        Self(msgid)
    }

    /// The English source string. For ids and logs, never for what the user reads.
    pub const fn msgid(self) -> &'static str {
        self.0
    }
}

/// Marks a string literal as translatable: a [`Msg`] with it as the msgid, which
/// `scripts/update-pot.sh` extracts. Usable in constants.
///
/// ```
/// use sway_things_core::i18n::Msg;
/// use sway_things_core::msg;
///
/// const KEYWORDS: &[Msg] = &[msg!("trackpad"), msg!("mouse")];
/// assert_eq!(KEYWORDS[0].msgid(), "trackpad");
/// ```
#[macro_export]
macro_rules! msg {
    ($msgid:literal) => {
        const { $crate::i18n::Msg::new($msgid) }
    };
}

/// The catalogs of the user's languages, in the order they are asked for.
#[derive(Debug, Clone, Default)]
pub struct Translations {
    catalogs: Vec<Catalog>,
}

impl Translations {
    /// The catalogs of `domain` in `dir` for `languages` (such as `de_DE.UTF-8`, tried as
    /// `de_DE.UTF-8`, `de_DE` and `de`). A language without a readable catalog is skipped.
    pub fn load(dir: &Path, domain: &str, languages: &[String]) -> Self {
        let file = format!("{domain}.mo");
        let catalogs = languages
            .iter()
            .filter_map(|language| {
                variants(language).into_iter().find_map(|variant| {
                    let path = dir.join(variant).join("LC_MESSAGES").join(&file);
                    let bytes = std::fs::read(path).ok()?;
                    Catalog::parse(&bytes).ok()
                })
            })
            .collect();
        Self { catalogs }
    }

    /// The catalogs of the user's languages, as the environment `var` names them.
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Self {
        Self::load(&locale_dir(&var), DOMAIN, &languages(&var))
    }

    /// The translation of `msgid` in the first language that has one.
    pub fn get(&self, msgid: &str) -> Option<&str> {
        self.catalogs.iter().find_map(|catalog| catalog.get(msgid))
    }
}

/// The translations [`tr`] uses, set by [`init`].
static TRANSLATIONS: OnceLock<Translations> = OnceLock::new();

/// Loads the catalogs of the user's languages for [`tr`]. Each binary calls it once at startup;
/// later calls change nothing.
pub fn init() {
    TRANSLATIONS.get_or_init(|| Translations::from_env(|name| std::env::var(name).ok()));
}

/// The text of `msg` in the user's language, or the English source string if it has no
/// translation or [`init`] wasn't called.
pub fn tr(msg: Msg) -> &'static str {
    TRANSLATIONS
        .get()
        .and_then(|translations| translations.get(msg.msgid()))
        .unwrap_or(msg.msgid())
}

/// The directory of the catalogs: `$SWAY_THINGS_LOCALEDIR`, else the build's default.
fn locale_dir(var: &impl Fn(&str) -> Option<String>) -> PathBuf {
    PathBuf::from(
        var(LOCALEDIR_VAR)
            .filter(|dir| !dir.is_empty())
            .unwrap_or_else(|| DEFAULT_LOCALEDIR.to_owned()),
    )
}

/// The languages to translate into, most wanted first, as gettext picks them: the locale of
/// messages (`LC_ALL`, else `LC_MESSAGES`, else `LANG`), or the list in `LANGUAGE` if that is set.
/// None in the C locale (also `C.UTF-8`), which means English; there `LANGUAGE` is ignored too.
pub fn languages(var: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let Some(locale) = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| var(name).filter(|value| !value.is_empty()))
    else {
        return Vec::new();
    };
    if is_c(&locale) {
        return Vec::new();
    }
    let list = var("LANGUAGE").unwrap_or_default();
    let mut languages: Vec<String> = list
        .split(':')
        .filter(|language| !language.is_empty() && !is_c(language))
        .map(str::to_owned)
        .collect();
    if languages.is_empty() {
        languages.push(locale);
    }
    languages
}

/// Whether `locale` is the C locale, in which nothing is translated.
fn is_c(locale: &str) -> bool {
    let name = locale.split(['.', '@']).next().unwrap_or(locale);
    name == "C" || name == "POSIX"
}

/// The names a catalog for `language` (`ll[_CC][.codeset][@modifier]`) may be found under, most
/// specific first, in the order gettext tries them: the modifier counts most, then the territory,
/// then the codeset.
fn variants(language: &str) -> Vec<String> {
    let (rest, modifier) = match language.split_once('@') {
        Some((rest, modifier)) => (rest, Some(modifier)),
        None => (language, None),
    };
    let (rest, codeset) = match rest.split_once('.') {
        Some((rest, codeset)) => (rest, Some(codeset)),
        None => (rest, None),
    };
    let (base, territory) = match rest.split_once('_') {
        Some((base, territory)) => (base, Some(territory)),
        None => (rest, None),
    };
    let mut variants = Vec::new();
    for modifier in [modifier, None] {
        for territory in [territory, None] {
            for codeset in [codeset, None] {
                let mut name = base.to_owned();
                if let Some(territory) = territory {
                    name.push('_');
                    name.push_str(territory);
                }
                if let Some(codeset) = codeset {
                    name.push('.');
                    name.push_str(codeset);
                }
                if let Some(modifier) = modifier {
                    name.push('@');
                    name.push_str(modifier);
                }
                if !variants.contains(&name) {
                    variants.push(name);
                }
            }
        }
    }
    variants
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use super::{DOMAIN, Msg, Translations, languages, tr, variants, write_mo};

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn install(dir: &Path, language: &str, messages: &[(&str, &str)]) {
        let messages_dir = dir.join(language).join("LC_MESSAGES");
        std::fs::create_dir_all(&messages_dir).unwrap();
        std::fs::write(
            messages_dir.join(format!("{DOMAIN}.mo")),
            write_mo(messages),
        )
        .unwrap();
    }

    #[test]
    fn languages_follow_the_locale_of_messages() {
        assert_eq!(languages(env(&[("LANG", "de_DE.UTF-8")])), ["de_DE.UTF-8"]);
        assert_eq!(
            languages(env(&[("LANG", "de_DE.UTF-8"), ("LC_MESSAGES", "fr_FR")])),
            ["fr_FR"]
        );
        assert_eq!(
            languages(env(&[("LC_ALL", "it_IT"), ("LC_MESSAGES", "fr_FR")])),
            ["it_IT"]
        );
        assert_eq!(languages(env(&[])), [] as [&str; 0]);
    }

    #[test]
    fn language_lists_come_first_except_in_the_c_locale() {
        let var = env(&[("LANG", "en_US.UTF-8"), ("LANGUAGE", "de:fr::C")]);
        assert_eq!(languages(var), ["de", "fr"]);
        for locale in ["C", "POSIX", "C.UTF-8"] {
            let var = env(&[("LC_ALL", locale), ("LANGUAGE", "de")]);
            assert_eq!(languages(var), [] as [&str; 0]);
        }
    }

    #[test]
    fn variants_drop_the_least_important_part_first() {
        assert_eq!(
            variants("de_DE.UTF-8@euro"),
            [
                "de_DE.UTF-8@euro",
                "de_DE@euro",
                "de.UTF-8@euro",
                "de@euro",
                "de_DE.UTF-8",
                "de_DE",
                "de.UTF-8",
                "de"
            ]
        );
        assert_eq!(variants("pt_BR"), ["pt_BR", "pt"]);
        assert_eq!(variants("de"), ["de"]);
    }

    #[test]
    fn translations_fall_back_through_the_languages() {
        let dir = tempfile::tempdir().unwrap();
        install(
            dir.path(),
            "de",
            &[("Keyboard", "Tastatur"), ("Bar", "Leiste")],
        );
        install(dir.path(), "fr_FR", &[("Keyboard", "Clavier")]);
        let languages = ["fr_FR.UTF-8".to_owned(), "de_AT".to_owned()];
        let translations = Translations::load(dir.path(), DOMAIN, &languages);
        assert_eq!(translations.get("Keyboard"), Some("Clavier"));
        assert_eq!(translations.get("Bar"), Some("Leiste"));
        assert_eq!(translations.get("Displays"), None);
    }

    #[test]
    fn the_locale_dir_comes_from_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "xx", &[("Keyboard", "[Kéýƀöåŕđ]")]);
        let path = dir.path().to_str().unwrap();
        let translations =
            Translations::from_env(env(&[("SWAY_THINGS_LOCALEDIR", path), ("LANG", "xx")]));
        assert_eq!(translations.get("Keyboard"), Some("[Kéýƀöåŕđ]"));
        let english =
            Translations::from_env(env(&[("SWAY_THINGS_LOCALEDIR", path), ("LANG", "C")]));
        assert_eq!(english.get("Keyboard"), None);
    }

    #[test]
    fn broken_catalogs_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let messages_dir = dir.path().join("de").join("LC_MESSAGES");
        std::fs::create_dir_all(&messages_dir).unwrap();
        std::fs::write(messages_dir.join(format!("{DOMAIN}.mo")), b"broken").unwrap();
        let translations = Translations::load(dir.path(), DOMAIN, &["de".to_owned()]);
        assert_eq!(translations.get("Keyboard"), None);
    }

    #[test]
    fn untranslated_messages_are_english() {
        // The tests never call `init`.
        assert_eq!(tr(Msg::new("Keyboard")), "Keyboard");
    }
}
