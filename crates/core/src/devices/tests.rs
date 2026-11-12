// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use async_io::block_on;

use super::{Change, Device, DeviceWatch, Devices, Lockout, Sending, switch_layout};
use crate::command::input::{InputSelector, InputType, XkbSwitch};
use crate::ipc::IpcError;
use crate::ipc::tests::{SWAY_1_10, obedient, with_inputs};
use crate::ipc::types::Input;
use crate::registry::{Registry, Value};
use crate::state::{Paths, State};

const KEYBOARD: &str = "1:1:AT_Translated_Set_2_keyboard";
const USB_KEYBOARD: &str = "1241:662:USB-HID_Keyboard";
const TOUCHPAD: &str = "1267:12377:ELAN1300:00_04F3:3059_Touchpad";
const MOUSE: &str = "1133:16500:Logitech_G305";
const POWER_BUTTON: &str = "0:1:Power_Button";

/// The devices of a laptop, as sway 1.10 reports them (shortened to the fields that are read,
/// plus some that aren't).
const LAPTOP: &str = r#"[
  {"identifier": "0:1:Power_Button", "name": "Power Button", "type": "keyboard",
   "vendor": 0, "product": 1, "repeat_delay": 600, "repeat_rate": 25,
   "xkb_layout_names": ["English (US)", "German"], "xkb_active_layout_index": 0,
   "xkb_active_layout_name": "English (US)", "libinput": {"send_events": "enabled"}},
  {"identifier": "0:6:Video_Bus", "name": "Video Bus", "type": "keyboard",
   "vendor": 0, "product": 6,
   "xkb_layout_names": ["English (US)", "German"], "xkb_active_layout_index": 0,
   "xkb_active_layout_name": "English (US)", "libinput": {"send_events": "enabled"}},
  {"identifier": "0:5:Lid_Switch", "name": "Lid Switch", "type": "switch",
   "vendor": 0, "product": 5, "libinput": {"send_events": "enabled"}},
  {"identifier": "1:1:AT_Translated_Set_2_keyboard", "name": "AT Translated Set 2 keyboard",
   "type": "keyboard", "vendor": 1, "product": 1, "repeat_delay": 600, "repeat_rate": 25,
   "xkb_layout_names": ["English (US)", "German"], "xkb_active_layout_index": 1,
   "xkb_active_layout_name": "German", "libinput": {"send_events": "enabled"}},
  {"identifier": "1267:12377:ELAN1300:00_04F3:3059_Touchpad",
   "name": "ELAN1300:00 04F3:3059 Touchpad", "type": "touchpad", "vendor": 1267,
   "product": 12377, "scroll_factor": 1.0,
   "libinput": {"send_events": "enabled", "tap": "enabled", "tap_button_map": "lrm",
                "tap_drag": "enabled", "tap_drag_lock": "disabled", "accel_speed": 0.0,
                "accel_profile": "adaptive", "natural_scroll": "disabled",
                "left_handed": "disabled", "click_method": "button_areas",
                "middle_emulation": "disabled", "scroll_method": "two_finger", "dwt": "enabled",
                "dwtp": "enabled"}},
  {"identifier": "0:0:Some_future_device", "name": "Some future device", "type": "dial"}
]"#;

fn inputs(json: &str) -> Vec<Input> {
    serde_json::from_str(json).unwrap()
}

/// The laptop with more devices: `(identifier, name, type, send_events)`.
fn laptop_with(more: &[(&str, &str, &str, &str)]) -> Vec<Input> {
    let mut all = inputs(LAPTOP);
    for (identifier, name, kind, send_events) in more {
        all.extend(inputs(&format!(
            r#"[{{"identifier": "{identifier}", "name": "{name}", "type": "{kind}",
                  "libinput": {{"send_events": "{send_events}"}}}}]"#
        )));
    }
    all
}

const USB: (&str, &str, &str, &str) = (USB_KEYBOARD, "USB-HID Keyboard", "keyboard", "enabled");
const G305: (&str, &str, &str, &str) = (MOUSE, "Logitech G305", "pointer", "enabled");

fn identifiers<'a>(devices: impl Iterator<Item = &'a Device>) -> Vec<&'a str> {
    devices.map(Device::identifier).collect()
}

