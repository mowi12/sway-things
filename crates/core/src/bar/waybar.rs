// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! waybar's config and stylesheet as Sway Things writes them.
//!
//! [`config`] builds the JSON from the bar settings and the module definitions; it is valid by
//! construction, since `serde_json` writes it. waybar has no option to check a config without
//! showing a bar (its options are `-c`, `-s`, `-l`, `-b`, `-v` and `-h`, 0.16), and one it can't
//! read ends it, also at a reload. So nothing is written that `serde_json` didn't make, and a
//! stylesheet only holds rules [`super::style`] made from checked values, plus the user's own
//! stylesheet, which stays theirs, by an `@import`.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use super::modules::Modules;
use super::{HEADER, HEADER_HINT, WaybarFiles, jsonc, style, value};
use crate::registry::{Value as Setting, bar as keys};
use crate::state::State;
use crate::sway_config::SwayConfig;

/// The three module lists of `state`: left, center, right.
pub fn lists(state: &State) -> [Vec<String>; 3] {
    [
        keys::MODULES_LEFT,
        keys::MODULES_CENTER,
        keys::MODULES_RIGHT,
    ]
    .map(|key| match value(state, key) {
        Setting::List(items) => items
            .into_iter()
            .filter(|item| super::is_module_name(item))
            .collect(),
        _ => Vec::new(),
    })
}

fn integer(state: &State, key: &str) -> i64 {
    match value(state, key) {
        Setting::Integer(number) => number,
        _ => 0,
    }
}

fn choice(state: &State, key: &str) -> String {
    match value(state, key) {
        Setting::Choice(id) => id,
        _ => String::new(),
    }
}

/// The config of waybar for `state` and `modules`: where the bar stands and what it shows from
/// the settings, the options of the bar without settings, and the definition of every module it
/// shows (also the members of groups).
pub fn config(state: &State, modules: &Modules) -> Value {
    let mut bar = Map::new();
    bar.insert("layer".to_owned(), Value::from(choice(state, keys::LAYER)));
    bar.insert(
        "position".to_owned(),
        Value::from(choice(state, keys::POSITION)),
    );
    let height = integer(state, keys::HEIGHT);
    if height > 0 {
        bar.insert("height".to_owned(), Value::from(height));
    }
    if let Setting::List(outputs) = value(state, keys::OUTPUTS) {
        if !outputs.is_empty() {
            bar.insert("output".to_owned(), Value::from(outputs));
        }
    }
    if let Setting::Bool(exclusive) = value(state, keys::EXCLUSIVE) {
        bar.insert("exclusive".to_owned(), Value::from(exclusive));
    }
    for (key, name) in [
        (keys::MARGIN_TOP, "margin-top"),
        (keys::MARGIN_RIGHT, "margin-right"),
        (keys::MARGIN_BOTTOM, "margin-bottom"),
        (keys::MARGIN_LEFT, "margin-left"),
    ] {
        bar.insert(name.to_owned(), Value::from(integer(state, key)));
    }
    bar.insert(
        "spacing".to_owned(),
        Value::from(integer(state, keys::SPACING)),
    );
    // Shown while a key is held: waybar follows the bar config of that ID over sway's IPC
    // (`75-bar.conf` makes it), which says when to show.
    if super::hold_key(state).is_some() {
        bar.insert("ipc".to_owned(), Value::from(true));
        bar.insert("id".to_owned(), Value::from(crate::command::bar::WAYBAR_ID));
    }
    let lists = lists(state);
    for (name, list) in ["modules-left", "modules-center", "modules-right"]
        .iter()
        .zip(&lists)
    {
        bar.insert((*name).to_owned(), Value::from(list.clone()));
    }
    for (key, option) in modules.bar_options() {
        bar.entry(key.clone()).or_insert_with(|| option.clone());
    }
    for name in modules.shown(&lists) {
        if let Some(definition) = modules.definition(&name) {
            bar.entry(name).or_insert(Value::Object(definition));
        }
    }
    Value::Object(bar)
}

/// The text of `config.jsonc`.
pub fn config_text(state: &State, modules: &Modules) -> String {
    jsonc::write(&format!("{HEADER}\n{HEADER_HINT}"), &config(state, modules))
}

/// The text of `style.css`: our header, then `@import` of `theme.css` and of the user's own
/// stylesheet, if one is set, in that order, so the user's rules win.
pub fn style_text(state: &State) -> String {
    let mut text = format!("/* {HEADER}\n   {HEADER_HINT} */\n\n");
    text.push_str("@import url(\"theme.css\");\n");
    if let Some(path) = style::extra_css_active(state) {
        if let Some(url) = style::file_url(&path) {
            let _ = writeln!(text, "@import url(\"{url}\");");
        }
    }
    text
}

/// The text of `theme.css`: waybar's own stylesheet, or one made from the style settings.
pub fn theme_text(state: &State, sway: Option<&SwayConfig>, modules: &Modules) -> String {
    let mut text = format!("/* {HEADER}\n   {HEADER_HINT} */\n\n");
    text.push_str(&style::stylesheet(state, sway, modules));
    text
}

