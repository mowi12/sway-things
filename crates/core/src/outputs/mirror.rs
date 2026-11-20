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
use crate::command::{SwayCmd, SwayText};

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
    /// The sway session they were gathered in (its IPC socket): after another login, the names
    /// are other workspaces and nothing goes back.
    pub session: String,
}

/// Whether `wl-mirror` is installed: an executable of that name in a directory of `$PATH`.
pub fn installed() -> bool {
    crate::delegate::find_program("wl-mirror", std::env::var_os("PATH").as_deref()).is_some()
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
    let Some(_lock) = super::SessionLock::take("sway-things-mirror.lock") else {
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

/// Whether a picture the profile of the combination of `outputs` asks for doesn't run: its
/// `wl-mirror` ended (it crashed, or its window was closed) or never started. Reads `/proc`;
/// nothing is started.
pub fn missing(profiles: &Profiles, outputs: &Outputs) -> bool {
    let running = running();
    wanted(profiles, outputs)
        .iter()
        .any(|arguments| !running.iter().any(|(_, have)| have == arguments))
}

/// Starts the pictures the profile of the combination connected now asks for, and ends the
/// others Sway Things started: the workspaces of a display about to show a picture go to the
/// display shown first ([`gather_workspaces`], noted in `profiles` and saved), the picture
/// doesn't take the focus, and a display whose picture ended shows a workspace like any other
/// again ([`leave_ended`]). Returns how many pictures it started. Nothing happens while no
/// combination mirrors.
///
/// # Errors
///
/// The IPC errors of asking sway and of the commands.
pub async fn start(
    client: &mut crate::ipc::Client,
    profiles: &mut Profiles,
) -> Result<usize, crate::ipc::IpcError> {
    if !profiles.iter().any(super::profiles::Profile::mirrors) {
        return Ok(0);
    }
    // One at a time in a session, the hooks and the app alike: each notes the workspaces it
    // moves in `displays.toml`, so the profiles are read again once it is this one's turn. A
    // holder that doesn't finish is waited for at most fifteen seconds.
    let _turn = super::SessionLock::wait_for(
        "sway-things-mirror-start.lock",
        std::time::Duration::from_secs(15),
    )
    .await;
    if profiles.path().exists() {
        if let Ok(fresh) = Profiles::load(profiles.path()) {
            *profiles = fresh;
        }
    }
    // Noted also when a command failed: the workspaces that did move find their way back.
    match gather_workspaces(client, profiles).await {
        Ok(0) => {}
        Ok(_) => {
            let _ = profiles.save();
        }
        Err(error) => {
            let _ = profiles.save();
            return Err(error);
        }
    }
    let found = Outputs::new(&client.outputs().await?);
    let _ = client.run(&no_focus_command()).await;
    let started = apply_keeping_focus(client, profiles, &found).await?;
    let _ = leave_ended(client, profiles).await;
    Ok(started)
}

/// [`apply`], with the focus given back to the workspace that had it if the window of a picture
/// that starts takes it ([`keep_focus`]).
///
/// # Errors
///
/// The IPC errors of asking sway and of the command.
pub async fn apply_keeping_focus(
    client: &mut crate::ipc::Client,
    profiles: &Profiles,
    outputs: &Outputs,
) -> Result<usize, crate::ipc::IpcError> {
    // Where the user works, to go back to.
    let focused = client
        .workspaces()
        .await?
        .into_iter()
        .find(|open| open.focused && open.name != workspace_name(&open.output))
        .map(|open| open.name);
    let windows = pictures_shown(&client.tree().await?);
    let started = apply(profiles, outputs);
    if started > 0 {
        if let Some(focused) = focused {
            keep_focus(client, &focused, windows + started).await?;
        }
    }
    Ok(started)
}

/// How many windows of pictures sway shows in `node`, its tree.
fn pictures_shown(node: &crate::ipc::types::Node) -> usize {
    let own = usize::from(node.name.as_deref() == Some(TITLE));
    own + node
        .nodes
        .iter()
        .chain(&node.floating_nodes)
        .map(pictures_shown)
        .sum::<usize>()
}

/// Gives the focus back to the workspace `focused` if the window of a picture that just started
/// takes it: sway gives a fullscreen window the focus when it asks for its display after it
/// appeared, whatever `no_focus` says. Looks until `windows` pictures are shown and a moment
/// after, at most three seconds; stops when the user went to another workspace meanwhile, and
/// goes back only to a workspace that is still there.
async fn keep_focus(
    client: &mut crate::ipc::Client,
    focused: &str,
    windows: usize,
) -> Result<(), crate::ipc::IpcError> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut shown_at = None;
    while std::time::Instant::now() < until {
        async_io::Timer::after(std::time::Duration::from_millis(100)).await;
        let open = client.workspaces().await?;
        let Some(now) = open.iter().find(|open| open.focused) else {
            continue;
        };
        if now.name == workspace_name(&now.output) {
            if open.iter().any(|open| open.name == focused) {
                if let Ok(name) = SwayText::new(focused) {
                    let back = SwayCmd::new("workspace")
                        .keyword("--no-auto-back-and-forth")
                        .text(name);
                    client.run(&back).await?;
                }
            }
            return Ok(());
        }
        // The user went elsewhere: that is where they work now.
        if now.name != focused {
            return Ok(());
        }
        if pictures_shown(&client.tree().await?) >= windows {
            let at = *shown_at.get_or_insert_with(std::time::Instant::now);
            if at.elapsed() >= std::time::Duration::from_millis(300) {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Starts a picture again that the profile of the combination connected now asks for but that
/// doesn't run ([`missing`]), as [`start`] does; while every picture runs, or none is asked
/// for, nothing is sent and no picture of another combination is ended. Sway Things keeps
/// nothing running to notice a picture that went: the hooks and the app call this where they
/// run anyway. Returns how many pictures it started.
///
/// # Errors
///
/// The IPC errors of asking sway and of the commands.
pub async fn heal(
    client: &mut crate::ipc::Client,
    profiles: &mut Profiles,
) -> Result<usize, crate::ipc::IpcError> {
    if !profiles.iter().any(super::profiles::Profile::mirrors) {
        return Ok(0);
    }
    let found = Outputs::new(&client.outputs().await?);
    if !missing(profiles, &found) {
        return Ok(0);
    }
    start(client, profiles).await
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
    // Each display that shows a picture: its connector, where it goes apart, and whether the
    // picture starts now.
    let mut apart: Vec<Apart> = Vec::new();
    for (target, source, _) in startable(profile, &outputs) {
        let shown = running.iter().any(|(_, arguments)| {
            arguments
                .windows(2)
                .any(|pair| pair[0] == "--fullscreen-output" && pair[1] == target.name())
        });
        // Only workspaces with windows: an empty one (the one sway makes on a display plugged
        // in) goes away by itself, and its name would stand for other work later. Also one that
        // came there while the picture is shown (the user's config or another tool put it
        // there): it would hide the picture, out of the pointer's reach.
        let own = workspace_name(target.name());
        let moved: Vec<String> = open
            .iter()
            .filter(|open| open.output == target.name() && open.name != own)
            .filter(|open| {
                open.representation
                    .as_deref()
                    .is_some_and(|windows| !windows.is_empty())
            })
            .map(|open| open.name.clone())
            .collect();
        for name in &moved {
            placement.insert(name.clone(), source.identifier().to_owned());
        }
        // Back apart, if something (a profile of the user's, say) put it among the others.
        let place = profile.apart(target.identifier());
        let stands = target.rect().map(|rect| (rect.x, rect.y));
        // Where it stands isn't known: left where it is.
        let in_place = place.is_none() || stands.is_none() || place == stands;
        if shown && moved.is_empty() && in_place {
            continue;
        }
        gathered.push((target.identifier().to_owned(), moved));
        apart.push(Apart {
            name: target.name().to_owned(),
            place: place.filter(|_| !in_place),
            fresh: !shown,
        });
    }
    if apart.is_empty() {
        return Ok(0);
    }
    let session = client.socket().display().to_string();
    if let Some(profile) = profiles.of_mut(&names) {
        for (target, moved) in gathered {
            profile.note_gathered(&target, &moved, &session);
        }
    }
    let Ok(mut commands) = super::workspaces::commands(&placement, &outputs, &open) else {
        return Ok(0);
    };
    commands.extend(set_apart(&apart, &open).unwrap_or_default());
    // One message: nobody else's command comes between a focus and the move after it.
    client.run_together(&commands).await?;
    Ok(commands.len())
}

/// A display that shows another one's picture, for [`set_apart`].
struct Apart {
    /// Its connector.
    name: String,
    /// Where it goes, if it doesn't stand there already.
    place: Option<(i32, i32)>,
    /// Whether the picture starts now: the display gets its workspace for it then.
    fresh: bool,
}

/// The commands that set each display of `apart` apart from the others and, where the picture
/// starts now, give it a workspace of its own for the picture, named after it, so no workspace
/// number leads there; then the focus goes back to the workspace of `open` that had it (which
/// moves the pointer off the display, if it was there).
fn set_apart(
    apart: &[Apart],
    open: &[crate::ipc::types::Workspace],
) -> Result<Vec<SwayCmd>, crate::command::TextError> {
    let mut commands = Vec::new();
    for display in apart {
        if let Some((x, y)) = display.place {
            commands.push(
                SwayCmd::new("output")
                    .text(SwayText::new(&display.name)?)
                    .keyword("pos")
                    .int(i64::from(x))
                    .int(i64::from(y)),
            );
        }
        if !display.fresh {
            continue;
        }
        commands.push(
            SwayCmd::new("focus")
                .keyword("output")
                .text(SwayText::new(&display.name)?),
        );
        commands.push(
            SwayCmd::new("workspace")
                .keyword("--no-auto-back-and-forth")
                .text(SwayText::new(workspace_name(&display.name))?),
        );
    }
    // Not onto the display of a picture: the workspace that had the focus may be one of those
    // that just left it.
    if let Some(focused) = open.iter().find(|open| open.focused) {
        if apart.iter().any(|display| display.fresh) {
            commands.push(
                SwayCmd::new("workspace")
                    .keyword("--no-auto-back-and-forth")
                    .text(SwayText::new(&focused.name)?),
            );
        }
    }
    Ok(commands)
}

/// The name of the workspace the picture on the display with connector `name` stands on.
pub fn workspace_name(name: &str) -> String {
    format!("Mirror {name}")
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
    place: Option<(i32, i32)>,
) -> Result<usize, crate::ipc::IpcError> {
    let outputs = Outputs::new(&client.outputs().await?);
    // Back where it stood among the others.
    if let Some(((x, y), output)) = place.zip(outputs.find(target)) {
        if let Ok(name) = SwayText::new(output.name()) {
            let back = SwayCmd::new("output")
                .text(name)
                .keyword("pos")
                .int(i64::from(x))
                .int(i64::from(y));
            client.run(&back).await?;
        }
    }
    if names.is_empty() {
        return Ok(0);
    }
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
    // One message: nobody else's command comes between a focus and the move after it.
    client.run_together(&commands).await?;
    Ok(commands.len())
}

/// Has each display that shows the workspace of a picture that ended (or never started) show a
/// workspace like any other again, so none named after a picture stays behind: one of its own
/// that is open, else the lowest number that isn't open anywhere. sway closes the picture's
/// workspace once it is empty and no longer shown. Returns how many commands were sent.
///
/// # Errors
///
/// The IPC errors of asking sway and of the commands.
pub async fn leave_ended(
    client: &mut crate::ipc::Client,
    profiles: &Profiles,
) -> Result<usize, crate::ipc::IpcError> {
    let outputs = Outputs::new(&client.outputs().await?);
    let names: Vec<&str> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .map(super::Output::identifier)
        .collect();
    // The displays whose picture runs on: theirs stay.
    let kept: Vec<&str> = profiles.of(&names).map_or_else(Vec::new, |profile| {
        startable(profile, &outputs)
            .into_iter()
            .map(|(target, _, _)| target.name())
            .collect()
    });
    let open = client.workspaces().await?;
    let commands = leaving(&kept, &open).unwrap_or_default();
    client.run_together(&commands).await?;
    Ok(commands.len())
}

/// The commands of [`leave_ended`] for the workspaces `open`, with the displays of `kept` (by
/// their connectors) still showing a picture.
fn leaving(
    kept: &[&str],
    open: &[crate::ipc::types::Workspace],
) -> Result<Vec<SwayCmd>, crate::command::TextError> {
    let number = |name: &str| name.parse::<u32>().ok();
    let mut taken: Vec<u32> = open.iter().filter_map(|open| number(&open.name)).collect();
    let mut commands = Vec::new();
    let mut focus_display = None;
    for ended in open.iter().filter(|open| {
        open.visible && open.name == workspace_name(&open.output) && !kept.contains(&&*open.output)
    }) {
        // One that is open on the display already (it came back), else a new number.
        let own = open
            .iter()
            .filter(|other| other.output == ended.output && other.name != ended.name)
            .min_by_key(|other| number(&other.name).unwrap_or(u32::MAX))
            .map(|other| other.name.clone());
        let next = own.unwrap_or_else(|| {
            let free = (1..=u32::MAX)
                .find(|free| !taken.contains(free))
                .unwrap_or(1);
            taken.push(free);
            free.to_string()
        });
        commands.push(
            SwayCmd::new("focus")
                .keyword("output")
                .text(SwayText::new(&ended.output)?),
        );
        commands.push(
            SwayCmd::new("workspace")
                .keyword("--no-auto-back-and-forth")
                .text(SwayText::new(next)?),
        );
        if ended.focused {
            focus_display = Some(ended.output.as_str());
        }
    }
    if commands.is_empty() {
        return Ok(commands);
    }
    match (focus_display, open.iter().find(|open| open.focused)) {
        (Some(display), _) => commands.push(
            SwayCmd::new("focus")
                .keyword("output")
                .text(SwayText::new(display)?),
        ),
        (None, Some(focused)) => commands.push(
            SwayCmd::new("workspace")
                .keyword("--no-auto-back-and-forth")
                .text(SwayText::new(&focused.name)?),
        ),
        (None, None) => {}
    }
    Ok(commands)
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
    startable(profile, outputs)
        .into_iter()
        .map(|(target, source, mirror)| {
            vec![
                "--fullscreen-output".to_owned(),
                target.name().to_owned(),
                "--scaling".to_owned(),
                mirror.scaling.name().to_owned(),
                "--title".to_owned(),
                TITLE.to_owned(),
                source.name().to_owned(),
            ]
        })
        .collect()
}

/// The mirrors of `profile` that can run on `outputs`: the display that shows the picture, the
/// one shown, and how. None without `wl-mirror`; not one whose display or source isn't on, nor
/// one of a chain or a cycle (a display shown elsewhere shows its own content). Whatever a
/// mirror takes (the workspaces of its display, its place among the others) is taken only for
/// these, so a picture that can't start leaves its display as it is.
fn startable<'a>(
    profile: &'a super::profiles::Profile,
    outputs: &'a Outputs,
) -> Vec<(&'a super::Output, &'a super::Output, &'a Mirror)> {
    if !installed() {
        return Vec::new();
    }
    profile
        .outputs()
        .filter_map(|(target, setup)| {
            let mirror = setup.mirror.as_ref()?;
            if !profile.mirror_runs(target) {
                return None;
            }
            let on = |name: &str| outputs.find(name).filter(|output| output.is_enabled());
            Some((on(target)?, on(&mirror.of)?, mirror))
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
    fn a_display_goes_apart_and_gets_its_workspace_only_when_the_picture_starts() {
        use crate::command::Target;
        let open: Vec<crate::ipc::types::Workspace> = serde_json::from_str(
            r#"[{"id": 1, "num": 3, "name": "3", "layout": "splith", "visible": true,
                 "focused": true, "urgent": false, "representation": null,
                 "orientation": "horizontal", "output": "eDP-1",
                 "rect": {"x": 0, "y": 0, "width": 0, "height": 0}}]"#,
        )
        .unwrap();
        let sent = |apart: &[super::Apart]| -> Vec<String> {
            super::set_apart(apart, &open)
                .unwrap()
                .iter()
                .map(|command| command.render(Target::Ipc).unwrap())
                .collect()
        };
        let starting = super::Apart {
            name: "DP-1".to_owned(),
            place: Some((4000, 0)),
            fresh: true,
        };
        assert_eq!(
            sent(&[starting]),
            [
                r#"output "DP-1" pos 4000 0"#,
                r#"focus output "DP-1""#,
                r#"workspace --no-auto-back-and-forth "Mirror DP-1""#,
                r#"workspace --no-auto-back-and-forth "3""#,
            ]
        );
        // Shown already, but something put the display among the others: only back apart, and
        // the focus stays where it is.
        let shown = super::Apart {
            name: "DP-1".to_owned(),
            place: Some((4000, 0)),
            fresh: false,
        };
        assert_eq!(sent(&[shown]), [r#"output "DP-1" pos 4000 0"#]);
    }

    #[test]
    fn a_picture_that_ended_leaves_no_workspace_of_its_own_behind() {
        use crate::command::Target;
        let open: Vec<crate::ipc::types::Workspace> = serde_json::from_str(
            r#"[{"id": 1, "num": 1, "name": "1", "layout": "splith", "visible": true,
                 "focused": true, "urgent": false, "representation": "H[foot]",
                 "orientation": "horizontal", "output": "eDP-1",
                 "rect": {"x": 0, "y": 0, "width": 0, "height": 0}},
                {"id": 2, "num": -1, "name": "Mirror DP-1", "layout": "splith", "visible": true,
                 "focused": false, "urgent": false, "representation": "",
                 "orientation": "horizontal", "output": "DP-1",
                 "rect": {"x": 0, "y": 0, "width": 0, "height": 0}}]"#,
        )
        .unwrap();
        let sent = |kept: &[&str]| -> Vec<String> {
            super::leaving(kept, &open)
                .unwrap()
                .iter()
                .map(|command| command.render(Target::Ipc).unwrap())
                .collect()
        };
        // The display shows the lowest number that isn't open, and the focus stays.
        assert_eq!(
            sent(&[]),
            [
                r#"focus output "DP-1""#,
                r#"workspace --no-auto-back-and-forth "2""#,
                r#"workspace --no-auto-back-and-forth "1""#,
            ]
        );
        // While its picture runs on, it keeps the workspace.
        let none: Vec<String> = Vec::new();
        assert_eq!(sent(&["DP-1"]), none);
    }

    #[test]
    fn scalings_have_the_names_wl_mirror_knows() {
        for scaling in Scaling::ALL {
            assert_eq!(Scaling::from_name(scaling.name()), Some(scaling));
        }
        assert_eq!(Scaling::from_name("stretch"), None);
    }
}
