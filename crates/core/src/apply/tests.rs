// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use async_io::block_on;

use super::{Applied, Applier, ApplyError, Effect, SAVE_DELAY};
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
