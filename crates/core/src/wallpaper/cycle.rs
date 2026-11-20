// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Taking turns: the current picture of an output's collection, and what changes it.
//!
//! The `bg` line of an output with a collection names a symbolic link in the state folder
//! (`$XDG_STATE_HOME/sway-things/wallpaper/all`, or `output-<identifier>` for one output, the
//! identifier [escaped](escape)), which points at the current picture. The generated config stays the same while the pictures change, and
//! sway shows the current picture at login and after a reload. The time of the last change is
//! the link's own modification time.
//!
//! The next picture is shown by `sway-things wallpaper next`: the link moves on, and the `bg`
//! command goes to sway, which starts swaybg again. Sway Things runs no process of its own
//! (no daemon): the generated config runs `sway-things wallpaper start` at login, which changes
//! the picture of every output with a collection if its time has come and starts a systemd user
//! timer for collections that change daily or after an interval. Without systemd, pictures change at login only.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt;
use std::fmt::Write as _;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::collection::{COLLECTION, Change, Collection, Order, home, tilde};
use crate::command::{SwayCmd, Target};
use crate::i18n::Text;
use crate::ipc::{Client, IpcError};
use crate::msg;
use crate::registry::{Registry, RenderError, Value};
use crate::state::State;
use crate::state::xdg_dir;

/// The name of the timer and service units, for every output; one output's add
/// `-<identifier>`, [escaped](escape).
const UNIT: &str = "sway-things-wallpaper";

/// A timer may fire this much early, and a daily change is still due.
const SLACK: Duration = Duration::from_secs(5 * 60);

/// The folder of the links: `$XDG_STATE_HOME/sway-things/wallpaper`.
pub fn state_dir() -> Option<PathBuf> {
    xdg_dir(
        std::env::var_os("XDG_STATE_HOME"),
        std::env::var_os("HOME"),
        ".local/state",
    )
    .map(|dir| dir.join("sway-things").join("wallpaper"))
}

/// `identifier` as part of a file or unit name, the way `systemd-escape` writes it: ASCII
/// letters, digits, `:`, `_` and `.` (but not a leading `.`) stay, `/` becomes `-`, and every other
/// byte is `\xNN`. An output's identifier is an external string (make, model and serial from the
/// screen), so it can hold anything a file or unit name can't.
pub fn escape(identifier: &str) -> String {
    let mut escaped = String::with_capacity(identifier.len());
    for (index, byte) in identifier.bytes().enumerate() {
        match byte {
            b'/' => escaped.push('-'),
            b'.' if index > 0 => escaped.push('.'),
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b':' | b'_' => escaped.push(char::from(byte)),
            // Writing to a `String` can't fail.
            _ => drop(write!(escaped, "\\x{byte:02x}")),
        }
    }
    escaped
}

/// The link to the current picture of `output`, or of every output, in `dir`.
pub fn link_in(dir: &Path, output: Option<&str>) -> PathBuf {
    dir.join(match output {
        None => "all".to_owned(),
        Some(identifier) => format!("output-{}", escape(identifier)),
    })
}

/// The link to the current picture of `output`, or of every output; `None` if the state folder
/// can't be found.
pub fn link(output: Option<&str>) -> Option<PathBuf> {
    state_dir().map(|dir| link_in(&dir, output))
}

/// The link of `output` as the `bg` line names it: with `~/` for the home folder, so the line
/// doesn't change with the user name.
pub fn config_link(output: Option<&str>) -> Option<PathBuf> {
    let link = link(output)?;
    Some(PathBuf::from(tilde(&link, home().as_deref())))
}

/// `exec sway-things wallpaper start`: the config line that shows the current picture of every
/// output's collection at login and starts their timers. It names no output, so no identifier
/// of a screen reaches the shell that runs the line.
pub fn start_command() -> SwayCmd {
    SwayCmd::new("exec")
        .keyword("sway-things")
        .keyword("wallpaper")
        .keyword("start")
        .config_only()
}

/// The picture `link` points at.
pub fn current(link: &Path) -> Option<PathBuf> {
    std::fs::read_link(link).ok()
}

/// When `link` last changed.
fn changed_at(link: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(link).ok()?.modified().ok()
}

