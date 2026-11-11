// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use super::{Order, Reach, Report, UserLine, check_all};
use crate::command::Color;
use crate::command::input::InputType;
use crate::devices::Devices;
use crate::generate::Locations;
use crate::include::IncludeStatus;
use crate::ipc::types::Input;
use crate::registry::{ReadError, Registry, Scope, Value};
use crate::sway_config::MemoryFiles;

const MAIN: &str = "/home/me/.config/sway/config";
const INCLUDE: &str = "include ~/.config/sway/sway-things/*.conf";
const TOUCHPAD: &str = "1267:12377:ELAN1300:00_04F3:3059_Touchpad";
const KEYBOARD: &str = "1:1:AT_Translated_Set_2_keyboard";

fn status_of(files: &MemoryFiles) -> IncludeStatus {
    let locations = Locations::new(
        Path::new("/home/me/.config/sway/sway-things"),
        Path::new("/home/me/.local/state/sway-things"),
    );
    IncludeStatus::detect(Some(Path::new(MAIN)), files, &locations)
}

fn status(main: &str) -> IncludeStatus {
    status_of(&MemoryFiles::new().var("HOME", "/home/me").file(MAIN, main))
}

fn report(status: &IncludeStatus, key: &str, device: Option<&str>) -> Report {
    let setting = Registry::builtin().get(key).unwrap();
    Report::check(status, setting, device, None).unwrap()
}

fn numbers<'a>(lines: impl IntoIterator<Item = &'a UserLine>) -> Vec<usize> {
    lines.into_iter().map(UserLine::line).collect()
}

fn devices() -> Devices {
    let inputs: Vec<Input> = serde_json::from_str(&format!(
        r#"[{{"identifier": "{TOUCHPAD}", "name": "Touchpad", "type": "touchpad",
              "vendor": 1267, "product": 12377}},
            {{"identifier": "{KEYBOARD}", "name": "Keyboard", "type": "keyboard",
              "vendor": 1, "product": 1}}]"#
    ))
    .unwrap();
    Devices::new(&inputs)
}

#[test]
fn lines_before_the_include_are_overridden_and_lines_after_it_win() {
    let status = status(&format!(
        "focus_follows_mouse no\nfocus_follows_mouse always\n{INCLUDE}\nfocus_follows_mouse yes\n"
    ));
    let report = report(&status, "behavior.focus_follows_mouse", None);
    assert_eq!(numbers(report.lines()), [1, 2, 4]);
    assert_eq!(numbers(report.overridden()), [1, 2]);
    let winner = report.overriding().unwrap();
    assert_eq!((winner.line(), winner.order), (4, Order::After));
    assert_eq!(winner.text(), "focus_follows_mouse yes");
    assert_eq!(winner.file(), Path::new(MAIN));
    // The user's config alone: the last line.
    assert_eq!(
        report.user_value().unwrap().value,
        Ok(Value::Choice("yes".to_owned()))
    );
}

#[test]
fn with_the_include_at_the_end_nothing_overrides_sway_things() {
    let status = status(&format!("gaps inner 4\n{INCLUDE}\n"));
    let report = report(&status, "windows.gaps.inner", None);
    assert_eq!(numbers(report.overridden()), [1]);
    assert_eq!(report.overriding(), None);
    assert_eq!(report.user_value().unwrap().value, Ok(Value::Integer(4)));
}

#[test]
fn without_the_include_lines_only_give_the_users_value() {
    let status = status("gaps inner 4\ngaps inner 6\n");
    let report = report(&status, "windows.gaps.inner", None);
    assert!(
        report
            .lines()
            .iter()
            .all(|line| line.order == Order::NotIncluded)
    );
    assert_eq!(report.overridden().count(), 0);
    assert_eq!(report.overriding(), None);
    assert_eq!(report.user_value().unwrap().line(), 2);
}

#[test]
fn a_line_sway_rejects_sets_nothing() {
    let status = status(&format!(
        "{INCLUDE}\nfocus_follows_mouse always\nfocus_follows_mouse Yes\n"
    ));
    let report = report(&status, "behavior.focus_follows_mouse", None);
    assert_eq!(report.lines().len(), 2);
    assert!(matches!(
        report.lines()[1].value,
        Err(ReadError::Rejected(_))
    ));
    assert_eq!(report.overriding().unwrap().line(), 2);
    assert_eq!(report.user_value().unwrap().line(), 2);
}

#[test]
fn a_value_the_setting_cant_hold_still_wins() {
    let status = status(&format!("{INCLUDE}\ninput * repeat_delay 70000\n"));
    let report = report(&status, "input.keyboard.repeat_delay", None);
    let winner = report.overriding().unwrap();
    assert!(matches!(winner.value, Err(ReadError::Unsupported(_))));
}

#[test]
fn a_client_line_conflicts_with_every_color_of_the_group() {
    let status = status(&format!(
        "client.focused #111111 #222222 #333333\n{INCLUDE}\n"
    ));
    for key in [
        "windows.focused.border",
        "windows.focused.background",
        "windows.focused.text",
        "windows.focused.indicator",
        "windows.focused.child_border",
    ] {
        let report = report(&status, key, None);
        assert_eq!(numbers(report.overridden()), [1], "{key}");
    }
    let child_border = report(&status, "windows.focused.child_border", None);
    assert_eq!(
        child_border.user_value().unwrap().value,
        Ok(Value::Color(Color::rgb(0x22, 0x22, 0x22)))
    );
}

