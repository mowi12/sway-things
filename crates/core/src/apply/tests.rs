// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use async_io::block_on;

use super::{Applied, Applier, ApplyError, Effect, SAVE_DELAY};
use crate::devices::Lockout;
use crate::generate::tests::{FakeValidator, SWAY_1_10, SWAYFX_0_5, TOUCHPAD, state};
use crate::generate::{ConfigError, HEADER, Locations, Outcome};
use crate::include::WriteAccess;
use crate::ipc::tests::{FakeSway, message, obedient};
use crate::ipc::{IpcError, Unmet};
use crate::registry::{ConfigFile, Value};
use crate::state::{ChangedKey, Layer, StateError};
use crate::version::SwayFxVersion;

const SWAY: &str = crate::ipc::tests::SWAY_1_10;
const GAPS: &str = "windows.gaps.inner";
const LAYOUT: &str = "input.keyboard.xkb_layout";
const RADIUS: &str = "effects.corner_radius";

/// An applier on settings files with the given contents, with the directory they are in.
struct Setup {
    dir: tempfile::TempDir,
    applier: Applier<FakeValidator>,
    access: WriteAccess,
}

impl Setup {
    fn parts(shared: &str) -> (tempfile::TempDir, crate::state::State, Locations) {
        let (dir, state) = state(shared, "version = 1\n");
        let locations = Locations::new(
            &dir.path().join("config/sway/sway-things"),
            &dir.path().join("state"),
        );
        (dir, state, locations)
    }

    /// In the session of `sway`.
    fn live(sway: &FakeSway, shared: &str) -> Self {
        let (dir, state, locations) = Self::parts(shared);
        Self {
            dir,
            applier: Applier::with_session(
                state,
                locations,
                FakeValidator::default(),
                sway.connect(),
            ),
            access: WriteAccess::for_tests(),
        }
    }

    /// Without a session.
    fn offline(shared: &str) -> Self {
        let (dir, state, locations) = Self::parts(shared);
        Self {
            dir,
            applier: Applier::without_session(
                state,
                locations,
                FakeValidator::default(),
                SWAY_1_10,
            ),
            access: WriteAccess::for_tests(),
        }
    }

    fn set(&mut self, key: &str, value: Value) -> Result<Applied, ApplyError> {
        block_on(self.applier.set(key, None, value, &self.access))
    }

    fn reset(&mut self, key: &str) -> Result<Applied, ApplyError> {
        block_on(self.applier.reset(key, None, &self.access))
    }

    fn persist(&mut self, overwrite: &[ConfigFile]) -> super::Persisted {
        block_on(self.applier.persist(overwrite, &self.access)).unwrap()
    }

    fn settings(&self) -> PathBuf {
        self.dir.path().join("config/settings.toml")
    }

    fn generated(&self, file: ConfigFile) -> String {
        fs::read_to_string(self.applier.locations().path(file)).unwrap_or_default()
    }
}

fn live(commands: &[&str]) -> Effect {
    Effect::Live {
        commands: commands
            .iter()
            .map(|command| (*command).to_owned())
            .collect(),
    }
}

#[test]
fn a_change_is_sent_stored_saved_and_generated() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n");
    let result = setup.set(GAPS, Value::Integer(8)).unwrap();
    assert_eq!(
        result,
        Applied {
            changed: true,
            effect: live(&["gaps inner 8", "gaps inner all set 8"]),
        }
    );
    assert_eq!(sway.commands(), ["gaps inner 8", "gaps inner all set 8"]);
    assert_eq!(
        setup.applier.state().get(GAPS, None),
        Some(&Value::Integer(8))
    );
    // Nothing is on disk before `persist`.
    assert_eq!(
        fs::read_to_string(setup.settings()).unwrap(),
        "version = 1\n"
    );
    assert_eq!(setup.generated(ConfigFile::Appearance), "");

    let persisted = setup.persist(&[]);
    assert!(persisted.saved);
    assert!(persisted.is_complete());
    assert_eq!(
        persisted.files,
        [
            (ConfigFile::Input, Outcome::NotCreated),
            (ConfigFile::Appearance, Outcome::Written),
            (ConfigFile::Behavior, Outcome::NotCreated),
            (ConfigFile::Effects, Outcome::NotCreated),
        ]
    );
    assert_eq!(
        fs::read_to_string(setup.settings()).unwrap(),
        "version = 1\n\n[windows.gaps]\ninner = 8\n"
    );
    assert_eq!(
        setup.generated(ConfigFile::Appearance),
        format!("{HEADER}gaps inner 8\n")
    );

    // The same value again: sent, but there is nothing to save.
    let result = setup.set(GAPS, Value::Integer(8)).unwrap();
    assert!(!result.changed);
    assert_eq!(setup.applier.save_due(), None);
    let persisted = setup.persist(&[]);
    assert!(!persisted.saved);
    assert_eq!(
        persisted.files[1],
        (ConfigFile::Appearance, Outcome::Unchanged)
    );
}

