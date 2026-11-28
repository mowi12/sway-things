// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::autostart::{self, Entry, Item, Place, Runner, Runners, Starts, Systemd};
use super::entry::{DesktopEntry, ExecError, locale_variants, split_exec};
use super::files::{self, Problem, Wanted};
use super::installed::{self, shows_in};
use super::launchers::{self, Binding, LAUNCHER, TERMINAL};
use super::mime::{self, Kind, Source};
use super::{Dirs, launch};
use crate::command::{SwayCmd, SwayText, Target, model};
use crate::manifest::{HEADER, Manifest};
use crate::paths::Paths;
use crate::registry::{Registry, ValueError};
use crate::state::State;

/// A home with its own XDG directories, a system data and config directory, and a `PATH`.
struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
    dirs: Dirs,
}

impl Home {
    fn new(desktops: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        let dirs = Dirs {
            config_home: root.join("home/.config"),
            config_dirs: vec![root.join("etc/xdg")],
            data_home: root.join("home/.local/share"),
            data_dirs: vec![root.join("usr/local/share"), root.join("usr/share")],
            desktops: desktops
                .iter()
                .map(|desktop| (*desktop).to_owned())
                .collect(),
            path: Some(root.join("bin").into_os_string()),
            languages: vec!["de_DE.UTF-8".to_owned()],
        };
        fs::create_dir_all(root.join("bin")).unwrap();
        Self {
            _dir: dir,
            root,
            dirs,
        }
    }

