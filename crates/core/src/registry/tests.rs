// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{BTreeMap, BTreeSet};

use super::{
    Apply, Block, ConfigFile, Defaults, Kind, Page, Registry, RenderError, Scope, Setting,
    SwayMapping, Value, ValueError,
};
use crate::command::input::InputSelector;
use crate::command::{Availability, Color, SwayCmd, Target};

fn registry() -> &'static Registry {
    Registry::builtin()
}

fn setting(key: &str) -> &'static Setting {
    registry()
        .get(key)
        .unwrap_or_else(|| panic!("no setting {key}"))
}

fn sway_settings() -> impl Iterator<Item = (&'static Setting, &'static SwayMapping)> {
    registry()
        .iter()
        .filter_map(|setting| setting.sway().map(|mapping| (setting, mapping)))
}

/// The default and the corners of the value type: both booleans, the limits of a number, every
/// choice.
fn samples(setting: &Setting) -> Vec<Value> {
    let mut samples = vec![setting.default_value()];
    match setting.kind() {
        Kind::Bool { .. } => samples.extend([Value::Bool(false), Value::Bool(true)]),
        Kind::Integer { min, max, .. } => {
            samples.extend([Value::Integer(*min), Value::Integer(*max)]);
        }
        Kind::Decimal { min, max, .. } => {
            samples.extend([Value::Decimal(*min), Value::Decimal(*max)]);
        }
        Kind::Choice { choices, .. } => {
            samples.extend(
                choices
                    .iter()
                    .map(|choice| Value::Choice(choice.id.to_owned())),
            );
        }
        Kind::Color { .. } => samples.push(Value::Color(Color::rgba(1, 2, 3, 4))),
        Kind::Text { .. } | Kind::List { .. } => {}
    }
    samples
}

fn stored(setting: &Setting, value: Value) -> BTreeMap<String, Value> {
    BTreeMap::from([(setting.key().to_owned(), value)])
}

fn commands(setting: &Setting, value: &Value, target: Target) -> Vec<SwayCmd> {
    registry()
        .render(setting, &stored(setting, value.clone()), None, target)
        .unwrap_or_else(|error| panic!("{} = {value:?} for {target:?}: {error}", setting.key()))
}

fn lines(setting: &Setting, value: &Value, target: Target) -> Vec<String> {
    commands(setting, value, target)
        .iter()
        .map(|command| {
            command.render(target).unwrap_or_else(|error| {
                panic!("{} = {value:?} for {target:?}: {error}", setting.key())
            })
        })
        .collect()
}

#[test]
fn keys_are_unique_toml_paths() {
    let mut keys = BTreeSet::new();
    for setting in registry().iter() {
        let key = setting.key();
        assert!(keys.insert(key), "duplicate key {key}");
        let segments: Vec<&str> = key.split('.').collect();
        assert!(segments.len() >= 2, "{key} needs a table");
        for segment in segments {
            assert!(
                segment.starts_with(|first: char| first.is_ascii_lowercase())
                    && segment.bytes().all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'),
                "{key}: segments are lowercase words with digits and underscores"
            );
        }
    }
    // A key can't be a value and a table at once.
    for key in &keys {
        let table = format!("{key}.");
        assert!(
            !keys.iter().any(|other| other.starts_with(&table)),
            "{key} is also a table"
        );
    }
}

#[test]
fn texts_are_filled_in() {
    for setting in registry().iter() {
        let key = setting.key();
        assert!(!setting.title().msgid().is_empty(), "{key} has no title");
        if let Some(subtitle) = setting.subtitle() {
            assert!(!subtitle.msgid().is_empty(), "{key}: empty subtitle");
        }
        for keyword in setting.keywords() {
            assert!(!keyword.msgid().is_empty(), "{key}: empty keyword");
        }
        if let Kind::Choice { choices, .. } = setting.kind() {
            let ids: BTreeSet<_> = choices.iter().map(|choice| choice.id).collect();
            assert_eq!(ids.len(), choices.len(), "{key}: duplicate choice");
            for choice in *choices {
                assert!(!choice.id.is_empty(), "{key}: empty choice id");
                assert!(!choice.label.msgid().is_empty(), "{key}: empty label");
            }
        }
    }
}

#[test]
fn defaults_and_samples_fit_their_type() {
    for setting in registry().iter() {
        for sample in samples(setting) {
            assert_eq!(
                setting.check(&sample),
                Ok(()),
                "{} = {sample:?}",
                setting.key()
            );
        }
    }
}

#[test]
fn every_sway_setting_renders_for_config() {
    for (setting, _) in sway_settings() {
        for sample in samples(setting) {
            assert_ne!(
                lines(setting, &sample, Target::Config),
                Vec::<String>::new()
            );
        }
    }
}

