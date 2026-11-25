// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::sync::{Arc, Mutex};

use super::ddc::{self, Access, Level, Monitor};
use super::{Device, Kind, Logind, devices_in, percent_of, raw_of};

fn write_device(root: &Path, class: &str, name: &str, kind: Option<&str>, value: u32, max: u32) {
    let dir = root.join(class).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("brightness"), format!("{value}\n")).unwrap();
    std::fs::write(dir.join("max_brightness"), format!("{max}\n")).unwrap();
    if let Some(kind) = kind {
        std::fs::write(dir.join("type"), format!("{kind}\n")).unwrap();
    }
}

#[test]
fn devices_are_the_backlights_by_preference_then_the_keyboard() {
    let root = tempfile::tempdir().unwrap();
    write_device(
        root.path(),
        "backlight",
        "intel_backlight",
        Some("raw"),
        600,
        1200,
    );
    write_device(
        root.path(),
        "backlight",
        "acpi_video0",
        Some("firmware"),
        5,
        10,
    );
    write_device(root.path(), "backlight", "broken", Some("raw"), 5, 0);
    write_device(root.path(), "leds", "tpacpi::kbd_backlight", None, 1, 2);
    write_device(root.path(), "leds", "input0::capslock", None, 0, 1);
    let found = devices_in(root.path());
    let names: Vec<(&str, Kind)> = found
        .iter()
        .map(|device| (device.name.as_str(), device.kind))
        .collect();
    assert_eq!(
        names,
        [
            ("acpi_video0", Kind::Firmware),
            ("intel_backlight", Kind::Raw),
            ("tpacpi::kbd_backlight", Kind::Keyboard),
        ]
    );
    assert_eq!(found[1].percent(), 50);
    assert_eq!(found[2].kind.subsystem(), "leds");
}

#[test]
fn a_backlight_never_goes_below_one_percent_but_the_keyboard_goes_off() {
    let panel = Device {
        name: "amdgpu_bl1".into(),
        kind: Kind::Raw,
        brightness: 49150,
        max: 65535,
    };
    assert_eq!(panel.min_raw(), 655);
    assert_eq!(panel.raw_for(0), 655);
    assert_eq!(panel.raw_for(100), 65535);
    assert_eq!(panel.raw_for(150), 65535);
    // A small range still keeps one step lit.
    let small = Device {
        name: "acpi_video0".into(),
        kind: Kind::Firmware,
        brightness: 1,
        max: 7,
    };
    assert_eq!(small.min_raw(), 1);
    assert_eq!(small.stepped(false), 1);
    assert_eq!(small.stepped(true), 2);
    let keyboard = Device {
        name: "tpacpi::kbd_backlight".into(),
        kind: Kind::Keyboard,
        brightness: 1,
        max: 2,
    };
    assert_eq!(keyboard.raw_for(0), 0);
    assert_eq!(keyboard.stepped(false), 0);
    assert_eq!(keyboard.stepped(true), 2);
}

#[test]
fn percent_and_raw_round_to_the_nearest() {
    assert_eq!(percent_of(49150, 65535), 75);
    assert_eq!(percent_of(0, 0), 0);
    assert_eq!(percent_of(20, 10), 100);
    assert_eq!(raw_of(75, 65535), 49151);
    assert_eq!(raw_of(50, 7), 4);
    for max in [1, 7, 100, 255, 1200, 65535, 120_000] {
        for percent in 0..=100 {
            let back = percent_of(raw_of(percent, max), max);
            assert!(
                back.abs_diff(percent) <= 100 / max.min(100) + 1,
                "{max} {percent}"
            );
        }
    }
}

const DETECT: &str = "\
Display 1
   I2C bus:          /dev/i2c-6
   DRM connector:    card1-DP-1
   drm_connector_id: 101
   Monitor:          DEL:DELL U2720Q:ABC123

Invalid display
   I2C bus:          /dev/i2c-2
   DRM connector:    card1-eDP-1
   Monitor:          LEN:TL140ADXP22:

Display 2
   I2C bus:          /dev/i2c-7
   DRM_connector:    card1-HDMI-A-1
   Monitor:          GSM::

Display 3
   I2C bus:          /dev/i2c-3
   DRM connector:    card0-eDP-1
   Monitor:          BOE:NE135:
";

#[test]
fn detect_finds_the_external_monitors_by_bus() {
    assert_eq!(
        ddc::parse_detect(DETECT),
        [
            Monitor {
                bus: 6,
                name: "DELL U2720Q".into(),
                connector: Some("DP-1".into()),
            },
            Monitor {
                bus: 7,
                name: "GSM".into(),
                connector: Some("HDMI-A-1".into()),
            },
        ]
    );
    assert_eq!(ddc::parse_detect("No displays found.\n"), []);
}

#[test]
fn a_brightness_answer_is_current_and_max() {
    assert_eq!(
        ddc::parse_level("VCP 10 C 40 100\n"),
        Some(Level {
            current: 40,
            max: 100
        })
    );
    assert_eq!(ddc::parse_level("VCP 10 ERR\n"), None);
    assert_eq!(ddc::parse_level("VCP 10 C 40 0\n"), None);
    assert_eq!(ddc::parse_level("VCP 12 C 40 100\n"), None);
    assert_eq!(
        ddc::parse_level("VCP 10 C 40 100\n").map(Level::percent),
        Some(40)
    );
}

#[test]
fn access_tells_missing_buses_from_missing_rights() {
    let dev = tempfile::tempdir().unwrap();
    assert_eq!(ddc::access_in(dev.path()), Access::NoBuses);
    // Regular files stand in for the buses: one that can be opened is enough.
    std::fs::write(dev.path().join("i2c-3"), "").unwrap();
    assert_eq!(ddc::access_in(dev.path()), Access::Granted);
}

/// A stand-in for logind's session, which notes what it was asked to set.
struct Session {
    calls: Arc<Mutex<Vec<(String, String, u32)>>>,
}

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Session {
    fn set_brightness(&self, subsystem: String, name: String, brightness: u32) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((subsystem, name, brightness));
        }
    }
}

#[test]
fn logind_is_asked_for_the_raw_value_within_the_limits() {
    async_io::block_on(async {
        let bus = crate::dbus::TestBus::start().await.unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let path = "/org/freedesktop/login1/session/auto";
        let server = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name("org.freedesktop.login1")
            .unwrap()
            .serve_at(
                path,
                Session {
                    calls: Arc::clone(&calls),
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        let logind = Logind::with(bus.connection.clone(), path);
        let panel = Device {
            name: "intel_backlight".into(),
            kind: Kind::Raw,
            brightness: 600,
            max: 1200,
        };
        logind.set(&panel, 0).await.unwrap();
        logind.set(&panel, 900).await.unwrap();
        let keyboard = Device {
            name: "tpacpi::kbd_backlight".into(),
            kind: Kind::Keyboard,
            brightness: 1,
            max: 2,
        };
        logind.set(&keyboard, 0).await.unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            [
                ("backlight".into(), "intel_backlight".into(), 12),
                ("backlight".into(), "intel_backlight".into(), 900),
                ("leds".into(), "tpacpi::kbd_backlight".into(), 0),
            ]
        );
        drop(server);
        let mut daemon = bus.daemon;
        let _ = daemon.kill();
        let _ = daemon.wait();
    });
}
