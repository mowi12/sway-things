// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};

use gio::glib;

use super::shortcuts::{self, Shortcut};
use super::*;
use crate::bindings::{ClashKind, DEFAULT_MODE, Listing, RowKind};
use crate::command::Target as Render;
use crate::generate::{self, Locations};
use crate::include::IncludeStatus;
use crate::ipc::types::{Node, Output};
use crate::paths::Paths;
use crate::registry::{ConfigFile, ValueError};
use crate::sway_config::MemoryFiles;

fn state(dir: &tempfile::TempDir, values: &[(&str, Value)]) -> State {
    let paths = Paths::new(
        &dir.path().join("config"),
        &dir.path().join("state"),
        "host",
    )
    .unwrap();
    let mut state = State::load(Registry::builtin(), paths);
    for (key, value) in values {
        state.set(key, None, value.clone(), None).unwrap();
    }
    state
}

fn env(home: &str, pictures: Option<&str>) -> name::Env {
    name::Env {
        home: Some(PathBuf::from(home)),
        pictures: pictures.map(PathBuf::from),
    }
}

#[test]
fn the_folder_is_screenshots_in_the_pictures_folder_unless_set() {
    let with = env("/home/me", Some("/home/me/Bilder"));
    assert_eq!(
        name::folder("", &with),
        Path::new("/home/me/Bilder/Screenshots")
    );
    // Without user-dirs, or with the pictures folder set to the home folder.
    for env in [env("/home/me", None), env("/home/me", Some("/home/me"))] {
        assert_eq!(
            name::folder("", &env),
            Path::new("/home/me/Pictures/Screenshots")
        );
    }
    assert_eq!(name::folder("~/Shots", &with), Path::new("/home/me/Shots"));
    assert_eq!(name::folder("~", &with), Path::new("/home/me"));
    assert_eq!(name::folder("/tmp/x", &with), Path::new("/tmp/x"));
    for good in ["", "/srv/shots", "~", "~/a b/c"] {
        assert_eq!(name::check_folder(good), Ok(()), "{good}");
    }
    for bad in ["shots", "~user/x", "/a\nb", "$HOME/x"] {
        assert_eq!(
            name::check_folder(bad),
            Err(ValueError::NotAFolder),
            "{bad}"
        );
    }
}

#[test]
fn names_come_from_the_pattern_and_are_numbered_in_the_same_second() {
    let time = glib::DateTime::new(&glib::TimeZone::utc(), 2026, 11, 20, 14, 3, 22.0).unwrap();
    assert_eq!(
        name::file_name(DEFAULT_FILE_NAME, &time).as_deref(),
        Some("Screenshot from 2026-11-20 14-03-22.png")
    );
    assert_eq!(
        name::file_name("100%% %j", &time).as_deref(),
        Some("100% 324")
    );
    for bad in ["", "a/%Y", "a\nb", "%Q", ".."] {
        assert_eq!(name::file_name(bad, &time), None, "{bad:?}");
        assert_eq!(name::check_file_name(bad), Err(ValueError::NotAFileName));
    }
    assert_eq!(name::check_file_name(DEFAULT_FILE_NAME), Ok(()));
    let shots = &shot::EXTENSIONS;
    assert_eq!(
        name::with_extension("a.PNG", shots),
        ("a.PNG".to_owned(), "png")
    );
    assert_eq!(
        name::with_extension("a.jpg", shots),
        ("a.jpg".to_owned(), "jpg")
    );
    assert_eq!(
        name::with_extension("a.gif", shots),
        ("a.gif.png".to_owned(), "png")
    );
    assert_eq!(
        name::with_extension("clip", &record::EXTENSIONS),
        ("clip.mp4".to_owned(), "mp4")
    );
    assert_eq!(name::numbered("a.png", 1), "a.png");
    assert_eq!(name::numbered("a.png", 3), "a (3).png");
    assert_eq!(name::numbered("a", 2), "a (2)");

    let dir = tempfile::tempdir().unwrap();
    // The folder is made, and a file of the same name is never written.
    let folder = dir.path().join("Pictures/Screenshots");
    let first = name::write_new(&folder, "a.png", b"one").unwrap();
    let second = name::write_new(&folder, "a.png", b"two").unwrap();
    assert_eq!(first, folder.join("a.png"));
    assert_eq!(second, folder.join("a (2).png"));
    assert_eq!(std::fs::read(&first).unwrap(), b"one");
    assert_eq!(
        name::free_path(&folder, "a.png"),
        Some(folder.join("a (3).png"))
    );
    assert_eq!(
        name::free_path(&folder, "b.png"),
        Some(folder.join("b.png"))
    );
    // A name that is only an extension keeps it at its end.
    assert_eq!(name::numbered(".png", 2), "(2).png");
    assert_eq!(name::numbered(".shot.png", 2), ".shot (2).png");
}

