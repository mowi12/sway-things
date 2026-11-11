// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The IPC client against a real sway or SwayFX.
//!
//! These tests need a running compositor, so a plain `cargo test` skips them.
//! `scripts/check-integration.sh` starts a headless sway and runs them with `--ignored`, with
//! `SWAYSOCK` pointing at it and the output of `sway --version` in `SWAY_THINGS_SWAY_VERSION`.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::future;
use sway_things_core::command::input::{DragLock, Input, InputSelector, InputType};
use sway_things_core::command::{SwayCmd, SwayText, behavior, effects, window};
use sway_things_core::devices::{Device, DeviceWatch};
use sway_things_core::ipc::types::{Event, EventType, NodeType};
use sway_things_core::ipc::{Client, Compositor, IpcError, Unmet};
use sway_things_core::version::SwayVersion;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const NEEDS_SWAY: &str = "needs a running sway; run scripts/check-integration.sh";

/// Runs a test, which fails instead of hanging if sway doesn't answer.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(30)).await;
        Err("sway didn't answer within 30 seconds".into())
    }))
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn the_compositor_and_its_version_are_detected() -> Result<()> {
    run(async {
        // `sway version 1.10.1`, or `swayfx version 0.5.3 (based on sway 1.11.0)`.
        let expected = std::env::var("SWAY_THINGS_SWAY_VERSION").map_err(|_| NEEDS_SWAY)?;
        let mut sway = Client::connect().await?;
        let compositor = sway.compositor();
        match compositor {
            Compositor::Sway { version } => {
                assert!(expected.starts_with("sway version "), "{expected}");
                assert!(expected.contains(&version.to_string()), "{expected}");
            }
            Compositor::SwayFx { version, base } => {
                assert!(expected.starts_with("swayfx version "), "{expected}");
                assert!(
                    expected.contains(&format!("swayfx version {version}")),
                    "{expected}"
                );
                assert!(
                    expected.contains(&format!("based on sway {base}")),
                    "{expected}"
                );
            }
        }
        assert!(compositor.sway_version() >= SwayVersion::OLDEST_SUPPORTED);

        let version = sway.version().await?;
        assert_eq!(version.compositor, compositor);
        assert_ne!(version.human_readable, "");
        assert!(version.loaded_config_file_name.contains("ci/integration/"));
        Ok(())
    })
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn many_clients_connect_at_once() -> Result<()> {
    // More than sway queues before it accepts them.
    let clients: Vec<_> = (0..32)
        .map(|_| {
            std::thread::spawn(|| {
                run(async {
                    let mut sway = Client::connect().await?;
                    assert_eq!(sway.binding_state().await?, "default");
                    Ok(())
                })
                .map_err(|error| error.to_string())
            })
        })
        .collect();
    for client in clients {
        client.join().map_err(|_| "a client panicked")??;
    }
    Ok(())
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn a_command_takes_effect_and_can_be_read_back() -> Result<()> {
    run(async {
        let mut sway = Client::connect().await?;
        // A seat exists once a command names it, and a hostile name must arrive unchanged.
        let name = "sway-things $ipc \"test\"; exit";
        let command = SwayCmd::new("seat")
            .text(SwayText::new(name)?)
            .keyword("hide_cursor")
            .int(1000);
        assert!(sway.seats().await?.iter().all(|seat| seat.name != name));
        sway.run(&command).await?;
        assert!(sway.seats().await?.iter().any(|seat| seat.name == name));

        // Commands from the builders, also several in a row.
        let results = sway
            .run_all(&[
                behavior::tiling_drag(false),
                window::gaps(window::Gaps::Inner, 6)?,
                window::gaps_runtime(window::Gaps::Inner, window::GapsScope::All, 6)?,
            ])
            .await;
        for result in results {
            result?;
        }
        Ok(())
    })
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn a_rejected_command_carries_sways_message() -> Result<()> {
    run(async {
        let mut sway = Client::connect().await?;
        // There is no window to make transparent.
        let error = sway.run(&window::opacity(0.5)?).await.unwrap_err();
        assert!(
            matches!(
                &error,
                IpcError::CommandFailed { command, message, parse_error: false }
                    if command == "opacity 0.5" && !message.is_empty()
            ),
            "{error:?}"
        );
        // The connection is still in step.
        sway.run(&behavior::tiling_drag(true)).await?;
        Ok(())
    })
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn commands_are_gated_by_the_running_compositor() -> Result<()> {
    run(async {
        let mut sway = Client::connect().await?;
        let rounded = effects::corner_radius(6)?;
        if sway.compositor().is_swayfx() {
            sway.run(&rounded).await?;
        } else {
            assert!(matches!(
                sway.run(&rounded).await,
                Err(IpcError::CommandUnsupported {
                    unmet: Unmet::SwayFx { .. },
                    ..
                })
            ));
        }
        let sticky =
            Input::new(InputSelector::Type(InputType::Touchpad)).drag_lock(DragLock::EnabledSticky);
        if sway.compositor().sway_version() >= SwayVersion::new(1, 11, 0) {
            sway.run(&sticky).await?;
        } else {
            assert!(matches!(
                sway.run(&sticky).await,
                Err(IpcError::CommandUnsupported {
                    unmet: Unmet::Sway { .. },
                    ..
                })
            ));
        }
        Ok(())
    })
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn the_state_of_the_session_is_read_as_typed_replies() -> Result<()> {
    run(async {
        let mut sway = Client::connect().await?;
        let outputs = sway.outputs().await?;
        assert!(
            outputs
                .iter()
                .any(|output| output.name.starts_with("HEADLESS-")),
            "{outputs:?}"
        );
        let tree = sway.tree().await?;
        assert_eq!(tree.node_type, NodeType::Root);
        assert!(tree.nodes.iter().any(|output| {
            output
                .name
                .as_deref()
                .is_some_and(|name| name.starts_with("HEADLESS-"))
        }));
        // The headless session has no input devices; the reply must still parse.
        sway.inputs().await?;
        assert!(sway.seats().await?.iter().any(|seat| seat.name == "seat0"));
        assert!(
            sway.binding_modes()
                .await?
                .iter()
                .any(|mode| mode == "default")
        );
        assert_eq!(sway.binding_state().await?, "default");
        // The text of the main config as it is on disk, without the files it includes.
        let loaded = sway.version().await?.loaded_config_file_name;
        assert_eq!(sway.config().await?, std::fs::read_to_string(loaded)?);
        Ok(())
    })
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn tick_and_workspace_events_arrive() -> Result<()> {
    run(async {
        let mut sway = Client::connect().await?;
        let mut events = sway
            .subscribe(&[EventType::Tick, EventType::Workspace])
            .await?;
        // The other tests use the same sway, so events of theirs arrive in between.
        // sway confirms a tick subscription with a first tick.
        loop {
            if let Event::Tick(tick) = events.next().await.ok_or("no events")?? {
                assert!(tick.first);
                break;
            }
        }

        sway.send_tick("sway-things ipc test").await?;
        loop {
            if let Event::Tick(tick) = events.next().await.ok_or("no events")?? {
                assert!(!tick.first);
                assert_eq!(tick.payload, "sway-things ipc test");
                break;
            }
        }

        // Switching to a new workspace is reported, with the workspace.
        let workspace = SwayCmd::new("workspace").text(SwayText::new("sway-things events")?);
        sway.run(&workspace).await?;
        loop {
            if let Event::Workspace(event) = events.next().await.ok_or("no events")?? {
                let current = event.current.as_ref().and_then(|node| node.name.as_deref());
                if current == Some("sway-things events") {
                    break;
                }
            }
        }
        Ok(())
    })
}

#[test]
#[ignore = "needs a running sway; run scripts/check-integration.sh"]
fn the_device_watch_starts_on_a_session_without_devices() -> Result<()> {
    run(async {
        let sway = Client::connect().await?;
        // The headless session has no input devices; hotplug is tested against a stand-in.
        let watch = DeviceWatch::start(&sway).await?;
        assert!(
            watch.devices().iter().all(Device::is_connected),
            "{:?}",
            watch.devices()
        );
        assert!(
            !watch
                .devices()
                .is_last_enabled_keyboard("1:1:AT_Translated_Set_2_keyboard")
        );
        Ok(())
    })
}
