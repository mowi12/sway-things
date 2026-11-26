// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use serde_json::Value as Json;

use super::files::{Files, Inclusion};
use super::{Daemon, HEADER, Stored, mako, starts, swaync};
use crate::command::Color;
use crate::registry::notifications::{self as keys, mako as mako_keys, swaync as swaync_keys};
use crate::registry::{Registry, Value};
use crate::state::{Paths, State};

fn state(dir: &Path, values: &[(&str, Value)]) -> State {
    let mut state = State::load(
        Registry::builtin(),
        Paths::new(&dir.join("config"), &dir.join("state"), "laptop").unwrap(),
    );
    for (key, value) in values {
        state.set(key, None, value.clone(), None).unwrap();
    }
    state
}

fn choice(id: &str) -> Value {
    Value::Choice(id.to_owned())
}

#[test]
fn mako_gets_only_what_is_stored_and_always_the_dnd_section() {
    let dir = tempfile::tempdir().unwrap();
    let empty = state(dir.path(), &[]);
    assert_eq!(
        mako::generate(&Stored::new(&empty)),
        format!(
            "# {HEADER}\n# mako reads it through the include= line of its config.\n\n\
             [mode=do-not-disturb]\ninvisible=1\n"
        )
    );
    let set = state(
        dir.path(),
        &[
            (keys::POSITION, choice("bottom-center")),
            (keys::OUTPUT, Value::Text("DP-1".into())),
            (mako_keys::WIDTH, Value::Integer(420)),
            (mako_keys::TIMEOUT, Value::Integer(6)),
            (mako_keys::CRITICAL_TIMEOUT, Value::Integer(0)),
            (mako_keys::GROUP, Value::Bool(true)),
            (mako_keys::FONT, Value::Text("Inter 11".into())),
            (keys::COLORS, choice("custom")),
            (
                keys::BACKGROUND,
                Value::Color(Color::rgba(0x10, 0x20, 0x30, 0x80)),
            ),
            (keys::CLICK_FOCUS, Value::Bool(true)),
        ],
    );
    let text = mako::generate(&Stored::new(&set));
    let body: Vec<&str> = text.lines().skip(2).collect();
    assert_eq!(
        body,
        [
            "anchor=bottom-center",
            "output=DP-1",
            "width=420",
            "default-timeout=6000",
            "group-by=app-name",
            "font=Inter 11",
            "background-color=#10203080",
            "text-color=#ffffffff",
            "border-color=#4c7899ff",
            "progress-color=over #4c7899ff",
            &mako::click_binding(),
            "",
            "[urgency=critical]",
            "default-timeout=0",
            "",
            "[mode=do-not-disturb]",
            "invisible=1",
        ]
    );
}

#[test]
fn mako_follows_the_focused_window_s_colors() {
    let dir = tempfile::tempdir().unwrap();
    let set = state(
        dir.path(),
        &[
            (keys::COLORS, choice("windows")),
            (
                "windows.focused.child_border",
                Value::Color(Color::rgb(0xb1, 0x62, 0xd5)),
            ),
        ],
    );
    let text = mako::generate(&Stored::new(&set));
    assert!(text.contains("\nbackground-color=#285577ff\n"), "{text}");
    assert!(text.contains("\nborder-color=#b162d5ff\n"), "{text}");
}

#[test]
fn a_mako_config_s_own_dnd_section_is_found() {
    assert!(mako::has_dnd_section(
        "font=x\n[mode=do-not-disturb]\ninvisible=1\n"
    ));
    assert!(mako::has_dnd_section("[app-name=x mode=do-not-disturb]\n"));
    assert!(!mako::has_dnd_section(
        "[mode=away]\n# [mode=do-not-disturb]\n"
    ));
}

#[test]
fn the_include_of_ours_counts_only_among_mako_s_global_options() {
    let home = Path::new("/home/u");
    let files = Files::new(Some(home.to_owned()), &home.join(".config"));
    assert_eq!(
        files.mako_include_line(),
        "include=~/.config/mako/sway-things"
    );
    assert_eq!(
        files.inclusion_in("font=x\ninclude=~/.config/mako/sway-things\n[urgency=low]\n"),
        Inclusion::Included
    );
    assert_eq!(
        files.inclusion_in("include=/home/u/.config/mako/sway-things\n"),
        Inclusion::Included
    );
    assert_eq!(
        files.inclusion_in("[urgency=low]\ninclude=~/.config/mako/sway-things\n"),
        Inclusion::InSection
    );
    assert_eq!(
        files.inclusion_in("include=~/.config/mako/other\n"),
        Inclusion::Missing
    );
    // mako refuses blanks around `=` and paths it can't expand, and with them the config.
    assert_eq!(
        files.inclusion_in("include = ~/.config/mako/sway-things\n"),
        Inclusion::Malformed
    );
    assert_eq!(
        files.inclusion_in("include=$XDG_CONFIG_HOME/mako/sway-things\n"),
        Inclusion::Malformed
    );
}