#[test]
fn a_rejected_command_changes_nothing() {
    let sway = FakeSway::with_version(SWAY, |kind, payload| {
        let reply = if payload.starts_with("gaps inner 12") {
            r#"[{"success": false, "error": "No"}]"#
        } else {
            r#"[{"success": true}]"#
        };
        message(kind, reply)
    });
    let mut setup = Setup::live(&sway, "version = 1\n\n[windows.gaps]\ninner = 8\n");
    match setup.set(GAPS, Value::Integer(12)) {
        Err(ApplyError::Ipc(IpcError::CommandFailed { command, .. })) => {
            assert_eq!(command, "gaps inner 12");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        setup.applier.state().get(GAPS, None),
        Some(&Value::Integer(8))
    );
    assert!(!setup.applier.state().is_dirty());
    assert_eq!(setup.applier.save_due(), None);
    // Nothing was sent before the failure, so nothing is undone.
    assert_eq!(sway.commands(), ["gaps inner 12"]);
}

#[test]
fn commands_sent_before_a_failure_are_undone() {
    let sway = FakeSway::with_version(SWAY, |kind, payload| {
        let reply = if payload == "gaps inner all set 12" {
            r#"[{"success": false, "error": "No"}]"#
        } else {
            r#"[{"success": true}]"#
        };
        message(kind, reply)
    });
    let mut setup = Setup::live(&sway, "version = 1\n\n[windows.gaps]\ninner = 8\n");
    assert!(matches!(
        setup.set(GAPS, Value::Integer(12)),
        Err(ApplyError::Ipc(IpcError::CommandFailed { .. }))
    ));
    assert_eq!(
        sway.commands(),
        [
            "gaps inner 12",
            "gaps inner all set 12",
            "gaps inner 8",
            "gaps inner all set 8"
        ]
    );
    assert_eq!(
        setup.applier.state().get(GAPS, None),
        Some(&Value::Integer(8))
    );
}

#[test]
fn without_a_session_the_value_is_saved_and_generated() {
    let mut setup = Setup::offline("version = 1\n");
    let result = setup.set(GAPS, Value::Integer(8)).unwrap();
    assert_eq!(result.effect, Effect::NoSession);
    assert!(setup.persist(&[]).is_complete());
    assert_eq!(
        setup.generated(ConfigFile::Appearance),
        format!("{HEADER}gaps inner 8\n")
    );
}

#[test]
fn what_doesnt_fit_the_registry_is_refused() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n");
    assert!(matches!(
        setup.set("windows.gaps.diagonal", Value::Integer(8)),
        Err(ApplyError::State(StateError::UnknownKey { .. }))
    ));
    assert!(matches!(
        setup.set(GAPS, Value::Integer(-1)),
        Err(ApplyError::State(StateError::InvalidValue { .. }))
    ));
    assert!(matches!(
        block_on(
            setup
                .applier
                .set(GAPS, Some(TOUCHPAD), Value::Integer(8), &setup.access)
        ),
        Err(ApplyError::State(StateError::NotPerDevice { .. }))
    ));
    // The builders reject a backslash, which the value type allows.
    assert!(matches!(
        setup.set("windows.font", Value::Text("Back\\slash 10".to_owned())),
        Err(ApplyError::Render(_))
    ));
    assert_eq!(sway.commands(), [] as [&str; 0]);
    assert!(!setup.applier.state().is_dirty());
    assert_eq!(setup.applier.state().entries().len(), 0);
}

#[test]
fn a_setting_for_the_config_only_needs_a_reload() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n");
    let result = setup
        .set(
            "behavior.workspace_layout",
            Value::Choice("tabbed".to_owned()),
        )
        .unwrap();
    assert_eq!(result.effect, Effect::AfterReload);
    assert_eq!(sway.commands(), [] as [&str; 0]);
    setup.persist(&[]);
    assert_eq!(
        setup.generated(ConfigFile::Behavior),
        format!("{HEADER}workspace_layout tabbed\n")
    );
}

#[test]
fn risky_settings_are_left_to_the_safety_net() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[input.keyboard]\nxkb_layout = [\"de\"]\n",
    );
    let layouts = Value::List(vec!["us".to_owned()]);
    assert!(matches!(
        setup.set(LAYOUT, layouts),
        Err(ApplyError::Risky { key: LAYOUT })
    ));
    assert!(matches!(
        setup.reset(LAYOUT),
        Err(ApplyError::Risky { key: LAYOUT })
    ));
    assert_eq!(sway.commands(), [] as [&str; 0]);
    assert!(setup.applier.state().get(LAYOUT, None).is_some());
}

#[test]
fn what_the_compositor_doesnt_support_is_refused_but_can_be_reset() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n\n[effects]\ncorner_radius = 8\n");
    match setup.set(RADIUS, Value::Integer(4)) {
        Err(ApplyError::Unsupported { key, unmet }) => {
            assert_eq!(key, RADIUS);
            assert_eq!(
                unmet,
                Unmet::SwayFx {
                    needed: SwayFxVersion::OLDEST_SUPPORTED
                }
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        setup.applier.state().get(RADIUS, None),
        Some(&Value::Integer(8))
    );

    let result = setup.reset(RADIUS).unwrap();
    assert!(result.changed);
    assert!(matches!(result.effect, Effect::Unsupported(_)));
    assert_eq!(setup.applier.state().get(RADIUS, None), None);
    assert_eq!(sway.commands(), [] as [&str; 0]);

    // SwayFX takes it.
    let (_dir, state, locations) = Setup::parts("version = 1\n");
    let mut applier =
        Applier::without_session(state, locations, FakeValidator::default(), SWAYFX_0_5);
    let result = block_on(applier.set(RADIUS, None, Value::Integer(4), &setup.access)).unwrap();
    assert_eq!(result.effect, Effect::NoSession);
}

#[test]
fn a_value_moves_between_the_files_without_sending_anything() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n\n[windows.gaps]\ninner = 8\n");
    let moved = setup
        .applier
        .move_to(GAPS, None, Layer::Host, &setup.access)
        .unwrap();
    assert!(moved);
    assert_eq!(setup.applier.state().layer(GAPS, None), Some(Layer::Host));
    assert!(setup.applier.save_due().is_some());
    assert_eq!(sway.commands(), [] as [&str; 0]);
    setup.persist(&[]);
    let host = fs::read_to_string(setup.dir.path().join("config/settings.d/laptop.toml")).unwrap();
    assert!(host.contains("inner = 8"), "{host}");
    assert_eq!(
        fs::read_to_string(setup.settings()).unwrap(),
        "version = 1\n"
    );

    // A value that isn't stored has nothing to move.
    let moved = setup
        .applier
        .move_to(RADIUS, None, Layer::Shared, &setup.access)
        .unwrap();
    assert!(!moved);
}

#[test]
fn a_reset_sends_the_default_and_removes_the_value() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[windows.gaps]\ninner = 8\n\n[windows.focused]\ntext = \"#000000\"\n\
         child_border = \"#3b6e4f\"\n",
    );
    setup.persist(&[]);
    let result = setup.reset(GAPS).unwrap();
    assert_eq!(
        result,
        Applied {
            changed: true,
            effect: live(&["gaps inner 0", "gaps inner all set 0"]),
        }
    );
    // One value of a directive: the others stay as stored.
    let result = setup.reset("windows.focused.text").unwrap();
    assert_eq!(
        result.effect,
        live(&["client.focused #4c7899 #285577 #ffffff #2e9ef4 #3b6e4f"])
    );
    let persisted = setup.persist(&[]);
    assert!(persisted.saved);
    assert_eq!(
        fs::read_to_string(setup.settings()).unwrap(),
        "version = 1\n\n[windows.focused]\nchild_border = \"#3b6e4f\"\n"
    );
    assert_eq!(
        setup.generated(ConfigFile::Appearance),
        format!("{HEADER}client.focused #4c7899 #285577 #ffffff #2e9ef4 #3b6e4f\n")
    );

    // What isn't stored isn't ours to change.
    let sent = sway.commands().len();
    let result = setup.reset(GAPS).unwrap();
    assert_eq!(
        result,
        Applied {
            changed: false,
            effect: Effect::NotStored,
        }
    );
    assert_eq!(sway.commands().len(), sent);
    assert_eq!(setup.applier.save_due(), None);
}

