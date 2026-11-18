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

/// `no_focus` for the windows of the mirrors Sway Things starts: a picture that starts doesn't
/// take the focus from where the user works.
pub fn no_focus_command() -> crate::command::SwayCmd {
    crate::command::SwayCmd::no_focus(
        crate::command::Criteria::new().exact(crate::command::CriteriaKey::Title, TITLE),
    )
}

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
    /// The workspaces that stood on the display when its picture started and moved to the one
    /// shown ([`gather_workspaces`]): they go back when it shows its own content again.
    pub gathered: Vec<String>,
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

/// Has the mirrors run that the profile of the combination of `outputs` asks for: ends the
/// mirrors Sway Things started in this session that it doesn't ask for, and starts those that
/// don't run yet, so running it twice changes nothing. Returns how many it started. A mirror
/// whose display or source isn't on, or that is part of a chain or a cycle (a display that is
/// shown elsewhere and shows another one itself), isn't started; without `wl-mirror`, none is.
pub fn apply(profiles: &Profiles, outputs: &Outputs) -> usize {
    // One at a time in a session: a kanshi hook and the app can run at once, and a mirror
    // started by one shows in `/proc` only a moment later. Whoever comes second leaves it to
    // the first, which has the same profiles.
    let Some(_lock) = Lock::take() else {
        return 0;
    };
    let wanted = wanted(profiles, outputs);
    let running = running();
    // Not wanted, or a second copy of one that is: ended.
    let mut kept: Vec<&Vec<String>> = Vec::new();
    let unwanted: Vec<String> = running
        .iter()
        .filter(|(_, arguments)| {
            if wanted.contains(arguments) && !kept.contains(&arguments) {
                kept.push(arguments);
                false
            } else {
                true
            }
        })
        .map(|(pid, _)| pid.clone())
        .collect();
    kill(&unwanted);
    let mut started = 0;
    for arguments in wanted
        .iter()
        .filter(|arguments| !running.iter().any(|(_, have)| have == *arguments))
    {
        // `setsid -f` starts it in a session of its own and returns at once, so it neither
        // ends with the caller nor stays behind as a child that is never waited for.
        let spawned = Command::new("setsid")
            .arg("-f")
            .arg("wl-mirror")
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if spawned.is_ok_and(|status| status.success()) {
            started += 1;
        }
    }
    started
}

/// Moves the workspaces that stand on a display about to show another one's picture to that other
/// display, as the profile of the combination of displays connected now asks: they stay within
/// reach instead of hiding behind the picture. Call it before [`apply`], whose window goes on
/// the display's own workspace. The profile notes them ([`Mirror::gathered`]), to be saved by the
/// caller, so they go back when the picture ends ([`give_back_workspaces`]). Returns how many
/// commands were sent.
///
/// # Errors
///
/// The IPC errors of asking sway and of the commands.
pub async fn gather_workspaces(
    client: &mut crate::ipc::Client,
    profiles: &mut Profiles,
) -> Result<usize, crate::ipc::IpcError> {
    let outputs = Outputs::new(&client.outputs().await?);
    let names: Vec<&str> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .map(super::Output::identifier)
        .collect();
    let Some(profile) = profiles.of(&names) else {
        return Ok(0);
    };
    let open = client.workspaces().await?;
    let running = running();
    let mut placement = std::collections::BTreeMap::new();
    let mut gathered: Vec<(String, Vec<String>)> = Vec::new();
    for (target, setup) in profile.outputs() {
        let Some(mirror) = &setup.mirror else {
            continue;
        };
        let on = |name: &str| outputs.find(name).filter(|output| output.is_enabled());
        let (Some(target), Some(source)) = (on(target), on(&mirror.of)) else {
            continue;
        };
        // A picture that is shown already took them along: what stands there now is its own.
        let shown = running.iter().any(|(_, arguments)| {
            arguments
                .windows(2)
                .any(|pair| pair[0] == "--fullscreen-output" && pair[1] == target.name())
        });
        if shown {
            continue;
        }
        let moved: Vec<String> = open
            .iter()
            .filter(|open| open.output == target.name())
            .map(|open| open.name.clone())
            .collect();
        for name in &moved {
            placement.insert(name.clone(), source.identifier().to_owned());
        }
        gathered.push((target.identifier().to_owned(), moved));
    }
    if placement.is_empty() {
        return Ok(0);
    }
    if let Some(profile) = profiles.of_mut(&names) {
        for (target, moved) in gathered {
            profile.note_gathered(&target, &moved);
        }
    }
    let Ok(commands) = super::workspaces::commands(&placement, &outputs, &open) else {
        return Ok(0);
    };
    for command in &commands {
        client.run(command).await?;
    }
    Ok(commands.len())
}

