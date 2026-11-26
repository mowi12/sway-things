// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The crates of the workspace that the architecture tests read.

use std::path::Path;

use crate::graph::{Files, Unit};

/// The libraries and binaries of `core`, `cli` and `gui`, read from the workspace at `root`.
pub fn units(root: &Path) -> Vec<Unit> {
    let unit = |name: &str, extern_name: Option<&str>, dir: &str, file: &str| Unit {
        name: name.to_owned(),
        extern_name: extern_name.map(str::to_owned),
        dir: dir.to_owned(),
        root: file.to_owned(),
        files: Files::Disk(root.join(dir)),
    };
    vec![
        unit(
            "core",
            Some("sway_things_core"),
            "crates/core/src",
            "lib.rs",
        ),
        unit("cli", Some("sway_things_cli"), "crates/cli/src", "lib.rs"),
        unit("cli-bin", None, "crates/cli/src", "main.rs"),
        unit("gui", Some("sway_things_gui"), "crates/gui/src", "lib.rs"),
        unit("gui-bin", None, "crates/gui/src", "main.rs"),
    ]
}
