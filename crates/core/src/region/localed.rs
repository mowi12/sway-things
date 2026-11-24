// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! systemd-localed on the system bus (`org.freedesktop.locale1`): the system's language and
//! formats, for every user, the login screen and the console (`/etc/locale.conf`).
//!
//! Sway Things reads its `Locale` property and sets it with `SetLocale`, which polkit guards
//! (`auth_admin_keep`): the user's polkit agent asks for an administrator's password. When polkit
//! refuses, [`SystemLocale::change`] gives the `localectl` command that does the same in a
//! terminal.

use std::collections::HashMap;

use zbus::Connection;
use zbus::proxy::MethodFlags;
use zbus::zvariant::OwnedValue;

use super::timedate::{call_failure, connect};
use super::{Bus, Change, Error};
use crate::i18n::Text;
use crate::msg;
use crate::region::locales::{Locale, is_locale_name};
use crate::registry::region::FORMAT_VARIABLES;

/// localed's bus name and interface.
const SERVICE: &str = "org.freedesktop.locale1";
/// Its object.
const PATH: &str = "/org/freedesktop/locale1";
/// The standard interface for properties.
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// The system's language and formats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemLocale {
    /// `LANG`, spelled as Sway Things writes a UTF-8 locale; empty if the system sets none.
    pub language: String,
    /// The locale of the formats, if the system sets them apart from the language: the one of
    /// `LC_TIME`, else of the first category of formats it sets.
    pub formats: Option<String>,
    /// Every variable as localed has it (`LANG=de_DE.UTF-8`), cleaned of control characters.
    pub variables: Vec<String>,
}

impl SystemLocale {
    /// The system locale localed reports as `variables` (`["LANG=de_DE.UTF-8", …]`).
    pub fn from_variables(variables: &[String]) -> Self {
        let variables: Vec<String> = variables
            .iter()
            .map(|variable| super::clean(variable))
            .filter(|variable| !variable.is_empty())
            .collect();
        let get = |name: &str| {
            variables.iter().find_map(|variable| {
                variable
                    .split_once('=')
                    .filter(|(key, _)| *key == name)
                    .map(|(_, value)| value.to_owned())
            })
        };
        let language = get("LANG").map(normalize).unwrap_or_default();
        let formats = std::iter::once("LC_TIME")
            .chain(FORMAT_VARIABLES)
            .find_map(get)
            .map(normalize)
            .filter(|formats| *formats != language);
        Self {
            language,
            formats,
            variables,
        }
    }

    /// The variables to set for `language` and `formats` (`None` or the same locale: they follow
    /// the language). `None` if either isn't a locale Sway Things writes.
    pub fn wanted(language: &str, formats: Option<&str>) -> Option<Vec<String>> {
        if !is_locale_name(language) {
            return None;
        }
        let mut variables = vec![format!("LANG={language}")];
        if let Some(formats) = formats.filter(|formats| *formats != language) {
            if !is_locale_name(formats) {
                return None;
            }
            variables.extend(
                FORMAT_VARIABLES
                    .iter()
                    .map(|name| format!("{name}={formats}")),
            );
        }
        Some(variables)
    }

    /// The change that sets `variables`, for the command that does it in a terminal.
    pub fn change(variables: &[String]) -> Change {
        Change::SystemLocale(variables.to_vec())
    }
}

/// `locale` as Sway Things writes it (`de_DE.utf8` is `de_DE.UTF-8`), or as it is if it isn't a
/// UTF-8 locale.
fn normalize(locale: String) -> String {
    if is_locale_name(&locale) {
        Locale::parse(&locale).map_or(locale, |parsed| parsed.name())
    } else {
        locale
    }
}

/// A connection to localed. Cheap to clone; every clone shares the connection.
#[derive(Debug, Clone)]
pub struct Localed {
    connection: Connection,
}

impl Localed {
    /// Connects to `bus`. Succeeds while localed isn't running too: the bus starts it on the
    /// first call.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] without the bus.
    pub async fn connect(bus: &Bus) -> Result<Self, Error> {
        Ok(Self {
            connection: connect(bus, gone).await?,
        })
    }

