// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Copying settings to GSettings, for the apps that read it instead of asking sway: GTK apps take
//! their cursor theme and size, their theme, icons, fonts and light or dark style from
//! `org.gnome.desktop.interface`.
//!
//! A setting with a [`GSettingsKey`] is copied after each change ([`copy`]): a stored value is
//! written, and once nothing is stored the key is reset to its schema default. At login and at
//! each reload of sway, `sway-things hook gsettings` writes the stored values again
//! ([`copy_stored`]), so a `settings.toml` synced from another machine reaches GSettings too.
//! That one only writes: a key Sway Things doesn't store stays as the user set it elsewhere.
//! Where nothing is stored for a setting without a sway directive, the app shows the value
//! GSettings has now ([`current`]), so a theme picked with another tool is what the row says.
//!
//! [`Gio`] writes through GIO, so the schema and the backend are those of the desktop (dconf).
//! It looks the schema up first, so a missing schema is an error instead of an abort.
//!
//! ```
//! use sway_things_core::gsettings::{self, MemoryGSettings};
//! use sway_things_core::registry::{Registry, Value};
//!
//! let store = MemoryGSettings::default();
//! let size = Registry::builtin().get("cursor.size").ok_or("no such setting")?;
//! gsettings::copy(&store, size, Some(&Value::Integer(48)))?;
//! assert_eq!(
//!     store.get("org.gnome.desktop.interface", "cursor-size"),
//!     Some(Value::Integer(48))
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use gio::prelude::*;

use crate::i18n::Text;
use crate::msg;
use crate::registry::{GSettingsKey, Kind, Setting, Value};
use crate::state::State;

/// The schema of the keys GTK apps read for their look: cursor, theme, icons, fonts, style.
pub const INTERFACE: &str = "org.gnome.desktop.interface";

/// Where settings are copied to.
pub trait GSettingsStore: fmt::Debug {
    /// Sets `key` to `value`: a text to a string key, a whole number to an integer key.
    ///
    /// # Errors
    ///
    /// A [`GSettingsError`] if the key can't take the value.
    fn set(&self, key: GSettingsKey, value: &Value) -> Result<(), GSettingsError>;

    /// Resets `key` to its schema default.
    ///
    /// # Errors
    ///
    /// A [`GSettingsError`] if the key can't be found or written.
    fn reset(&self, key: GSettingsKey) -> Result<(), GSettingsError>;

    /// The value `key` has now, set by anyone or its schema default: a string as a text, an
    /// integer as a whole number, a double as a decimal. `None` if it can't be read.
    fn value(&self, key: GSettingsKey) -> Option<Value>;

    /// Checks that `key` can take `value`, without writing it.
    ///
    /// # Errors
    ///
    /// The [`GSettingsError`] a write would give: no schema, no key, another type, locked, a
    /// value outside the key's range, or nothing that keeps it.
    fn check(&self, key: GSettingsKey, value: &Value) -> Result<(), GSettingsError>;
}

/// The value GSettings has for `setting` now, read as the setting's type: an enum key's string
/// as a choice. `None` for a setting without a key, or if the store's value doesn't fit the
/// setting.
pub fn current(store: &dyn GSettingsStore, setting: &Setting) -> Option<Value> {
    let value = match (store.value(setting.gsettings()?)?, setting.kind()) {
        (Value::Text(id), Kind::Choice { .. }) => Value::Choice(id),
        (value, _) => value,
    };
    setting.check(&value).is_ok().then_some(value)
}

/// Copies the value of `setting` to its GSettings key: `value` if one is stored, the schema
/// default if not. Nothing for a setting without a key.
///
/// # Errors
///
/// The error of the store.
pub fn copy(
    store: &dyn GSettingsStore,
    setting: &Setting,
    value: Option<&Value>,
) -> Result<(), GSettingsError> {
    let Some(key) = setting.gsettings() else {
        return Ok(());
    };
    match value {
        Some(value) => store.set(key, value),
        None => store.reset(key),
    }
}

/// The keys of the stored settings GSettings can't take, each with why: what `doctor` reports.
pub fn check_stored(
    state: &State,
    store: &dyn GSettingsStore,
) -> Vec<(&'static str, GSettingsError)> {
    state
        .registry()
        .iter()
        .filter_map(|setting| {
            let value = state.get(setting.key(), None)?;
            let error = store.check(setting.gsettings()?, value).err()?;
            Some((setting.key(), error))
        })
        .collect()
}

