// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use async_io::block_on;

use super::{Change, OutputWatch, Outputs, Rect};
use crate::command::output::{Mode, Transform};
use crate::devices::Lockout;
use crate::ipc::tests::with_outputs;
use crate::ipc::types;
use crate::registry::{Registry, Value};
use crate::state::{Paths, State};

const LAPTOP: &str = "InfoVision Optoelectronics (Kunshan) Co.,Ltd China 0x057D Unknown";
const LEFT: &str = "Samsung Electric Company LF27T850 H4LR303241";
const RIGHT: &str = "Samsung Electric Company LF27T850 H4LR100370";

/// A laptop panel at scale 1.3 right of two monitors, and a switched-off projector, as sway 1.12
/// reports them (shortened to the fields that are read, plus some that aren't).
const DESK: &str = r#"[
  {"id": 3, "type": "output", "name": "eDP-1", "active": true, "dpms": true, "power": true,
   "primary": false, "make": "InfoVision Optoelectronics (Kunshan) Co.,Ltd", "model": "China",
   "serial": "0x057D Unknown", "scale": 1.2999999523162842, "transform": "normal",
   "focused": false, "current_workspace": "3", "subpixel_hinting": "unknown",
   "modes": [{"width": 1920, "height": 1080, "refresh": 60010}],
   "current_mode": {"width": 1920, "height": 1080, "refresh": 60010},
   "rect": {"x": 5120, "y": 304, "width": 1476, "height": 830}},
  {"id": 4, "type": "output", "name": "DP-5", "active": true, "dpms": true, "power": true,
   "primary": false, "make": "Samsung Electric Company", "model": "LF27T850",
   "serial": "H4LR100370", "scale": 1.0, "transform": "normal", "focused": true,
   "modes": [{"width": 2560, "height": 1440, "refresh": 59951},
             {"width": 1920, "height": 1080, "refresh": 60000},
             {"width": 1920, "height": 1080, "refresh": 60000}],
   "current_mode": {"width": 2560, "height": 1440, "refresh": 59951},
   "rect": {"x": 2560, "y": 0, "width": 2560, "height": 1440}},
  {"id": 5, "type": "output", "name": "DP-4", "active": true, "dpms": true, "power": true,
   "primary": false, "make": "Samsung Electric Company", "model": "LF27T850",
   "serial": "H4LR303241", "scale": 1.0, "transform": "90", "focused": false,
   "modes": [{"width": 2560, "height": 1440, "refresh": 59951}],
   "current_mode": {"width": 2560, "height": 1440, "refresh": 59951},
   "rect": {"x": 0, "y": 0, "width": 1440, "height": 2560}},
  {"type": "output", "name": "HDMI-A-1", "active": false, "dpms": false, "power": false,
   "primary": false, "make": "Unknown", "model": "Unknown", "serial": "Unknown",
   "non_desktop": false, "current_workspace": null,
   "modes": [{"width": 1280, "height": 720, "refresh": 60000}],
   "rect": {"x": 0, "y": 0, "width": 0, "height": 0}},
  {"type": "output", "name": "DP-9", "primary": false, "make": "Valve", "model": "Index",
   "serial": "X", "non_desktop": true, "modes": []}
]"#;

fn outputs(json: &str) -> Vec<types::Output> {
    serde_json::from_str(json).unwrap()
}

fn desk() -> Outputs {
    Outputs::new(&outputs(DESK))
}

#[test]
fn outputs_are_read_from_sway() {
    let desk = desk();
    let names: Vec<&str> = desk.iter().map(super::Output::name).collect();
    // The VR headset isn't a desktop.
    assert_eq!(names, ["eDP-1", "DP-5", "DP-4", "HDMI-A-1"]);

    let laptop = desk.get(LAPTOP).unwrap();
    assert_eq!(laptop.name(), "eDP-1");
    assert_eq!(
        laptop.label(),
        "InfoVision Optoelectronics (Kunshan) Co.,Ltd China"
    );
    assert!(laptop.is_connected() && laptop.is_enabled() && !laptop.is_focused());
    assert_eq!(
        laptop.mode(),
        Some(Mode::new(1920, 1080, Some(60_010)).unwrap())
    );
    assert_eq!(
        laptop.rect(),
        Some(Rect {
            x: 5120,
            y: 304,
            width: 1476,
            height: 830
        })
    );
    assert_eq!(laptop.transform(), Some(Transform::Normal));

    let right = desk.get(RIGHT).unwrap();
    assert!(right.is_focused());
    // A mode that sway lists twice is offered once.
    assert_eq!(right.modes().len(), 2);
    assert_eq!(desk.get(LEFT).unwrap().transform(), Some(Transform::R90));

    // Without make, model and serial an output is stored by its name, and shown by it.
    let projector = desk.get("HDMI-A-1").unwrap();
    assert_eq!(projector.identifier(), "HDMI-A-1");
    assert_eq!(projector.label(), "HDMI-A-1");
    assert!(projector.is_connected() && !projector.is_enabled());
    assert_eq!(
        (projector.mode(), projector.rect(), projector.scale()),
        (None, None, None)
    );
    assert_eq!(projector.modes().len(), 1);

    let enabled: Vec<&str> = desk.enabled().map(super::Output::name).collect();
    assert_eq!(enabled, ["eDP-1", "DP-5", "DP-4"]);
}

