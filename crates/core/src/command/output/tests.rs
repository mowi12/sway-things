// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use proptest::prelude::*;

use super::{Fill, Mode, Output, OutputError, OutputSelector, Transform, parse_position};
use crate::command::model::{self, Parsed};
use crate::command::{Color, OutOfRange, SwayCmd, Target, TextError};

fn all() -> Output {
    Output::new(OutputSelector::All)
}

/// Renders for both targets and checks that they agree (no text that needs target-specific
/// quoting).
fn rendered(cmd: &SwayCmd) -> String {
    let config = cmd.render(Target::Config).unwrap();
    assert_eq!(cmd.render(Target::Ipc).unwrap(), config);
    config
}

#[test]
fn every_subcommand_renders() {
    let o = all();
    let mode = |text| Mode::parse(text).unwrap();
    let cases = [
        (o.mode(mode("1920x1080")), "output * mode 1920x1080"),
        (
            o.mode(mode("2560x1440@59.951Hz")),
            "output * mode 2560x1440@59.951Hz",
        ),
        (
            o.mode(mode("1920x1080@60Hz")),
            "output * mode 1920x1080@60Hz",
        ),
        (o.position(0, 0), "output * pos 0 0"),
        (o.position(-1280, 304), "output * pos -1280 304"),
        (o.scale(1.0).unwrap(), "output * scale 1"),
        (o.scale(1.25).unwrap(), "output * scale 1.25"),
        (o.transform(Transform::Normal), "output * transform normal"),
        (o.transform(Transform::R270), "output * transform 270"),
        (
            o.transform(Transform::Flipped90),
            "output * transform flipped-90",
        ),
        (o.enable(true), "output * enable"),
        (o.enable(false), "output * disable"),
    ];
    for (cmd, expected) in cases {
        assert_eq!(rendered(&cmd), expected);
    }
}

#[test]
fn an_output_is_text_in_both_targets() {
    let output = Output::new(
        OutputSelector::output("InfoVision Optoelectronics (Kunshan) Co.,Ltd China 0x057D Unknown")
            .unwrap(),
    );
    // The comma would separate two commands over IPC if it weren't quoted.
    assert_eq!(
        output.enable(true).render(Target::Ipc).unwrap(),
        r#"output "InfoVision Optoelectronics (Kunshan) Co.,Ltd China 0x057D Unknown" enable"#
    );
    let named = Output::new(OutputSelector::output("Dell $1 \"U2415\"").unwrap());
    assert_eq!(
        named.scale(2.0).unwrap().render(Target::Ipc).unwrap(),
        r#"output 'Dell $$1 "U2415"' scale 2"#
    );
    assert_eq!(
        named.scale(2.0).unwrap().render(Target::Config).unwrap(),
        r#"output "Dell $$1 \"U2415\"" scale 2"#
    );
}

#[test]
fn reserved_names_and_bad_text_are_no_output() {
    for reserved in ["*", "-", "--"] {
        assert_eq!(OutputSelector::output(reserved), Err(OutputError::Reserved));
    }
    assert_eq!(
        OutputSelector::output(""),
        Err(OutputError::Text(TextError::Empty))
    );
    assert!(matches!(
        OutputSelector::output("DP\\1"),
        Err(OutputError::Text(TextError::Backslash { .. }))
    ));
    assert!(OutputSelector::output("DP-4").is_ok());
}

#[test]
fn modes_read_back_as_written() {
    for text in [
        "1920x1080",
        "1920x1080@60Hz",
        "2560x1440@59.951Hz",
        "1920x1080@60.01Hz",
        "3840x2160@143.912Hz",
        "800x600@0.5Hz",
    ] {
        assert_eq!(Mode::parse(text).unwrap().to_string(), text);
    }
    let mode = Mode::parse("2560x1440@59.951Hz").unwrap();
    assert_eq!(
        (mode.width(), mode.height(), mode.refresh()),
        (2560, 1440, Some(59_951))
    );
    assert_eq!(
        Mode::new(1920, 1080, Some(60_010)).unwrap().to_string(),
        "1920x1080@60.01Hz"
    );
    assert_eq!(
        Mode::new(1920, 1080, Some(0)).unwrap().to_string(),
        "1920x1080"
    );
}