#[test]
fn ours_is_written_a_missing_mako_config_created_and_a_foreign_file_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let files = Files::new(Some(dir.path().to_owned()), &dir.path().join(".config"));
    let text = format!("# {HEADER}\nwidth=1\n");
    assert!(files.write_mako(&text, true).unwrap());
    assert_eq!(std::fs::read_to_string(&files.mako_ours).unwrap(), text);
    let config = std::fs::read_to_string(&files.mako_config).unwrap();
    assert!(
        config.ends_with("\ninclude=~/.config/mako/sway-things\n"),
        "{config}"
    );
    assert_eq!(files.mako_inclusion(), Inclusion::Included);
    assert!(!files.write_mako(&text, true).unwrap());
    // The user's config is never written again, whatever it says.
    std::fs::write(&files.mako_config, "font=x\n").unwrap();
    files
        .write_mako(&format!("# {HEADER}\nwidth=2\n"), true)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&files.mako_config).unwrap(),
        "font=x\n"
    );
    // A file of the same name that isn't ours stays as it is.
    std::fs::write(&files.mako_ours, "width=9\n").unwrap();
    assert!(files.mako_is_foreign());
    assert!(files.write_mako(&text, true).is_err());
    assert_eq!(
        std::fs::read_to_string(&files.mako_ours).unwrap(),
        "width=9\n"
    );
}

fn base() -> serde_json::Map<String, Json> {
    serde_json::from_str(include_str!("swaync-config.json")).unwrap()
}

#[test]
fn swaync_gets_its_own_default_while_nothing_is_stored() {
    let dir = tempfile::tempdir().unwrap();
    let empty = state(dir.path(), &[]);
    let text = swaync::config(&Stored::new(&empty), base());
    let written: serde_json::Map<String, Json> = serde_json::from_str(&text).unwrap();
    let mut expected = base();
    expected.insert("$comment".into(), Json::from(HEADER));
    assert_eq!(written, expected);
    assert_eq!(
        swaync::style(&Stored::new(&empty), swaync::Gtk::Four),
        format!(
            "/* {HEADER}\n   swaync loads its own style sheet first; this goes on top of it. */\n"
        )
    );
}

#[test]
fn swaync_gets_the_stored_settings_on_top_of_its_default() {
    let dir = tempfile::tempdir().unwrap();
    let set = state(
        dir.path(),
        &[
            (keys::POSITION, choice("bottom-left")),
            (keys::OUTPUT, Value::Text(String::new())),
            (swaync_keys::TIMEOUT, Value::Integer(6)),
            (swaync_keys::GROUPING, Value::Bool(false)),
            (
                swaync_keys::WIDGETS,
                Value::List(vec!["title".into(), "buttons-grid".into(), "mpris".into()]),
            ),
            (
                swaync_keys::BUTTONS,
                Value::List(vec!["dnd".into(), "wifi".into()]),
            ),
            (
                swaync_keys::OTHER,
                Value::Text(r#"{"cssPriority": "application", "text-empty": "Nothing"}"#.into()),
            ),
            (keys::CLICK_FOCUS, Value::Bool(true)),
            (swaync_keys::STYLE, choice("frosted")),
            (swaync_keys::CORNER_RADIUS, Value::Integer(10)),
            (keys::COLORS, choice("custom")),
        ],
    );
    let text = swaync::config(&Stored::new(&set), base());
    let config: Json = serde_json::from_str(&text).unwrap();
    assert_eq!(config["$comment"], Json::from(HEADER));
    assert!(config.get("notification-window-preferred-output").is_none());
    assert_eq!(config["positionX"], "left");
    assert_eq!(config["positionY"], "bottom");
    assert_eq!(config["timeout"], 6);
    assert_eq!(config["timeout-low"], 5);
    assert_eq!(config["notification-grouping"], false);
    assert_eq!(config["cssPriority"], "application");
    assert_eq!(config["text-empty"], "Nothing");
    assert_eq!(
        config["widgets"],
        serde_json::json!(["title", "buttons-grid", "mpris"])
    );
    let grid = &config["widget-config"]["buttons-grid"];
    assert_eq!(grid["buttons-per-row"], 2);
    assert_eq!(grid["actions"][1]["command"], "sway-things hook quick wifi");
    assert_eq!(
        grid["actions"][1]["update-command"],
        "sway-things hook quick wifi state"
    );
    // The others of swaync's widget-config stay.
    assert_eq!(config["widget-config"]["title"]["text"], "Notifications");
    let click = &config["scripts"][swaync::CLICK_SCRIPT];
    assert_eq!(click["exec"], "sway-things hook notification-click");
    assert_eq!(click["run-on"], "action");
    let style = swaync::style(&Stored::new(&set), swaync::Gtk::Four);
    assert!(style.contains(".control-center"), "{style}");
    assert!(!style.contains("SPDX"), "{style}");
    assert!(
        style.contains("\n@define-color noti-bg rgba(40, 85, 119, 1.00);\n"),
        "{style}"
    );
    assert!(style.ends_with(
        ":root {\n  --noti-bg: 40, 85, 119;\n  --noti-bg-alpha: 1.00;\n  --text-color: rgba(255, \
         255, 255, 1.00);\n  --noti-border-color: rgba(76, 120, 153, 1.00);\n  --bg-selected: \
         rgba(76, 120, 153, 1.00);\n  --border-radius: 10px;\n}\n"
    ), "{style}");
}

/// A swaync config like one set up by hand: other positions and sizes, shorter timeouts, own
/// widgets with a button grid of its own, and a script.
const HAND_MADE: &str = r#"{
  "$schema": "/etc/xdg/swaync/configSchema.json",
  "positionX": "right",
  "positionY": "top",
  "layer": "top",
  "control-center-layer": "overlay",
  "control-center-margin-top": 8,
  "control-center-width": 420,
  "control-center-height": 700,
  "notification-window-width": 400,
  "notification-icon-size": 48,
  "notification-grouping": true,
  "timeout": 6,
  "timeout-low": 4,
  "timeout-critical": 0,
  "hide-on-action": false,
  "widgets": ["title#quick", "buttons-grid", "title", "notifications", "mpris"],
  "widget-config": {
    "title#quick": { "text": "Quick settings", "clear-all-button": false },
    "buttons-grid": {
      "buttons-per-row": 3,
      "actions": [{ "label": "Wi-Fi", "type": "toggle", "command": "my-wifi-toggle" }]
    }
  },
  "scripts": {
    "focus": { "exec": "my-focus-script", "app-name": ".*", "run-on": "action" }
  }
}"#;

