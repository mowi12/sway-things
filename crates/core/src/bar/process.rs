// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The running bar: which waybar processes of this session run our config, starting ours and
//! having it read its files again.
//!
//! Processes are found in `/proc` as `hook mirror` finds its `wl-mirror`s: of this user, with
//! this session's `XDG_RUNTIME_DIR` and `WAYLAND_DISPLAY`, and waybar by their name, the first
//! word of their command line or their executable (`waybar`, or nix's `.waybar-wrapped`). One
//! whose command line has `-c` or `--config` with our config runs ours. Only those get `SIGUSR2`
//! (reload) or, when the bar is handed back, `SIGTERM`, sent with `kill` and their numbers;
//! right before, each process is checked to be still the same one (its start time). waybar is
//! started with `setsid -f waybar -c … -s …`, an argument list without a shell, under a lock in
//! `$XDG_RUNTIME_DIR`, and checked to be still running a moment later. What went wrong is noted
//! in `$XDG_RUNTIME_DIR/sway-things-waybar.problem` for the page and `doctor`, since the hook's
//! own messages only reach sway's log.

use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::WaybarFiles;
use super::starter::is_waybar;
pub use super::starter::{
    Commands, Edit, Handover, Line, Starter, TakesEffect, UNIT_EDIT, arguments, handover,
    handover_commands, handover_edit, shell_path, swaybars, unit_commands,
};
use crate::i18n::Text;
use crate::msg;

/// A waybar process of this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waybar {
    /// Its process number.
    pub pid: u32,
    /// When it started, in clock ticks after boot (`/proc/<pid>/stat`): a number taken by
    /// another process later has another start time.
    pub start: u64,
    /// The config it was started with (`-c`), as given.
    pub config: Option<PathBuf>,
    /// Whether it runs as a unit of its own (a systemd user service): its control group is named
    /// after waybar.
    pub unit: bool,
}

impl Waybar {
    /// Whether it runs the config of `files`.
    pub fn runs(&self, files: &WaybarFiles) -> bool {
        self.config
            .as_deref()
            .is_some_and(|config| same_file(config, &files.config()))
    }

    /// Whether the process is still this one.
    fn is_alive(&self) -> bool {
        start_time(&PathBuf::from(format!("/proc/{}", self.pid))) == Some(self.start)
    }
}

/// Whether `a` and `b` are the same file, also through symbolic links.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
}

/// When the process of `/proc/<pid>` started: the 22nd field of its `stat`.
fn start_time(dir: &Path) -> Option<u64> {
    let stat = std::fs::read_to_string(dir.join("stat")).ok()?;
    // The name in parentheses can hold blanks and parentheses itself.
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(19)?.parse().ok()
}

/// Whether the process of `/proc/<pid>` is a waybar: by its name, the first word of its command
/// line or its executable.
fn is_waybar_process(dir: &Path) -> bool {
    let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
    if matches!(comm.trim_end(), "waybar" | ".waybar-wrapped") {
        return true;
    }
    let first = std::fs::read(dir.join("cmdline")).ok().and_then(|cmdline| {
        let first = cmdline.split(|byte| *byte == 0).next()?.to_vec();
        String::from_utf8(first).ok()
    });
    if first.is_some_and(|first| is_waybar(&first)) {
        return true;
    }
    std::fs::read_link(dir.join("exe"))
        .ok()
        .and_then(|exe| exe.to_str().map(is_waybar))
        .unwrap_or(false)
}

/// The waybar processes of this session. Reads `/proc`; nothing is run.
///
/// # Blocking
///
/// Reads `/proc`.
pub fn running() -> Vec<Waybar> {
    crate::outputs::session_processes_where(is_waybar_process)
        .into_iter()
        .filter_map(|dir| {
            let pid = dir.file_name()?.to_str()?.parse().ok()?;
            let start = start_time(&dir)?;
            let cmdline = std::fs::read(dir.join("cmdline")).ok()?;
            let cwd = std::fs::read_link(dir.join("cwd")).ok();
            let unit = std::fs::read_to_string(dir.join("cgroup")).is_ok_and(|groups| {
                groups
                    .lines()
                    .filter_map(|line| line.rsplit('/').next())
                    .any(|unit| unit.contains("waybar"))
            });
            Some(Waybar {
                pid,
                start,
                config: config_argument(&cmdline, cwd.as_deref()),
                unit,
            })
        })
        .collect()
}