#[test]
fn input_lines_for_all_devices_and_the_type_decide_the_type() {
    let status = status(&format!(
        "input type:touchpad {{\n  tap enabled\n}}\n{INCLUDE}\ninput * tap disabled\n\
         input type:keyboard tap enabled\ninput {TOUCHPAD} tap enabled\n"
    ));
    let report = report(&status, "input.touchpad.tap", None);
    // `type:keyboard` is another type; the line for one touchpad doesn't decide the type.
    assert_eq!(numbers(report.lines()), [2, 5, 7]);
    assert_eq!(numbers(report.overridden()), [2]);
    let winner = report.overriding().unwrap();
    assert_eq!((winner.line(), &winner.value), (5, &Ok(Value::Bool(false))));
    assert_eq!(numbers(report.for_devices()), [7]);
    assert_eq!(report.lines()[2].reach, Reach::Device(TOUCHPAD.to_owned()));
}

#[test]
fn for_one_device_its_identifier_counts_like_the_type() {
    let config = status(&format!(
        "{INCLUDE}\ninput * tap disabled\ninput {TOUCHPAD} tap enabled\ninput 2:2:Other tap disabled\n"
    ));
    let for_touchpad = report(&config, "input.touchpad.tap", Some(TOUCHPAD));
    assert_eq!(for_touchpad.device(), Some(TOUCHPAD));
    assert_eq!(numbers(for_touchpad.lines()), [2, 3]);
    let winner = for_touchpad.overriding().unwrap();
    assert_eq!((winner.line(), &winner.value), (3, &Ok(Value::Bool(true))));
    // Identifiers compare like sway's `strcmp`.
    let lower = TOUCHPAD.to_lowercase();
    let lowercase = status(&format!("{INCLUDE}\ninput {lower} tap enabled\n"));
    assert_eq!(
        report_lines(&lowercase, Some(TOUCHPAD)),
        Vec::<usize>::new()
    );
}

fn report_lines(status: &IncludeStatus, device: Option<&str>) -> Vec<usize> {
    numbers(report(status, "input.touchpad.tap", device).lines())
}

#[test]
fn known_devices_of_another_type_are_left_out() {
    let status = status(&format!(
        "{INCLUDE}\ninput {KEYBOARD} tap enabled\ninput {TOUCHPAD} tap enabled\n"
    ));
    let devices = devices();
    let setting = Registry::builtin().get("input.touchpad.tap").unwrap();
    let report = Report::check(&status, setting, None, Some(&devices)).unwrap();
    assert_eq!(numbers(report.lines()), [3]);
}

#[test]
fn lines_in_included_files_count_where_sway_reads_them() {
    let files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .file(
            MAIN,
            format!("include ~/.config/sway/before\n{INCLUDE}\ninclude ~/.config/sway/after\n"),
        )
        .file("/home/me/.config/sway/before", "gaps inner 2\n")
        .file("/home/me/.config/sway/after", "\ngaps inner 9\n");
    let report = report(&status_of(&files), "windows.gaps.inner", None);
    let overridden: Vec<_> = report.overridden().collect();
    assert_eq!(overridden.len(), 1);
    assert_eq!(
        overridden[0].file(),
        Path::new("/home/me/.config/sway/before")
    );
    let winner = report.overriding().unwrap();
    assert_eq!(
        (winner.file(), winner.line()),
        (Path::new("/home/me/.config/sway/after"), 2)
    );
}

#[test]
fn the_include_in_an_included_file_sets_the_position() {
    let files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .file(
            MAIN,
            "gaps inner 2\ninclude ~/.config/sway/conf.d/*\ngaps inner 5\n",
        )
        .file(
            "/home/me/.config/sway/conf.d/50-things",
            format!("{INCLUDE}\n"),
        );
    let report = report(&status_of(&files), "windows.gaps.inner", None);
    assert_eq!(numbers(report.overridden()), [1]);
    assert_eq!(report.overriding().unwrap().line(), 3);
}

#[test]
fn every_sway_setting_and_known_device_gets_a_report() {
    let status = status(&format!("{INCLUDE}\n"));
    let devices = devices();
    let reports = check_all(&status, Registry::builtin(), Some(&devices));
    let sway_settings = Registry::builtin()
        .iter()
        .filter(|setting| setting.sway().is_some())
        .count();
    let per_device: Vec<_> = reports
        .iter()
        .filter_map(|report| report.device().map(|device| (report.key(), device)))
        .collect();
    // Every touchpad setting for the touchpad, every keyboard setting for the keyboard.
    let of_type = |kind| {
        Registry::builtin()
            .iter()
            .filter(|setting| setting.scope() == Scope::PerDevice(kind))
            .count()
    };
    assert_eq!(reports.len(), sway_settings + per_device.len());
    assert_eq!(
        per_device.len(),
        of_type(InputType::Touchpad) + of_type(InputType::Keyboard)
    );
    assert!(per_device.contains(&("input.touchpad.tap", TOUCHPAD)));
    assert!(per_device.contains(&("input.keyboard.repeat_delay", KEYBOARD)));
    assert!(reports.iter().all(|report| report.lines().is_empty()));
}

#[test]
fn preferences_of_sway_things_have_no_report() {
    let status = status(&format!("{INCLUDE}\n"));
    let setting = Registry::builtin().get("app.safety_net_timeout").unwrap();
    assert_eq!(Report::check(&status, setting, None, None), None);
}
