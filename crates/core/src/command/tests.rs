// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;
use regex::Regex;

use super::model::{self, Bytes, Parsed};
use super::{
    BoolStyle, Color, CommandError, Criteria, CriteriaKey, SwayCmd, SwayText, Target, TextError,
};
use crate::version::SwayFxVersion;

fn text(value: &str) -> SwayText {
    SwayText::new(value).unwrap()
}

fn strings(argv: &[Bytes]) -> Vec<String> {
    argv.iter()
        .map(|arg| String::from_utf8(arg.clone()).unwrap())
        .collect()
}

/// The arguments sway's handler receives for a rendered IPC command, which must stay one command.
fn ipc_argv(payload: &str) -> Result<Vec<String>, String> {
    match model::execute_command(payload.as_bytes())?.as_slice() {
        [
            Parsed {
                criteria: None,
                argv,
            },
        ] => Ok(strings(argv)),
        commands => Err(format!("expected one command, got {commands:?}")),
    }
}

fn config_argv(line: &str) -> Result<Vec<String>, String> {
    model::config_command(line.as_bytes()).map(|argv| strings(&argv))
}

fn argv_for(target: Target, rendered: &str) -> Result<Vec<String>, String> {
    match target {
        Target::Ipc => ipc_argv(rendered),
        Target::Config => config_argv(rendered),
    }
}

// --- The model reproduces sway's behavior ---

