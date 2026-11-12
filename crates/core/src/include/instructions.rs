// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the user has to do to enable Sway Things: the line, and the file it goes into.

use std::path::{Path, PathBuf};

use super::find;
use super::overlay::Overlay;
use crate::registry::ConfigFile;
use crate::sway_config::Files;

/// The one thing the user has to do: add [`Instructions::line`] at the end of
/// [`Instructions::config`]. The GUI and the CLI show the same text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instructions {
    config: PathBuf,
    copy_from: Option<PathBuf>,
    line: String,
    home: Option<PathBuf>,
}

impl Instructions {
    /// The instructions for the main config `main` (`None` if sway finds none) and the generated
    /// files in `directory`.
    pub(super) fn new(main: Option<&Path>, files: &dyn Files, directory: &Path) -> Self {
        let home = files
            .env("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute());
        // The config of the system isn't the user's to edit, and sway stops reading it once the
        // user has one.
        let system = main.filter(|main| main.starts_with("/etc"));
        let config = match main {
            Some(main) if system.is_none() => main.to_owned(),
            _ => user_config(files, home.as_deref()),
        };
        let line = include_line(directory, &config, files, home.as_deref());
        Self {
            config,
            copy_from: system.map(Path::to_owned),
            line,
            home,
        }
    }

    /// The file the line goes into: the main config, or the one the user has to create.
    pub fn config(&self) -> &Path {
        &self.config
    }

    /// The config of the system, if sway reads that one because the user has none. The user's
    /// config starts as a copy of it.
    pub fn copy_from(&self) -> Option<&Path> {
        self.copy_from.as_deref()
    }

    /// The line to add, for the copy button: `include ~/.config/sway/sway-things/*.conf`.
    pub fn line(&self) -> &str {
        &self.line
    }

    /// [`Instructions::config`] for showing, with `~` for the home directory.
    pub fn config_display(&self) -> String {
        tilde(&self.config, self.home.as_deref())
    }

    /// The instructions as text, with the line on a line of its own at the end.
    pub fn text(&self) -> String {
        format!("{}\n\n    {}", self.explanation(), self.line)
    }

    /// The instructions as text without the line, which the app shows on its own with a copy
    /// button. It ends with a colon.
    pub fn explanation(&self) -> String {
        let config = self.config_display();
        let place = match &self.copy_from {
            Some(system) => format!(
                "needs one line in it. You have no sway config of your own yet: copy {} to \
                 {config} first, then add this line at its end",
                system.display()
            ),
            None => format!("needs one line at the end of {config}"),
        };
        format!("Sway Things never edits your sway config. It writes its own files and {place}:")
    }
}

/// `path` with `~` for the home directory.
pub(super) fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Where sway looks for the user's config: `$XDG_CONFIG_HOME/sway/config`.
fn user_config(files: &dyn Files, home: Option<&Path>) -> PathBuf {
    files
        .env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home.map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("sway/config")
}

/// The include line for the generated files in `directory`, written into `config`: with `~` if
/// they are in the home directory, and with whatever quoting sway needs to read it as that
/// directory.
fn include_line(directory: &Path, config: &Path, files: &dyn Files, home: Option<&Path>) -> String {
    let absolute = directory.display().to_string();
    let short = tilde(directory, home);
    let mut candidates = Vec::new();
    for path in [&short, &absolute] {
        if path.chars().all(is_plain) && !candidates.contains(path) {
            candidates.push(path.clone());
        }
    }
    // sway resolves backslashes once when it reads the line, and the expansion of the path once
    // more; which of the two forms it needs depends on the character.
    for backslashes in ["\\", "\\\\"] {
        let mut escaped = String::new();
        for char in absolute.chars() {
            if !is_plain(char) {
                escaped.push_str(backslashes);
            }
            escaped.push(char);
        }
        candidates.push(escaped);
    }
    let line = |path: &str| format!("include {path}/*.conf");
    candidates
        .iter()
        .map(|path| line(path))
        .find(|line| {
            let overlay =
                Overlay::new(files, directory).replacing(config, format!("{line}\n").into_bytes());
            find(config, &overlay).positions.len() == ConfigFile::ALL.len()
        })
        .unwrap_or_else(|| line(&absolute))
}

/// Whether `char` means only itself to sway and to the expansion of an include path. The `~` is
/// only at the start of a path here, where it is meant.
fn is_plain(char: char) -> bool {
    char.is_alphanumeric() || "/._-+@,:%=~".contains(char)
}
