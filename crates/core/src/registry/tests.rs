// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{BTreeMap, BTreeSet};

use super::{
    Apply, Block, ConfigFile, Defaults, Kind, Page, ReadError, Registry, RenderError, Scope,
    Setting, SwayMapping, Value, ValueError, When,
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
            // The default needs what the setting declares; another value may need a newer
            // sway (sticky drag lock), which `Compositor::supports` checks per command.
            let default = setting.default_value();
            for sample in samples(setting) {
                for command in commands(setting, &sample, target) {
                    let needs = (command.min_sway(), command.min_swayfx());
                    let declared = (requirements.min_sway, requirements.min_swayfx);
                    if sample == default {
                        assert_eq!(needs, declared, "{} = {sample:?}", setting.key());
                    } else {
                        assert!(
                            needs.0 >= declared.0 && needs.1 >= declared.1,
                            "{} = {sample:?} needs less than its setting: {needs:?}",
                            setting.key()
                        );
                    }
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
fn settings_of_one_directive_form_a_group() {
    for setting in registry().iter() {
        let members: Vec<&str> = registry().group(setting).map(Setting::key).collect();
        if members.len() == 1 {
            assert_eq!(
                setting.group(),
                None,
                "{} is alone in its group",
                setting.key()
            );
            continue;
        }
        let group = setting
            .group()
            .unwrap_or_else(|| panic!("{} shares its directive but has no group", setting.key()));
        let declared: Vec<&str> = registry()
            .iter()
            .filter(|other| other.group() == Some(group))
            .map(Setting::key)
            .collect();
        assert_eq!(declared, members, "group {}", group.id());
        assert!(!group.title().msgid().is_empty(), "group {}", group.id());
    }
}

#[test]
fn members_of_a_group_are_next_to_each_other_in_one_section() {
    let settings: Vec<&Setting> = registry().iter().collect();
    let mut seen = BTreeMap::new();
    for (index, setting) in settings.iter().enumerate() {
        let Some(group) = setting.group() else {
            continue;
        };
        let first = *seen.entry(group.id()).or_insert(index);
        let previous = settings[index - usize::from(index > first)];
        assert_eq!(
            previous.group(),
            Some(group),
            "{} isn't next to the other members of {}",
            setting.key(),
            group.id()
        );
        assert_eq!(previous.page(), setting.page(), "{}", setting.key());
        assert_eq!(previous.section(), setting.section(), "{}", setting.key());
    }
}

#[test]
fn group_ids_are_unique_and_no_keys() {
    let mut groups = BTreeMap::new();
    for setting in registry().iter() {
        let Some(group) = setting.group() else {
            continue;
        };
        let id = group.id();
        assert!(
            registry().get(id).is_none(),
            "group {id} is a setting's key"
        );
        assert!(
            id.split('.').all(|part| !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')),
            "group id {id}"
        );
        assert_eq!(
            *groups.entry(id).or_insert(group),
            group,
            "two groups have the id {id}"
        );
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
fn the_directive_line_is_off_by_default() {
    let setting = setting("app.show_directives");
    assert_eq!(setting.kind(), &Kind::Bool { default: false });
    assert_eq!(setting.section().msgid(), "Sway Things");
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
fn a_setting_cleared_first_is_a_risky_one_of_the_same_scope() {
    for setting in registry().iter() {
        let Some(key) = setting.clears_first() else {
            continue;
        };
        let cleared = registry()
            .get(key)
            .unwrap_or_else(|| panic!("{key} is unknown"));
        assert_eq!(setting.apply(), Apply::Risky, "{}", setting.key());
        assert_eq!(cleared.apply(), Apply::Risky, "{key}");
        assert_eq!(cleared.scope(), setting.scope(), "{key}");
        assert_ne!(cleared.key(), setting.key());
    }
    assert_eq!(
        setting("input.keyboard.xkb_layout").clears_first(),
        Some("input.keyboard.xkb_variant")
    );
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

/// The commands of `key` for these stored values.
fn render_with(key: &str, values: &[(&str, Value)]) -> Result<Vec<String>, RenderError> {
    let values: BTreeMap<String, Value> = values
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect();
    Ok(registry()
        .render(setting(key), &values, None, Target::Config)?
        .iter()
        .map(|command| command.render(Target::Config).unwrap())
        .collect())
}

fn list(items: &[&str]) -> Value {
    Value::List(items.iter().map(|item| (*item).to_owned()).collect())
}

#[test]
fn variants_line_up_with_the_layouts() {
    const LAYOUT: &str = "input.keyboard.xkb_layout";
    const VARIANT: &str = "input.keyboard.xkb_variant";
    let layouts = (LAYOUT, list(&["us", "de", "fr"]));
    let variants =
        |items: &[&str]| render_with(VARIANT, &[layouts.clone(), (VARIANT, list(items))]);
    assert_eq!(
        variants(&["", "nodeadkeys", "bepo"]).unwrap(),
        [r#"input type:keyboard xkb_variant ",nodeadkeys,bepo""#]
    );
    // A shorter list leaves the other layouts as they are.
    assert_eq!(
        variants(&["dvorak"]).unwrap(),
        [r#"input type:keyboard xkb_variant "dvorak,,""#]
    );
    assert_eq!(
        variants(&[]).unwrap(),
        [r#"input type:keyboard xkb_variant ",,""#]
    );
    // With the default of one layout, no variant is an empty one.
    assert_eq!(
        render_with(VARIANT, &[]).unwrap(),
        [r#"input type:keyboard xkb_variant """#]
    );
    // A variant without its layout is refused, with the numbers in the message.
    let error = variants(&["", "nodeadkeys", "bepo", "dvorak"]).unwrap_err();
    assert_eq!(
        error,
        RenderError::TooManyVariants {
            key: VARIANT,
            variants: 4,
            layouts: 3
        }
    );
    assert!(
        error.to_string().contains("4 variants for 3 layouts"),
        "{error}"
    );
    assert!(matches!(
        variants(&["dvorak,colemak"]),
        Err(RenderError::Input(_))
    ));
}

#[test]
fn keyboard_lists_are_read_as_xkb_reads_them() {
    let ok = |key: &str, line: &str| match read(key, line) {
        Some(Ok(value)) => value,
        other => panic!("{key} from {line}: {other:?}"),
    };
    let variant = "input.keyboard.xkb_variant";
    assert_eq!(
        ok(variant, "input * xkb_variant ,nodeadkeys"),
        list(&["", "nodeadkeys"])
    );
    // Empty variants at the end are the padding rendering adds anyway.
    assert_eq!(
        ok(variant, "input * xkb_variant dvorak,,"),
        list(&["dvorak"])
    );
    assert_eq!(ok(variant, r#"input * xkb_variant """#), list(&[]));
    let options = "input.keyboard.xkb_options";
    assert_eq!(
        ok(options, "input * xkb_options ctrl:nocaps,,compose:ralt"),
        list(&["ctrl:nocaps", "compose:ralt"])
    );
    assert_eq!(ok(options, r#"input * xkb_options """#), list(&[]));
    assert_eq!(
        ok("input.keyboard.xkb_model", "input * xkb_model pc104"),
        Value::Text("pc104".to_owned())
    );
    // sway's parse_boolean(…, false): toggle turns the lock on.
    assert_eq!(
        ok("input.keyboard.xkb_numlock", "input * xkb_numlock toggle"),
        Value::Bool(true)
    );
    assert_eq!(
        ok("input.keyboard.xkb_capslock", "input * xkb_capslock off"),
        Value::Bool(false)
    );
    assert_eq!(
        ok("input.keyboard.repeat_rate", "input * repeat_rate 40/s"),
        Value::Integer(40)
    );
    assert!(matches!(
        read("input.keyboard.repeat_rate", "input * repeat_rate -1"),
        Some(Err(ReadError::Rejected(_)))
    ));
    // sway takes 0, which turns repeating off; the setting keeps at least one per second.
    assert!(matches!(
        read("input.keyboard.repeat_rate", "input * repeat_rate 0"),
        Some(Err(ReadError::Unsupported(_)))
    ));
}

#[test]
fn keyboard_settings_apply_as_their_effect_needs() {
    let apply = |key: &str| setting(key).apply();
    // What the keys produce: tried under the safety net.
    for key in [
        "input.keyboard.xkb_layout",
        "input.keyboard.xkb_variant",
        "input.keyboard.xkb_options",
    ] {
        assert_eq!(apply(key), Apply::Risky, "{key}");
    }
    for key in [
        "input.keyboard.xkb_model",
        "input.keyboard.repeat_delay",
        "input.keyboard.repeat_rate",
    ] {
        assert_eq!(apply(key), Apply::Live, "{key}");
    }
    // sway only reads the lock states from its config.
    for key in ["input.keyboard.xkb_numlock", "input.keyboard.xkb_capslock"] {
        assert_eq!(apply(key), Apply::Reload, "{key}");
    }
    assert_eq!(
        setting("input.keyboard.repeat_rate").check(&Value::Integer(0)),
        Err(ValueError::IntegerOutOfRange {
            min: 1,
            max: i64::from(u16::MAX)
        })
    );
    // Below 100 ms every key repeats almost at once.
    assert_eq!(
        setting("input.keyboard.repeat_delay").check(&Value::Integer(99)),
        Err(ValueError::IntegerOutOfRange {
            min: 100,
            max: i64::from(u16::MAX)
        })
    );
    assert_eq!(
        setting("input.keyboard.repeat_delay").check(&Value::Integer(100)),
        Ok(())
    );
    let rate = setting("input.keyboard.repeat_rate").slider().unwrap();
    assert_eq!((rate.min, rate.max, rate.step), (1.0, 100.0, 1.0));
}

#[test]
fn search_terms_find_the_keyboard_settings() {
    let index = super::SearchIndex::new(registry(), super::Msg::msgid);
    let finds = |query: &str| -> Vec<&'static str> {
        index
            .search(query)
            .iter()
            .filter_map(|hit| hit.setting().map(Setting::key))
            .collect()
    };
    for (query, key) in [
        ("caps lock", "input.keyboard.xkb_options"),
        ("compose", "input.keyboard.xkb_options"),
        ("dvorak", "input.keyboard.xkb_variant"),
        ("colemak", "input.keyboard.xkb_variant"),
        ("language", "input.keyboard.xkb_layout"),
        ("model", "input.keyboard.xkb_model"),
        ("key repeat", "input.keyboard.repeat_rate"),
        ("key repeat", "input.keyboard.repeat_delay"),
        ("numlock", "input.keyboard.xkb_numlock"),
        ("num lock", "input.keyboard.xkb_numlock"),
        ("caps lock", "input.keyboard.xkb_capslock"),
    ] {
        assert!(finds(query).contains(&key), "{query} doesn't find {key}");
    }
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
    let group = accent.group().unwrap();
    assert_eq!(
        (group.id(), group.title().msgid()),
        ("windows.focused", "Focused window")
    );
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

/// The directives of a config with these lines, as the parser reads them.
fn parsed(text: &str) -> Vec<crate::sway_config::Directive> {
    let files = crate::sway_config::MemoryFiles::new().file("/config", text);
    crate::sway_config::parse(std::path::Path::new("/config"), &files).directives
}

/// What `key` reads from the only directive of `line`.
fn read(key: &str, line: &str) -> Option<Result<Value, ReadError>> {
    let directives = parsed(line);
    assert_eq!(directives.len(), 1, "{line}");
    setting(key).read(&directives[0])
}

#[test]
fn every_setting_parses_back_the_values_it_shows() {
    for setting in registry().iter() {
        for value in samples(setting) {
            let text = value.to_string();
            assert_eq!(
                setting.kind().parse(&text).as_ref(),
                Ok(&value),
                "{} = {text}",
                setting.key()
            );
        }
    }
}

#[test]
fn values_are_parsed_as_typed_on_the_command_line() {
    let parse = |key: &str, text: &str| setting(key).kind().parse(text);
    assert_eq!(
        parse("input.keyboard.xkb_layout", "us, de,,"),
        Ok(Value::List(vec!["us".to_owned(), "de".to_owned()]))
    );
    assert_eq!(
        parse("input.keyboard.xkb_layout", ""),
        Ok(Value::List(Vec::new()))
    );
    // Empty items before others stay: a variant lines up with its layout.
    assert_eq!(
        parse("input.keyboard.xkb_variant", ", nodeadkeys,"),
        Ok(Value::List(vec![String::new(), "nodeadkeys".to_owned()]))
    );
    assert_eq!(
        parse("windows.focused.border", "#FFaa00"),
        Ok(Value::Color(Color::rgb(0xff, 0xaa, 0)))
    );
    assert_eq!(
        parse("windows.font", "pango:Sans, Bold 11"),
        Ok(Value::Text("pango:Sans, Bold 11".to_owned()))
    );

    let error = |key: &str, text: &str| parse(key, text).unwrap_err().to_string();
    assert_eq!(error("input.touchpad.tap", "yes"), "must be true or false");
    assert_eq!(error("windows.gaps.inner", "8.5"), "must be a whole number");
    assert_eq!(
        error("windows.gaps.inner", "-1"),
        "must be between 0 and 32767"
    );
    assert_eq!(
        error("input.touchpad.pointer_accel", "fast"),
        "must be a number"
    );
    assert_eq!(
        error("input.touchpad.pointer_accel", "NaN"),
        "must be between -1 and 1"
    );
    assert_eq!(
        error("windows.focused.border", "red"),
        "must be a color like #rrggbb or #rrggbbaa"
    );
    assert!(error("behavior.focus_follows_mouse", "maybe").starts_with("must be one of: "));
}

#[test]
fn every_sway_setting_reads_back_what_it_renders() {
    for (setting, _) in sway_settings() {
        let mut samples = samples(setting);
        if let Kind::List { .. } = setting.kind() {
            samples.push(Value::List(vec!["us".to_owned(), "de".to_owned()]));
        }
        for sample in samples {
            // Two layouts, so that two variants fit.
            let mut values = BTreeMap::from([(
                "input.keyboard.xkb_layout".to_owned(),
                Value::List(vec!["us".to_owned(), "de".to_owned()]),
            )]);
            values.insert(setting.key().to_owned(), sample.clone());
            let text = registry()
                .render(setting, &values, None, Target::Config)
                .unwrap()
                .iter()
                .map(|command| command.render(Target::Config).unwrap())
                .collect::<Vec<_>>()
                .join("\n");
            let read: Vec<_> = parsed(&text)
                .iter()
                .filter_map(|directive| setting.read(directive))
                .collect();
            assert_eq!(read, [Ok(sample.clone())], "{}: {text}", setting.key());
        }
    }
}

#[test]
fn values_are_read_as_sway_reads_them() {
    let ok = |key: &str, line: &str| match read(key, line) {
        Some(Ok(value)) => value,
        other => panic!("{key} from {line}: {other:?}"),
    };
    let color = |red, green, blue| Value::Color(Color::rgb(red, green, blue));
    // sway's words for true, and everything else is false.
    assert_eq!(
        ok("input.touchpad.tap", "input * tap On"),
        Value::Bool(true)
    );
    assert_eq!(
        ok("input.touchpad.tap", "input type:touchpad tap toggle"),
        Value::Bool(false)
    );
    assert_eq!(
        ok(
            "input.touchpad.tap",
            "input type:touchpad {\n  tap maybe\n}"
        ),
        Value::Bool(false)
    );
    // Without indicator and child border, sway takes its default and the background.
    let line = "client.focused #111111 #222222 #333333";
    assert_eq!(
        ok("windows.focused.indicator", line),
        color(0x2e, 0x9e, 0xf4)
    );
    assert_eq!(
        ok("windows.focused.child_border", line),
        color(0x22, 0x22, 0x22)
    );
    assert_eq!(
        ok(
            "windows.focused.text",
            "client.focused 111111 222222 333333ff"
        ),
        color(0x33, 0x33, 0x33)
    );
    assert_eq!(
        ok("windows.gaps.inner", "gaps INNER 8px"),
        Value::Integer(8)
    );
    assert_eq!(ok("windows.gaps.inner", "gaps inner -4"), Value::Integer(0));
    assert_eq!(
        ok("windows.font", "font pango:Sans Bold 11"),
        Value::Text("Sans Bold 11".to_owned())
    );
    assert_eq!(
        ok("input.keyboard.repeat_delay", "input * repeat_delay 250ms"),
        Value::Integer(250)
    );
    assert_eq!(
        ok("behavior.workspace_layout", "Workspace_Layout TABBED"),
        Value::Choice("tabbed".to_owned())
    );
    assert_eq!(
        ok(
            "input.keyboard.xkb_layout",
            "input type:keyboard xkb_layout \"us,de\""
        ),
        Value::List(vec!["us".to_owned(), "de".to_owned()])
    );
}

#[test]
fn window_and_effect_lines_are_read_as_sway_and_swayfx_read_them() {
    let ok = |key: &str, line: &str| match read(key, line) {
        Some(Ok(value)) => value,
        other => panic!("{key} from {line}: {other:?}"),
    };
    let color = |red, green, blue| Value::Color(Color::rgb(red, green, blue));
    // A border line without a width leaves sway's default; `atoi` reads the start of one.
    assert_eq!(
        ok("windows.border.width", "default_border pixel"),
        Value::Integer(2)
    );
    assert_eq!(
        ok(
            "windows.floating_border.width",
            "default_floating_border normal 3px"
        ),
        Value::Integer(3)
    );
    // `smart` hides no edges: it turns on smart borders, a setting of its own.
    assert_eq!(
        ok("windows.hide_edge_borders", "hide_edge_borders --i3 smart"),
        Value::Choice("none".to_owned())
    );
    assert_eq!(
        ok("windows.hide_lone_tab", "hide_edge_borders --i3 both"),
        Value::Bool(true)
    );
    assert_eq!(
        ok("windows.smart_borders", "smart_borders enable"),
        Value::Choice("on".to_owned())
    );
    assert_eq!(
        ok("windows.smart_gaps", "smart_gaps inverse_outer"),
        Value::Choice("inverse_outer".to_owned())
    );
    assert_eq!(
        ok("windows.gaps.outer", "gaps outer -4"),
        Value::Integer(-4)
    );
    // One padding is both.
    assert_eq!(
        ok("windows.titlebar.padding_vertical", "titlebar_padding 6"),
        Value::Integer(6)
    );
    // Unset, the focused tab title is drawn like an inactive window; a line takes 3 to 5 colors.
    assert_eq!(
        ok(
            "windows.focused_tab_title.text",
            "client.focused_tab_title #111111 #222222 #333333 #444444"
        ),
        color(0x33, 0x33, 0x33)
    );
    assert_eq!(
        ok(
            "windows.urgent.indicator",
            "client.urgent #111111 #222222 #333333"
        ),
        color(0x90, 0, 0)
    );
    assert_eq!(
        ok("effects.shadow_offset_y", "shadow_offset 2 -4.5"),
        Value::Decimal(-4.5)
    );
    assert_eq!(ok("effects.blur", "blur on"), Value::Bool(true));
    // SwayFX reads the second argument of several.
    assert_eq!(
        ok(
            "effects.animation_duration_ms",
            "animation_duration_ms ignored 250"
        ),
        Value::Integer(250)
    );
}

#[test]
fn lines_sway_rejects_or_the_setting_cant_hold_are_errors() {
    let rejected = |key: &str, line: &str| {
        assert!(
            matches!(read(key, line), Some(Err(ReadError::Rejected(_)))),
            "{key}: {line}"
        );
    };
    rejected("behavior.focus_follows_mouse", "focus_follows_mouse Yes");
    rejected(
        "behavior.workspace_layout",
        "workspace_layout tabbed stacking",
    );
    rejected("windows.focused.border", "client.focused #111111 #222222");
    rejected(
        "windows.focused.border",
        "client.focused #111111 #222222 #333333 #444444 #555555 #666666",
    );
    rejected(
        "windows.focused.border",
        "client.focused #111111 #222222 red",
    );
    rejected("windows.gaps.inner", "gaps inner 8em");
    rejected("input.touchpad.pointer_accel", "input * pointer_accel 1.5");
    rejected("input.touchpad.pointer_accel", "input * pointer_accel fast");
    rejected("input.keyboard.repeat_delay", "input * repeat_delay -1");
    rejected("effects.corner_radius", "corner_radius 100");
    rejected("windows.border.style", "default_border Pixel 2");
    rejected("windows.hide_edge_borders", "hide_edge_borders --i3");
    rejected("windows.title_align", "title_align middle");
    rejected(
        "windows.titlebar.padding_horizontal",
        "titlebar_padding -1 4",
    );
    rejected(
        "windows.titlebar.border_thickness",
        "titlebar_border_thickness 2px",
    );
    rejected("effects.blur_passes", "blur_passes 11");
    rejected("effects.blur_noise", "blur_noise 1.5");
    rejected("effects.shadow_offset_x", "shadow_offset 100 0");
    rejected("effects.shadow_offset_x", "shadow_offset 1");
    rejected(
        "effects.animation_duration_ms",
        "animation_duration_ms 6000",
    );
    rejected("input.touchpad.tap", "input * tap");
    // sway accepts it, but the setting's range is narrower.
    assert!(matches!(
        read("input.keyboard.repeat_delay", "input * repeat_delay 70000"),
        Some(Err(ReadError::Unsupported(_)))
    ));
    assert!(matches!(
        read("input.keyboard.repeat_delay", "input * repeat_delay 50"),
        Some(Err(ReadError::Unsupported(_)))
    ));
}

#[test]
fn only_the_settings_own_directive_matches() {
    assert_eq!(read("windows.gaps.inner", "gaps outer 8"), None);
    assert_eq!(read("windows.gaps.inner", "gaps"), None);
    assert!(read("windows.gaps.inner", "gaps INNER 8").is_some());
    assert_eq!(read("input.touchpad.tap", "tap enabled"), None);
    assert_eq!(read("input.touchpad.tap", "bar {\n  tap enabled\n}"), None);
    assert_eq!(read("windows.font", "input * font monospace"), None);
    assert_eq!(read("app.safety_net_timeout", "gaps inner 8"), None);
}

#[test]
fn dependencies_name_an_earlier_setting_of_the_page_with_values_it_can_take() {
    let mut earlier: Vec<&Setting> = Vec::new();
    for setting in registry().iter() {
        if let Some(dependency) = setting.dependency() {
            let other = earlier
                .iter()
                .find(|other| other.key() == dependency.key)
                .unwrap_or_else(|| {
                    panic!(
                        "{} depends on {}, which isn't an earlier setting",
                        setting.key(),
                        dependency.key
                    )
                });
            assert_eq!(other.page(), setting.page(), "{}", setting.key());
            let fits = match (dependency.when, other.kind()) {
                (When::On, Kind::Bool { .. })
                | (When::Above(_), Kind::Integer { .. } | Kind::Decimal { .. }) => true,
                (When::OneOf(ids), Kind::Choice { choices, .. }) => {
                    !ids.is_empty()
                        && ids
                            .iter()
                            .all(|id| choices.iter().any(|choice| choice.id == *id))
                }
                _ => false,
            };
            assert!(
                fits,
                "{}: {dependency:?} doesn't fit {}",
                setting.key(),
                other.key()
            );
        }
        earlier.push(setting);
    }
}

#[test]
fn a_dependency_holds_for_the_values_it_names() {
    assert!(When::On.holds(&Value::Bool(true)));
    assert!(!When::On.holds(&Value::Bool(false)));
    assert!(When::OneOf(&["pixel"]).holds(&Value::Choice("pixel".to_owned())));
    assert!(!When::OneOf(&["pixel"]).holds(&Value::Choice("none".to_owned())));
    assert!(When::Above(0.0).holds(&Value::Integer(1)));
    assert!(!When::Above(0.0).holds(&Value::Decimal(0.0)));
    // A value of another type never meets it.
    assert!(!When::On.holds(&Value::Integer(1)));
}

#[test]
fn every_setting_has_a_section_and_its_settings_are_together() {
    let mut seen = BTreeSet::new();
    let mut previous = None;
    for setting in registry().iter() {
        let section = (setting.page().id(), setting.section().msgid());
        assert!(!section.1.is_empty(), "{} has no section", setting.key());
        if previous != Some(section) {
            assert!(
                seen.insert(section),
                "{}: section {section:?} is split by another one",
                setting.key()
            );
        }
        previous = Some(section);
    }
}

#[test]
fn sliders_fit_the_values_their_setting_accepts() {
    for setting in registry().iter() {
        let slider = setting.slider();
        let range = match setting.kind() {
            #[expect(clippy::cast_precision_loss, reason = "test ranges are small")]
            Kind::Integer { min, max, .. } => Some((*min as f64, *max as f64)),
            Kind::Decimal { min, max, .. } => Some((*min, *max)),
            _ => None,
        };
        let key = setting.key();
        match (slider, range) {
            (None, None) => {}
            (Some(slider), Some((min, max))) => {
                assert!(min <= slider.min && slider.max <= max, "{key}: {slider:?}");
                assert!(
                    slider.min < slider.max && slider.step > 0.0,
                    "{key}: {slider:?}"
                );
                let steps = (slider.max - slider.min) / slider.step;
                assert!((steps - steps.round()).abs() < 1e-9, "{key}: {slider:?}");
                if matches!(setting.kind(), Kind::Integer { .. }) {
                    assert_eq!(slider.step.fract(), 0.0, "{key}: {slider:?}");
                }
            }
            (slider, _) => panic!("{key}: slider {slider:?} for {:?}", setting.kind()),
        }
    }
    let delay = setting("input.keyboard.repeat_delay").slider().unwrap();
    assert_eq!((delay.min, delay.max), (100.0, 2000.0));
    let radius = setting("effects.corner_radius").slider().unwrap();
    assert_eq!((radius.min, radius.max, radius.step), (0.0, 99.0, 1.0));
}

#[test]
fn the_config_line_shows_the_option_without_the_input_selector() {
    let line = |key: &str, value: Value, device: Option<&InputSelector>| {
        let setting = setting(key);
        registry()
            .config_line(setting, &stored(setting, value), device)
            .unwrap()
    };
    assert_eq!(
        line("input.touchpad.tap", Value::Bool(true), None),
        "tap enabled"
    );
    let device = InputSelector::device("1133:16500:Logitech_G305;quoted").unwrap();
    assert_eq!(
        line(
            "input.touchpad.pointer_accel",
            Value::Decimal(0.3),
            Some(&device)
        ),
        "pointer_accel 0.3"
    );
    assert_eq!(
        line("windows.gaps.inner", Value::Integer(8), None),
        "gaps inner 8"
    );
    assert_eq!(
        line(
            "windows.focused.text",
            Value::Color(Color::rgb(0, 0, 0)),
            None
        ),
        "client.focused #4c7899 #285577 #000000 #2e9ef4 #285577"
    );
    assert!(matches!(
        registry().config_line(setting("app.safety_net_timeout"), &Defaults, None),
        Err(RenderError::NotASwaySetting { .. })
    ));
}
