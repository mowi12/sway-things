// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The keyboard models, layouts, variants and XKB options of the system, with their
//! descriptions, from the XKB rules registry of xkeyboard-config. Sway Things only reads it.
//!
//! [`XkbRules::load`] reads `rules/evdev.xml` in the XKB config root (`$XKB_CONFIG_ROOT`,
//! otherwise `/usr/share/X11/xkb`), the rules sway uses, and merges `rules/evdev.extras.xml`
//! into it, with every entry from there marked as [extra](Item::extra). A missing or broken
//! rules file is a [`RulesError`] and leaves the lists empty, so names typed by hand are the
//! only way left. The files are read as data: their size is bounded, entity declarations are
//! refused, nothing is fetched or run, unknown elements are skipped and so are entries without
//! a name.
//!
//! The descriptions are the English source strings of xkeyboard-config.
//!
//! ```
//! use sway_things_core::sway_config::MemoryFiles;
//! use sway_things_core::xkb::XkbRules;
//!
//! let files = MemoryFiles::new().file(
//!     "/usr/share/X11/xkb/rules/evdev.xml",
//!     r#"<?xml version="1.0"?>
//! <!DOCTYPE xkbConfigRegistry SYSTEM "xkb.dtd">
//! <xkbConfigRegistry version="1.1">
//!   <layoutList>
//!     <layout>
//!       <configItem><name>de</name><description>German</description></configItem>
//!       <variantList>
//!         <variant>
//!           <configItem><name>nodeadkeys</name><description>German (no dead keys)</description></configItem>
//!         </variant>
//!       </variantList>
//!     </layout>
//!   </layoutList>
//!   <optionList>
//!     <group allowMultipleSelection="true">
//!       <configItem><name>grp</name><description>Switching to another layout</description></configItem>
//!       <option>
//!         <configItem><name>grp:alt_shift_toggle</name><description>Alt+Shift</description></configItem>
//!       </option>
//!     </group>
//!   </optionList>
//! </xkbConfigRegistry>"#,
//! );
//! let (rules, errors) = XkbRules::load(&files);
//!
//! assert!(errors.is_empty());
//! assert_eq!(rules.layout("de").map(|layout| layout.item.label()), Some("German"));
//! assert_eq!(
//!     rules.variant("de", "nodeadkeys").map(|variant| variant.label()),
//!     Some("German (no dead keys)")
//! );
//! assert_eq!(rules.options_of("grp")[0].name, "grp:alt_shift_toggle");
//! ```

mod parse;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::sway_config::Files;

pub use parse::parse;

/// The XKB config root when `$XKB_CONFIG_ROOT` isn't set.
pub const DEFAULT_ROOT: &str = "/usr/share/X11/xkb";

/// The rules sway uses when the config doesn't name others.
pub const RULES: &str = "evdev";

/// The largest rules file read, in bytes. xkeyboard-config's `evdev.xml` has about 250 KiB.
pub const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;

/// One entry of the registry: a model, layout, variant, option group or option.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    /// The name sway takes: `pc105`, `de`, `nodeadkeys`, `grp`, `grp:alt_shift_toggle`. An
    /// external string: it goes into commands only through the command builders.
    pub name: String,
    /// What it is, in English: `German (no dead keys)`.
    pub description: Option<String>,
    /// The short label of a layout or variant, as status bars show it: `de`.
    pub short_description: Option<String>,
    /// The maker of a keyboard model.
    pub vendor: Option<String>,
    /// The ISO 639 codes of the languages a layout or variant is for (`ger`, `eng`).
    pub languages: Vec<String>,
    /// The ISO 3166 codes of the countries a layout or variant is used in (`DE`).
    pub countries: Vec<String>,
    /// Whether the entry is exotic: it comes from `evdev.extras.xml`, or the registry marks it
    /// so. Pages show these only on request.
    pub extra: bool,
}

impl Item {
    /// The description to show, or the name if there is none.
    pub fn label(&self) -> &str {
        self.description.as_deref().unwrap_or(&self.name)
    }
}

