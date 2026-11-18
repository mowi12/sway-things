// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A sound server for tests: PipeWire, WirePlumber and pipewire-pulse of their own, in a temporary
//! directory with a private D-Bus bus, with virtual devices only (null sinks and sources) and
//! apps that play silence. Built with the `test-support` feature.
//!
//! WirePlumber's monitors of ALSA, Bluetooth and cameras are switched off, so the stand-in never
//! opens the machine's sound cards, even next to a running desktop. It needs `dbus-daemon`,
//! `pipewire`, `wireplumber`, `pipewire-pulse`, `pactl` and `pacat`, which the integration
//! containers have; the tests that use it are ignored by a plain `cargo test`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use tempfile::TempDir;

use super::Sound;

/// The error of a stand-in.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// WirePlumber without the monitors of real hardware.
const NO_HARDWARE: &str = "wireplumber.profiles = {
  main = {
    monitor.alsa = disabled
    monitor.alsa-midi = disabled
    monitor.bluez = disabled
    monitor.bluez-midi = disabled
    monitor.v4l2 = disabled
    monitor.libcamera = disabled
  }
}
";

/// A private sound server. Stops everything it started when dropped.
#[derive(Debug)]
pub struct StandIn {
    /// pipewire-pulse, which tests can stop and start again.
    pulse: Option<Child>,
    /// The other servers, the bus last, in the order they are stopped.
    servers: Vec<Child>,
    /// The apps playing or recording.
    apps: Vec<Child>,
    /// The address of the bus.
    bus: String,
    /// Holds the runtime directory, the configs and the logs.
    dir: TempDir,
}