    /// The system's language and formats.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] without localed, [`Error::Failed`] if it doesn't answer.
    pub async fn locale(&self) -> Result<SystemLocale, Error> {
        let reply = self
            .connection
            .call_method(Some(SERVICE), PATH, Some(PROPERTIES), "GetAll", &(SERVICE,))
            .await
            .map_err(failure)?;
        let properties: HashMap<String, OwnedValue> =
            reply.body().deserialize().map_err(failure)?;
        let variables = properties
            .get("Locale")
            .and_then(|value| Vec::<String>::try_from(value.clone()).ok())
            .unwrap_or_default();
        Ok(SystemLocale::from_variables(&variables))
    }

    /// Sets the system's locale to `variables` (`["LANG=de_DE.UTF-8", …]`, from
    /// [`SystemLocale::wanted`]); a category not among them is unset. polkit may ask the user
    /// for a password first, which the call waits for.
    ///
    /// # Errors
    ///
    /// [`Error::Denied`] if polkit says no, [`Error::Unavailable`] without localed,
    /// [`Error::Failed`] if localed refuses.
    pub async fn set_locale(&self, variables: &[String]) -> Result<(), Error> {
        let proxy = zbus::Proxy::new(&self.connection, SERVICE, PATH, SERVICE)
            .await
            .map_err(failure)?;
        proxy
            .call_with_flags::<_, _, ()>(
                "SetLocale",
                MethodFlags::AllowInteractiveAuth.into(),
                &(variables, true),
            )
            .await
            .map(|_| ())
            .map_err(failure)
    }
}

/// The text for a system without localed.
pub fn gone() -> Text {
    msg!("systemd-localed isn't available").into()
}

/// The error of a failed call.
fn failure(error: zbus::Error) -> Error {
    call_failure(error, gone, |other| {
        Text::new(msg!("systemd-localed failed: {error}")).arg("error", other)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|text| (*text).to_owned()).collect()
    }

    #[test]
    fn the_system_locale_says_language_and_formats() {
        let plain = SystemLocale::from_variables(&strings(&["LANG=de_DE.UTF-8"]));
        assert_eq!(plain.language, "de_DE.UTF-8");
        assert_eq!(plain.formats, None);
        let mixed = SystemLocale::from_variables(&strings(&[
            "LANG=en_US.UTF-8",
            "LC_TIME=de_DE.UTF-8\u{1b}",
            "",
        ]));
        assert_eq!(mixed.formats.as_deref(), Some("de_DE.UTF-8"));
        // Another spelling of the same locale.
        let spelled = SystemLocale::from_variables(&strings(&[
            "LANG=en_US.UTF-8",
            "LC_NUMERIC=de_DE.utf8",
            "LC_TIME=de_DE.utf8",
        ]));
        assert_eq!(spelled.formats.as_deref(), Some("de_DE.UTF-8"));
        assert_eq!(mixed.variables, ["LANG=en_US.UTF-8", "LC_TIME=de_DE.UTF-8"]);
        let same =
            SystemLocale::from_variables(&strings(&["LANG=de_DE.UTF-8", "LC_NUMERIC=de_DE.UTF-8"]));
        assert_eq!(same.formats, None);
        assert_eq!(SystemLocale::from_variables(&[]), SystemLocale::default());
    }

    #[test]
    fn the_wanted_locale_sets_every_format_apart_from_the_language() {
        assert_eq!(
            SystemLocale::wanted("de_DE.UTF-8", None),
            Some(strings(&["LANG=de_DE.UTF-8"]))
        );
        assert_eq!(
            SystemLocale::wanted("de_DE.UTF-8", Some("de_DE.UTF-8")),
            Some(strings(&["LANG=de_DE.UTF-8"]))
        );
        let mixed = SystemLocale::wanted("en_US.UTF-8", Some("de_DE.UTF-8")).unwrap();
        assert_eq!(mixed.len(), 1 + FORMAT_VARIABLES.len());
        assert!(mixed.contains(&"LC_PAPER=de_DE.UTF-8".to_owned()));
        assert!(
            !mixed
                .iter()
                .any(|variable| variable.starts_with("LC_MESSAGES"))
        );
        assert_eq!(SystemLocale::wanted("de_DE", None), None);
        assert_eq!(SystemLocale::wanted("de_DE.UTF-8", Some("x y")), None);
        assert_eq!(
            SystemLocale::change(&strings(&["LANG=de_DE.UTF-8", "LC_TIME=en_GB.UTF-8"])).command(),
            "localectl set-locale LANG=de_DE.UTF-8 LC_TIME=en_GB.UTF-8"
        );
    }
}