/// The picture after `current` in `pictures`. `random` picks it for [`Order::Shuffle`]: never
/// the current one, unless it is the only one.
pub fn choose<'a>(
    pictures: &'a [PathBuf],
    current: Option<&Path>,
    order: Order,
    random: u64,
) -> Option<&'a PathBuf> {
    let position =
        current.and_then(|current| pictures.iter().position(|picture| picture == current));
    match order {
        Order::InOrder => match position {
            Some(position) => pictures.get((position + 1) % pictures.len()),
            None => pictures.first(),
        },
        Order::Shuffle => {
            let others: Vec<&PathBuf> = pictures
                .iter()
                .enumerate()
                .filter(|(index, _)| Some(*index) != position)
                .map(|(_, picture)| picture)
                .collect();
            if others.is_empty() {
                return pictures.first();
            }
            let index = usize::try_from(random % others.len() as u64).unwrap_or(0);
            others.get(index).copied()
        }
    }
}

/// A number that differs from call to call.
fn random() -> u64 {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    if let Ok(since) = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        hasher.write_u128(since.as_nanos());
    }
    hasher.finish()
}

/// Points `link` at `picture`, atomically: a new link replaces the old one.
///
/// # Errors
///
/// The error of the operating system if the folder or the link can't be made.
pub fn point(link: &Path, picture: &Path) -> io::Result<()> {
    let (Some(dir), Some(name)) = (link.parent(), link.file_name()) else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    std::fs::create_dir_all(dir)?;
    let mut staged = OsString::from(".");
    staged.push(name);
    staged.push(format!(".{}", std::process::id()));
    let staged = dir.join(staged);
    match std::fs::remove_file(&staged) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    std::os::unix::fs::symlink(picture, &staged)?;
    std::fs::rename(&staged, link).inspect_err(|_| {
        let _ = std::fs::remove_file(&staged);
    })
}

/// Why the picture would change now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    /// The session starts (`wallpaper start`).
    Login,
    /// The timer fired (`wallpaper next --due`).
    Timer,
    /// The user asked for the next picture.
    Now,
}

/// Whether the picture of a collection that changes at `change`, and last changed at `changed`,
/// changes at `moment`.
pub fn is_due(
    change: Change,
    changed: Option<SystemTime>,
    now: SystemTime,
    moment: Moment,
) -> bool {
    let elapsed = changed.map(|changed| now.duration_since(changed).unwrap_or_default());
    match (moment, change) {
        // The timer of an interval fires when the interval has passed.
        (Moment::Now, _) | (Moment::Login, Change::Login) | (Moment::Timer, Change::Every(_)) => {
            true
        }
        (Moment::Timer, Change::Login) => false,
        (Moment::Login | Moment::Timer, Change::Daily | Change::Every(_)) => {
            match (elapsed, change.period()) {
                (Some(elapsed), Some(period)) => elapsed + SLACK >= period,
                _ => true,
            }
        }
    }
}

/// An output, or every output, that shows a collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// The output; `None` for every output.
    pub output: Option<String>,
    /// The collection.
    pub collection: Collection,
}

/// The outputs that show a collection, by the stored values of `state`: every output first.
pub fn shown(state: &State) -> Vec<Shown> {
    state
        .entries()
        .into_iter()
        .filter(|entry| entry.setting.key() == COLLECTION)
        .filter_map(|entry| match entry.value {
            Value::Text(name) if !name.is_empty() => Some(Shown {
                output: entry.device.map(ToOwned::to_owned),
                collection: Collection::load(state, name),
            }),
            _ => None,
        })
        .collect()
}

/// The collection `output` shows (`None`: every output), if it has one of its own.
pub fn shown_on(state: &State, output: Option<&str>) -> Option<Shown> {
    shown(state)
        .into_iter()
        .find(|shown| shown.output.as_deref() == output)
}

/// Makes sure the link of every output with a collection points at a picture of it, before the
/// config names the link: a link that is missing, broken or points elsewhere moves on to the
/// next picture. A collection without pictures is skipped.
///
/// # Errors
///
/// The first error of the operating system; the other links are made anyway.
pub fn prepare(state: &State) -> io::Result<()> {
    let home = home();
    let mut result = Ok(());
    for shown in shown(state) {
        let Some(link) = link(shown.output.as_deref()) else {
            continue;
        };
        let pictures = shown.collection.pictures(home.as_deref());
        let current = current(&link).filter(|current| pictures.contains(current));
        if current.is_some() {
            continue;
        }
        if let Some(picture) = choose(&pictures, None, shown.collection.order, random()) {
            if let Err(error) = point(&link, picture) {
                if result.is_ok() {
                    result = Err(error);
                }
            }
        }
    }
    result
}

