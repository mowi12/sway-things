// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The question about a new display against a stand-in notification server on a private bus.
//!
//! These tests need `dbus-daemon`, so a plain `cargo test` skips them.
//! `scripts/check-integration.sh` runs them with `--ignored` in the containers, which have it.

use std::collections::HashMap;
use std::error::Error;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::future;
use sway_things_core::outputs::Outputs;
use sway_things_core::outputs::new_display::{Answer, Asked, Decision, Question, ask_on, decide};
use sway_things_core::outputs::profiles::{Profile, Profiles};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, interface};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// What the stand-in saw and does.
#[derive(Debug, Default)]
struct Seen {
    /// The summary, body and actions of each notification.
    notified: Vec<(String, String, Vec<String>)>,
    /// The app, its icon, the hints and the timeout of the last one.
    sender: Option<(String, String, Vec<String>, i32)>,
    /// The notifications closed by their sender.
    closed: Vec<u32>,
}

/// A notification server that answers each notification at once with `answer`: an action key,
/// or `None` to dismiss it.
struct Server {
    capabilities: Vec<String>,
    answer: Option<String>,
    seen: Arc<Mutex<Seen>>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<String> {
        self.capabilities.clone()
    }

    #[expect(clippy::too_many_arguments, reason = "the method of the specification")]
    async fn notify(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = record(
            &self.seen,
            (summary, body, actions),
            (app_name, app_icon, hints, expire_timeout),
        ) + replaces_id;
        // Another notification's answer first: it isn't taken for ours.
        let _ = Self::action_invoked(&emitter, id + 100, "mirror").await;
        match &self.answer {
            Some(key) => {
                let _ = Self::action_invoked(&emitter, id, key).await;
            }
            None => {
                let _ = Self::notification_closed(&emitter, id, 2).await;
            }
        }
        id
    }

    fn close_notification(&self, id: u32) {
        if let Ok(mut seen) = self.seen.lock() {
            seen.closed.push(id);
        }
    }

    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, key: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;
}

/// Notes a notification in `seen`; its id.
fn record(
    seen: &Mutex<Seen>,
    shown: (String, String, Vec<String>),
    sender: (String, String, HashMap<String, OwnedValue>, i32),
) -> u32 {
    let Ok(mut seen) = seen.lock() else {
        return 0;
    };
    seen.notified.push(shown);
    let (app, icon, hints, timeout) = sender;
    let mut hints: Vec<String> = hints.into_keys().collect();
    hints.sort_unstable();
    seen.sender = Some((app, icon, hints, timeout));
    u32::try_from(seen.notified.len()).unwrap_or(u32::MAX)
}

/// A private bus, stopped when dropped.
struct Bus {
    daemon: Child,
    address: String,
    _dir: tempfile::TempDir,
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn private_bus() -> Result<Bus> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("bus");
    // Without the session bus's service files: a real notification server must never be started
    // for a test (on the user's display).
    let config = dir.path().join("bus.conf");
    std::fs::write(
        &config,
        format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\"\n \
             \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig><type>session</type><listen>unix:path={}</listen><auth>EXTERNAL</auth>\
             <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
             <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n",
            socket.display()
        ),
    )?;
    let mut daemon = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .arg("--nofork")
        .arg("--nopidfile")
        .arg("--print-address")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut address = String::new();
    if let Some(stdout) = daemon.stdout.take() {
        BufReader::new(stdout).read_line(&mut address)?;
    }
    Ok(Bus {
        daemon,
        address: address.trim().to_owned(),
        _dir: dir,
    })
}

/// The stand-in on `bus` with `capabilities`, answering with `answer`; a connection of the
/// one who asks.
async fn serve(
    bus: &Bus,
    capabilities: &[&str],
    answer: Option<&str>,
) -> Result<(Connection, Connection, Arc<Mutex<Seen>>)> {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let server = Server {
        capabilities: capabilities.iter().map(|&text| text.to_owned()).collect(),
        answer: answer.map(str::to_owned),
        seen: Arc::clone(&seen),
    };
    let server_side = zbus::connection::Builder::address(bus.address.as_str())?
        .name("org.freedesktop.Notifications")?
        .serve_at("/org/freedesktop/Notifications", server)?
        .build()
        .await?;
    let asking = zbus::connection::Builder::address(bus.address.as_str())?
        .build()
        .await?;
    Ok((server_side, asking, seen))
}