/// The config of a waybar command line (`/proc/<pid>/cmdline`, its arguments separated by NUL
/// bytes): the argument of `-c` or `--config`, as given or as `--config=…`, relative to `cwd`.
pub fn config_argument(cmdline: &[u8], cwd: Option<&Path>) -> Option<PathBuf> {
    let args: Vec<&[u8]> = cmdline.split(|byte| *byte == 0).collect();
    let mut found = None;
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        if *arg == b"-c" || *arg == b"--config" {
            found = iter.next().copied();
        } else if let Some(rest) = arg.strip_prefix(b"--config=") {
            found = Some(rest);
        } else if let Some(rest) = arg.strip_prefix(b"-c").filter(|rest| !rest.is_empty()) {
            found = Some(rest);
        }
    }
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(found?));
    Some(match cwd {
        Some(cwd) if path.is_relative() => cwd.join(path),
        _ => path,
    })
}

/// Whether `waybar` is installed: an executable of that name in a directory of `$PATH`.
pub fn installed() -> bool {
    crate::delegate::find_program("waybar", std::env::var_os("PATH").as_deref()).is_some()
}

/// What went wrong with the running bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    /// waybar isn't installed.
    NotInstalled,
    /// A program Sway Things runs for it (`setsid`, `kill`) isn't there or failed.
    Tool(&'static str),
    /// waybar ended right after it started or read its files again: an option it can't take,
    /// say, or no display to show it on.
    Ended,
}

impl ProcessError {
    /// What went wrong, to show.
    pub fn text(self) -> Text {
        match self {
            Self::NotInstalled => msg!("waybar isn't installed").into(),
            Self::Tool(program) => {
                Text::new(msg!("{program} couldn't be run")).arg("program", program)
            }
            Self::Ended => msg!(
                "waybar ended right after it read the config of Sway Things; run waybar -c with \
                 it in a terminal to see why"
            )
            .into(),
        }
    }

    /// The word the problem file holds.
    fn word(self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::Tool("kill") => "no_kill",
            Self::Tool(_) => "no_setsid",
            Self::Ended => "ended",
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        Some(match word.trim() {
            "not_installed" => Self::NotInstalled,
            "no_kill" => Self::Tool("kill"),
            "no_setsid" => Self::Tool("setsid"),
            "ended" => Self::Ended,
            _ => return None,
        })
    }
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for ProcessError {}

/// The file the last problem is noted in, in `$XDG_RUNTIME_DIR`.
fn problem_file() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("sway-things-waybar.problem"))
}

