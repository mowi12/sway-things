// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the displays are set up with for each combination of displays that were connected
//! together: the profiles kanshi applies whenever that combination comes back.
//!
//! The user never names or picks a profile. Whenever the displays are set up on the Displays
//! page, what they run with is remembered for the combination that is connected
//! ([`Profiles::remember`]), and [`kanshi_config`] turns every combination into a kanshi
//! profile. They are stored in `displays.toml` next to the settings files:
//!
//! ```toml
//! version = 1
//!
//! [[profile]]
//!
//! [profile.output."Dell Inc. DELL U2723QE ABC"]
//! enabled = true
//! mode = "2560x1440@59.951Hz"
//! position = "0,0"
//! scale = 1.5
//! transform = "normal"
//!
//! [profile.output.eDP-1]
//! enabled = false
//! ```
//!
//! A display is named as in the settings (make, model and serial, or the connector for one
//! without a serial; [`Output::identifier`]). A combination is the set of those names.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item, Table, value};

use super::mirror::{Mirror, Scaling};
use super::{ENABLED, MODE, Output, Outputs, POSITION, SCALE, TRANSFORM};
use crate::atomic;
use crate::command::output::{Mode, Transform, parse_position};
use crate::registry::Value;
use crate::state::State;

const VERSION: i64 = 1;

/// The gap in logical pixels between the displays and one that shows another display's picture
/// ([`Profile::apart`]).
pub const APART: i32 = 2000;

/// The width of a display set up as `setup` in logical pixels: its mode turned as it is, divided
/// by its scale. `None` without a mode.
fn logical_width(setup: &Setup) -> Option<i32> {
    let mode = setup.mode?;
    let turned = matches!(
        setup.transform,
        Some(Transform::R90 | Transform::R270 | Transform::Flipped90 | Transform::Flipped270)
    );
    let pixels = if turned { mode.height() } else { mode.width() };
    let scale = setup.scale.filter(|scale| *scale > 0.0).unwrap_or(1.0);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a mode's width divided by a positive scale fits easily"
    )]
    Some((f64::from(pixels) / scale).ceil() as i32)
}

/// The first line of `displays.toml`: a file without it wasn't written by Sway Things and is
/// never written over.
const HEADER: &str = "# Written by Sway Things: the displays remembered per combination.";

/// How one display of a combination is set up.
#[derive(Debug, Clone, PartialEq)]
pub struct Setup {
    /// Whether it is on.
    pub enabled: bool,
    /// Its mode, if it is on and sway reported one.
    pub mode: Option<Mode>,
    /// Its top left corner in the layout, if it is on.
    pub position: Option<(i32, i32)>,
    /// Its scale, if it is on.
    pub scale: Option<f64>,
    /// Its rotation, if it is on.
    pub transform: Option<Transform>,
    /// The display it shows instead of its own content ([`super::mirror`]).
    pub mirror: Option<Mirror>,
}

impl Setup {
    /// Off.
    const OFF: Self = Self {
        enabled: false,
        mode: None,
        position: None,
        scale: None,
        transform: None,
        mirror: None,
    };

    /// How `output` runs now.
    fn of(output: &Output) -> Self {
        if !output.is_enabled() {
            return Self::OFF;
        }
        Self {
            enabled: true,
            mode: output.mode(),
            position: output.rect().map(|rect| (rect.x, rect.y)),
            scale: output.scale(),
            transform: output.transform(),
            mirror: None,
        }
    }

    /// How `output` is set up by what `state` stores for it, with what it runs with now where
    /// nothing (or nothing readable) is stored.
    fn stored(output: &Output, state: &State) -> Self {
        let identifier = output.identifier();
        let value = |key: &str| {
            state
                .get(key, Some(identifier))
                .cloned()
                .or_else(|| output.session_value(key))
        };
        let enabled = match value(ENABLED) {
            Some(Value::Bool(enabled)) => enabled,
            _ => output.is_enabled(),
        };
        if !enabled {
            return Self::OFF;
        }
        let text = |key: &str| match value(key) {
            Some(Value::Text(text) | Value::Choice(text)) => Some(text),
            _ => None,
        };
        Self {
            enabled: true,
            mode: text(MODE)
                .and_then(|text| Mode::parse(&text).ok())
                .or_else(|| output.mode()),
            position: text(POSITION)
                .and_then(|text| parse_position(&text).ok())
                .or_else(|| output.rect().map(|rect| (rect.x, rect.y))),
            scale: match value(SCALE) {
                Some(Value::Decimal(scale)) => Some(scale),
                _ => output.scale(),
            },
            transform: text(TRANSFORM)
                .and_then(|name| Transform::from_name(&name))
                .or_else(|| output.transform()),
            mirror: None,
        }
    }

