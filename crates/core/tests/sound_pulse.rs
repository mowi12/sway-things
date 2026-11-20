// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sound backend against a private PipeWire with pipewire-pulse and virtual devices.
//!
//! These tests need PipeWire, WirePlumber, pipewire-pulse, `pactl` and `pacat`, so a plain
//! `cargo test` skips them. `scripts/check-integration.sh` runs them with `--ignored` in the
//! containers, which have all of them.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::{StreamExt, future};
use sway_things_core::sound::stand_in::StandIn;
use sway_things_core::sound::{Error as SoundError, Meter, NORMAL, Snapshot, Sound, Volume};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Runs a test, which fails instead of hanging if the server doesn't answer.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(60)).await;
        Err("the sound server didn't answer within 60 seconds".into())
    }))
}

/// The stand-in with two outputs and a microphone; our connection to it.
fn with_devices() -> Result<(StandIn, Sound)> {
    let stand_in = StandIn::start()?;
    stand_in.add_output("speakers", "Speakers")?;
    stand_in.add_output("hdmi", "HDMI_Monitor")?;
    stand_in.add_input("mic", "Microphone")?;
    stand_in.pactl(&["set-default-sink", "speakers"])?;
    stand_in.pactl(&["set-default-source", "mic"])?;
    let sound = stand_in.sound()?;
    Ok((stand_in, sound))
}

/// Waits until the snapshot satisfies `done`.
async fn until(sound: &Sound, done: impl Fn(&Snapshot) -> bool) -> Result<Snapshot> {
    for _ in 0..50 {
        if let Ok(snapshot) = sound.snapshot().await {
            if done(&snapshot) {
                return Ok(snapshot);
            }
        }
        Timer::after(Duration::from_millis(100)).await;
    }
    Err(format!("the snapshot didn't change: {:?}", sound.snapshot().await).into())
}

#[test]
#[ignore = "needs PipeWire and pipewire-pulse; run scripts/check-integration.sh"]
fn the_devices_and_defaults_are_read() -> Result<()> {
    run(async {
        let (_stand_in, sound) = with_devices()?;
        let snapshot = until(&sound, |snapshot| {
            snapshot.server.default_output.as_deref() == Some("speakers")
                && snapshot.server.default_input.as_deref() == Some("mic")
        })
        .await?;
        assert!(snapshot.server.name.contains("PipeWire"), "{snapshot:?}");
        let outputs: Vec<&str> = snapshot.outputs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(outputs, ["hdmi", "speakers"], "sorted by description");
        // The monitors of the outputs are no inputs.
        let inputs: Vec<&str> = snapshot.inputs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(inputs, ["mic"]);
        assert_eq!(
            snapshot.output().map(|d| d.description.as_str()),
            Some("Speakers")
        );
        assert_eq!(snapshot.input().map(|d| d.name.as_str()), Some("mic"));
        Ok(())
    })
}

#[test]
#[ignore = "needs PipeWire and pipewire-pulse; run scripts/check-integration.sh"]
fn the_default_output_and_input_are_switched() -> Result<()> {
    run(async {
        let (stand_in, sound) = with_devices()?;
        let mut changes = sound.changes();
        until(&sound, |snapshot| snapshot.server.default_output.is_some()).await?;
        sound.set_default_output("hdmi").await?;
        let snapshot = until(&sound, |snapshot| {
            snapshot.server.default_output.as_deref() == Some("hdmi")
        })
        .await?;
        assert_eq!(snapshot.output().map(|d| d.name.as_str()), Some("hdmi"));
        assert!(stand_in.pactl(&["info"])?.contains("Default Sink: hdmi"));
        // The change was reported.
        assert!(changes.next().await.is_some());
        stand_in.add_input("line", "Line_In")?;
        until(&sound, |snapshot| snapshot.inputs.len() == 2).await?;
        sound.set_default_input("line").await?;
        until(&sound, |snapshot| {
            snapshot.server.default_input.as_deref() == Some("line")
        })
        .await?;
        Ok(())
    })
}

#[test]
#[ignore = "needs PipeWire and pipewire-pulse; run scripts/check-integration.sh"]
fn volume_and_mute_reach_the_devices() -> Result<()> {
    run(async {
        let (stand_in, sound) = with_devices()?;
        if !stand_in.device_volume_works("speakers") {
            // PipeWire 1.4 keeps virtual devices at 100 %; the apps' volume is tested apart.
            eprintln!("note: this PipeWire doesn't change the volume of virtual devices");
            return Ok(());
        }
        let snapshot = until(&sound, |snapshot| snapshot.output().is_some()).await?;
        let speakers = snapshot.output().ok_or("no output")?.clone();
        // A balance to the left stays when the level changes.
        let left = Volume {
            channels: vec![NORMAL, NORMAL / 2],
        };
        sound.set_output_volume(speakers.index, &left).await?;
        sound
            .set_output_volume(speakers.index, &left.with_level(0.5))
            .await?;
        let snapshot = until(&sound, |snapshot| {
            snapshot
                .output()
                .is_some_and(|d| d.volume.channels == [NORMAL / 2, NORMAL / 4])
        })
        .await?;
        assert!((snapshot.output().ok_or("no output")?.volume.level() - 0.5).abs() < 1e-9);
        sound.set_output_muted(speakers.index, true).await?;
        until(&sound, |snapshot| {
            snapshot.output().is_some_and(|d| d.muted)
        })
        .await?;
        assert!(
            stand_in
                .pactl(&["get-sink-mute", "speakers"])?
                .contains("yes")
        );

        let mic = snapshot.input().ok_or("no input")?.clone();
        sound
            .set_input_volume(mic.index, &mic.volume.with_level(0.25))
            .await?;
        sound.set_input_muted(mic.index, true).await?;
        until(&sound, |snapshot| {
            snapshot
                .input()
                .is_some_and(|d| d.muted && (d.volume.level() - 0.25).abs() < 1e-3)
        })
        .await?;
        // A device that is gone answers with an error, not a hang.
        assert!(matches!(
            sound.set_output_muted(9999, true).await,
            Err(SoundError::Failed(_))
        ));
        Ok(())
    })
}