#[test]
fn the_devices_are_read_from_sways_reply() {
    let devices = Devices::new(&inputs(LAPTOP));
    assert_eq!(devices.iter().count(), 6);

    let touchpad = devices.get(TOUCHPAD).unwrap();
    assert_eq!(touchpad.name(), "ELAN1300:00 04F3:3059 Touchpad");
    assert_eq!(touchpad.kind(), Some(InputType::Touchpad));
    assert!(touchpad.is_connected());
    assert_eq!(touchpad.sending(), Some(Sending::Enabled));
    assert!(touchpad.is_enabled());
    assert_eq!(touchpad.layouts(), None);

    let keyboard = devices.get(KEYBOARD).unwrap();
    let layouts = keyboard.layouts().unwrap();
    assert_eq!(layouts.names, ["English (US)", "German"]);
    assert_eq!(layouts.active, Some(1));
    assert_eq!(layouts.active_name(), Some("German"));

    // A type this build doesn't know is listed without one, and without libinput it can't be
    // switched off.
    let future = devices.get("0:0:Some_future_device").unwrap();
    assert_eq!(future.kind(), None);
    assert_eq!(future.sending(), None);
    assert!(future.is_enabled());

    assert_eq!(
        identifiers(devices.of_type(InputType::Keyboard)),
        [POWER_BUTTON, "0:6:Video_Bus", KEYBOARD]
    );
    assert_eq!(identifiers(devices.of_type(InputType::Pointer)).len(), 0);
    assert!(devices.get("1:2:Nothing").is_none());
}

#[test]
fn a_layout_index_sway_doesnt_list_is_no_layout() {
    let devices = Devices::new(&inputs(
        r#"[{"identifier": "1:1:a", "name": "a", "type": "keyboard",
             "xkb_layout_names": ["English (US)"], "xkb_active_layout_index": 3},
            {"identifier": "1:1:b", "name": "b", "type": "keyboard"}]"#,
    ));
    let layouts = devices.get("1:1:a").unwrap().layouts().unwrap();
    assert_eq!(layouts.active, None);
    assert_eq!(layouts.active_name(), None);
    assert_eq!(
        devices.get("1:1:b").unwrap().layouts().unwrap().names.len(),
        0
    );
}

#[test]
fn the_buttons_of_the_system_are_no_keyboards_to_type_on() {
    let devices = Devices::new(&inputs(LAPTOP));
    assert!(!devices.get(POWER_BUTTON).unwrap().is_usable_keyboard());
    assert!(devices.get(KEYBOARD).unwrap().is_usable_keyboard());
    // The power button and the video bus are still "keyboards", but nobody types on them.
    assert!(devices.is_last_enabled_keyboard(KEYBOARD));
    assert!(!devices.is_last_enabled_keyboard(POWER_BUTTON));
    assert!(!devices.is_last_enabled_keyboard(TOUCHPAD));
    assert!(!devices.is_last_enabled_keyboard("1:2:Nothing"));

    // A second keyboard: neither is the last one.
    let devices = Devices::new(&laptop_with(&[USB]));
    assert!(!devices.is_last_enabled_keyboard(KEYBOARD));
    assert!(!devices.is_last_enabled_keyboard(USB_KEYBOARD));

    // Unless it is switched off.
    let off = (USB_KEYBOARD, "USB-HID Keyboard", "keyboard", "disabled");
    let devices = Devices::new(&laptop_with(&[off]));
    assert_eq!(
        devices.get(USB_KEYBOARD).unwrap().sending(),
        Some(Sending::Disabled)
    );
    assert!(devices.is_last_enabled_keyboard(KEYBOARD));
    assert!(!devices.is_last_enabled_keyboard(USB_KEYBOARD));
}