#[test]
fn keys_must_be_plain_and_leave_typing_alone() {
    for good in [
        "",
        "Print",
        "Alt+Print",
        "$mod+Shift+v",
        "Ctrl+Alt+Shift+r",
        "F12",
    ] {
        assert_eq!(shortcuts::check_keys(good), Ok(()), "{good}");
    }
    for typing in ["v", "Shift+v", "Return"] {
        assert_eq!(
            shortcuts::check_keys(typing),
            Err(ValueError::TypingKey),
            "{typing}"
        );
    }
    for bad in ["Mod4+\"x\"", "button1", "Mod4+", "Mod4+$x", "a b"] {
        assert_eq!(
            shortcuts::check_keys(bad),
            Err(ValueError::NotAKey),
            "{bad}"
        );
    }
    // Every default can be bound.
    for shortcut in shortcuts::SHORTCUTS {
        let setting = Registry::builtin().get(shortcut.key).unwrap();
        assert!(setting.check(&setting.default_value()).is_ok());
    }
}

#[test]
fn switched_on_keys_go_to_the_bindings_file_and_run_the_command_line() {
    let dir = tempfile::tempdir().unwrap();
    let off = state(&dir, &[]);
    assert_eq!(shortcuts::file_lines(&off).len(), 0);
    let dir = tempfile::tempdir().unwrap();
    let on = state(
        &dir,
        &[
            (SCREENSHOTS, Value::Bool(true)),
            (KEY_ALL, Value::Text(String::new())),
            (HISTORY, Value::Bool(true)),
            (
                crate::bindings::ours::MODIFIER,
                Value::Choice("alt".to_owned()),
            ),
        ],
    );
    let lines: Vec<String> = crate::bindings::ours::file_lines(&on)
        .iter()
        .map(|line| line.command().unwrap().render(Render::Config).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            "bindsym --no-repeat --no-warn Print exec \"sway-things screenshot region\"",
            "bindsym --no-repeat --no-warn Alt+Print exec \"sway-things screenshot window\"",
            "bindsym --no-repeat --no-warn Shift+Print exec \"sway-things screenshot display\"",
            "bindsym --no-repeat --no-warn Mod1+Shift+v exec \"sway-things clipboard pick\"",
        ]
    );
    let text = generate::generate(Registry::builtin(), &on, generate::tests::SWAY_1_10)
        .into_iter()
        .find(|file| file.file() == ConfigFile::Hooks)
        .unwrap()
        .text();
    assert!(
        text.contains("exec_always sway-things hook clipboard\n"),
        "{text}"
    );
    assert_eq!(shortcuts::label(&on, HISTORY_KEY), "Alt + Shift + V");
    assert_eq!(
        Shortcut::of(KEY_REGION).map(|shortcut| shortcut.switch),
        Some(SCREENSHOTS)
    );
}

