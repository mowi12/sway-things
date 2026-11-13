// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line of `sway-things-gui`: at most one link, a page id or the key of a setting.
//!
//! `GApplication` handles its own options (`--help`, `--gapplication-service`) before the
//! arguments get here; what is left is the link.

use std::fmt;

use sway_things_core::registry::{Link, LinkError, Registry};

/// Why the arguments of `sway-things-gui` can't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentError {
    /// The argument is no page id and no key of a setting.
    Link(LinkError),
    /// More than one argument.
    TooMany,
}

impl fmt::Display for ArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Link(error) => error.fmt(f),
            Self::TooMany => {
                f.write_str("one link at most: sway-things-gui [<page id or setting key>]")
            }
        }
    }
}

impl std::error::Error for ArgumentError {}

/// The link the arguments after the program name ask for: none without arguments, the parsed
/// link for one.
///
/// # Errors
///
/// [`ArgumentError::Link`] for an argument that is no link, [`ArgumentError::TooMany`] for more
/// than one argument.
pub fn link_argument(arguments: &[String]) -> Result<Option<Link<'static>>, ArgumentError> {
    match arguments {
        [] => Ok(None),
        [text] => Link::parse(text, Registry::builtin())
            .map(Some)
            .map_err(ArgumentError::Link),
        _ => Err(ArgumentError::TooMany),
    }
}

/// The text of `link`, as [`link_argument`] reads it: the key of its setting, else its page id.
pub fn link_text(link: &Link<'_>) -> &'static str {
    link.setting()
        .map_or_else(|| link.page().id(), |setting| setting.key())
}

#[cfg(test)]
mod tests {
    use sway_things_core::registry::{LinkError, Page};

    use super::{ArgumentError, link_argument, link_text};

    fn arguments(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_owned()).collect()
    }

    #[test]
    fn no_argument_is_no_link() {
        assert!(link_argument(&[]).unwrap().is_none());
    }

    #[test]
    fn a_page_id_links_to_the_page() {
        let link = link_argument(&arguments(&["keyboard"])).unwrap().unwrap();
        assert_eq!(link.page(), Page::Keyboard);
        assert!(link.setting().is_none());
        assert_eq!(link_text(&link), "keyboard");
    }

    #[test]
    fn a_key_links_to_the_setting() {
        let link = link_argument(&arguments(&["input.keyboard.repeat_rate"]))
            .unwrap()
            .unwrap();
        assert_eq!(link.page(), Page::Keyboard);
        assert_eq!(link_text(&link), "input.keyboard.repeat_rate");
    }

    #[test]
    fn an_unknown_link_is_an_error() {
        let error = link_argument(&arguments(&["nowhere"])).unwrap_err();
        assert!(
            matches!(&error, ArgumentError::Link(LinkError::UnknownPage { id }) if id == "nowhere"),
            "{error:?}"
        );
    }

    #[test]
    fn two_links_are_too_many() {
        let error = link_argument(&arguments(&["keyboard", "mouse-touchpad"])).unwrap_err();
        assert_eq!(error, ArgumentError::TooMany);
        assert_eq!(
            error.to_string(),
            "one link at most: sway-things-gui [<page id or setting key>]"
        );
    }
}