#[test]
fn the_last_pointing_device_is_a_mouse_or_a_touchpad() {
    let devices = Devices::new(&inputs(LAPTOP));
    assert!(devices.is_last_enabled_pointing_device(TOUCHPAD));
    assert!(!devices.is_last_enabled_pointing_device(KEYBOARD));

    let devices = Devices::new(&laptop_with(&[G305]));
    assert!(!devices.is_last_enabled_pointing_device(TOUCHPAD));
    assert!(!devices.is_last_enabled_pointing_device(MOUSE));

    // A touchpad that is off while a mouse is connected works again once the mouse is gone.
    let waiting = (
        "2:7:SynPS/2_Synaptics_TouchPad",
        "SynPS/2 Synaptics TouchPad",
        "touchpad",
        "disabled_on_external_mouse",
    );
    let devices = Devices::new(&laptop_with(&[waiting]));
    let synaptics = devices.get(waiting.0).unwrap();
    assert_eq!(synaptics.sending(), Some(Sending::DisabledOnExternalMouse));
    assert!(synaptics.is_usable_pointing_device());
    assert!(!devices.is_last_enabled_pointing_device(TOUCHPAD));

    // A virtual pointer of another program doesn't count.
    let virtual_pointer = (
        "0:0:wlr_virtual_pointer_v1",
        "virtual",
        "pointer",
        "enabled",
    );
    let devices = Devices::new(&laptop_with(&[virtual_pointer]));
    assert!(devices.is_last_enabled_pointing_device(TOUCHPAD));
}

#[test]
fn devices_of_the_same_model_are_one_entry() {
    let mut devices = Devices::new(&laptop_with(&[G305, G305]));
    assert_eq!(identifiers(devices.of_type(InputType::Pointer)), [MOUSE]);
    // One of the two is unplugged: the other is still there.
    assert_eq!(devices.update(&laptop_with(&[G305])), []);
    assert!(devices.get(MOUSE).is_some());
    assert_eq!(
        devices.update(&inputs(LAPTOP)),
        [Change::Removed(MOUSE.to_owned())]
    );
}

#[test]
fn an_update_reports_what_changed() {
    let mut devices = Devices::new(&inputs(LAPTOP));
    assert_eq!(devices.update(&inputs(LAPTOP)), []);

    let changes = devices.update(&laptop_with(&[G305, USB]));
    assert_eq!(
        changes,
        [
            Change::Added(MOUSE.to_owned()),
            Change::Added(USB_KEYBOARD.to_owned())
        ]
    );
    assert_eq!(changes[0].identifier(), MOUSE);

    // The mouse goes, the USB keyboard is switched off, the layout of the built-in one changes.
    let mut next = laptop_with(&[(USB_KEYBOARD, "USB-HID Keyboard", "keyboard", "disabled")]);
    next[3].xkb_active_layout_index = Some(0);
    assert_eq!(
        devices.update(&next),
        [
            Change::Removed(MOUSE.to_owned()),
            Change::Changed(KEYBOARD.to_owned()),
            Change::Changed(USB_KEYBOARD.to_owned()),
        ]
    );
    assert_eq!(
        devices.get(KEYBOARD).unwrap().layouts().unwrap().active,
        Some(0)
    );
    assert!(!devices.get(USB_KEYBOARD).unwrap().is_enabled());
}

#[test]
fn devices_with_stored_values_stay_listed_while_disconnected() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(
        &dir.path().join("config"),
        &dir.path().join("state"),
        "laptop",
    )
    .unwrap();
    let mut state = State::load(Registry::builtin(), paths);
    let tap = "input.touchpad.tap";
    let other = "1739:52619:SYNA8006:00_06CB:CD8B_Touchpad";
    state
        .set(tap, Some(TOUCHPAD), Value::Bool(true), None)
        .unwrap();
    state
        .set(tap, Some(other), Value::Bool(true), None)
        .unwrap();
    state
        .set(
            "input.keyboard.repeat_delay",
            Some(USB_KEYBOARD),
            Value::Integer(300),
            None,
        )
        .unwrap();
    // A value for every touchpad is no device.
    state.set(tap, None, Value::Bool(false), None).unwrap();

    let mut devices = Devices::new(&inputs(LAPTOP));
    assert!(devices.set_stored(&state));
    assert!(!devices.set_stored(&state));
    // The connected devices first, then the others by identifier. The connected touchpad is
    // listed once.
    assert_eq!(identifiers(devices.iter().skip(6)), [USB_KEYBOARD, other]);
    assert_eq!(
        identifiers(devices.of_type(InputType::Touchpad)),
        [TOUCHPAD, other]
    );

    let keyboard = devices.get(USB_KEYBOARD).unwrap();
    assert!(!keyboard.is_connected());
    assert!(!keyboard.is_enabled());
    assert_eq!(keyboard.kind(), Some(InputType::Keyboard));
    assert_eq!(keyboard.name(), "USB-HID_Keyboard");
    assert_eq!(keyboard.sending(), None);
    // It doesn't help against a lockout.
    assert!(devices.is_last_enabled_keyboard(KEYBOARD));

    // Plugged in, it is a connected device; unplugged, it is back as a stored one.
    assert_eq!(
        devices.update(&laptop_with(&[USB])),
        [Change::Added(USB_KEYBOARD.to_owned())]
    );
    assert_eq!(identifiers(devices.iter().skip(6)), [USB_KEYBOARD, other]);
    assert!(devices.get(USB_KEYBOARD).unwrap().is_connected());
    assert!(!devices.is_last_enabled_keyboard(KEYBOARD));
    assert_eq!(
        devices.update(&inputs(LAPTOP)),
        [Change::Removed(USB_KEYBOARD.to_owned())]
    );
    assert!(!devices.get(USB_KEYBOARD).unwrap().is_connected());

    // Without stored values a disconnected device is gone.
    state
        .reset("input.keyboard.repeat_delay", Some(USB_KEYBOARD))
        .unwrap();
    assert!(devices.set_stored(&state));
    assert!(devices.get(USB_KEYBOARD).is_none());
    assert_eq!(identifiers(devices.iter().skip(6)), [other]);
}

