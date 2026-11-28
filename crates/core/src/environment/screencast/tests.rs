// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use super::*;
use crate::paths::Paths;
use crate::registry::Registry;
use crate::state::State;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// An executable `name` in `bin`.
fn program(bin: &Path, name: &str) {
    let path = bin.join(name);
    write(&path, "#!/bin/sh\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn dirs(root: &Path) -> Dirs {
    Dirs {
        config_home: root.join("home/.config"),
        config_dirs: vec![root.join("etc/xdg")],
        data_home: root.join("home/.local/share"),
        data_dirs: vec![root.join("usr/share")],
        desktops: vec!["sway".to_owned()],
        path: Some(root.join("bin").into_os_string()),
        languages: Vec::new(),
    }
}

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

#[test]
fn each_chooser_has_its_lines() {
    let fuzzel = MENUS.first().copied();
    assert_eq!(lines(Chooser::Default, "", true, fuzzel), None);
    assert_eq!(
        lines(Chooser::Slurp, "", true, None).as_deref(),
        Some("[screencast]\nchooser_type=simple\nchooser_cmd=slurp -f %o -or\n")
    );
    assert_eq!(
        lines(Chooser::Menu, "", false, fuzzel).as_deref(),
        Some("[screencast]\nchooser_type=dmenu\nchooser_cmd=fuzzel --dmenu\n")
    );
    assert_eq!(
        lines(Chooser::Output, "DP-1", false, None).as_deref(),
        Some("[screencast]\nchooser_type=none\noutput_name=DP-1\n")
    );
    // Without the tool or the display, no file: xdg-desktop-portal-wlr picks a chooser itself.
    assert_eq!(lines(Chooser::Slurp, "", false, fuzzel), None);
    assert_eq!(lines(Chooser::Menu, "", true, None), None);
    assert_eq!(lines(Chooser::Output, "", true, fuzzel), None);
    assert_eq!(lines(Chooser::Output, "DP-1; rm -rf ~", true, fuzzel), None);
}

#[test]
fn only_plain_output_names_are_taken() {
    for name in [
        "DP-1",
        "HEADLESS-1",
        "eDP-1",
        "HDMI-A-2",
        "Virtual.1",
        "a_b",
    ] {
        assert!(is_output_name(name), "{name}");
    }
    for name in [
        "",
        "DP 1",
        "DP-1\n",
        "$(x)",
        "a;b",
        "a'b",
        "é",
        &"x".repeat(65),
    ] {
        assert!(!is_output_name(name), "{name:?}");
    }
    let check = Registry::builtin().get(SCREENCAST_OUTPUT).unwrap();
    assert_eq!(check.check(&Value::Text(String::new())), Ok(()));
    assert_eq!(check.check(&Value::Text("DP-1".to_owned())), Ok(()));
    assert!(check.check(&Value::Text("DP-1 && x".to_owned())).is_err());
}

#[test]
fn the_first_launcher_installed_makes_the_menu() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs(root.path());
    let bin = root.path().join("bin");
    assert_eq!(menu(&dirs), None);
    assert!(!has_slurp(&dirs));
    program(&bin, "wmenu");
    program(&bin, "rofi");
    assert_eq!(menu(&dirs).map(|menu| menu.command), Some("rofi -dmenu"));
    program(&bin, "slurp");
    assert!(has_slurp(&dirs));
}

#[test]
fn the_file_xdpw_reads_is_found_in_its_order() {
    let root = tempfile::tempdir().unwrap();
    let mut dirs = dirs(root.path());
    dirs.desktops = vec!["sway".to_owned(), "wlroots".to_owned()];
    let system = root.path().join("etc/xdg");
    let user = |name: &str| dirs.config_home.join("xdg-desktop-portal-wlr").join(name);
    let etc = |name: &str| system.join("xdg-desktop-portal-wlr").join(name);

    assert_eq!(deciding_in(&dirs, &system), None);
    assert_eq!(shadowed_in(&dirs, &system), None);
    write(&etc("config"), "");
    assert_eq!(deciding_in(&dirs, &system), Some(etc("config")));
    assert_eq!(shadowed_in(&dirs, &system), Some(etc("config")));
    write(&etc("wlroots"), "");
    assert_eq!(shadowed_in(&dirs, &system), Some(etc("wlroots")));
    // Any file of the user's comes before the system's.
    write(&user("config"), "");
    assert_eq!(deciding_in(&dirs, &system), Some(user("config")));
    write(&user("wlroots"), "");
    assert_eq!(deciding_in(&dirs, &system), Some(user("wlroots")));
    write(&user("sway"), "");
    assert_eq!(deciding_in(&dirs, &system), Some(user("sway")));
}

#[test]
fn our_file_is_wanted_only_where_the_user_has_none() {
    let root = tempfile::tempdir().unwrap();
    let mut dirs = dirs(root.path());
    program(&root.path().join("bin"), "slurp");
    let ours = path(&dirs);
    assert_eq!(
        ours,
        root.path().join("home/.config/xdg-desktop-portal-wlr/sway")
    );

    let (_default_dir, default) = load("");
    assert_eq!(Chooser::of(&default), Chooser::Default);
    assert_eq!(wanted(&default, &dirs).text, None);
    let (_slurp_dir, slurp) = load("[environment]\nscreencast_chooser = \"slurp\"\n");
    assert_eq!(Chooser::of(&slurp), Chooser::Slurp);
    let text = wanted(&slurp, &dirs).text.unwrap();
    assert!(text.starts_with(HEADER), "{text}");
    assert!(text.ends_with("chooser_cmd=slurp -f %o -or\n"), "{text}");
    let (_output_dir, output) =
        load("[environment]\nscreencast_chooser = \"output\"\nscreencast_output = \"HDMI-A-1\"\n");
    assert!(
        wanted(&output, &dirs)
            .text
            .unwrap()
            .ends_with("chooser_type=none\noutput_name=HDMI-A-1\n")
    );

    // The user's config would be hidden by ours, and theirs for another desktop would hide it.
    let config = ours.with_file_name("config");
    write(&config, "[screencast]\n");
    assert_eq!(wanted(&slurp, &dirs).text, None);
    assert_eq!(users_config(&dirs), Some(config.clone()));
    fs::remove_file(&config).unwrap();
    dirs.desktops = vec!["wlroots".to_owned(), "sway".to_owned()];
    write(&ours.with_file_name("wlroots"), "");
    assert_eq!(wanted(&slurp, &dirs).text, None);
    fs::remove_file(ours.with_file_name("wlroots")).unwrap();
    // A sway of the user's is theirs; ours is ours.
    write(&ours, "[screencast]\n");
    assert_eq!(wanted(&slurp, &dirs).text, None);
    write(&ours, &text);
    assert_eq!(users_config(&dirs), None);
    assert_eq!(wanted(&slurp, &dirs).text, Some(text));
}
