// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::manifest::HEADER;
use crate::registry::Registry;
use crate::state::{Paths, State};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn portal(data: &Path, name: &str, interfaces: &str, use_in: &str) {
    write(
        &data.join(format!("xdg-desktop-portal/portals/{name}.portal")),
        &format!(
            "[portal]\nDBusName=org.freedesktop.impl.portal.desktop.{name}\n\
             Interfaces={interfaces}\nUseIn={use_in}\n"
        ),
    );
}

fn wlr_and_gtk(data: &Path) {
    portal(
        data,
        "wlr",
        "org.freedesktop.impl.portal.Screenshot;org.freedesktop.impl.portal.ScreenCast;",
        "wlroots;sway;Wayfire;river;phosh;Hyprland;",
    );
    portal(
        data,
        "gtk",
        "org.freedesktop.impl.portal.FileChooser;org.freedesktop.impl.portal.Settings;\
         org.freedesktop.impl.portal.Screenshot;",
        "gnome",
    );
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn dirs(root: &Path) -> Dirs {
    Dirs {
        config_home: root.join("home/.config"),
        config_dirs: vec![root.join("etc/xdg")],
        data_home: root.join("home/.local/share"),
        data_dirs: vec![root.join("usr/local/share"), root.join("usr/share")],
        desktops: strings(&["sway"]),
        path: None,
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
fn backends_are_read_from_their_portal_files_and_the_first_directory_wins() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    wlr_and_gtk(&second);
    portal(
        &first,
        "wlr",
        "org.freedesktop.impl.portal.ScreenCast",
        "sway",
    );
    write(
        &first.join("xdg-desktop-portal/portals/README"),
        "not a portal",
    );

    let portals = |data: &Path| data.join("xdg-desktop-portal/portals");
    let found = backends(&[portals(&first), portals(&second)]);
    assert_eq!(
        found,
        [
            Backend {
                name: "gtk".to_owned(),
                interfaces: strings(&["FileChooser", "Settings", "Screenshot"]),
                use_in: strings(&["gnome"]),
            },
            Backend {
                name: "wlr".to_owned(),
                interfaces: strings(&["ScreenCast"]),
                use_in: strings(&["sway"]),
            },
        ]
    );
}

#[test]
fn a_component_is_installed_with_its_portal_file_and_its_program() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("share");
    let program = root.path().join("libexec/xdg-desktop-portal-wlr");
    let service = |name: &str, exec: &Path| {
        write(
            &data.join(format!("dbus-1/services/{name}.service")),
            &format!(
                "[D-BUS Service]\nName={name}\nExec={} --flag\n",
                exec.display()
            ),
        );
    };
    let data_dirs = [data.clone()];
    let backends = |data: &[PathBuf]| backends(&[data[0].join("xdg-desktop-portal/portals")]);

    // Neither portal file nor service.
    assert!(!installed(
        Component::Wlr,
        &data_dirs,
        &backends(&data_dirs)
    ));
    // The portal file alone isn't enough: the program has to be there.
    wlr_and_gtk(&data);
    service("org.freedesktop.impl.portal.desktop.wlr", &program);
    assert!(!installed(
        Component::Wlr,
        &data_dirs,
        &backends(&data_dirs)
    ));
    write(&program, "");
    assert!(installed(Component::Wlr, &data_dirs, &backends(&data_dirs)));
    // xdg-desktop-portal itself has no portal file.
    let portal_program = root.path().join("libexec/xdg-desktop-portal");
    write(&portal_program, "");
    service("org.freedesktop.portal.Desktop", &portal_program);
    assert!(installed(
        Component::Portal,
        &data_dirs,
        &backends(&data_dirs)
    ));
    assert!(!installed(
        Component::Gtk,
        &data_dirs,
        &backends(&data_dirs)
    ));
}

#[test]
fn the_configuration_is_searched_like_xdg_desktop_portal_does() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs(root.path());
    assert_eq!(
        config_dirs(&dirs, Some(Path::new("/test"))),
        [PathBuf::from("/test")]
    );
    let search = config_dirs(&dirs, None);
    assert_eq!(
        search,
        [
            root.path().join("home/.config/xdg-desktop-portal"),
            root.path().join("etc/xdg/xdg-desktop-portal"),
            PathBuf::from("/etc/xdg-desktop-portal"),
            root.path().join("home/.local/share/xdg-desktop-portal"),
            root.path().join("usr/local/share/xdg-desktop-portal"),
            root.path().join("usr/share/xdg-desktop-portal"),
            PathBuf::from("/usr/share/xdg-desktop-portal"),
        ]
    );
    let search: Vec<PathBuf> = search
        .into_iter()
        .filter(|dir| dir.starts_with(root.path()))
        .collect();
    let desktops = strings(&["sway", "wlroots"]);

    assert_eq!(winning_config(&search, &desktops), None);
    write(&search[4].join("wlroots-portals.conf"), "");
    assert_eq!(
        winning_config(&search, &desktops),
        Some(search[4].join("wlroots-portals.conf"))
    );
    // The first directory that has one decides, even with a portals.conf only.
    write(&search[1].join("portals.conf"), "");
    assert_eq!(
        winning_config(&search, &desktops),
        Some(search[1].join("portals.conf"))
    );
    // In a directory, the first desktop's file wins over the later one's and over portals.conf.
    write(&search[1].join("wlroots-portals.conf"), "");
    assert_eq!(
        winning_config(&search, &desktops),
        Some(search[1].join("wlroots-portals.conf"))
    );
    write(&search[1].join("sway-portals.conf"), "");
    assert_eq!(
        winning_config(&search, &desktops),
        Some(search[1].join("sway-portals.conf"))
    );
    // XDG_CURRENT_DESKTOP's case doesn't matter.
    assert_eq!(
        winning_config(&search, &strings(&["SWAY"])),
        Some(search[1].join("sway-portals.conf"))
    );
}