/// Whether the link of `output` points at an existing picture, so a `bg` line may name it.
pub fn has_picture(output: Option<&str>) -> bool {
    link(output).is_some_and(|link| link.is_file())
}

/// Shows the next picture of the collection of `output` (`None`: every output) if it is due at
/// `moment`: moves the link on and sends the `bg` command. Returns the picture, or `None` if
/// nothing changed.
///
/// # Errors
///
/// [`CycleError`] if the output has no collection, the link can't be moved, or sway rejects the
/// command.
pub async fn next(
    client: Option<&mut Client>,
    state: &State,
    output: Option<&str>,
    moment: Moment,
) -> Result<Option<PathBuf>, CycleError> {
    let shown = shown_on(state, output).ok_or(CycleError::NoCollection)?;
    let link = link(output).ok_or(CycleError::NoStateDir)?;
    let current = current(&link);
    let pictures = shown.collection.pictures(home().as_deref());
    let still_there = current
        .as_ref()
        .filter(|current| pictures.contains(current));
    if still_there.is_some()
        && !is_due(
            shown.collection.change,
            changed_at(&link),
            SystemTime::now(),
            moment,
        )
    {
        return Ok(None);
    }
    let Some(picture) = choose(
        &pictures,
        current.as_deref(),
        shown.collection.order,
        random(),
    ) else {
        return Err(CycleError::NoPictures);
    };
    let picture = picture.clone();
    point(&link, &picture).map_err(CycleError::Io)?;
    if let Some(client) = client {
        show(client, state, output).await?;
    }
    Ok(Some(picture))
}

/// Sends the `bg` command of `output` (`None`: every output) again, so swaybg reads the picture
/// the link points at. For every output, the outputs with a background of their own get theirs
/// again after it: `output *` at runtime replaces it.
///
/// # Errors
///
/// [`CycleError::Render`] or [`CycleError::Ipc`] if a command can't be built or sway rejects it.
pub async fn show(
    client: &mut Client,
    state: &State,
    output: Option<&str>,
) -> Result<(), CycleError> {
    let registry = Registry::builtin();
    let Some(setting) = registry.get(COLLECTION) else {
        return Ok(());
    };
    let mut targets = vec![output.map(ToOwned::to_owned)];
    if output.is_none() {
        let own: BTreeSet<String> = state
            .entries()
            .into_iter()
            .filter(|entry| {
                registry
                    .group(setting)
                    .any(|member| member.key() == entry.setting.key())
            })
            .filter_map(|entry| entry.device.map(ToOwned::to_owned))
            .collect();
        targets.extend(own.into_iter().map(Some));
    }
    for target in targets {
        let commands = registry
            .render(
                setting,
                &state.values(target.as_deref()),
                target.as_deref(),
                Target::Ipc,
            )
            .map_err(CycleError::Render)?;
        for command in &commands {
            client.run(command).await.map_err(CycleError::Ipc)?;
        }
    }
    Ok(())
}

/// The name of the timer and service of `output` (`None`: every output).
pub fn unit(output: Option<&str>) -> String {
    match output {
        None => UNIT.to_owned(),
        Some(identifier) => format!("{UNIT}-{}", escape(identifier)),
    }
}

/// Whether a systemd user manager runs for this user, so timers can change the pictures.
pub fn has_timers() -> bool {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .is_some_and(|dir| dir.join("systemd").join("private").exists())
}

/// The `sway-things` program for the timer: next to the running program, else from `PATH`.
pub fn program() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("sway-things")))
        .filter(|program| program.is_file())
        .unwrap_or_else(|| PathBuf::from("sway-things"))
}

/// Stops the timer of `output` (`None`: every output), if one runs.
pub async fn unschedule(output: Option<&str>) {
    let timer = format!("{}.timer", unit(output));
    // Not running is fine, and without systemd there is nothing to stop.
    let _ = async_process::Command::new("systemctl")
        .args(["--user", "stop", "--quiet", &timer])
        .output()
        .await;
}