#[test]
fn a_value_for_one_device_goes_to_that_device() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n\n[input.touchpad]\ntap = true\n");
    let tap = "input.touchpad.tap";
    let result = block_on(setup.applier.set(
        tap,
        Some(TOUCHPAD),
        Value::Bool(false),
        &setup.access,
    ))
    .unwrap();
    assert_eq!(
        result.effect,
        live(&[&format!("input \"{TOUCHPAD}\" tap disabled")])
    );
    assert_eq!(
        setup.applier.state().layer(tap, Some(TOUCHPAD)),
        Some(Layer::Host)
    );
    setup.persist(&[]);
    assert_eq!(
        setup.generated(ConfigFile::Input),
        format!("{HEADER}input type:touchpad tap enabled\ninput \"{TOUCHPAD}\" tap disabled\n")
    );

    // Without a value of its own, the device follows its type again.
    let result = block_on(setup.applier.reset(tap, Some(TOUCHPAD), &setup.access)).unwrap();
    assert_eq!(
        result.effect,
        live(&[&format!("input \"{TOUCHPAD}\" tap enabled")])
    );
}

#[test]
fn a_preference_of_the_app_is_only_saved() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n");
    let result = setup
        .set("app.safety_net_timeout", Value::Integer(30))
        .unwrap();
    assert_eq!(result.effect, Effect::Preference);
    let persisted = setup.persist(&[]);
    assert!(persisted.saved);
    assert!(
        persisted
            .files
            .iter()
            .all(|(_, outcome)| *outcome == Outcome::NotCreated)
    );
    assert_eq!(sway.commands(), [] as [&str; 0]);
}

#[test]
fn saving_is_due_half_a_second_after_the_last_change() {
    let mut setup = Setup::offline("version = 1\n");
    assert_eq!(setup.applier.save_due(), None);
    let before = Instant::now();
    setup.set(GAPS, Value::Integer(8)).unwrap();
    let first = setup.applier.save_due().unwrap();
    assert!(first >= before + SAVE_DELAY);
    setup.set(GAPS, Value::Integer(9)).unwrap();
    assert!(setup.applier.save_due().unwrap() >= first);
    // A change that failed doesn't count.
    let due = setup.applier.save_due();
    assert!(setup.set(GAPS, Value::Integer(-1)).is_err());
    assert_eq!(setup.applier.save_due(), due);

    setup.persist(&[]);
    assert_eq!(setup.applier.save_due(), None);
    assert_eq!(
        fs::read_to_string(setup.settings()).unwrap(),
        "version = 1\n\n[windows.gaps]\ninner = 9\n"
    );
}

#[test]
fn files_that_cant_be_written_are_reported() {
    let mut setup = Setup::offline("version = 1\n\n[windows.gaps]\ninner = 8\n");
    setup.persist(&[]);
    let appearance = setup.applier.locations().path(ConfigFile::Appearance);
    fs::write(&appearance, format!("{HEADER}gaps inner 30\n")).unwrap();

    setup.set(GAPS, Value::Integer(9)).unwrap();
    let persisted = setup.persist(&[]);
    assert!(persisted.saved);
    assert!(!persisted.is_complete());
    assert_eq!(
        persisted.problems().collect::<Vec<_>>(),
        [(ConfigFile::Appearance, &Outcome::EditedOutside)]
    );
    // After the user agreed.
    let persisted = setup.persist(&[ConfigFile::Appearance]);
    assert!(!persisted.saved);
    assert!(persisted.is_complete());
    assert_eq!(
        setup.generated(ConfigFile::Appearance),
        format!("{HEADER}gaps inner 9\n")
    );

    // sway rejects the file: the old one stays, and the error names the setting.
    let (_dir, state, locations) = Setup::parts("version = 1\n");
    let validator = FakeValidator {
        errors: vec![ConfigError {
            line: 3,
            message: "Invalid".to_owned(),
        }],
        ..FakeValidator::default()
    };
    let mut applier = Applier::without_session(state, locations, validator, SWAY_1_10);
    block_on(applier.set(GAPS, None, Value::Integer(8), &setup.access)).unwrap();
    let persisted = block_on(applier.persist(&[], &setup.access)).unwrap();
    match persisted.problems().next() {
        Some((ConfigFile::Appearance, Outcome::Invalid(lines))) => {
            assert_eq!(lines[0].source.as_ref().unwrap().keys, [GAPS]);
        }
        other => panic!("{other:?}"),
    }

    // Without sway to validate, the settings are saved and no file is replaced.
    let (dir, state, locations) = Setup::parts("version = 1\n");
    let validator = FakeValidator {
        missing: true,
        ..FakeValidator::default()
    };
    let mut applier = Applier::without_session(state, locations, validator, SWAY_1_10);
    block_on(applier.set(GAPS, None, Value::Integer(8), &setup.access)).unwrap();
    assert!(matches!(
        block_on(applier.persist(&[], &setup.access)),
        Err(ApplyError::Write(_))
    ));
    assert!(!applier.state().is_dirty());
    assert!(
        fs::read_to_string(dir.path().join("config/settings.toml"))
            .unwrap()
            .contains("inner = 8")
    );
}

#[test]
fn changes_on_disk_are_sent_and_regenerated_except_risky_ones() {
    let sway = FakeSway::with_version(SWAY, |kind, payload| {
        let reply = if payload.starts_with("client.focused") {
            r#"[{"success": false, "error": "No"}]"#
        } else {
            r#"[{"success": true}]"#
        };
        message(kind, reply)
    });
    let mut setup = Setup::live(&sway, "version = 1\n\n[windows.gaps]\ninner = 8\n");
    setup.persist(&[]);
    assert_eq!(
        block_on(setup.applier.reload(&setup.access)).applied.len(),
        0
    );
    assert_eq!(setup.applier.save_due(), None);

    fs::write(
        setup.settings(),
        "version = 1\n\n[input.keyboard]\nxkb_layout = [\"de\"]\n\n[windows.focused]\n\
         text = \"#000000\"\nborder = \"#111111\"\n",
    )
    .unwrap();
    let reloaded = block_on(setup.applier.reload(&setup.access));
    assert_eq!(reloaded.changes.layers, [Layer::Shared]);
    let key = |key| ChangedKey { device: None, key };
    let result = &reloaded.applied;
    assert_eq!(result.len(), 3, "{result:?}");
    // The risky layout waits for a reload of sway.
    assert_eq!(result[0].0, key(LAYOUT));
    assert!(matches!(result[0].1, Ok(Effect::AfterReload)));
    // The two colors are one directive; its command fails, and the file still wins.
    assert_eq!(result[1].0, key("windows.focused.border"));
    assert!(matches!(result[1].1, Err(ApplyError::Ipc(_))));
    // The removed gaps are back at the default.
    assert_eq!(result[2].0, key(GAPS));
    assert_eq!(
        result[2].1.as_ref().unwrap(),
        &live(&["gaps inner 0", "gaps inner all set 0"])
    );
    assert!(
        !sway
            .commands()
            .iter()
            .any(|command| command.contains("xkb"))
    );

    assert!(setup.applier.save_due().is_some());
    let persisted = setup.persist(&[]);
    assert!(!persisted.saved);
    assert_eq!(
        setup.generated(ConfigFile::Input),
        format!("{HEADER}input type:keyboard xkb_layout \"de\"\n")
    );
    assert_eq!(
        setup.generated(ConfigFile::Appearance),
        format!("{HEADER}client.focused #111111 #285577 #000000 #2e9ef4 #285577\n")
    );
}