#[test]
#[ignore = "needs PipeWire and pipewire-pulse; run scripts/check-integration.sh"]
fn apps_are_listed_and_set_apart() -> Result<()> {
    run(async {
        let (mut stand_in, sound) = with_devices()?;
        stand_in.play("Music Player")?;
        stand_in.record("Recorder")?;
        let snapshot = until(&sound, |snapshot| {
            !snapshot.playing.is_empty() && !snapshot.recording.is_empty()
        })
        .await?;
        let music = &snapshot.playing[0];
        assert_eq!(music.app, "Music Player");
        let speakers = snapshot.output().ok_or("no output")?;
        assert_eq!(music.device, speakers.index);
        let volume = music.volume.clone().ok_or("no volume")?;
        sound
            .set_playing_volume(music.index, &volume.with_level(0.3))
            .await?;
        sound.set_playing_muted(music.index, true).await?;
        let index = music.index;
        until(&sound, |snapshot| {
            snapshot.playing.iter().any(|stream| {
                stream.index == index
                    && stream.muted
                    && stream
                        .volume
                        .as_ref()
                        .is_some_and(|v| (v.level() - 0.3).abs() < 1e-3)
            })
        })
        .await?;
        let recorder = &snapshot.recording[0];
        assert_eq!(recorder.app, "Recorder");
        sound.set_recording_muted(recorder.index, true).await?;

        // Our own level meter records too, but isn't an app.
        let mic = snapshot.input().ok_or("no input")?.name.clone();
        let meter = Meter::start(&sound, &mic)?;
        let mut levels = meter.levels();
        let level = levels.next().await.ok_or("no level")?;
        assert!((0.0..=1.0).contains(&level));
        let snapshot = sound.snapshot().await?;
        assert_eq!(snapshot.recording.len(), 1, "{:?}", snapshot.recording);
        drop(meter);
        Ok(())
    })
}

#[test]
#[ignore = "needs PipeWire and pipewire-pulse; run scripts/check-integration.sh"]
fn the_meter_works_while_nothing_records() -> Result<()> {
    run(async {
        let (_stand_in, sound) = with_devices()?;
        let snapshot = sound.snapshot().await?;
        assert!(snapshot.recording.is_empty(), "{:?}", snapshot.recording);
        // An input nobody records from is suspended; the meter must wake it, or it shows nothing.
        let mic = snapshot.input().ok_or("no input")?.name.clone();
        let meter = Meter::start(&sound, &mic)?;
        let mut levels = meter.levels();
        for _ in 0..3 {
            let level = levels.next().await.ok_or("no level")?;
            assert!((0.0..=1.0).contains(&level));
        }
        Ok(())
    })
}

#[test]
#[ignore = "needs PipeWire and pipewire-pulse; run scripts/check-integration.sh"]
fn reading_reports_no_change() -> Result<()> {
    run(async {
        let (_stand_in, sound) = with_devices()?;
        let mut changes = sound.changes();
        until(&sound, |snapshot| snapshot.output().is_some()).await?;
        // Whatever came up to now is taken; a quiet server then reports nothing, however often
        // it is read.
        Timer::after(Duration::from_millis(300)).await;
        while future::poll_once(changes.next()).await.is_some() {}
        for _ in 0..3 {
            sound.snapshot().await?;
        }
        Timer::after(Duration::from_millis(300)).await;
        assert!(
            future::poll_once(changes.next()).await.is_none(),
            "reading the snapshot reported a change"
        );
        Ok(())
    })
}

#[test]
#[ignore = "needs PipeWire and pipewire-pulse; run scripts/check-integration.sh"]
fn a_server_that_goes_away_is_followed_back() -> Result<()> {
    run(async {
        let (mut stand_in, sound) = with_devices()?;
        until(&sound, |snapshot| snapshot.output().is_some()).await?;
        stand_in.stop_server();
        let mut gone = false;
        for _ in 0..50 {
            if matches!(sound.snapshot().await, Err(SoundError::Unavailable)) {
                gone = true;
                break;
            }
            Timer::after(Duration::from_millis(100)).await;
        }
        assert!(gone, "the server's end wasn't noticed");
        let before = sound.generation();
        stand_in.start_server()?;
        // The same connection comes back by itself. The virtual devices were pipewire-pulse's own
        // and went with it.
        until(&sound, |snapshot| !snapshot.server.name.is_empty()).await?;
        assert!(sound.generation() > before, "no new connection counted");
        // And it reports changes again.
        let mut changes = sound.changes();
        while future::poll_once(changes.next()).await.is_some() {}
        stand_in.add_output("usb", "USB_Headset")?;
        until(&sound, |snapshot| snapshot.outputs.len() == 1).await?;
        assert!(changes.next().await.is_some());
        Ok(())
    })
}