#[test]
fn every_live_setting_renders_for_ipc() {
    for (setting, _) in sway_settings() {
        if setting.apply().is_live() {
            for sample in samples(setting) {
                assert_ne!(lines(setting, &sample, Target::Ipc), Vec::<String>::new());
            }
        }
    }
}

#[test]
fn config_only_commands_force_a_reload() {
    let mut found = false;
    for (setting, _) in sway_settings() {
        let config_only = commands(setting, &setting.default_value(), Target::Config)
            .iter()
            .any(|command| command.availability() == Availability::ConfigOnly);
        if config_only {
            found = true;
            assert_eq!(setting.apply(), Apply::Reload, "{}", setting.key());
        }
    }
    assert!(found, "no config-only setting to check");
}

#[test]
fn requirements_match_the_builders() {
    for (setting, mapping) in sway_settings() {
        let requirements = setting.requirements();
        let mut targets = vec![Target::Config];
        if setting.apply().is_live() {
            targets.push(Target::Ipc);
        }
        for target in targets {
            for sample in samples(setting) {
                for command in commands(setting, &sample, target) {
                    assert_eq!(
                        (command.min_sway(), command.min_swayfx()),
                        (requirements.min_sway, requirements.min_swayfx),
                        "{} = {sample:?}",
                        setting.key()
                    );
                }
            }
        }
        // SwayFX directives must stay out of the files vanilla sway reads.
        assert_eq!(
            mapping.file() == ConfigFile::Effects,
            requirements.min_swayfx.is_some(),
            "{}",
            setting.key()
        );
    }
}

#[test]
fn settings_of_one_directive_agree() {
    for (setting, mapping) in sway_settings() {
        let expected = lines(setting, &setting.default_value(), Target::Config);
        for member in registry().group(setting) {
            let member_mapping = member.sway().expect("grouped by directive");
            assert_eq!(member_mapping.file(), mapping.file());
            assert_eq!(member.scope(), setting.scope());
            assert_eq!(member.apply(), setting.apply());
            assert_eq!(member.requirements(), setting.requirements());
            assert_eq!(member.page(), setting.page());
            assert_eq!(
                lines(member, &member.default_value(), Target::Config),
                expected,
                "{} and {} render differently",
                member.key(),
                setting.key()
            );
        }
    }
}

#[test]
fn every_choice_renders_its_own_command() {
    for (setting, _) in sway_settings() {
        if let Kind::Choice { choices, .. } = setting.kind() {
            let rendered: BTreeSet<_> = choices
                .iter()
                .map(|choice| {
                    lines(
                        setting,
                        &Value::Choice(choice.id.to_owned()),
                        Target::Config,
                    )
                })
                .collect();
            assert_eq!(rendered.len(), choices.len(), "{}", setting.key());
        }
    }
}

#[test]
fn every_stored_value_changes_the_commands() {
    for (setting, _) in sway_settings() {
        for sample in samples(setting) {
            if sample != setting.default_value() {
                assert_ne!(
                    lines(setting, &sample, Target::Config),
                    lines(setting, &setting.default_value(), Target::Config),
                    "{} = {sample:?} isn't read by its render function",
                    setting.key()
                );
            }
        }
    }
}

#[test]
fn app_preferences_have_no_sway_mapping() {
    for setting in registry().iter() {
        let is_app = setting.key().starts_with("app.");
        assert_eq!(setting.sway().is_none(), is_app, "{}", setting.key());
        if is_app {
            assert_eq!(setting.page(), Page::About);
            assert_eq!(setting.scope(), Scope::Global);
            assert_eq!(
                registry().render(setting, &Defaults, None, Target::Config),
                Err(RenderError::NotASwaySetting { key: setting.key() })
            );
            assert_eq!(registry().group(setting).count(), 1);
        }
    }
}

#[test]
fn input_settings_are_per_device() {
    let device = InputSelector::device("1267:12377:ELAN1300:00_04F3:3059_Touchpad").unwrap();
    for (setting, mapping) in sway_settings() {
        let per_device = matches!(setting.scope(), Scope::PerDevice(_));
        assert_eq!(
            mapping.directive().block == Block::Input,
            per_device,
            "{}",
            setting.key()
        );
        let rendered = registry().render(setting, &Defaults, Some(&device), Target::Config);
        if per_device {
            for command in rendered.unwrap() {
                let line = command.render(Target::Config).unwrap();
                assert!(
                    line.starts_with("input \"1267:12377:ELAN1300:00_04F3:3059_Touchpad\" "),
                    "{line}"
                );
            }
        } else {
            assert_eq!(rendered, Err(RenderError::NotPerDevice));
        }
    }
}

