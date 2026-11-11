// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A setting changed end to end against a real sway: sent, read back, saved, generated, and
//! still in effect after sway reloaded its config.
//!
//! This test needs a running compositor, so a plain `cargo test` skips it.
//! `scripts/check-integration.sh` starts a headless sway whose config includes
//! `$XDG_RUNTIME_DIR/sway-things/*.conf` and runs it with `--ignored`.

use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::future;
use sway_things_core::apply::{Applier, Effect};
use sway_things_core::command::{self, SwayCmd, SwayText};
use sway_things_core::generate::{Locations, Outcome, SwayValidator};
use sway_things_core::include::IncludeStatus;
use sway_things_core::ipc::Client;
use sway_things_core::ipc::types::{Node, NodeType};
use sway_things_core::registry::{ConfigFile, Registry, Value};
use sway_things_core::state::{Paths, State};
use sway_things_core::sway_config::Disk;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const GAPS: &str = "windows.gaps.inner";

/// How far the workspace `name` is inset from its output on the left: its gaps. `None` if there
/// is no such workspace.
fn inset(tree: &Node, name: &str) -> Option<i32> {
    tree.nodes.iter().find_map(|output| {
        let workspace = output.nodes.iter().find(|node| {
            node.node_type == NodeType::Workspace && node.name.as_deref() == Some(name)
        })?;
        Some(workspace.rect.x - output.rect.x)
    })
}

/// The name of the focused workspace.
fn focused_workspace(tree: &Node) -> Option<String> {
    tree.nodes
        .iter()
        .flat_map(|output| &output.nodes)
        .find(|node| {
            node.node_type == NodeType::Workspace && (node.focused || !node.focus.is_empty())
        })
        .or_else(|| {
            tree.nodes
                .iter()
                .filter(|output| output.name.as_deref() != Some("__i3"))
                .flat_map(|output| &output.nodes)
                .find(|node| node.node_type == NodeType::Workspace)
        })
        .and_then(|node| node.name.clone())
}

async fn tree(applier: &mut Applier) -> Result<Node> {
    Ok(applier.client().ok_or("no session")?.tree().await?)
}

async fn switch_to(sway: &mut Client, workspace: &str) -> Result<()> {
    let command = SwayCmd::new("workspace").text(SwayText::new(workspace)?);
    Ok(sway.run(&command).await?)
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn a_changed_setting_survives_a_reload() -> Result<()> {
    block_on(future::or(
        async {
            let dir = tempfile::tempdir()?;
            let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or("no XDG_RUNTIME_DIR")?;
            let locations = Locations::new(
                &PathBuf::from(runtime).join("sway-things"),
                &dir.path().join("state"),
            );
            let mut client = Client::connect().await?;

            // The permission comes from the config sway loaded, which includes the directory.
            let main = PathBuf::from(client.version().await?.loaded_config_file_name);
            let status = IncludeStatus::detect(Some(&main), &Disk, &locations);
            let access = status
                .write_access()
                .map_err(|error| format!("{error}: {}", status.message()))?;

            let paths = Paths::new(&dir.path().join("config"), &dir.path().join("state"), "ci")?;
            let state = State::load(Registry::builtin(), paths);
            let mut applier = Applier::with_session(state, locations, SwayValidator::new(), client);

            let start = tree(&mut applier).await?;
            let home = focused_workspace(&start).ok_or("no workspace")?;

            // Live: the open workspace moves in by the difference.
            let result = applier.set(GAPS, None, Value::Integer(4), &access).await?;
            assert!(matches!(result.effect, Effect::Live { .. }), "{result:?}");
            let small = inset(&tree(&mut applier).await?, &home).ok_or("no workspace")?;
            let result = applier.set(GAPS, None, Value::Integer(23), &access).await?;
            assert!(matches!(result.effect, Effect::Live { .. }), "{result:?}");
            let large = inset(&tree(&mut applier).await?, &home).ok_or("no workspace")?;
            assert_eq!(large - small, 19);

            // After a reload, a new workspace gets its gaps from the config alone. The other gaps
            // of the sample config add to ours, so only the difference between two values counts.
            let mut from_config = Vec::new();
            for (round, px) in [(1, 23), (2, 35)] {
                applier.set(GAPS, None, Value::Integer(px), &access).await?;
                let persisted = applier.persist(&[], &access).await?;
                assert!(persisted.saved, "{persisted:?}");
                assert_eq!(
                    persisted.files[1],
                    (ConfigFile::Appearance, Outcome::Written),
                    "{persisted:?}"
                );
                // sway reloads right after it answered, before it reads the next request.
                let sway = applier.client().ok_or("no session")?;
                sway.run(&command::reload()).await?;
                let new = format!("sway-things apply {round}");
                switch_to(sway, &new).await?;
                from_config.push(inset(&tree(&mut applier).await?, &new).ok_or("no workspace")?);
            }
            assert_eq!(from_config[1] - from_config[0], 12, "{from_config:?}");

            // A reset brings the default back, live and in the file.
            let result = applier.reset(GAPS, None, &access).await?;
            assert!(matches!(result.effect, Effect::Live { .. }), "{result:?}");
            let reset = inset(&tree(&mut applier).await?, "sway-things apply 2");
            // sway doesn't let the sum with negative outer gaps go below zero.
            assert_eq!(reset, Some((from_config[1] - 35).max(0)));
            let persisted = applier.persist(&[], &access).await?;
            assert!(persisted.saved && persisted.is_complete(), "{persisted:?}");
            let sway = applier.client().ok_or("no session")?;
            switch_to(sway, &home).await?;
            Ok(())
        },
        async {
            Timer::after(Duration::from_secs(60)).await;
            Err("sway didn't answer within 60 seconds".into())
        },
    ))
}
