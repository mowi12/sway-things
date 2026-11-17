// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Showing one display on another with `wl-mirror`, per combination of displays.
//!
//! A display that mirrors another stays on and shows a fullscreen `wl-mirror` window of the
//! other one. [`apply`] ends the mirrors Sway Things started before (found in `/proc` by their
//! window title) and starts those the profile of the connected combination asks for. It runs no
//! shell; `wl-mirror` is started with an argument list and keeps running on its own, as a window
//! does, until it is ended here or its display goes.

use std::process::{Command, Stdio};

use super::Outputs;
use super::profiles::Profiles;

/// The title of the windows of the mirrors Sway Things starts, by which it finds them again.
pub const TITLE: &str = "Sway Things mirror";

/// How a mirror fits the display it is shown on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scaling {
    /// Whole, with bars where the shapes differ.
    #[default]
    Fit,
    /// Filling the display, cut where the shapes differ.
    Cover,
    /// Only by whole multiples, sharp.
    Exact,
}

impl Scaling {
    /// Every scaling, in the order a page offers them.
    pub const ALL: [Self; 3] = [Self::Fit, Self::Cover, Self::Exact];

    /// The name in `displays.toml` and for `wl-mirror --scaling`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fit => "fit",
            Self::Cover => "cover",
            Self::Exact => "exact",
        }
    }

    /// The scaling called `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scaling| scaling.name() == name)
    }
}

/// Which display another one mirrors, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirror {
    /// The display shown, by its name in the settings.
    pub of: String,
    /// How it fits.
    pub scaling: Scaling,
}

/// Whether `wl-mirror` is installed: an executable of that name in a directory of `$PATH`.
pub fn installed() -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        std::fs::metadata(dir.join("wl-mirror"))
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    })
}

/// Ends the mirrors Sway Things started, and starts those the profile of the combination of
/// `outputs` asks for. Returns how many it started. Without `wl-mirror`, or for a display or a
/// mirrored one that isn't on, nothing is started.
pub fn apply(profiles: &Profiles, outputs: &Outputs) -> usize {
    stop();
    let names: Vec<&str> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .map(super::Output::identifier)
        .collect();
    let Some(profile) = profiles.of(&names) else {
        return 0;
    };
    if !installed() {
        return 0;
    }
    let mut started = 0;
    for (target, setup) in profile.outputs() {
        let Some(mirror) = &setup.mirror else {
            continue;
        };
        let on = |name: &str| outputs.find(name).filter(|output| output.is_enabled());
        let (Some(target), Some(source)) = (on(target), on(&mirror.of)) else {
            continue;
        };
        let spawned = Command::new("wl-mirror")
            .args(["--fullscreen-output", target.name()])
            .args(["--scaling", mirror.scaling.name()])
            .args(["--title", TITLE])
            .arg(source.name())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if spawned.is_ok() {
            started += 1;
        }
    }
    started
}

/// Ends the mirrors Sway Things started: the `wl-mirror` processes of this user with our title.
/// Reads `/proc` and runs `kill` with their numbers.
pub fn stop() {
    use std::os::unix::fs::MetadataExt;
    let Ok(me) = std::fs::metadata("/proc/self").map(|meta| meta.uid()) else {
        return;
    };
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return;
    };
    let ours: Vec<String> = processes
        .filter_map(Result::ok)
        .filter(|process| process.metadata().is_ok_and(|meta| meta.uid() == me))
        .filter(|process| {
            let path = process.path();
            let named = std::fs::read_to_string(path.join("comm"))
                .is_ok_and(|name| name.trim_end() == "wl-mirror");
            named
                && std::fs::read(path.join("cmdline")).is_ok_and(|line| {
                    line.split(|byte| *byte == 0)
                        .any(|argument| argument == TITLE.as_bytes())
                })
        })
        .filter_map(|process| process.file_name().into_string().ok())
        .filter(|pid| pid.bytes().all(|byte| byte.is_ascii_digit()))
        .collect();
    if ours.is_empty() {
        return;
    }
    let _ = Command::new("kill")
        .args(&ours)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::Scaling;

    #[test]
    fn scalings_have_the_names_wl_mirror_knows() {
        for scaling in Scaling::ALL {
            assert_eq!(Scaling::from_name(scaling.name()), Some(scaling));
        }
        assert_eq!(Scaling::from_name("stretch"), None);
    }
}
