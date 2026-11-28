// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings of Region & Time that Sway Things stores. The time zone, automatic time and the
//! clock are timedated's own and not stored ([`crate::region`]); the automatic time zone is
//! stored here and reaches the session through `sway-things hook timezone` in `80-hooks.conf`.
//!
//! The language and the formats are the user's own: they go into Sway Things' environment.d
//! file ([`crate::environment`]) as `LANG` and the `LC_*` variables, which the session gets at
//! the next login. The language also sets every category of formats, so the system's formats
//! (`/etc/locale.conf`) can't win over it; formats set on their own come later in the registry
//! and so replace those.

use super::environment::Variable;
use super::{Apply, Page, Setting, Value, ValueError};
use crate::msg;

/// A locale's name, taken apart.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Locale {
    /// The language, two or three lowercase letters (`de`), or `C` for the C locale.
    pub language: String,
    /// The territory, two uppercase letters or three digits (`DE`), if any.
    pub territory: Option<String>,
    /// The modifier (`latin`), if any.
    pub modifier: Option<String>,
}

impl Locale {
    /// The locale named `name` in any spelling glibc takes (`de_DE.utf8`, `de_DE.UTF-8@euro`,
    /// `eo`), without regard to its codeset; `None` for a name that isn't a locale's. `C` and
    /// `POSIX` aren't languages: only `C.UTF-8` is taken, as the C locale.
    pub fn parse(name: &str) -> Option<Self> {
        let (rest, modifier) = match name.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier)),
            None => (name, None),
        };
        let (base, codeset) = match rest.split_once('.') {
            Some((base, codeset)) => (base, Some(codeset)),
            None => (rest, None),
        };
        if let Some(modifier) = modifier {
            let valid = !modifier.is_empty()
                && modifier.len() <= 16
                && modifier.chars().all(|char| char.is_ascii_alphanumeric());
            if !valid {
                return None;
            }
        }
        if let Some(codeset) = codeset {
            let valid = !codeset.is_empty()
                && codeset.len() <= 16
                && codeset
                    .chars()
                    .all(|char| char.is_ascii_alphanumeric() || char == '-');
            if !valid {
                return None;
            }
        }
        if base == "C" {
            return (codeset.is_some_and(is_utf8) && modifier.is_none()).then(|| Self {
                language: "C".to_owned(),
                territory: None,
                modifier: None,
            });
        }
        let (language, territory) = match base.split_once('_') {
            Some((language, territory)) => (language, Some(territory)),
            None => (base, None),
        };
        let language_ok = (2..=3).contains(&language.len())
            && language.chars().all(|char| char.is_ascii_lowercase());
        let territory_ok = territory.is_none_or(|territory| {
            (territory.len() == 2 && territory.chars().all(|char| char.is_ascii_uppercase()))
                || (territory.len() == 3 && territory.chars().all(|char| char.is_ascii_digit()))
        });
        (language_ok && territory_ok).then(|| Self {
            language: language.to_owned(),
            territory: territory.map(str::to_owned),
            modifier: modifier.map(str::to_owned),
        })
    }

    /// The name Sway Things writes: `ll_CC.UTF-8[@modifier]`.
    pub fn name(&self) -> String {
        let mut name = self.language.clone();
        if let Some(territory) = &self.territory {
            name.push('_');
            name.push_str(territory);
        }
        name.push_str(".UTF-8");
        if let Some(modifier) = &self.modifier {
            name.push('@');
            name.push_str(modifier);
        }
        name
    }
}

/// Whether `name` is a UTF-8 locale's name that Sway Things can write into its environment.d
/// file: a language, maybe a territory, the codeset `UTF-8` (or `utf8`) and maybe a modifier
/// (`de_DE.UTF-8`, `sr_RS.UTF-8@latin`, `eo.UTF-8`, `C.UTF-8`).
pub fn is_locale_name(name: &str) -> bool {
    let codeset = name
        .split('@')
        .next()
        .and_then(|rest| rest.split_once('.'))
        .map(|(_, codeset)| codeset);
    codeset.is_some_and(is_utf8) && Locale::parse(name).is_some()
}

