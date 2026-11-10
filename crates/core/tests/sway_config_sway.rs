// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The config parser against a real sway.
//!
//! `ci/integration/parser.conf` defines binding modes with the constructs that are easy to read
//! differently from sway: variables, continued lines, a brace on the next line, and includes
//! with globs and relative paths. sway reports its modes over IPC, so the parser must find the
//! same names in the same order.
//!
//! This test needs a running sway, so a plain `cargo test` skips it;
//! `scripts/check-integration.sh` runs it with `--ignored`.

use std::error::Error;
use std::path::Path;

use async_io::block_on;
use sway_things_core::ipc::Client;
use sway_things_core::sway_config::{DiagnosticKind, Disk, parse};

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn the_parser_finds_the_modes_sway_has() -> Result<(), Box<dyn Error>> {
    block_on(async {
        let mut sway = Client::connect().await?;
        let loaded = sway.version().await?.loaded_config_file_name;
        let config = parse(Path::new(&loaded), &Disk);

        // The modes in the order they are first defined. sway removes the quotes around a name.
        let mut parsed: Vec<String> = vec!["default".to_owned()];
        for directive in &config.directives {
            if let [block, name] = directive.block.as_slice() {
                let name = name.trim_matches('"').to_owned();
                if block == "mode" && !parsed.contains(&name) {
                    parsed.push(name);
                }
            }
        }
        assert_eq!(parsed, sway.binding_modes().await?);
        assert_eq!(
            parsed[1..],
            [
                "one",
                "two words",
                "continued",
                "yx",
                "from-variable",
                "in-a",
                "in-c",
                "in-b-from-a",
                "in-d",
                "after-from-a"
            ]
        );

        // What sway skipped is what the parser reports.
        let kinds: Vec<&str> = config
            .diagnostics
            .iter()
            .map(|diagnostic| match &diagnostic.kind {
                DiagnosticKind::AlreadyIncluded { .. } => "already included",
                DiagnosticKind::IncludeNotFound { .. } => "not found",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["already included", "not found"]);
        Ok(())
    })
}