#[test]
fn first_entries_cover_every_value_type_and_apply_mode() {
    let kinds: BTreeSet<_> = registry()
        .iter()
        .map(|setting| setting.kind().type_name())
        .collect();
    assert_eq!(
        kinds,
        BTreeSet::from([
            "boolean", "choice", "color", "decimal", "integer", "list", "text"
        ])
    );
    for apply in [Apply::Live, Apply::Reload, Apply::Risky] {
        assert!(registry().iter().any(|setting| setting.apply() == apply));
    }
}

#[test]
fn a_type_setting_renders_for_all_devices_of_the_type() {
    let tap = setting("input.touchpad.tap");
    assert_eq!(
        lines(tap, &Value::Bool(true), Target::Ipc),
        ["input type:touchpad tap enabled"]
    );
    assert_eq!(
        lines(
            setting("input.keyboard.xkb_layout"),
            &Value::List(vec!["us".to_owned(), "de".to_owned()]),
            Target::Config
        ),
        ["input type:keyboard xkb_layout \"us,de\""]
    );
}

#[test]
fn a_live_gaps_change_also_reaches_open_workspaces() {
    let gaps = setting("windows.gaps.inner");
    assert_eq!(
        lines(gaps, &Value::Integer(8), Target::Config),
        ["gaps inner 8"]
    );
    assert_eq!(
        lines(gaps, &Value::Integer(8), Target::Ipc),
        ["gaps inner 8", "gaps inner all set 8"]
    );
}

#[test]
fn one_stored_color_renders_with_the_defaults_of_the_others() {
    let accent = setting("windows.focused.child_border");
    assert_eq!(registry().group(accent).count(), 5);
    assert_eq!(
        lines(
            accent,
            &Value::Color(Color::rgb(0x3b, 0x6e, 0x4f)),
            Target::Config
        ),
        ["client.focused #4c7899 #285577 #ffffff #2e9ef4 #3b6e4f"]
    );
    // Any member of the group renders the stored values of all of them.
    let border = setting("windows.focused.border");
    let values = stored(accent, Value::Color(Color::rgb(0x3b, 0x6e, 0x4f)));
    let commands = registry()
        .render(border, &values, None, Target::Config)
        .unwrap();
    assert_eq!(
        commands[0].render(Target::Config).unwrap(),
        "client.focused #4c7899 #285577 #ffffff #2e9ef4 #3b6e4f"
    );
}

#[test]
fn invalid_stored_values_are_reported_with_their_key() {
    let render = |key: &str, value: Value| {
        let setting = setting(key);
        registry().render(setting, &stored(setting, value), None, Target::Config)
    };
    assert_eq!(
        render("effects.corner_radius", Value::Integer(100)),
        Err(RenderError::InvalidValue {
            key: "effects.corner_radius",
            error: ValueError::IntegerOutOfRange { min: 0, max: 99 },
        })
    );
    assert_eq!(
        render("input.touchpad.pointer_accel", Value::Decimal(f64::NAN)),
        Err(RenderError::InvalidValue {
            key: "input.touchpad.pointer_accel",
            error: ValueError::DecimalOutOfRange {
                min: -1.0,
                max: 1.0
            },
        })
    );
    assert_eq!(
        render("input.touchpad.tap", Value::Integer(1)),
        Err(RenderError::InvalidValue {
            key: "input.touchpad.tap",
            error: ValueError::WrongType {
                expected: "boolean",
                found: "integer"
            },
        })
    );
    assert!(matches!(
        render(
            "behavior.focus_follows_mouse",
            Value::Choice("sometimes".to_owned())
        ),
        Err(RenderError::InvalidValue {
            error: ValueError::UnknownChoice { .. },
            ..
        })
    ));
    // What sway accepts in a text is up to the command builder.
    assert!(matches!(
        render("windows.font", Value::Text("pango:monospace 10".to_owned())),
        Err(RenderError::Window(_))
    ));
    assert!(matches!(
        render("input.keyboard.xkb_layout", Value::List(Vec::new())),
        Err(RenderError::Input(_))
    ));
}

#[test]
fn value_errors_read_well() {
    let error = setting("behavior.focus_follows_mouse")
        .check(&Value::Choice("sometimes".to_owned()))
        .unwrap_err();
    assert_eq!(error.to_string(), "must be one of: yes, no, always");
    assert_eq!(
        RenderError::InvalidValue {
            key: "effects.corner_radius",
            error: ValueError::IntegerOutOfRange { min: 0, max: 99 },
        }
        .to_string(),
        "effects.corner_radius: must be between 0 and 99"
    );
}