/// Whether `codeset` is UTF-8 in one of its spellings (`UTF-8`, `utf8`).
pub fn is_utf8(codeset: &str) -> bool {
    codeset.replace('-', "").eq_ignore_ascii_case("utf8")
}

/// The key of the automatic time zone: off by default, since it asks geoclue, which asks a
/// location service on the network.
pub const AUTOMATIC_TIMEZONE: &str = "region.automatic_timezone";

/// The key of the language of apps (`LANG`): a UTF-8 locale, empty for the system's.
pub const LANGUAGE: &str = "region.language";

/// The key of the formats (every `LC_*` but `LC_MESSAGES`): a UTF-8 locale, empty to follow the
/// language.
pub const FORMATS: &str = "region.formats";

/// The categories of formats: every one of glibc's but the messages (the language), the
/// character set and the sort order, which follow `LANG`.
pub const FORMAT_VARIABLES: [&str; 9] = [
    "LC_NUMERIC",
    "LC_TIME",
    "LC_MONETARY",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
];

/// A locale Sway Things can write, or empty.
fn check_locale(text: &str) -> Result<(), ValueError> {
    if text.is_empty() || is_locale_name(text) {
        Ok(())
    } else {
        Err(ValueError::NotALocale)
    }
}

/// The locale of `value`, if it is one that can be written.
fn locale(value: &Value) -> Option<&str> {
    match value {
        Value::Text(text) if is_locale_name(text) => Some(text),
        _ => None,
    }
}

/// `LANG`, `LC_MESSAGES` and every category of formats for the language.
fn language(value: &Value) -> Vec<Variable> {
    let Some(locale) = locale(value) else {
        return Vec::new();
    };
    ["LANG", "LC_MESSAGES"]
        .into_iter()
        .chain(FORMAT_VARIABLES)
        .map(|name| Variable::literal(name, locale))
        .collect()
}

/// Every category of formats for the formats.
fn formats(value: &Value) -> Vec<Variable> {
    let Some(locale) = locale(value) else {
        return Vec::new();
    };
    FORMAT_VARIABLES
        .into_iter()
        .map(|name| Variable::literal(name, locale))
        .collect()
}

/// The section of the language and formats.
const LANGUAGE_SECTION: super::Msg = msg!("Language & Formats");

pub(super) fn settings() -> Vec<Setting> {
    vec![
        Setting::toggle(AUTOMATIC_TIMEZONE, Page::RegionTime, false)
            .section(msg!("Date & Time"))
            .title(msg!("Automatic time zone"))
            .subtitle(msg!(
                "Sets the time zone where you are, at each login and while this page is open. \
                 geoclue finds your city through an online service, which sees your IP address \
                 and may see the Wi-Fi networks nearby."
            ))
            .keywords(&[
                msg!("location"),
                msg!("geoclue"),
                msg!("travel"),
                msg!("time zone"),
            ])
            .build(),
        Setting::text(LANGUAGE, Page::RegionTime, "")
            .section(LANGUAGE_SECTION)
            .title(msg!("Language"))
            .subtitle(msg!("The language of apps, for you only"))
            .placeholder(msg!("System default"))
            .apply(Apply::Relogin)
            .check_text(check_locale)
            .environment(&["LANG", "LC_*"], language)
            .keywords(&[
                msg!("LANG"),
                msg!("locale"),
                msg!("translation"),
                msg!("display language"),
            ])
            .build(),
        Setting::text(FORMATS, Page::RegionTime, "")
            .section(LANGUAGE_SECTION)
            .title(msg!("Formats"))
            .subtitle(msg!(
                "Dates, times, numbers, money, paper size and units, for you only"
            ))
            .placeholder(msg!("Same as language"))
            .apply(Apply::Relogin)
            .check_text(check_locale)
            .environment(&FORMAT_VARIABLES, formats)
            .keywords(&[
                msg!("LC_TIME"),
                msg!("LC_NUMERIC"),
                msg!("LC_MONETARY"),
                msg!("LC_PAPER"),
                msg!("date format"),
                msg!("number format"),
                msg!("currency"),
                msg!("paper size"),
                msg!("metric"),
                msg!("imperial"),
                msg!("region"),
            ])
            .build(),
    ]
}
