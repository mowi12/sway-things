// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The workspaces placed by Sway Things while the user's own kanshi profile sets up the
//! displays, against a real sway and a real kanshi: the user's display setup applies, their own
//! `exec swaymsg workspace` lines don't, and the placement survives a reload of sway and an
//! edit of their profile.
//!
//! This test needs sway and kanshi, so a plain `cargo test` skips it.
//! `scripts/check-integration.sh` runs it with `--ignored`. It starts a headless sway of its own
//! (two displays, its own runtime directory), so the session the other tests share keeps its one
//! display and no kanshi. Not on SwayFX, which can't apply a display setup in a container.

use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// How long kanshi and the hooks get to do something.
const PATIENCE: Duration = Duration::from_secs(20);

/// The user's kanshi config: our include at the top, then their profile for the two displays,
/// with the right one scaled and their own lines that put workspaces 1 to 4 on it.
fn user_config(ours: &Path, scale: &str) -> String {
    let mut text = format!(
        "include \"{}\"\n\nprofile 'My Desk' {{\n    output HEADLESS-1 enable scale 1 position \
         0,0\n    output HEADLESS-2 enable scale {scale} position 1920,0\n",
        ours.display()
    );
    for number in 1..=4 {
        let _ = writeln!(
            text,
            "    exec swaymsg workspace {number} output HEADLESS-2, workspace {number}, move \
             workspace to output HEADLESS-2"
        );
    }
    text.push_str("}\n");
    text
}

/// A headless sway of the test's own, ended when dropped.
struct Sway {
    child: Child,
    socket: PathBuf,
    env: Vec<(&'static str, PathBuf)>,
}

impl Sway {
    /// Starts sway with `config` in the runtime directory `runtime`, the files of Sway Things
    /// and kanshi under `config_home`, and the binary of Sway Things on its `PATH`, so kanshi
    /// started by it runs the hooks.
    fn start(runtime: &Path, config_home: &Path, state_home: &Path, config: &Path) -> Result<Self> {
        let bin = PathBuf::from(env!("CARGO_BIN_EXE_sway-things"));
        let bin_dir = bin.parent().ok_or("no directory of the binary")?;
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin_dir.to_owned()];
        paths.extend(std::env::split_paths(&path));
        let env = vec![
            ("XDG_RUNTIME_DIR", runtime.to_owned()),
            ("XDG_CONFIG_HOME", config_home.to_owned()),
            ("XDG_STATE_HOME", state_home.to_owned()),
            ("PATH", PathBuf::from(std::env::join_paths(paths)?)),
        ];
        let log = fs::File::create(runtime.join("sway.log"))?;
        let child = Command::new("sway")
            .arg("--config")
            .arg(config)
            .env_remove("SWAYSOCK")
            .env_remove("WAYLAND_DISPLAY")
            .env("WLR_BACKENDS", "headless")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .envs(env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        let mut sway = Self {
            child,
            socket: PathBuf::new(),
            env,
        };
        sway.socket = wait_for(|| {
            fs::read_dir(runtime)
                .ok()?
                .flatten()
                .map(|entry| entry.path())
                .find(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("sway-ipc."))
                })
        })
        .ok_or("sway didn't start")?;
        // Its Wayland display, as sway gives it to what it starts: kanshictl finds kanshi by it.
        let display = fs::read_dir(runtime)?
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .find(|name| name.starts_with("wayland-") && Path::new(name).extension().is_none())
            .ok_or("sway has no Wayland display")?;
        sway.env.push(("WAYLAND_DISPLAY", PathBuf::from(display)));
        Ok(sway)
    }

    /// `swaymsg` against this sway.
    fn msg(&self, args: &[&str]) -> Result<Output> {
        Ok(Command::new("swaymsg")
            .arg("--socket")
            .arg(&self.socket)
            .args(args)
            .stdin(Stdio::null())
            .output()?)
    }

    fn json(&self, kind: &str) -> Result<Value> {
        let output = self.msg(&["--raw", "--type", kind])?;
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    /// The scale of the output `name`.
    fn scale(&self, name: &str) -> Option<f64> {
        self.json("get_outputs")
            .ok()?
            .as_array()?
            .iter()
            .find(|output| output["name"] == name)?["scale"]
            .as_f64()
    }

    /// The output the workspace `number` is on, if it is open.
    fn workspace_output(&self, number: i64) -> Option<String> {
        self.json("get_workspaces")
            .ok()?
            .as_array()?
            .iter()
            .find(|workspace| workspace["num"] == number)?["output"]
            .as_str()
            .map(str::to_owned)
    }

    /// The `sway-things` binary in this session, as a hook or the user runs it: with sway's
    /// socket and Wayland display.
    fn sway_things(&self, args: &[&str]) -> Result<Output> {
        Ok(Command::new(env!("CARGO_BIN_EXE_sway-things"))
            .args(args)
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .env("SWAYSOCK", &self.socket)
            .stdin(Stdio::null())
            .output()?)
    }

    fn log(&self) -> String {
        self.env
            .iter()
            .find(|(key, _)| *key == "XDG_RUNTIME_DIR")
            .and_then(|(_, runtime)| fs::read_to_string(runtime.join("sway.log")).ok())
            .unwrap_or_default()
    }
}

