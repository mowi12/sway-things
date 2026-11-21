// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The definitions of waybar's modules, kept as JSON in `bar.jsonc` next to the settings files.
//!
//! A module's definition is the object waybar reads under its name (`"clock": { "format": … }`).
//! Only definitions changed here are stored, each as a whole; a module without one uses the
//! definition of waybar's default config ([`default_definition`]), or none, which leaves waybar's
//! built-in defaults. Options are kept as they are, so options the page doesn't know pass
//! through unchanged; the same goes for the options of the bar itself that have no setting
//! (`fixed-center`, `name`, …), kept under `bar`.
//!
//! ```text
//! // Sway Things keeps the modules of the bar here. …
//! {
//!   "version": 1,
//!   "modules": { "clock": { "format": "{:%H:%M}" }, "custom/updates": { "exec": "…" } },
//!   "bar": { "fixed-center": false }
//! }
//! ```
//!
//! The file starts with our header; one without it wasn't written by Sway Things and is never
//! written over. Hand edits are fine: the app reads the file again before it changes it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde_json::{Map, Value};

use super::jsonc;
use crate::atomic;
use crate::i18n::Text;
use crate::include::WriteAccess;
use crate::msg;

/// The first line of `bar.jsonc`.
pub const HEADER: &str =
    "Sway Things keeps the modules of the bar here. Change them on the Bar page of Sway Things.";

/// The version of the file's layout.
const VERSION: u64 = 1;

/// The options of a bar that have settings of their own on the page, or that Sway Things
/// resolves itself (`include`); they never pass through.
pub const BAR_SETTINGS: &[&str] = &[
    "layer",
    "position",
    "height",
    "output",
    "exclusive",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "spacing",
    "modules-left",
    "modules-center",
    "modules-right",
    "include",
    // What SIGUSR2 does: Sway Things sends it to reload, so a bar never hides or toggles instead.
    "on-sigusr2",
];

/// waybar's default config, as waybar 0.16 ships it (`resources/config.jsonc`).
pub const WAYBAR_DEFAULT_CONFIG: &str = include_str!("waybar-default/config.jsonc");

/// waybar's default stylesheet, as waybar 0.16 ships it (`resources/style.css`).
pub const WAYBAR_DEFAULT_STYLE: &str = include_str!("waybar-default/style.css");

static DEFAULTS: LazyLock<Map<String, Value>> =
    LazyLock::new(|| match jsonc::parse(WAYBAR_DEFAULT_CONFIG) {
        Ok(Value::Object(config)) => config
            .into_iter()
            .filter(|(_, value)| value.is_object())
            .collect(),
        _ => Map::new(),
    });

/// The definition of `name` in waybar's default config, if it has one.
pub fn default_definition(name: &str) -> Option<&'static Map<String, Value>> {
    DEFAULTS.get(name).and_then(Value::as_object)
}

/// The value of `key` at the top of waybar's default config (`height`, `spacing`, …).
pub fn default_option(key: &str) -> Option<Value> {
    match jsonc::parse(WAYBAR_DEFAULT_CONFIG) {
        Ok(Value::Object(config)) => config.get(key).cloned(),
        _ => None,
    }
}

/// The module definitions and the options of the bar without settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Modules {
    definitions: BTreeMap<String, Map<String, Value>>,
    bar: Map<String, Value>,
}

/// Why `bar.jsonc` can't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModulesError {
    /// It can't be read or written, with the system's error.
    Io(PathBuf, String),
    /// It isn't JSON with comments, or not the layout Sway Things writes.
    Broken(PathBuf, Text),
    /// It doesn't start with our header, so it is left alone.
    Foreign(PathBuf),
}

impl ModulesError {
    /// What is wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Io(path, error) => Text::new(msg!("Can't use {path}: {error}"))
                .text("path", Text::data(path.display().to_string()))
                .text("error", Text::data(error.clone())),
            Self::Broken(path, reason) => Text::new(msg!("Can't read {path}: {reason}"))
                .text("path", Text::data(path.display().to_string()))
                .text("reason", reason.clone()),
            Self::Foreign(path) => Text::new(msg!(
                "{path} wasn't written by Sway Things, so the modules can't be changed"
            ))
            .text("path", Text::data(path.display().to_string())),
        }
    }
}

impl std::fmt::Display for ModulesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for ModulesError {}

impl Modules {
    /// `bar.jsonc` in `config_dir`, the directory of the settings files.
    pub fn path_in(config_dir: &Path) -> PathBuf {
        config_dir.join("bar.jsonc")
    }