#[test]
fn the_preferred_backends_are_read_from_the_configuration() {
    let choices = preferred(
        "# comment\n[other]\ndefault=none\n[preferred]\ndefault=gtk\n\
         org.freedesktop.impl.portal.ScreenCast=wlr;gtk;\nSettings = *\n",
    );
    assert_eq!(
        choices,
        HashMap::from([
            ("default".to_owned(), strings(&["gtk"])),
            ("ScreenCast".to_owned(), strings(&["wlr", "gtk"])),
            ("Settings".to_owned(), strings(&["*"])),
        ])
    );
}

#[test]
fn the_backend_of_an_interface_follows_the_configuration_or_use_in() {
    let root = tempfile::tempdir().unwrap();
    wlr_and_gtk(root.path());
    let backends = backends(&portal_dirs(
        &dirs(root.path()),
        Some(&root.path().join("xdg-desktop-portal/portals")),
    ));
    let sway = strings(&["sway"]);
    let backend = |interface: &str, config: Option<&str>| {
        let choices = config.map(preferred);
        backend_for(interface, choices.as_ref(), &backends, &sway)
    };

    // Without a configuration: the backends meant for the desktop, else gtk as the last resort.
    assert_eq!(backend("ScreenCast", None), Some("wlr".to_owned()));
    assert_eq!(backend("FileChooser", None), Some("gtk".to_owned()));
    // sway's own configuration.
    let sways = "[preferred]\ndefault=gtk\norg.freedesktop.impl.portal.ScreenCast=wlr\n\
                 org.freedesktop.impl.portal.Screenshot=wlr\n";
    assert_eq!(backend("ScreenCast", Some(sways)), Some("wlr".to_owned()));
    assert_eq!(backend("Screenshot", Some(sways)), Some("wlr".to_owned()));
    assert_eq!(backend("FileChooser", Some(sways)), Some("gtk".to_owned()));
    assert_eq!(backend("Settings", Some(sways)), Some("gtk".to_owned()));
    // A backend that doesn't implement the interface is skipped; none means none, wherever it
    // is in the list.
    let wrong = "[preferred]\ndefault=wlr;gtk\nSettings=gtk;none\n";
    assert_eq!(backend("FileChooser", Some(wrong)), Some("gtk".to_owned()));
    assert_eq!(backend("Settings", Some(wrong)), None);
    assert_eq!(
        backend("ScreenCast", Some("[preferred]\ndefault=*\n")),
        Some("wlr".to_owned())
    );
    // A default of none counts only for the interfaces without a line of their own.
    let quiet = "[preferred]\ndefault=none\nScreenCast=wlr\n";
    assert_eq!(backend("ScreenCast", Some(quiet)), Some("wlr".to_owned()));
    assert_eq!(backend("FileChooser", Some(quiet)), None);
    // Where the configuration picks nothing, UseIn and gtk decide.
    let gnome = "[preferred]\ndefault=gnome\n";
    assert_eq!(backend("ScreenCast", Some(gnome)), Some("wlr".to_owned()));
    assert_eq!(backend("FileChooser", Some(gnome)), Some("gtk".to_owned()));
}

