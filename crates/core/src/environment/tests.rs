// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::session::{self, Session};
use super::{files, syntax};
use super::{line, path, quote, text, variables, wanted};
use crate::apps::Dirs;
use crate::manifest::{HEADER, Manifest};
use crate::manifest::{Problem, keep};
use crate::registry::environment::{
    ELECTRON_WAYLAND, JAVA_NONREPARENTING, QT_WAYLAND, SDL_WAYLAND, is_variable_name,
};
use crate::registry::region::{FORMAT_VARIABLES, FORMATS, LANGUAGE};
use crate::registry::{Registry, Value, Variable};
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

#[test]
fn the_language_sets_every_format_and_the_formats_replace_them() {
    let names = |state: &State| -> Vec<(String, String)> {
        variables(Registry::builtin(), state)
            .into_iter()
            .map(|variable| (variable.name, variable.value))
            .collect()
    };
    let (_a, language) = load("[region]\nlanguage = \"en_US.UTF-8\"\n");
    let mut expected = vec![
        ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
        ("LC_MESSAGES".to_owned(), "en_US.UTF-8".to_owned()),
    ];
    expected.extend(
        FORMAT_VARIABLES
            .iter()
            .map(|name| ((*name).to_owned(), "en_US.UTF-8".to_owned())),
    );
    assert_eq!(names(&language), expected);

    let (_b, both) = load("[region]\nlanguage = \"en_US.UTF-8\"\nformats = \"de_DE.UTF-8\"\n");
    let mut expected = vec![
        ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
        ("LC_MESSAGES".to_owned(), "en_US.UTF-8".to_owned()),
    ];
    expected.extend(
        FORMAT_VARIABLES
            .iter()
            .map(|name| ((*name).to_owned(), "de_DE.UTF-8".to_owned())),
    );
    assert_eq!(names(&both), expected);

    // Formats alone leave the language to the system.
    let (_c, formats) = load("[region]\nformats = \"de_DE.UTF-8\"\n");
    assert_eq!(names(&formats).len(), FORMAT_VARIABLES.len());
    assert!(
        names(&formats)
            .iter()
            .all(|(name, _)| name.starts_with("LC_"))
    );
}

