// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;
use std::path::Path;

use super::location::{self, Coordinates};
use super::{HEADER, PlanError, Problem, Schedule, Settings, Tool, own_lines, plan, write_config};
use crate::registry::Value;
use crate::sway_config::{MemoryFiles, parse};

fn settings(values: &[(&str, Value)]) -> Settings {
    let values: BTreeMap<String, Value> = values
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect();
    Settings::read(|key| values.get(key).cloned())
}

fn args(plan: &super::Plan) -> Vec<String> {
    plan.args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

const BERLIN: Coordinates = Coordinates {
    latitude: 52.5,
    longitude: 13.366_666,
};

#[expect(
    clippy::unnecessary_wraps,
    reason = "it stands in for the location of the time zone, which may be missing"
)]
fn berlin() -> Result<Coordinates, Option<String>> {
    Ok(BERLIN)
}

#[test]
fn the_defaults_are_sunset_to_sunrise_at_4000_k_with_wlsunset() {
    let defaults = settings(&[]);
    assert!(!defaults.on);
    assert_eq!(defaults.tool, Tool::Wlsunset);
    assert_eq!(defaults.schedule, Schedule::Sunset);
    assert_eq!(defaults.custom, None);
    assert_eq!((defaults.night, defaults.day), (4000, 6500));
    assert_eq!((defaults.from, defaults.to), ((20, 0), (7, 0)));
    let plan = plan(&defaults, Tool::Wlsunset, berlin, Path::new("/c.ini")).unwrap();
    assert_eq!(
        args(&plan),
        ["-l", "52.5000", "-L", "13.3667", "-t", "4000", "-T", "6500"]
    );
    assert_eq!(plan.config, None);
}

#[test]
fn a_location_entered_by_hand_wins_over_the_time_zone() {
    let custom = settings(&[
        ("night_light.location", Value::Choice("custom".into())),
        ("night_light.latitude", Value::Decimal(-33.86)),
        ("night_light.longitude", Value::Decimal(151.2)),
    ]);
    let plan = plan(&custom, Tool::Wlsunset, || Err(None), Path::new("/c.ini")).unwrap();
    assert_eq!(
        args(&plan),
        [
            "-l", "-33.8600", "-L", "151.2000", "-t", "4000", "-T", "6500"
        ]
    );
    let zone = settings(&[]);
    assert_eq!(
        super::plan(
            &zone,
            Tool::Wlsunset,
            || Err(Some("Etc/UTC".into())),
            Path::new("/c")
        ),
        Err(PlanError::NoLocation(Some("Etc/UTC".into())))
    );
}

#[test]
fn fixed_times_fade_for_half_an_hour_around_them() {
    let times = settings(&[
        ("night_light.schedule", Value::Choice("times".into())),
        ("night_light.from", Value::Text("21:15".into())),
        ("night_light.to", Value::Text("6:00".into())),
        ("night_light.temperature_night", Value::Integer(3000)),
    ]);
    let wlsunset = plan(&times, Tool::Wlsunset, || Err(None), Path::new("/c.ini")).unwrap();
    assert_eq!(
        args(&wlsunset),
        [
            "-S", "06:00", "-s", "21:15", "-d", "1800", "-t", "3000", "-T", "6500"
        ]
    );
    let gammastep = plan(&times, Tool::Gammastep, || Err(None), Path::new("/c.ini")).unwrap();
    assert_eq!(args(&gammastep), ["-c", "/c.ini"]);
    assert_eq!(
        gammastep.config.as_deref(),
        Some(
            format!(
                "{HEADER}\n[general]\ntemp-day=6500\ntemp-night=3000\nfade=1\n\
                 adjustment-method=wayland\ndawn-time=05:30-06:00\ndusk-time=21:15-21:45\n"
            )
            .as_str()
        )
    );
    // Near midnight the fades stay within the day, as gammastep wants them.
    let late = settings(&[
        ("night_light.schedule", Value::Choice("times".into())),
        ("night_light.from", Value::Text("23:50".into())),
        ("night_light.to", Value::Text("00:10".into())),
    ]);
    let config = plan(&late, Tool::Gammastep, || Err(None), Path::new("/c.ini"))
        .unwrap()
        .config
        .unwrap();
    assert!(
        config.contains("dawn-time=00:00-00:10\ndusk-time=23:50-23:59\n"),
        "{config}"
    );
    let wrong = settings(&[
        ("night_light.schedule", Value::Choice("times".into())),
        ("night_light.from", Value::Text("01:00".into())),
        ("night_light.to", Value::Text("05:00".into())),
    ]);
    assert_eq!(
        plan(&wrong, Tool::Wlsunset, || Err(None), Path::new("/c")),
        Err(PlanError::TimesOrder)
    );
}

#[test]
fn always_on_is_the_night_temperature_all_day() {
    let always = settings(&[
        ("night_light.schedule", Value::Choice("always".into())),
        ("night_light.temperature_night", Value::Integer(3500)),
        ("night_light.temperature_day", Value::Integer(5000)),
    ]);
    let wlsunset = plan(&always, Tool::Wlsunset, || Err(None), Path::new("/c.ini")).unwrap();
    assert_eq!(
        args(&wlsunset),
        [
            "-S", "06:00", "-s", "18:00", "-d", "1800", "-t", "3499", "-T", "3500"
        ]
    );
    let gammastep = plan(&always, Tool::Gammastep, || Err(None), Path::new("/c.ini")).unwrap();
    let config = gammastep.config.unwrap_or_default();
    assert!(
        config.contains("temp-day=3500\ntemp-night=3500\n"),
        "{config}"
    );
    assert!(!config.contains("location-provider"), "{config}");
}