/// Notes `problem` for the page and `doctor`, or that all is well.
pub fn note(problem: Option<ProcessError>) {
    let Some(path) = problem_file() else {
        return;
    };
    match problem {
        Some(problem) => {
            let _ = std::fs::write(path, problem.word());
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The problem noted last in this session, if any.
pub fn last_problem() -> Option<ProcessError> {
    std::fs::read_to_string(problem_file()?)
        .ok()
        .and_then(|word| ProcessError::from_word(&word))
}

/// Sends `signal` to each of `targets` that is still the same process. Says how many got it.
///
/// # Blocking
///
/// Waits for `kill`.
fn signal(targets: &[Waybar], signal: &str) -> Result<usize, ProcessError> {
    let pids: Vec<String> = targets
        .iter()
        .filter(|waybar| waybar.is_alive())
        .map(|waybar| waybar.pid.to_string())
        .collect();
    if pids.is_empty() {
        return Ok(0);
    }
    let status = Command::new("kill")
        .arg("-s")
        .arg(signal)
        .arg("--")
        .args(&pids)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| ProcessError::Tool("kill"))?;
    // A process that ended in between makes `kill` fail too; the others got it.
    if status.success() || targets.iter().any(|waybar| !waybar.is_alive()) {
        Ok(pids.len())
    } else {
        Err(ProcessError::Tool("kill"))
    }
}

/// The waybar processes of this session that run our config.
///
/// # Blocking
///
/// Reads `/proc`.
pub fn ours(files: &WaybarFiles) -> Vec<Waybar> {
    running()
        .into_iter()
        .filter(|waybar| waybar.runs(files))
        .collect()
}

/// Ends every waybar of this session that runs our config (`SIGTERM`): the bar was handed back.
/// Says how many got the signal.
///
/// # Errors
///
/// [`ProcessError::Tool`] if `kill` can't be run.
///
/// # Blocking
///
/// Reads `/proc` and waits for `kill`.
pub fn stop(files: &WaybarFiles) -> Result<usize, ProcessError> {
    signal(&ours(files), "TERM")
}

/// The waybars of ours among `running` that would be a second bar: those Sway Things started
/// (not a unit), where `starter` surely starts the user's own waybar or one with another config
/// runs. A waybar the user's own line starts with our config is never one, nor is ours where
/// their bar is taken over.
pub fn surplus(starter: &Starter, running: &[Waybar], files: &WaybarFiles) -> Vec<Waybar> {
    // Taken over, sway runs ours as their bar; theirs may still be ending after the reload.
    if matches!(starter, Starter::TakenOver(_)) {
        return Vec::new();
    }
    let theirs = starter.starts_theirs() || running.iter().any(|waybar| !waybar.runs(files));
    if !theirs {
        return Vec::new();
    }
    running
        .iter()
        .filter(|waybar| waybar.runs(files) && !waybar.unit)
        .cloned()
        .collect()
}

/// Ends every waybar that Sway Things started (`SIGTERM`), not one a unit runs: the backend
/// isn't waybar any more. Says how many got the signal.
///
/// # Errors
///
/// [`ProcessError::Tool`] if `kill` can't be run.
///
/// # Blocking
///
/// Reads `/proc` and waits for `kill`.
pub fn stop_started(files: &WaybarFiles) -> Result<usize, ProcessError> {
    let started: Vec<Waybar> = ours(files)
        .into_iter()
        .filter(|waybar| !waybar.unit)
        .collect();
    signal(&started, "TERM")
}

/// The version of the waybar that is installed, as `waybar --version` says it (`0.16.0`).
/// Runs it: call it off the main loop.
///
/// # Blocking
///
/// Waits for `waybar --version`.
pub fn version() -> Option<String> {
    let output = Command::new("waybar")
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    version_in(&String::from_utf8_lossy(&output.stdout))
}

/// The version in what `waybar --version` printed: the word after `Waybar v`.
fn version_in(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (_, rest) = line.split_once("Waybar v")?;
        rest.split_whitespace().next().map(str::to_owned)
    })
}

/// What [`reload`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reloaded {
    /// No waybar runs our config.
    NoneRuns,
    /// The waybar processes that run it read it again.
    Read,
    /// One ended when it read the files again and was started again with them.
    Restarted,
}

/// Has every waybar of this session that runs our config read it again (`SIGUSR2`), then checks
/// a moment later that it still runs; one that ended is started again once.
///
/// # Errors
///
/// [`ProcessError`] if `kill` can't be run, or waybar ended and couldn't be started again.
///
/// # Blocking
///
/// Reads `/proc` and waits for `kill` and, if waybar ended, for `setsid`.
pub async fn reload(files: &WaybarFiles) -> Result<Reloaded, ProcessError> {
    let before = ours(files);
    if signal(&before, "USR2")? == 0 {
        return Ok(Reloaded::NoneRuns);
    }
    async_io::Timer::after(Duration::from_millis(1500)).await;
    if before.iter().any(Waybar::is_alive) || !ours(files).is_empty() {
        return Ok(Reloaded::Read);
    }
    start_checked(files).await?;
    Ok(Reloaded::Restarted)
}

/// Starts waybar with our config and stylesheet, in a session of its own (`setsid -f`), so it
/// neither ends with the caller nor stays behind as a child nobody waits for, and checks that it
/// runs our config a moment later.
///
/// # Errors
///
/// [`ProcessError::NotInstalled`], [`ProcessError::Tool`] if `setsid` can't be run, and
/// [`ProcessError::Ended`] if no waybar of ours runs a moment later.
///
/// # Blocking
///
/// Waits for `setsid` and reads `/proc`.
pub async fn start_checked(files: &WaybarFiles) -> Result<(), ProcessError> {
    if !installed() {
        return Err(ProcessError::NotInstalled);
    }
    let started = Command::new("setsid")
        .arg("-f")
        .arg("waybar")
        .arg("-c")
        .arg(files.config())
        .arg("-s")
        .arg(files.style())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| ProcessError::Tool("setsid"))?;
    if !started.success() {
        return Err(ProcessError::Tool("setsid"));
    }
    // It shows up, and is still there a moment later: a config it can't read ends it at once.
    let until = Instant::now() + Duration::from_secs(3);
    let mut seen = Vec::new();
    while Instant::now() < until {
        seen = ours(files);
        if !seen.is_empty() {
            break;
        }
        async_io::Timer::after(Duration::from_millis(100)).await;
    }
    async_io::Timer::after(Duration::from_millis(700)).await;
    if seen.iter().any(Waybar::is_alive) {
        Ok(())
    } else {
        Err(ProcessError::Ended)
    }
}