#[test]
fn the_keyboard_shortcuts_page_lists_the_keys_with_what_they_replace() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(&dir, &[(SCREENSHOTS, Value::Bool(true))]);
    let locations = Locations::new(
        Path::new("/home/me/.config/sway/sway-things"),
        Path::new("/home/me/.local/state/sway-things"),
    );
    let files = MemoryFiles::new().var("HOME", "/home/me").file(
        "/home/me/.config/sway/config",
        "bindsym Print exec grimshot copy area\n\
         include ~/.config/sway/sway-things/*.conf\n",
    );
    let status = IncludeStatus::detect(
        Some(Path::new("/home/me/.config/sway/config")),
        &files,
        &locations,
    );
    let listing = Listing::new(&status, &state, &locations.path(ConfigFile::Bindings), None);
    let region = listing
        .rows()
        .iter()
        .find(|row| row.kind == RowKind::Feature { key: KEY_REGION })
        .unwrap();
    assert!(region.in_effect && !region.editable());
    assert_eq!(region.clashes[0].kind, ClashKind::Replaces);
    assert_eq!(region.clashes[0].source.line, 1);
    let user = listing
        .rows()
        .iter()
        .find(|row| row.kind == RowKind::Hand)
        .unwrap();
    assert!(!user.in_effect);
    // The Keyboard Shortcuts page's dialog sees the key as taken.
    let new = crate::bindings::ours::parse_item("bindsym Print exec foot").unwrap();
    let users = listing.users_of(&new, DEFAULT_MODE, None, None);
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].kind, RowKind::Feature { key: KEY_REGION });
}

/// A window of the tree: `focused`, `visible`, at `rect` (x, y, width, height).
fn window(id: i64, kind: &str, focused: bool, visible: bool, rect: [i32; 4]) -> serde_json::Value {
    serde_json::json!({
        "id": id, "name": "w", "type": kind, "border": "pixel", "current_border_width": 2,
        "layout": "none", "orientation": "none", "percent": 1.0,
        "rect": {"x": rect[0], "y": rect[1], "width": rect[2], "height": rect[3]},
        "window_rect": {"x": 0, "y": 0, "width": 1, "height": 1},
        "deco_rect": {"x": 0, "y": 0, "width": 0, "height": 0},
        "geometry": {"x": 0, "y": 0, "width": 1, "height": 1},
        "urgent": false, "focused": focused, "focus": [], "sticky": false,
        "floating_nodes": [], "nodes": [], "app_id": "foot", "pid": 10, "visible": visible
    })
}

/// A container of `kind` with `nodes` and `floating`.
fn container(
    kind: &str,
    focused: bool,
    nodes: &[serde_json::Value],
    floating: &[serde_json::Value],
) -> serde_json::Value {
    serde_json::json!({
        "id": 1, "name": "c", "type": kind, "border": "none", "current_border_width": 0,
        "layout": "splith", "orientation": "horizontal", "percent": null,
        "rect": {"x": 0, "y": 0, "width": 3840, "height": 1080},
        "window_rect": {"x": 0, "y": 0, "width": 0, "height": 0},
        "deco_rect": {"x": 0, "y": 0, "width": 0, "height": 0},
        "geometry": {"x": 0, "y": 0, "width": 0, "height": 0},
        "urgent": false, "focused": focused, "focus": [], "sticky": false,
        "floating_nodes": floating, "nodes": nodes
    })
}

#[test]
fn the_window_and_the_boxes_come_from_the_tree() {
    let tree = |focus: bool| -> Node {
        let workspace = container(
            "workspace",
            !focus,
            &[
                window(4, "con", focus, true, [1920, 0, 1920, 1080]),
                window(6, "con", false, false, [1920, 0, 1920, 1080]),
            ],
            &[window(
                5,
                "floating_con",
                false,
                true,
                [2000, 100, 300, 200],
            )],
        );
        serde_json::from_value(container("root", false, &[workspace], &[])).unwrap()
    };
    let focused = shot::focused_window(&tree(true)).unwrap();
    assert_eq!(shot::geometry(&focused), "1920,0 1920x1080");
    let boxes: Vec<String> = shot::visible_windows(&tree(true))
        .iter()
        .map(shot::geometry)
        .collect();
    assert_eq!(boxes, ["1920,0 1920x1080", "2000,100 300x200"]);
    // An empty workspace has the focus: no window.
    assert_eq!(shot::focused_window(&tree(false)), None);
    assert_eq!(
        shot::Target::Geometry("1,2 3x4".to_owned()).grim_args(),
        ["-g", "1,2 3x4"]
    );
    assert_eq!(
        shot::Target::Output("DP-1".to_owned()).grim_args(),
        ["-o", "DP-1"]
    );
    assert_eq!(shot::Target::All.grim_args(), Vec::<String>::new());
}