#[test]
fn wlsunset_gets_a_day_warmer_than_the_night_never_equal() {
    let same = settings(&[
        ("night_light.temperature_night", Value::Integer(6500)),
        ("night_light.temperature_day", Value::Integer(5000)),
    ]);
    let plan = plan(&same, Tool::Wlsunset, berlin, Path::new("/c.ini")).unwrap();
    assert_eq!(&args(&plan)[4..], ["-t", "6499", "-T", "6500"]);
}

#[test]
fn gammastep_reads_the_location_from_its_manual_section() {
    let on = settings(&[("night_light.tool", Value::Choice("gammastep".into()))]);
    assert_eq!(on.tool, Tool::Gammastep);
    let plan = plan(&on, Tool::Gammastep, berlin, Path::new("/c.ini")).unwrap();
    let config = plan.config.unwrap_or_default();
    assert!(
        config.ends_with("location-provider=manual\n\n[manual]\nlat=52.5000\nlon=13.3667\n"),
        "{config}"
    );
}

#[test]
fn the_fingerprint_changes_with_the_plan() {
    let a = plan(&settings(&[]), Tool::Wlsunset, berlin, Path::new("/c")).unwrap();
    let b = plan(
        &settings(&[("night_light.temperature_night", Value::Integer(3900))]),
        Tool::Wlsunset,
        berlin,
        Path::new("/c"),
    )
    .unwrap();
    assert_eq!(a.fingerprint(), a.clone().fingerprint());
    assert_ne!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.fingerprint().len(), 16);
}

#[test]
fn the_tool_chosen_runs_if_installed_else_the_other() {
    let wlsunset = settings(&[]);
    assert_eq!(wlsunset.tool_of(&Tool::ALL), Some(Tool::Wlsunset));
    assert_eq!(wlsunset.tool_of(&[Tool::Gammastep]), Some(Tool::Gammastep));
    assert_eq!(wlsunset.tool_of(&[]), None);
}

#[test]
fn only_our_own_config_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gammastep/config.ini");
    let text = format!("{HEADER}\n[general]\n");
    write_config(&path, &text).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    std::fs::write(&path, "[general]\ntemp-day=5000\n").unwrap();
    assert_eq!(
        write_config(&path, &text),
        Err(Problem::Foreign(path.clone()))
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[general]\ntemp-day=5000\n"
    );
}

#[test]
fn lines_of_the_config_that_start_a_night_light_are_found() {
    let text = "exec wlsunset -l 52 -L 13\n\
                exec_always --no-startup-id /usr/bin/gammastep-indicator\n\
                exec sh -c 'pkill redshift; redshift -m wayland'\n\
                exec swaybg -i ~/a.png\n\
                exec_always sway-things hook night-light\n\
                bindsym $mod+n exec wlsunset\n";
    let files = MemoryFiles::new().file("/c/config", text);
    let config = parse(Path::new("/c/config"), &files);
    let lines: Vec<usize> = own_lines(&config)
        .into_iter()
        .map(|directive| directive.line)
        .collect();
    assert_eq!(lines, [1, 2, 3]);
}

#[test]
fn the_time_zone_gives_its_main_city() {
    let table = "# tzdb timezone descriptions\n\
                 DE,DK,NO,SE,SJ\t+5230+01322\tEurope/Berlin\tmost of Germany\n\
                 US\t+404251-0740023\tAmerica/New_York\tEastern (most areas)\n\
                 AU\t-3352+15113\tAustralia/Sydney\tNew South Wales (most areas)\n";
    let berlin = location::in_table(table, "Europe/Berlin").unwrap();
    assert!((berlin.latitude - 52.5).abs() < 1e-9);
    assert!((berlin.longitude - (13.0 + 22.0 / 60.0)).abs() < 1e-9);
    let new_york = location::in_table(table, "America/New_York").unwrap();
    assert!((new_york.latitude - (40.0 + 42.0 / 60.0 + 51.0 / 3600.0)).abs() < 1e-9);
    assert!((new_york.longitude + (74.0 + 23.0 / 3600.0)).abs() < 1e-9);
    let sydney = location::in_table(table, "Australia/Sydney").unwrap();
    assert!(sydney.latitude < 0.0 && sydney.longitude > 0.0);
    assert_eq!(location::in_table(table, "Etc/UTC"), None);
    for wrong in [
        "",
        "+",
        "5230+01322",
        "+52+013",
        "+5230+0132",
        "+52a0+01322",
    ] {
        assert_eq!(location::parse_iso6709(wrong), None, "{wrong}");
    }
}

#[test]
fn the_zone_is_what_follows_zoneinfo() {
    assert_eq!(
        location::zone_of_path(Path::new("/usr/share/zoneinfo/Europe/Berlin")).as_deref(),
        Some("Europe/Berlin")
    );
    assert_eq!(
        location::zone_of_path(Path::new("../usr/share/zoneinfo/posix/America/New_York"))
            .as_deref(),
        Some("America/New_York")
    );
    assert_eq!(
        location::zone_of_path(Path::new("/usr/share/zoneinfo/UTC")),
        None
    );
    assert_eq!(location::zone_of_path(Path::new("/etc/foo")), None);
}

#[test]
fn the_tables_of_the_database_are_read_in_turn() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("zone1970.tab"),
        "DE\t+5230+01322\tEurope/Berlin\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("zone.tab"),
        "DK\t+5540+01235\tEurope/Copenhagen\n",
    )
    .unwrap();
    assert!(location::of_zone("Europe/Berlin", dir.path()).is_some());
    assert!(location::of_zone("Europe/Copenhagen", dir.path()).is_some());
    assert_eq!(location::of_zone("Europe/Paris", dir.path()), None);
}