/// What [`ensure`] found or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ensured {
    /// A waybar of ours runs already.
    Runs,
    /// Ours was started.
    Started,
    /// The user's sway config or a unit starts waybar.
    Elsewhere,
    /// A waybar with another config runs (or came up while waiting): ours would be a second bar.
    Other(u32),
    /// Another `ensure` of this session is busy with it.
    Busy,
}

/// Has ours run unless one runs already, `starter` says something else starts waybar, or a
/// waybar with another config runs or comes up within two seconds (at login the user's own
/// lines start theirs at the same time). Under a lock in `$XDG_RUNTIME_DIR`, so the app's start
/// and the hook's at a reload never start two. Notes the outcome ([`note`]).
///
/// # Errors
///
/// What [`start_checked`] says.
///
/// # Blocking
///
/// Reads `/proc` and waits for `kill` and `setsid`.
pub async fn ensure(files: &WaybarFiles, starter: &Starter) -> Result<Ensured, ProcessError> {
    let Some(_lock) = Lock::take().await else {
        return Ok(Ensured::Busy);
    };
    // A waybar of ours next to the user's own (their `bar` block came back, say): ours goes.
    let all = running();
    let surplus = surplus(starter, &all, files);
    if !surplus.is_empty() {
        signal(&surplus, "TERM")?;
        note(None);
        return Ok(match all.iter().find(|waybar| !waybar.runs(files)) {
            Some(theirs) if !starter.is_elsewhere() => Ensured::Other(theirs.pid),
            _ => Ensured::Elsewhere,
        });
    }
    if !ours(files).is_empty() {
        note(None);
        return Ok(Ensured::Runs);
    }
    if starter.is_elsewhere() {
        return Ok(Ensured::Elsewhere);
    }
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(other) = running().into_iter().find(|waybar| !waybar.runs(files)) {
            return Ok(Ensured::Other(other.pid));
        }
        if Instant::now() >= until {
            break;
        }
        async_io::Timer::after(Duration::from_millis(200)).await;
    }
    let outcome = start_checked(files).await;
    note(outcome.err());
    outcome.map(|()| Ensured::Started)
}

/// The lock of [`ensure`]: a file in `$XDG_RUNTIME_DIR` with a token of its own, made only if it
/// isn't there and removed when dropped, if it still holds the token. One older than 15 seconds
/// was left behind and is taken over.
struct Lock {
    path: PathBuf,
    token: String,
}

