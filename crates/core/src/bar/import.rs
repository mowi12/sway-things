// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! "Import as starting point": the user's own waybar config read into the settings and the
//! module definitions. Nothing is written here and nothing is run.
//!
//! The config is found as waybar finds it without `-c` (`config`, then `config.jsonc`, in
//! `$WAYBAR_CONFIG_DIR`, `$XDG_CONFIG_HOME/waybar`, `~/.config/waybar`, `~/waybar`; the
//! system's default config isn't the user's), and read as waybar reads it: JSON with comments, and `include`s merged so
//! that the first definition of a key wins, objects merged key by key (`Config::mergeConfig` in
//! waybar 0.16). A config with several bars gives its first one. Include paths are expanded like
//! `wordexp` without a shell, relative to the including file, then in waybar's directories; one
//! that needs a shell is left out and noted.
//!
//! What has a setting becomes its value; every module definition (all objects, also unused
//! ones) and every other option of the bar are kept as they are, so nothing is lost. The
//! stylesheet next to the config is offered as the user's own, imported after ours.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value as Json};

use super::jsonc;
use super::modules::Modules;
use crate::i18n::Text;
use crate::msg;
use crate::registry::{Registry, Value, bar as keys};
use crate::sway_config::Files;

/// waybar's directories of the user, as `Config::CONFIG_DIRS` (0.16) has them, after
/// `$WAYBAR_CONFIG_DIR`. The system's (`/etc/xdg/waybar`) holds waybar's default config, which
/// is the default bar here anyway, so it is no config of the user's to import.
const DIRS: &[&str] = &[
    "$XDG_CONFIG_HOME/waybar",
    "$HOME/.config/waybar",
    "$HOME/waybar",
];

/// The deepest includes go, as in waybar.
const MAX_DEPTH: usize = 100;
/// The most files read, so includes can't make the work grow without end.
const MAX_FILES: usize = 64;
/// The most bytes read from all files together.
const MAX_TOTAL: usize = 16 * 1024 * 1024;

/// The user's waybar files, as waybar finds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The config.
    pub config: PathBuf,
    /// The stylesheet next to it, if there is one.
    pub style: Option<PathBuf>,
}

/// The directories waybar looks in, with the variables of `files` replaced; those whose
/// variable isn't set are left out.
fn dirs(files: &dyn Files) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = files.env("WAYBAR_CONFIG_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    let home = files.env("HOME").filter(|home| home.starts_with('/'));
    let config_home = files
        .env("XDG_CONFIG_HOME")
        .filter(|dir| dir.starts_with('/'))
        .or_else(|| home.as_ref().map(|home| format!("{home}/.config")));
    for dir in DIRS {
        let expanded = if let Some(rest) = dir.strip_prefix("$XDG_CONFIG_HOME") {
            config_home.as_ref().map(|base| format!("{base}{rest}"))
        } else if let Some(rest) = dir.strip_prefix("$HOME") {
            home.as_ref().map(|base| format!("{base}{rest}"))
        } else {
            Some((*dir).to_owned())
        };
        if let Some(dir) = expanded {
            let dir = PathBuf::from(dir);
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs
}

/// The user's waybar config and stylesheet, as waybar without `-c` finds them, leaving out
/// `ours` (the directory Sway Things writes to). `None` if there is none.
pub fn find(files: &dyn Files, ours: &Path) -> Option<Found> {
    for dir in dirs(files) {
        if dir.starts_with(ours) {
            continue;
        }
        for name in ["config", "config.jsonc"] {
            let config = dir.join(name);
            if files.is_file(&config).unwrap_or(false) {
                let style = dir.join("style.css");
                let style = files.is_file(&style).unwrap_or(false).then_some(style);
                return Some(Found { config, style });
            }
        }
    }
    None
}

/// What an import gives.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    /// The values of the bar settings, each checked against its setting.
    pub values: Vec<(&'static str, Value)>,
    /// The module definitions and the options of the bar without settings.
    pub modules: Modules,
    /// The files read, the config first.
    pub read: Vec<PathBuf>,
    /// What couldn't be taken over as it is.
    pub notes: Vec<Text>,
}

/// Why the config can't be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// It can't be read.
    Io(PathBuf, String),
    /// It isn't a config waybar can read.
    Broken(PathBuf, Text),
}

impl ImportError {
    /// What is wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Io(path, error) => Text::new(msg!("Can't read {path}: {error}"))
                .text("path", Text::data(path.display().to_string()))
                .text("error", Text::data(error.clone())),
            Self::Broken(path, reason) => Text::new(msg!("Can't read {path}: {reason}"))
                .text("path", Text::data(path.display().to_string()))
                .text("reason", reason.clone()),
        }
    }
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for ImportError {}

