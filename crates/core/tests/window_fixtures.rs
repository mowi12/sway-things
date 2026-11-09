// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generates the window and SwayFX effect fixtures in `ci/integration/` from the builders and
//! checks that the committed files are up to date. `scripts/check-integration.sh` feeds them to a
//! real sway: `window.conf` and `effects.conf` must pass `sway --validate` on their own, and every
//! command in `window-ipc.txt` and `effects-ipc.txt` must succeed. The effect fixtures only run on
//! SwayFX.
//!
//! Commands that need an open window (`opacity` on its own, commands with leading criteria) aren't
//! included: the headless sway has no windows, so they fail with "No matching node".
//!
//! After changing the builders, regenerate the files with
//! `SWAY_THINGS_UPDATE_FIXTURES=1 cargo test -p sway-things-core`.

mod common;

use common::{Result, push_line};
use sway_things_core::command::window::{
    self, BorderStyle, ClientClass, Gaps, GapsScope, HideEdgeBorders, SmartBorders, SmartGaps,
    TitleAlign, TitleColors, WindowColors, Workspace,
};
use sway_things_core::command::{
    Availability, Color, Criteria, CriteriaKey, SwayCmd, Target, effects,
};

const GAPS: [Gaps; 8] = [
    Gaps::Inner,
    Gaps::Outer,
    Gaps::Horizontal,
    Gaps::Vertical,
    Gaps::Top,
    Gaps::Right,
    Gaps::Bottom,
    Gaps::Left,
];

fn color_commands() -> Vec<SwayCmd> {
    let colors = WindowColors {
        border: Color::rgb(0x4c, 0x78, 0x99),
        background: Color::rgb(0x28, 0x55, 0x77),
        text: Color::rgb(0xff, 0xff, 0xff),
        indicator: Color::rgb(0x2e, 0x9e, 0xf4),
        child_border: Color::rgba(0x28, 0x55, 0x77, 0xcc),
    };
    let mut commands: Vec<SwayCmd> = [
        ClientClass::Focused,
        ClientClass::FocusedInactive,
        ClientClass::Unfocused,
        ClientClass::Urgent,
    ]
    .into_iter()
    .map(|class| window::client(class, &colors))
    .collect();
    commands.push(window::client_focused_tab_title(&TitleColors {
        border: colors.border,
        background: colors.background,
        text: colors.text,
    }));
    commands
}

fn border_commands() -> Vec<SwayCmd> {
    let mut commands = Vec::new();
    for style in [
        BorderStyle::None,
        BorderStyle::Normal(2),
        BorderStyle::Pixel(3),
    ] {
        commands.push(window::default_border(style));
        commands.push(window::default_floating_border(style));
    }
    for hide in [
        HideEdgeBorders::None,
        HideEdgeBorders::Vertical,
        HideEdgeBorders::Horizontal,
        HideEdgeBorders::Both,
        HideEdgeBorders::Smart,
        HideEdgeBorders::SmartNoGaps,
    ] {
        commands.push(window::hide_edge_borders(hide, false));
    }
    commands.push(window::hide_edge_borders(HideEdgeBorders::None, true));
    for smart in [SmartBorders::On, SmartBorders::NoGaps, SmartBorders::Off] {
        commands.push(window::smart_borders(smart));
    }
    commands
}

fn gaps_commands() -> Result<Vec<SwayCmd>> {
    // A workspace name with quotes, `;` and the `$mod` variable of ci/integration/sway.conf.
    let workspace = Workspace::new(r#"3: "mail" $mod;x"#)?;
    let mut commands = Vec::new();
    for which in GAPS {
        commands.push(window::gaps(which, 4)?);
        commands.push(workspace.gaps(which, 2)?);
        commands.push(window::gaps_runtime(which, GapsScope::All, 6)?);
    }
    commands.push(window::gaps(Gaps::Outer, -2)?);
    commands.push(window::gaps_runtime(Gaps::Inner, GapsScope::Current, 8)?);
    for smart in [SmartGaps::On, SmartGaps::InverseOuter, SmartGaps::Off] {
        commands.push(window::smart_gaps(smart));
    }
    Ok(commands)
}

fn title_bar_commands() -> Result<Vec<SwayCmd>> {
    Ok(vec![
        window::font("DejaVu Sans Mono 10", false)?,
        window::font("monospace 10", true)?,
        window::title_align(TitleAlign::Left),
        window::title_align(TitleAlign::Center),
        window::title_align(TitleAlign::Right),
        // sway rejects padding below the border thickness, so the thickness comes first.
        window::titlebar_border_thickness(1),
        window::titlebar_padding(6, 4),
    ])
}

fn window_commands() -> Result<Vec<SwayCmd>> {
    let mut commands = color_commands();
    commands.extend(border_commands());
    commands.extend(gaps_commands()?);
    commands.extend(title_bar_commands()?);
    let criteria = Criteria::new().exact(CriteriaKey::AppId, "org.gnome.Nautilus");
    commands.push(SwayCmd::for_window(criteria, window::opacity(0.9)?)?);
    Ok(commands)
}

fn effect_commands() -> Result<Vec<SwayCmd>> {
    let shadow = Color::rgba(0, 0, 0, 0x7f);
    Ok(vec![
        effects::corner_radius(8)?,
        effects::smart_corner_radius(true),
        effects::blur(true),
        effects::blur_passes(2)?,
        effects::blur_radius(5)?,
        effects::blur_noise(0.02)?,
        effects::blur_brightness(0.9)?,
        effects::blur_contrast(0.9)?,
        effects::blur_saturation(1.1)?,
        effects::shadows(true),
        effects::shadow_color(shadow),
        effects::shadow_inactive_color(shadow),
        effects::shadow_blur_radius(20)?,
        effects::shadow_offset(-2.5, 4.0)?,
        effects::default_dim_inactive(0.1)?,
        effects::dim_inactive_color(effects::DimClass::Unfocused, Color::rgba(0, 0, 0, 0xff)),
        effects::dim_inactive_color(effects::DimClass::Urgent, Color::rgba(0x90, 0, 0, 0xff)),
        effects::animation_duration_ms(300)?,
    ])
}

fn fixture(commands: &[SwayCmd], target: Target, needs_swayfx: bool) -> Result<String> {
    let unavailable = match target {
        Target::Ipc => Availability::ConfigOnly,
        Target::Config => Availability::RuntimeOnly,
    };
    let mut out = common::header("window_fixtures.rs", needs_swayfx);
    for cmd in commands
        .iter()
        .filter(|cmd| cmd.availability() != unavailable)
    {
        push_line(&mut out, &cmd.render(target)?);
    }
    Ok(out)
}

#[test]
fn fixtures_are_up_to_date() -> Result<()> {
    let window = window_commands()?;
    let effects = effect_commands()?;
    assert!(window.iter().all(|cmd| cmd.min_swayfx().is_none()));
    assert!(effects.iter().all(|cmd| cmd.min_swayfx().is_some()));
    common::check(&[
        ("window.conf", fixture(&window, Target::Config, false)?),
        ("window-ipc.txt", fixture(&window, Target::Ipc, false)?),
        ("effects.conf", fixture(&effects, Target::Config, true)?),
        ("effects-ipc.txt", fixture(&effects, Target::Ipc, true)?),
    ])
}