#[test]
fn malformed_modes_are_refused() {
    for text in [
        "",
        "1920",
        "1920x",
        "x1080",
        "0x1080",
        "1920x0",
        "1920 1080",
        "+1920x1080",
        "1920x1080@",
        "1920x1080@60",
        "1920x1080@60hz",
        "1920x1080@60.Hz",
        "1920x1080@.5Hz",
        "1920x1080@60.0001Hz",
        "1920x1080@0Hz",
        "1920x1080@-60Hz",
        "3000000000x1080",
        "1920x1080@60Hz ",
    ] {
        assert_eq!(Mode::parse(text), Err(OutputError::InvalidMode), "{text:?}");
    }
}

#[test]
fn scales_outside_the_range_are_refused() {
    let o = all();
    for scale in [0.0, 0.2, -1.0, 10.5, f64::NAN, f64::INFINITY] {
        assert_eq!(
            o.scale(scale),
            Err(OutputError::OutOfRange(OutOfRange {
                option: "scale",
                min: super::MIN_SCALE,
                max: super::MAX_SCALE,
            }))
        );
    }
}

#[test]
fn transforms_read_back_by_name() {
    for transform in Transform::ALL {
        assert_eq!(Transform::from_name(transform.name()), Some(transform));
    }
    assert_eq!(Transform::from_name("0"), Some(Transform::Normal));
    assert_eq!(Transform::from_name("Normal"), None);
    assert!(Transform::R90.is_sideways() && Transform::Flipped270.is_sideways());
    assert!(!Transform::R180.is_sideways() && !Transform::Flipped.is_sideways());
}

#[test]
fn positions_read_as_two_numbers() {
    assert_eq!(parse_position("1920,0"), Ok((1920, 0)));
    assert_eq!(parse_position("-1280,-5"), Ok((-1280, -5)));
    for text in [
        "",
        "1920",
        "1920,",
        ",0",
        "1920 0",
        "1920,0,0",
        "+1,0",
        "1,0x",
        "3000000000,0",
        "-,0",
    ] {
        assert_eq!(
            parse_position(text),
            Err(OutputError::InvalidPosition),
            "{text:?}"
        );
    }
}

proptest! {
    #[test]
    fn every_mode_reads_back(
        width in 1_u32..=100_000,
        height in 1_u32..=100_000,
        refresh in proptest::option::of(1_u32..=1_000_000),
    ) {
        let mode = Mode::new(width, height, refresh).unwrap();
        prop_assert_eq!(Mode::parse(&mode.to_string()), Ok(mode));
        // The word passes the command's own check.
        prop_assert!(all().mode(mode).render(Target::Ipc).is_ok());
    }
}

/// What `wordexp(3)` makes of a word that only uses single quotes, and double quotes around
/// characters without a meaning there, with `HOME=/home/me`: the quotes go, a leading `~/`
/// becomes the home folder, everything else stays.
fn wordexp(word: &str) -> Result<String, String> {
    let (mut out, word) = match word.strip_prefix("~/") {
        Some(rest) => ("/home/me/".to_owned(), rest),
        None => (String::new(), word),
    };
    let mut quote = None;
    for character in word.chars() {
        match (quote, character) {
            (None, '\'' | '"') => quote = Some(character),
            (Some(open), _) if open == character => quote = None,
            (Some('"'), '$' | '`' | '\\') => return Err(format!("{character} in double quotes")),
            (Some(_), _) => out.push(character),
            (None, _) => return Err(format!("{character:?} outside quotes")),
        }
    }
    match quote {
        None => Ok(out),
        Some(_) => Err("unterminated quote".to_owned()),
    }
}

/// The file sway reads from a rendered `output * bg <file> …` command for `target`.
fn file_for(target: Target, rendered: &str) -> Result<String, String> {
    let argv = match target {
        Target::Ipc => match model::execute_command(rendered.as_bytes())?.as_slice() {
            [Parsed { argv, .. }] => argv.clone(),
            commands => return Err(format!("expected one command, got {commands:?}")),
        },
        Target::Config => model::config_command(rendered.as_bytes())?,
    };
    let file = argv.get(3).ok_or("no file")?;
    wordexp(&String::from_utf8(file.clone()).map_err(|error| error.to_string())?)
}