#[test]
fn outputs_are_found_by_name_or_identifier() {
    let desk = desk();
    assert_eq!(desk.find("DP-4").unwrap().identifier(), LEFT);
    assert_eq!(desk.find(LEFT).unwrap().name(), "DP-4");
    assert!(desk.find("DP-1").is_none());
    // The identifier is case-sensitive, as in sway.
    assert!(desk.find(&LEFT.to_lowercase()).is_none());
}

#[test]
fn twins_without_serial_are_stored_by_name() {
    let twins = r#"[
      {"name": "DP-1", "active": true, "primary": false, "make": "Acme", "model": "One",
       "serial": "Unknown", "modes": []},
      {"name": "DP-2", "active": true, "primary": false, "make": "Acme", "model": "One",
       "serial": "Unknown", "modes": []},
      {"name": "DP-3", "active": true, "primary": false, "make": "Acme", "model": "Two",
       "serial": "Unknown", "modes": []}
    ]"#;
    let list = Outputs::new(&outputs(twins));
    let identifiers: Vec<&str> = list.iter().map(super::Output::identifier).collect();
    assert_eq!(identifiers, ["DP-1", "DP-2", "DP-3"]);
}

#[test]
fn an_identifier_too_long_for_sway_is_the_name() {
    let long = format!(
        r#"[{{"name": "DP-1", "active": true, "primary": false, "make": "{}", "model": "M",
            "serial": "S", "modes": []}}]"#,
        "A".repeat(124)
    );
    let list = Outputs::new(&outputs(&long));
    assert_eq!(list.iter().next().unwrap().identifier(), "DP-1");
}

#[test]
fn session_values_are_what_sway_runs_with() {
    let desk = desk();
    let laptop = desk.get(LAPTOP).unwrap();
    assert_eq!(
        laptop.session_value("output.mode"),
        Some(Value::Text("1920x1080@60.01Hz".to_owned()))
    );
    assert_eq!(
        laptop.session_value("output.position"),
        Some(Value::Text("5120,304".to_owned()))
    );
    assert_eq!(
        laptop.session_value("output.scale"),
        Some(Value::Decimal(1.299_999_952_316_284_2))
    );
    assert_eq!(
        desk.get(LEFT).unwrap().session_value("output.transform"),
        Some(Value::Choice("90".to_owned()))
    );
    assert_eq!(
        laptop.session_value("output.enabled"),
        Some(Value::Bool(true))
    );
    assert_eq!(laptop.session_value("input.touchpad.tap"), None);

    let projector = desk.get("HDMI-A-1").unwrap();
    assert_eq!(
        projector.session_value("output.enabled"),
        Some(Value::Bool(false))
    );
    assert_eq!(projector.session_value("output.mode"), None);
}

#[test]
fn every_session_value_fits_its_setting() {
    let registry = Registry::builtin();
    let desk = desk();
    for output in desk.iter() {
        for key in [
            super::MODE,
            super::POSITION,
            super::SCALE,
            super::TRANSFORM,
            super::ENABLED,
        ] {
            let setting = registry.get(key).unwrap();
            if let Some(value) = output.session_value(key) {
                setting.check(&value).unwrap();
            }
        }
    }
}

#[test]
fn the_last_output_that_is_on_cant_be_switched_off() {
    let desk = desk();
    assert_eq!(desk.lockout(None, false), Some(Lockout::Display));
    assert_eq!(desk.lockout(None, true), None);
    assert_eq!(desk.lockout(Some(LAPTOP), false), None);
    assert!(!desk.is_last_enabled(LAPTOP));

    let single = r#"[
      {"name": "eDP-1", "active": true, "primary": false, "make": "A", "model": "B",
       "serial": "C", "modes": []},
      {"name": "HDMI-A-1", "active": false, "primary": false, "make": "D", "model": "E",
       "serial": "F", "modes": []}
    ]"#;
    let single = Outputs::new(&outputs(single));
    assert!(single.is_last_enabled("A B C"));
    for name in ["A B C", "eDP-1"] {
        assert_eq!(single.lockout(Some(name), false), Some(Lockout::Display));
    }
    assert_eq!(single.lockout(Some("D E F"), false), None);
    assert_eq!(single.lockout(Some("A B C"), true), None);

    // A session without outputs has nothing to lose.
    assert_eq!(Outputs::new(&[]).lockout(None, false), None);

    assert_eq!(
        Outputs::lockout_unknown(None, false),
        Some(Lockout::Display)
    );
    assert_eq!(Outputs::lockout_unknown(Some("eDP-1"), false), None);
    assert_eq!(Outputs::lockout_unknown(None, true), None);
}