/// Starts the timer that changes the picture of `output` (`None`: every output) for `change`, in
/// place of the one that runs. A collection that changes at login only needs none. The timer is
/// transient: systemd forgets it when the user's session ends, and no unit file is written.
///
/// # Errors
///
/// [`CycleError::Systemd`] if `systemd-run` can't be run or fails.
pub async fn schedule(
    output: Option<&str>,
    change: Change,
    program: &Path,
) -> Result<(), CycleError> {
    unschedule(output).await;
    let period = match change {
        Change::Login => return Ok(()),
        // Checked every hour: a day counts from the last change, which may be a login.
        Change::Daily => Duration::from_secs(60 * 60),
        Change::Every(period) => period,
    };
    let seconds = period.as_secs().max(60);
    let mut command = async_process::Command::new("systemd-run");
    command.args([
        "--user".to_owned(),
        "--quiet".to_owned(),
        "--collect".to_owned(),
        format!("--unit={}", unit(output)),
        format!("--on-active={seconds}s"),
        format!("--on-unit-active={seconds}s"),
        "--timer-property=AccuracySec=1s".to_owned(),
    ]);
    // The timer's service runs outside the session: it needs the socket of this sway, and the
    // state folder this process uses.
    for variable in ["SWAYSOCK", "XDG_STATE_HOME"] {
        if let Some(value) = std::env::var_os(variable) {
            let mut assignment = OsString::from(format!("--setenv={variable}="));
            assignment.push(value);
            command.arg(assignment);
        }
    }
    command
        .arg("--")
        .arg(program)
        .args(["wallpaper", "next", "--due"]);
    // An argument of its own, never a shell's word.
    if let Some(identifier) = output {
        command.args(["--device", identifier]);
    }
    let finished = command
        .output()
        .await
        .map_err(|error| CycleError::Systemd(error.to_string()))?;
    if finished.status.success() {
        Ok(())
    } else {
        Err(CycleError::Systemd(
            String::from_utf8_lossy(&finished.stderr).trim().to_owned(),
        ))
    }
}

/// Brings the timers in line with `state`: stops every timer of Sway Things, then starts one for
/// each output with a collection that changes daily or after an interval.
///
/// # Errors
///
/// The first [`CycleError::Systemd`]; the other timers are started anyway.
pub async fn reschedule(state: &State, program: &Path) -> Result<(), CycleError> {
    let _ = async_process::Command::new("systemctl")
        .args(["--user", "stop", "--quiet", &format!("{UNIT}*.timer")])
        .output()
        .await;
    let mut result = Ok(());
    for shown in shown(state) {
        if shown.collection.change == Change::Login {
            continue;
        }
        if let Err(error) =
            schedule(shown.output.as_deref(), shown.collection.change, program).await
        {
            if result.is_ok() {
                result = Err(error);
            }
        }
    }
    result
}

/// Why the picture can't change.
#[derive(Debug)]
pub enum CycleError {
    /// The output shows no collection.
    NoCollection,
    /// The collection has no pictures.
    NoPictures,
    /// Neither `XDG_STATE_HOME` nor `HOME` is an absolute path.
    NoStateDir,
    /// The link can't be made.
    Io(io::Error),
    /// The `bg` command can't be built.
    Render(RenderError),
    /// sway rejected the command or can't be reached.
    Ipc(IpcError),
    /// `systemd-run` can't be run or failed, with its message.
    Systemd(String),
}

impl CycleError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::NoCollection => msg!("the output shows no collection").into(),
            Self::NoPictures => msg!("the collection has no pictures").into(),
            Self::NoStateDir => msg!(
                "the folder for the picture of a collection can't be found: neither \
                 XDG_STATE_HOME nor HOME is an absolute path"
            )
            .into(),
            Self::Io(error) => {
                Text::new(msg!("the picture can't be changed: {error}")).arg("error", error)
            }
            Self::Render(error) => error.text(),
            Self::Ipc(error) => error.text(),
            Self::Systemd(message) => {
                Text::new(msg!("the timer can't be started: {error}")).arg("error", message)
            }
        }
    }
}