#[test]
fn only_locales_are_taken() {
    let registry = Registry::builtin();
    for key in [LANGUAGE, FORMATS] {
        let setting = registry.get(key).unwrap();
        for good in ["", "de_DE.UTF-8", "sr_RS.UTF-8@latin", "C.UTF-8"] {
            assert!(
                setting.check(&Value::Text(good.to_owned())).is_ok(),
                "{key} {good:?}"
            );
        }
        for bad in ["de_DE", "de_DE.UTF-8\nEDITOR=vim", "x\"y", "$HOME"] {
            assert!(
                setting.check(&Value::Text(bad.to_owned())).is_err(),
                "{key} {bad:?}"
            );
        }
    }
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
    for (key, name) in [
        (LANGUAGE, "LANG"),
        (LANGUAGE, "LC_MESSAGES"),
        (LANGUAGE, "LC_TIME"),
        (LANGUAGE, "LC_ALL"),
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

/// The variables of `text`, as `NAME=value` without quotes.
fn assignments(text: &str) -> Vec<String> {
    syntax::parse(text)
        .assignments
        .iter()
        .map(|assignment| format!("{}={}", assignment.name, assignment.value))
        .collect()
}

#[test]
fn files_are_read_as_the_generator_reads_them() {
    // Each case as systemd 262's environment.d generator read it.
    assert_eq!(
        assignments(
            "A=\"a\" \"b\"\nB='a' 'b'\nC=\\ a\nD=\"x\\ny\\\"z\\\\w\\`v\"\nE='a\\'\nF=\"a\nb\"\n\
             G=x # c\nH=\"a\"   \nI=a   b  \nJ\nK=\nL=\"\"\n#M=1\\\nN=2\n S = s \n; U=no\n\
             R=a\\\nb\nY=\"a\"b'c'\nexport O=1\n1E=bad\nAD=a\\$b\n"
        ),
        [
            "A=ab",
            "B=ab",
            "C= a",
            "D=x\\ny\"z\\w`v",
            "E=a\\",
            "F=a\nb",
            "G=x # c",
            "H=a",
            "I=a   b",
            "N=2",
            "S=s",
            "R=ab",
            "Y=ab'c'",
            "AD=a$b",
        ]
    );
    let parsed = syntax::parse("A=1\n\nB=2\\\n3\nC=4");
    let lines: Vec<usize> = parsed.assignments.iter().map(|a| a.line).collect();
    assert_eq!(lines, [1, 3, 5]);
    assert_eq!(parsed.end, Some(syntax::End::Complete));
    // A quote left open takes the rest of the file, as systemd does.
    let open = syntax::parse("K=\"open\nL=after\n");
    assert_eq!(open.end, Some(syntax::End::Quote));
    assert_eq!(assignments("K=\"open\nL=after\n"), ["K=open\nL=after\n"]);
}

#[test]
fn values_expand_as_the_generator_expands_them() {
    let lookup = |name: &str| match name {
        "HOME" => Some("/h".to_owned()),
        "EMPTY" => Some(String::new()),
        _ => None,
    };
    for (value, expanded) in [
        ("$HOME/x", "/h/x"),
        ("${HOME}x", "/hx"),
        ("$HOMEDIR", ""),
        ("$$HOME", "$HOME"),
        ("$", "$"),
        ("a $NOTSET b", "a  b"),
        ("${UNSET:-def$HOME}", "def/h"),
        ("${EMPTY:-d}", "d"),
        ("${HOME:-x}y${UNSET:+z}", "/hy"),
        ("${HOME:+alt}", "alt"),
        ("${HOME:-${UNSET}}", "/h"),
        ("${UNSET:-${HOME}}", "/h"),
        ("${FOO", "${FOO"),
        ("${A-x}", ""),
        ("$HOME$HOME", "/h/h"),
    ] {
        assert_eq!(syntax::expand(value, &lookup), expanded, "{value}");
    }
    assert!(syntax::refers_to("$HOME/bin:$PATH", "PATH"));
    assert!(syntax::refers_to("${PATH:-/usr/bin}", "PATH"));
    assert!(syntax::refers_to("${X:-${PATH}}", "PATH"));
    assert!(!syntax::refers_to("$PATHX:$$PATH", "PATH"));
}

#[test]
fn typed_values_are_checked() {
    use syntax::SyntaxError;
    for good in [
        "vim",
        "$HOME/.local/bin:$PATH",
        "\"a b\"",
        "'$x'",
        "${X:-${Y:+z}}",
        "$$",
        "100$",
    ] {
        assert_eq!(syntax::check_value(good), Ok(()), "{good}");
    }
    for (bad, error) in [
        ("a\nb", SyntaxError::Control),
        ("a\tb", SyntaxError::Control),
        ("\"open", SyntaxError::Quote),
        ("'open", SyntaxError::Quote),
        ("end\\", SyntaxError::Backslash),
        ("", SyntaxError::Empty),
        ("\"\"", SyntaxError::Empty),
        ("${HOME", SyntaxError::Brace),
        ("${HOME-x}", SyntaxError::Expansion),
        ("${:-x}", SyntaxError::Expansion),
        ("$1", SyntaxError::Expansion),
    ] {
        assert_eq!(syntax::check_value(bad), Err(error), "{bad:?}");
    }
}

proptest::proptest! {
    #[test]
    fn reading_never_panics(text in "[ -~\n\t'\"\\\\${}:+-]{0,64}") {
        let _ = syntax::parse(&text);
        let _ = syntax::check_value(&text);
        let _ = syntax::expand(&text, &|_| Some("v".to_owned()));
        let _ = syntax::refers_to(&text, "PATH");
    }

    #[test]
    fn a_checked_value_is_one_line_of_the_file(value in "[ -~]{1,32}") {
        if syntax::check_value(&value).is_ok() {
            let parsed = syntax::parse(&format!("N={value}\nM=1\n"));
            proptest::prop_assert_eq!(parsed.assignments.len(), 2);
            proptest::prop_assert_eq!(&parsed.assignments[1].name, "M");
        }
    }

    #[test]
    fn a_quoted_value_reads_back_as_it_was(value in "[ -~]{1,32}") {
        let text = format!("N={}\n", quote(&value));
        let parsed = syntax::parse(&text);
        let read = parsed.assignments.first().map(|a| syntax::expand(&a.value, &|_| None));
        proptest::prop_assert_eq!(read, Some(value));
    }
}

#[test]
fn the_editor_guards_names_and_path() {
    use crate::registry::environment::{VariableError, check_variable, warning};
    assert_eq!(check_variable("EDITOR=vim"), Ok(()));
    assert_eq!(check_variable("PATH=$HOME/.local/bin:$PATH"), Ok(()));
    assert_eq!(check_variable("PATH=/opt/x/bin:/usr/bin"), Ok(()));
    assert_eq!(
        check_variable("PATH=/opt/x/bin"),
        Err(VariableError::PathLost)
    );
    assert_eq!(
        check_variable("EDITOR"),
        Err(VariableError::NotAnAssignment)
    );
    assert_eq!(check_variable("1X=a"), Err(VariableError::NotAName));
    assert_eq!(check_variable("HOME=/x"), Err(VariableError::Session));
    assert_eq!(
        check_variable("XDG_SESSION_TYPE=x"),
        Err(VariableError::Session)
    );
    assert_eq!(
        check_variable("QT_QPA_PLATFORM=xcb"),
        Err(VariableError::Switch)
    );
    assert_eq!(
        check_variable("LC_TIME=de_DE.UTF-8"),
        Err(VariableError::OtherPage(crate::registry::Page::RegionTime))
    );
    assert_eq!(
        check_variable("EDITOR=\"vim"),
        Err(VariableError::Syntax(syntax::SyntaxError::Quote))
    );
    assert!(warning("LD_PRELOAD").is_some());
    assert!(warning("EDITOR").is_none());
}

#[test]
fn the_users_variables_come_last_as_they_are() {
    let (_dir, state) = load(
        "[environment]\nvariables = [\"PATH=$HOME/bin:$PATH\", \"EDITOR='vi m'\"]\n\
         qt_wayland = true\n",
    );
    assert_eq!(
        text_of(&state).unwrap(),
        format!("{HEADER}QT_QPA_PLATFORM=\"wayland;xcb\"\nPATH=$HOME/bin:$PATH\nEDITOR='vi m'\n")
    );
}

#[test]
fn every_file_is_read_in_systemds_order() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user");
    let etc = dir.path().join("etc");
    let library = dir.path().join("lib");
    for path in [&user, &etc, &library] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(user.join("10-mine.conf"), "EDITOR=vi\n").unwrap();
    fs::write(etc.join("10-mine.conf"), "EDITOR=hidden\n").unwrap();
    fs::write(
        library.join("60-later.conf"),
        "# x\nQT_QPA_PLATFORM=xcb\nPATH=$PATH:/x\n",
    )
    .unwrap();
    fs::write(library.join("20-skip.txt"), "SKIPPED=1\n").unwrap();
    fs::write(user.join(super::FILE_NAME), "IGNORED=1\n").unwrap();
    let ours = [
        Variable::literal("QT_QPA_PLATFORM", "wayland;xcb"),
        Variable {
            name: "PATH".to_owned(),
            value: "\"$HOME/bin:$PATH\"".to_owned(),
            raw: true,
        },
    ];
    let search = [user.clone(), etc, library];
    let lines = files::lines(&search, &user.join(super::FILE_NAME), &ours);
    let shown: Vec<(String, String, files::Origin)> = lines
        .iter()
        .map(|line| {
            (
                format!(
                    "{}:{}",
                    line.file.file_name().unwrap().to_string_lossy(),
                    line.line
                ),
                format!("{}={}", line.name, line.value),
                line.origin,
            )
        })
        .collect();
    let header = super::HEADER_LINES;
    let ours_at = |offset: usize| format!("{}:{}", super::FILE_NAME, header + offset);
    assert_eq!(
        shown,
        [
            (
                "10-mine.conf:1".to_owned(),
                "EDITOR=vi".to_owned(),
                files::Origin::User
            ),
            (
                ours_at(1),
                "QT_QPA_PLATFORM=wayland;xcb".to_owned(),
                files::Origin::Ours
            ),
            (
                ours_at(2),
                "PATH=$HOME/bin:$PATH".to_owned(),
                files::Origin::Ours
            ),
            (
                "60-later.conf:2".to_owned(),
                "QT_QPA_PLATFORM=xcb".to_owned(),
                files::Origin::System
            ),
            (
                "60-later.conf:3".to_owned(),
                "PATH=$PATH:/x".to_owned(),
                files::Origin::System
            ),
        ]
    );
    // The later Qt line replaces ours; the later PATH builds on ours.
    assert_eq!(
        files::overridden_by(&lines),
        [None, Some(3), None, None, None]
    );
    let base = HashMap::from([
        ("HOME".to_owned(), "/h".to_owned()),
        ("PATH".to_owned(), "/usr/bin".to_owned()),
    ]);
    assert_eq!(
        files::expanded(&lines, &base),
        [
            "vi",
            "wayland;xcb",
            "/h/bin:/usr/bin",
            "xcb",
            "/h/bin:/usr/bin:/x"
        ]
    );
}

#[test]
fn the_user_managers_environment_is_read() {
    let parsed = files::parse_show_environment("HOME=/h\nX=$'a b\\n\\xc3\\xa4\\''\nbroken\n");
    assert_eq!(parsed.get("HOME").map(String::as_str), Some("/h"));
    assert_eq!(parsed.get("X").map(String::as_str), Some("a b\nä'"));
    assert_eq!(parsed.len(), 2);
}