impl Drop for Sway {
    fn drop(&mut self) {
        let _ = self.msg(&["exit"]);
        if wait_for(|| self.child.try_wait().ok().flatten()).is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// `check` once it gives something, trying for [`PATIENCE`].
fn wait_for<T>(mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let until = Instant::now() + PATIENCE;
    loop {
        if let Some(found) = check() {
            return Some(found);
        }
        if Instant::now() > until {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether a process named `name` runs.
fn runs(name: &str) -> bool {
    fs::read_dir("/proc").is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            fs::read_to_string(entry.path().join("comm")).is_ok_and(|comm| comm.trim() == name)
        })
    })
}

/// Whether `program` is on `PATH`.
fn installed(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Waits until the workspaces stand round robin from left to right on the two displays: the odd
/// ones on HEADLESS-1, the even ones on HEADLESS-2. Workspaces 3 and 4 are opened to see where
/// sway puts them, then left again.
fn assert_round_robin(sway: &Sway, when: &str) -> Result<()> {
    let placed =
        wait_for(|| (sway.workspace_output(1).as_deref() == Some("HEADLESS-1")).then_some(()));
    assert!(
        placed.is_some(),
        "{when}: workspace 1 is on {:?}\n{}",
        sway.workspace_output(1),
        sway.log()
    );
    // The user's own lines would move them a moment after kanshi applied the profile.
    std::thread::sleep(Duration::from_secs(2));
    for (number, display) in [(1, "HEADLESS-1"), (3, "HEADLESS-1"), (4, "HEADLESS-2")] {
        sway.msg(&["workspace", &number.to_string()])?;
        assert_eq!(
            sway.workspace_output(number).as_deref(),
            Some(display),
            "{when}: workspace {number}"
        );
    }
    sway.msg(&["workspace", "1"])?;
    Ok(())
}

#[test]
#[ignore = "needs sway and kanshi; run scripts/check-integration.sh"]
fn the_workspaces_are_placed_while_the_users_own_kanshi_profile_sets_up_the_displays() -> Result<()>
{
    assert!(installed("kanshi"), "kanshi isn't installed");
    // SwayFX can't allocate buffers in a container (its renderer needs a render node), so it
    // can't apply a display setup, the scale of the user's profile here. What is tested is the
    // same on sway.
    let version = Command::new("sway").arg("--version").output()?;
    if String::from_utf8_lossy(&version.stdout)
        .to_lowercase()
        .contains("swayfx")
    {
        eprintln!("Skipped: SwayFX can't apply a display setup in a container");
        return Ok(());
    }
    let home = tempfile::tempdir()?;
    let runtime = home.path().join("runtime");
    let config_home = home.path().join("config");
    let state_home = home.path().join("state");
    for dir in [&runtime, &config_home.join("kanshi"), &state_home] {
        fs::create_dir_all(dir)?;
    }
    // Wayland wants a runtime directory only its user can enter.
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))?;
    let ours = config_home.join("kanshi/sway-things.conf");
    let kanshi_config = config_home.join("kanshi/config");
    fs::write(&kanshi_config, user_config(&ours, "2"))?;
    let sway_config = home.path().join("sway.conf");
    fs::write(
        &sway_config,
        format!(
            "include {}/sway/sway-things/*.conf\n",
            config_home.display()
        ),
    )?;

    let sway = Sway::start(&runtime, &config_home, &state_home, &sway_config)?;
    let created = sway.msg(&["create_output"])?;
    assert!(created.status.success(), "{created:?}");

    // Workspaces placed round robin: the file of ours gets a copy of the user's profile, and
    // sway starts kanshi, which applies the copy.
    let set = sway.sway_things(&["set", "workspaces.placement", "round_robin"])?;
    assert_eq!(set.status.code(), Some(0), "{set:?}");
    let written = fs::read_to_string(&ours)?;
    assert!(
        written.contains("# Your profile \u{201c}My Desk\u{201d}"),
        "{written}"
    );
    assert!(
        written.contains("    output HEADLESS-2 enable scale 2 position 1920,0\n"),
        "{written}"
    );
    assert!(
        written.contains("    exec sway-things hook workspaces\n"),
        "{written}"
    );
    assert!(!written.contains("swaymsg"), "{written}");
    if wait_for(|| runs("kanshi").then_some(())).is_none() {
        sway.msg(&["exec", "kanshi"])?;
    }
    // The user's display setup applies.
    let scaled = wait_for(|| (sway.scale("HEADLESS-2") == Some(2.0)).then_some(()));
    assert!(
        scaled.is_some(),
        "HEADLESS-2 has scale {:?}\n{}",
        sway.scale("HEADLESS-2"),
        sway.log()
    );
    assert_round_robin(&sway, "after kanshi applied the copy")?;

    // A reload of sway: kanshi applies the profile again, and the placement stays.
    let reloaded = sway.msg(&["reload"])?;
    assert!(reloaded.status.success(), "{reloaded:?}");
    std::thread::sleep(Duration::from_secs(2));
    assert_round_robin(&sway, "after a reload of sway")?;

    // The user changes their profile: the next hook makes the copy again, and kanshi applies it.
    fs::write(&kanshi_config, user_config(&ours, "1.5"))?;
    let hook = sway.sway_things(&["hook", "workspaces"])?;
    assert_eq!(hook.status.code(), Some(0), "{hook:?}");
    let copied = fs::read_to_string(&ours)?;
    assert!(
        copied.contains("    output HEADLESS-2 enable scale 1.5 position 1920,0\n"),
        "{copied}"
    );
    if installed("kanshictl") {
        let rescaled = wait_for(|| (sway.scale("HEADLESS-2") == Some(1.5)).then_some(()));
        assert!(
            rescaled.is_some(),
            "HEADLESS-2 has scale {:?}\n{}",
            sway.scale("HEADLESS-2"),
            sway.log()
        );
        assert_round_robin(&sway, "after the user's profile changed")?;
    }
    Ok(())
}
