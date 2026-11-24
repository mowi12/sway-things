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
use crate::region::locales::is_locale_name;

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
