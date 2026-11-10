// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::{Layer, Paths, ProblemKind, State, StateError};
use crate::command::input::InputSelector;
use crate::command::{Color, Target};
use crate::registry::{Registry, Value, ValueError, Values};

const TOUCHPAD: &str = "1267:12377:ELAN1300:00_04F3:3059_Touchpad";
const TAP: &str = "input.touchpad.tap";
const GAPS: &str = "windows.gaps.inner";

/// A config directory with the given contents of the shared file and the host overlay.
struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new(shared: Option<&str>, host: Option<&str>) -> Self {
        let fixture = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        if let Some(shared) = shared {
            fs::write(fixture.path(Layer::Shared), shared).unwrap();
        }
        if let Some(host) = host {
            fs::create_dir(fixture.dir.path().join("settings.d")).unwrap();
            fs::write(fixture.path(Layer::Host), host).unwrap();
        }
        fixture
    }

    fn paths(&self) -> Paths {
        Paths::new(self.dir.path(), "laptop").unwrap()
    }

    fn path(&self, layer: Layer) -> std::path::PathBuf {
        self.paths().path(layer).to_owned()
    }

    fn load(&self) -> State {
        State::load(Registry::builtin(), self.paths())
    }

    fn text(&self, layer: Layer) -> String {
        fs::read_to_string(self.path(layer)).unwrap()
    }
}