#[test]
fn colors_render_without_alpha() {
    let cmd = all().background_color(Color::rgba(1, 2, 3, 0x80));
    assert_eq!(
        cmd.render(Target::Ipc).unwrap(),
        "output * bg #010203 solid_color"
    );
    assert_eq!(
        cmd.render(Target::Config).unwrap(),
        "output * bg #010203 solid_color"
    );
}

#[test]
fn every_fill_mode_renders() {
    for mode in Fill::ALL {
        let cmd = all()
            .background_image(Path::new("/a.png"), mode, Color::rgb(0, 0, 0))
            .unwrap();
        assert_eq!(
            cmd.render(Target::Ipc).unwrap(),
            format!("output * bg \"'/a.png'\" {} #000000", mode.keyword())
        );
        assert_eq!(
            Fill::from_keyword(&mode.keyword().to_uppercase()),
            Some(mode)
        );
    }
    assert_eq!(Fill::from_keyword("solid_color"), None);
}

#[test]
fn a_background_selects_one_output() {
    let cmd = Output::new(OutputSelector::output("HDMI-A-1").unwrap())
        .background_color(Color::rgb(0, 0, 0));
    assert_eq!(
        cmd.render(Target::Ipc).unwrap(),
        r#"output "HDMI-A-1" bg #000000 solid_color"#
    );
}

#[test]
fn file_names_are_data_for_wordexp() {
    let path = Path::new("/home/me/it's $(rm -rf ~) `id` \"x\" $HOME.png");
    let cmd = all()
        .background_image(path, Fill::Fit, Color::rgb(1, 2, 3))
        .unwrap();
    for target in [Target::Ipc, Target::Config] {
        let rendered = cmd.render(target).unwrap();
        assert_eq!(
            file_for(target, &rendered).unwrap(),
            path.to_str().unwrap(),
            "{rendered}"
        );
    }
}

#[test]
fn the_home_folder_is_left_to_wordexp() {
    let cmd = all()
        .background_image(Path::new("~/it's.png"), Fill::Fill, Color::rgb(0, 0, 0))
        .unwrap();
    assert_eq!(
        cmd.render(Target::Config).unwrap(),
        r#"output * bg "~/'it'\"'\"'s.png'" fill #000000"#
    );
    for target in [Target::Ipc, Target::Config] {
        let rendered = cmd.render(target).unwrap();
        assert_eq!(file_for(target, &rendered).unwrap(), "/home/me/it's.png");
    }
}

#[test]
fn paths_sway_cant_read_are_refused() {
    let image =
        |path: &str| all().background_image(Path::new(path), Fill::Fill, Color::rgb(0, 0, 0));
    assert_eq!(image("a.png"), Err(OutputError::NotAbsolute));
    assert_eq!(image("~me/a.png"), Err(OutputError::NotAbsolute));
    assert_eq!(image("./a.png"), Err(OutputError::NotAbsolute));
    assert_eq!(image("/a  b.png"), Err(OutputError::DoubleSpace));
    assert_eq!(
        image("/a\\b.png"),
        Err(OutputError::Text(TextError::Backslash { index: 2 }))
    );
    assert!(matches!(
        image("/a\nb.png"),
        Err(OutputError::Text(TextError::ControlCharacter { .. }))
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// Every path the builder takes reaches sway's `wordexp` as a word that expands to itself.
    #[test]
    fn accepted_paths_arrive_unchanged(path in "/[^\\x00-\\x1f\\x7f\\\\]{0,40}") {
        let Ok(cmd) = all().background_image(Path::new(&path), Fill::Fill, Color::rgb(0, 0, 0))
        else {
            prop_assert!(path.contains("  ") || path.chars().any(char::is_control));
            return Ok(());
        };
        for target in [Target::Ipc, Target::Config] {
            let rendered = cmd.render(target).unwrap();
            prop_assert_eq!(file_for(target, &rendered), Ok(path.clone()), "{}", rendered);
        }
    }
}