#[test]
fn backends_meant_for_the_desktop_come_first() {
    let backend = |name: &str, use_in: &[&str]| Backend {
        name: name.to_owned(),
        interfaces: strings(&["ScreenCast"]),
        use_in: strings(use_in),
    };
    let mut backends = vec![
        backend("a", &["gnome"]),
        backend("b", &["wlroots"]),
        backend("c", &["sway"]),
        backend("d", &[]),
    ];
    sort_backends(&mut backends, &strings(&["sway", "wlroots"]));
    let names: Vec<&str> = backends
        .iter()
        .map(|backend| backend.name.as_str())
        .collect();
    assert_eq!(names, ["c", "b", "a", "d"]);
    let any = "[preferred]\ndefault=*\n";
    assert_eq!(
        backend_for(
            "ScreenCast",
            Some(&preferred(any)),
            &backends,
            &strings(&["sway"])
        ),
        Some("c".to_owned())
    );
}

#[test]
fn the_check_says_what_needs_a_fix() {
    let served = |screencast: Option<&str>, file_chooser: Option<&str>| Check {
        served: vec![
            ("ScreenCast", screencast.map(ToOwned::to_owned)),
            ("Screenshot", None),
            ("FileChooser", file_chooser.map(ToOwned::to_owned)),
            ("Settings", None),
        ],
        can_configure: true,
        ..Check::default()
    };
    let good = served(Some("wlr"), Some("gtk"));
    assert!(!good.config_wrong());
    assert!(good.ok());
    assert!(served(Some("gtk"), Some("gtk")).config_wrong());
    assert!(served(Some("wlr"), None).config_wrong());

    let missing = Check {
        missing: vec![Component::Wlr],
        ..good.clone()
    };
    assert!(!missing.ok());
    let variables = Check {
        session_variables: Some((true, false)),
        ..good.clone()
    };
    assert!(variables.variables_missing());
    assert!(!variables.ok());
    // What couldn't be asked doesn't count against it.
    assert!(good.session_variables.is_none() && good.running.is_none());
    let stopped = Check {
        running: Some(false),
        ..good
    };
    assert!(!stopped.ok());
}

#[test]
fn the_session_variables_have_to_be_set_and_not_empty() {
    let environment = HashMap::from([
        ("WAYLAND_DISPLAY".to_owned(), "wayland-1".to_owned()),
        ("XDG_CURRENT_DESKTOP".to_owned(), String::new()),
    ]);
    assert_eq!(session_variables(&environment), (true, false));
    assert_eq!(session_variables(&HashMap::new()), (false, false));
}

#[test]
fn our_configuration_is_wanted_only_where_the_user_has_none() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs(root.path());
    let ours = path(&dirs);
    assert_eq!(
        ours,
        root.path()
            .join("home/.config/xdg-desktop-portal/sway-portals.conf")
    );
    assert!(text().starts_with(HEADER));
    assert!(text().contains("org.freedesktop.impl.portal.ScreenCast=wlr\n"));

    let (_off_dir, off) = load("");
    assert!(!enabled(&off));
    assert_eq!(wanted(&off, &dirs).text, None);
    let (_on_dir, on) = load("[environment]\nportal_config = true\n");
    assert!(enabled(&on));
    assert_eq!(wanted(&on, &dirs).text, Some(text()));
    assert!(look(&dirs).can_configure);

    // The user's portals.conf would lose against ours: nothing is written then.
    write(
        &ours.with_file_name("portals.conf"),
        "[preferred]\ndefault=gtk\n",
    );
    assert_eq!(wanted(&on, &dirs).text, None);
    let check = look(&dirs);
    assert!(!check.can_configure);
    assert_eq!(
        check.users_config,
        Some(ours.with_file_name("portals.conf"))
    );
    fs::remove_file(ours.with_file_name("portals.conf")).unwrap();

    // So would another desktop's of XDG_CURRENT_DESKTOP; one of a desktop that isn't doesn't
    // count.
    let mut wlroots = dirs.clone();
    wlroots.desktops = strings(&["sway", "wlroots"]);
    write(&ours.with_file_name("wlroots-portals.conf"), "");
    write(&ours.with_file_name("gnome-portals.conf"), "");
    assert_eq!(wanted(&on, &dirs).text, Some(text()));
    assert_eq!(wanted(&on, &wlroots).text, None);
    assert_eq!(
        users_config(&wlroots),
        Some(ours.with_file_name("wlroots-portals.conf"))
    );

    // The user's own sway-portals.conf is never ours.
    write(&ours, "[preferred]\ndefault=wlr\n");
    assert_eq!(wanted(&on, &dirs).text, None);
    assert_eq!(look(&dirs).users_config, Some(ours.clone()));
    // Ours is.
    write(&ours, &text());
    assert_eq!(wanted(&on, &dirs).text, Some(text()));
    let check = look(&dirs);
    assert!(check.can_configure);
    assert_eq!(check.users_config, None);
}