impl StandIn {
    /// Starts the bus, PipeWire, WirePlumber and pipewire-pulse, without any device.
    ///
    /// # Errors
    ///
    /// If one of the programs is missing or the server doesn't answer within ten seconds.
    pub fn start() -> Result<Self, Error> {
        let dir = tempfile::tempdir()?;
        for sub in [
            "run",
            "home",
            "config/wireplumber/wireplumber.conf.d",
            "state",
        ] {
            fs::create_dir_all(dir.path().join(sub))?;
        }
        fs::write(
            dir.path()
                .join("config/wireplumber/wireplumber.conf.d/90-no-hardware.conf"),
            NO_HARDWARE,
        )?;
        let bus = format!("unix:path={}", dir.path().join("bus").display());
        let mut stand_in = Self {
            pulse: None,
            servers: Vec::new(),
            apps: Vec::new(),
            bus,
            dir,
        };
        let daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile"])
            .arg(format!("--address={}", stand_in.bus))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("can't start dbus-daemon: {error}"))?;
        stand_in.servers.push(daemon);
        stand_in.wait_for(&stand_in.dir.path().join("bus"))?;
        for program in ["pipewire", "wireplumber"] {
            let child = stand_in.spawn_logged(program)?;
            // The newest first, so they stop in reverse order.
            stand_in.servers.insert(0, child);
            if program == "pipewire" {
                stand_in.wait_for(&stand_in.dir.path().join("run/pipewire-0"))?;
            }
        }
        stand_in.start_server()?;
        Ok(stand_in)
    }

    /// Stops pipewire-pulse, as when the sound server goes away.
    pub fn stop_server(&mut self) {
        if let Some(mut pulse) = self.pulse.take() {
            let _ = pulse.kill();
            let _ = pulse.wait();
        }
        let _ = fs::remove_file(self.dir.path().join("run/pulse/native"));
    }

    /// Starts pipewire-pulse (again) and waits until it answers.
    ///
    /// # Errors
    ///
    /// If it is missing or doesn't answer within ten seconds.
    pub fn start_server(&mut self) -> Result<(), Error> {
        self.stop_server();
        self.pulse = Some(self.spawn_logged("pipewire-pulse")?);
        self.wait_for(&self.dir.path().join("run/pulse/native"))?;
        for _ in 0..100 {
            if self.pactl(&["info"]).is_ok() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(format!("the sound server didn't answer: {}", self.log()).into())
    }

    fn spawn_logged(&self, program: &str) -> Result<Child, Error> {
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.path().join(format!("{program}.log")))?;
        Ok(self
            .command(program)
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .map_err(|error| format!("can't start {program}: {error}"))?)
    }

    /// The server's address, for [`Sound::connect_to`].
    pub fn server(&self) -> String {
        format!(
            "unix:{}",
            self.dir.path().join("run/pulse/native").display()
        )
    }

    /// A connection to the stand-in.
    ///
    /// # Errors
    ///
    /// If libpulse can't be set up.
    pub fn sound(&self) -> Result<Sound, Error> {
        Ok(Sound::connect_to(Some(&self.server()))?)
    }

    /// Runs `pactl` with `args` against the stand-in and returns what it prints.
    ///
    /// # Errors
    ///
    /// If `pactl` is missing or fails.
    pub fn pactl(&self, args: &[&str]) -> Result<String, Error> {
        let output = self
            .command("pactl")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("can't run pactl: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "pactl {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Adds an output (a null sink) with the server's `name` and the shown `description`.
    ///
    /// # Errors
    ///
    /// If the server refuses.
    pub fn add_output(&self, name: &str, description: &str) -> Result<(), Error> {
        self.pactl(&[
            "load-module",
            "module-null-sink",
            &format!("sink_name={name}"),
            &format!("sink_properties=device.description={description}"),
        ])?;
        Self::wait_until(|| self.pactl(&["list", "short", "sinks"]), name)
    }

    /// Adds an input (a virtual source) with the server's `name` and the shown `description`.
    ///
    /// # Errors
    ///
    /// If the server refuses.
    pub fn add_input(&self, name: &str, description: &str) -> Result<(), Error> {
        self.pactl(&[
            "load-module",
            "module-null-sink",
            "media.class=Audio/Source/Virtual",
            &format!("sink_name={name}"),
            &format!("sink_properties=device.description={description}"),
        ])?;
        Self::wait_until(|| self.pactl(&["list", "short", "sources"]), name)
    }

    /// Whether the server lets the volume of the virtual output `name` change: PipeWire 1.4
    /// keeps its null sinks at 100 % and unmuted (1.6 doesn't), so tests of a device's volume
    /// only mean something where this holds. Leaves the volume at 100 %.
    pub fn device_volume_works(&self, name: &str) -> bool {
        let changed = self.pactl(&["set-sink-volume", name, "37%"]).is_ok()
            && Self::wait_for_text(|| self.pactl(&["get-sink-volume", name]), " 37%", 20).is_ok();
        let _ = self.pactl(&["set-sink-volume", name, "100%"]);
        changed
    }

    /// Starts an app named `app` that plays silence until the stand-in is dropped.
    ///
    /// # Errors
    ///
    /// If `pacat` is missing or the stream doesn't show up.
    pub fn play(&mut self, app: &str) -> Result<(), Error> {
        self.app(app, "--playback")?;
        Self::wait_until(|| self.pactl(&["list", "sink-inputs"]), app)
    }

    /// Starts an app named `app` that records from the default input until the stand-in is
    /// dropped.
    ///
    /// # Errors
    ///
    /// If `pacat` is missing or the stream doesn't show up.
    pub fn record(&mut self, app: &str) -> Result<(), Error> {
        self.app(app, "--record")?;
        Self::wait_until(|| self.pactl(&["list", "source-outputs"]), app)
    }

    fn app(&mut self, app: &str, direction: &str) -> Result<(), Error> {
        let (stdin, stdout) = if direction == "--playback" {
            (Stdio::from(fs::File::open("/dev/zero")?), Stdio::null())
        } else {
            (Stdio::null(), Stdio::from(fs::File::create("/dev/null")?))
        };
        let child = self
            .command("pacat")
            .args([direction, &format!("--client-name={app}")])
            .stdin(stdin)
            .stdout(stdout)
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("can't run pacat: {error}"))?;
        self.apps.push(child);
        Ok(())
    }

    /// The logs of the servers, for a failed test.
    pub fn log(&self) -> String {
        ["pipewire", "wireplumber", "pipewire-pulse"]
            .iter()
            .map(|program| {
                let log = fs::read_to_string(self.dir.path().join(format!("{program}.log")))
                    .unwrap_or_default();
                format!("{program}:\n{log}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A command with the environment of the stand-in: its own runtime, home, config and bus.
    fn command(&self, program: &str) -> Command {
        let path = |sub: &str| -> PathBuf { self.dir.path().join(sub) };
        let mut command = Command::new(program);
        command
            .env("XDG_RUNTIME_DIR", path("run"))
            .env("PIPEWIRE_RUNTIME_DIR", path("run"))
            .env("PULSE_RUNTIME_PATH", path("run/pulse"))
            .env("HOME", path("home"))
            .env("XDG_CONFIG_HOME", path("config"))
            .env("XDG_STATE_HOME", path("state"))
            .env("DBUS_SESSION_BUS_ADDRESS", &self.bus)
            .env("PULSE_SERVER", self.server())
            .env_remove("PIPEWIRE_REMOTE")
            .env_remove("PULSE_COOKIE");
        command
    }

    fn wait_for(&self, path: &Path) -> Result<(), Error> {
        for _ in 0..100 {
            if path.exists() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(format!("{} didn't appear: {}", path.display(), self.log()).into())
    }

    fn wait_until(read: impl Fn() -> Result<String, Error>, needle: &str) -> Result<(), Error> {
        Self::wait_for_text(read, needle, 100)
    }

    fn wait_for_text(
        read: impl Fn() -> Result<String, Error>,
        needle: &str,
        tries: u32,
    ) -> Result<(), Error> {
        for _ in 0..tries {
            if read()?.contains(needle) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(format!("{needle} didn't show up").into())
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        for child in self
            .apps
            .iter_mut()
            .chain(self.pulse.iter_mut())
            .chain(self.servers.iter_mut())
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