/// A keyboard layout and its variants.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    /// The layout itself.
    pub item: Item,
    /// Its variants, in the order of the registry.
    pub variants: Vec<Item>,
}

impl Layout {
    /// The variant called `name`.
    pub fn variant(&self, name: &str) -> Option<&Item> {
        self.variants.iter().find(|variant| variant.name == name)
    }
}

/// A group of XKB options, such as `grp` for the shortcut that switches layouts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptionGroup {
    /// The group itself.
    pub item: Item,
    /// Whether more than one option of the group can be set at a time.
    pub multiple: bool,
    /// Its options, in the order of the registry.
    pub options: Vec<Item>,
}

/// The models, layouts and options of the XKB rules registry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XkbRules {
    /// The keyboard models.
    pub models: Vec<Item>,
    /// The layouts, each with its variants.
    pub layouts: Vec<Layout>,
    /// The option groups, each with its options.
    pub option_groups: Vec<OptionGroup>,
}

impl XkbRules {
    /// Reads the rules registry of the system through `files`: `evdev.xml` in
    /// [`rules_dir`], and `evdev.extras.xml` merged into it if it exists.
    ///
    /// Each file that can't be used is in the errors. Without the base file the lists are
    /// empty; a broken extras file leaves the base file's entries.
    pub fn load(files: &impl Files) -> (Self, Vec<RulesError>) {
        let directory = rules_dir(files);
        let mut errors = Vec::new();
        let base = directory.join(format!("{RULES}.xml"));
        let mut rules = match read(files, &base, false) {
            Ok(rules) => rules,
            Err(kind) => {
                errors.push(RulesError { path: base, kind });
                return (Self::default(), errors);
            }
        };
        let extras = directory.join(format!("{RULES}.extras.xml"));
        match read(files, &extras, true) {
            Ok(more) => rules.merge(more),
            Err(RulesErrorKind::Missing) => {}
            Err(kind) => errors.push(RulesError { path: extras, kind }),
        }
        (rules, errors)
    }

    /// Adds the entries of `other` that aren't here yet. Variants of a layout that is here
    /// already go into that layout, options of a known group into that group.
    pub fn merge(&mut self, other: Self) {
        let mut builder = Builder::default();
        builder.add(std::mem::take(self));
        builder.add(other);
        *self = builder.rules;
    }

    /// The keyboard model called `name`.
    pub fn model(&self, name: &str) -> Option<&Item> {
        self.models.iter().find(|model| model.name == name)
    }

    /// The layout called `name`.
    pub fn layout(&self, name: &str) -> Option<&Layout> {
        self.layouts.iter().find(|layout| layout.item.name == name)
    }

    /// The variant `variant` of the layout `layout`.
    pub fn variant(&self, layout: &str, variant: &str) -> Option<&Item> {
        self.layout(layout)?.variant(variant)
    }

    /// The option group called `name`.
    pub fn option_group(&self, name: &str) -> Option<&OptionGroup> {
        self.option_groups
            .iter()
            .find(|group| group.item.name == name)
    }

    /// The options of the group `group`, such as `grp`; none if there is no such group.
    pub fn options_of(&self, group: &str) -> &[Item] {
        self.option_group(group)
            .map_or(&[], |group| group.options.as_slice())
    }

    /// The option called `name`, in whichever group it is.
    pub fn option(&self, name: &str) -> Option<&Item> {
        self.option_groups
            .iter()
            .flat_map(|group| &group.options)
            .find(|option| option.name == name)
    }
}

/// The directory with the rules files: `rules` in `$XKB_CONFIG_ROOT`, or in [`DEFAULT_ROOT`]
/// if that isn't set.
pub fn rules_dir(files: &impl Files) -> PathBuf {
    files
        .env("XKB_CONFIG_ROOT")
        .filter(|root| !root.is_empty())
        .map_or_else(|| PathBuf::from(DEFAULT_ROOT), PathBuf::from)
        .join("rules")
}