#[test]
fn updates_report_what_changed() {
    let mut list = desk();
    let without_projector = DESK.replace(r#""name": "HDMI-A-1""#, r#""name": "gone""#);
    let mut fewer = outputs(&without_projector);
    fewer.retain(|output| output.name != "gone");
    for output in &mut fewer {
        if output.name == "DP-5" {
            output.focused = false;
        }
    }
    let changes = list.update(&fewer);
    assert_eq!(
        changes,
        [
            Change::Removed("HDMI-A-1".to_owned()),
            Change::Changed(RIGHT.to_owned())
        ]
    );
    assert_eq!(list.update(&fewer), [] as [crate::outputs::Change; 0]);
    assert_eq!(
        list.update(&outputs(DESK)),
        [
            Change::Changed(RIGHT.to_owned()),
            Change::Added("HDMI-A-1".to_owned())
        ]
    );
}

#[test]
fn outputs_with_stored_values_stay_listed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("settings.toml"),
        "[display.\"Dell Inc. DELL U2723QE ABC\".output]\nscale = 1.5\n",
    )
    .unwrap();
    let state = State::load(
        Registry::builtin(),
        Paths::new(dir.path(), dir.path(), "host").unwrap(),
    );
    let mut list = desk();
    assert!(list.set_stored(&state));
    assert!(!list.set_stored(&state));
    let dell = list.get("Dell Inc. DELL U2723QE ABC").unwrap();
    assert!(!dell.is_connected() && !dell.is_enabled());
    assert_eq!(dell.label(), "Dell Inc. DELL U2723QE ABC");
    assert_eq!(dell.session_value("output.enabled"), None);
    assert_eq!(list.iter().count(), 5);
}

#[test]
fn the_watch_reads_the_outputs_again_after_each_event() {
    const ONE: &str = r#"[{"name": "eDP-1", "active": true, "primary": false, "make": "A",
        "model": "B", "serial": "C", "modes": []}]"#;
    const TWO: &str = r#"[{"name": "eDP-1", "active": true, "primary": false, "make": "A",
        "model": "B", "serial": "C", "modes": []},
        {"name": "DP-1", "active": true, "primary": false, "make": "D", "model": "E",
        "serial": "F", "modes": []}]"#;
    let sway = with_outputs(&[ONE, ONE, TWO]);
    block_on(async {
        let client = sway.connect();
        let mut watch = OutputWatch::start(&client).await.unwrap();
        assert_eq!(watch.outputs().iter().count(), 1);
        // The first event changes nothing and is skipped.
        assert_eq!(
            watch.changed().await.unwrap().unwrap(),
            Change::Added("D E F".to_owned())
        );
    });
}

#[test]
fn an_output_is_switched_on_when_none_is() {
    const ALL_OFF: &str = r#"[
      {"name": "eDP-1", "active": false, "primary": false, "make": "A", "model": "B",
       "serial": "C", "modes": []},
      {"name": "HDMI-A-1", "active": false, "primary": false, "make": "D", "model": "E",
       "serial": "F", "modes": []}
    ]"#;
    let sway = with_outputs(&[ALL_OFF]);
    let switched = block_on(super::ensure_one_on(&mut sway.connect())).unwrap();
    assert_eq!(switched.as_deref(), Some("eDP-1"));
    assert_eq!(sway.commands(), [r#"output "eDP-1" enable"#]);

    // One is on: nothing to do.
    let sway = with_outputs(&[DESK]);
    let switched = block_on(super::ensure_one_on(&mut sway.connect())).unwrap();
    assert_eq!(switched, None);
    assert_eq!(sway.commands(), [] as [&str; 0]);

    // Nothing connected at first: the output that comes with the next event is switched on.
    let sway = with_outputs(&["[]", ALL_OFF]);
    let switched = block_on(super::ensure_one_on(&mut sway.connect())).unwrap();
    assert_eq!(switched.as_deref(), Some("eDP-1"));
}

#[test]
fn the_panel_stays_off_when_the_docks_monitor_comes_a_moment_later() {
    // At a login at the dock: first only the panel (off), then the dock's monitor (on).
    const PANEL: &str = r#"[{"name": "eDP-1", "active": false, "primary": false, "make": "A",
        "model": "B", "serial": "C", "modes": []}]"#;
    const DOCKED: &str = r#"[{"name": "eDP-1", "active": false, "primary": false, "make": "A",
        "model": "B", "serial": "C", "modes": []},
        {"name": "DP-4", "active": true, "primary": false, "make": "D", "model": "E",
        "serial": "F", "modes": []}]"#;
    let sway = with_outputs(&[PANEL, DOCKED]);
    let switched = block_on(super::ensure_one_on(&mut sway.connect())).unwrap();
    assert_eq!(switched, None);
    assert_eq!(sway.commands(), [] as [&str; 0]);
}

#[test]
fn the_guard_runs_the_hook_at_every_start() {
    assert_eq!(
        super::guard_command()
            .render(crate::command::Target::Config)
            .unwrap(),
        "exec_always sway-things hook displays"
    );
}
