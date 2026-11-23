// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SwayOSD, the on-screen display the media keys can show: whether its client is installed and
//! whether its server runs or is started.
//!
//! `swayosd-client` changes the volume, the brightness or the player and asks `swayosd-server`
//! over the session bus to show the new value. Without the server it does nothing at all, the
//! change included: it exits with an error. Sway Things never starts the server (no daemon); it
//! tells the user how to start it with sway.

use std::path::{Path, PathBuf};

use crate::sway_config::SwayConfig;

/// The client the bindings run.
pub const CLIENT: &str = "swayosd-client";
/// The server that shows the display.
pub const SERVER: &str = "swayosd-server";

/// Whether `swayosd-client` is installed (in `$PATH`).
pub fn installed() -> bool {
    installed_in(std::env::var_os("PATH").as_deref())
}

/// Whether `swayosd-client` is in one of the directories of `path`, a list like `$PATH`.
pub fn installed_in(path: Option<&std::ffi::OsStr>) -> bool {
    crate::delegate::find_program(CLIENT, path).is_some()
}

/// Whether and how the server of SwayOSD runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Server {
    /// It runs in this session.
    Runs,
    /// It doesn't run, but a line of the user's config starts it when sway starts: an `exec`,
    /// which sway doesn't run again on a reload.
    StartedByConfig {
        /// The file of the line.
        file: PathBuf,
        /// Its number, from 1.
        line: usize,
    },
    /// It doesn't run, but a systemd user unit of this name is enabled.
    Unit(String),
    /// It doesn't run, and nothing starts it.
    Missing,
}

/// Whether the server runs for this session, or what starts it: an `exec` or `exec_always` of
/// the user's `config` with `swayosd-server` in it (outside the files Sway Things generates in
/// `ours`), or an enabled systemd user unit of SwayOSD. Reads `/proc` and the directories of
/// the units; nothing is run.
pub fn server(config: &SwayConfig, ours: &Path) -> Server {
    if !crate::outputs::session_processes(SERVER).is_empty() {
        return Server::Runs;
    }
    started(config, ours, &unit_directories())
}

/// What starts the server, without asking whether it runs: `config` outside `ours`, then the
/// systemd units enabled in `units` (the `*.wants` directories in them).
pub fn started(config: &SwayConfig, ours: &Path, units: &[PathBuf]) -> Server {
    let line = config.directives.iter().find(|directive| {
        directive.block.is_empty()
            && (directive.is(&[], "exec") || directive.is(&[], "exec_always"))
            && starts_server(&directive.args.join(" "))
            && !directive.file.starts_with(ours)
    });
    if let Some(directive) = line {
        return Server::StartedByConfig {
            file: directive.file.to_path_buf(),
            line: directive.line,
        };
    }
    enabled_unit(units).map_or(Server::Missing, Server::Unit)
}

/// Whether the shell line `line` runs the server: a word `swayosd-server`, also with a path.
fn starts_server(line: &str) -> bool {
    line.split(|character: char| {
        character.is_whitespace() || matches!(character, ';' | '&' | '|' | '(' | ')' | '\'' | '"')
    })
    .any(|word| word.rsplit('/').next() == Some(SERVER))
}

/// The directories where systemd keeps the user units that are enabled: the user's own, then the
/// ones enabled for every user.
fn unit_directories() -> Vec<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".config"))
        });
    config
        .map(|config| config.join("systemd/user"))
        .into_iter()
        .chain([PathBuf::from("/etc/systemd/user")])
        .collect()
}

/// The first unit of SwayOSD's server enabled in `directories`: a `swayosd…` service in one of
/// their `*.wants` directories, other than the libinput backend, which is a system service.
fn enabled_unit(directories: &[PathBuf]) -> Option<String> {
    directories.iter().find_map(|directory| {
        let mut wants: Vec<PathBuf> = std::fs::read_dir(directory)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "wants")
            })
            .collect();
        wants.sort();
        wants.iter().find_map(|wants| {
            let mut units: Vec<String> = std::fs::read_dir(wants)
                .ok()?
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| {
                    name.starts_with("swayosd")
                        && name.ends_with(".service")
                        && !name.contains("libinput")
                })
                .collect();
            units.sort();
            units.into_iter().next()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sway_config::{MemoryFiles, parse};

    fn config(text: &str) -> SwayConfig {
        let files = MemoryFiles::new().file("/home/u/.config/sway/config", text);
        parse(Path::new("/home/u/.config/sway/config"), &files)
    }

    const OURS: &str = "/home/u/.config/sway/sway-things";

    #[test]
    fn an_exec_line_of_the_user_starts_the_server() {
        let found = started(
            &config("exec foot\nexec swayosd-server\n"),
            Path::new(OURS),
            &[],
        );
        assert_eq!(
            found,
            Server::StartedByConfig {
                file: PathBuf::from("/home/u/.config/sway/config"),
                line: 2,
            }
        );
        for line in [
            "exec_always /usr/bin/swayosd-server --top-margin 0.5",
            "exec sh -c 'pkill swayosd; swayosd-server'",
        ] {
            assert!(
                matches!(
                    started(&config(line), Path::new(OURS), &[]),
                    Server::StartedByConfig { .. }
                ),
                "{line}"
            );
        }
    }

    #[test]
    fn the_client_a_binding_or_a_mode_doesnt_start_the_server() {
        for text in [
            "exec swayosd-client --output-volume raise\n",
            "bindsym XF86AudioMute exec swayosd-server\n",
            "mode \"x\" {\n  exec swayosd-server\n}\n",
            "exec swayosd-server-other\n",
        ] {
            assert_eq!(
                started(&config(text), Path::new(OURS), &[]),
                Server::Missing,
                "{text}"
            );
        }
    }

    #[test]
    fn an_enabled_user_unit_starts_the_server() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let wants = dir.path().join("graphical-session.target.wants");
        std::fs::create_dir_all(&wants)?;
        std::fs::write(wants.join("swayosd-libinput-backend.service"), "")?;
        let units = [dir.path().to_path_buf()];
        assert_eq!(
            started(&config(""), Path::new(OURS), &units),
            Server::Missing
        );
        std::fs::write(wants.join("swayosd.service"), "")?;
        assert_eq!(
            started(&config(""), Path::new(OURS), &units),
            Server::Unit("swayosd.service".to_owned())
        );
        Ok(())
    }
}