impl fmt::Display for CycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for CycleError {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use super::{Moment, choose, current, escape, is_due, link_in, point, start_command, unit};
    use crate::command::Target;
    use crate::wallpaper::{Change, DAY, Order};

    fn pictures() -> Vec<PathBuf> {
        ["/p/a.png", "/p/b.png", "/p/c.png"]
            .into_iter()
            .map(PathBuf::from)
            .collect()
    }

    #[test]
    fn in_order_goes_round() {
        let pictures = pictures();
        let next = |current: &str| {
            choose(&pictures, Some(current.as_ref()), Order::InOrder, 0)
                .unwrap()
                .clone()
        };
        assert_eq!(next("/p/a.png"), pictures[1]);
        assert_eq!(next("/p/c.png"), pictures[0]);
        assert_eq!(next("/elsewhere.png"), pictures[0]);
        assert_eq!(choose(&[], None, Order::InOrder, 0), None);
    }

    #[test]
    fn shuffle_never_repeats_the_current_picture() {
        let pictures = pictures();
        for random in 0..20 {
            let next = choose(&pictures, Some("/p/b.png".as_ref()), Order::Shuffle, random);
            assert_ne!(next, Some(&pictures[1]));
            assert!(next.is_some());
        }
        let one = &pictures[..1];
        assert_eq!(
            choose(one, Some("/p/a.png".as_ref()), Order::Shuffle, 7),
            Some(&pictures[0])
        );
    }

    #[test]
    fn changes_are_due_by_their_kind() {
        let now = SystemTime::UNIX_EPOCH + DAY * 100;
        let ago = |seconds: u64| Some(now - Duration::from_secs(seconds));
        let hour = Change::Every(Duration::from_secs(3600));
        assert!(is_due(Change::Login, ago(1), now, Moment::Login));
        assert!(!is_due(Change::Login, ago(1), now, Moment::Timer));
        assert!(is_due(Change::Login, ago(1), now, Moment::Now));
        assert!(!is_due(Change::Daily, ago(3600), now, Moment::Login));
        assert!(!is_due(Change::Daily, ago(3600), now, Moment::Timer));
        assert!(is_due(
            Change::Daily,
            ago(DAY.as_secs() - 60),
            now,
            Moment::Timer
        ));
        assert!(is_due(Change::Daily, None, now, Moment::Login));
        assert!(!is_due(hour, ago(600), now, Moment::Login));
        assert!(is_due(hour, ago(7200), now, Moment::Login));
        assert!(is_due(hour, ago(600), now, Moment::Timer));
    }

    #[test]
    fn links_move_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let link = link_in(&dir.path().join("wallpaper"), Some("Dell Inc. U2415/7MT0"));
        assert!(link.ends_with("wallpaper/output-Dell\\x20Inc.\\x20U2415-7MT0"));
        assert_eq!(current(&link), None);
        point(&link, "/p/a.png".as_ref()).unwrap();
        point(&link, "/p/b.png".as_ref()).unwrap();
        assert_eq!(current(&link), Some(PathBuf::from("/p/b.png")));
        assert_eq!(
            std::fs::read_dir(dir.path().join("wallpaper"))
                .unwrap()
                .count(),
            1
        );
        assert!(link_in(dir.path(), None).ends_with("all"));
    }

    #[test]
    fn the_config_starts_every_collection_without_naming_an_output() {
        assert_eq!(
            start_command().render(Target::Config).unwrap(),
            "exec sway-things wallpaper start"
        );
        assert!(start_command().render(Target::Ipc).is_err());
    }

    #[test]
    fn identifiers_are_escaped_like_systemd_escape() {
        assert_eq!(escape("DP-3"), "DP\\x2d3");
        assert_eq!(
            escape("Fake Make Co.,Ltd/Model $(id) .5"),
            "Fake\\x20Make\\x20Co.\\x2cLtd-Model\\x20\\x24\\x28id\\x29\\x20.5"
        );
        assert_eq!(escape(".hidden"), "\\x2ehidden");
        assert_eq!(
            unit(Some("HDMI-A-1")),
            "sway-things-wallpaper-HDMI\\x2dA\\x2d1"
        );
        assert_eq!(unit(None), "sway-things-wallpaper");
    }
}
