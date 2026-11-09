// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::Target;

/// A window property that criteria can match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CriteriaKey {
    /// `app_id` (Wayland windows).
    AppId,
    /// `class` (Xwayland windows).
    Class,
    /// `instance` (Xwayland windows).
    Instance,
    /// `title`.
    Title,
    /// `shell` (`xdg_shell` or `xwayland`).
    Shell,
    /// `window_role` (Xwayland windows).
    WindowRole,
    /// `con_mark`.
    ConMark,
}

impl CriteriaKey {
    const fn name(self) -> &'static str {
        match self {
            Self::AppId => "app_id",
            Self::Class => "class",
            Self::Instance => "instance",
            Self::Title => "title",
            Self::Shell => "shell",
            Self::WindowRole => "window_role",
            Self::ConMark => "con_mark",
        }
    }
}

/// Window criteria such as `[app_id="^firefox\z"]`. Every value matches the property exactly.
///
/// sway compiles criteria values as PCRE2 regexes. A value is turned into an anchored regex in
/// which every character except ASCII letters, digits and `_` is written as `\x{…}`, so the
/// rendered criteria contain no quote, `$`, `]` or whitespace, whatever the value is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Criteria {
    matches: Vec<(CriteriaKey, String)>,
}

impl Criteria {
    /// Criteria without matches. Add at least one with [`Criteria::exact`] before rendering.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a match: the window's `key` property equals `value` exactly. Any string is allowed.
    #[must_use]
    pub fn exact(mut self, key: CriteriaKey, value: impl Into<String>) -> Self {
        self.matches.push((key, value.into()));
        self
    }

    pub(super) fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// Renders the criteria as one argument. Config lines are unescaped by sway, so their
    /// backslashes are doubled.
    pub(super) fn render(&self, target: Target) -> String {
        let backslash = match target {
            Target::Ipc => "\\",
            Target::Config => "\\\\",
        };
        let mut out = String::from("[");
        for (index, (key, value)) in self.matches.iter().enumerate() {
            if index > 0 {
                out.push(' ');
            }
            out.push_str(key.name());
            out.push_str("=\"^");
            for character in value.chars() {
                if character.is_ascii_alphanumeric() || character == '_' {
                    out.push(character);
                } else {
                    let code = format!("{:x}", u32::from(character));
                    out.push_str(backslash);
                    out.push_str("x{");
                    out.push_str(&code);
                    out.push('}');
                }
            }
            out.push_str(backslash);
            out.push_str("z\"");
        }
        out.push(']');
        out
    }
}