/// The question about a projector plugged in to a laptop whose panel is remembered.
fn question() -> Result<Question> {
    let display = |name: &str, model: &str, x: i32| {
        format!(
            r#"{{"name": "{name}", "active": true, "primary": false, "make": "Make",
               "model": "{model}", "serial": "1", "scale": 1.0, "transform": "normal",
               "modes": [{{"width": 1920, "height": 1080, "refresh": 60000}}],
               "current_mode": {{"width": 1920, "height": 1080, "refresh": 60000}},
               "rect": {{"x": {x}, "y": 0, "width": 1920, "height": 1080}}}}"#
        )
    };
    let outputs = |list: &[&String]| -> Result<Outputs> {
        let list: Vec<&str> = list.iter().map(|text| text.as_str()).collect();
        Ok(Outputs::new(&serde_json::from_str::<Vec<_>>(&format!(
            "[{}]",
            list.join(",")
        ))?))
    };
    let laptop = display("eDP-1", "Panel", 0);
    let projector = display("HDMI-A-1", "Beamer", 1920);
    let mut profiles = Profiles::empty(std::path::Path::new("/nonexistent/displays.toml"));
    profiles.remember(Profile::capture(&outputs(&[&laptop])?).ok_or("no profile")?);
    match decide(&profiles, &outputs(&[&laptop, &projector])?) {
        Decision::Ask(question) => Ok(question),
        other => Err(format!("no question: {other:?}").into()),
    }
}

/// Runs a test, which fails instead of hanging.
fn run(test: impl Future<Output = Result<()>>) -> Result<()> {
    block_on(future::or(test, async {
        Timer::after(Duration::from_secs(30)).await;
        Err("the stand-in didn't answer within 30 seconds".into())
    }))
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn the_answer_is_the_action_chosen() -> Result<()> {
    run(async {
        for (key, answer) in [
            ("mirror", Answer::Mirror),
            ("default", Answer::Mirror),
            ("extend", Answer::Extend),
        ] {
            let bus = private_bus()?;
            let (_served, asking, seen) = serve(&bus, &["body", "actions"], Some(key)).await?;
            let got = ask_on(&asking, &question()?, Duration::from_secs(10)).await?;
            assert_eq!(got, Asked::Answer(answer), "{key}");
            let seen = seen.lock().unwrap();
            let (summary, body, actions) = &seen.notified[0];
            assert_eq!(summary, "New display: Make Beamer");
            assert!(body.contains("Make Panel"), "{body}");
            assert_eq!(
                actions,
                &["default", "Mirror", "mirror", "Mirror", "extend", "Extend"]
            );
            // Answered: the notification goes.
            assert_eq!(seen.closed, [1]);
            let (app, icon, hints, timeout) = seen.sender.clone().unwrap();
            assert_eq!(
                (app.as_str(), icon.as_str()),
                ("Sway Things", "io.github.mowi12.SwayThings")
            );
            assert_eq!(hints, ["desktop-entry"]);
            assert_eq!(timeout, 10_000);
        }
        Ok(())
    })
}

#[test]
#[ignore = "needs dbus-daemon; run scripts/check-integration.sh"]
fn dismissed_timed_out_or_without_actions_there_is_no_answer() -> Result<()> {
    run(async {
        let bus = private_bus()?;
        let (_served, asking, _) = serve(&bus, &["body", "actions"], None).await?;
        assert_eq!(
            ask_on(&asking, &question()?, Duration::from_secs(10)).await?,
            Asked::Nothing
        );
        // An unknown action: nothing until the time is up, then the notification is closed.
        let bus = private_bus()?;
        let (_served, asking, seen) = serve(&bus, &["actions"], Some("other")).await?;
        let got = ask_on(&asking, &question()?, Duration::from_millis(300)).await?;
        assert_eq!(got, Asked::Nothing);
        assert_eq!(seen.lock().unwrap().closed, [1]);
        // A server that shows no actions isn't asked.
        let bus = private_bus()?;
        let (_served, asking, seen) = serve(&bus, &["body"], Some("mirror")).await?;
        assert_eq!(
            ask_on(&asking, &question()?, Duration::from_secs(10)).await?,
            Asked::NoActions
        );
        assert_eq!(seen.lock().unwrap().notified.len(), 0);
        // Without a server, an error.
        let bus = private_bus()?;
        let lonely = zbus::connection::Builder::address(bus.address.as_str())?
            .build()
            .await?;
        let alone = ask_on(&lonely, &question()?, Duration::from_secs(1)).await;
        assert!(alone.is_err(), "{alone:?}");
        Ok(())
    })
}
