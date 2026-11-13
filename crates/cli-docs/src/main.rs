// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `sway-things-cli-docs <DATA_DIR>`: writes the man pages and shell completions below
//! `<DATA_DIR>` and removes the files there it no longer generates. `scripts/update-cli-docs.sh`
//! runs it.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cli_docs::{COMPLETIONS_DIR, MAN_DIR, files};

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let [data] = arguments.as_slice() else {
        eprintln!("usage: sway-things-cli-docs <DATA_DIR>");
        return ExitCode::FAILURE;
    };
    match write(Path::new(data)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Writes every generated file below `data`, then removes the files in its directories that
/// aren't generated any more.
fn write(data: &Path) -> io::Result<()> {
    let files = files()?;
    let mut written = BTreeSet::new();
    for (path, content) in files {
        let path = data.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content).map_err(|error| {
            io::Error::new(error.kind(), format!("{}: {error}", path.display()))
        })?;
        written.insert(path);
    }
    for directory in [MAN_DIR, COMPLETIONS_DIR] {
        for entry in fs::read_dir(data.join(directory))? {
            let path: PathBuf = entry?.path();
            if path.is_file() && !written.contains(&path) {
                fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}
