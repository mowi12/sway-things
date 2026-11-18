// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Copying settings to GSettings, for the apps that read it instead of asking sway: GTK apps take
//! their cursor theme and size from `org.gnome.desktop.interface`.
//!
//! A setting with a [`GSettingsKey`] is copied after each change ([`copy`]): a stored value is
//! written, and once nothing is stored the key is reset to its schema default. At login and at
//! each reload of sway, `sway-things hook gsettings` writes the stored values again
//! ([`copy_stored`]), so a `settings.toml` synced from another machine reaches GSettings too.
//! That one only writes: a key Sway Things doesn't store stays as the user set it elsewhere.
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
use crate::registry::{GSettingsKey, Setting, Value};
use crate::state::State;

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
    /// The settings of the schema of `key`, after checking the schema has the key, and the type
    /// the key holds.
    fn open(key: GSettingsKey) -> Result<(gio::Settings, gio::glib::VariantType), GSettingsError> {
        let schema = gio::SettingsSchemaSource::default()
            .and_then(|source| source.lookup(key.schema, true))
            .ok_or(GSettingsError::NoSchema { key })?;
        if !schema.has_key(key.key) {
            return Err(GSettingsError::NoKey { key });
        }
        let value_type = schema.key(key.key).value_type();
        let settings = gio::Settings::new_full(&schema, gio::SettingsBackend::NONE, None::<&str>);
        if !settings.is_writable(key.key) {
            return Err(GSettingsError::NotWritable { key });
        }
        Ok((settings, value_type))
    }
}

impl GSettingsStore for Gio {
    fn set(&self, key: GSettingsKey, value: &Value) -> Result<(), GSettingsError> {
        let (settings, value_type) = Self::open(key)?;
        let variant = match value {
            Value::Text(text) if value_type == *gio::glib::VariantTy::STRING => text.to_variant(),
            Value::Integer(number) if value_type == *gio::glib::VariantTy::INT32 => {
                i32::try_from(*number)
                    .map_err(|_| GSettingsError::WrongType { key })?
                    .to_variant()
            }
            _ => return Err(GSettingsError::WrongType { key }),
        };
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
        if !matches!(value, Value::Text(_) | Value::Integer(_)) {
            return Err(GSettingsError::WrongType { key });
        }
        self.values
            .borrow_mut()
            .insert((key.schema.to_owned(), key.key.to_owned()), value.clone());
        Ok(())
    }

    fn reset(&self, key: GSettingsKey) -> Result<(), GSettingsError> {
        self.values
            .borrow_mut()
            .remove(&(key.schema.to_owned(), key.key.to_owned()));
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
    /// The backend refused the value.
    Rejected {
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
            | Self::Rejected { key } => *key,
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
