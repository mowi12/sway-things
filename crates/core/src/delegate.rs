// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The specialist tools a page hands what it doesn't cover to: its "Advanced…" button starts one
//! (pwvucontrol or pavucontrol for Sound, blueman-manager for Bluetooth, nm-connection-editor for
//! Network, system-config-printer for Printers). A button only shows while its tool is installed.

use std::ffi::OsStr;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;

use async_process::{Child, Command};

/// A tool a page delegates to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tool {
    /// The name of its executable, looked up in `PATH`.
    pub program: &'static str,
}

/// blueman's manager, for what the Bluetooth page doesn't do (adapter settings, file transfer,
/// network access, audio profiles).
pub const BLUEMAN: Tool = Tool {
    program: "blueman-manager",
};

/// NetworkManager's connection editor, for what the Network page doesn't do (802.1X, VPNs, IP
/// settings, other kinds of connections).
pub const NM_CONNECTION_EDITOR: Tool = Tool {
    program: "nm-connection-editor",
};

/// pwvucontrol, the volume control of PipeWire, for what the Sound page doesn't do (balance,
/// latency offsets, the routing of every stream). The Sound page prefers it to pavucontrol.
pub const PWVUCONTROL: Tool = Tool {
    program: "pwvucontrol",
};

/// pavucontrol, PulseAudio's volume control, which works with pipewire-pulse too; for the Sound
/// page when pwvucontrol isn't installed.
pub const PAVUCONTROL: Tool = Tool {
    program: "pavucontrol",
};

/// system-config-printer, for what the Printers page doesn't do: printers that need a driver,
/// classes, CUPS's server settings.
pub const SYSTEM_CONFIG_PRINTER: Tool = Tool {
    program: "system-config-printer",
};

/// nm-connection-editor's arguments for a new Wi-Fi connection.
pub const NEW_WIFI: &[&str] = &["--create", "--type=802-11-wireless"];

/// nm-connection-editor's arguments to edit the connection with `uuid`; `None` if `uuid` isn't
/// one (36 hexadecimal digits and dashes), so nothing else becomes an argument.
pub fn edit_connection(uuid: &str) -> Option<[String; 2]> {
    let valid = uuid.len() == 36
        && uuid.chars().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    valid.then(|| ["--edit".to_owned(), uuid.to_owned()])
}

impl Tool {
    /// Where the tool is installed, if it is.
    pub fn find(self) -> Option<PathBuf> {
        find_program(self.program, std::env::var_os("PATH").as_deref())
    }

    /// Starts the tool, without arguments. Wait for the child, so it doesn't stay behind as a
    /// zombie once it ends.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::NotFound`] if it isn't installed, else the error of starting it.
    pub fn launch(self) -> io::Result<Child> {
        self.launch_with(&[])
    }

    /// Starts the tool with the arguments `args`, each passed as it is (never through a shell).
    /// Wait for the child, as for [`launch`](Self::launch).
    ///
    /// # Errors
    ///
    /// As [`launch`](Self::launch).
    pub fn launch_with(self, args: &[&str]) -> io::Result<Child> {
        let program = self.find().ok_or(io::ErrorKind::NotFound)?;
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }
}

/// The path of the executable `name` in one of the directories of `path` (a `PATH` value).
pub(crate) fn find_program(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use super::*;

    #[test]
    fn only_uuids_are_edited() {
        let uuid = "0b9e5d6c-1f2a-4b3c-8d4e-5f6a7b8c9d0e";
        assert_eq!(
            edit_connection(uuid),
            Some(["--edit".to_owned(), uuid.to_owned()])
        );
        assert_eq!(edit_connection("--help"), None);
        assert_eq!(
            edit_connection("0b9e5d6c 1f2a-4b3c-8d4e-5f6a7b8c9d0e"),
            None
        );
        assert_eq!(edit_connection(""), None);
    }

    #[test]
    fn programs_are_found_only_as_executable_files_in_absolute_directories() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("blueman-manager");
        fs::write(&tool, "").unwrap();
        let path = OsString::from(format!("relative:{}", dir.path().display()));
        assert_eq!(find_program("blueman-manager", Some(&path)), None);
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_program("blueman-manager", Some(&path)), Some(tool));
        assert_eq!(find_program("blueman-manager", None), None);
        assert_eq!(find_program("missing", Some(&path)), None);
    }
}