/// The conflict report of `key` for a user's config with these lines and the include at the end.
fn users(key: &str, lines: &str) -> crate::conflict::Report {
    let main = "/home/me/.config/sway/config";
    let files = crate::sway_config::MemoryFiles::new()
        .var("HOME", "/home/me")
        .file(
            main,
            format!("{lines}include ~/.config/sway/sway-things/*.conf\n"),
        );
    let locations = Locations::new(
        std::path::Path::new("/home/me/.config/sway/sway-things"),
        std::path::Path::new("/home/me/.local/state/sway-things"),
    );
    let status =
        crate::include::IncludeStatus::detect(Some(std::path::Path::new(main)), &files, &locations);
    let setting = crate::registry::Registry::builtin().get(key).unwrap();
    crate::conflict::Report::check(&status, setting, None, None).unwrap()
}

impl Setup {
    fn let_config_win(&mut self, report: &crate::conflict::Report) -> Result<Applied, ApplyError> {
        block_on(self.applier.let_config_win(report, &self.access))
    }
}

#[test]
fn letting_the_config_win_sends_the_users_value_and_removes_ours() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n\n[windows.gaps]\ninner = 8\n");
    let result = setup
        .let_config_win(&users(GAPS, "gaps inner 4\n"))
        .unwrap();
    assert_eq!(
        result,
        Applied {
            changed: true,
            effect: live(&["gaps inner 4", "gaps inner all set 4"]),
        }
    );
    assert_eq!(setup.applier.state().get(GAPS, None), None);
    assert!(setup.applier.save_due().is_some());
    setup.persist(&[]);
    assert_eq!(
        fs::read_to_string(setup.settings()).unwrap(),
        "version = 1\n"
    );
    assert_eq!(setup.generated(ConfigFile::Appearance), "");
}

#[test]
fn letting_the_config_win_removes_every_value_of_the_directive() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[windows.focused]\ntext = \"#000000\"\nchild_border = \"#3b6e4f\"\n",
    );
    let report = users(
        "windows.focused.text",
        "client.focused #111111 #222222 #333333\n",
    );
    let result = setup.let_config_win(&report).unwrap();
    // sway's default indicator, and the background as child border, as sway reads the line.
    assert_eq!(
        result.effect,
        live(&["client.focused #111111 #222222 #333333 #2e9ef4 #222222"])
    );
    assert_eq!(
        setup
            .applier
            .state()
            .get("windows.focused.child_border", None),
        None
    );
    assert_eq!(
        setup.applier.state().get("windows.focused.text", None),
        None
    );
}

#[test]
fn without_a_line_of_the_user_the_default_is_sent() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n\n[windows.gaps]\ninner = 8\n");
    let result = setup.let_config_win(&users(GAPS, "")).unwrap();
    assert_eq!(
        result.effect,
        live(&["gaps inner 0", "gaps inner all set 0"])
    );
}

#[test]
fn a_value_that_cant_be_read_is_left_to_a_reload() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[input.keyboard]\nrepeat_delay = 300\n",
    );
    let report = users(
        "input.keyboard.repeat_delay",
        "input * repeat_delay 70000\n",
    );
    let result = setup.let_config_win(&report).unwrap();
    assert_eq!(
        result,
        Applied {
            changed: true,
            effect: Effect::AfterReload,
        }
    );
    assert_eq!(sway.commands(), Vec::<String>::new());
    assert_eq!(
        setup
            .applier
            .state()
            .get("input.keyboard.repeat_delay", None),
        None
    );
}

#[test]
fn letting_the_config_win_needs_a_stored_value_and_no_safety_net() {
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n");
    let result = setup
        .let_config_win(&users(GAPS, "gaps inner 4\n"))
        .unwrap();
    assert_eq!(
        result,
        Applied {
            changed: false,
            effect: Effect::NotStored,
        }
    );
    assert_eq!(sway.commands(), Vec::<String>::new());
    assert!(matches!(
        setup.let_config_win(&users(LAYOUT, "input * xkb_layout de\n")),
        Err(ApplyError::Risky { .. })
    ));
}

#[test]
fn letting_the_config_win_undoes_everything_if_sway_rejects_it() {
    let sway = FakeSway::with_version(SWAY, |kind, _| {
        message(kind, r#"[{"success": false, "error": "No"}]"#)
    });
    let mut setup = Setup::live(&sway, "version = 1\n\n[windows.gaps]\ninner = 8\n");
    let result = setup.let_config_win(&users(GAPS, "gaps inner 4\n"));
    assert!(matches!(result, Err(ApplyError::Ipc(_))));
    assert_eq!(
        setup.applier.state().get(GAPS, None),
        Some(&Value::Integer(8))
    );
}

/// The include status of a user's config with these lines and the include at the end.
fn include_status(lines: &str) -> crate::include::IncludeStatus {
    let main = "/home/me/.config/sway/config";
    let files = crate::sway_config::MemoryFiles::new()
        .var("HOME", "/home/me")
        .file(
            main,
            format!("{lines}include ~/.config/sway/sway-things/*.conf\n"),
        );
    let locations = Locations::new(
        std::path::Path::new("/home/me/.config/sway/sway-things"),
        std::path::Path::new("/home/me/.local/state/sway-things"),
    );
    crate::include::IncludeStatus::detect(Some(std::path::Path::new(main)), &files, &locations)
}

const KEYBOARD_DEVICE: &str = "1:1:AT_Translated_Set_2_keyboard";
const KEYBOARD_EVENTS: &str = "input.keyboard.events";
const TOUCHPAD_EVENTS: &str = "input.touchpad.events";

/// A laptop's sway: one keyboard, one touchpad and the power button, and every command succeeds.
fn laptop() -> FakeSway {
    let inputs = format!(
        r#"[{{"identifier": "{KEYBOARD_DEVICE}", "name": "AT keyboard", "type": "keyboard",
              "libinput": {{"send_events": "enabled"}}}},
            {{"identifier": "{TOUCHPAD}", "name": "Touchpad", "type": "touchpad",
              "libinput": {{"send_events": "enabled"}}}},
            {{"identifier": "0:1:Power_Button", "name": "Power Button", "type": "keyboard",
              "libinput": {{"send_events": "enabled"}}}}]"#
    );
    FakeSway::with_version(SWAY, move |kind, _| {
        if kind == crate::ipc::wire::GET_INPUTS {
            message(kind, &inputs)
        } else {
            message(kind, r#"[{"success": true}]"#)
        }
    })
}