#[test]
fn the_focused_display_is_found_by_its_name() {
    let output = |name: &str, focused: bool| -> Output {
        serde_json::from_value(serde_json::json!({
            "id": 1, "name": name, "make": "", "model": "", "serial": "", "active": true,
            "dpms": true, "power": true, "primary": false, "scale": 1.0, "transform": "normal",
            "current_workspace": "1", "modes": [], "current_mode": null,
            "rect": {"x": 0, "y": 0, "width": 1, "height": 1}, "focus": [], "focused": focused,
            "non_desktop": false, "subpixel_hinting": "unknown"
        }))
        .unwrap()
    };
    let outputs = [output("eDP-1", false), output("DP-2", true)];
    assert_eq!(shot::focused_output(&outputs).as_deref(), Some("DP-2"));
    assert_eq!(shot::focused_output(&outputs[..1]), None);
}

#[test]
fn recorders_and_the_watcher_get_their_arguments() {
    let target = shot::Target::Geometry("0,0 10x10".to_owned());
    let args = record::args(
        tools::WF_RECORDER,
        &target,
        Path::new("/v/a b.mp4"),
        true,
        Some(record::CODECS[1]),
    );
    // The file is made empty beforehand: wf-recorder must not ask before writing over it.
    assert_eq!(
        args,
        [
            "-g",
            "0,0 10x10",
            "-c",
            "libopenh264",
            "-x",
            "yuv420p",
            "-F",
            "crop=trunc(iw/2)*2:trunc(ih/2)*2:0:0",
            "-y",
            "-f",
            "/v/a b.mp4",
            "--audio"
        ]
    );
    let args = record::args(
        tools::WL_SCREENREC,
        &shot::Target::Output("DP-1".to_owned()),
        Path::new("/v/a.mkv"),
        false,
        None,
    );
    assert_eq!(args, ["-o", "DP-1", "-f", "/v/a.mkv"]);
    assert_eq!(
        name::with_extension("clip.webm", &record::EXTENSIONS),
        ("clip.webm.mp4".to_owned(), "mp4")
    );
    assert_eq!(
        clipboard::watcher_args(500),
        ["--watch", "cliphist", "-max-items", "500", "store"]
    );
    assert_eq!(
        clipboard::parse_version("wl-clipboard 2.3.0\nCopyright"),
        Some((2, 3, 0))
    );
    assert_eq!(clipboard::parse_version("version\tv0.7.0"), Some((0, 7, 0)));
    assert_eq!(clipboard::parse_version("nothing here"), None);
}

#[test]
fn settings_have_their_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::of(&state(&dir, &[]));
    assert_eq!(settings.destination, Destination::Both);
    assert_eq!(settings.afterwards, Afterwards::Notify);
    assert_eq!(settings.file_name, DEFAULT_FILE_NAME);
    assert_eq!(settings.recording_file_name, DEFAULT_RECORDING_FILE_NAME);
    assert!(settings.record_region && !settings.audio);
    assert_eq!(settings.max_entries, 750);
    assert_eq!(settings.recording_keys, "");
    assert!(Destination::of("clipboard").copies() && !Destination::of("clipboard").saves());
    assert!(!Destination::of("folder").copies() && Destination::of("folder").saves());
    assert_eq!(Afterwards::of("edit"), Afterwards::Edit);
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::of(&state(&dir, &[(RECORDING, Value::Bool(true))]));
    assert_eq!(settings.recording_keys, "Ctrl + Alt + Shift + R");
}