    /// The values of the Displays settings that set a display up like this, by key: whether it
    /// is on, and for one that is on, what is known of its mode, position, scale and rotation.
    pub fn values(&self) -> Vec<(&'static str, Value)> {
        let mut values = vec![(ENABLED, Value::Bool(self.enabled))];
        if !self.enabled {
            return values;
        }
        if let Some(mode) = self.mode {
            values.push((MODE, Value::Text(mode.to_string())));
        }
        if let Some((x, y)) = self.position {
            values.push((POSITION, Value::Text(format!("{x},{y}"))));
        }
        if let Some(scale) = self.scale {
            values.push((SCALE, Value::Decimal(scale)));
        }
        if let Some(transform) = self.transform {
            values.push((TRANSFORM, Value::Choice(transform.name().to_owned())));
        }
        values
    }
}

/// How every display of one combination is set up, by the display's name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Profile {
    outputs: BTreeMap<String, Setup>,
    /// Which workspace goes on which display, by name ([`super::workspaces`]); empty for none.
    workspaces: BTreeMap<String, String>,
}

impl Profile {
    /// How the connected outputs of `outputs` run now. `None` if none is connected.
    pub fn capture(outputs: &Outputs) -> Option<Self> {
        let connected: BTreeMap<String, Setup> = outputs
            .iter()
            .filter(|output| output.is_connected())
            .map(|output| (output.identifier().to_owned(), Setup::of(output)))
            .collect();
        // A combination with every display off is never remembered: kanshi would leave all
        // screens dark with it.
        let any_on = connected.values().any(|setup| setup.enabled);
        (any_on).then_some(Self {
            outputs: connected,
            workspaces: BTreeMap::new(),
        })
    }

    /// How the connected outputs of `outputs` are set up by what `state` stores for them, with
    /// what they run with now where nothing is stored. `None` if none is connected, or if every
    /// one would be off.
    pub fn stored(outputs: &Outputs, state: &State) -> Option<Self> {
        let connected: BTreeMap<String, Setup> = outputs
            .iter()
            .filter(|output| output.is_connected())
            .map(|output| (output.identifier().to_owned(), Setup::stored(output, state)))
            .collect();
        let any_on = connected.values().any(|setup| setup.enabled);
        any_on.then_some(Self {
            outputs: connected,
            workspaces: BTreeMap::new(),
        })
    }

    /// The displays `names` set up by what `state` stores for them, for a combination that
    /// isn't connected any more (nothing runs to fill in what isn't stored). `None` if every
    /// one would be off.
    pub fn stored_for(names: &[String], state: &State) -> Option<Self> {
        let outputs: BTreeMap<String, Setup> = names
            .iter()
            .map(|name| {
                (
                    name.clone(),
                    Setup::stored(&Output::disconnected(name), state),
                )
            })
            .collect();
        let any_on = outputs.values().any(|setup| setup.enabled);
        any_on.then_some(Self {
            outputs,
            workspaces: BTreeMap::new(),
        })
    }

    /// The names of its displays, sorted: what makes the combination.
    pub fn combination(&self) -> Vec<&str> {
        self.outputs.keys().map(String::as_str).collect()
    }

    /// Each display's name and setup, sorted by name.
    pub fn outputs(&self) -> impl Iterator<Item = (&str, &Setup)> {
        self.outputs
            .iter()
            .map(|(name, setup)| (name.as_str(), setup))
    }

    /// Which workspace goes on which display.
    pub fn workspaces(&self) -> &BTreeMap<String, String> {
        &self.workspaces
    }

    /// The placement of the workspaces of this combination.
    pub fn set_workspaces(&mut self, workspaces: BTreeMap<String, String>) {
        self.workspaces = workspaces;
    }