fn read(files: &impl Files, path: &Path, extra: bool) -> Result<XkbRules, RulesErrorKind> {
    let content = files
        .read_bounded(path, MAX_FILE_SIZE)
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => RulesErrorKind::Missing,
            kind => RulesErrorKind::Unreadable(kind),
        })?;
    parse(&content, extra)
}

/// A rules file that couldn't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesError {
    /// The file.
    pub path: PathBuf,
    /// What is wrong with it.
    pub kind: RulesErrorKind,
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.kind)
    }
}

impl std::error::Error for RulesError {}

/// What is wrong with a rules file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesErrorKind {
    /// The file doesn't exist.
    Missing,
    /// The file can't be read.
    Unreadable(io::ErrorKind),
    /// The file is larger than [`MAX_FILE_SIZE`].
    TooLarge,
    /// The file isn't UTF-8.
    NotUtf8,
    /// The file declares XML entities, which a rules registry has no use for.
    EntityDeclaration,
    /// The file isn't well-formed XML; the message of the XML parser.
    Xml(String),
    /// The file is XML, but not an XKB rules registry.
    NotRegistry,
}

impl fmt::Display for RulesErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("not found"),
            Self::Unreadable(kind) => write!(f, "can't be read: {kind}"),
            Self::TooLarge => write!(f, "larger than {MAX_FILE_SIZE} bytes"),
            Self::NotUtf8 => f.write_str("not UTF-8"),
            Self::EntityDeclaration => f.write_str("declares XML entities"),
            Self::Xml(message) => write!(f, "not well-formed XML: {message}"),
            Self::NotRegistry => f.write_str("not an XKB rules registry"),
        }
    }
}

impl std::error::Error for RulesErrorKind {}

/// Collects entries, each name once per list. A later entry with a known name adds its
/// variants or options to the known one and is otherwise dropped.
#[derive(Default)]
struct Builder {
    rules: XkbRules,
    models: HashSet<String>,
    layouts: HashMap<String, usize>,
    variants: HashSet<(usize, String)>,
    groups: HashMap<String, usize>,
    options: HashSet<(usize, String)>,
}

impl Builder {
    fn add(&mut self, rules: XkbRules) {
        for model in rules.models {
            self.model(model);
        }
        for layout in rules.layouts {
            let index = self.layout(layout.item);
            for variant in layout.variants {
                self.variant(index, variant);
            }
        }
        for group in rules.option_groups {
            let index = self.group(group.item, group.multiple);
            for option in group.options {
                self.option(index, option);
            }
        }
    }

    fn model(&mut self, item: Item) {
        if self.models.insert(item.name.clone()) {
            self.rules.models.push(item);
        }
    }

    fn layout(&mut self, item: Item) -> usize {
        if let Some(&index) = self.layouts.get(&item.name) {
            return index;
        }
        let index = self.rules.layouts.len();
        self.layouts.insert(item.name.clone(), index);
        self.rules.layouts.push(Layout {
            item,
            variants: Vec::new(),
        });
        index
    }

    fn variant(&mut self, layout: usize, item: Item) {
        if let Some(target) = self.rules.layouts.get_mut(layout) {
            if self.variants.insert((layout, item.name.clone())) {
                target.variants.push(item);
            }
        }
    }

    fn group(&mut self, item: Item, multiple: bool) -> usize {
        if let Some(&index) = self.groups.get(&item.name) {
            return index;
        }
        let index = self.rules.option_groups.len();
        self.groups.insert(item.name.clone(), index);
        self.rules.option_groups.push(OptionGroup {
            item,
            multiple,
            options: Vec::new(),
        });
        index
    }

    fn option(&mut self, group: usize, item: Item) {
        if let Some(target) = self.rules.option_groups.get_mut(group) {
            if self.options.insert((group, item.name.clone())) {
                target.options.push(item);
            }
        }
    }
}
