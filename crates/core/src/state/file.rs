// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One settings file: its TOML document and the values in it that fit the registry.

use std::collections::BTreeMap;

use toml_edit::{Array, DocumentMut, Item, Table, TableLike};

use super::ProblemKind;
use crate::command::Color;
use crate::command::input::InputSelector;
use crate::registry::{Kind, Registry, Scope, Value, ValueError};

/// The schema version this build reads and writes.
pub(super) const VERSION: i64 = 1;

const VERSION_KEY: &str = "version";
const DEVICE_TABLE: &str = "device";

/// The values of one file, by key.
pub(super) type Values = BTreeMap<&'static str, Value>;

/// A settings file. The document keeps the comments, the order and everything Sway Things doesn't
/// know, so a save only changes the lines of the settings that changed.
#[derive(Debug, Clone)]
pub(super) struct SettingsFile {
    /// `None` if the text couldn't be read as a file of this version. Such a file is never
    /// written.
    document: Option<DocumentMut>,
    /// Values for the whole session or an input type.
    values: Values,
    /// Values for one input device, by its identifier.
    devices: BTreeMap<String, Values>,
    problems: Vec<ProblemKind>,
    dirty: bool,
}

impl SettingsFile {
    /// A file that doesn't exist yet.
    pub(super) fn empty() -> Self {
        Self {
            document: Some(DocumentMut::new()),
            values: Values::new(),
            devices: BTreeMap::new(),
            problems: Vec::new(),
            dirty: false,
        }
    }

    /// A file that can't be used, e.g. because it can't be read.
    pub(super) fn unusable(problem: ProblemKind) -> Self {
        Self {
            document: None,
            problems: vec![problem],
            ..Self::empty()
        }
    }

    /// Reads `text`. Whatever doesn't fit the registry becomes a problem and stays in the
    /// document; the rest loads.
    pub(super) fn parse(text: &str, registry: &'static Registry) -> Self {
        let document = match text.parse::<DocumentMut>() {
            Ok(document) => document,
            Err(error) => {
                return Self::unusable(ProblemKind::Syntax {
                    message: error.to_string(),
                });
            }
        };
        match document.get(VERSION_KEY) {
            // A hand-written file may leave the version out; a save adds it.
            None => {}
            Some(item) if item.as_integer() == Some(VERSION) => {}
            Some(item) => {
                return Self::unusable(ProblemKind::UnsupportedVersion {
                    found: item.to_string().trim().to_owned(),
                });
            }
        }
        let mut file = Self::empty();
        for (name, item) in document.iter() {
            match name {
                VERSION_KEY => {}
                DEVICE_TABLE => file.read_devices(item, registry),
                _ => file.read(name.to_owned(), item, None, registry),
            }
        }
        file.document = Some(document);
        file
    }

    fn read_devices(&mut self, item: &Item, registry: &'static Registry) {
        let Some(devices) = item.as_table_like() else {
            self.problems.push(ProblemKind::UnknownKey {
                key: DEVICE_TABLE.to_owned(),
                device: None,
            });
            return;
        };
        for (device, item) in devices.iter() {
            match (InputSelector::device(device), item.as_table_like()) {
                (Ok(_), Some(table)) => {
                    for (name, item) in table.iter() {
                        self.read(name.to_owned(), item, Some(device), registry);
                    }
                }
                _ => self.problems.push(ProblemKind::InvalidDevice {
                    device: device.to_owned(),
                }),
            }
        }
    }

    /// Reads the item at the path `key`: a setting, or a table of settings.
    fn read(
        &mut self,
        key: String,
        item: &Item,
        device: Option<&str>,
        registry: &'static Registry,
    ) {
        let device_name = || device.map(ToOwned::to_owned);
        if let Some(setting) = registry.get(&key) {
            let key = setting.key();
            if device.is_some() && !matches!(setting.scope(), Scope::PerDevice(_)) {
                self.problems.push(ProblemKind::NotPerDevice {
                    key,
                    device: device_name().unwrap_or_default(),
                });
                return;
            }
            let value = from_toml(setting.kind(), item).and_then(|value| {
                setting.check(&value)?;
                Ok(value)
            });
            match value {
                Ok(value) => {
                    let values = match device {
                        Some(device) => self.devices.entry(device.to_owned()).or_default(),
                        None => &mut self.values,
                    };
                    values.insert(key, value);
                }
                Err(error) => self.problems.push(ProblemKind::InvalidValue {
                    key,
                    device: device_name(),
                    error,
                }),
            }
        } else if let Some(table) = item.as_table_like().filter(|table| !table.is_empty()) {
            for (name, item) in table.iter() {
                self.read(format!("{key}.{name}"), item, device, registry);
            }
        } else {
            self.problems.push(ProblemKind::UnknownKey {
                key,
                device: device_name(),
            });
        }
    }