/// Moves the workspaces `names` back to the display `target` (by its name in the settings), as
/// far as they are open: they stood there before it showed another display's picture.
///
/// # Errors
///
/// The IPC errors of asking sway and of the commands.
pub async fn give_back_workspaces(
    client: &mut crate::ipc::Client,
    target: &str,
    names: &[String],
) -> Result<usize, crate::ipc::IpcError> {
    if names.is_empty() {
        return Ok(0);
    }
    let outputs = Outputs::new(&client.outputs().await?);
    let open = client.workspaces().await?;
    let placement: std::collections::BTreeMap<String, String> = names
        .iter()
        .filter(|name| open.iter().any(|open| open.name == **name))
        .map(|name| (name.clone(), target.to_owned()))
        .collect();
    let Ok(mut commands) = super::workspaces::commands(&placement, &outputs, &open) else {
        return Ok(0);
    };
    // The display shows the first of them again rather than an empty one, and the focus goes
    // back to where it was.
    let first = names
        .iter()
        .find(|name| placement.contains_key(*name))
        .and_then(|name| crate::command::SwayText::new(name).ok());
    let focused = open
        .iter()
        .find(|open| open.focused)
        .and_then(|open| crate::command::SwayText::new(&open.name).ok());
    if let (Some(first), Some(focused)) = (first, focused) {
        for name in [first, focused] {
            commands.push(
                crate::command::SwayCmd::new("workspace")
                    .keyword("--no-auto-back-and-forth")
                    .text(name),
            );
        }
    }
    for command in &commands {
        client.run(command).await?;
    }
    Ok(commands.len())
}

/// The lock of [`apply`]: a file in `$XDG_RUNTIME_DIR`, made only if it isn't there, and
/// removed when dropped. One older than ten seconds was left behind and is taken over.
struct Lock(std::path::PathBuf);

impl Lock {
    fn take() -> Option<Self> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from)?;
        let path = dir.join("sway-things-mirror.lock");
        let created = || {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .is_ok()
        };
        if created() {
            return Some(Self(path));
        }
        let stale = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > std::time::Duration::from_secs(10));
        if stale {
            let _ = std::fs::remove_file(&path);
            if created() {
                return Some(Self(path));
            }
        }
        None
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The arguments of `wl-mirror` for each mirror the profile of the combination of `outputs`
/// asks for.
fn wanted(profiles: &Profiles, outputs: &Outputs) -> Vec<Vec<String>> {
    let names: Vec<&str> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .map(super::Output::identifier)
        .collect();
    let Some(profile) = profiles.of(&names) else {
        return Vec::new();
    };
    if !installed() {
        return Vec::new();
    }
    let mirrored: Vec<&str> = profile
        .outputs()
        .filter_map(|(_, setup)| Some(setup.mirror.as_ref()?.of.as_str()))
        .collect();
    let mirroring: Vec<&str> = profile
        .outputs()
        .filter(|(_, setup)| setup.mirror.is_some())
        .map(|(name, _)| name)
        .collect();
    profile
        .outputs()
        .filter_map(|(target, setup)| {
            let mirror = setup.mirror.as_ref()?;
            // No chains, no cycles: a display shown elsewhere shows its own content.
            if mirrored.contains(&target) || mirroring.contains(&mirror.of.as_str()) {
                return None;
            }
            let on = |name: &str| outputs.find(name).filter(|output| output.is_enabled());
            let (target, source) = (on(target)?, on(&mirror.of)?);
            Some(vec![
                "--fullscreen-output".to_owned(),
                target.name().to_owned(),
                "--scaling".to_owned(),
                mirror.scaling.name().to_owned(),
                "--title".to_owned(),
                TITLE.to_owned(),
                source.name().to_owned(),
            ])
        })
        .collect()
}

/// The mirrors Sway Things started in this session: the process number and the arguments of
/// each `wl-mirror` with our title.
fn running() -> Vec<(String, Vec<String>)> {
    super::session_processes("wl-mirror")
        .into_iter()
        .filter_map(|path| {
            let line = std::fs::read(path.join("cmdline")).ok()?;
            let mut arguments: Vec<String> = line
                .split(|byte| *byte == 0)
                .filter(|argument| !argument.is_empty())
                .map(|argument| String::from_utf8_lossy(argument).into_owned())
                .collect();
            if !arguments.iter().any(|argument| argument == TITLE) {
                return None;
            }
            // Without the program itself.
            if !arguments.is_empty() {
                arguments.remove(0);
            }
            let pid = path.file_name()?.to_str()?.to_owned();
            pid.bytes()
                .all(|byte| byte.is_ascii_digit())
                .then_some((pid, arguments))
        })
        .collect()
}

/// Ends the mirrors Sway Things started in this session.
pub fn stop() {
    let pids: Vec<String> = running().into_iter().map(|(pid, _)| pid).collect();
    kill(&pids);
}

/// Ends the processes `pids` with `kill`.
fn kill(pids: &[String]) {
    if pids.is_empty() {
        return;
    }
    let _ = Command::new("kill")
        .args(pids)
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