/// Writes every stored value of a setting with a GSettings key, as `sway-things hook gsettings`
/// does. Keys without a stored value are left alone. Returns the keys that couldn't be written.
pub fn copy_stored(
    state: &State,
    store: &dyn GSettingsStore,
) -> Vec<(&'static str, GSettingsError)> {
    let mut failed = Vec::new();
    for setting in state.registry().iter() {
        if setting.gsettings().is_none() {
            continue;
        }
        if let Some(value) = state.get(setting.key(), None) {
            if let Err(error) = copy(store, setting, Some(value)) {
                failed.push((setting.key(), error));
            }
        }
    }
    failed
}

/// Writes through GIO, to the desktop's GSettings backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct Gio;

impl Gio {
    /// The settings of the schema of `key`, after checking the schema has the key, that it can
    /// be written and that something keeps it, and the key's schema entry.
    fn open(key: GSettingsKey) -> Result<(gio::Settings, gio::SettingsSchemaKey), GSettingsError> {
        let schema = gio::SettingsSchemaSource::default()
            .and_then(|source| source.lookup(key.schema, true))
            .ok_or(GSettingsError::NoSchema { key })?;
        if !schema.has_key(key.key) {
            return Err(GSettingsError::NoKey { key });
        }
        if !Self::keeps_values() {
            return Err(GSettingsError::NotKept { key });
        }
        let settings = gio::Settings::new_full(&schema, gio::SettingsBackend::NONE, None::<&str>);
        if !settings.is_writable(key.key) {
            return Err(GSettingsError::NotWritable { key });
        }
        Ok((settings, schema.key(key.key)))
    }

    /// Whether GSettings keeps what is written. Without dconf, GIO falls back to keeping values
    /// in memory: a write works and is gone when the program ends.
    pub fn keeps_values() -> bool {
        gio::SettingsBackend::default().type_().name() != "GMemorySettingsBackend"
    }

    /// `value` as the variant `key` holds, checked against the key's range (an enum's values).
    fn variant(
        key: GSettingsKey,
        schema_key: &gio::SettingsSchemaKey,
        value: &Value,
    ) -> Result<gio::glib::Variant, GSettingsError> {
        let value_type = schema_key.value_type();
        let variant = match value {
            Value::Text(text) | Value::Choice(text)
                if value_type == *gio::glib::VariantTy::STRING =>
            {
                text.to_variant()
            }
            Value::Integer(number) if value_type == *gio::glib::VariantTy::INT32 => {
                i32::try_from(*number)
                    .map_err(|_| GSettingsError::WrongType { key })?
                    .to_variant()
            }
            Value::Decimal(number) if value_type == *gio::glib::VariantTy::DOUBLE => {
                number.to_variant()
            }
            _ => return Err(GSettingsError::WrongType { key }),
        };
        if !schema_key.range_check(&variant) {
            return Err(GSettingsError::Rejected { key });
        }
        Ok(variant)
    }
}

impl GSettingsStore for Gio {
    fn set(&self, key: GSettingsKey, value: &Value) -> Result<(), GSettingsError> {
        let (settings, schema_key) = Self::open(key)?;
        let variant = Self::variant(key, &schema_key, value)?;
        settings
            .set_value(key.key, &variant)
            .map_err(|_| GSettingsError::Rejected { key })?;
        gio::Settings::sync();
        Ok(())
    }

    fn reset(&self, key: GSettingsKey) -> Result<(), GSettingsError> {
        let (settings, _) = Self::open(key)?;
        settings.reset(key.key);
        gio::Settings::sync();
        Ok(())
    }

    fn check(&self, key: GSettingsKey, value: &Value) -> Result<(), GSettingsError> {
        let (_, schema_key) = Self::open(key)?;
        Self::variant(key, &schema_key, value).map(|_| ())
    }

    fn value(&self, key: GSettingsKey) -> Option<Value> {
        let schema = gio::SettingsSchemaSource::default()?.lookup(key.schema, true)?;
        if !schema.has_key(key.key) {
            return None;
        }
        let settings = gio::Settings::new_full(&schema, gio::SettingsBackend::NONE, None::<&str>);
        let variant = settings.value(key.key);
        variant
            .str()
            .map(|text| Value::Text(text.to_owned()))
            .or_else(|| {
                variant
                    .get::<i32>()
                    .map(|number| Value::Integer(number.into()))
            })
            .or_else(|| variant.get::<f64>().map(Value::Decimal))
    }
}