    /// Has the display `target` show `mirror` instead of its own content, or with `None` its
    /// own again. Says whether it does: not for a display that isn't in the combination, nor
    /// for a chain or a cycle (a source that shows another display itself, or a target that is
    /// shown elsewhere), which would leave screens without content.
    pub fn set_mirror(&mut self, target: &str, mirror: Option<Mirror>) -> bool {
        if let Some(mirror) = &mirror {
            let source_mirrors = self
                .outputs
                .get(&mirror.of)
                .is_none_or(|setup| setup.mirror.is_some());
            let target_shown = self.outputs.values().any(|setup| {
                setup
                    .mirror
                    .as_ref()
                    .is_some_and(|other| other.of == target)
            });
            if mirror.of == target || source_mirrors || target_shown {
                return false;
            }
        }
        match self.outputs.get_mut(target) {
            Some(setup) => {
                // The same picture with another fit: the workspaces it took along stay noted.
                let mut mirror = mirror;
                if let (Some(new), Some(old)) = (&mut mirror, &setup.mirror) {
                    // Its workspaces went to the display shown before; they come back from wherever
                    // they are when it shows its own content again.
                    if new.gathered.is_empty() {
                        new.gathered.clone_from(&old.gathered);
                        new.session.clone_from(&old.session);
                    }
                }
                setup.mirror = mirror;
                true
            }
            None => false,
        }
    }

    /// Notes that the workspaces `names` moved off the display `target` when its picture started
    /// ([`super::mirror::gather_workspaces`]).
    pub fn note_gathered(&mut self, target: &str, names: &[String], session: &str) {
        let Some(mirror) = self
            .outputs
            .get_mut(target)
            .and_then(|setup| setup.mirror.as_mut())
        else {
            return;
        };
        // Names of another sway session are other workspaces now.
        if mirror.session != session {
            mirror.gathered.clear();
            session.clone_into(&mut mirror.session);
        }
        for name in names {
            if !mirror.gathered.contains(name) {
                mirror.gathered.push(name.clone());
            }
        }
    }

    /// Where the display `target` stands while it shows another display's picture: right of the
    /// displays that show their own content, with a gap of [`APART`] logical pixels, which the
    /// pointer doesn't cross, so it can't go onto the picture by accident. `None` if it shows
    /// its own content, or if the others' places or sizes aren't known.
    pub fn apart(&self, target: &str) -> Option<(i32, i32)> {
        self.outputs.get(target)?.mirror.as_ref()?;
        let others: Vec<(i32, i32, i32)> = self
            .outputs
            .iter()
            .filter(|(name, setup)| *name != target && setup.enabled && setup.mirror.is_none())
            .map(|(_, setup)| {
                let (x, y) = setup.position?;
                Some((x, y, x.saturating_add(logical_width(setup)?)))
            })
            .collect::<Option<_>>()?;
        let right = others.iter().map(|(_, _, right)| *right).max()?;
        let top = others.iter().map(|(_, y, _)| *y).min()?;
        Some((right.saturating_add(APART), top))
    }

    /// The workspaces that moved off the display `target` when its picture started, in the sway
    /// `session` (its IPC socket); none for another one.
    pub fn gathered(&self, target: &str, session: &str) -> Vec<String> {
        self.outputs
            .get(target)
            .and_then(|setup| setup.mirror.as_ref())
            .filter(|mirror| mirror.session == session)
            .map(|mirror| mirror.gathered.clone())
            .unwrap_or_default()
    }

    /// Whether a display of the combination mirrors another.
    pub fn mirrors(&self) -> bool {
        self.outputs.values().any(|setup| setup.mirror.is_some())
    }

    /// The name kanshi knows the profile by: `sway-things-` and the start of a hash of the
    /// combination, so it stays the same however the displays are set up.
    pub fn name(&self) -> String {
        let mut hash = Sha256::new();
        for name in self.combination() {
            hash.update(name.as_bytes());
            hash.update([0]);
        }
        let digest = hash.finalize();
        let mut name = String::from("sway-things-");
        for byte in digest.iter().take(4) {
            let _ = write!(name, "{byte:02x}");
        }
        name
    }
}