const WITH_MOUSE: &str = r#"[
  {"identifier": "1:1:AT_Translated_Set_2_keyboard", "name": "AT Translated Set 2 keyboard",
   "type": "keyboard", "libinput": {"send_events": "enabled"}},
  {"identifier": "1133:16500:Logitech_G305", "name": "Logitech G305", "type": "pointer",
   "libinput": {"send_events": "enabled"}}
]"#;
const WITHOUT_MOUSE: &str = r#"[
  {"identifier": "1:1:AT_Translated_Set_2_keyboard", "name": "AT Translated Set 2 keyboard",
   "type": "keyboard", "libinput": {"send_events": "enabled"}}
]"#;
const WITH_TWO: &str = r#"[
  {"identifier": "1:1:AT_Translated_Set_2_keyboard", "name": "AT Translated Set 2 keyboard",
   "type": "keyboard", "libinput": {"send_events": "disabled"}},
  {"identifier": "1133:16500:Logitech_G305", "name": "Logitech G305", "type": "pointer",
   "libinput": {"send_events": "enabled"}},
  {"identifier": "1241:662:USB-HID_Keyboard", "name": "USB-HID Keyboard", "type": "keyboard",
   "libinput": {"send_events": "enabled"}}
]"#;

#[test]
fn the_watch_follows_sways_events() {
    // Each event makes the watch read the devices again. The second one changes nothing and is
    // skipped; the third brings two changes.
    let sway = with_inputs(&[
        WITHOUT_MOUSE,
        WITH_MOUSE,
        WITH_MOUSE,
        WITH_TWO,
        WITHOUT_MOUSE,
    ]);
    let client = sway.connect();
    let mut watch = block_on(DeviceWatch::start(&client)).unwrap();
    assert_eq!(identifiers(watch.devices().iter()), [KEYBOARD]);

    let mut next = || block_on(watch.changed()).map(Result::unwrap);
    assert_eq!(next(), Some(Change::Added(MOUSE.to_owned())));
    assert_eq!(next(), Some(Change::Changed(KEYBOARD.to_owned())));
    assert_eq!(next(), Some(Change::Added(USB_KEYBOARD.to_owned())));
    assert_eq!(next(), Some(Change::Removed(MOUSE.to_owned())));
    assert_eq!(next(), Some(Change::Removed(USB_KEYBOARD.to_owned())));
    assert_eq!(next(), Some(Change::Changed(KEYBOARD.to_owned())));
    // sway closed the connection.
    assert_eq!(next(), None);
    assert_eq!(identifiers(watch.devices().iter()), [KEYBOARD]);
    assert!(watch.devices().get(KEYBOARD).unwrap().is_enabled());
}

#[test]
fn a_device_list_that_cant_be_read_is_an_error_for_that_event() {
    let sway = with_inputs(&[WITHOUT_MOUSE, "{}", WITH_MOUSE]);
    let client = sway.connect();
    let mut watch = block_on(DeviceWatch::start(&client)).unwrap();
    assert!(matches!(
        block_on(watch.changed()),
        Some(Err(crate::ipc::IpcError::InvalidReply { .. }))
    ));
    assert_eq!(
        block_on(watch.changed()).map(Result::unwrap),
        Some(Change::Added(MOUSE.to_owned()))
    );
    assert!(block_on(watch.changed()).is_none());
}

