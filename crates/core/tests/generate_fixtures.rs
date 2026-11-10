// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Golden files for the generators: the sample settings in `ci/integration/settings/` in, the
//! exact files in `ci/integration/generated/` out, once for the oldest supported sway and once
//! for the oldest supported SwayFX. `scripts/check-integration.sh` feeds them to a real sway:
//! every file must pass `sway --validate` on its own, and the sample configs include them.
//!
//! After changing the registry, the builders or the generator, regenerate the files with
//! `SWAY_THINGS_UPDATE_FIXTURES=1 cargo test -p sway-things-core`.

#[expect(
    dead_code,
    reason = "these fixtures are whole files, so only `check` is used"
)]
mod common;

use std::path::PathBuf;

use common::Result;
use sway_things_core::generate::generate;
use sway_things_core::ipc::Compositor;
use sway_things_core::registry::Registry;
use sway_things_core::state::{Paths, State};
use sway_things_core::version::{SwayFxVersion, SwayVersion};

#[test]
fn fixtures_are_up_to_date() -> Result<()> {
    let settings = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ci/integration/settings");
    // Loading only reads; the state directory is never touched.
    let state_dir = tempfile::tempdir()?;
    let registry = Registry::builtin();
    let state = State::load(registry, Paths::new(&settings, state_dir.path(), "ci")?);
    assert_eq!(state.problems(), []);

    let base = SwayVersion::OLDEST_SUPPORTED;
    let compositors = [
        ("sway", Compositor::Sway { version: base }),
        (
            "swayfx",
            Compositor::SwayFx {
                version: SwayFxVersion::OLDEST_SUPPORTED,
                base,
            },
        ),
    ];
    let mut fixtures = Vec::new();
    for (directory, compositor) in compositors {
        for file in generate(registry, &state, compositor) {
            let name = format!("generated/{directory}/{}", file.file().file_name());
            fixtures.push((name, file.text()));
        }
    }
    let fixtures: Vec<(&str, String)> = fixtures
        .iter()
        .map(|(name, text)| (name.as_str(), text.clone()))
        .collect();
    common::check(&fixtures)
}