/// Every remembered combination, as in `displays.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Profiles {
    path: PathBuf,
    list: Vec<Profile>,
    /// Whether the workspaces are placed on the displays as each profile says (opt-in).
    place_workspaces: bool,
}

impl Profiles {
    /// None, to be saved at `path`.
    pub fn empty(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            list: Vec::new(),
            place_workspaces: false,
        }
    }

    /// The profiles in `path`. A missing file has none; a file that can't be read or parsed, or
    /// has another version, is an error, and nothing is saved over it.
    ///
    /// # Errors
    ///
    /// [`ProfilesError`] if the file can't be read or isn't one this build knows.
    pub fn load(path: &Path) -> Result<Self, ProfilesError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::empty(path)),
            Err(error) => return Err(ProfilesError::Io(error)),
        };
        let document: DocumentMut = text.parse().map_err(|_| ProfilesError::Invalid)?;
        if document.get("version").and_then(Item::as_integer) != Some(VERSION) {
            return Err(ProfilesError::Invalid);
        }
        let mut profiles = Vec::new();
        let tables = document
            .get("profile")
            .and_then(Item::as_array_of_tables)
            .into_iter()
            .flatten();
        for table in tables {
            let mut outputs = BTreeMap::new();
            let entries = table
                .get("output")
                .and_then(Item::as_table)
                .into_iter()
                .flat_map(Table::iter);
            for (name, item) in entries {
                let Some(setup) = item.as_table().and_then(read_setup) else {
                    return Err(ProfilesError::Invalid);
                };
                outputs.insert(name.to_owned(), setup);
            }
            let workspaces = table
                .get("workspaces")
                .and_then(Item::as_table)
                .into_iter()
                .flat_map(Table::iter)
                .filter_map(|(name, item)| Some((name.to_owned(), item.as_str()?.to_owned())))
                .collect();
            if !outputs.is_empty() {
                profiles.push(Profile {
                    outputs,
                    workspaces,
                });
            }
        }
        let place_workspaces = document
            .get("workspaces")
            .and_then(|item| item.get("place"))
            .and_then(Item::as_bool)
            .unwrap_or(false);
        Ok(Self {
            path: path.to_owned(),
            list: profiles,
            place_workspaces,
        })
    }

    /// The file they are saved in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The remembered combinations.
    pub fn iter(&self) -> impl Iterator<Item = &Profile> {
        self.list.iter()
    }

    /// The profile of the combination of `names`, if one is remembered.
    pub fn of(&self, names: &[&str]) -> Option<&Profile> {
        let mut wanted: Vec<&str> = names.to_vec();
        wanted.sort_unstable();
        self.list
            .iter()
            .find(|profile| profile.combination() == wanted)
    }

    /// Whether the workspaces are placed on the displays as each profile says.
    pub fn places_workspaces(&self) -> bool {
        self.place_workspaces
    }

    /// Places the workspaces as each profile says, or not.
    pub fn set_places_workspaces(&mut self, place: bool) {
        self.place_workspaces = place;
    }

    /// Remembers `profile` for its combination, in place of what was remembered for it; a
    /// profile without a placement of the workspaces keeps the one remembered. Says whether
    /// anything changed.
    pub fn remember(&mut self, mut profile: Profile) -> bool {
        let combination: Vec<String> = profile
            .combination()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let known = self.list.iter_mut().find(|known| {
            known
                .combination()
                .iter()
                .copied()
                .eq(combination.iter().map(String::as_str))
        });
        let Some(known) = known else {
            self.list.push(profile);
            return true;
        };
        // What a capture doesn't see stays: the placement and the mirrors.
        if profile.workspaces.is_empty() {
            profile.workspaces.clone_from(&known.workspaces);
        }
        for (name, setup) in &mut profile.outputs {
            if setup.mirror.is_none() {
                setup.mirror = known
                    .outputs
                    .get(name)
                    .and_then(|known| known.mirror.clone());
            }
        }
        if *known == profile {
            return false;
        }
        *known = profile;
        true
    }

    /// The profile of the combination of `names`, to change, if one is remembered.
    pub fn of_mut(&mut self, names: &[&str]) -> Option<&mut Profile> {
        let mut wanted: Vec<&str> = names.to_vec();
        wanted.sort_unstable();
        self.list
            .iter_mut()
            .find(|profile| profile.combination() == wanted)
    }

    /// Saves the profiles to the file they were loaded from, atomically.
    ///
    /// # Errors
    ///
    /// The error of writing the file.
    pub fn save(&self) -> io::Result<()> {
        match std::fs::read_to_string(&self.path) {
            Ok(old) if !old.starts_with(HEADER) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "it wasn't written by Sway Things, so it is left alone",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut document = DocumentMut::new();
        document.insert("version", value(VERSION));
        if self.place_workspaces {
            let mut workspaces = Table::new();
            workspaces.insert("place", value(true));
            document.insert("workspaces", Item::Table(workspaces));
        }
        let mut tables = toml_edit::ArrayOfTables::new();
        for profile in &self.list {
            let mut outputs = Table::new();
            for (name, setup) in &profile.outputs {
                outputs.insert(name, Item::Table(write_setup(setup)));
            }
            let mut table = Table::new();
            table.insert("output", Item::Table(outputs));
            if !profile.workspaces.is_empty() {
                let mut workspaces = Table::new();
                for (workspace, display) in &profile.workspaces {
                    workspaces.insert(workspace, value(display.as_str()));
                }
                table.insert("workspaces", Item::Table(workspaces));
            }
            tables.push(table);
        }
        document.insert("profile", Item::ArrayOfTables(tables));
        atomic::write(&self.path, &format!("{HEADER}\n{document}"))
    }
}