#[test]
fn a_hand_made_swaync_config_is_a_starting_point() {
    let imported = swaync::import(HAND_MADE, &base()).unwrap();
    let values: Vec<(&str, Value)> = imported.values.clone();
    assert_eq!(
        values,
        [
            (swaync_keys::PANEL_HEIGHT, Value::Integer(700)),
            (swaync_keys::PANEL_WIDTH, Value::Integer(420)),
            (swaync_keys::WIDTH, Value::Integer(400)),
            (swaync_keys::TIMEOUT, Value::Integer(6)),
            (swaync_keys::TIMEOUT_LOW, Value::Integer(4)),
            (
                swaync_keys::WIDGETS,
                Value::List(
                    [
                        "title#quick",
                        "buttons-grid",
                        "title",
                        "notifications",
                        "mpris"
                    ]
                    .map(String::from)
                    .to_vec()
                )
            ),
        ]
    );
    let other: serde_json::Map<String, Json> = serde_json::from_str(&imported.other).unwrap();
    let mut names: Vec<&str> = other.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "control-center-layer",
            "control-center-margin-top",
            "hide-on-action",
            "layer",
            "notification-icon-size",
            "scripts",
            "widget-config",
        ]
    );
    // Stored, it gives back what the config said.
    let dir = tempfile::tempdir().unwrap();
    let mut values = imported.values.clone();
    values.push((swaync_keys::OTHER, Value::Text(imported.other.clone())));
    let state = state(dir.path(), &values);
    let written: Json =
        serde_json::from_str(&swaync::config(&Stored::new(&state), base())).unwrap();
    let original: Json = serde_json::from_str(HAND_MADE).unwrap();
    for (key, value) in original.as_object().unwrap() {
        if key != "$schema" {
            assert_eq!(&written[key], value, "{key}");
        }
    }
    assert!(swaync::import("[1]", &base()).is_err());
}

#[test]
fn the_daemons_started_by_sway_lines_are_found() {
    assert_eq!(starts::daemon_in("mako"), Some(Daemon::Mako));
    assert_eq!(
        starts::daemon_in("/usr/bin/swaync -c ~/x.json"),
        Some(Daemon::Swaync)
    );
    assert_eq!(
        starts::daemon_in("sh -c 'sleep 1; dunst'"),
        Some(Daemon::Dunst)
    );
    assert_eq!(starts::daemon_in("swaync-client -t"), None);
    assert_eq!(starts::daemon_in("makoctl reload"), None);
    assert_eq!(
        starts::service_daemon(
            "[D-BUS Service]\nName=org.freedesktop.Notifications\nExec=/usr/bin/mako\n"
        ),
        Some(Daemon::Mako)
    );
    assert_eq!(
        starts::service_daemon(
            "[D-BUS Service]\nName=org.erikreider.swaync.cc\nExec=/usr/bin/swaync\n"
        ),
        None
    );
}

