// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;

use super::{Mode, Output, OutputError, OutputSelector, Transform, parse_position};
use crate::command::{OutOfRange, SwayCmd, Target, TextError};

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