impl Lock {
    /// Takes the lock in `$XDG_RUNTIME_DIR`, waiting up to five seconds for another holder.
    async fn take() -> Option<Self> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())?;
        Self::take_in(&dir, Duration::from_secs(5)).await
    }

    /// Takes the lock in `dir`, waiting up to `wait` for another holder.
    async fn take_in(dir: &Path, wait: Duration) -> Option<Self> {
        let path = dir.join("sway-things-waybar.lock");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.subsec_nanos());
        let token = format!("{} {nanos}", std::process::id());
        let until = Instant::now() + wait;
        loop {
            let created = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .and_then(|mut file| std::io::Write::write_all(&mut file, token.as_bytes()));
            if created.is_ok() {
                return Some(Self { path, token });
            }
            let stale = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > Duration::from_secs(15));
            if stale {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if Instant::now() >= until {
                return None;
            }
            async_io::Timer::after(Duration::from_millis(100)).await;
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.path).is_ok_and(|held| held == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_read_from_waybar_s_words() {
        assert_eq!(
            version_in("[2026-11-21] [info] no JOURNAL_STREAM\nWaybar v0.16.0\n").as_deref(),
            Some("0.16.0")
        );
        assert_eq!(version_in("waybar: not found"), None);
    }

    fn files() -> WaybarFiles {
        WaybarFiles {
            dir: PathBuf::from("/c/sway-things/waybar"),
        }
    }

    #[test]
    fn the_config_of_a_waybar_command_line_is_found() {
        let cmdline = b"waybar\0-l\0info\0-c\0/c/sway-things/waybar/config.jsonc\0-s\0x.css\0";
        let waybar = Waybar {
            pid: 1,
            start: 1,
            config: config_argument(cmdline, None),
            unit: false,
        };
        assert!(waybar.runs(&files()));
        assert_eq!(
            config_argument(b"waybar\0--config=rel.jsonc\0", Some(Path::new("/w"))),
            Some(PathBuf::from("/w/rel.jsonc"))
        );
        assert_eq!(config_argument(b"waybar\0", None), None);
        assert_eq!(
            config_argument(b"/usr/bin/waybar\0-c/a\0", None),
            Some(PathBuf::from("/a"))
        );
    }

    #[test]
    fn ours_goes_where_the_user_s_own_waybar_runs() {
        let waybar = |pid, config: Option<&str>, unit| Waybar {
            pid,
            start: 1,
            config: config.map(PathBuf::from),
            unit,
        };
        let ours = waybar(1, Some("/c/sway-things/waybar/config.jsonc"), false);
        let theirs = waybar(2, None, false);
        let line = |ours| Line {
            file: PathBuf::from("/c/sway/config"),
            line: 3,
            text: String::new(),
            ours,
        };
        let block = Starter::SwaybarCommand(line(false));
        let alone = vec![ours];
        let both = vec![theirs, alone[0].clone()];
        // Their `bar` block came back while ours runs: ours goes, also before theirs is up.
        assert_eq!(surplus(&block, &both, &files()), alone);
        assert_eq!(surplus(&block, &alone, &files()), alone);
        assert_eq!(
            surplus(&Starter::Exec(line(false)), &alone, &files()),
            alone
        );
        // A waybar with another config runs: ours would be the second.
        assert_eq!(surplus(&Starter::Nothing, &both, &files()), alone);
        // Ours alone, or started by the user's own line with our config, stays.
        assert_eq!(
            surplus(&Starter::Nothing, &alone, &files()),
            Vec::<Waybar>::new()
        );
        assert_eq!(
            surplus(&Starter::Exec(line(true)), &alone, &files()),
            Vec::<Waybar>::new()
        );
        // A line that only may start waybar isn't sure.
        assert_eq!(
            surplus(&Starter::Maybe(line(false)), &alone, &files()),
            Vec::<Waybar>::new()
        );
        // Taken over, sway runs ours as their bar, while theirs may still be ending.
        assert_eq!(
            surplus(&Starter::TakenOver(line(false)), &both, &files()),
            Vec::<Waybar>::new()
        );
        // A unit of the user's is never ended by Sway Things.
        let unit = waybar(3, Some("/c/sway-things/waybar/config.jsonc"), true);
        assert_eq!(surplus(&block, &[unit], &files()), Vec::<Waybar>::new());
    }

    #[test]
    fn processes_are_known_by_their_start_time() {
        let me = PathBuf::from(format!("/proc/{}", std::process::id()));
        let start = start_time(&me).unwrap();
        let alive = Waybar {
            pid: std::process::id(),
            start,
            config: None,
            unit: false,
        };
        assert!(alive.is_alive());
        let reused = Waybar {
            start: start + 1,
            ..alive
        };
        assert!(!reused.is_alive());
        // A process that is gone gets no signal and isn't an error.
        assert_eq!(signal(&[reused], "USR2"), Ok(0));
        assert!(!is_waybar_process(&me));
    }

    #[test]
    fn the_lock_is_held_once_and_only_its_holder_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let short = Duration::from_millis(300);
        async_io::block_on(async {
            let first = Lock::take_in(dir.path(), short).await.unwrap();
            assert!(Lock::take_in(dir.path(), short).await.is_none());
            // Someone else's lock in its place stays when the first is dropped.
            std::fs::write(&first.path, "someone else").unwrap();
            drop(first);
            assert!(dir.path().join("sway-things-waybar.lock").exists());
            std::fs::remove_file(dir.path().join("sway-things-waybar.lock")).unwrap();
            let again = Lock::take_in(dir.path(), short).await.unwrap();
            drop(again);
            assert!(!dir.path().join("sway-things-waybar.lock").exists());
        });
    }

    #[test]
    fn problems_are_noted_by_a_word() {
        for problem in [
            ProcessError::NotInstalled,
            ProcessError::Tool("kill"),
            ProcessError::Tool("setsid"),
            ProcessError::Ended,
        ] {
            assert_eq!(ProcessError::from_word(problem.word()), Some(problem));
        }
    }
}