/// Why the profiles couldn't be loaded.
#[derive(Debug)]
pub enum ProfilesError {
    /// The file can't be read.
    Io(io::Error),
    /// It isn't TOML, has another version, or a display's setup can't be read.
    Invalid,
}

impl std::fmt::Display for ProfilesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "the remembered displays can't be read: {error}"),
            Self::Invalid => {
                f.write_str("the remembered displays aren't in a form Sway Things knows")
            }
        }
    }
}

impl std::error::Error for ProfilesError {}

fn read_setup(table: &Table) -> Option<Setup> {
    let enabled = table.get("enabled").and_then(Item::as_bool)?;
    let mode = match table.get("mode").and_then(Item::as_str) {
        Some(text) => Some(Mode::parse(text).ok()?),
        None => None,
    };
    let position = match table.get("position").and_then(Item::as_str) {
        Some(text) => Some(crate::command::output::parse_position(text).ok()?),
        None => None,
    };
    let scale = table.get("scale").and_then(Item::as_float);
    let transform = match table.get("transform").and_then(Item::as_str) {
        Some(name) => Some(Transform::from_name(name)?),
        None => None,
    };
    let mirror = table.get("mirror").and_then(Item::as_str).map(|of| Mirror {
        session: table
            .get("mirror_session")
            .and_then(Item::as_str)
            .unwrap_or_default()
            .to_owned(),
        gathered: table
            .get("mirror_workspaces")
            .and_then(Item::as_array)
            .into_iter()
            .flatten()
            .filter_map(|name| name.as_str().map(str::to_owned))
            .collect(),
        of: of.to_owned(),
        scaling: table
            .get("mirror_scaling")
            .and_then(Item::as_str)
            .and_then(Scaling::from_name)
            .unwrap_or_default(),
    });
    Some(Setup {
        enabled,
        mode,
        position,
        scale,
        transform,
        mirror,
    })
}

fn write_setup(setup: &Setup) -> Table {
    let mut table = Table::new();
    table.insert("enabled", value(setup.enabled));
    if let Some(mode) = setup.mode {
        table.insert("mode", value(mode.to_string()));
    }
    if let Some((x, y)) = setup.position {
        table.insert("position", value(format!("{x},{y}")));
    }
    if let Some(scale) = setup.scale {
        table.insert("scale", value(scale));
    }
    if let Some(transform) = setup.transform {
        table.insert("transform", value(transform.name()));
    }
    if let Some(mirror) = &setup.mirror {
        table.insert("mirror", value(mirror.of.as_str()));
        table.insert("mirror_scaling", value(mirror.scaling.name()));
        if !mirror.gathered.is_empty() {
            let names: toml_edit::Array = mirror.gathered.iter().map(String::as_str).collect();
            table.insert("mirror_workspaces", value(names));
            table.insert("mirror_session", value(mirror.session.as_str()));
        }
    }
    table
}

