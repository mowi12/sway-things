// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Changes that are stored but not in effect yet: sway only reads their directives while it
//! loads its config, at a reload or at the next login.

use crate::registry::{ConfigFile, Value};
use crate::state::State;

/// When a waiting change takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Until {
    /// At the next reload of sway.
    Reload,
    /// At the next login.
    Relogin,
}

/// A stored change that isn't in effect yet.
#[derive(Debug, Clone, PartialEq)]
pub struct WaitingChange {
    /// The key of the setting.
    pub key: &'static str,
    /// The input device the value is for, or `None` for the whole session or input type.
    pub device: Option<String>,
    /// When it takes effect.
    pub until: Until,
    /// The generated file the setting is written to, if it has a sway directive.
    file: Option<ConfigFile>,
    /// The stored value when the change was first noted, which sway still runs with.
    before: Option<Value>,
    /// Whether the change is in the generated files, so a reload of sway reads it.
    written: bool,
}

/// The stored changes that wait for a reload of sway or the next login, in the order they were
/// first made. A change that is undone (its stored value is back at what sway runs with) no
/// longer waits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Waiting {
    changes: Vec<WaitingChange>,
}

impl Waiting {
    /// The changes that wait until `until`.
    pub fn until(&self, until: Until) -> impl Iterator<Item = &WaitingChange> {
        self.changes
            .iter()
            .filter(move |change| change.until == until)
    }

    /// Whether a change waits until `until`.
    pub fn any(&self, until: Until) -> bool {
        self.until(until).next().is_some()
    }

    /// Whether the change of `key` for `device` waits, and until when.
    pub fn get(&self, key: &str, device: Option<&str>) -> Option<Until> {
        self.changes
            .iter()
            .find(|change| change.key == key && change.device.as_deref() == device)
            .map(|change| change.until)
    }

    /// Notes that `key` for `device` changed and waits until `until`; `before` is its stored
    /// value before the change. A change that waits already keeps its first `before`, which is
    /// what sway still runs with.
    pub(super) fn note(
        &mut self,
        key: &'static str,
        device: Option<&str>,
        until: Until,
        file: Option<ConfigFile>,
        before: Option<Value>,
    ) {
        match self
            .changes
            .iter_mut()
            .find(|change| change.key == key && change.device.as_deref() == device)
        {
            Some(change) => change.written = false,
            None => self.changes.push(WaitingChange {
                key,
                device: device.map(str::to_owned),
                until,
                file,
                before,
                written: false,
            }),
        }
    }

    /// Drops the changes whose stored value in `state` is back at what sway runs with.
    pub(super) fn settle(&mut self, state: &State) {
        self.changes.retain(|change| {
            state.get(change.key, change.device.as_deref()) != change.before.as_ref()
        });
    }

    /// Notes that the generated files were written, except `unwritten`.
    pub(super) fn written(&mut self, unwritten: &[ConfigFile]) {
        for change in &mut self.changes {
            if change.file.is_none_or(|file| !unwritten.contains(&file)) {
                change.written = true;
            }
        }
    }

    /// The generated files of the changes that wait for a reload but aren't written yet.
    pub(super) fn unwritten(&self) -> Vec<ConfigFile> {
        let mut files: Vec<ConfigFile> = self
            .until(Until::Reload)
            .filter(|change| !change.written)
            .filter_map(|change| change.file)
            .collect();
        files.sort_unstable();
        files.dedup();
        files
    }

    /// Notes that sway reloaded its config: the changes that wait for it and are in the
    /// generated files are in effect now.
    pub(super) fn reloaded(&mut self) {
        self.changes
            .retain(|change| change.until != Until::Reload || !change.written);
    }
}
