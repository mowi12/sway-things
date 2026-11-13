// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings registry: every setting is defined once, here.
//!
//! A [`Setting`] has a key, a value type with default and limits, translatable texts, a scope, an
//! apply mode, requirements, and the sway directive it maps to. The rows of the app, search, the
//! command line keys, the checks on `settings.toml`, the generated config and conflict detection
//! all read these definitions.
//!
//! ```
//! use sway_things_core::command::Target;
//! use sway_things_core::registry::{Defaults, Registry, Value};
//! use std::collections::BTreeMap;
//!
//! let registry = Registry::builtin();
//! let tap = registry.get("input.touchpad.tap").ok_or("no such setting")?;
//! assert_eq!(tap.default_value(), Value::Bool(false));
//!
//! let mut values = BTreeMap::new();
//! values.insert("input.touchpad.tap".to_owned(), Value::Bool(true));
//! let commands = registry.render(tap, &values, None, Target::Config)?;
//! assert_eq!(commands[0].render(Target::Config)?, "input type:touchpad tap enabled");
//!
//! // Settings of one directive render together; values that aren't stored use the default.
//! let accent = registry.get("windows.focused.child_border").ok_or("no such setting")?;
//! let commands = registry.render(accent, &Defaults, None, Target::Config)?;
//! assert_eq!(
//!     commands[0].render(Target::Config)?,
//!     "client.focused #4c7899 #285577 #ffffff #2e9ef4 #285577"
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod app;
mod behavior;
mod effects;
mod input;
mod link;
mod page;
mod read;
mod search;
mod setting;
mod sway;
#[cfg(test)]
mod tests;
mod value;
mod windows;

use std::sync::LazyLock;

use crate::command::input::InputSelector;
use crate::command::{SwayCmd, Target};

pub use link::{Link, LinkError};
pub use page::{Group, Page};
pub use read::{ReadError, ReadFn};
pub use search::{Field, Hit, SearchIndex};
pub use setting::{
    Apply, Dependency, Msg, Requirements, Scope, Setting, SettingBuilder, SettingGroup, Slider,
    TextCheck, When,
};
pub use sway::{
    Block, ConfigFile, Context, Defaults, Directive, RenderError, RenderFn, SwayMapping, Values,
};
pub use value::{Choice, Kind, Value, ValueError};

/// All settings Sway Things knows.
#[derive(Debug, Clone)]
pub struct Registry {
    settings: Vec<Setting>,
}

static BUILTIN: LazyLock<Registry> = LazyLock::new(|| {
    let mut settings = Vec::new();
    settings.extend(input::settings());
    settings.extend(windows::settings());
    settings.extend(effects::settings());
    settings.extend(behavior::settings());
    settings.extend(app::settings());
    Registry { settings }
});

impl Registry {
    /// The settings of Sway Things.
    pub fn builtin() -> &'static Self {
        &BUILTIN
    }

    /// The setting with this key.
    pub fn get(&self, key: &str) -> Option<&Setting> {
        self.settings.iter().find(|setting| setting.key() == key)
    }

    /// Every setting, in the order of the pages and their rows.
    pub fn iter(&self) -> impl Iterator<Item = &Setting> {
        self.settings.iter()
    }

    /// The settings that are values of the same sway directive as `setting`, including `setting`
    /// itself: one command sets all of them, so they render, conflict and get stored together.
    /// An `input` option is a directive of its own for each input type (`events` of keyboards and
    /// of touchpads), so settings of another type aren't in the group. A preference of Sway Things
    /// itself is alone in its group.
    ///
    /// The app shows a group of more than one setting as one row with the members below it: each
    /// member names the same [`SettingGroup`], with its id and title ([`Setting::group`]).
    pub fn group<'a>(&'a self, setting: &'a Setting) -> impl Iterator<Item = &'a Setting> {
        let directive = setting.sway().map(SwayMapping::directive);
        self.settings.iter().filter(move |other| match directive {
            Some(directive) => {
                other.sway().map(SwayMapping::directive) == Some(directive)
                    && other.scope() == setting.scope()
            }
            None => other.key() == setting.key(),
        })
    }

    /// Builds the sway commands that set `setting` and the other settings of its
    /// [group](Registry::group) to their values in `values`, or to their defaults where nothing is
    /// stored.
    ///
    /// `device` selects one input device for a setting with [`Scope::PerDevice`]; `None` renders
    /// for every device of the setting's type. `values` then holds the values for that device.
    ///
    /// For [`Target::Ipc`] the commands change the running session, for [`Target::Config`] they
    /// are the lines of the generated file; render each command for the same target. The two can
    /// differ: gaps need one more command to reach workspaces that are already open.
    ///
    /// # Errors
    ///
    /// [`RenderError::NotASwaySetting`] for a preference of Sway Things itself,
    /// [`RenderError::NotPerDevice`] if `device` is given for another scope,
    /// [`RenderError::InvalidValue`] for a stored value that doesn't fit its setting, and the
    /// error of a command builder that rejects a value.
    pub fn render(
        &self,
        setting: &Setting,
        values: &dyn Values,
        device: Option<&InputSelector>,
        target: Target,
    ) -> Result<Vec<SwayCmd>, RenderError> {
        let Some(mapping) = setting.sway() else {
            return Err(RenderError::NotASwaySetting { key: setting.key() });
        };
        if device.is_some() && !matches!(setting.scope(), Scope::PerDevice(_)) {
            return Err(RenderError::NotPerDevice);
        }
        (mapping.render)(&Context {
            registry: self,
            values,
            scope: setting.scope(),
            device,
            target,
        })
    }

    /// The line of the generated file that sets `setting` to its value in `values`, for showing
    /// it in the app: what [`Registry::render`] builds for the config, without the
    /// `input <selector>` an option of an `input` block starts with (`pointer_accel 0.3`).
    ///
    /// # Errors
    ///
    /// Like [`Registry::render`], and [`RenderError::Command`] if a command can't be written as a
    /// config line.
    pub fn config_line(
        &self,
        setting: &Setting,
        values: &dyn Values,
        device: Option<&InputSelector>,
    ) -> Result<String, RenderError> {
        let skip = match setting.sway().map(|mapping| mapping.directive().block) {
            Some(Block::Input) => 2,
            Some(Block::Top) | None => 0,
        };
        let lines = self
            .render(setting, values, device, Target::Config)?
            .iter()
            .map(|command| command.render_from(Target::Config, skip))
            .collect::<Result<Vec<_>, _>>()
            .map_err(RenderError::Command)?;
        Ok(lines.join("\n"))
    }
}
