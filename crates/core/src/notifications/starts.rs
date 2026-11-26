// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What starts the notification daemon: an `exec` line of the user's sway config, the line of
//! Sway Things' include, a systemd user unit, or the bus itself (D-Bus activation: a call to
//! `org.freedesktop.Notifications` while nobody owns it starts the daemon whose service file
//! claims the name). Reads files and `/proc`; nothing is run.

use std::path::{Path, PathBuf};

use super::Daemon;
use crate::sway_config::SwayConfig;

/// An `exec` or `exec_always` line that starts a notification daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartLine {
    /// The daemon it starts.
    pub daemon: Daemon,
    /// The file it is in.
    pub file: PathBuf,
    /// Its line, counted from 1.
    pub line: usize,
    /// The line as written.
    pub text: String,
    /// Whether it is in a file Sway Things generates.
    pub ours: bool,
}

/// The daemon an `exec` command starts, if any: the program of one of its commands (split at
/// `;`, `&` and `|`, quotes and `sh -c` taken apart, variable assignments and `exec` before it
/// skipped) whose file name is `mako`, `swaync` or `dunst`. `pkill mako`, `systemctl restart
/// mako` and `swaync-client -t` start none; `killall swaync; swaync` starts swaync.
pub fn daemon_in(command: &str) -> Option<Daemon> {
    command
        .replace(['\'', '"', '`', '(', ')'], " ")
        .split([';', '&', '|', '\n'])
        .find_map(|part| {
            let mut words = part.split_whitespace().skip_while(|word| {
                word.contains('=') || ["exec", "env", "nohup", "setsid"].contains(word)
            });
            let program = words.next()?;
            let name = program.rsplit('/').next()?;
            if name == "sh" || name == "bash" {
                // `sh -c 'swaync'`: the command follows `-c`, its quotes already gone.
                let rest: Vec<&str> = words.skip_while(|word| *word != "-c").skip(1).collect();
                return daemon_in(&rest.join(" "));
            }
            Daemon::from_program(name)
        })
}

/// The config a daemon's process `pid` was given with `-c`/`--config` (`--config=…`), from
/// `/proc/<pid>/cmdline`.
pub fn config_arg(pid: u32) -> Option<PathBuf> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = cmdline
        .split(|byte| *byte == 0)
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect();
    config_in(&args)
}

/// The config named by `-c`/`--config` among `args` (the program first).
pub fn config_in(args: &[String]) -> Option<PathBuf> {
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        if let Some(path) = arg.strip_prefix("--config=") {
            return Some(PathBuf::from(path));
        }
        if arg == "-c" || arg == "--config" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

/// The `exec` and `exec_always` lines of `config` that start a notification daemon, with those
/// in `ours` (the directory of the generated files) marked. Our line in `80-hooks.conf` that
/// runs `sway-things hook notification-daemon` starts `chosen`, the daemon chosen on the page;
/// it is read from the file, since the parser leaves our files out.
pub fn start_lines(config: &SwayConfig, ours: &Path, chosen: Option<Daemon>) -> Vec<StartLine> {
    let hooks = ours.join(crate::registry::ConfigFile::Hooks.file_name());
    let hook_line = super::start_line()
        .render(crate::command::Target::Config)
        .ok();
    let hook = chosen.and_then(|daemon| {
        let text = std::fs::read_to_string(&hooks).ok()?;
        let (index, line) = text
            .lines()
            .enumerate()
            .find(|(_, line)| Some(line.trim()) == hook_line.as_deref())?;
        Some(StartLine {
            daemon,
            file: hooks.clone(),
            line: index + 1,
            text: line.trim().to_owned(),
            ours: true,
        })
    });
    let mut lines: Vec<StartLine> = config
        .directives
        .iter()
        .filter(|directive| directive.is(&[], "exec") || directive.is(&[], "exec_always"))
        .filter_map(|directive| {
            let command = directive.args.join(" ");
            let daemon = daemon_in(&command)?;
            Some(StartLine {
                daemon,
                file: directive.file.to_path_buf(),
                line: directive.line,
                text: directive.text.clone(),
                ours: directive.file.starts_with(ours),
            })
        })
        .filter(|line| !(line.ours && line.file == hooks))
        .collect();
    lines.extend(hook);
    lines
}

/// How the daemon that runs was started, as far as can be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// By a line of the user's sway config (or ours).
    Line(StartLine),
    /// As a systemd user unit of this name.
    Unit(String),
    /// By the bus or something else: no line of sway's starts it, and it runs in no unit of its
    /// own.
    Elsewhere,
}

/// How the daemon `daemon` with the process `pid` was started: a line of `lines` that starts
/// it (one of the user's first), else its control group, else elsewhere.
pub fn origin(daemon: Daemon, pid: Option<u32>, lines: &[StartLine]) -> Origin {
    if let Some(unit) = pid.and_then(unit_of) {
        return Origin::Unit(unit);
    }
    let mut starting: Vec<&StartLine> = lines.iter().filter(|line| line.daemon == daemon).collect();
    starting.sort_by_key(|line| line.ours);
    match starting.first() {
        Some(line) => Origin::Line((*line).clone()),
        None => Origin::Elsewhere,
    }
}

/// The systemd unit the process `pid` runs in, if it is a service of its own
/// (`…/app.slice/swaync.service`); one sway started is in the session's scope.
pub fn unit_of(pid: u32) -> Option<String> {
    let groups = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    groups
        .lines()
        .filter_map(|line| line.rsplit('/').next())
        .find(|unit| unit.ends_with(".service"))
        .map(ToOwned::to_owned)
}

/// The daemons that have a D-Bus service file for `org.freedesktop.Notifications`, which the
/// bus starts when something calls it while none runs: in `$XDG_DATA_HOME/dbus-1/services` and
/// `$XDG_DATA_DIRS/dbus-1/services`.
///
/// # Blocking
///
/// Reads the D-Bus service directories.
pub fn activatable() -> Vec<Daemon> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")))
    {
        dirs.push(data_home);
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|dir| dir.starts_with('/'))
            .map(PathBuf::from),
    );
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir.join("dbus-1/services")) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            if let Some(daemon) = service_daemon(&text).filter(|daemon| !found.contains(daemon)) {
                found.push(daemon);
            }
        }
    }
    found
}

/// The daemon a D-Bus service file starts for `org.freedesktop.Notifications`, if it is one.
pub fn service_daemon(text: &str) -> Option<Daemon> {
    let value = |key: &str| {
        text.lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix(key)?
                    .trim_start()
                    .strip_prefix('=')
            })
            .map(str::trim)
    };
    if value("Name")? != super::bus::NAME {
        return None;
    }
    let exec = value("Exec")?;
    daemon_in(exec)
}