    /// Writes `text` to `path` below the root.
    fn write(&self, path: &str, text: &str) -> PathBuf {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    /// An executable `name` in `PATH`.
    fn program(&self, name: &str) {
        let path = self.write(&format!("bin/{name}"), "#!/bin/sh\n");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn app(&self, dir: &str, id_path: &str, keys: &str) -> PathBuf {
        self.write(
            &format!("{dir}/applications/{id_path}"),
            &format!("[Desktop Entry]\nType=Application\n{keys}\n"),
        )
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
fn desktop_entries_are_read_as_the_specification_says() {
    let entry = DesktopEntry::parse(
        "# a comment\nName=Ignored\n[Desktop Entry]\nName=Files\nName[de]=Dateien\n\
         Name[sr@latin]=Datoteke\nComment=Line\\sone\\nline two \\\\ \\q\n\
         MimeType=inode/directory;text/a\\;b;;\nTerminal=true\nNoDisplay=1\nHidden = false\n\
         [Desktop Action new]\nName=New Window\n",
    );
    assert_eq!(entry.string("Name").as_deref(), Some("Files"));
    assert_eq!(
        entry
            .localized("Name", &["de_AT.UTF-8".to_owned()])
            .as_deref(),
        Some("Dateien")
    );
    assert_eq!(
        entry
            .localized("Name", &["sr_RS@latin".to_owned()])
            .as_deref(),
        Some("Datoteke")
    );
    assert_eq!(
        entry.localized("Name", &["fr".to_owned()]).as_deref(),
        Some("Files")
    );
    assert_eq!(
        entry.string("Comment").as_deref(),
        Some("Line one\nline two \\ \\q")
    );
    assert_eq!(entry.list("MimeType"), ["inode/directory", "text/a;b"]);
    assert!(entry.boolean("Terminal"));
    assert!(entry.boolean("NoDisplay"));
    assert!(entry.is_false("Hidden"));
    assert!(!entry.boolean("Missing") && !entry.is_false("Missing"));
    assert_eq!(
        locale_variants("sr_RS.UTF-8@latin"),
        ["sr_RS@latin", "sr_RS", "sr@latin", "sr"]
    );
}

#[test]
fn the_command_line_is_split_and_its_field_codes_filled_in() {
    assert_eq!(
        split_exec(r#"app "a b" "c\"d\$e\\f" g\ h 'i j'"#).unwrap(),
        ["app", "a b", "c\"d$e\\f", "g h", "i j"]
    );
    assert_eq!(split_exec("app \"open"), Err(ExecError::Quoting));
    assert_eq!(split_exec("app 'open"), Err(ExecError::Quoting));
    let path = Path::new("/usr/share/applications/viewer.desktop");
    let entry = DesktopEntry::parse(
        "[Desktop Entry]\nName=Viewer\nName[de]=Betrachter\nIcon=viewer\n\
         Exec=viewer --name %c %U -x %f --file=%k 100%% %i \"%u\" --opt=%F\n",
    );
    assert_eq!(
        entry.command(path, &["de".to_owned()]).unwrap(),
        [
            "viewer",
            "--name",
            "Betrachter",
            "-x",
            "--file=/usr/share/applications/viewer.desktop",
            "100%",
            "--icon",
            "viewer",
            "--opt="
        ]
    );
    // The escapes of a string come first: `\\\\` is one backslash in the quoted argument.
    let escaped = DesktopEntry::parse("[Desktop Entry]\nExec=sh -c \"echo \\\\\\\\x\"\n");
    assert_eq!(
        escaped.command(path, &[]).unwrap(),
        ["sh", "-c", "echo \\x"]
    );
    let empty = DesktopEntry::parse("[Desktop Entry]\nExec=%U\n");
    assert_eq!(empty.command(path, &[]), Err(ExecError::Empty));
    let none = DesktopEntry::parse("[Desktop Entry]\nName=x\n");
    assert_eq!(none.command(path, &[]), Err(ExecError::Missing));
}

#[test]
fn installed_apps_take_the_first_directory_and_hidden_ones_go() {
    let home = Home::new(&["sway"]);
    home.program("present");
    home.app(
        "usr/share",
        "org.example.Viewer.desktop",
        "Name=System viewer\nExec=viewer",
    );
    home.app(
        "home/.local/share",
        "org.example.Viewer.desktop",
        "Name=My viewer\nExec=viewer",
    );
    home.app(
        "usr/share",
        "kde/konsole.desktop",
        "Name=Konsole\nExec=konsole",
    );
    home.app("usr/share", "gone.desktop", "Name=Gone\nExec=gone");
    home.app("usr/local/share", "gone.desktop", "Name=Gone\nHidden=true");
    home.app(
        "usr/share",
        "tryexec.desktop",
        "Name=Missing\nTryExec=absent\nExec=absent",
    );
    home.app(
        "usr/share",
        "tryexec-ok.desktop",
        "Name=Present\nTryExec=present\nExec=present",
    );
    home.write(
        "usr/share/applications/link.desktop",
        "[Desktop Entry]\nType=Link\nName=Link\nURL=https://example.org\n",
    );
    let apps = installed::installed(&home.dirs);
    let ids: Vec<(&str, &str)> = apps
        .iter()
        .map(|app| (app.id.as_str(), app.name.as_str()))
        .collect();
    assert_eq!(
        ids,
        [
            ("kde-konsole.desktop", "Konsole"),
            ("org.example.Viewer.desktop", "My viewer"),
            ("tryexec-ok.desktop", "Present"),
        ]
    );
    let konsole = installed::find(&home.dirs, "kde-konsole.desktop").unwrap();
    assert!(konsole.path.ends_with("applications/kde/konsole.desktop"));
    assert!(installed::find(&home.dirs, "gone.desktop").is_none());
    assert!(installed::find(&home.dirs, "tryexec.desktop").is_none());
    for outside in [
        "../x.desktop",
        "../../usr/share/applications/gone.desktop",
        "kde/konsole.desktop",
    ] {
        assert!(installed::find(&home.dirs, outside).is_none(), "{outside}");
    }
    // An ID of directories at any depth that is searched, with dashes in the names too.
    home.app(
        "usr/share",
        "a-b/c/d-e/f/g-h.desktop",
        "Name=Deep\nExec=present",
    );
    assert_eq!(
        installed::find(&home.dirs, "a-b-c-d-e-f-g-h.desktop").map(|app| app.name),
        Some("Deep".to_owned())
    );
    assert!(
        installed::installed(&home.dirs)
            .iter()
            .any(|app| app.id == "a-b-c-d-e-f-g-h.desktop")
    );
}

#[test]
fn only_and_not_show_in_follow_the_order_of_the_desktops() {
    let list = |names: &[&str]| {
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    let desktops = list(&["sway", "wlroots"]);
    assert!(shows_in(&[], &[], &desktops));
    assert!(shows_in(&list(&["wlroots"]), &[], &desktops));
    assert!(!shows_in(&list(&["GNOME"]), &[], &desktops));
    assert!(!shows_in(&[], &list(&["sway"]), &desktops));
    // The first desktop named decides.
    assert!(shows_in(&list(&["sway"]), &list(&["wlroots"]), &desktops));
    assert!(!shows_in(&list(&["wlroots"]), &list(&["sway"]), &desktops));
    assert!(!shows_in(&list(&["sway"]), &[], &[]));
}

/// A home with a browser, an editor and the lists of the user and the system.
fn mime_home(desktops: &[&str]) -> Home {
    let home = Home::new(desktops);
    home.app(
        "usr/share",
        "firefox.desktop",
        "Name=Firefox\nExec=firefox %u\nMimeType=x-scheme-handler/https;x-scheme-handler/http;text/html;",
    );
    home.app(
        "usr/share",
        "chromium.desktop",
        "Name=Chromium\nExec=chromium %U\nMimeType=x-scheme-handler/https;text/html;",
    );
    home.app(
        "usr/share",
        "imv.desktop",
        "Name=imv\nExec=imv %F\nMimeType=image/png;image/jpeg;",
    );
    home.write(
        "usr/share/applications/mimeapps.list",
        "[Default Applications]\nx-scheme-handler/https=chromium.desktop\n",
    );
    home.write(
        "home/.config/mimeapps.list",
        "[Default Applications]\nx-scheme-handler/https=not-installed.desktop;firefox.desktop;\n\
         [Added Associations]\nimage/png=imv.desktop;\n",
    );
    home
}

#[test]
fn the_first_list_that_names_an_installed_app_wins() {
    let home = mime_home(&["sway", "wlroots"]);
    let dirs = &home.dirs;
    let config = &dirs.config_home;
    assert_eq!(
        mime::list_paths(dirs)[..3],
        [
            config.join("sway-mimeapps.list"),
            config.join("wlroots-mimeapps.list"),
            config.join("mimeapps.list")
        ]
    );
    let apps = installed::installed(dirs);
    let https = mime::default_for(dirs, "x-scheme-handler/HTTPS", &apps, None).unwrap();
    assert_eq!(https.id, "firefox.desktop");
    assert_eq!(https.source, Source::List(config.join("mimeapps.list")));
    // Without a default, an added association, then an app that opens the type.
    let png = mime::default_for(dirs, "image/png", &apps, None).unwrap();
    assert_eq!(
        (png.id.as_str(), png.source),
        ("imv.desktop", Source::Fallback)
    );
    assert!(mime::default_for(dirs, "application/pdf", &apps, None).is_none());

    // Sway Things' list comes first while XDG_CURRENT_DESKTOP names sway.
    let ours = mime::ours(dirs);
    fs::write(
        &ours,
        "[Default Applications]\nx-scheme-handler/https=chromium.desktop;\n",
    )
    .unwrap();
    let shown = mime::shown(dirs, Kind::Browser, &apps);
    assert!(shown.ours_wins);
    assert_eq!(shown.now.unwrap().id, "chromium.desktop");
    assert_eq!(shown.without_ours.unwrap().id, "firefox.desktop");
    // Another desktop's list before sway's wins over it.
    home.write(
        "home/.config/a-mimeapps.list",
        "[Default Applications]\nx-scheme-handler/https=firefox.desktop;\n",
    );
    let first = Dirs {
        desktops: vec!["A".to_owned(), "sway".to_owned()],
        ..dirs.clone()
    };
    assert!(!mime::shown(&first, Kind::Browser, &apps).ours_wins);
    // Without sway in XDG_CURRENT_DESKTOP, nobody reads it.
    let other = Dirs {
        desktops: vec!["GNOME".to_owned()],
        ..dirs.clone()
    };
    assert!(!other.is_sway());
    let shown = mime::shown(&other, Kind::Browser, &apps);
    assert!(!shown.ours_wins);
    assert_eq!(shown.now.unwrap().id, "firefox.desktop");
}

#[test]
fn the_list_of_sway_things_names_the_types_the_app_opens() {
    let home = mime_home(&["sway"]);
    let apps = installed::installed(&home.dirs);
    let (_dir, empty) = load("");
    assert_eq!(mime::ours_text(&empty, &apps), None);
    assert_eq!(mime::terminals_text(&empty), None);
    let (_dir, chosen) = load(
        "[apps.default]\nbrowser = \"chromium.desktop\"\nimage_viewer = \"imv.desktop\"\n\
         pdf_viewer = \"gone.desktop\"\nterminal = \"foot.desktop\"\n",
    );
    let text = mime::ours_text(&chosen, &apps).unwrap();
    assert_eq!(
        text,
        format!(
            "{HEADER}\n[Default Applications]\n\
             x-scheme-handler/https=chromium.desktop;\ntext/html=chromium.desktop;\n\
             image/png=imv.desktop;\nimage/jpeg=imv.desktop;\n\
             application/pdf=gone.desktop;\n\
             x-scheme-handler/terminal=foot.desktop;\n"
        )
    );
    // It reads as a MIME apps list, the header being comments.
    let list = mime::List::parse(&text);
    assert_eq!(list.defaults["image/png"], ["imv.desktop"]);
    assert_eq!(
        mime::terminals_text(&chosen).unwrap(),
        format!("{HEADER}foot.desktop\n")
    );
    assert_eq!(
        mime::check_id("a b.desktop"),
        Err(ValueError::NotADesktopId)
    );
    assert_eq!(mime::check_id("-x.desktop"), Err(ValueError::NotADesktopId));
    assert_eq!(mime::check_id(""), Ok(()));
}

#[test]
fn autostart_entries_are_read_and_written() {
    let exec = Entry::parse("exec nm-applet --indicator").unwrap();
    assert_eq!(
        exec,
        Entry {
            on: true,
            item: Item::Exec("nm-applet --indicator".to_owned())
        }
    );
    let off = Entry::parse("# app org.example.App.desktop").unwrap();
    assert!(!off.on);
    assert_eq!(off.text(), "# app org.example.App.desktop");
    assert_eq!(
        off.command_line(),
        "sway-things hook launch org.example.App.desktop"
    );
    assert_eq!(
        Entry::parse("app evil;rm -rf ~.desktop"),
        Err(ValueError::NotADesktopId)
    );
    assert_eq!(
        Entry::parse("exec --no-startup-id nm-applet"),
        Err(ValueError::NotAnAutostartEntry)
    );
    assert_eq!(
        Entry::parse("run something"),
        Err(ValueError::NotAnAutostartEntry)
    );
    assert!(matches!(
        Entry::parse("exec a\\b"),
        Err(ValueError::NotSwayText(_))
    ));
    assert!(autostart::check_hidden("org.example.App.desktop").is_ok());
    assert!(autostart::check_hidden("../passwd").is_err());
}

/// What sway's `exec` gets of `rendered` (over IPC, and from a config file, whose `exec` lines
/// run later like an IPC command): the single quoted argument without its quotes.
fn exec_gets(rendered: &str) -> String {
    let commands = model::execute_command(rendered.as_bytes()).unwrap();
    assert_eq!(commands.len(), 1, "{rendered}");
    let argv = &commands[0].argv;
    assert_eq!(argv.len(), 2, "{rendered}");
    String::from_utf8(model::strip_quotes(&argv[1])).unwrap()
}

#[test]
fn an_exec_line_gives_the_shell_exactly_the_command_line() {
    for line in [
        "nm-applet --indicator",
        "swaybg -c '#333333' & echo \"$HOME; a, b\"",
        "a 'b\"c' \"d'e\" $$ ; ,",
        "notify-send \"a # b\" '#c' # d",
        "# not a comment",
        "[x] {y} }",
    ] {
        let command = SwayCmd::exec(SwayText::new(line).unwrap(), false);
        let ipc = command.render(Target::Ipc).unwrap();
        let config = command.render(Target::Config).unwrap();
        assert_eq!(exec_gets(&ipc), line, "{ipc}");
        assert_eq!(exec_gets(&config), line, "{config}");
        // The line is read as a config line first; it must not end a block or fail there.
        let read = model::config_command(config.as_bytes()).unwrap();
        assert_eq!(read[0], b"exec");
    }
    let always = SwayCmd::exec(SwayText::new("x").unwrap(), true);
    assert_eq!(always.render(Target::Config).unwrap(), "exec_always \"x\"");
}

#[test]
fn the_autostart_file_has_the_runner_then_the_entries_switched_on() {
    let (_dir, state) = load(
        "[autostart]\nrunner = \"dex\"\nentries = [\"exec a\", \"# exec b\", \"app c.desktop\"]\n",
    );
    let lines: Vec<String> = autostart::file_lines(&state)
        .into_iter()
        .map(|line| line.command.unwrap().render(Target::Config).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            "exec \"dex --autostart --environment sway\"",
            "exec \"a\"",
            "exec \"sway-things hook launch c.desktop\""
        ]
    );
}

#[test]
fn runners_are_found_in_command_lines() {
    for (line, runner) in [
        ("dex -a", Some(Runner::Dex)),
        (
            "/usr/bin/dex --autostart --environment sway",
            Some(Runner::Dex),
        ),
        ("sleep 2 && dex -ae sway", Some(Runner::Dex)),
        ("dex app.desktop", None),
        ("dex --autostart-not", None),
        ("dex -v", None),
        ("dexter -a", None),
        ("echo dex-a", None),
        ("systemctl --user start graphical-session.target", None),
        ("systemctl --user start sway-session.target", None),
        (
            "systemctl --user start xdg-desktop-autostart.target",
            Some(Runner::Systemd),
        ),
        (
            "systemctl --user start sway-things-autostart.target",
            Some(Runner::Systemd),
        ),
        ("index -a", None),
        ("foot", None),
    ] {
        assert_eq!(autostart::runner_of(line), runner, "{line}");
    }
    let mut runners = Runners {
        systemd: Systemd::default(),
        lines: Vec::new(),
        ours: None,
        dex_installed: true,
    };
    assert_eq!(runners.offer(), Some(Runner::Dex));
    runners.systemd.available = true;
    assert_eq!(runners.offer(), Some(Runner::Systemd));
    runners.ours = Some(Runner::Systemd);
    runners.systemd.active = true;
    assert!(runners.any() && !runners.twice());
    // The user's line and Sway Things both start the target: once is all it takes.
    runners.lines.push(autostart::HandExec {
        file: PathBuf::from("/c"),
        line: 1,
        always: false,
        command: "systemctl --user start xdg-desktop-autostart.target".to_owned(),
        runner: Some(Runner::Systemd),
    });
    assert!(!runners.twice());
    runners.lines.clear();
    runners.systemd.ours_active = true;
    runners.ours = Some(Runner::Dex);
    // Switched to dex: the target Sway Things started this session is gone at the next login.
    assert!(runners.stale() && !runners.twice());
    runners.systemd.ours_active = false;
    assert!(runners.twice());
}

/// A home with system and user autostart entries of every kind.
fn autostart_home() -> Home {
    let home = Home::new(&["sway"]);
    home.program("nextcloud");
    home.program("applet");
    let system = "etc/xdg/autostart";
    let user = "home/.config/autostart";
    let entry = |keys: &str| format!("[Desktop Entry]\nType=Application\n{keys}\n");
    home.write(
        &format!("{system}/nextcloud.desktop"),
        &entry("Name=Nextcloud\nExec=nextcloud --background"),
    );
    home.write(
        &format!("{system}/gnome-only.desktop"),
        &entry("Name=GNOME thing\nExec=applet\nOnlyShowIn=GNOME;Unity;"),
    );
    home.write(
        &format!("{system}/not-sway.desktop"),
        &entry("Name=Not here\nExec=applet\nNotShowIn=sway;"),
    );
    home.write(
        &format!("{system}/missing.desktop"),
        &entry("Name=Missing\nTryExec=absent\nExec=absent"),
    );
    home.write(
        &format!("{system}/gnome-off.desktop"),
        &entry("Name=Applet\nName[de]=Miniprogramm\nExec=applet\nX-GNOME-Autostart-enabled=false"),
    );
    home.write(
        &format!("{system}/switched-off.desktop"),
        &entry("Name=Geoclue\nIcon=geo\nExec=applet"),
    );
    home.write(
        &format!("{user}/switched-off.desktop"),
        &autostart::override_text("switched-off.desktop"),
    );
    home.write(
        &format!("{system}/mine.desktop"),
        &entry("Name=System version\nExec=applet"),
    );
    home.write(
        &format!("{user}/mine.desktop"),
        &entry("Name=Mine\nExec=applet\nHidden=true"),
    );
    home
}

#[test]
fn xdg_autostart_entries_say_whether_they_start() {
    let home = autostart_home();
    let entries = autostart::xdg_entries(&home.dirs);
    let found: Vec<(&str, &str, Place, &Starts, bool)> = entries
        .iter()
        .map(|entry| {
            (
                entry.id.as_str(),
                entry.name.as_str(),
                entry.place,
                &entry.starts,
                entry.system.is_some(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                "switched-off.desktop",
                "Geoclue",
                Place::Ours,
                &Starts::Hidden,
                true
            ),
            (
                "gnome-only.desktop",
                "GNOME thing",
                Place::System,
                &Starts::OnlyIn(vec!["GNOME".to_owned(), "Unity".to_owned()]),
                false
            ),
            ("mine.desktop", "Mine", Place::User, &Starts::Hidden, true),
            (
                "gnome-off.desktop",
                "Miniprogramm",
                Place::System,
                &Starts::Yes,
                false
            ),
            (
                "missing.desktop",
                "Missing",
                Place::System,
                &Starts::NotInstalled("absent".to_owned()),
                false
            ),
            (
                "nextcloud.desktop",
                "Nextcloud",
                Place::System,
                &Starts::Yes,
                false
            ),
            (
                "not-sway.desktop",
                "Not here",
                Place::System,
                &Starts::NotIn(vec!["sway".to_owned()]),
                false
            ),
        ]
    );
    let gnome_off = entries.iter().find(|entry| entry.id == "gnome-off.desktop");
    assert!(gnome_off.is_some_and(|entry| entry.gnome_disabled));
    assert_eq!(entries[0].icon.as_deref(), Some("geo"));
    // Sway Things' override shows the command of the system entry it switches off.
    assert_eq!(entries[0].exec.as_deref(), Some("applet"));
}

#[test]
fn only_files_of_sway_things_are_written_and_deleted() {
    let home = Home::new(&["sway"]);
    let manifest_path = home.root.join("state/manifest.toml");
    let user = home.root.join("home/.config/autostart");
    // The user's own entry of an ID: never written, even when asked to switch it off.
    home.write(
        "home/.config/autostart/own.desktop",
        "[Desktop Entry]\nName=Own\nExec=own\n",
    );
    let (_dir, state) = load(
        "[apps.default]\nbrowser = \"firefox.desktop\"\n\n[autostart]\nrunner = \"systemd\"\n\
         hidden = [\"geoclue.desktop\", \"own.desktop\"]\n",
    );
    let wanted = files::wanted(&state, &home.dirs);
    let mut manifest = Manifest::load(&manifest_path);
    let problems = files::keep(&wanted, &mut manifest);
    assert_eq!(problems, [Problem::Foreign(user.join("own.desktop"))]);
    manifest.save().unwrap();
    let geoclue = user.join("geoclue.desktop");
    assert_eq!(
        fs::read_to_string(&geoclue).unwrap(),
        autostart::override_text("geoclue.desktop")
    );
    assert!(
        fs::read_to_string(home.root.join("home/.config/sway-mimeapps.list"))
            .unwrap()
            .contains("x-scheme-handler/https=firefox.desktop;")
    );
    assert!(
        home.root
            .join("home/.config/systemd/user/sway-things-autostart.target")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(user.join("own.desktop")).unwrap(),
        "[Desktop Entry]\nName=Own\nExec=own\n"
    );

    // Switched on again and nothing chosen: Sway Things' files go, the user's stays.
    let (_dir, empty) = load("");
    let mut manifest = Manifest::load(&manifest_path);
    let wanted = files::wanted(&empty, &home.dirs);
    assert!(wanted.contains(&Wanted {
        path: geoclue.clone(),
        text: None
    }));
    assert_eq!(files::keep(&wanted, &mut manifest), []);
    assert!(!geoclue.exists());
    assert!(!home.root.join("home/.config/sway-mimeapps.list").exists());
    assert!(user.join("own.desktop").exists());

    // A dangling link is never replaced, nor a file that appears where none was.
    std::os::unix::fs::symlink(home.root.join("nowhere"), &geoclue).unwrap();
    let (_dir, hidden) = load("[autostart]\nhidden = [\"geoclue.desktop\"]\n");
    let wanted = files::wanted(&hidden, &home.dirs);
    assert_eq!(
        files::keep(&wanted, &mut manifest),
        [Problem::Foreign(geoclue.clone())]
    );
    assert!(!home.root.join("nowhere").exists());
    fs::remove_file(&geoclue).unwrap();
    // A file of Sway Things changed by hand is left as it is.
    let (_dir, hidden) = load("[autostart]\nhidden = [\"geoclue.desktop\"]\n");
    let wanted = files::wanted(&hidden, &home.dirs);
    assert_eq!(files::keep(&wanted, &mut manifest), []);
    fs::write(&geoclue, format!("{HEADER}[Desktop Entry]\nHidden=false\n")).unwrap();
    let wanted = files::wanted(&empty, &home.dirs);
    assert_eq!(
        files::keep(&wanted, &mut manifest),
        [Problem::Edited(geoclue.clone())]
    );
    assert!(geoclue.exists());
}

#[test]
fn terminal_and_launcher_become_bindings_of_their_keys() {
    let (_dir, none) = load("");
    assert!(Binding::of(TERMINAL).unwrap().line(&none).is_none());
    let (_dir, state) = load(
        "[shortcuts]\nmodifier = \"alt\"\n\n[apps]\nterminal = \"foot\"\n\
         launcher = \"wofi --show drun\"\nlauncher_keys = \"$mod+space\"\n",
    );
    let file_lines = crate::bindings::ours::file_lines(&state);
    let lines: Vec<String> = file_lines
        .iter()
        .map(|line| line.command().unwrap().render(Target::Config).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            "bindsym --no-warn Mod1+Return exec \"foot\"",
            "bindsym --no-warn Mod1+space exec \"wofi --show drun\""
        ]
    );
    assert_eq!(crate::bindings::ours::key(&file_lines[1]), LAUNCHER);
    assert!(launchers::check_keys("$mod+Return").is_ok());
    assert_eq!(launchers::check_keys("Return"), Err(ValueError::TypingKeys));
    assert!(launchers::check_keys("F12").is_ok());
    assert!(launchers::check_keys("$mod+\"x\"").is_err());
    assert!(launchers::check_command("").is_ok());
    assert!(launchers::check_command("foot -e 'tmux'").is_ok());
    assert!(launchers::check_command("foot\\").is_err());
}

#[test]
fn the_user_s_binding_on_the_same_keys_is_replaced_and_says_so() {
    use crate::bindings::{ClashKind, Listing, RowKind};
    use crate::include::IncludeStatus;
    use crate::sway_config::Disk;

    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    fs::write(
        &config,
        format!(
            "set $mod Mod4\nset $term foot\nbindsym $mod+Return exec $term\ninclude {}/*.conf\n",
            dir.path().join("gen").display()
        ),
    )
    .unwrap();
    let locations = crate::generate::Locations::new(&dir.path().join("gen"), dir.path());
    let status = IncludeStatus::detect(Some(&config), &Disk, &locations);
    let (_state_dir, state) = load("[apps]\nterminal = \"kitty\"\n");
    let listing = Listing::new(&status, &state, Path::new("50-bindings.conf"), None);
    let ours = listing
        .rows()
        .iter()
        .find(|row| row.kind == RowKind::App { key: TERMINAL })
        .unwrap();
    assert!(ours.in_effect);
    assert_eq!(ours.clashes[0].kind, ClashKind::Replaces);
    assert_eq!(ours.clashes[0].command.as_deref(), Some("exec foot"));
    assert!(!ours.editable());
}

#[test]
fn an_app_in_a_terminal_starts_through_one() {
    let home = Home::new(&["sway"]);
    home.program("htop");
    let path = home.app(
        "usr/share",
        "htop.desktop",
        "Name=htop\nExec=htop %F\nTerminal=true",
    );
    let app = installed::find(&home.dirs, "htop.desktop").unwrap();
    assert_eq!(app.path, path);
    assert_eq!(
        launch::command(&home.dirs, &app),
        Err(launch::LaunchError::NoTerminal)
    );
    home.program("alacritty");
    assert_eq!(
        launch::command(&home.dirs, &app).unwrap(),
        (vec!["alacritty".into(), "-e".into(), "htop".into()], None)
    );
    // A terminal of its own by its absolute path stays so; a bare name stays a name.
    home.program("myterm");
    let myterm = home.root.join("bin/myterm");
    home.app(
        "usr/share",
        "myterm.desktop",
        &format!(
            "Name=My Term\nCategories=System;TerminalEmulator;\nExec={}",
            myterm.display()
        ),
    );
    home.app(
        "usr/share",
        "other.desktop",
        "Name=Other Term\nCategories=TerminalEmulator;\nExec=htop -x",
    );
    let terminals = launchers::terminals(&home.dirs, &installed::installed(&home.dirs));
    assert!(terminals.contains(&(myterm.display().to_string(), "My Term".to_owned())));
    assert!(terminals.contains(&("htop".to_owned(), "Other Term".to_owned())));
    assert!(terminals.contains(&("alacritty".to_owned(), "Alacritty".to_owned())));
    home.program("xdg-terminal-exec");
    assert_eq!(
        launch::command(&home.dirs, &app).unwrap().0,
        ["xdg-terminal-exec", "htop"]
    );
    assert_eq!(
        async_io::block_on(launch::launch(&home.dirs, "absent.desktop")),
        Err(launch::LaunchError::NotFound("absent.desktop".to_owned()))
    );
    home.app(
        "usr/share",
        "gone.desktop",
        "Name=Gone\nTryExec=absent\nExec=absent",
    );
    assert_eq!(
        async_io::block_on(launch::launch(&home.dirs, "gone.desktop")),
        Err(launch::LaunchError::NotInstalled("absent".to_owned()))
    );
    assert_eq!(
        launch::bus_name_and_path("org.gnome.Clocks-Beta.desktop"),
        Some((
            "org.gnome.Clocks-Beta".to_owned(),
            "/org/gnome/Clocks_Beta".to_owned()
        ))
    );
    assert_eq!(launch::bus_name_and_path("htop.desktop"), None);
}