#[test]
fn daemons_are_told_apart_by_the_name_they_give() {
    assert_eq!(Daemon::from_server_name("mako"), Daemon::Mako);
    assert_eq!(
        Daemon::from_server_name("SwayNotificationCenter"),
        Daemon::Swaync
    );
    assert_eq!(Daemon::from_server_name("dunst"), Daemon::Dunst);
    assert_eq!(Daemon::from_server_name("fnott"), Daemon::Other);
}

#[test]
fn swaync_0_11_gets_named_colors_only() {
    assert_eq!(swaync::Gtk::of_version("0.11.0"), swaync::Gtk::Three);
    assert_eq!(swaync::Gtk::of_version("0.12.6"), swaync::Gtk::Four);
    assert_eq!(swaync::Gtk::of_version("1.0"), swaync::Gtk::Four);
    assert_eq!(swaync::Gtk::of_version("?"), swaync::Gtk::Four);
    assert_eq!(swaync::Gtk::of_version("v0.11"), swaync::Gtk::Three);
    assert_eq!(swaync::Gtk::of_version(" 0.10.1-2 "), swaync::Gtk::Three);
    let dir = tempfile::tempdir().unwrap();
    let set = state(
        dir.path(),
        &[
            (swaync_keys::STYLE, choice("frosted")),
            (swaync_keys::CORNER_RADIUS, Value::Integer(10)),
            (keys::COLORS, choice("custom")),
        ],
    );
    let style = swaync::style(&Stored::new(&set), swaync::Gtk::Three);
    assert!(style.contains("@define-color noti-bg "), "{style}");
    assert!(!style.contains("--"), "{style}");
    assert!(!style.contains(".control-center"), "{style}");
}

#[test]
fn a_mako_config_is_only_created_for_a_mako_chosen_without_a_config_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let files = Files::new(Some(dir.path().to_owned()), &dir.path().join(".config"));
    let text = format!("# {HEADER}\n");
    // Not chosen on the page: only ours is written.
    files.write_mako(&text, false).unwrap();
    assert!(files.mako_ours.is_file());
    assert!(!files.mako_config.exists());
    // Started with `-c`: that config is mako's, and it is never created.
    let given = files
        .clone()
        .with_mako_config(dir.path().join("elsewhere/mako.conf"));
    assert!(!given.may_create_mako_config());
    given.write_mako(&text, true).unwrap();
    assert!(!dir.path().join("elsewhere/mako.conf").exists());
    assert!(files.may_create_mako_config());
}

#[test]
fn our_mako_file_s_place_behind_a_symbolic_link_is_told() {
    let dir = tempfile::tempdir().unwrap();
    let dotfiles = dir.path().join("dotfiles/mako");
    std::fs::create_dir_all(&dotfiles).unwrap();
    std::fs::create_dir_all(dir.path().join(".config")).unwrap();
    std::os::unix::fs::symlink(&dotfiles, dir.path().join(".config/mako")).unwrap();
    let files = Files::new(Some(dir.path().to_owned()), &dir.path().join(".config"));
    assert_eq!(
        files.mako_ours_target(),
        Some(
            std::fs::canonicalize(&dotfiles)
                .unwrap()
                .join("sway-things")
        )
    );
    let plain = tempfile::tempdir().unwrap();
    let files = Files::new(Some(plain.path().to_owned()), &plain.path().join(".config"));
    assert_eq!(files.mako_ours_target(), None);
}

#[test]
fn a_daemon_counts_only_where_it_is_the_program_run() {
    assert_eq!(starts::daemon_in("pkill mako"), None);
    assert_eq!(starts::daemon_in("systemctl --user restart mako"), None);
    assert_eq!(
        starts::daemon_in("killall swaync; swaync"),
        Some(Daemon::Swaync)
    );
    assert_eq!(
        starts::daemon_in("env FOO=1 mako -c ~/m"),
        Some(Daemon::Mako)
    );
    assert_eq!(
        starts::daemon_in("sh -c 'sleep 2 && dunst'"),
        Some(Daemon::Dunst)
    );
    assert_eq!(starts::daemon_in("notify-send mako"), None);
    let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        starts::config_in(&args(&["mako", "-c", "/m/config"])),
        Some("/m/config".into())
    );
    assert_eq!(
        starts::config_in(&args(&["swaync", "--config=/s.json"])),
        Some("/s.json".into())
    );
    assert_eq!(starts::config_in(&args(&["mako"])), None);
}
