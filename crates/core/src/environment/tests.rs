// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::session::{self, Session};
use super::{line, path, quote, text, variables, wanted};
use crate::apps::Dirs;
use crate::generate::other::{Problem, keep};
use crate::generate::{HEADER, Manifest};
use crate::registry::environment::{
    ELECTRON_WAYLAND, JAVA_NONREPARENTING, QT_WAYLAND, SDL_WAYLAND, is_variable_name,
};
use crate::registry::{Registry, Variable};
use crate::state::{Paths, State};

fn load(toml: &str) -> (tempfile::TempDir, State) {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("settings.toml"), format!("version = 1\n{toml}")).unwrap();
    let paths = Paths::new(&config, &dir.path().join("state"), "host").unwrap();
    let state = State::load(Registry::builtin(), paths);
    assert_eq!(state.problems(), []);
    (dir, state)
}

fn dirs(root: &std::path::Path) -> Dirs {
    Dirs::with(|name| match name {
        "HOME" => Some(root.join("home").display().to_string()),
        _ => None,
    })
    .unwrap()
}

#[test]
fn values_are_quoted_only_where_needed() {
    assert_eq!(quote("1"), "1");
    assert_eq!(quote("wayland,x11"), "wayland,x11");
    assert_eq!(quote("/usr/bin:/bin"), "/usr/bin:/bin");
    assert_eq!(quote("wayland;xcb"), "\"wayland;xcb\"");
    assert_eq!(quote(""), "\"\"");
    assert_eq!(quote("a b"), "\"a b\"");
    assert_eq!(
        quote("$HOME \"x\" \\ `y`"),
        "\"$$HOME \\\"x\\\" \\\\ \\`y\\`\""
    );
    let raw = Variable {
        name: "PATH".to_owned(),
        value: "$HOME/bin:$PATH".to_owned(),
        raw: true,
    };
    assert_eq!(line(&raw), "PATH=$HOME/bin:$PATH");
}

#[test]
fn names_are_checked() {
    assert!(is_variable_name("_JAVA_AWT_WM_NONREPARENTING"));
    assert!(is_variable_name("LC_ALL"));
    assert!(!is_variable_name(""));
    assert!(!is_variable_name("1X"));
    assert!(!is_variable_name("A-B"));
    assert!(!is_variable_name("Ä"));
}

#[test]
fn the_switches_write_their_variables_with_a_fallback() {
    let (_dir, state) = load(
        "[environment]\nqt_wayland = true\nsdl_wayland = true\njava_nonreparenting = true\n\
         electron_wayland = true\n",
    );
    let text = text(&variables(Registry::builtin(), &state)).unwrap();
    assert_eq!(
        text,
        format!(
            "{HEADER}QT_QPA_PLATFORM=\"wayland;xcb\"\nSDL_VIDEODRIVER=wayland,x11\n\
             _JAVA_AWT_WM_NONREPARENTING=1\nELECTRON_OZONE_PLATFORM_HINT=auto\n"
        )
    );
    let (_dir, nothing) = load("");
    assert_eq!(variables(Registry::builtin(), &nothing), []);
    assert_eq!(text_of(&nothing), None);
}

fn text_of(state: &State) -> Option<String> {
    text(&variables(Registry::builtin(), state))
}

#[test]
fn every_variable_has_one_owner() {
    let registry = Registry::builtin();
    for (key, name) in [
        (QT_WAYLAND, "QT_QPA_PLATFORM"),
        (SDL_WAYLAND, "SDL_VIDEODRIVER"),
        (JAVA_NONREPARENTING, "_JAVA_AWT_WM_NONREPARENTING"),
        (ELECTRON_WAYLAND, "ELECTRON_OZONE_PLATFORM_HINT"),
    ] {
        assert_eq!(
            registry
                .environment_owner(name)
                .map(crate::registry::Setting::key),
            Some(key)
        );
    }
    assert!(registry.environment_owner("EDITOR").is_none());
    let mut names = Vec::new();
    for setting in registry.iter() {
        if let Some(mapping) = setting.environment() {
            for name in mapping.names() {
                let bare = name.strip_suffix('*').unwrap_or(name);
                assert!(is_variable_name(bare), "{name}");
                assert!(!names.contains(name), "{name} has two owners");
                names.push(name);
            }
        }
    }
}