/// The text of `~/.config/kanshi/sway-things.conf` for `profiles`: a profile per remembered
/// combination, then for each display of them that has no profile of its own alone, one that
/// switches it on when it is the only one connected, so a display stored as off for a
/// combination (the laptop's panel at the dock) never leaves the screen dark when the others go.
/// There is no profile for any displays at all (`...output *`): kanshi keeps a profile that still
/// matches when displays are plugged in, so it would never switch to a remembered combination
/// again. A display whose name can't be quoted for kanshi (with a `"` or a line break in it)
/// leaves out its whole combination, since kanshi would match it otherwise.
pub fn kanshi_config(profiles: &Profiles) -> String {
    let mut text = String::from(
        "# Generated by Sway Things from ~/.config/sway-things/displays.toml. DO NOT EDIT.\n\
         # Set up the displays on the Displays page; each combination of displays is remembered.\n",
    );
    // Once any combination mirrors, every profile has the mirrors ended or started.
    let mirrors = profiles.iter().any(Profile::mirrors);
    for profile in profiles.iter() {
        if let Some(block) = kanshi_profile(profile, profiles.place_workspaces, mirrors) {
            text.push('\n');
            text.push_str(&block);
        }
    }
    let mut alone: Vec<&str> = profiles
        .iter()
        .flat_map(Profile::combination)
        .filter(|name| profiles.of(&[name]).is_none())
        .collect();
    alone.sort_unstable();
    alone.dedup();
    if !alone.is_empty() {
        text.push_str("\n# A display alone: on.\n");
    }
    for name in alone {
        let profile = Profile {
            outputs: std::iter::once((
                name.to_owned(),
                Setup {
                    enabled: true,
                    ..Setup::OFF
                },
            ))
            .collect(),
            workspaces: BTreeMap::new(),
        };
        // The mirrors of another combination end here too.
        if let Some(block) = kanshi_profile(&profile, false, mirrors) {
            text.push('\n');
            text.push_str(&block);
        }
    }
    text
}

fn kanshi_profile(profile: &Profile, place_workspaces: bool, mirrors: bool) -> Option<String> {
    // Never a profile that leaves every display off.
    if !profile.outputs.values().any(|setup| setup.enabled) {
        return None;
    }
    let mut block = format!("profile {} {{\n", profile.name());
    for (name, setup) in profile.outputs() {
        let quoted = kanshi_quote(name)?;
        // A display that shows another one's picture stands apart from the others.
        let position = profile.apart(name).or(setup.position);
        if !setup.enabled {
            let _ = writeln!(block, "    output {quoted} disable");
            continue;
        }
        let _ = write!(block, "    output {quoted} enable");
        if let Some(mode) = setup.mode {
            let _ = write!(block, " mode {mode}");
        }
        if let Some((x, y)) = position {
            let _ = write!(block, " position {x},{y}");
        }
        if let Some(scale) = setup.scale {
            let _ = write!(block, " scale {scale}");
        }
        if let Some(transform) = setup.transform {
            let _ = write!(block, " transform {}", transform.name());
        }
        block.push('\n');
    }
    if place_workspaces && !profile.workspaces.is_empty() {
        // kanshi runs it through a shell; it is our own command, without anything of the user's.
        block.push_str("    exec sway-things hook workspaces\n");
    }
    if mirrors {
        // Ends the mirrors of another combination, and starts this one's.
        block.push_str("    exec sway-things hook mirror\n");
    }
    block.push_str("}\n");
    Some(block)
}

/// `name` in double quotes for kanshi, which has no escapes inside them: `None` for a name with
/// a `"` or a control character.
fn kanshi_quote(name: &str) -> Option<String> {
    if name.is_empty() || name.chars().any(|char| char == '"' || char.is_control()) {
        return None;
    }
    Some(format!("\"{name}\""))
}

#[cfg(test)]
mod tests {
    use super::{Profile, Profiles, kanshi_config};
    use crate::outputs::Outputs;

    fn outputs(json: &str) -> Outputs {
        Outputs::new(&serde_json::from_str::<Vec<_>>(json).unwrap())
    }