/// Reads the config at `found`, with its includes, into the settings and module definitions.
/// The stylesheet, if found, becomes the user's own stylesheet with no look of ours under it.
///
/// # Errors
///
/// [`ImportError`] if the config itself can't be read or isn't JSON with comments. Includes
/// that can't be read are noted instead.
pub fn read(found: &Found, files: &dyn Files) -> Result<Imported, ImportError> {
    let mut reader = Reader {
        files,
        read: Vec::new(),
        notes: Vec::new(),
        total: 0,
    };
    let bar = reader.load(&found.config, 0)?;
    let mut imported = settings(bar, &mut reader.notes);
    if let Some(style) = &found.style {
        if let Some(text) = style.to_str() {
            imported
                .values
                .push((keys::EXTRA_CSS, Value::Text(text.to_owned())));
            imported
                .values
                .push((keys::THEME, Value::Choice("plain".to_owned())));
        }
    }
    imported.read = reader.read;
    imported.notes.append(&mut reader.notes);
    // Every value is one its setting takes.
    let registry = Registry::builtin();
    imported.values.retain(|(key, value)| {
        registry
            .get(key)
            .is_some_and(|setting| setting.check(value).is_ok())
    });
    Ok(imported)
}

struct Reader<'a> {
    files: &'a dyn Files,
    read: Vec<PathBuf>,
    notes: Vec<Text>,
    /// The bytes read so far.
    total: usize,
}