fn problems(state: &State) -> Vec<(Layer, ProblemKind)> {
    state
        .problems()
        .into_iter()
        .map(|problem| (problem.layer, problem.kind))
        .collect()
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn missing_files_are_an_empty_state() {
    let fixture = Fixture::new(None, None);
    let mut state = fixture.load();
    assert_eq!(state.problems(), []);
    assert!(state.entries().is_empty());
    assert_eq!(state.get(TAP, None), None);
    assert!(!state.is_dirty());
    state.save().unwrap();
    assert_eq!(file_names(fixture.dir.path()), Vec::<String>::new());
}

#[test]
fn the_overlay_overrides_key_by_key() {
    let fixture = Fixture::new(
        Some("version = 1\n[input.touchpad]\ntap = true\npointer_accel = 0.5\n"),
        Some("version = 1\n[input.touchpad]\ntap = false\n[windows.gaps]\ninner = 4\n"),
    );
    let state = fixture.load();
    assert_eq!(state.problems(), []);
    assert_eq!(state.get(TAP, None), Some(&Value::Bool(false)));
    assert_eq!(state.layer(TAP, None), Some(Layer::Host));
    assert_eq!(
        state.get("input.touchpad.pointer_accel", None),
        Some(&Value::Decimal(0.5))
    );
    assert_eq!(
        state.layer("input.touchpad.pointer_accel", None),
        Some(Layer::Shared)
    );
    assert_eq!(state.get(GAPS, None), Some(&Value::Integer(4)));
    assert_eq!(state.get("windows.font", None), None);
    assert_eq!(state.layer("windows.font", None), None);

    let entries: Vec<_> = state
        .entries()
        .into_iter()
        .map(|entry| (entry.setting.key(), entry.device, entry.layer))
        .collect();
    assert_eq!(
        entries,
        [
            ("input.touchpad.pointer_accel", None, Layer::Shared),
            (TAP, None, Layer::Host),
            (GAPS, None, Layer::Host),
        ]
    );
}

#[test]
fn every_value_type_loads() {
    let fixture = Fixture::new(
        Some(
            r##"
[input.touchpad]
tap = true
pointer_accel = 1

[input.keyboard]
repeat_delay = 300
xkb_layout = ["us", "de"]

[windows]
font = "Inter 11"
focused.child_border = "#3B6E4Fcc"

[behavior]
focus_follows_mouse = "always"
"##,
        ),
        None,
    );
    let state = fixture.load();
    assert_eq!(state.problems(), []);
    let get = |key| state.get(key, None).cloned();
    assert_eq!(get(TAP), Some(Value::Bool(true)));
    // A whole number is fine for a decimal setting.
    assert_eq!(
        get("input.touchpad.pointer_accel"),
        Some(Value::Decimal(1.0))
    );
    assert_eq!(
        get("input.keyboard.repeat_delay"),
        Some(Value::Integer(300))
    );
    assert_eq!(
        get("input.keyboard.xkb_layout"),
        Some(Value::List(vec!["us".to_owned(), "de".to_owned()]))
    );
    assert_eq!(
        get("windows.font"),
        Some(Value::Text("Inter 11".to_owned()))
    );
    assert_eq!(
        get("windows.focused.child_border"),
        Some(Value::Color(Color::rgba(0x3b, 0x6e, 0x4f, 0xcc)))
    );
    assert_eq!(
        get("behavior.focus_follows_mouse"),
        Some(Value::Choice("always".to_owned()))
    );
}

#[test]
fn unknown_keys_and_invalid_values_are_reported_and_the_rest_loads() {
    let fixture = Fixture::new(
        Some(
            r##"
version = 1
theme = "red"

[input.touchpad]
tap = "yes"
pointer_accel = 1.5
natural = true

[input.keyboard]
repeat_delay = 300
xkb_layout = ["us", 3]

[windows.focused]
border = "blue"
text = "#ffffff"

[behavior]
focus_follows_mouse = "sometimes"

[effects]
corner_radius = 100

[nothing]
"##,
        ),
        None,
    );
    let state = fixture.load();
    let unknown = |key: &str| {
        (
            Layer::Shared,
            ProblemKind::UnknownKey {
                key: key.to_owned(),
                device: None,
            },
        )
    };
    let invalid = |key, error| {
        (
            Layer::Shared,
            ProblemKind::InvalidValue {
                key,
                device: None,
                error,
            },
        )
    };
    let wrong_type = |expected, found| ValueError::WrongType { expected, found };
    let found = problems(&state);
    let focus_choices = match &found[6].1 {
        ProblemKind::InvalidValue {
            error: ValueError::UnknownChoice { choices },
            ..
        } => *choices,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        found,
        [
            unknown("theme"),
            invalid(TAP, wrong_type("boolean", "string")),
            invalid(
                "input.touchpad.pointer_accel",
                ValueError::DecimalOutOfRange {
                    min: -1.0,
                    max: 1.0
                }
            ),
            unknown("input.touchpad.natural"),
            invalid("input.keyboard.xkb_layout", wrong_type("list", "array")),
            invalid("windows.focused.border", ValueError::NotAColor),
            invalid(
                "behavior.focus_follows_mouse",
                ValueError::UnknownChoice {
                    choices: focus_choices
                }
            ),
            invalid(
                "effects.corner_radius",
                ValueError::IntegerOutOfRange { min: 0, max: 99 }
            ),
            unknown("nothing"),
        ]
    );
    assert_eq!(
        state.problems()[1].to_string(),
        format!(
            "{}: input.touchpad.tap: expected a boolean value, found a string value",
            fixture.path(Layer::Shared).display()
        )
    );

    // An invalid value counts as not stored.
    assert_eq!(state.get(TAP, None), None);
    assert_eq!(
        state.get("input.keyboard.repeat_delay", None),
        Some(&Value::Integer(300))
    );
    assert_eq!(
        state.get("windows.focused.text", None),
        Some(&Value::Color(Color::rgb(0xff, 0xff, 0xff)))
    );
}

#[test]
fn device_tables_hold_per_device_values() {
    let fixture = Fixture::new(
        Some("[input.touchpad]\ntap = true\npointer_accel = 0.2\n"),
        Some(&format!(
            r#"
[device."{TOUCHPAD}".input.touchpad]
tap = false

[device."{TOUCHPAD}".windows.gaps]
inner = 3

[device.touchpad.input.touchpad]
tap = false

[device]
"1:2:x" = 5
"#
        )),
    );
    let state = fixture.load();
    assert_eq!(
        problems(&state),
        [
            (
                Layer::Host,
                ProblemKind::NotPerDevice {
                    key: GAPS,
                    device: TOUCHPAD.to_owned()
                }
            ),
            (
                Layer::Host,
                ProblemKind::InvalidDevice {
                    device: "touchpad".to_owned()
                }
            ),
            (
                Layer::Host,
                ProblemKind::InvalidDevice {
                    device: "1:2:x".to_owned()
                }
            ),
        ]
    );
    assert_eq!(state.get(TAP, None), Some(&Value::Bool(true)));
    assert_eq!(state.get(TAP, Some(TOUCHPAD)), Some(&Value::Bool(false)));
    assert_eq!(state.layer(TAP, Some(TOUCHPAD)), Some(Layer::Host));
    // Only what is stored for the device itself.
    assert_eq!(
        state.get("input.touchpad.pointer_accel", Some(TOUCHPAD)),
        None
    );

    let entries: Vec<_> = state
        .entries()
        .into_iter()
        .map(|entry| (entry.setting.key(), entry.device))
        .collect();
    assert_eq!(
        entries,
        [
            ("input.touchpad.pointer_accel", None),
            (TAP, None),
            (TAP, Some(TOUCHPAD)),
        ]
    );
}

#[test]
fn values_render_through_the_registry() {
    let fixture = Fixture::new(
        Some("[input.touchpad]\ntap = true\n[windows.focused]\nchild_border = \"#3b6e4f\"\n"),
        Some(&format!(
            "[device.\"{TOUCHPAD}\".input.touchpad]\ntap = false\n"
        )),
    );
    let state = fixture.load();
    let registry = Registry::builtin();
    let render = |key: &str, device: Option<&str>| {
        let selector = device.map(|device| InputSelector::device(device).unwrap());
        registry
            .render(
                registry.get(key).unwrap(),
                &state.values(device),
                selector.as_ref(),
                Target::Config,
            )
            .unwrap()
            .iter()
            .map(|command| command.render(Target::Config).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(render(TAP, None), ["input type:touchpad tap enabled"]);
    assert_eq!(
        render(TAP, Some(TOUCHPAD)),
        [format!("input \"{TOUCHPAD}\" tap disabled")]
    );
    assert_eq!(
        render("windows.focused.border", None),
        ["client.focused #4c7899 #285577 #ffffff #2e9ef4 #3b6e4f"]
    );
    // A device without a value of its own shows the value of its type.
    let other = "1:1:Other_Touchpad";
    assert_eq!(state.values(Some(other)).get(TAP), Some(&Value::Bool(true)));
}

#[test]
fn a_file_of_another_version_is_reported_and_left_alone() {
    for (text, found) in [
        ("version = 2\n[input.touchpad]\ntap = true\n", "2"),
        ("version = \"1\"\n", "\"1\""),
        ("version = 0\n", "0"),
    ] {
        let fixture = Fixture::new(Some(text), None);
        let mut state = fixture.load();
        assert_eq!(
            problems(&state),
            [(
                Layer::Shared,
                ProblemKind::UnsupportedVersion {
                    found: found.to_owned()
                }
            )]
        );
        assert_eq!(state.get(TAP, None), None);
        assert!(matches!(
            state.set(TAP, None, Value::Bool(false), None),
            Err(StateError::Unusable { path }) if path == fixture.path(Layer::Shared)
        ));
        // The overlay still works.
        state
            .set(TAP, None, Value::Bool(false), Some(Layer::Host))
            .unwrap();
        state.save().unwrap();
        assert_eq!(fixture.text(Layer::Shared), text);
        assert_eq!(
            fixture.text(Layer::Host),
            "version = 1\n\n[input.touchpad]\ntap = false\n"
        );
    }
}

#[test]
fn a_file_with_a_syntax_error_is_reported_and_left_alone() {
    let text = "[input.touchpad\ntap = true\n";
    let fixture = Fixture::new(None, Some(text));
    let mut state = fixture.load();
    let found = problems(&state);
    assert!(
        matches!(
            &found[..],
            [(Layer::Host, ProblemKind::Syntax { message })] if message.contains("line 1")
        ),
        "{found:?}"
    );
    assert!(matches!(
        state.set(TAP, Some(TOUCHPAD), Value::Bool(true), None),
        Err(StateError::Unusable { .. })
    ));
    assert!(!state.is_dirty());
    state.save().unwrap();
    assert_eq!(fixture.text(Layer::Host), text);
}

#[test]
fn a_new_value_goes_to_the_file_of_its_scope() {
    let registry = Registry::builtin();
    let default = |key: &str, device| Layer::default_for(registry.get(key).unwrap(), device);
    assert_eq!(default(GAPS, None), Layer::Shared);
    assert_eq!(default(TAP, None), Layer::Shared);
    assert_eq!(default(TAP, Some(TOUCHPAD)), Layer::Host);

    let fixture = Fixture::new(None, None);
    let mut state = fixture.load();
    state.set(GAPS, None, Value::Integer(8), None).unwrap();
    state.set(TAP, None, Value::Bool(true), None).unwrap();
    state
        .set(TAP, Some(TOUCHPAD), Value::Bool(false), None)
        .unwrap();
    assert!(state.is_dirty());
    state.save().unwrap();
    assert!(!state.is_dirty());
    assert_eq!(
        fixture.text(Layer::Shared),
        "version = 1\n\n[windows.gaps]\ninner = 8\n\n[input.touchpad]\ntap = true\n"
    );
    assert_eq!(
        fixture.text(Layer::Host),
        format!("version = 1\n\n[device.\"{TOUCHPAD}\".input.touchpad]\ntap = false\n")
    );
    assert_eq!(
        file_names(fixture.dir.path()),
        ["settings.d", "settings.toml"]
    );
    assert_eq!(
        file_names(&fixture.dir.path().join("settings.d")),
        ["laptop.toml"]
    );

    // What was saved loads again.
    let reloaded = fixture.load();
    assert_eq!(reloaded.problems(), []);
    assert_eq!(reloaded.get(GAPS, None), Some(&Value::Integer(8)));
    assert_eq!(reloaded.get(TAP, Some(TOUCHPAD)), Some(&Value::Bool(false)));
}

#[test]
fn a_changed_value_stays_in_its_file() {
    let fixture = Fixture::new(
        Some("version = 1\n[input.touchpad]\ntap = true\n"),
        Some("version = 1\n[windows.gaps]\ninner = 4\n"),
    );
    let mut state = fixture.load();
    state.set(GAPS, None, Value::Integer(6), None).unwrap();
    state.set(TAP, None, Value::Bool(false), None).unwrap();
    assert_eq!(state.layer(GAPS, None), Some(Layer::Host));
    assert_eq!(state.layer(TAP, None), Some(Layer::Shared));
    state.save().unwrap();
    assert_eq!(
        fixture.text(Layer::Host),
        "version = 1\n[windows.gaps]\ninner = 6\n"
    );
    assert_eq!(
        fixture.text(Layer::Shared),
        "version = 1\n[input.touchpad]\ntap = false\n"
    );
}

#[test]
fn this_machine_only_moves_the_value() {
    let fixture = Fixture::new(Some("version = 1\n[windows.gaps]\ninner = 4\n"), None);
    let mut state = fixture.load();
    assert!(state.move_to(GAPS, None, Layer::Host).unwrap());
    assert_eq!(state.layer(GAPS, None), Some(Layer::Host));
    assert_eq!(state.get(GAPS, None), Some(&Value::Integer(4)));
    state.save().unwrap();
    assert_eq!(fixture.text(Layer::Shared), "version = 1\n");
    assert_eq!(
        fixture.text(Layer::Host),
        "version = 1\n\n[windows.gaps]\ninner = 4\n"
    );

    assert!(state.move_to(GAPS, None, Layer::Shared).unwrap());
    state.save().unwrap();
    assert_eq!(
        fixture.text(Layer::Shared),
        "version = 1\n\n[windows.gaps]\ninner = 4\n"
    );
    assert_eq!(fixture.text(Layer::Host), "version = 1\n");
    assert!(!state.move_to(TAP, None, Layer::Host).unwrap());
    assert!(!state.is_dirty());
}

#[test]
fn a_value_for_the_shared_file_replaces_the_one_in_the_overlay() {
    let fixture = Fixture::new(
        Some("[windows.gaps]\ninner = 4\n"),
        Some("[windows.gaps]\ninner = 6\n"),
    );
    let mut state = fixture.load();
    state
        .set(GAPS, None, Value::Integer(8), Some(Layer::Shared))
        .unwrap();
    assert_eq!(state.get(GAPS, None), Some(&Value::Integer(8)));
    assert_eq!(state.layer(GAPS, None), Some(Layer::Shared));
    // A value for the overlay leaves the shared one for the other machines.
    state
        .set(GAPS, None, Value::Integer(2), Some(Layer::Host))
        .unwrap();
    state.save().unwrap();
    assert_eq!(
        fixture.text(Layer::Shared),
        "version = 1\n\n[windows.gaps]\ninner = 8\n"
    );
    assert_eq!(
        fixture.text(Layer::Host),
        "version = 1\n\n[windows.gaps]\ninner = 2\n"
    );
}

#[test]
fn reset_removes_the_key_from_both_files() {
    let fixture = Fixture::new(
        Some("version = 1\n\n[windows.gaps]\ninner = 4\n\n[windows.focused]\ntext = \"#000000\"\n"),
        Some(&format!(
            "version = 1\n\n[windows.gaps]\ninner = \"wide\"\n\n[device.\"{TOUCHPAD}\".input.touchpad]\ntap = true\n"
        )),
    );
    let mut state = fixture.load();
    assert_eq!(state.problems().len(), 1);
    assert!(state.reset(GAPS, None).unwrap());
    assert!(state.reset(TAP, Some(TOUCHPAD)).unwrap());
    assert!(!state.reset(TAP, None).unwrap());
    assert_eq!(state.problems(), []);
    assert_eq!(state.get(GAPS, None), None);
    assert!(state.entries().iter().all(|entry| entry.device.is_none()));
    state.save().unwrap();
    // Tables that are empty now go away too.
    assert_eq!(
        fixture.text(Layer::Shared),
        "version = 1\n\n[windows.focused]\ntext = \"#000000\"\n"
    );
    assert_eq!(fixture.text(Layer::Host), "version = 1\n");
}

#[test]
fn hand_edited_files_keep_their_comments_order_and_unknown_keys() {
    let text = r##"# My sway settings, shared between laptop and desktop.

[windows.gaps]
inner = 4 # roomy

# Colors from my theme
[windows.focused]
child_border = "#3b6e4f"
text = "#ffffff"   # keep readable

[input]
touchpad.tap = true
keyboard = { repeat_delay = 300 }

[future]
setting = "kept"
"##;
    let fixture = Fixture::new(Some(text), None);
    let mut state = fixture.load();
    assert_eq!(state.problems().len(), 1);
    assert_eq!(state.get(TAP, None), Some(&Value::Bool(true)));
    assert_eq!(
        state.get("input.keyboard.repeat_delay", None),
        Some(&Value::Integer(300))
    );

    // Nothing changed: nothing is written, not even the version.
    state.save().unwrap();
    assert_eq!(fixture.text(Layer::Shared), text);

    state.set(GAPS, None, Value::Integer(8), None).unwrap();
    state
        .set(
            "windows.focused.text",
            None,
            Value::Color(Color::rgb(0xee, 0xee, 0xee)),
            None,
        )
        .unwrap();
    state.set(TAP, None, Value::Bool(false), None).unwrap();
    state
        .set(
            "input.keyboard.repeat_delay",
            None,
            Value::Integer(250),
            None,
        )
        .unwrap();
    state.reset("windows.focused.child_border", None).unwrap();
    state
        .set(
            "windows.font",
            None,
            Value::Text("Inter 11".to_owned()),
            None,
        )
        .unwrap();
    state.save().unwrap();
    assert_eq!(
        fixture.text(Layer::Shared),
        r##"# My sway settings, shared between laptop and desktop.
version = 1

[windows]
font = "Inter 11"

[windows.gaps]
inner = 8 # roomy

# Colors from my theme
[windows.focused]
text = "#eeeeee"   # keep readable

[input]
touchpad.tap = false
keyboard = { repeat_delay = 250 }

[future]
setting = "kept"
"##
    );
    assert_eq!(fixture.load().problems().len(), 1);
}

#[test]
fn a_value_in_the_way_of_a_table_is_an_error() {
    let fixture = Fixture::new(Some("input = 1\n"), None);
    let mut state = fixture.load();
    assert!(matches!(
        state.set(TAP, None, Value::Bool(true), None),
        Err(StateError::NotATable { key: TAP, .. })
    ));
    assert!(!state.is_dirty());
}

#[test]
fn changes_are_checked_against_the_registry() {
    let fixture = Fixture::new(None, None);
    let mut state = fixture.load();
    assert!(matches!(
        state.set("input.touchpad.tapp", None, Value::Bool(true), None),
        Err(StateError::UnknownKey { key }) if key == "input.touchpad.tapp"
    ));
    assert!(matches!(
        state.set(GAPS, None, Value::Integer(-1), None),
        Err(StateError::InvalidValue {
            key: GAPS,
            error: ValueError::IntegerOutOfRange { .. }
        })
    ));
    assert!(matches!(
        state.set(GAPS, Some(TOUCHPAD), Value::Integer(1), None),
        Err(StateError::NotPerDevice { key: GAPS })
    ));
    assert!(matches!(
        state.set(TAP, Some("type:touchpad"), Value::Bool(true), None),
        Err(StateError::InvalidDevice { .. })
    ));
    assert!(matches!(
        state.reset("nope", None),
        Err(StateError::UnknownKey { .. })
    ));
    assert!(!state.is_dirty());
    assert_eq!(
        StateError::InvalidValue {
            key: GAPS,
            error: ValueError::IntegerOutOfRange { min: 0, max: 32767 }
        }
        .to_string(),
        "windows.gaps.inner: must be between 0 and 32767"
    );
}

#[test]
fn a_linked_file_stays_a_link() {
    let fixture = Fixture::new(None, None);
    let dotfiles = tempfile::tempdir().unwrap();
    let real = dotfiles.path().join("settings.toml");
    fs::write(&real, "version = 1\n").unwrap();
    std::os::unix::fs::symlink(&real, fixture.path(Layer::Shared)).unwrap();

    let mut state = fixture.load();
    state.set(GAPS, None, Value::Integer(8), None).unwrap();
    state.save().unwrap();
    assert!(
        fs::symlink_metadata(fixture.path(Layer::Shared))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(&real).unwrap(),
        "version = 1\n\n[windows.gaps]\ninner = 8\n"
    );
    // No temporary file is left behind.
    assert_eq!(file_names(dotfiles.path()), ["settings.toml"]);
    assert_eq!(file_names(fixture.dir.path()), ["settings.toml"]);
}

#[test]
fn a_failed_save_keeps_the_state_dirty() {
    let fixture = Fixture::new(None, None);
    // A directory is where the file should be.
    fs::create_dir(fixture.path(Layer::Shared)).unwrap();
    let mut state = fixture.load();
    assert!(matches!(
        &problems(&state)[..],
        [(Layer::Shared, ProblemKind::Unreadable { .. })]
    ));
    assert!(matches!(
        state.set(GAPS, None, Value::Integer(8), None),
        Err(StateError::Unusable { .. })
    ));

    let fixture = Fixture::new(None, None);
    let mut state = fixture.load();
    state.set(GAPS, None, Value::Integer(8), None).unwrap();
    fs::create_dir(fixture.path(Layer::Shared)).unwrap();
    assert!(matches!(state.save(), Err(StateError::Io { .. })));
    assert!(state.is_dirty());
    assert_eq!(file_names(fixture.dir.path()), ["settings.toml"]);
}
