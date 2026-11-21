// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generates `ci/integration/generated/kanshi/sway-things.conf` from the kanshi config of a user
//! who set up their displays before Sway Things (`ci/integration/kanshi/config`), with nothing
//! set up on the Displays page and the workspaces placed round robin, and checks that the
//! committed file is up to date. It also checks what a copy of the user's profiles keeps and
//! leaves out, independent of the committed file.
//!
//! After changing what the file for kanshi holds, regenerate it with
//! `SWAY_THINGS_UPDATE_FIXTURES=1 cargo test -p sway-things-core`.

#[expect(
    dead_code,
    reason = "this fixture is a whole file, so only `check` is used"
)]
mod common;

use std::path::{Path, PathBuf};

use common::Result;
use sway_things_core::outputs::kanshi_config::{self, Item, UserProfile};
use sway_things_core::outputs::profiles::{Profiles, kanshi_config_with};
use sway_things_core::outputs::workspaces::{Placing, Rule};

/// Where the copies say they come from.
const FROM: &str = "~/.config/kanshi/config";

const ROUND_ROBIN: Placing = Placing {
    rule: Rule::RoundRobin,
    count: 10,
};

fn user_config() -> Result<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ci/integration/kanshi/config");
    Ok(std::fs::read_to_string(path)?)
}

/// The user's profiles after the include of ours, which a copy takes the place of.
fn after_ours(text: &str) -> Vec<UserProfile> {
    kanshi_config::parse(text)
        .into_iter()
        .skip_while(|item| !matches!(item, Item::Include(_)))
        .filter_map(|item| match item {
            Item::Profile(profile) => Some(profile),
            Item::Include(_) => None,
        })
        .collect()
}

/// The file for kanshi with nothing set up on the Displays page.
fn ours(theirs: &[UserProfile], placing: Option<Placing>) -> String {
    kanshi_config_with(
        &Profiles::empty(Path::new("/nonexistent")),
        placing,
        theirs,
        FROM,
        false,
    )
}

#[test]
fn fixtures_are_up_to_date() -> Result<()> {
    let theirs = after_ours(&user_config()?);
    common::check(&[(
        "generated/kanshi/sway-things.conf",
        ours(&theirs, Some(ROUND_ROBIN)),
    )])
}

#[test]
fn a_copy_keeps_the_users_displays_and_places_the_workspaces_instead_of_their_lines() -> Result<()>
{
    let text = user_config()?;
    let theirs = after_ours(&text);
    assert_eq!(theirs.len(), 7);
    let generated = ours(&theirs, Some(ROUND_ROBIN));
    // Every display line exactly as written.
    for line in text
        .lines()
        .filter(|line| line.trim_start().starts_with("output "))
    {
        assert!(
            generated.contains(&format!("{line}\n")),
            "{line}\n{generated}"
        );
    }
    // Their lines that place workspaces are gone, the other one stays.
    assert!(!generated.contains("swaymsg"), "{generated}");
    assert!(
        generated.contains("    exec pactl set-default-sink tv\n"),
        "{generated}"
    );
    // A copy of each profile, each running the placement of Sway Things.
    for profile in &theirs {
        assert!(
            generated.contains(&format!("profile {} {{\n", profile.copy_name())),
            "{}\n{generated}",
            profile.name
        );
    }
    assert_eq!(
        generated
            .matches("    exec sway-things hook workspaces\n")
            .count(),
        theirs.len()
    );
    Ok(())
}

#[test]
fn without_a_placement_nothing_is_copied() -> Result<()> {
    let theirs = after_ours(&user_config()?);
    let generated = ours(&theirs, None);
    assert!(!generated.contains("sway-things-of-"), "{generated}");
    assert!(!generated.contains("profile "), "{generated}");
    Ok(())
}