impl Reader<'_> {
    /// The first bar of the file at `path` with its includes merged in.
    fn load(&mut self, path: &Path, depth: usize) -> Result<Map<String, Json>, ImportError> {
        let bytes = self
            .files
            .read_bounded(path, jsonc::MAX_SIZE)
            .map_err(|error| ImportError::Io(path.to_owned(), error.to_string()))?;
        self.total += bytes.len();
        if self.total > MAX_TOTAL {
            return Err(ImportError::Broken(
                path.to_owned(),
                msg!("the files read are larger than 16 MiB together").into(),
            ));
        }
        self.read.push(path.to_owned());
        let text = String::from_utf8_lossy(&bytes);
        let value = jsonc::parse(&text)
            .map_err(|error| ImportError::Broken(path.to_owned(), error.text()))?;
        let mut bar = match value {
            Json::Object(bar) => bar,
            Json::Array(bars) => {
                let count = bars.len();
                if count > 1 && depth == 0 {
                    self.notes.push(
                        Text::new(msg!(
                            "The config has {count} bars; the first one is taken over"
                        ))
                        .arg("count", count),
                    );
                }
                match bars.into_iter().find_map(|bar| match bar {
                    Json::Object(bar) => Some(bar),
                    _ => None,
                }) {
                    Some(bar) => bar,
                    None => Map::new(),
                }
            }
            _ => {
                return Err(ImportError::Broken(
                    path.to_owned(),
                    msg!("expected an object").into(),
                ));
            }
        };
        self.includes(&mut bar, path, depth);
        Ok(bar)
    }

    /// Merges the files `bar` includes into it, the first definition of a key winning.
    fn includes(&mut self, bar: &mut Map<String, Json>, path: &Path, depth: usize) {
        let names: Vec<String> = match bar.get("include") {
            Some(Json::String(name)) => vec![name.clone()],
            Some(Json::Array(names)) => names
                .iter()
                .filter_map(Json::as_str)
                .map(ToOwned::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        let directory = path.parent().unwrap_or(Path::new("/"));
        for name in names {
            if depth + 1 > MAX_DEPTH {
                self.notes.push(
                    Text::new(msg!("Includes go too deep; {name} isn't read"))
                        .text("name", Text::data(name.clone())),
                );
                continue;
            }
            for include in self.resolve(&name, directory) {
                // Each file counts, also those of one pattern.
                if self.read.len() >= MAX_FILES {
                    self.notes.push(
                        Text::new(msg!("More than 64 files are included; {file} isn't read"))
                            .text("file", Text::data(include.display().to_string())),
                    );
                    continue;
                }
                match self.load(&include, depth + 1) {
                    Ok(mut other) => merge(bar, &mut other),
                    Err(error) => self.notes.push(error.text()),
                }
            }
        }
    }

    /// The files an include of `name` stands for: expanded as `wordexp` does without a shell,
    /// relative to `directory`, else in waybar's directories.
    fn resolve(&mut self, name: &str, directory: &Path) -> Vec<PathBuf> {
        let existing = |paths: Vec<PathBuf>| -> Vec<PathBuf> {
            paths
                .into_iter()
                .filter(|path| self.files.is_file(path).unwrap_or(false))
                .collect()
        };
        let Ok(paths) = crate::sway_config::expand(name, directory, self.files) else {
            self.notes.push(
                Text::new(msg!(
                    "The include {name} needs a shell to be found, so it isn't read"
                ))
                .text("name", Text::data(name.to_owned())),
            );
            return Vec::new();
        };
        let found = existing(paths);
        if !found.is_empty() {
            return found;
        }
        for dir in dirs(self.files) {
            if let Ok(paths) = crate::sway_config::expand(name, &dir, self.files) {
                let found = existing(paths);
                if !found.is_empty() {
                    return found;
                }
            }
        }
        self.notes.push(
            Text::new(msg!("The include {name} wasn't found"))
                .text("name", Text::data(name.to_owned())),
        );
        Vec::new()
    }
}

/// Merges `other` into `bar` as waybar does: objects key by key, otherwise the value already
/// there wins.
fn merge(bar: &mut Map<String, Json>, other: &mut Map<String, Json>) {
    for (key, value) in std::mem::take(other) {
        match (bar.get_mut(&key), value) {
            (Some(Json::Object(mine)), Json::Object(mut theirs)) => merge(mine, &mut theirs),
            (Some(_), _) => {}
            (None, value) => {
                bar.insert(key, value);
            }
        }
    }
}

/// The settings and modules of a bar's config.
fn settings(mut bar: Map<String, Json>, notes: &mut Vec<Text>) -> Imported {
    let mut values: Vec<(&'static str, Value)> = Vec::new();
    let text = |value: Option<&Json>| value.and_then(Json::as_str).map(ToOwned::to_owned);
    let choice = |value: Option<String>, default: &str| {
        Value::Choice(value.unwrap_or_else(|| default.to_owned()))
    };
    values.push((keys::POSITION, choice(text(bar.get("position")), "top")));
    values.push((keys::LAYER, choice(text(bar.get("layer")), "bottom")));
    // Without a height, waybar fits the bar to its modules.
    let height = bar.get("height").and_then(Json::as_u64).unwrap_or(0);
    values.push((
        keys::HEIGHT,
        Value::Integer(i64::try_from(height).unwrap_or(0)),
    ));
    values.push((
        keys::EXCLUSIVE,
        Value::Bool(bar.get("exclusive").and_then(Json::as_bool).unwrap_or(true)),
    ));
    values.push((
        keys::SPACING,
        Value::Integer(bar.get("spacing").and_then(Json::as_i64).unwrap_or(0)),
    ));
    let margins = margins(&bar);
    for (key, margin) in [
        keys::MARGIN_TOP,
        keys::MARGIN_RIGHT,
        keys::MARGIN_BOTTOM,
        keys::MARGIN_LEFT,
    ]
    .into_iter()
    .zip(margins)
    {
        values.push((key, Value::Integer(margin)));
    }
    let outputs: Vec<String> = match bar.get("output") {
        Some(Json::String(output)) => vec![output.clone()],
        Some(Json::Array(outputs)) => outputs
            .iter()
            .filter_map(Json::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    };
    let (plain, special): (Vec<String>, Vec<String>) = outputs
        .into_iter()
        .partition(|output| !output.starts_with(['!', '*', '$']));
    if !special.is_empty() {
        notes.push(
            Text::new(msg!(
                "The displays {outputs} can't be picked here; the bar shows on the others named, or on all"
            ))
            .text("outputs", Text::data(special.join(", "))),
        );
    }
    values.push((keys::OUTPUTS, Value::List(plain)));
    for (key, name) in [
        (keys::MODULES_LEFT, "modules-left"),
        (keys::MODULES_CENTER, "modules-center"),
        (keys::MODULES_RIGHT, "modules-right"),
    ] {
        let list: Vec<String> = bar
            .get(name)
            .and_then(Json::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(Json::as_str)
                    .filter(|name| super::is_module_name(name))
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        values.push((key, Value::List(list)));
    }
    let mut modules = Modules::default();
    let mut options = Map::new();
    for (key, value) in std::mem::take(&mut bar) {
        match value {
            Json::Object(definition) if super::is_module_name(&key) => {
                modules.set(&key, definition);
            }
            other => {
                options.insert(key, other);
            }
        }
    }
    modules.set_bar_options(options);
    Imported {
        values,
        modules,
        read: Vec::new(),
        notes: Vec::new(),
    }
}

/// The margins of a bar, top, right, bottom, left, as waybar reads them: `margin-*` numbers if
/// any is given, else `margin` as a number or as one to four numbers in a string.
fn margins(bar: &Map<String, Json>) -> [i64; 4] {
    let sides = ["margin-top", "margin-right", "margin-bottom", "margin-left"];
    if sides
        .iter()
        .any(|side| bar.get(*side).is_some_and(Json::is_i64))
    {
        return sides.map(|side| bar.get(side).and_then(Json::as_i64).unwrap_or(0).max(0));
    }
    match bar.get("margin") {
        Some(Json::Number(number)) => [number.as_i64().unwrap_or(0).max(0); 4],
        Some(Json::String(text)) => {
            let numbers: Vec<i64> = text
                .split_whitespace()
                .map(|word| word.parse::<i64>().unwrap_or(0).max(0))
                .collect();
            match numbers[..] {
                [all] => [all; 4],
                [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
                [top, horizontal, bottom] => [top, horizontal, bottom, horizontal],
                [top, right, bottom, left] => [top, right, bottom, left],
                _ => [0; 4],
            }
        }
        _ => [0; 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sway_config::MemoryFiles;
    use serde_json::json;

    const RICH: &str = include_str!("fixtures/rich.jsonc");
    const EXTRA: &str = include_str!("fixtures/extra.jsonc");

    fn files() -> MemoryFiles {
        MemoryFiles::new()
            .var("HOME", "/home/u")
            .file("/home/u/.config/waybar/config.jsonc", RICH)
            .file("/home/u/.config/waybar/extra.jsonc", EXTRA)
            .file("/home/u/.config/waybar/style.css", "#clock { color: red; }")
    }

    fn value(imported: &Imported, key: &str) -> Value {
        imported
            .values
            .iter()
            .find(|(found, _)| *found == key)
            .map(|(_, value)| value.clone())
            .unwrap()
    }

    #[test]
    fn the_user_s_config_is_found_but_not_ours() {
        let files = files();
        let found = find(&files, Path::new("/home/u/.config/sway-things/waybar")).unwrap();
        assert_eq!(
            found.config,
            Path::new("/home/u/.config/waybar/config.jsonc")
        );
        assert_eq!(
            found.style.as_deref(),
            Some(Path::new("/home/u/.config/waybar/style.css"))
        );
        assert_eq!(find(&files, Path::new("/home/u/.config/waybar")), None);
    }

    #[test]
    fn a_rich_config_loses_nothing() {
        let files = files();
        let found = find(&files, Path::new("/nowhere")).unwrap();
        let imported = read(&found, &files).unwrap();
        assert_eq!(imported.read.len(), 2, "the include was read");
        assert_eq!(value(&imported, keys::LAYER), Value::Choice("top".into()));
        assert_eq!(value(&imported, keys::HEIGHT), Value::Integer(30));
        assert_eq!(value(&imported, keys::MARGIN_TOP), Value::Integer(6));
        assert_eq!(value(&imported, keys::MARGIN_BOTTOM), Value::Integer(0));
        assert_eq!(value(&imported, keys::MARGIN_LEFT), Value::Integer(8));
        assert_eq!(
            value(&imported, keys::MODULES_CENTER),
            Value::List(vec!["group/time".into(), "mpris".into()])
        );
        assert_eq!(value(&imported, keys::THEME), Value::Choice("plain".into()));
        let modules = &imported.modules;
        // Nested groups with drawers, instances, images, privacy, calendar actions and custom
        // modules come over unchanged.
        let time = modules.definition("group/time").unwrap();
        assert_eq!(
            time["modules"],
            json!(["group/clock-drawer", "custom/pomodoro"])
        );
        let drawer = modules.definition("group/clock-drawer").unwrap();
        assert_eq!(drawer["drawer"]["transition-left-to-right"], json!(false));
        let date = modules.definition("clock#date").unwrap();
        assert_eq!(date["actions"]["on-scroll-up"], json!("shift_up"));
        assert_eq!(
            date["calendar"]["format"]["today"],
            json!("<b><u>{}</u></b>")
        );
        let privacy = modules.definition("privacy").unwrap();
        assert_eq!(privacy["modules"][0]["type"], json!("screenshare"));
        assert_eq!(
            modules.definition("image#expand").unwrap()["interval"],
            json!(86400)
        );
        let pomodoro = modules.definition("custom/pomodoro").unwrap();
        assert_eq!(pomodoro["on-click-middle"], json!("~/bin/timer skip"));
        assert_eq!(pomodoro["return-type"], json!("json"));
        // A module defined only in the include, and the include's value of a key the config
        // sets itself loses.
        assert_eq!(
            modules.definition("custom/from-include").unwrap()["exec"],
            json!("echo hi")
        );
        assert_eq!(modules.definition("clock").unwrap()["interval"], json!(1));
        assert_eq!(
            modules.definition("clock").unwrap()["tooltip"],
            json!(false)
        );
        // Unused definitions and options of the bar without settings stay.
        assert!(modules.definition("custom/unused").is_some());
        assert_eq!(modules.bar_options()["fixed-center"], json!(false));
        assert!(modules.bar_options().get("include").is_none());
        // Every module the bar shows has its definition, so the config waybar gets has them all.
        let lists: Vec<Vec<String>> = [
            keys::MODULES_LEFT,
            keys::MODULES_CENTER,
            keys::MODULES_RIGHT,
        ]
        .iter()
        .map(|key| match value(&imported, key) {
            Value::List(list) => list,
            _ => Vec::new(),
        })
        .collect();
        for name in modules.shown(&lists) {
            if name != "tray" {
                assert!(modules.is_stored(&name), "{name}");
            }
        }
    }

    #[test]
    fn several_bars_give_the_first_and_includes_that_need_a_shell_are_noted() {
        let files = MemoryFiles::new()
            .var("HOME", "/home/u")
            .file(
                "/home/u/.config/waybar/config",
                r#"[{"position": "bottom", "include": "$(evil)", "margin": "1 2"}, {"position": "top"}]"#,
            );
        let found = find(&files, Path::new("/nowhere")).unwrap();
        let imported = read(&found, &files).unwrap();
        assert_eq!(
            value(&imported, keys::POSITION),
            Value::Choice("bottom".into())
        );
        assert_eq!(value(&imported, keys::MARGIN_RIGHT), Value::Integer(2));
        assert_eq!(value(&imported, keys::MARGIN_BOTTOM), Value::Integer(1));
        assert_eq!(imported.notes.len(), 2, "{:?}", imported.notes);
        assert!(
            !imported
                .values
                .iter()
                .any(|(key, _)| *key == keys::EXTRA_CSS)
        );
    }

    #[test]
    fn files_of_a_pattern_count_one_by_one() {
        let mut files = MemoryFiles::new().var("HOME", "/home/u").file(
            "/home/u/.config/waybar/config",
            r#"{"include": "parts/*.jsonc"}"#,
        );
        for n in 0..100 {
            files = files.file(&format!("/home/u/.config/waybar/parts/{n:03}.jsonc"), "{}");
        }
        let found = find(&files, Path::new("/nowhere")).unwrap();
        let imported = read(&found, &files).unwrap();
        assert_eq!(imported.read.len(), MAX_FILES);
        assert_ne!(imported.notes.len(), 0);
    }

    #[test]
    fn a_broken_config_is_an_error_and_includes_end() {
        let files = MemoryFiles::new()
            .var("HOME", "/home/u")
            .file("/home/u/.config/waybar/config", "{ \"a\": ")
            .file("/home/u/loop.jsonc", r#"{"include": "/home/u/loop.jsonc"}"#);
        let found = find(&files, Path::new("/nowhere")).unwrap();
        assert!(matches!(read(&found, &files), Err(ImportError::Broken(..))));
        let found = Found {
            config: PathBuf::from("/home/u/loop.jsonc"),
            style: None,
        };
        let imported = read(&found, &files).unwrap();
        assert!(imported.read.len() <= MAX_FILES);
    }
}