#[test]
fn the_parts_switched_on_say_what_they_miss() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(
        &dir,
        &[
            (SCREENSHOTS, Value::Bool(true)),
            (HISTORY, Value::Bool(true)),
        ],
    );
    let names = |missing: Vec<(&'static str, Vec<tools::Program>)>| -> Vec<(&str, Vec<&str>)> {
        missing
            .into_iter()
            .map(|(key, programs)| (key, programs.iter().map(|p| p.name).collect()))
            .collect()
    };
    assert_eq!(names(tools::missing_for(&state, |_| true)), []);
    assert_eq!(
        names(tools::missing_for(&state, |_| false)),
        [
            (SCREENSHOTS, vec!["grim", "slurp", "wl-copy"]),
            (HISTORY, vec!["wl-paste", "cliphist", "wl-copy", "fuzzel"]),
        ]
    );
    // One launcher is enough.
    assert_eq!(
        names(tools::missing_for(&state, |program| program.name != "grim")),
        [(SCREENSHOTS, vec!["grim"])]
    );
    assert_eq!(
        tools::packages(&[tools::WL_COPY, tools::WL_PASTE, tools::GRIM]),
        ["wl-clipboard", "grim"]
    );
}

#[test]
fn picked_entries_get_the_type_of_their_picture() {
    use clipboard::image_type;
    assert_eq!(image_type(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
    assert_eq!(image_type(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
    assert_eq!(image_type(b"GIF89a...."), Some("image/gif"));
    assert_eq!(image_type(b"BM0123456789abcdef"), Some("image/bmp"));
    assert_eq!(image_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
    assert_eq!(image_type(b"BM"), None);
    assert_eq!(image_type(b"hello"), None);
    assert_eq!(image_type(b""), None);
}

#[test]
fn only_a_quiet_exit_status_1_is_a_canceled_launcher() {
    assert!(clipboard::canceled(Some(1), b""));
    assert!(clipboard::canceled(Some(1), b"\n"));
    assert!(!clipboard::canceled(Some(1), b"error: no display"));
    assert!(!clipboard::canceled(Some(2), b""));
    assert!(!clipboard::canceled(None, b""));
}

#[test]
fn notification_bodies_escape_markup() {
    assert_eq!(
        notify::escape("~/a & b/<x>.png"),
        "~/a &amp; b/&lt;x&gt;.png"
    );
}

#[test]
fn without_a_session_no_process_is_found() {
    // Not every process of the user: none.
    assert_eq!(crate::session::process::find_in(None, "cargo", "ANY"), []);
    assert_eq!(crate::session::process::find_in(None, "sway", "ANY"), []);
}

#[test]
fn the_users_own_history_line_is_found() {
    let files = MemoryFiles::new().var("HOME", "/home/me").file(
        "/home/me/.config/sway/config",
        "exec foot\nexec wl-paste --watch cliphist store\n",
    );
    let config = crate::sway_config::parse(Path::new("/home/me/.config/sway/config"), &files);
    let line = clipboard::own_history(&config.directives).unwrap();
    assert_eq!(line.line, 2);
}

#[test]
fn wf_recorder_tries_h264_then_vp9_in_the_container_each_needs() {
    let encoders: Vec<&str> = record::codecs(tools::WF_RECORDER)
        .iter()
        .map(|codec| codec.encoder)
        .collect();
    assert_eq!(encoders, ["libx264", "libopenh264", "libvpx-vp9"]);
    assert_eq!(record::codecs(tools::WL_SCREENREC), []);
    let [x264, _, vp9] = record::CODECS;
    // H.264 goes into the name's container, VP9 into Matroska.
    assert_eq!(record::with_container("a.b.mp4", Some(x264)), "a.b.mp4");
    assert_eq!(record::with_container("a.mkv", Some(x264)), "a.mkv");
    assert_eq!(record::with_container("a.b.mp4", Some(vp9)), "a.b.mkv");
    assert_eq!(record::with_container("a.mkv", Some(vp9)), "a.mkv");
    assert_eq!(record::with_container("a.mp4", None), "a.mp4");
}

/// What `attempts` returns: the folder, the codec and file of each attempt, and the result.
type Attempts = (
    tempfile::TempDir,
    Vec<(Option<String>, PathBuf)>,
    Result<PathBuf, CaptureError>,
);

/// The attempts of `record_with` in a new folder: for each, the codec and the file it got, and
/// what the fake recorder does then (`None` runs).
fn attempts(
    recorder: tools::Program,
    name: &str,
    outcomes: Vec<Option<record::Failure>>,
) -> Attempts {
    let dir = tempfile::tempdir().unwrap();
    let mut outcomes = outcomes.into_iter();
    let mut seen = Vec::new();
    let target = shot::Target::Geometry("0,0 11x11".to_owned());
    let result = record::record_with(recorder, &target, dir.path(), name, false, |args| {
        let codec = args
            .iter()
            .position(|arg| arg == "-c")
            .map(|index| args[index + 1].to_string_lossy().into_owned());
        let file = PathBuf::from(args.last().unwrap());
        // The file was made empty for this attempt.
        assert_eq!(std::fs::read(&file).unwrap(), b"");
        seen.push((codec, file));
        outcomes.next().unwrap().map_or(Ok(()), Err)
    });
    (dir, seen, result)
}

fn ended(message: &str) -> record::Failure {
    record::Failure::Ended(CaptureError::Failed {
        program: "wf-recorder".to_owned(),
        message: message.to_owned(),
    })
}

#[test]
fn a_recorder_that_ends_at_once_gets_the_next_codec() {
    let (dir, seen, result) = attempts(
        tools::WF_RECORDER,
        "clip.mp4",
        vec![Some(ended("Failed to find the given codec: libx264")), None],
    );
    // The first attempt's file went, so the second makes it anew.
    let file = dir.path().join("clip.mp4");
    assert_eq!(
        seen,
        [
            (Some("libx264".to_owned()), file.clone()),
            (Some("libopenh264".to_owned()), file.clone())
        ]
    );
    assert_eq!(result.unwrap(), dir.path().join("clip.mp4"));
    assert!(dir.path().join("clip.mp4").exists());

    // VP9 last, into `.mkv`; the empty files of the failed attempts go, and the last says why.
    let (dir, seen, result) = attempts(
        tools::WF_RECORDER,
        "clip.mp4",
        vec![Some(ended("one")), Some(ended("two")), None],
    );
    assert_eq!(
        seen[2],
        (Some("libvpx-vp9".to_owned()), dir.path().join("clip.mkv"))
    );
    assert_eq!(result.unwrap(), dir.path().join("clip.mkv"));
    assert!(!dir.path().join("clip.mp4").exists());
    let (dir, seen, result) = attempts(
        tools::WF_RECORDER,
        "clip.mp4",
        vec![Some(ended("one")), Some(ended("two")), Some(ended("three"))],
    );
    assert_eq!(seen.len(), 3);
    assert_eq!(result.unwrap_err().to_string(), "wf-recorder failed: three");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn other_failures_and_wl_screenrec_try_no_other_codec() {
    let (dir, seen, result) = attempts(
        tools::WF_RECORDER,
        "clip.mkv",
        vec![Some(record::Failure::Other(CaptureError::Missing {
            program: "wf-recorder",
            package: "wf-recorder",
        }))],
    );
    assert_eq!(seen.len(), 1);
    assert!(matches!(result, Err(CaptureError::Missing { .. })));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

    let (_dir, seen, result) = attempts(tools::WL_SCREENREC, "clip.mp4", vec![Some(ended("no"))]);
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, None);
    assert!(result.is_err());

    // Someone else's file of the same name is never touched: the attempt numbers its own.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("clip.mkv"), b"theirs").unwrap();
    let target = shot::Target::Output("DP-1".to_owned());
    let mut files = Vec::new();
    let result = record::record_with(
        tools::WF_RECORDER,
        &target,
        dir.path(),
        "clip.mp4",
        false,
        |args| {
            files.push(PathBuf::from(args.last().unwrap()));
            Err(record::Failure::Ended(CaptureError::Canceled))
        },
    );
    assert!(result.is_err());
    assert_eq!(files[2], dir.path().join("clip (2).mkv"));
    assert_eq!(
        std::fs::read(dir.path().join("clip.mkv")).unwrap(),
        b"theirs"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
