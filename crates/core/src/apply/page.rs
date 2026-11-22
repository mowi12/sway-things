// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resetting a page: every stored value of the page goes, except those that can lock the user
//! out, which are only ever changed through the safety net, and the wallpaper collections, which
//! the user made rather than set. The policy is here, so the app's _Reset Page_ and its
//! confirmation say and do the same.
//!
//! What comes back is what the rows then show: the value of the user's own config, sent to the
//! session at once, else sway's default.

use super::{Applier, ApplyError};
use crate::conflict::{Known, Report};
use crate::generate::Validate;
use crate::include::{IncludeStatus, WriteAccess};
use crate::registry::{Apply, Page, Scope, Setting};
use crate::state::State;

/// What a reset of a page does to the stored values.
#[derive(Debug, Clone, Default)]
pub struct PageReset {
    /// The stored values it removes, by key and device.
    pub removed: Vec<(&'static str, Option<String>)>,
    /// The settings it leaves alone because they can lock the user out (risky), each once.
    pub kept: Vec<&'static Setting>,
}

impl PageReset {
    /// What a reset of `page` does with the values stored in `state`.
    pub fn plan(state: &State, page: Page) -> Self {
        let mut plan = Self::default();
        for entry in state.entries() {
            // A collection is the user's own, like a file: it isn't a value that decides instead
            // of their config.
            if entry.setting.page() != page || entry.setting.scope() == Scope::PerCollection {
                continue;
            }
            if entry.setting.apply() == Apply::Risky {
                if !plan
                    .kept
                    .iter()
                    .any(|setting| setting.key() == entry.setting.key())
                {
                    plan.kept.push(entry.setting);
                }
            } else {
                plan.removed
                    .push((entry.setting.key(), entry.device.map(str::to_owned)));
            }
        }
        plan
    }
}

/// What went wrong in a reset of a page; the rest was reset.
#[derive(Debug, Default)]
pub struct PageResetErrors {
    /// The stored values that couldn't be removed, by key and device, with why.
    pub failed: Vec<(&'static str, Option<String>, ApplyError)>,
    /// Why the remembered displays couldn't be forgotten, for the Displays page while kanshi
    /// sets up the displays.
    pub displays: Option<ApplyError>,
}

impl<V: Validate> Applier<V> {
    /// Resets `page` as [`PageReset::plan`] says: removes each stored value that can't lock the
    /// user out. Where a line of the user's config, as `status` read it, sets the value, the
    /// line's values are sent, as [`Applier::let_config_win`] does for all settings of its
    /// directive; else sway's default, as [`Applier::reset`] does. `known` matches the lines with
    /// the devices and outputs. While kanshi sets up the displays, a reset of the Displays page
    /// also forgets the profile of the displays connected ([`Applier::forget_displays`]), so what
    /// set them up before applies again.
    pub async fn reset_page(
        &mut self,
        page: Page,
        status: &IncludeStatus,
        known: Known<'_>,
        access: &WriteAccess,
    ) -> PageResetErrors {
        let mut errors = PageResetErrors::default();
        for (key, device) in PageReset::plan(&self.state, page).removed {
            // Gone with another setting of its directive.
            if self.state.get(key, device.as_deref()).is_none() {
                continue;
            }
            let reset = match self.users_line(key, device.as_deref(), page, status, known) {
                Some(report) => self.let_config_win(&report, access).await,
                None => self.reset(key, device.as_deref(), access).await,
            };
            if let Err(error) = reset {
                errors.failed.push((key, device, error));
            }
        }
        if page == Page::Displays && self.displays.is_kanshi() {
            errors.displays = self.forget_displays(access).await.err();
        }
        errors
    }

    /// The report of `key` for `device` if a line of the user's config sets it and all settings
    /// of its directive can go with it: none is risky, all are on `page`, and kanshi doesn't set
    /// up the display. `None` if the value is reset alone.
    fn users_line(
        &self,
        key: &str,
        device: Option<&str>,
        page: Page,
        status: &IncludeStatus,
        known: Known<'_>,
    ) -> Option<Report> {
        let registry = self.state.registry();
        let setting = registry.get(key)?;
        if self.displays.is_kanshi() && setting.sets_up_display() {
            return None;
        }
        let whole = registry
            .group(setting)
            .all(|member| member.page() == page && member.apply() != Apply::Risky);
        let report = Report::check(status, setting, device, known)?;
        (whole && report.user_value().is_some()).then_some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::PageReset;
    use crate::registry::{Page, Registry};
    use crate::state::{Paths, State};

    #[test]
    fn a_page_reset_removes_every_value_but_those_that_can_lock_the_user_out() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.toml"),
            "[input.keyboard]\nxkb_layout = [\"us\", \"de\"]\nrepeat_delay = 300\nrepeat_rate = 40\n",
        )
        .unwrap();
        let state = State::load(
            Registry::builtin(),
            Paths::new(dir.path(), dir.path(), "host").unwrap(),
        );
        let plan = PageReset::plan(&state, Page::Keyboard);
        let mut removed: Vec<&str> = plan.removed.iter().map(|(key, _)| *key).collect();
        removed.sort_unstable();
        assert_eq!(
            removed,
            ["input.keyboard.repeat_delay", "input.keyboard.repeat_rate"]
        );
        let kept: Vec<&str> = plan.kept.iter().map(|setting| setting.key()).collect();
        assert_eq!(kept, ["input.keyboard.xkb_layout"]);

        // Another page has nothing stored.
        let plan = PageReset::plan(&state, Page::Displays);
        assert!(plan.removed.is_empty() && plan.kept.is_empty());
    }

    #[test]
    fn a_page_reset_keeps_the_wallpaper_collections() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.toml"),
            "[wallpaper]\ncollection = \"Holidays\"\n\n\
             [collection.\"Holidays\"]\npictures = [\"~/Pictures/Holidays\"]\n",
        )
        .unwrap();
        let state = State::load(
            Registry::builtin(),
            Paths::new(dir.path(), dir.path(), "host").unwrap(),
        );
        let plan = PageReset::plan(&state, Page::Wallpaper);
        let removed: Vec<&str> = plan.removed.iter().map(|(key, _)| *key).collect();
        assert_eq!(removed, ["wallpaper.collection"]);
        assert!(plan.kept.is_empty());
    }
}