    /// The file next to the settings files of `state`.
    pub fn path_of(state: &crate::state::State) -> Option<PathBuf> {
        let shared = state.paths().path(crate::state::Layer::Shared);
        shared.parent().map(Self::path_in)
    }

    /// Reads `path`; a missing file is no definitions.
    ///
    /// # Errors
    ///
    /// [`ModulesError`] if it can't be read or isn't what Sway Things writes.
    pub fn load(path: &Path) -> Result<Self, ModulesError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(ModulesError::Io(path.to_owned(), error.to_string())),
        };
        // A file without our header isn't ours: it is reported, never used.
        if !text.starts_with(&format!("// {HEADER}")) {
            return Err(ModulesError::Foreign(path.to_owned()));
        }
        let broken = |reason: Text| ModulesError::Broken(path.to_owned(), reason);
        let value = jsonc::parse(&text).map_err(|error| broken(error.text()))?;
        let Value::Object(mut file) = value else {
            return Err(broken(msg!("expected an object").into()));
        };
        if file.get("version").and_then(Value::as_u64) != Some(VERSION) {
            return Err(broken(msg!("expected version 1").into()));
        }
        let mut modules = Self::default();
        if let Some(Value::Object(definitions)) = file.remove("modules") {
            for (name, definition) in definitions {
                if let Value::Object(definition) = definition {
                    if super::is_module_name(&name) {
                        modules.definitions.insert(name, definition);
                    }
                }
            }
        }
        if let Some(Value::Object(bar)) = file.remove("bar") {
            modules.set_bar_options(bar);
        }
        Ok(modules)
    }

    /// Writes the definitions to `path`, atomically, unless the file there isn't ours. A file
    /// that would hold nothing isn't created.
    ///
    /// # Errors
    ///
    /// [`ModulesError::Foreign`] if the file there doesn't start with our header,
    /// [`ModulesError::Io`] if it can't be written.
    pub fn save(&self, path: &Path, _access: &WriteAccess) -> Result<(), ModulesError> {
        match std::fs::read_to_string(path) {
            Ok(old) => {
                if !old.starts_with(&format!("// {HEADER}")) {
                    return Err(ModulesError::Foreign(path.to_owned()));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if self == &Self::default() {
                    return Ok(());
                }
            }
            Err(error) => return Err(ModulesError::Io(path.to_owned(), error.to_string())),
        }
        atomic::write(path, &self.text())
            .map_err(|error| ModulesError::Io(path.to_owned(), error.to_string()))
    }

    /// The text of `bar.jsonc`.
    pub fn text(&self) -> String {
        let mut file = Map::new();
        file.insert("version".to_owned(), Value::from(VERSION));
        file.insert(
            "modules".to_owned(),
            Value::Object(
                self.definitions
                    .iter()
                    .map(|(name, definition)| (name.clone(), Value::Object(definition.clone())))
                    .collect(),
            ),
        );
        if !self.bar.is_empty() {
            file.insert("bar".to_owned(), Value::Object(self.bar.clone()));
        }
        jsonc::write(
            &format!(
                "{HEADER}\nHand edits are kept; the app reads the file again before it changes it."
            ),
            &Value::Object(file),
        )
    }

    /// The definition waybar gets for `name`: the one stored here, else the one of waybar's
    /// default config, else none.
    pub fn definition(&self, name: &str) -> Option<Map<String, Value>> {
        self.definitions
            .get(name)
            .cloned()
            .or_else(|| default_definition(name).cloned())
    }

    /// Whether a definition of `name` is stored here.
    pub fn is_stored(&self, name: &str) -> bool {
        self.definitions.contains_key(name)
    }

    /// The names of the stored definitions.
    pub fn stored(&self) -> impl Iterator<Item = &str> {
        self.definitions.keys().map(String::as_str)
    }

    /// Stores `definition` for `name`, as a whole. A name that can't be a module is ignored.
    pub fn set(&mut self, name: &str, definition: Map<String, Value>) {
        if super::is_module_name(name) {
            self.definitions.insert(name.to_owned(), definition);
        }
    }

    /// Removes the stored definition of `name`, so waybar's default applies again.
    pub fn reset(&mut self, name: &str) -> bool {
        self.definitions.remove(name).is_some()
    }

    /// Removes every stored definition and option.
    pub fn clear(&mut self) {
        self.definitions.clear();
        self.bar.clear();
    }

    /// The options of the bar without settings, passed through to waybar.
    pub fn bar_options(&self) -> &Map<String, Value> {
        &self.bar
    }

    /// Replaces the options of the bar without settings; those that have settings
    /// ([`BAR_SETTINGS`]) and module definitions (objects) are left out.
    pub fn set_bar_options(&mut self, options: Map<String, Value>) {
        self.bar = options
            .into_iter()
            .filter(|(key, value)| !BAR_SETTINGS.contains(&key.as_str()) && !value.is_object())
            .collect();
    }

    /// Every module the bar shows with `lists` (left, center, right): those in the lists and,
    /// for each group, its members, each once, in order.
    pub fn shown(&self, lists: &[Vec<String>]) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut order = Vec::new();
        let mut queue: Vec<String> = lists.iter().flatten().cloned().collect();
        queue.reverse();
        while let Some(name) = queue.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if let Some(definition) = self.definition(&name) {
                for member in members(&definition).into_iter().rev() {
                    queue.push(member);
                }
            }
            order.push(name);
        }
        order
    }
}