#[test]
fn model_splits_commands_on_unquoted_separators() {
    let commands =
        model::execute_command(br#"seat a hide_cursor 1; seat "b;c" hide_cursor 2, nop 'd,e'"#)
            .unwrap();
    let argvs: Vec<Vec<String>> = commands
        .iter()
        .map(|command| strings(&command.argv))
        .collect();
    assert_eq!(
        argvs,
        [
            vec!["seat", "a", "hide_cursor", "1"],
            vec!["seat", "b;c", "hide_cursor", "2"],
            vec!["nop", "d,e"]
        ]
    );
}

#[test]
fn model_keeps_backslashes_over_ipc_and_unescapes_them_in_config() {
    assert_eq!(
        ipc_argv(r#"seat "a\"b" x"#).unwrap(),
        [r"seat", r#"a\"b"#, "x"]
    );
    assert_eq!(
        config_argv(r#"seat "a\"b" x"#).unwrap(),
        ["seat", r#"a"b"#, "x"]
    );
}

#[test]
fn model_replaces_variables() {
    assert_eq!(ipc_argv("seat a$$b x").unwrap(), ["seat", "a$b", "x"]);
    assert_eq!(config_argv("seat a$$b x").unwrap(), ["seat", "a$b", "x"]);
    assert!(ipc_argv("seat $mod x").is_err());
    assert!(config_argv("seat $mod x").is_err());
    assert_eq!(ipc_argv(r"seat a\$b x").unwrap(), ["seat", r"a\$b", "x"]);
}

#[test]
fn model_shows_why_text_rejects_backslashes() {
    // argsep keeps its escape flag across `\'`, so the closing quote doesn't close and `;` in the
    // next argument splits.
    let commands = model::execute_command(br#"seat "\'" x "a;b""#).unwrap();
    assert_eq!(commands.len(), 2);
}

#[test]
fn model_parses_criteria() {
    let (pairs, len) = model::criteria_parse(br#"[app_id="^a\"b\z" title=x] rest"#).unwrap();
    assert_eq!(
        pairs,
        [
            (b"app_id".to_vec(), br#"^a"b\z"#.to_vec()),
            (b"title".to_vec(), b"x".to_vec())
        ]
    );
    assert_eq!(len, 26);
}

#[test]
fn model_parses_criteria_tokens_like_sway() {
    let (pairs, _) = model::criteria_parse(b"[floating app_id=x]").unwrap();
    assert_eq!(
        pairs,
        [
            (b"floating".to_vec(), Vec::new()),
            (b"app_id".to_vec(), b"x".to_vec())
        ]
    );
    assert!(model::criteria_parse(b"[title]").is_err());
    assert!(model::criteria_parse(b"[tag=x]").is_err());
}

#[test]
fn model_rejects_lines_sway_would_not_read_as_one_command() {
    for line in [
        "# comment",
        "input foo {",
        "}",
        "a\nb",
        "seat x \\",
        "$mod x",
    ] {
        assert!(config_argv(line).is_err(), "{line:?}");
    }
}

// --- SwayText ---

#[test]
fn text_rejects_what_cannot_be_rendered() {
    assert_eq!(SwayText::new(""), Err(TextError::Empty));
    assert_eq!(
        SwayText::new("a\nb"),
        Err(TextError::ControlCharacter {
            index: 1,
            character: '\n'
        })
    );
    assert_eq!(
        SwayText::new("ab\u{7f}"),
        Err(TextError::ControlCharacter {
            index: 2,
            character: '\u{7f}'
        })
    );
    assert_eq!(SwayText::new("ä\\"), Err(TextError::Backslash { index: 2 }));
    assert_eq!(text("DP-1 ä ✓").as_str(), "DP-1 ä ✓");
}

#[test]
fn text_renders_for_ipc() {
    let cases = [
        ("plain", r#""plain""#),
        ("with space", r#""with space""#),
        (r#"say "hi""#, r#"'say "hi"'"#),
        ("it's", r#""it's""#),
        (r#"a"b'c"#, r#"'a"b'"'c""#),
        ("$mod;x", r#""$$mod;x""#),
    ];
    for (input, expected) in cases {
        assert_eq!(text(input).render_ipc(), expected, "{input:?}");
    }
}

#[test]
fn text_renders_for_config() {
    let cases = [
        ("plain", r#""plain""#),
        (r#"say "hi""#, r#""say \"hi\"""#),
        ("it's", r#""it's""#),
        ("$mod # x", r#""$$mod # x""#),
    ];
    for (input, expected) in cases {
        assert_eq!(text(input).render_config(), expected, "{input:?}");
    }
}

// --- Color ---

#[test]
fn color_parses_and_renders() {
    assert_eq!("#285577".parse::<Color>(), Ok(Color::rgb(0x28, 0x55, 0x77)));
    assert_eq!(
        "#4C7899AA".parse::<Color>(),
        Ok(Color::rgba(0x4c, 0x78, 0x99, 0xaa))
    );
    assert_eq!(Color::rgba(0x4c, 0x78, 0x99, 0xaa).to_string(), "#4c7899aa");
    for invalid in ["285577", "#28557", "#2855777", "#28557g", "#+85577", "#ÿÿÿ"] {
        assert!(invalid.parse::<Color>().is_err(), "{invalid:?}");
    }
}

// --- SwayCmd ---

#[test]
fn command_renders_typed_parts() {
    let cmd = SwayCmd::new("client.focused")
        .color(Color::rgb(0x4c, 0x78, 0x99))
        .color(Color::rgba(0x28, 0x55, 0x77, 0xff))
        .keyword("type:touchpad")
        .int(-3)
        .float(0.25)
        .bool(false, BoolStyle::YesNo)
        .bool(true, BoolStyle::EnabledDisabled)
        .bool(true, BoolStyle::EnableDisable)
        .bool(false, BoolStyle::OnOff);
    let expected = "client.focused #4c7899 #285577ff type:touchpad -3 0.25 no enabled enable off";
    assert_eq!(cmd.render(Target::Ipc).unwrap(), expected);
    assert_eq!(cmd.render(Target::Config).unwrap(), expected);
}

#[test]
fn command_renders_text_per_target() {
    let cmd = SwayCmd::new("output")
        .text(text(r#"Dell "U2415" $1"#))
        .keyword("disable");
    assert_eq!(
        cmd.render(Target::Ipc).unwrap(),
        r#"output 'Dell "U2415" $$1' disable"#
    );
    assert_eq!(
        cmd.render(Target::Config).unwrap(),
        r#"output "Dell \"U2415\" $$1" disable"#
    );
}

#[test]
fn command_rejects_invalid_parts() {
    let render = |cmd: SwayCmd| cmd.render(Target::Config);
    assert_eq!(
        render(SwayCmd::new("seat").keyword("a;b")),
        Err(CommandError::InvalidKeyword { keyword: "a;b" })
    );
    assert_eq!(
        render(SwayCmd::new("")),
        Err(CommandError::InvalidKeyword { keyword: "" })
    );
    assert_eq!(
        render(SwayCmd::new("gaps").keyword("{")),
        Err(CommandError::InvalidKeyword { keyword: "{" })
    );
    assert_eq!(
        render(SwayCmd::new("opacity").float(f64::NAN)),
        Err(CommandError::NotFinite)
    );
    assert_eq!(
        render(SwayCmd::new("opacity").float(f64::INFINITY)),
        Err(CommandError::NotFinite)
    );
    assert_eq!(
        render(SwayCmd::new("exec").text(text("x"))),
        Err(CommandError::TextKeepsQuotes { command: "exec" })
    );
}

#[test]
fn command_respects_availability() {
    let config_only = SwayCmd::new("xwayland").keyword("disable").config_only();
    assert_eq!(
        config_only.render(Target::Ipc),
        Err(CommandError::Unavailable {
            target: Target::Ipc
        })
    );
    assert_eq!(
        config_only.render(Target::Config).unwrap(),
        "xwayland disable"
    );
    let runtime_only = SwayCmd::new("input")
        .keyword("*")
        .keyword("xkb_switch_layout")
        .keyword("next")
        .runtime_only();
    assert_eq!(
        runtime_only.render(Target::Config),
        Err(CommandError::Unavailable {
            target: Target::Config
        })
    );
    assert_eq!(
        runtime_only.render(Target::Ipc).unwrap(),
        "input * xkb_switch_layout next"
    );
}

#[test]
fn an_empty_text_arrives_as_an_empty_argument() {
    let cmd = SwayCmd::new("input")
        .keyword("type:keyboard")
        .keyword("xkb_variant")
        .empty_text();
    for target in [Target::Ipc, Target::Config] {
        let rendered = cmd.render(target).unwrap();
        assert_eq!(rendered, r#"input type:keyboard xkb_variant """#);
        assert_eq!(
            argv_for(target, &rendered).unwrap(),
            ["input", "type:keyboard", "xkb_variant", ""]
        );
    }
    let criteria = Criteria::new().exact(CriteriaKey::AppId, "x");
    let wrapped = SwayCmd::for_window(criteria, SwayCmd::new("title_format").empty_text()).unwrap();
    for target in [Target::Ipc, Target::Config] {
        let (_, inner) = for_window_parts(target, &wrapped.render(target).unwrap());
        assert_eq!(inner, ["title_format", ""]);
    }
    assert_eq!(
        SwayCmd::new("exec").empty_text().render(Target::Ipc),
        Err(CommandError::TextKeepsQuotes { command: "exec" })
    );
}

#[test]
fn for_window_renders_criteria_per_target() {
    let criteria = Criteria::new()
        .exact(CriteriaKey::AppId, "org.gnome.Nautilus")
        .exact(CriteriaKey::Title, "a\"b");
    let cmd = SwayCmd::for_window(criteria, SwayCmd::new("opacity").float(0.9)).unwrap();
    let ipc = r#"[app_id="^org\x{2e}gnome\x{2e}Nautilus\z" title="^a\x{22}b\z"]"#;
    let config = r#"[app_id="^org\\x{2e}gnome\\x{2e}Nautilus\\z" title="^a\\x{22}b\\z"]"#;
    assert_eq!(
        cmd.render(Target::Ipc).unwrap(),
        format!("for_window {ipc} opacity 0.9")
    );
    assert_eq!(
        cmd.render(Target::Config).unwrap(),
        format!("for_window {config} opacity 0.9")
    );
}

#[test]
fn for_window_quotes_inner_text_twice() {
    let criteria = Criteria::new().exact(CriteriaKey::AppId, "x");
    let inner = SwayCmd::new("title_format").text(text(r#"say "hi" $1"#));
    let cmd = SwayCmd::for_window(criteria, inner).unwrap();
    assert_eq!(
        cmd.render(Target::Ipc).unwrap(),
        r#"for_window [app_id="^x\z"] title_format "'say "'"hi" $$$$1'"'""#
    );
    assert_eq!(
        cmd.render(Target::Config).unwrap(),
        r#"for_window [app_id="^x\\z"] title_format 'say "hi" $$$$1'"#
    );
}

#[test]
fn for_window_rejects_what_sway_would_parse_again() {
    let criteria = || Criteria::new().exact(CriteriaKey::AppId, "x");
    let nested = SwayCmd::for_window(criteria(), SwayCmd::new("opacity").float(0.5)).unwrap();
    assert_eq!(
        SwayCmd::for_window(criteria(), nested),
        Err(CommandError::NestedCriteria)
    );
    let keeps_quotes = SwayCmd::new("exec").text(text("x"));
    let cmd = SwayCmd::for_window(criteria(), keeps_quotes).unwrap();
    assert_eq!(
        cmd.render(Target::Config),
        Err(CommandError::TextKeepsQuotes { command: "exec" })
    );
    let config_only = SwayCmd::new("xwayland").keyword("disable").config_only();
    assert_eq!(
        SwayCmd::for_window(criteria(), config_only),
        Err(CommandError::Unavailable {
            target: Target::Ipc
        })
    );
    let empty = SwayCmd::for_window(Criteria::new(), SwayCmd::new("opacity").float(0.5)).unwrap();
    assert_eq!(empty.render(Target::Ipc), Err(CommandError::EmptyCriteria));
}

#[test]
fn leading_criteria_render_over_ipc_only() {
    let criteria = Criteria::new().exact(CriteriaKey::AppId, "org.gnome.Nautilus");
    let cmd = SwayCmd::with_criteria(criteria, SwayCmd::new("opacity").float(0.8)).unwrap();
    assert_eq!(
        cmd.render(Target::Ipc).unwrap(),
        r#"[app_id="^org\x{2e}gnome\x{2e}Nautilus\z"] opacity 0.8"#
    );
    assert_eq!(
        cmd.render(Target::Config),
        Err(CommandError::Unavailable {
            target: Target::Config
        })
    );
    let criteria = || Criteria::new().exact(CriteriaKey::AppId, "x");
    let twice = SwayCmd::with_criteria(criteria(), cmd.clone());
    assert_eq!(twice, Err(CommandError::NestedCriteria));
    assert_eq!(
        SwayCmd::for_window(criteria(), cmd),
        Err(CommandError::NestedCriteria)
    );
    let empty = SwayCmd::with_criteria(Criteria::new(), SwayCmd::new("kill")).unwrap();
    assert_eq!(empty.render(Target::Ipc), Err(CommandError::EmptyCriteria));
}

// --- Round trips through the model ---

const SPECIAL_CHARS: &[char] = &[
    '"', '\'', '$', ';', ',', '[', ']', '{', '}', '#', ' ', '=', '*', '-', 'a', '0', 'ä',
];

fn hostile_char() -> impl Strategy<Value = char> {
    prop_oneof![
        3 => prop::sample::select(SPECIAL_CHARS),
        1 => any::<char>(),
    ]
}

fn sway_text() -> impl Strategy<Value = SwayText> {
    prop::collection::vec(
        hostile_char().prop_filter("renderable", |c| !c.is_control() && *c != '\\'),
        1..24,
    )
    .prop_map(|chars| SwayText::new(chars.into_iter().collect::<String>()).unwrap())
}

fn any_value() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![3 => hostile_char(), 1 => prop::sample::select(vec!['\\', '\n', '\t', '\0'])],
        0..24,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn target() -> impl Strategy<Value = Target> {
    prop_oneof![Just(Target::Ipc), Just(Target::Config)]
}

/// The criteria and inner command of a rendered `for_window`, as sway sees them when a window
/// appears.
fn for_window_parts(target: Target, rendered: &str) -> (Vec<(Bytes, Bytes)>, Vec<String>) {
    let argv = argv_for(target, rendered).unwrap();
    assert_eq!(argv[0], "for_window");
    let (pairs, len) = model::criteria_parse(argv[1].as_bytes()).unwrap();
    assert_eq!(len, argv[1].len(), "criteria end early in {:?}", argv[1]);
    let inner = argv[2..].join(" ");
    (pairs, ipc_argv(&inner).unwrap())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn text_arrives_unchanged(target in target(), first in sway_text(), second in sway_text()) {
        let cmd = SwayCmd::new("workspace")
            .text(first.clone())
            .keyword("output")
            .text(second.clone());
        let rendered = cmd.render(target).unwrap();
        let argv = argv_for(target, &rendered).map_err(TestCaseError::fail)?;
        prop_assert_eq!(argv, ["workspace", first.as_str(), "output", second.as_str()]);
    }

    #[test]
    fn criteria_match_exactly(target in target(), app_id in any_value(), title in any_value()) {
        let criteria = Criteria::new()
            .exact(CriteriaKey::AppId, app_id.clone())
            .exact(CriteriaKey::Title, title.clone());
        let cmd = SwayCmd::for_window(criteria, SwayCmd::new("opacity").float(0.5)).unwrap();
        let (pairs, inner) = for_window_parts(target, &cmd.render(target).unwrap());
        prop_assert_eq!(inner, ["opacity", "0.5"]);
        prop_assert_eq!(pairs.len(), 2);
        let expected = [("app_id", &app_id), ("title", &title)];
        for ((name, pattern), (expected_name, value)) in pairs.iter().zip(expected) {
            prop_assert_eq!(name.as_slice(), expected_name.as_bytes());
            let regex = Regex::new(std::str::from_utf8(pattern).unwrap()).unwrap();
            prop_assert!(regex.is_match(value));
            let (longer, prefixed) = (format!("{value}x"), format!("x{value}"));
            prop_assert!(!regex.is_match(&longer));
            prop_assert!(!regex.is_match(&prefixed));
        }
    }

    #[test]
    fn leading_criteria_and_text_arrive_unchanged(app_id in any_value(), mark in sway_text()) {
        let criteria = Criteria::new().exact(CriteriaKey::AppId, app_id.clone());
        let inner = SwayCmd::new("mark").keyword("--add").text(mark.clone());
        let cmd = SwayCmd::with_criteria(criteria, inner).unwrap();
        let rendered = cmd.render(Target::Ipc).unwrap();
        let commands = model::execute_command(rendered.as_bytes()).map_err(TestCaseError::fail)?;
        prop_assert_eq!(commands.len(), 1);
        let Some(pairs) = &commands[0].criteria else {
            return Err(TestCaseError::fail("criteria missing"));
        };
        prop_assert_eq!(strings(&commands[0].argv), ["mark", "--add", mark.as_str()]);
        prop_assert_eq!(pairs.len(), 1);
        let regex = Regex::new(std::str::from_utf8(&pairs[0].1).unwrap()).unwrap();
        prop_assert!(regex.is_match(&app_id));
    }

    #[test]
    fn for_window_text_arrives_unchanged(
        target in target(),
        first in sway_text(),
        second in sway_text(),
    ) {
        let criteria = Criteria::new().exact(CriteriaKey::AppId, "x");
        let inner = SwayCmd::new("mark")
            .text(first.clone())
            .keyword("--add")
            .text(second.clone());
        let cmd = SwayCmd::for_window(criteria, inner).unwrap();
        let (_, inner) = for_window_parts(target, &cmd.render(target).unwrap());
        prop_assert_eq!(inner, ["mark", first.as_str(), "--add", second.as_str()]);
    }
}

#[test]
fn swayfx_requirement_keeps_the_highest_version() {
    let plain = SwayCmd::new("opacity").float(0.5);
    assert_eq!(plain.min_swayfx(), None);
    let effect = SwayCmd::new("blur")
        .keyword("enable")
        .requires_swayfx(SwayFxVersion::new(0, 6, 0))
        .requires_swayfx(SwayFxVersion::OLDEST_SUPPORTED);
    assert_eq!(effect.min_swayfx(), Some(SwayFxVersion::new(0, 6, 0)));
    let criteria = Criteria::new().exact(CriteriaKey::AppId, "x");
    let wrapped = SwayCmd::for_window(criteria, effect).unwrap();
    assert_eq!(wrapped.min_swayfx(), Some(SwayFxVersion::new(0, 6, 0)));
}
