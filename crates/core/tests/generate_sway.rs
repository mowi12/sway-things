// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The write path of the generators with a real `sway --validate`.
//!
//! These tests need sway, so a plain `cargo test` skips them. `scripts/check-integration.sh` runs
//! them with `--ignored` next to a headless sway, with `SWAYSOCK` pointing at it.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use async_io::block_on;
use sway_things_core::generate::{Locations, Outcome, SwayValidator, WriteError, generate, write};
use sway_things_core::include::{IncludeStatus, WriteAccess};
use sway_things_core::ipc::{Client, Compositor};
use sway_things_core::registry::{ConfigFile, Registry};
use sway_things_core::state::{Paths, State};
use sway_things_core::sway_config::Disk;
use sway_things_core::version::SwayFxVersion;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// The sample settings of `ci/integration/settings/`.
fn sample(state_dir: &std::path::Path) -> Result<State> {
    let settings = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ci/integration/settings");
    let state = State::load(Registry::builtin(), Paths::new(&settings, state_dir, "ci")?);
    assert_eq!(state.problems(), []);
    Ok(state)
}

/// The permission to write, from a sway config next to the generated files that includes them
/// with the line of the instructions.
fn access(locations: &Locations) -> Result<WriteAccess> {
    let config = locations.directory().with_file_name("config");
    fs::create_dir_all(locations.directory())?;
    let status = IncludeStatus::detect(Some(&config), &Disk, locations);
    fs::write(&config, format!("{}\n", status.instructions().line()))?;
    Ok(IncludeStatus::detect(Some(&config), &Disk, locations).write_access()?)
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn generated_files_pass_sway_and_are_written() -> Result<()> {
    block_on(async {
        let dir = tempfile::tempdir()?;
        let state = sample(&dir.path().join("state"))?;
        let locations = Locations::new(
            &dir.path().join("config/sway/sway-things"),
            &dir.path().join("state"),
        );
        // What runs decides what is generated: the effects only on SwayFX. Without a session,
        // the installed binary says the same.
        let compositor = Client::connect().await?.compositor();
        assert_eq!(SwayValidator::new().compositor().await?, Some(compositor));
        let files = generate(Registry::builtin(), &state, compositor);
        let effects = &files[3];
        assert_eq!(
            effects.lines()[0].skipped.is_none(),
            compositor.is_swayfx(),
            "{effects:?}"
        );

        let report = write(
            &files,
            &locations,
            &SwayValidator::new(),
            &[],
            &access(&locations)?,
        )
        .await?;
        for (file, outcome) in &report {
            assert_eq!(*outcome, Outcome::Written, "{file:?}");
            let generated = files.iter().find(|generated| generated.file() == *file);
            assert_eq!(
                fs::read_to_string(locations.path(*file))?,
                generated.ok_or("no such file")?.text()
            );
        }
        assert_eq!(report.len(), 4);
        // No staged file is left behind.
        assert_eq!(fs::read_dir(locations.directory())?.count(), 4);

        let report = write(
            &files,
            &locations,
            &SwayValidator::new(),
            &[],
            &access(&locations)?,
        )
        .await?;
        assert!(
            report
                .iter()
                .all(|(_, outcome)| *outcome == Outcome::Unchanged)
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn a_file_sway_rejects_keeps_its_old_content() -> Result<()> {
    block_on(async {
        let compositor = Client::connect().await?.compositor();
        if compositor.is_swayfx() {
            // Every line the generator can produce is valid on SwayFX.
            return Ok(());
        }
        let dir = tempfile::tempdir()?;
        let state = sample(&dir.path().join("state"))?;
        let locations = Locations::new(
            &dir.path().join("config/sway/sway-things"),
            &dir.path().join("state"),
        );
        let valid = generate(Registry::builtin(), &state, compositor);
        write(
            &valid,
            &locations,
            &SwayValidator::new(),
            &[],
            &access(&locations)?,
        )
        .await?;
        let effects = locations.path(ConfigFile::Effects);
        let old = fs::read_to_string(&effects)?;

        // Generated for SwayFX by mistake: sway doesn't know `corner_radius`.
        let swayfx = Compositor::SwayFx {
            version: SwayFxVersion::OLDEST_SUPPORTED,
            base: compositor.sway_version(),
        };
        let invalid = generate(Registry::builtin(), &state, swayfx);
        let report = write(
            &invalid,
            &locations,
            &SwayValidator::new(),
            &[],
            &access(&locations)?,
        )
        .await?;
        match &report[3] {
            (ConfigFile::Effects, Outcome::Invalid(lines)) => {
                assert_eq!(lines.len(), 1, "{lines:?}");
                assert_eq!(lines[0].line, 3);
                assert!(lines[0].message.contains("corner_radius"), "{lines:?}");
                let source = lines[0].source.as_ref().ok_or("no source")?;
                assert_eq!(source.keys, ["effects.corner_radius"]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(fs::read_to_string(&effects)?, old);
        assert!(
            report[..3]
                .iter()
                .all(|(_, outcome)| *outcome == Outcome::Unchanged)
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn without_sway_nothing_is_written() -> Result<()> {
    block_on(async {
        let dir = tempfile::tempdir()?;
        let state = sample(&dir.path().join("state"))?;
        let locations = Locations::new(&dir.path().join("sway-things"), &dir.path().join("state"));
        let compositor = Client::connect().await?.compositor();
        let files = generate(Registry::builtin(), &state, compositor);
        let missing = SwayValidator::with_program(dir.path().join("no-such-sway"));
        assert!(matches!(
            write(&files, &locations, &missing, &[], &access(&locations)?).await,
            Err(WriteError::Validator(_))
        ));
        assert_eq!(fs::read_dir(locations.directory())?.count(), 0);
        Ok(())
    })
}