    const DOCK: &str = r#"[
      {"name": "eDP-1", "active": false, "primary": false, "make": "A", "model": "B",
       "serial": "Unknown", "modes": []},
      {"name": "DP-4", "active": true, "primary": false, "make": "Dell", "model": "U27",
       "serial": "ABC", "scale": 1.5, "transform": "normal",
       "modes": [{"width": 2560, "height": 1440, "refresh": 59951}],
       "current_mode": {"width": 2560, "height": 1440, "refresh": 59951},
       "rect": {"x": 0, "y": 0, "width": 1707, "height": 960}}
    ]"#;

    #[test]
    fn a_combination_is_remembered_as_it_runs() {
        let profile = Profile::capture(&outputs(DOCK)).unwrap();
        assert_eq!(profile.combination(), ["Dell U27 ABC", "eDP-1"]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("displays.toml");
        let mut profiles = Profiles::empty(&path);
        assert!(profiles.remember(profile.clone()));
        assert!(!profiles.remember(profile.clone()));
        profiles.save().unwrap();
        let loaded = Profiles::load(&path).unwrap();
        assert_eq!(loaded, profiles);
        // A file of the user's of that name is never written over.
        let theirs = dir.path().join("theirs.toml");
        std::fs::write(&theirs, "version = 1\n").unwrap();
        assert!(Profiles::load(&theirs).unwrap().save().is_err());
        assert_eq!(loaded.of(&["eDP-1", "Dell U27 ABC"]), Some(&profile));
        assert_eq!(loaded.of(&["eDP-1"]), None);
        // The same combination again: replaced, not added.
        let mut again = Profiles::load(&path).unwrap();
        assert!(!again.remember(Profile::capture(&outputs(DOCK)).unwrap()));
        assert_eq!(again.iter().count(), 1);
    }

    #[test]
    fn a_profile_is_made_of_the_stored_values_and_what_runs_where_none_is() {
        let (_dir, state) = crate::generate::tests::state(
            "version = 1\n\
             [display.\"Dell U27 ABC\".output]\nscale = 1.25\nposition = \"1920,0\"\n\
             [display.eDP-1.output]\nenabled = true\n",
            "version = 1\n",
        );
        let profile = Profile::stored(&outputs(DOCK), &state).unwrap();
        let setups: Vec<(&str, &super::Setup)> = profile.outputs().collect();
        let (_, dell) = setups[0];
        assert_eq!(dell.scale, Some(1.25));
        assert_eq!(dell.position, Some((1920, 0)));
        // Not stored: as it runs.
        assert_eq!(
            dell.mode.map(|mode| mode.to_string()).as_deref(),
            Some("2560x1440@59.951Hz")
        );
        // Stored as on, though off now: on, the rest left to sway.
        let (_, panel) = setups[1];
        assert!(panel.enabled);
        assert_eq!(panel.scale, None);
        // Its values are what the settings store.
        let values = dell.values();
        assert!(values.contains(&("output.scale", crate::registry::Value::Decimal(1.25))));
        assert!(values.contains(&(
            "output.position",
            crate::registry::Value::Text("1920,0".to_owned())
        )));
    }

    #[test]
    fn workspaces_a_picture_took_along_are_noted_per_sway_session() {
        let mut profile = Profile::capture(&outputs(DOCK)).unwrap();
        let mirror = crate::outputs::mirror::Mirror {
            of: "Dell U27 ABC".to_owned(),
            scaling: crate::outputs::mirror::Scaling::Fit,
            gathered: Vec::new(),
            session: String::new(),
        };
        profile.outputs.get_mut("eDP-1").unwrap().enabled = true;
        assert!(profile.set_mirror("eDP-1", Some(mirror.clone())));
        profile.note_gathered("eDP-1", &["2".to_owned()], "/run/sway-ipc.1.sock");
        profile.note_gathered("eDP-1", &["4".to_owned()], "/run/sway-ipc.1.sock");
        assert_eq!(
            profile.gathered("eDP-1", "/run/sway-ipc.1.sock"),
            ["2", "4"]
        );
        // Another fit keeps them.
        assert!(profile.set_mirror("eDP-1", Some(mirror)));
        assert_eq!(
            profile.gathered("eDP-1", "/run/sway-ipc.1.sock"),
            ["2", "4"]
        );
        // After another login, the names are other workspaces.
        assert_eq!(
            profile.gathered("eDP-1", "/run/sway-ipc.2.sock"),
            [] as [&str; 0]
        );
        profile.note_gathered("eDP-1", &["1".to_owned()], "/run/sway-ipc.2.sock");
        assert_eq!(profile.gathered("eDP-1", "/run/sway-ipc.2.sock"), ["1"]);
    }

    #[test]
    fn a_display_showing_a_picture_stands_apart_where_the_pointer_cant_go() {
        let mut profile = Profile::capture(&outputs(DOCK)).unwrap();
        let panel = profile.outputs.get_mut("eDP-1").unwrap();
        panel.enabled = true;
        panel.position = Some((1707, 0));
        let mirror = crate::outputs::mirror::Mirror {
            of: "Dell U27 ABC".to_owned(),
            scaling: crate::outputs::mirror::Scaling::Fit,
            gathered: Vec::new(),
            session: String::new(),
        };
        assert_eq!(profile.apart("eDP-1"), None);
        assert!(profile.set_mirror("eDP-1", Some(mirror)));
        // Right of the Dell (2560 at scale 1.5: 1707 wide), with the gap.
        assert_eq!(profile.apart("eDP-1"), Some((1707 + super::APART, 0)));
        let mut profiles = Profiles::empty(std::path::Path::new("/nonexistent"));
        profiles.remember(profile);
        let text = kanshi_config(&profiles);
        assert!(
            text.contains(&format!("position {},0", 1707 + super::APART)),
            "{text}"
        );
    }

    #[test]
    fn kanshi_gets_a_profile_per_combination_and_a_safety_one() {
        let mut profiles = Profiles::empty(std::path::Path::new("/nonexistent"));
        profiles.remember(Profile::capture(&outputs(DOCK)).unwrap());
        let text = kanshi_config(&profiles);
        let name = profiles.iter().next().unwrap().name();
        assert!(
            text.contains(&format!(
                "profile {name} {{\n    output \"Dell U27 ABC\" enable mode 2560x1440@59.951Hz \
             position 0,0 scale 1.5 transform normal\n    output \"eDP-1\" disable\n}}\n"
            )),
            "{text}"
        );
        // Each display alone is switched on, after the remembered combinations; there is no
        // profile for any displays, which kanshi would never leave.
        let alone = |display: &str| {
            let mut profile = Profile::default();
            profile.outputs.insert(
                display.to_owned(),
                super::Setup {
                    enabled: true,
                    ..super::Setup::OFF
                },
            );
            format!(
                "profile {} {{\n    output \"{display}\" enable\n}}\n",
                profile.name()
            )
        };
        assert!(text.contains(&alone("eDP-1")), "{text}");
        assert!(text.contains(&alone("Dell U27 ABC")), "{text}");
        assert!(text.find(&name) < text.find(&alone("eDP-1")));
        assert!(!text.contains("...output"));
        // No cycles: a display shown elsewhere can't show another one.
        let mut profile = profiles.iter().next().unwrap().clone();
        let mirror = |of: &str| {
            Some(crate::outputs::mirror::Mirror {
                of: of.to_owned(),
                scaling: crate::outputs::mirror::Scaling::Fit,
                gathered: Vec::new(),
                session: String::new(),
            })
        };
        assert!(profile.set_mirror("eDP-1", mirror("Dell U27 ABC")));
        assert!(!profile.set_mirror("Dell U27 ABC", mirror("eDP-1")));
        assert!(profile.set_mirror("eDP-1", None));
        // Every display off: never remembered.
        let dark = r#"[{"name": "eDP-1", "active": false, "primary": false, "make": "A",
            "model": "B", "serial": "C", "modes": []}]"#;
        assert_eq!(Profile::capture(&outputs(dark)), None);
    }

    #[test]
    fn a_name_kanshi_can_t_quote_leaves_its_combination_out() {
        let json = r#"[{"name": "DP-1", "active": true, "primary": false, "make": "Odd \"Co\"",
            "model": "M", "serial": "S", "modes": []}]"#;
        let mut profiles = Profiles::empty(std::path::Path::new("/nonexistent"));
        profiles.remember(Profile::capture(&outputs(json)).unwrap());
        assert!(!kanshi_config(&profiles).contains("Odd"));
    }
}