/// The files of `files` with their texts: `config.jsonc`, `style.css`, `theme.css`, each with
/// the comment marker its header starts with.
pub fn texts(
    files: &WaybarFiles,
    state: &State,
    modules: &Modules,
    sway: Option<&SwayConfig>,
) -> Vec<(std::path::PathBuf, &'static str, String)> {
    vec![
        (files.config(), "//", config_text(state, modules)),
        (files.style(), "/*", style_text(state)),
        (
            files.dir.join("theme.css"),
            "/*",
            theme_text(state, sway, modules),
        ),
    ]
}

/// Writes the files for waybar where they changed. Says whether any was written.
///
/// # Errors
///
/// The first [`super::BarError`]: a file that isn't ours or can't be written; the others are
/// still written.
pub fn write(
    files: &WaybarFiles,
    state: &State,
    modules: &Modules,
    sway: Option<&SwayConfig>,
    _access: &crate::include::WriteAccess,
) -> Result<bool, super::BarError> {
    let mut written = false;
    let mut first_error = None;
    for (path, comment, text) in texts(files, state, modules, sway) {
        match WaybarFiles::write(&path, comment, &text) {
            Ok(changed) => written |= changed,
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(written),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use crate::state::Paths;
    use serde_json::json;

    fn state(dir: &std::path::Path) -> State {
        State::load(
            Registry::builtin(),
            Paths::new(&dir.join("config"), &dir.join("state"), "host").unwrap(),
        )
    }

    #[test]
    fn a_fresh_bar_has_the_common_items_of_waybar_s_default_bar() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let config = config(&state, &Modules::default());
        let default = jsonc::parse(super::super::modules::WAYBAR_DEFAULT_CONFIG).unwrap();
        for key in ["height", "spacing"] {
            assert_eq!(config[key], default[key], "{key}");
        }
        // A short bar of the common items, each with its definition of the default config.
        assert_eq!(config["modules-left"], json!(keys::DEFAULT_LEFT));
        assert_eq!(config["modules-center"], json!(keys::DEFAULT_CENTER));
        assert_eq!(config["modules-right"], json!(keys::DEFAULT_RIGHT));
        for name in keys::DEFAULT_LEFT
            .iter()
            .chain(keys::DEFAULT_CENTER)
            .chain(keys::DEFAULT_RIGHT)
        {
            if let Some(definition) = default.get(*name) {
                assert_eq!(&config[*name], definition, "{name}");
            }
        }
        assert!(config.get("mpd").is_none(), "not shown, not written");
        assert_eq!(config["layer"], json!("bottom"));
        assert_eq!(config["position"], json!("top"));
        assert_eq!(config["exclusive"], json!(true));
        assert!(config.get("output").is_none());
        assert!(
            config.get("niri/workspaces").is_none(),
            "not shown, not written"
        );
    }

    #[test]
    fn a_bar_shown_while_a_key_is_held_follows_sway_s_bar_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state(dir.path());
        assert!(config(&state, &Modules::default()).get("ipc").is_none());
        state
            .set(keys::VISIBILITY, None, Setting::Choice("hold".into()), None)
            .unwrap();
        let config = config(&state, &Modules::default());
        assert_eq!(config["ipc"], json!(true));
        assert_eq!(config["id"], json!("sway-things-waybar"));
    }

    #[test]
    fn settings_and_modules_reach_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state(dir.path());
        state
            .set(keys::HEIGHT, None, Setting::Integer(0), None)
            .unwrap();
        state
            .set(
                keys::OUTPUTS,
                None,
                Setting::List(vec!["eDP-1".into()]),
                None,
            )
            .unwrap();
        state
            .set(
                keys::MODULES_LEFT,
                None,
                Setting::List(vec!["custom/hello".into(), "group/g".into()]),
                None,
            )
            .unwrap();
        let mut modules = Modules::default();
        let Value::Object(hello) = json!({"exec": "echo \"hi\"; rm -rf ~", "interval": 5}) else {
            unreachable!()
        };
        modules.set("custom/hello", hello);
        let Value::Object(group) = json!({"modules": ["cpu"], "drawer": {}}) else {
            unreachable!()
        };
        modules.set("group/g", group);
        let Value::Object(bar) = json!({"fixed-center": false, "layer": "overlay"}) else {
            unreachable!()
        };
        modules.set_bar_options(bar);
        let text = config_text(&state, &modules);
        assert!(text.starts_with(&format!("// {HEADER}\n")));
        let config = jsonc::parse(&text).unwrap();
        assert!(config.get("height").is_none(), "0 fits the modules");
        assert_eq!(config["output"], json!(["eDP-1"]));
        assert_eq!(config["fixed-center"], json!(false));
        assert_eq!(config["layer"], json!("bottom"), "the setting wins");
        assert_eq!(
            config["custom/hello"]["exec"],
            json!("echo \"hi\"; rm -rf ~")
        );
        assert_eq!(
            config["cpu"],
            Value::Object(
                super::super::modules::default_definition("cpu")
                    .unwrap()
                    .clone()
            )
        );
    }

    #[test]
    fn the_stylesheet_imports_ours_then_the_user_s() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state(dir.path());
        assert!(!style_text(&state).contains("file://"));
        state
            .set(
                keys::EXTRA_CSS,
                None,
                Setting::Text("/home/me/my style \"x\".css".into()),
                None,
            )
            .unwrap();
        let text = style_text(&state);
        let ours = text.find("theme.css").unwrap();
        let theirs = text
            .find("file:///home/me/my%20style%20%22x%22.css")
            .unwrap();
        assert!(ours < theirs);
    }
}