#[test]
fn the_file_is_only_written_while_it_is_ours() {
    let dir = tempfile::tempdir().unwrap();
    let dirs = dirs(dir.path());
    let file = path(&dirs);
    assert_eq!(
        file,
        dir.path()
            .join("home/.config/environment.d/50-sway-things.conf")
    );
    let manifest_path = dir.path().join("state/manifest.toml");
    let (_a, on) = load("[environment]\njava_nonreparenting = true\n");
    let (_b, off) = load("");

    let mut manifest = Manifest::load(&manifest_path);
    assert_eq!(keep(&[wanted(&on, &dirs)], &mut manifest), []);
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .ends_with("\n_JAVA_AWT_WM_NONREPARENTING=1\n")
    );
    // No variable left: the file goes.
    assert_eq!(keep(&[wanted(&off, &dirs)], &mut manifest), []);
    assert!(!file.exists());

    // Someone else's file of that name is never touched.
    fs::write(&file, "EDITOR=vim\n").unwrap();
    assert_eq!(
        keep(&[wanted(&on, &dirs)], &mut manifest),
        [Problem::Foreign(file.clone())]
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "EDITOR=vim\n");
    fs::remove_file(&file).unwrap();

    // Ours, changed by hand: left as it is.
    assert_eq!(keep(&[wanted(&on, &dirs)], &mut manifest), []);
    fs::write(&file, format!("{HEADER}EDITOR=vim\n")).unwrap();
    assert_eq!(
        keep(&[wanted(&off, &dirs)], &mut manifest),
        [Problem::Edited(file.clone())]
    );
    assert!(file.exists());
}

#[test]
fn the_socket_names_sways_process() {
    assert_eq!(
        session::pid_of(&PathBuf::from("/run/user/1000/sway-ipc.1000.4242.sock")),
        Some(4242)
    );
    assert_eq!(session::pid_of(&PathBuf::from("/tmp/i3-ipc.sock")), None);
    assert_eq!(
        session::pid_of(&PathBuf::from("/run/user/1000/sway-ipc.1000.x.sock")),
        None
    );
    let environ = session::parse_environ(b"A=1\0B=x=y\0broken\0\0");
    assert_eq!(environ.get("A").map(String::as_str), Some("1"));
    assert_eq!(environ.get("B").map(String::as_str), Some("x=y"));
    assert_eq!(environ.len(), 2);
}

#[test]
fn sways_environment_is_read_from_proc() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("sway-ipc.1000.77.sock");
    fs::write(&socket, "").unwrap();
    fs::create_dir_all(dir.path().join("proc/77")).unwrap();
    fs::write(
        dir.path().join("proc/77/environ"),
        b"QT_QPA_PLATFORM=wayland;xcb\0",
    )
    .unwrap();
    let session = Session::at(&socket, &dir.path().join("proc")).unwrap();
    assert_eq!(
        session.variables.get("QT_QPA_PLATFORM").map(String::as_str),
        Some("wayland;xcb")
    );
    assert!(Session::at(&dir.path().join("sway-ipc.1000.78.sock"), dir.path()).is_none());
}

#[test]
fn a_session_that_didnt_read_the_file_is_noticed() {
    let started = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
    let ours = [
        Variable::literal("SDL_VIDEODRIVER", "wayland,x11"),
        Variable::literal("_JAVA_AWT_WM_NONREPARENTING", "1"),
        Variable {
            name: "PATH".to_owned(),
            value: "$HOME/bin:$PATH".to_owned(),
            raw: true,
        },
    ];
    let session = Session {
        variables: HashMap::from([("SDL_VIDEODRIVER".to_owned(), "wayland,x11".to_owned())]),
        started,
    };
    let before = started - Duration::from_secs(1);
    assert_eq!(
        session::not_read(&ours, before, &session),
        ["_JAVA_AWT_WM_NONREPARENTING"]
    );
    // Written since the login: the change waits for the next one.
    assert_eq!(
        session::not_read(&ours, started + Duration::from_secs(1), &session),
        Vec::<String>::new()
    );
    let read = Session {
        variables: HashMap::from([
            ("SDL_VIDEODRIVER".to_owned(), "wayland,x11".to_owned()),
            ("_JAVA_AWT_WM_NONREPARENTING".to_owned(), "1".to_owned()),
        ]),
        started,
    };
    assert_eq!(
        session::not_read(&ours, before, &read),
        Vec::<String>::new()
    );
}

#[test]
fn the_import_line_runs_the_generator() {
    assert_eq!(
        session::import_line(&PathBuf::from("/usr/lib/gen")),
        "set -a; eval \"$(/usr/lib/gen)\"; set +a"
    );
}