    pub(super) fn get(&self, key: &str, device: Option<&str>) -> Option<&Value> {
        match device {
            Some(device) => self.devices.get(device)?.get(key),
            None => self.values.get(key),
        }
    }

    /// The values for the whole session or an input type.
    pub(super) fn values(&self) -> &Values {
        &self.values
    }

    /// The values for single devices, by identifier.
    pub(super) fn devices(&self) -> &BTreeMap<String, Values> {
        &self.devices
    }

    pub(super) fn problems(&self) -> &[ProblemKind] {
        &self.problems
    }

    /// Whether the file could be read. Only such a file is ever written.
    pub(super) fn is_usable(&self) -> bool {
        self.document.is_some()
    }

    /// Marks the file as different from the one on disk.
    pub(super) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub(super) fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// The text to write.
    pub(super) fn text(&self) -> Option<String> {
        self.document.as_ref().map(ToString::to_string)
    }

    /// Stores a checked value. An invalid value that was at this place is replaced, so its
    /// problem goes away.
    pub(super) fn set(
        &mut self,
        key: &'static str,
        device: Option<&str>,
        value: Value,
    ) -> Result<(), FileError> {
        let document = self.document.as_mut().ok_or(FileError::Unusable)?;
        if !document.contains_key(VERSION_KEY) {
            document.insert(VERSION_KEY, toml_edit::value(VERSION));
            place_version(document);
        }
        let path = path(key, device);
        let (leaf, tables) = path.split_last().ok_or(FileError::NotATable)?;
        let mut table: &mut dyn TableLike = document.as_table_mut();
        for name in tables {
            table = table
                .entry(name)
                .or_insert_with(|| {
                    // No header of its own: `[input.touchpad]`, not `[input]` before it.
                    let mut table = Table::new();
                    table.set_implicit(true);
                    Item::Table(table)
                })
                .as_table_like_mut()
                .ok_or(FileError::NotATable)?;
        }
        let mut new = to_toml(&value);
        // Keep a comment at the end of the line.
        if let Some(Item::Value(old)) = table.get(leaf) {
            *new.decor_mut() = old.decor().clone();
        }
        table.insert(leaf, Item::Value(new));
        let values = match device {
            Some(device) => self.devices.entry(device.to_owned()).or_default(),
            None => &mut self.values,
        };
        values.insert(key, value);
        self.forget_problems(key, device);
        self.dirty = true;
        Ok(())
    }

    /// Removes the setting, whether its value is valid or not. Returns whether there was one.
    pub(super) fn remove(&mut self, key: &'static str, device: Option<&str>) -> bool {
        let Some(document) = self.document.as_mut() else {
            return false;
        };
        let path = path(key, device);
        if !remove_at(document.as_table_mut(), &path) {
            return false;
        }
        match device {
            Some(device) => {
                if let Some(values) = self.devices.get_mut(device) {
                    values.remove(key);
                    if values.is_empty() {
                        self.devices.remove(device);
                    }
                }
            }
            None => {
                self.values.remove(key);
            }
        }
        self.forget_problems(key, device);
        self.dirty = true;
        true
    }

    fn forget_problems(&mut self, key: &str, device: Option<&str>) {
        self.problems.retain(|problem| {
            !matches!(
                problem,
                ProblemKind::InvalidValue { key: problem_key, device: problem_device, .. }
                    if *problem_key == key && problem_device.as_deref() == device
            )
        });
    }
}

/// Why a value can't be stored in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FileError {
    /// The file couldn't be read, so it isn't written either.
    Unusable,
    /// Something that isn't a table is where a table of the key's path must be.
    NotATable,
}

/// The path of a setting in the document: the key's segments, for one device below
/// `device."<identifier>"`.
fn path<'a>(key: &'a str, device: Option<&'a str>) -> Vec<&'a str> {
    device
        .into_iter()
        .flat_map(|device| [DEVICE_TABLE, device])
        .chain(key.split('.'))
        .collect()
}

