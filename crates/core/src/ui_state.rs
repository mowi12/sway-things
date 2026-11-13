// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! State of the app's window that isn't a setting: which sections of which pages are collapsed.
//!
//! It is stored per machine in `ui.toml` in the state directory (`$XDG_STATE_HOME/sway-things`),
//! never in the settings files, so a dotfiles repository with those files stays free of it:
//!
//! ```toml
//! version = 1
//!
//! [collapsed]
//! mouse-touchpad = ["mouse", "test-area"]
//! ```
//!
//! A page is named by its id ([`Page::id`]), a section by [`section_id`] of its English title.
//! A missing or broken file means every section is expanded, and ids no page or section has
//! (any more) are ignored.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::atomic;
use crate::registry::Page;

const VERSION: i64 = 1;

/// The id of the section titled `title` (its English source title): lowercase letters and
/// digits, with a dash between words. `Focused tab title` is `focused-tab-title`.
pub fn section_id(title: &str) -> String {
    let mut id = String::with_capacity(title.len());
    for word in title
        .split(|char: char| !char.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        if !id.is_empty() {
            id.push('-');
        }
        id.push_str(&word.to_ascii_lowercase());
    }
    id
}

/// The collapsed sections, by page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiState {
    path: PathBuf,
    /// By page id, so the file lists the pages in a stable order.
    collapsed: BTreeMap<&'static str, BTreeSet<String>>,
}

impl UiState {
    /// Every section expanded, to be saved at `path`.
    pub fn empty(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            collapsed: BTreeMap::new(),
        }
    }

    /// The state in the file at `path`. A file that is missing, can't be read or isn't a state
    /// of this version leaves every section expanded; pages that don't exist are skipped.
    pub fn load(path: &Path) -> Self {
        let mut state = Self::empty(path);
        let Some(document) = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.parse::<DocumentMut>().ok())
        else {
            return state;
        };
        if document.get("version").and_then(Item::as_integer) != Some(VERSION) {
            return state;
        }
        let Some(collapsed) = document.get("collapsed").and_then(Item::as_table) else {
            return state;
        };
        for (page, sections) in collapsed {
            let (Some(page), Some(sections)) = (Page::from_id(page), sections.as_array()) else {
                continue;
            };
            let sections: BTreeSet<String> = sections
                .iter()
                .filter_map(|section| section.as_str())
                .map(str::to_owned)
                .collect();
            if !sections.is_empty() {
                state.collapsed.insert(page.id(), sections);
            }
        }
        state
    }

    /// Where the state is saved.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the section `section` (a [`section_id`]) of `page` is collapsed.
    pub fn is_collapsed(&self, page: Page, section: &str) -> bool {
        self.collapsed
            .get(page.id())
            .is_some_and(|sections| sections.contains(section))
    }

    /// Collapses or expands the section `section` of `page`. Returns whether that changed
    /// anything.
    pub fn set_collapsed(&mut self, page: Page, section: &str, collapsed: bool) -> bool {
        if collapsed {
            self.collapsed
                .entry(page.id())
                .or_default()
                .insert(section.to_owned())
        } else {
            let Some(sections) = self.collapsed.get_mut(page.id()) else {
                return false;
            };
            let removed = sections.remove(section);
            if sections.is_empty() {
                self.collapsed.remove(page.id());
            }
            removed
        }
    }

    /// The text of the file.
    pub fn text(&self) -> String {
        let mut document = DocumentMut::new();
        document.insert("version", value(VERSION));
        let mut collapsed = Table::new();
        for (page, sections) in &self.collapsed {
            let sections: Array = sections.iter().map(String::as_str).collect();
            collapsed.insert(page, value(sections));
        }
        document.insert("collapsed", Item::Table(collapsed));
        document.to_string()
    }

    /// Writes the file atomically. It holds no setting, so it is written also while Sway Things
    /// is read-only.
    ///
    /// # Errors
    ///
    /// The I/O error if the file can't be written.
    pub fn save(&self) -> io::Result<()> {
        atomic::write(&self.path, &self.text())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{UiState, section_id};
    use crate::registry::Page;

    #[test]
    fn section_ids_are_dashed_lowercase_words() {
        assert_eq!(section_id("Focused tab title"), "focused-tab-title");
        assert_eq!(section_id("X11 apps"), "x11-apps");
        assert_eq!(section_id("Mouse"), "mouse");
        assert_eq!(section_id("Sway Things"), "sway-things");
        assert_eq!(section_id("  Odd -- spacing "), "odd-spacing");
    }

    #[test]
    fn collapsed_sections_are_saved_and_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/ui.toml");
        let mut state = UiState::load(&path);
        assert!(!state.is_collapsed(Page::MouseTouchpad, "mouse"));
        assert!(state.set_collapsed(Page::MouseTouchpad, "mouse", true));
        assert!(!state.set_collapsed(Page::MouseTouchpad, "mouse", true));
        assert!(state.set_collapsed(Page::Windows, "gaps", true));
        state.save().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "version = 1\n\n[collapsed]\nmouse-touchpad = [\"mouse\"]\nwindows = [\"gaps\"]\n"
        );

        let mut loaded = UiState::load(&path);
        assert_eq!(loaded, state);
        assert!(loaded.is_collapsed(Page::Windows, "gaps"));
        assert!(loaded.set_collapsed(Page::Windows, "gaps", false));
        assert!(!loaded.set_collapsed(Page::Windows, "gaps", false));
        assert!(!loaded.is_collapsed(Page::Windows, "gaps"));
        loaded.save().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "version = 1\n\n[collapsed]\nmouse-touchpad = [\"mouse\"]\n"
        );
        // No temporary file is left next to it.
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn a_missing_or_broken_file_expands_everything() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui.toml");
        assert_eq!(UiState::load(&path), UiState::empty(&path));
        for text in [
            "not toml [",
            "version = 2\n[collapsed]\nwindows = [\"gaps\"]\n",
            "[collapsed]\nwindows = [\"gaps\"]\n",
            "version = 1\ncollapsed = 3\n",
        ] {
            fs::write(&path, text).unwrap();
            assert_eq!(UiState::load(&path), UiState::empty(&path), "{text}");
        }
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui.toml");
        fs::write(
            &path,
            "version = 1\n[collapsed]\nno-such-page = [\"gaps\"]\nwindows = [\"no-such-section\", 7, \"gaps\"]\nkeyboard = \"layouts\"\n",
        )
        .unwrap();
        let state = UiState::load(&path);
        assert!(state.is_collapsed(Page::Windows, "gaps"));
        assert!(!state.is_collapsed(Page::Keyboard, "layouts"));
        assert!(!state.text().contains("no-such-page"));
    }
}
