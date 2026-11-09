// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::DimClass;
use crate::command::{Color, OutOfRange, SwayCmd, Target};
use crate::version::SwayFxVersion;

/// Renders for both targets, checks that they agree and that the command needs SwayFX.
fn rendered(cmd: &SwayCmd) -> String {
    let config = cmd.render(Target::Config).unwrap();
    assert_eq!(cmd.render(Target::Ipc).unwrap(), config);
    assert!(cmd.min_swayfx().is_some(), "{config}");
    config
}

#[test]
fn every_effect_renders() {
    let shadow = Color::rgba(0, 0, 0, 0x7f);
    let cases = [
        (super::corner_radius(8).unwrap(), "corner_radius 8"),
        (
            super::smart_corner_radius(true),
            "smart_corner_radius enable",
        ),
        (super::blur(false), "blur disable"),
        (super::blur_passes(2).unwrap(), "blur_passes 2"),
        (super::blur_radius(5).unwrap(), "blur_radius 5"),
        (super::blur_noise(0.02).unwrap(), "blur_noise 0.02"),
        (super::blur_brightness(0.9).unwrap(), "blur_brightness 0.9"),
        (super::blur_contrast(0.9).unwrap(), "blur_contrast 0.9"),
        (super::blur_saturation(1.1).unwrap(), "blur_saturation 1.1"),
        (super::shadows(true), "shadows enable"),
        (super::shadow_color(shadow), "shadow_color #0000007f"),
        (
            super::shadow_inactive_color(shadow),
            "shadow_inactive_color #0000007f",
        ),
        (
            super::shadow_blur_radius(20).unwrap(),
            "shadow_blur_radius 20",
        ),
        (
            super::shadow_offset(-2.5, 4.0).unwrap(),
            "shadow_offset -2.5 4",
        ),
        (
            super::default_dim_inactive(0.1).unwrap(),
            "default_dim_inactive 0.1",
        ),
        (
            super::dim_inactive_color(DimClass::Unfocused, Color::rgba(0, 0, 0, 0xff)),
            "dim_inactive_colors.unfocused #000000ff",
        ),
        (
            super::dim_inactive_color(DimClass::Urgent, Color::rgba(0x90, 0, 0, 0xff)),
            "dim_inactive_colors.urgent #900000ff",
        ),
        (
            super::animation_duration_ms(300).unwrap(),
            "animation_duration_ms 300",
        ),
    ];
    for (cmd, expected) in cases {
        assert_eq!(rendered(&cmd), expected);
    }
}

#[test]
fn effects_need_swayfx() {
    assert_eq!(
        super::blur(true).min_swayfx(),
        Some(SwayFxVersion::OLDEST_SUPPORTED)
    );
    assert_eq!(
        super::animation_duration_ms(0).unwrap().min_swayfx(),
        Some(SwayFxVersion::new(0, 6, 0))
    );
}

#[test]
fn ranges_are_checked() {
    let out_of_range = |option: &'static str, min: f64, max: f64| OutOfRange { option, min, max };
    assert_eq!(
        super::corner_radius(100),
        Err(out_of_range("corner_radius", 0.0, 99.0))
    );
    assert_eq!(
        super::blur_passes(11),
        Err(out_of_range("blur_passes", 0.0, 10.0))
    );
    assert_eq!(
        super::blur_radius(11),
        Err(out_of_range("blur_radius", 0.0, 10.0))
    );
    assert_eq!(
        super::shadow_blur_radius(100),
        Err(out_of_range("shadow_blur_radius", 0.0, 99.0))
    );
    assert_eq!(
        super::animation_duration_ms(5001),
        Err(out_of_range("animation_duration_ms", 0.0, 5000.0))
    );
    for value in [-0.01, 1.01, f64::NAN] {
        assert!(super::blur_noise(value).is_err(), "{value}");
        assert!(super::default_dim_inactive(value).is_err(), "{value}");
    }
    for value in [-0.01, 2.01, f64::NAN] {
        assert!(super::blur_brightness(value).is_err(), "{value}");
        assert!(super::blur_contrast(value).is_err(), "{value}");
        assert!(super::blur_saturation(value).is_err(), "{value}");
    }
    assert!(super::shadow_offset(-99.9, 99.9).is_ok());
    assert!(super::shadow_offset(100.0, 0.0).is_err());
    assert!(super::shadow_offset(0.0, f64::NAN).is_err());
    assert!(super::corner_radius(99).is_ok());
    assert!(super::animation_duration_ms(5000).is_ok());
}