/// Writes nothing: for an applier that mustn't touch the user's desktop, as in tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoGSettings;

impl GSettingsStore for NoGSettings {
    fn set(&self, _key: GSettingsKey, _value: &Value) -> Result<(), GSettingsError> {
        Ok(())
    }

    fn reset(&self, _key: GSettingsKey) -> Result<(), GSettingsError> {
        Ok(())
    }

    fn value(&self, _key: GSettingsKey) -> Option<Value> {
        None
    }

    fn check(&self, _key: GSettingsKey, _value: &Value) -> Result<(), GSettingsError> {
        Ok(())
    }
}

/// Keeps the values in memory; clones share them. For tests: a reset key is absent.
#[derive(Debug, Clone, Default)]
pub struct MemoryGSettings {
    values: Rc<RefCell<BTreeMap<(String, String), Value>>>,
}

impl MemoryGSettings {
    /// The value of `key` in `schema`, unless it is at its default.
    pub fn get(&self, schema: &str, key: &str) -> Option<Value> {
        self.values
            .borrow()
            .get(&(schema.to_owned(), key.to_owned()))
            .cloned()
    }
}

impl GSettingsStore for MemoryGSettings {
    fn set(&self, key: GSettingsKey, value: &Value) -> Result<(), GSettingsError> {
        // As GSettings keeps them: a choice is the string of an enum key.
        let value = match value {
            Value::Choice(id) => Value::Text(id.clone()),
            Value::Text(_) | Value::Integer(_) | Value::Decimal(_) => value.clone(),
            _ => return Err(GSettingsError::WrongType { key }),
        };
        self.values
            .borrow_mut()
            .insert((key.schema.to_owned(), key.key.to_owned()), value);
        Ok(())
    }

    fn reset(&self, key: GSettingsKey) -> Result<(), GSettingsError> {
        self.values
            .borrow_mut()
            .remove(&(key.schema.to_owned(), key.key.to_owned()));
        Ok(())
    }

    fn value(&self, key: GSettingsKey) -> Option<Value> {
        self.get(key.schema, key.key)
    }

    fn check(&self, _key: GSettingsKey, _value: &Value) -> Result<(), GSettingsError> {
        Ok(())
    }
}

/// Why a value couldn't be copied to GSettings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GSettingsError {
    /// The schema isn't installed (`gsettings-desktop-schemas`).
    NoSchema {
        /// The key.
        key: GSettingsKey,
    },
    /// The schema has no such key.
    NoKey {
        /// The key.
        key: GSettingsKey,
    },
    /// The key holds another type of value.
    WrongType {
        /// The key.
        key: GSettingsKey,
    },
    /// The key is locked down by the system administrator.
    NotWritable {
        /// The key.
        key: GSettingsKey,
    },
    /// The value is outside the key's range, or the backend refused it.
    Rejected {
        /// The key.
        key: GSettingsKey,
    },
    /// Nothing keeps the value: GIO found no backend like dconf and keeps values in memory.
    NotKept {
        /// The key.
        key: GSettingsKey,
    },
}

impl GSettingsError {
    /// The key the value was for.
    pub fn key(&self) -> GSettingsKey {
        match self {
            Self::NoSchema { key }
            | Self::NoKey { key }
            | Self::WrongType { key }
            | Self::NotWritable { key }
            | Self::Rejected { key }
            | Self::NotKept { key } => *key,
        }
    }

    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        let key = self.key();
        let message = match self {
            Self::NoSchema { .. } => msg!(
                "GTK apps keep their own setting: the GSettings schema {schema} isn't installed"
            ),
            Self::NoKey { .. } => {
                msg!("GTK apps keep their own setting: GSettings has no key {key} in {schema}")
            }
            Self::WrongType { .. } => {
                msg!(
                    "GTK apps keep their own setting: GSettings key {key} in {schema} takes another type"
                )
            }
            Self::NotWritable { .. } => {
                msg!("GTK apps keep their own setting: GSettings key {key} in {schema} is locked")
            }
            Self::Rejected { .. } => {
                msg!(
                    "GTK apps keep their own setting: GSettings refused the value of {key} in {schema}"
                )
            }
            Self::NotKept { .. } => {
                msg!("GTK apps keep their own setting: nothing keeps GSettings values (no dconf)")
            }
        };
        Text::new(message)
            .arg("schema", key.schema)
            .arg("key", key.key)
    }
}

impl fmt::Display for GSettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for GSettingsError {}