/// Makes room for the version line in a document that had none: it goes above the first table
/// header, separated by an empty line. A comment block at the top of the file that an empty line
/// separates from the first header describes the file, not the table, so it stays at the top.
fn place_version(document: &mut DocumentMut) {
    fn first(table: &Table) -> Option<isize> {
        table
            .iter()
            .filter_map(|(_, item)| item.as_table())
            .filter_map(|table| {
                let own = table.position().filter(|_| !table.is_implicit());
                own.into_iter().chain(first(table)).min()
            })
            .min()
    }
    /// Returns the comment block that belongs to the file.
    fn separate(table: &mut Table, position: isize) -> Option<String> {
        for (_, item) in table.iter_mut() {
            let Some(table) = item.as_table_mut() else {
                continue;
            };
            if table.position() != Some(position) || table.is_implicit() {
                if let Some(header) = separate(table, position) {
                    return Some(header);
                }
                continue;
            }
            let prefix = table
                .decor()
                .prefix()
                .and_then(|prefix| prefix.as_str())
                .unwrap_or_default()
                .to_owned();
            if prefix.starts_with('\n') {
                return None;
            }
            return match prefix.split_once("\n\n") {
                Some((header, rest)) if header.starts_with('#') => {
                    table.decor_mut().set_prefix(format!("\n{rest}"));
                    Some(format!("{header}\n"))
                }
                _ => {
                    table.decor_mut().set_prefix(format!("\n{prefix}"));
                    None
                }
            };
        }
        None
    }
    let header = first(document).and_then(|position| separate(document, position));
    if let (Some(header), Some(mut key)) = (header, document.key_mut(VERSION_KEY)) {
        key.leaf_decor_mut().set_prefix(header);
    }
}

/// Removes the item at `path` and every table that is empty afterwards.
fn remove_at(table: &mut dyn TableLike, path: &[&str]) -> bool {
    match path {
        [] => false,
        [leaf] => table.remove(leaf).is_some(),
        [name, rest @ ..] => {
            let Some(child) = table.get_mut(name).and_then(Item::as_table_like_mut) else {
                return false;
            };
            let removed = remove_at(child, rest);
            if removed && child.is_empty() {
                table.remove(name);
            }
            removed
        }
    }
}

fn from_toml(kind: &Kind, item: &Item) -> Result<Value, ValueError> {
    let wrong_type = || ValueError::WrongType {
        expected: kind.type_name(),
        found: item.type_name(),
    };
    Ok(match kind {
        Kind::Bool { .. } => Value::Bool(item.as_bool().ok_or_else(wrong_type)?),
        Kind::Integer { .. } => Value::Integer(item.as_integer().ok_or_else(wrong_type)?),
        Kind::Decimal { .. } => Value::Decimal(
            item.as_float()
                // `1` is as good as `1.0`. Whole numbers of this size are exact as a float.
                .or_else(|| {
                    item.as_integer()
                        .and_then(|integer| i32::try_from(integer).ok())
                        .map(f64::from)
                })
                .ok_or_else(wrong_type)?,
        ),
        Kind::Choice { .. } => Value::Choice(item.as_str().ok_or_else(wrong_type)?.to_owned()),
        Kind::Color { .. } => Value::Color(
            item.as_str()
                .ok_or_else(wrong_type)?
                .parse::<Color>()
                .map_err(|_| ValueError::NotAColor)?,
        ),
        Kind::Text { .. } => Value::Text(item.as_str().ok_or_else(wrong_type)?.to_owned()),
        Kind::List { .. } => Value::List(
            item.as_array()
                .ok_or_else(wrong_type)?
                .iter()
                .map(|element| element.as_str().map(ToOwned::to_owned))
                .collect::<Option<_>>()
                .ok_or_else(wrong_type)?,
        ),
    })
}

fn to_toml(value: &Value) -> toml_edit::Value {
    match value {
        Value::Bool(value) => (*value).into(),
        Value::Integer(value) => (*value).into(),
        Value::Decimal(value) => (*value).into(),
        Value::Choice(value) | Value::Text(value) => value.as_str().into(),
        Value::Color(value) => value.to_string().into(),
        Value::List(values) => values.iter().map(String::as_str).collect::<Array>().into(),
    }
}
