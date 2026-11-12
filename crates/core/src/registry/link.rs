// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deep links: what `sway-things open <link>` and `sway-things-gui <link>` open.
//!
//! A link is a page id (`keyboard`) or the key of a setting (`input.keyboard.repeat_rate`), which
//! opens the setting's page at its row. Keys contain dots, page ids don't.

use std::fmt;

use super::{Page, Registry, Setting};

/// A page of the app, and maybe a setting on it.
#[derive(Debug, Clone, Copy)]
pub struct Link<'a> {
    page: Page,
    setting: Option<&'a Setting>,
}

impl<'a> Link<'a> {
    /// The link `text` stands for: a page id or the key of a setting of `registry`.
    ///
    /// # Errors
    ///
    /// [`LinkError::UnknownPage`] for text without a dot that is no page id,
    /// [`LinkError::UnknownSetting`] for text with a dot that is no key.
    pub fn parse(text: &str, registry: &'a Registry) -> Result<Self, LinkError> {
        if !text.contains('.') {
            return Page::from_id(text)
                .map(|page| Self {
                    page,
                    setting: None,
                })
                .ok_or_else(|| LinkError::UnknownPage {
                    id: text.to_owned(),
                });
        }
        if let Some(setting) = registry.get(text) {
            return Ok(Self {
                page: setting.page(),
                setting: Some(setting),
            });
        }
        Err(unknown_setting(text, registry))
    }

    /// The page to open.
    pub fn page(&self) -> Page {
        self.page
    }

    /// The setting to show on the page, if the link names one.
    pub fn setting(&self) -> Option<&'a Setting> {
        self.setting
    }
}

/// Why a link can't be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// There is no page with this id.
    UnknownPage {
        /// The id that was asked for.
        id: String,
    },
    /// There is no setting with this key.
    UnknownSetting {
        /// The key that was asked for.
        key: String,
        /// The longest start of `key`, up to a dot, that keys of settings share, e.g.
        /// `input.keyboard.`; empty if not even the first part fits.
        prefix: String,
        /// The keys that start with `prefix`, or the first parts of all keys (`input.`) if
        /// `prefix` is empty.
        valid: Vec<String>,
    },
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPage { id } => {
                let ids: Vec<&str> = Page::ALL.iter().map(|page| page.id()).collect();
                write!(
                    f,
                    "unknown page {id:?}; the pages are {}, and a setting is opened by its key \
                     (input.keyboard.repeat_rate)",
                    ids.join(", ")
                )
            }
            Self::UnknownSetting { key, prefix, valid } if prefix.is_empty() => write!(
                f,
                "unknown setting {key:?}; the keys of settings start with {}",
                valid.join(", ")
            ),
            Self::UnknownSetting { key, prefix, valid } => write!(
                f,
                "unknown setting {key:?}; the settings that start with {prefix} are {}",
                valid.join(", ")
            ),
        }
    }
}

impl std::error::Error for LinkError {}

/// The error for `key`, which no setting has, with the keys it comes closest to.
fn unknown_setting(key: &str, registry: &Registry) -> LinkError {
    let mut prefix = String::new();
    for part in key.split_inclusive('.').filter(|part| part.ends_with('.')) {
        let longer = format!("{prefix}{part}");
        if registry
            .iter()
            .any(|setting| setting.key().starts_with(&longer))
        {
            prefix = longer;
        } else {
            break;
        }
    }
    let mut valid: Vec<String> = if prefix.is_empty() {
        registry
            .iter()
            .filter_map(|setting| {
                let (first, _) = setting.key().split_once('.')?;
                Some(format!("{first}."))
            })
            .collect()
    } else {
        registry
            .iter()
            .map(Setting::key)
            .filter(|other| other.starts_with(&prefix))
            .map(str::to_owned)
            .collect()
    };
    valid.sort();
    valid.dedup();
    LinkError::UnknownSetting {
        key: key.to_owned(),
        prefix,
        valid,
    }
}

#[cfg(test)]
mod tests {
    use super::{Link, LinkError};
    use crate::registry::{Page, Registry, Setting};

    #[test]
    fn a_page_id_opens_the_page() {
        let registry = Registry::builtin();
        for page in Page::ALL {
            let link = Link::parse(page.id(), registry).unwrap();
            assert_eq!(link.page(), page);
            assert!(link.setting().is_none());
        }
    }

    #[test]
    fn a_key_opens_the_page_of_the_setting() {
        let registry = Registry::builtin();
        let link = Link::parse("input.keyboard.repeat_rate", registry).unwrap();
        assert_eq!(link.page(), Page::Keyboard);
        assert_eq!(
            link.setting().map(Setting::key),
            Some("input.keyboard.repeat_rate")
        );
        for setting in registry.iter() {
            let link = Link::parse(setting.key(), registry).unwrap();
            assert_eq!(link.page(), setting.page());
            assert_eq!(link.setting().map(Setting::key), Some(setting.key()));
        }
    }

    #[test]
    fn an_unknown_page_names_the_pages() {
        let registry = Registry::builtin();
        for text in ["keyboards", "", "Keyboard", "keyboard/repeat-rate"] {
            let error = Link::parse(text, registry).unwrap_err();
            assert_eq!(
                error,
                LinkError::UnknownPage {
                    id: text.to_owned()
                }
            );
            let message = error.to_string();
            assert!(message.starts_with(&format!("unknown page {text:?}; the pages are ")));
            for page in Page::ALL {
                assert!(message.contains(page.id()), "{message}");
            }
        }
    }

    #[test]
    fn an_unknown_setting_names_the_keys_it_comes_closest_to() {
        let registry = Registry::builtin();
        let error = Link::parse("input.keyboard.repeat_speed", registry).unwrap_err();
        let LinkError::UnknownSetting { key, prefix, valid } = &error else {
            panic!("{error:?}");
        };
        assert_eq!(key, "input.keyboard.repeat_speed");
        assert_eq!(prefix, "input.keyboard.");
        assert!(valid.iter().any(|key| key == "input.keyboard.repeat_rate"));
        assert!(valid.iter().all(|key| key.starts_with("input.keyboard.")));
        assert!(
            error
                .to_string()
                .starts_with("unknown setting \"input.keyboard.repeat_speed\"; the settings that start with input.keyboard. are "),
            "{error}"
        );

        let error = Link::parse("input.keyboard", registry).unwrap_err();
        let LinkError::UnknownSetting { prefix, valid, .. } = &error else {
            panic!("{error:?}");
        };
        assert_eq!(prefix, "input.");
        assert!(valid.iter().any(|key| key == "input.touchpad.tap"));

        let error = Link::parse("display.scale", registry).unwrap_err();
        let LinkError::UnknownSetting { prefix, valid, .. } = &error else {
            panic!("{error:?}");
        };
        assert_eq!(prefix, "");
        assert!(valid.contains(&"input.".to_owned()), "{valid:?}");
        assert!(valid.contains(&"windows.".to_owned()), "{valid:?}");
        assert!(
            error
                .to_string()
                .starts_with("unknown setting \"display.scale\"; the keys of settings start with "),
            "{error}"
        );
    }
}
