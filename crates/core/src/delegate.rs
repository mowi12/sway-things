// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The specialist tools a page hands what it doesn't cover to: its "Advanced…" button starts one
//! (blueman-manager for Bluetooth). A button only shows while its tool is installed.

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
        let program = self.find().ok_or(io::ErrorKind::NotFound)?;
        Command::new(program)
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