/// The members of a group's definition (`"modules": [...]`).
pub fn members(definition: &Map<String, Value>) -> Vec<String> {
    definition
        .get("modules")
        .and_then(Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(Value::as_str)
                .filter(|name| super::is_module_name(name))
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => Map::new(),
        }
    }

    #[test]
    fn waybar_s_default_config_gives_the_default_definitions() {
        let clock = default_definition("clock").unwrap();
        assert_eq!(clock["format-alt"], json!("{:%Y-%m-%d}"));
        assert_eq!(default_option("height"), Some(json!(30)));
        assert_eq!(default_option("spacing"), Some(json!(4)));
        // The registry's default lists are those of the file.
        let Value::Object(config) = jsonc::parse(WAYBAR_DEFAULT_CONFIG).unwrap() else {
            panic!("not an object");
        };
        for (key, default) in [
            ("modules-left", crate::registry::bar::DEFAULT_LEFT),
            ("modules-center", crate::registry::bar::DEFAULT_CENTER),
            ("modules-right", crate::registry::bar::DEFAULT_RIGHT),
        ] {
            assert_eq!(config[key], json!(default), "{key}");
        }
    }

    #[test]
    fn stored_definitions_win_and_are_saved_and_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = Modules::path_in(dir.path());
        let mut modules = Modules::load(&path).unwrap();
        assert_eq!(modules, Modules::default());
        let access = WriteAccess::for_tests();
        modules.save(&path, &access).unwrap();
        assert!(!path.exists(), "nothing to save, nothing created");

        modules.set(
            "clock",
            object(json!({"format": "{:%H:%M}", "x-unknown": [1, {"a": null}]})),
        );
        modules.set_bar_options(object(
            json!({"fixed-center": false, "height": 20, "clock": {}}),
        ));
        assert_eq!(
            modules.definition("clock").unwrap()["format"],
            json!("{:%H:%M}")
        );
        assert_eq!(
            modules.bar_options(),
            &object(json!({"fixed-center": false}))
        );
        modules.save(&path, &access).unwrap();
        assert_eq!(Modules::load(&path).unwrap(), modules);

        assert!(modules.reset("clock"));
        assert_eq!(
            modules.definition("clock").unwrap()["format-alt"],
            json!("{:%Y-%m-%d}")
        );

        std::fs::write(&path, "{\"version\": 1}").unwrap();
        assert_eq!(
            modules.save(&path, &access),
            Err(ModulesError::Foreign(path.clone()))
        );
        assert_eq!(
            Modules::load(&path),
            Err(ModulesError::Foreign(path.clone()))
        );
        std::fs::write(&path, format!("// {HEADER}\n{{")).unwrap();
        assert!(matches!(
            Modules::load(&path),
            Err(ModulesError::Broken(..))
        ));
    }

    #[test]
    fn groups_show_their_members() {
        let mut modules = Modules::default();
        modules.set(
            "group/net",
            object(json!({"modules": ["network", "group/inner", "network"]})),
        );
        modules.set("group/inner", object(json!({"modules": ["bluetooth"]})));
        let lists = vec![
            vec!["clock".to_owned()],
            vec![],
            vec!["group/net".to_owned(), "tray".to_owned()],
        ];
        assert_eq!(
            modules.shown(&lists),
            [
                "clock",
                "group/net",
                "network",
                "group/inner",
                "bluetooth",
                "tray"
            ]
        );
    }
}