impl Setup {
    fn try_risky(
        &mut self,
        key: &str,
        device: Option<&str>,
        value: Value,
        users: &str,
    ) -> Result<super::Trial, ApplyError> {
        block_on(
            self.applier
                .try_risky(key, device, value, &include_status(users), &self.access),
        )
    }
}

impl Setup {
    fn try_risky_reset(
        &mut self,
        key: &str,
        device: Option<&str>,
        users: &str,
    ) -> Result<Option<super::Trial>, ApplyError> {
        block_on(
            self.applier
                .try_risky_reset(key, device, &include_status(users), &self.access),
        )
    }

    fn reset_risky_unchecked(
        &mut self,
        key: &str,
        device: Option<&str>,
        users: &str,
    ) -> Result<Applied, ApplyError> {
        block_on(self.applier.reset_risky_unchecked(
            key,
            device,
            &include_status(users),
            &self.access,
        ))
    }
}

fn layouts(layouts: &[&str]) -> Value {
    Value::List(layouts.iter().map(|layout| (*layout).to_owned()).collect())
}

#[test]
fn a_risky_change_is_tried_and_stored_only_when_kept() {
    let sway = laptop();
    let mut setup = Setup::live(&sway, "version = 1\n");
    let trial = setup.try_risky(LAYOUT, None, layouts(&["de"]), "").unwrap();
    assert_eq!(trial.commands, [r#"input type:keyboard xkb_layout "de""#]);
    assert_eq!(sway.commands(), [r#"input type:keyboard xkb_layout "de""#]);
    // Nothing is stored yet.
    assert_eq!(setup.applier.state().get(LAYOUT, None), None);
    assert_eq!(setup.applier.save_due(), None);
    let pending = setup.applier.pending().unwrap();
    assert_eq!(
        (pending.key(), pending.value()),
        (LAYOUT, &layouts(&["de"]))
    );
    // The default timeout of the safety net.
    let left = trial.countdown.remaining();
    assert!(
        left <= Duration::from_secs(15) && left > Duration::from_secs(14),
        "{left:?}"
    );

    let kept = setup.applier.keep(&setup.access).unwrap();
    assert_eq!(
        kept.effect,
        live(&[r#"input type:keyboard xkb_layout "de""#])
    );
    assert_eq!(
        setup.applier.state().get(LAYOUT, None),
        Some(&layouts(&["de"]))
    );
    assert!(setup.applier.pending().is_none());
    assert!(setup.applier.save_due().is_some());
    // Keeping sent nothing more.
    assert_eq!(sway.commands().len(), 1);
}

#[test]
fn reverting_sends_what_was_in_effect() {
    let sway = laptop();
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[app]\nsafety_net_timeout = 5\n\n[input.keyboard]\nxkb_layout = [\"us\", \"de\"]\n",
    );
    let trial = setup.try_risky(LAYOUT, None, layouts(&["fr"]), "").unwrap();
    assert!(trial.countdown.remaining() <= Duration::from_secs(5));
    let reverted = block_on(setup.applier.revert()).unwrap();
    assert_eq!(
        reverted,
        live(&[r#"input type:keyboard xkb_layout "us,de""#])
    );
    assert_eq!(
        setup.applier.state().get(LAYOUT, None),
        Some(&layouts(&["us", "de"]))
    );
    assert!(setup.applier.pending().is_none());

    // Without a stored value, what the user's config sets comes back.
    let sway = laptop();
    let mut setup = Setup::live(&sway, "version = 1\n");
    setup
        .try_risky(
            LAYOUT,
            None,
            layouts(&["fr"]),
            "input type:keyboard xkb_layout de\n",
        )
        .unwrap();
    let reverted = block_on(setup.applier.revert()).unwrap();
    assert_eq!(reverted, live(&[r#"input type:keyboard xkb_layout "de""#]));
    assert_eq!(setup.applier.save_due(), None);
}

#[test]
fn one_risky_change_waits_at_a_time() {
    let sway = laptop();
    let mut setup = Setup::live(&sway, "version = 1\n");
    setup.try_risky(LAYOUT, None, layouts(&["de"]), "").unwrap();
    assert!(matches!(
        setup.try_risky(
            TOUCHPAD_EVENTS,
            None,
            Value::Choice("enabled".to_owned()),
            ""
        ),
        Err(ApplyError::PendingChange { key: LAYOUT })
    ));
    // A reload would put the stored layout back under the safety net's feet.
    assert!(matches!(
        block_on(setup.applier.reload_sway(&setup.access)),
        Err(ApplyError::PendingChange { key: LAYOUT })
    ));
    block_on(setup.applier.revert()).unwrap();
    assert!(matches!(
        setup.applier.keep(&setup.access),
        Err(ApplyError::NothingPending)
    ));
    assert!(matches!(
        block_on(setup.applier.revert()),
        Err(ApplyError::NothingPending)
    ));
}

#[test]
fn only_risky_settings_go_through_the_safety_net_and_only_in_a_session() {
    let sway = laptop();
    let mut setup = Setup::live(&sway, "version = 1\n");
    assert!(matches!(
        setup.try_risky(GAPS, None, Value::Integer(4), ""),
        Err(ApplyError::NotRisky { .. })
    ));
    assert!(matches!(
        setup.set(KEYBOARD_EVENTS, Value::Bool(false)),
        Err(ApplyError::Risky { .. })
    ));
    let mut offline = Setup::offline("version = 1\n");
    let error = offline
        .try_risky(LAYOUT, None, layouts(&["de"]), "")
        .unwrap_err();
    assert!(matches!(error, ApplyError::NoSession { key: LAYOUT }));
    assert_eq!(
        error.to_string(),
        "input.keyboard.xkb_layout can lock you out, so it is only tried in a running sway \
         session, where it can be reverted"
    );
}

#[test]
fn switching_off_the_last_keyboard_or_touchpad_is_refused() {
    let sway = laptop();
    let mut setup = Setup::live(&sway, "version = 1\n");
    let error = setup
        .try_risky(KEYBOARD_EVENTS, None, Value::Bool(false), "")
        .unwrap_err();
    assert!(matches!(
        error,
        ApplyError::Lockout {
            lockout: Lockout::Keyboard,
            ..
        }
    ));
    assert_eq!(
        error.to_string(),
        "input.keyboard.events wasn't changed: this would turn off the last keyboard you can type with"
    );
    assert!(matches!(
        setup.try_risky(
            KEYBOARD_EVENTS,
            Some(KEYBOARD_DEVICE),
            Value::Bool(false),
            ""
        ),
        Err(ApplyError::Lockout { .. })
    ));
    assert!(matches!(
        setup.try_risky(
            TOUCHPAD_EVENTS,
            None,
            Value::Choice("disabled".to_owned()),
            ""
        ),
        Err(ApplyError::Lockout {
            lockout: Lockout::PointingDevice,
            ..
        })
    ));
    assert_eq!(sway.commands(), Vec::<String>::new());
    // Off while a mouse is connected leaves the touchpad usable without one.
    let trial = setup
        .try_risky(
            TOUCHPAD_EVENTS,
            Some(TOUCHPAD),
            Value::Choice("disabled_on_external_mouse".to_owned()),
            "",
        )
        .unwrap();
    assert_eq!(
        trial.commands,
        [format!(
            r#"input "{TOUCHPAD}" events disabled_on_external_mouse"#
        )]
    );
}

#[test]
fn an_unchecked_risky_change_is_stored_but_still_protected() {
    // In a session: sent and stored at once.
    let sway = laptop();
    let mut setup = Setup::live(&sway, "version = 1\n");
    let applied = block_on(setup.applier.set_risky_unchecked(
        LAYOUT,
        None,
        layouts(&["de"]),
        &setup.access,
    ))
    .unwrap();
    assert_eq!(
        applied.effect,
        live(&[r#"input type:keyboard xkb_layout "de""#])
    );
    assert_eq!(
        setup.applier.state().get(LAYOUT, None),
        Some(&layouts(&["de"]))
    );
    assert!(matches!(
        block_on(setup.applier.set_risky_unchecked(
            KEYBOARD_EVENTS,
            None,
            Value::Bool(false),
            &setup.access
        )),
        Err(ApplyError::Lockout { .. })
    ));

    // Without a session the devices aren't known: a whole type can't be switched off, one
    // device can.
    let mut offline = Setup::offline("version = 1\n");
    assert!(matches!(
        block_on(offline.applier.set_risky_unchecked(
            KEYBOARD_EVENTS,
            None,
            Value::Bool(false),
            &offline.access
        )),
        Err(ApplyError::Lockout {
            lockout: Lockout::Keyboard,
            ..
        })
    ));
    let applied = block_on(offline.applier.set_risky_unchecked(
        KEYBOARD_EVENTS,
        Some(KEYBOARD_DEVICE),
        Value::Bool(false),
        &offline.access,
    ))
    .unwrap();
    assert_eq!(applied.effect, Effect::NoSession);
    assert_eq!(
        offline
            .applier
            .state()
            .get(KEYBOARD_EVENTS, Some(KEYBOARD_DEVICE)),
        Some(&Value::Bool(false))
    );
}

#[test]
fn a_risky_reset_is_tried_and_removes_the_value_only_when_kept() {
    let sway = laptop();
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[input.keyboard]\nxkb_layout = [\"de\"]\n",
    );
    let trial = setup.try_risky_reset(LAYOUT, None, "").unwrap().unwrap();
    assert_eq!(trial.commands, [r#"input type:keyboard xkb_layout "us""#]);
    // Nothing is removed yet.
    assert_eq!(
        setup.applier.state().get(LAYOUT, None),
        Some(&layouts(&["de"]))
    );
    assert_eq!(setup.applier.save_due(), None);
    let pending = setup.applier.pending().unwrap();
    assert!(pending.is_reset());
    assert_eq!(pending.value(), &layouts(&["us"]));

    let kept = setup.applier.keep(&setup.access).unwrap();
    assert!(kept.changed);
    assert_eq!(
        kept.effect,
        live(&[r#"input type:keyboard xkb_layout "us""#])
    );
    assert_eq!(setup.applier.state().get(LAYOUT, None), None);
    assert!(setup.applier.save_due().is_some());
    assert_eq!(sway.commands().len(), 1);

    // Nothing stored, nothing to try.
    assert!(setup.try_risky_reset(LAYOUT, None, "").unwrap().is_none());
    assert!(setup.applier.pending().is_none());
    assert_eq!(sway.commands().len(), 1);
}

#[test]
fn a_risky_reset_tries_what_the_users_config_sets_and_reverts_to_the_stored_value() {
    let sway = laptop();
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[input.keyboard]\nxkb_layout = [\"us\"]\n",
    );
    let trial = setup
        .try_risky_reset(LAYOUT, None, "input type:keyboard xkb_layout de\n")
        .unwrap()
        .unwrap();
    assert_eq!(trial.commands, [r#"input type:keyboard xkb_layout "de""#]);
    assert_eq!(
        setup.applier.pending().map(super::Pending::value),
        Some(&layouts(&["de"]))
    );
    let reverted = block_on(setup.applier.revert()).unwrap();
    assert_eq!(reverted, live(&[r#"input type:keyboard xkb_layout "us""#]));
    assert_eq!(
        setup.applier.state().get(LAYOUT, None),
        Some(&layouts(&["us"]))
    );
    assert!(setup.applier.pending().is_none());
}

#[test]
fn a_risky_reset_of_one_device_tries_the_value_of_its_type() {
    let sway = laptop();
    let mut setup = Setup::live(
        &sway,
        &format!(
            "version = 1\n\n[input.keyboard]\nxkb_layout = [\"fr\"]\n\n\
             [device.\"{KEYBOARD_DEVICE}\".input.keyboard]\nxkb_layout = [\"de\"]\n"
        ),
    );
    let trial = setup
        .try_risky_reset(LAYOUT, Some(KEYBOARD_DEVICE), "")
        .unwrap()
        .unwrap();
    assert_eq!(
        trial.commands,
        [format!(r#"input "{KEYBOARD_DEVICE}" xkb_layout "fr""#)]
    );
    setup.applier.keep(&setup.access).unwrap();
    assert_eq!(
        setup.applier.state().get(LAYOUT, Some(KEYBOARD_DEVICE)),
        None
    );
    assert_eq!(
        setup.applier.state().get(LAYOUT, None),
        Some(&layouts(&["fr"]))
    );
}

#[test]
fn a_risky_reset_is_refused_like_a_risky_change() {
    // The user's config switches the keyboards off, so removing the stored value would too.
    let sway = laptop();
    let stored = "version = 1\n\n[input.keyboard]\nevents = true\n";
    let users = "input type:keyboard events disabled\n";
    let mut setup = Setup::live(&sway, stored);
    assert!(matches!(
        setup.try_risky_reset(KEYBOARD_EVENTS, None, users),
        Err(ApplyError::Lockout {
            lockout: Lockout::Keyboard,
            ..
        })
    ));
    assert!(matches!(
        setup.reset_risky_unchecked(KEYBOARD_EVENTS, None, users),
        Err(ApplyError::Lockout { .. })
    ));
    assert_eq!(sway.commands(), Vec::<String>::new());
    assert_eq!(
        setup.applier.state().get(KEYBOARD_EVENTS, None),
        Some(&Value::Bool(true))
    );

    // One change at a time, only risky settings, only in a session.
    setup.try_risky(LAYOUT, None, layouts(&["de"]), "").unwrap();
    assert!(matches!(
        setup.try_risky_reset(KEYBOARD_EVENTS, None, ""),
        Err(ApplyError::PendingChange { key: LAYOUT })
    ));
    assert!(matches!(
        setup.reset_risky_unchecked(KEYBOARD_EVENTS, None, ""),
        Err(ApplyError::PendingChange { key: LAYOUT })
    ));
    block_on(setup.applier.revert()).unwrap();
    assert!(matches!(
        setup.try_risky_reset(GAPS, None, ""),
        Err(ApplyError::NotRisky { .. })
    ));
    let mut offline = Setup::offline(stored);
    assert!(matches!(
        offline.try_risky_reset(KEYBOARD_EVENTS, None, ""),
        Err(ApplyError::NoSession { .. })
    ));
}

#[test]
fn an_unchecked_risky_reset_removes_the_value_but_is_still_protected() {
    let sway = laptop();
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[input.keyboard]\nxkb_layout = [\"de\"]\n",
    );
    let applied = setup
        .reset_risky_unchecked(LAYOUT, None, "input type:keyboard xkb_layout fr\n")
        .unwrap();
    assert!(applied.changed);
    assert_eq!(
        applied.effect,
        live(&[r#"input type:keyboard xkb_layout "fr""#])
    );
    assert_eq!(setup.applier.state().get(LAYOUT, None), None);
    assert!(setup.applier.save_due().is_some());
    let applied = setup.reset_risky_unchecked(LAYOUT, None, "").unwrap();
    assert_eq!(
        applied,
        Applied {
            changed: false,
            effect: Effect::NotStored
        }
    );

    // Without a session the devices aren't known: what would switch off a whole type is
    // refused.
    let mut offline = Setup::offline("version = 1\n\n[input.keyboard]\nevents = true\n");
    assert!(matches!(
        offline.reset_risky_unchecked(
            KEYBOARD_EVENTS,
            None,
            "input type:keyboard events disabled\n"
        ),
        Err(ApplyError::Lockout {
            lockout: Lockout::Keyboard,
            ..
        })
    ));
    let applied = offline
        .reset_risky_unchecked(KEYBOARD_EVENTS, None, "")
        .unwrap();
    assert_eq!(applied.effect, Effect::NoSession);
    assert_eq!(offline.applier.state().get(KEYBOARD_EVENTS, None), None);
}

const VARIANT: &str = "input.keyboard.xkb_variant";

impl Setup {
    fn try_risky_all(
        &mut self,
        changes: &[(&str, Option<Value>)],
        users: &str,
    ) -> Result<Option<super::Trial>, ApplyError> {
        block_on(
            self.applier
                .try_risky_all(None, changes, &include_status(users), &self.access),
        )
    }
}

#[test]
fn layouts_and_variants_are_tried_kept_and_reverted_as_one_change() {
    let sway = laptop();
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[input.keyboard]\nxkb_layout = [\"us\", \"de\"]\nxkb_variant = [\"dvorak\", \"nodeadkeys\"]\n",
    );
    // Swapped: the variants are cleared first, so no command pairs a layout with a variant
    // of the other one.
    let trial = setup
        .try_risky_all(
            &[
                (LAYOUT, Some(layouts(&["de", "us"]))),
                (VARIANT, Some(layouts(&["nodeadkeys", "dvorak"]))),
            ],
            "",
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        trial.commands,
        [
            r#"input type:keyboard xkb_variant ",""#,
            r#"input type:keyboard xkb_layout "de,us""#,
            r#"input type:keyboard xkb_variant "nodeadkeys,dvorak""#,
        ]
    );
    let pending = setup.applier.pending().unwrap();
    let keys: Vec<&str> = pending.values().map(|value| value.key).collect();
    assert_eq!((pending.key(), keys), (LAYOUT, vec![LAYOUT, VARIANT]));
    // Another risky change waits for this one.
    assert!(matches!(
        setup.try_risky(LAYOUT, None, layouts(&["fr"]), ""),
        Err(ApplyError::PendingChange { key: LAYOUT })
    ));

    let reverted = block_on(setup.applier.revert()).unwrap();
    assert_eq!(
        reverted,
        live(&[
            r#"input type:keyboard xkb_variant ",""#,
            r#"input type:keyboard xkb_layout "us,de""#,
            r#"input type:keyboard xkb_variant "dvorak,nodeadkeys""#,
        ])
    );

    // A layout removed with its variant, then kept: both are stored.
    let trial = setup
        .try_risky_all(
            &[
                (LAYOUT, Some(layouts(&["de"]))),
                (VARIANT, Some(layouts(&["nodeadkeys"]))),
            ],
            "",
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        trial.commands,
        [
            r#"input type:keyboard xkb_variant ",""#,
            r#"input type:keyboard xkb_layout "de""#,
            r#"input type:keyboard xkb_variant "nodeadkeys""#,
        ]
    );
    setup.applier.keep(&setup.access).unwrap();
    assert_eq!(
        setup.applier.state().get(LAYOUT, None),
        Some(&layouts(&["de"]))
    );
    assert_eq!(
        setup.applier.state().get(VARIANT, None),
        Some(&layouts(&["nodeadkeys"]))
    );
}

#[test]
fn layouts_and_variants_are_reset_together() {
    let sway = laptop();
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[input.keyboard]\nxkb_layout = [\"us\", \"de\"]\nxkb_variant = [\"\", \"nodeadkeys\"]\n",
    );
    let trial = setup
        .try_risky_all(
            &[(LAYOUT, None), (VARIANT, None)],
            "input type:keyboard xkb_layout fr\n",
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        trial.commands,
        [
            r#"input type:keyboard xkb_variant ",""#,
            r#"input type:keyboard xkb_layout "fr""#,
            r#"input type:keyboard xkb_variant """#,
        ]
    );
    let pending = setup.applier.pending().unwrap();
    assert!(pending.values().all(|value| value.reset));
    assert_eq!(pending.value(), &layouts(&["fr"]));
    setup.applier.keep(&setup.access).unwrap();
    assert_eq!(setup.applier.state().get(LAYOUT, None), None);
    assert_eq!(setup.applier.state().get(VARIANT, None), None);

    // Nothing stored, nothing to try.
    assert!(
        setup
            .try_risky_all(&[(LAYOUT, None), (VARIANT, None)], "")
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_change_of_several_settings_is_refused_like_one() {
    let sway = laptop();
    let mut setup = Setup::live(&sway, "version = 1\n");
    assert!(matches!(
        setup.try_risky_all(
            &[
                (LAYOUT, Some(layouts(&["de"]))),
                (GAPS, Some(Value::Integer(4)))
            ],
            ""
        ),
        Err(ApplyError::NotRisky { key: GAPS })
    ));
    let mut offline = Setup::offline("version = 1\n");
    assert!(matches!(
        offline.try_risky_all(&[(LAYOUT, Some(layouts(&["de"])))], ""),
        Err(ApplyError::NoSession { key: LAYOUT })
    ));
    assert_eq!(sway.commands(), [] as [&str; 0]);
}

const WORKSPACE_LAYOUT: &str = "behavior.workspace_layout";
const XWAYLAND: &str = "behavior.xwayland";

fn choice(id: &str) -> Value {
    Value::Choice(id.to_owned())
}

fn waiting(applier: &Applier<FakeValidator>, until: super::Until) -> Vec<&'static str> {
    applier
        .waiting()
        .until(until)
        .map(|change| change.key)
        .collect()
}

#[test]
fn a_change_for_the_config_only_waits_for_a_reload_of_sway() {
    use super::Until;
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n");
    setup.set(GAPS, Value::Integer(8)).unwrap();
    assert!(!setup.applier.waiting().any(Until::Reload));

    setup.set(WORKSPACE_LAYOUT, choice("tabbed")).unwrap();
    setup.set(WORKSPACE_LAYOUT, choice("stacking")).unwrap();
    assert_eq!(waiting(&setup.applier, Until::Reload), [WORKSPACE_LAYOUT]);
    assert_eq!(
        setup.applier.waiting().get(WORKSPACE_LAYOUT, None),
        Some(Until::Reload)
    );
    // Back to what sway runs with: nothing waits.
    setup.reset(WORKSPACE_LAYOUT).unwrap();
    assert!(!setup.applier.waiting().any(Until::Reload));

    // The reload writes the files first, then asks sway.
    setup.set(WORKSPACE_LAYOUT, choice("tabbed")).unwrap();
    block_on(setup.applier.reload_sway(&setup.access)).unwrap();
    assert_eq!(sway.commands().last().map(String::as_str), Some("reload"));
    assert_eq!(
        setup.generated(ConfigFile::Behavior),
        format!("{HEADER}workspace_layout tabbed\n")
    );
    assert!(!setup.applier.waiting().any(Until::Reload));

    // Back to the default is a change too now, since sway runs with tabbed.
    setup.reset(WORKSPACE_LAYOUT).unwrap();
    assert_eq!(waiting(&setup.applier, Until::Reload), [WORKSPACE_LAYOUT]);
}

#[test]
fn a_change_for_the_next_login_outlasts_a_reload() {
    use super::Until;
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n");
    let applied = setup.set(XWAYLAND, choice("disable")).unwrap();
    assert_eq!(applied.effect, Effect::AfterRelogin);
    assert_eq!(waiting(&setup.applier, Until::Relogin), [XWAYLAND]);
    assert!(!setup.applier.waiting().any(Until::Reload));
    block_on(setup.applier.reload_sway(&setup.access)).unwrap();
    assert_eq!(waiting(&setup.applier, Until::Relogin), [XWAYLAND]);
}

#[test]
fn a_reload_from_elsewhere_only_counts_for_written_changes() {
    use super::Until;
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n");
    setup.set(WORKSPACE_LAYOUT, choice("tabbed")).unwrap();
    // Not saved yet: sway read the old file.
    setup.applier.reloaded();
    assert_eq!(waiting(&setup.applier, Until::Reload), [WORKSPACE_LAYOUT]);
    setup.persist(&[]);
    setup.applier.reloaded();
    assert!(!setup.applier.waiting().any(Until::Reload));
}

#[test]
fn a_reload_of_sway_is_refused_when_it_couldnt_apply_the_changes() {
    let mut setup = Setup::offline("version = 1\n");
    setup.set(WORKSPACE_LAYOUT, choice("tabbed")).unwrap();
    // Nothing waits: sway reads the files when it starts.
    assert!(!setup.applier.waiting().any(super::Until::Reload));
    assert!(matches!(
        block_on(setup.applier.reload_sway(&setup.access)),
        Err(ApplyError::ReloadWithoutSession)
    ));

    // A generated file that was changed outside isn't replaced, so sway wouldn't read the change.
    let sway = obedient(SWAY);
    let mut setup = Setup::live(
        &sway,
        "version = 1\n\n[behavior]\nworkspace_layout = \"stacking\"\n",
    );
    setup.persist(&[]);
    let behavior = setup.applier.locations().path(ConfigFile::Behavior);
    fs::write(&behavior, format!("{HEADER}workspace_layout default\n")).unwrap();
    setup.set(WORKSPACE_LAYOUT, choice("tabbed")).unwrap();
    match block_on(setup.applier.reload_sway(&setup.access)) {
        Err(ApplyError::Unwritten { files }) => assert_eq!(files, [ConfigFile::Behavior]),
        other => panic!("{other:?}"),
    }
    assert!(!sway.commands().iter().any(|command| command == "reload"));
}

#[test]
fn changes_on_disk_for_the_config_only_wait_for_a_reload_of_sway() {
    use super::Until;
    let sway = obedient(SWAY);
    let mut setup = Setup::live(&sway, "version = 1\n");
    setup.persist(&[]);
    fs::write(
        setup.settings(),
        "version = 1\n\n[behavior]\nworkspace_layout = \"tabbed\"\n\n[input.keyboard]\n\
         xkb_layout = [\"de\"]\n",
    )
    .unwrap();
    block_on(setup.applier.reload(&setup.access));
    // The risky layout is left to the safety net.
    assert_eq!(waiting(&setup.applier, Until::Reload), [WORKSPACE_LAYOUT]);
}