#[test]
fn switching_off_the_last_keyboard_is_a_lockout() {
    let laptop = Devices::new(&inputs(LAPTOP));
    let off = Sending::Disabled;
    // The power button and the video bus don't count: nobody types with them.
    assert_eq!(
        laptop.lockout(InputType::Keyboard, None, off),
        Some(Lockout::Keyboard)
    );
    assert_eq!(
        laptop.lockout(InputType::Keyboard, Some(KEYBOARD), off),
        Some(Lockout::Keyboard)
    );
    // A keyboard that isn't connected changes nothing.
    assert_eq!(
        laptop.lockout(InputType::Keyboard, Some(USB_KEYBOARD), off),
        None
    );
    assert_eq!(
        laptop.lockout(InputType::Keyboard, None, Sending::Enabled),
        None
    );
    let docked = Devices::new(&laptop_with(&[USB]));
    assert_eq!(
        docked.lockout(InputType::Keyboard, Some(KEYBOARD), off),
        None
    );
    assert_eq!(
        docked.lockout(InputType::Keyboard, None, off),
        Some(Lockout::Keyboard)
    );
}

#[test]
fn switching_off_the_last_pointing_device_is_a_lockout() {
    let laptop = Devices::new(&inputs(LAPTOP));
    assert_eq!(
        laptop.lockout(InputType::Touchpad, None, Sending::Disabled),
        Some(Lockout::PointingDevice)
    );
    // Off while a mouse is connected works again without the mouse.
    assert_eq!(
        laptop.lockout(
            InputType::Touchpad,
            Some(TOUCHPAD),
            Sending::DisabledOnExternalMouse
        ),
        None
    );
    let with_mouse = Devices::new(&laptop_with(&[G305]));
    assert_eq!(
        with_mouse.lockout(InputType::Touchpad, None, Sending::Disabled),
        None
    );
    assert_eq!(
        with_mouse.lockout(InputType::Pointer, Some(MOUSE), Sending::Disabled),
        None
    );
}

#[test]
fn a_session_without_devices_has_nothing_to_lose() {
    let none = Devices::new(&[]);
    assert_eq!(
        none.lockout(InputType::Keyboard, None, Sending::Disabled),
        None
    );
}

#[test]
fn without_a_device_list_only_switching_off_a_whole_type_is_a_lockout() {
    let off = Sending::Disabled;
    assert_eq!(
        Lockout::without_devices(InputType::Keyboard, None, off),
        Some(Lockout::Keyboard)
    );
    assert_eq!(
        Lockout::without_devices(InputType::Touchpad, None, off),
        Some(Lockout::PointingDevice)
    );
    assert_eq!(
        Lockout::without_devices(InputType::Keyboard, Some(KEYBOARD), off),
        None
    );
    assert_eq!(
        Lockout::without_devices(InputType::Touchpad, None, Sending::DisabledOnExternalMouse),
        None
    );
    assert_eq!(
        Lockout::Keyboard.to_string(),
        "this would turn off the last keyboard you can type with"
    );
}

#[test]
fn the_layout_is_switched_over_ipc() {
    let sway = obedient(SWAY_1_10);
    let mut client = sway.connect();
    let keyboard = InputSelector::device(KEYBOARD).unwrap();
    block_on(switch_layout(&mut client, None, XkbSwitch::Next)).unwrap();
    block_on(switch_layout(&mut client, None, XkbSwitch::Prev)).unwrap();
    block_on(switch_layout(
        &mut client,
        Some(&keyboard),
        XkbSwitch::Index(1),
    ))
    .unwrap();
    assert_eq!(
        sway.commands(),
        [
            "input type:keyboard xkb_switch_layout next",
            "input type:keyboard xkb_switch_layout prev",
            r#"input "1:1:AT_Translated_Set_2_keyboard" xkb_switch_layout 1"#,
        ]
    );
    // The stand-in fails commands that contain "fail".
    let failing = InputSelector::device("1:1:fail").unwrap();
    assert!(matches!(
        block_on(switch_layout(&mut client, Some(&failing), XkbSwitch::Next)),
        Err(IpcError::CommandFailed { .. })
    ));
}
