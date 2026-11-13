// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Whether the user's sway config includes the files Sway Things generates.
//!
//! Sway Things never adds its include line itself. Until the user did, nothing it generates is
//! in effect, so it stays read-only: [`IncludeStatus::detect`] reads the user's config, and
//! only a status that found the line hands out the [`WriteAccess`] that saving settings and
//! writing generated files require.
//!
//! The line counts in any spelling sway resolves to the generated files (`~`, `$HOME`, an
//! absolute path, a glob), in the main config or in a file it includes, and only if it covers
//! every file Sway Things can generate, also those that weren't written yet. An
//! [`IncludeWatch`] keeps the status current while the user edits the config.
//!
//! ```
//! use std::path::Path;
//! use sway_things_core::generate::Locations;
//! use sway_things_core::include::{IncludeStatus, Inclusion};
//! use sway_things_core::sway_config::MemoryFiles;
//!
//! let locations = Locations::new(
//!     Path::new("/home/me/.config/sway/sway-things"),
//!     Path::new("/home/me/.local/state/sway-things"),
//! );
//! let main = Path::new("/home/me/.config/sway/config");
//! let files = MemoryFiles::new()
//!     .var("HOME", "/home/me")
//!     .file("/home/me/.config/sway/config", "gaps inner 8\n");
//!
//! let status = IncludeStatus::detect(Some(main), &files, &locations);
//! assert_eq!(status.inclusion(), &Inclusion::Missing);
//! assert_eq!(status.instructions().line(), "include ~/.config/sway/sway-things/*.conf");
//! assert!(status.write_access().is_err());
//!
//! let files = files.file(
//!     "/home/me/.config/sway/config",
//!     "include $HOME/.config/sway/sway-things/*.conf\ngaps inner 8\n",
//! );
//! let status = IncludeStatus::detect(Some(main), &files, &locations);
//! assert!(status.write_access().is_ok());
//! // The user's `gaps` line runs after the generated files, so it wins over Sway Things.
//! assert_eq!(status.after().len(), 1);
//! ```

mod instructions;
mod overlay;
#[cfg(test)]
mod tests;
mod watch;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub use instructions::Instructions;
use instructions::tilde;
use overlay::Overlay;
pub use watch::{IncludeWatch, MainConfig};

use crate::generate::Locations;
use crate::registry::ConfigFile;
use crate::sway_config::{
    Diagnostic, DiagnosticKind, Directive, Files, Include, SwayConfig, parse,
};

/// Whether the user's sway config includes the generated files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inclusion {
    /// Every generated file is included. Settings can be changed.
    Active,
    /// Only some of the generated files are included, so a setting in one of the others would
    /// be lost at the next reload.
    Partial {
        /// The files no include covers.
        missing: Vec<ConfigFile>,
    },
    /// No include of the generated files was found, but part of the config couldn't be
    /// followed: the line may be in there.
    Unknown {
        /// What couldn't be followed: includes that need a shell command, files that can't be
        /// read, and includes nested too deeply.
        unfollowed: Vec<Diagnostic>,
    },
    /// The config doesn't include the generated files, or there is no config.
    Missing,
}

/// The user's sway config, and whether and where it includes the generated files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeStatus {
    main: Option<PathBuf>,
    inclusion: Inclusion,
    config: SwayConfig,
    positions: BTreeMap<ConfigFile, usize>,
    lines: Vec<Include>,
    instructions: Instructions,
    home: Option<PathBuf>,
}

/// The user's config without the generated files, and where sway reads those.
struct Found {
    config: SwayConfig,
    /// For each generated file that is included, how many of the user's directives sway runs
    /// before it.
    positions: BTreeMap<ConfigFile, usize>,
    /// The includes that read a generated file.
    lines: Vec<Include>,
}

/// Parses the config at `main` and takes the generated files out of it.
fn find(main: &Path, overlay: &Overlay<'_>) -> Found {
    let generated: BTreeMap<PathBuf, ConfigFile> = ConfigFile::ALL
        .into_iter()
        .map(|file| (overlay.path(file), file))
        .collect();
    let mut config = parse(main, overlay);
    let mut positions = BTreeMap::new();
    let mut directives = Vec::with_capacity(config.directives.len());
    for directive in config.directives {
        match generated.get(&*directive.file) {
            Some(&file) => {
                positions.entry(file).or_insert(directives.len());
            }
            None => directives.push(directive),
        }
    }
    config.directives = directives;
    config.files.retain(|file| !generated.contains_key(&**file));
    let lines = config
        .includes
        .iter()
        .filter(|include| {
            include
                .loaded
                .iter()
                .any(|file| generated.contains_key(&**file))
        })
        .cloned()
        .collect();
    Found {
        config,
        positions,
        lines,
    }
}

impl IncludeStatus {
    /// Reads the sway config at `main` through `files` and looks for includes of the generated
    /// files in `locations`. `main` is the config sway loads: the one `GET_VERSION` names in a
    /// session, otherwise [`default_path`](crate::sway_config::default_path); `None` if there
    /// is none.
    ///
    /// Nothing is executed and nothing is written. The files on disk decide, not what the
    /// running sway loaded: the line counts as soon as it is saved.
    pub fn detect(main: Option<&Path>, files: &dyn Files, locations: &Locations) -> Self {
        let overlay = Overlay::new(files, locations.directory());
        let found = match main {
            Some(main) => find(main, &overlay),
            None => Found {
                config: SwayConfig::default(),
                positions: BTreeMap::new(),
                lines: Vec::new(),
            },
        };
        let missing: Vec<ConfigFile> = ConfigFile::ALL
            .into_iter()
            .filter(|file| !found.positions.contains_key(file))
            .collect();
        // A main config that can't be read has no files, and nothing in it to follow.
        let unfollowed: Vec<Diagnostic> = found
            .config
            .diagnostics
            .iter()
            .filter(|diagnostic| match diagnostic.kind {
                DiagnosticKind::UnresolvedInclude { .. } | DiagnosticKind::TooDeep => true,
                DiagnosticKind::Unreadable { .. }
                | DiagnosticKind::NotAFile
                | DiagnosticKind::TooBig => !found.config.files.is_empty(),
                _ => false,
            })
            .cloned()
            .collect();
        let inclusion = if missing.is_empty() {
            Inclusion::Active
        } else if !found.positions.is_empty() {
            Inclusion::Partial { missing }
        } else if !unfollowed.is_empty() {
            Inclusion::Unknown { unfollowed }
        } else {
            Inclusion::Missing
        };
        Self {
            main: main.map(Path::to_owned),
            inclusion,
            config: found.config,
            positions: found.positions,
            lines: found.lines,
            instructions: Instructions::new(main, files, locations.directory()),
            home: files
                .env("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute()),
        }
    }

    /// The main config that was read, as given to [`IncludeStatus::detect`].
    pub fn main(&self) -> Option<&Path> {
        self.main.as_deref()
    }

    /// Whether the generated files are included.
    pub fn inclusion(&self) -> &Inclusion {
        &self.inclusion
    }

    /// Whether every generated file is included, so that settings can be changed.
    pub fn is_active(&self) -> bool {
        self.inclusion == Inclusion::Active
    }

    /// The user's config: every directive except those of the generated files, in the order
    /// sway runs them, with the parser's diagnostics.
    pub fn config(&self) -> &SwayConfig {
        &self.config
    }

    /// The `include` lines that read a generated file, with file and line. Normally one.
    pub fn lines(&self) -> &[Include] {
        &self.lines
    }

    /// How many directives of [`IncludeStatus::config`] sway runs before it reads `file`:
    /// those lose against a setting in `file`, the ones from this index on win. `None` if the
    /// file isn't included.
    pub fn position(&self, file: ConfigFile) -> Option<usize> {
        self.positions.get(&file).copied()
    }

    /// The user's directives that sway runs after it started reading the generated files. Where
    /// one of them sets the same thing as Sway Things, the user's line wins. Empty if no
    /// generated file is included.
    pub fn after(&self) -> &[Directive] {
        self.positions
            .values()
            .min()
            .and_then(|&first| self.config.directives.get(first..))
            .unwrap_or_default()
    }

    /// Whether the generated files are included and nothing of the user's config runs after
    /// them, as it is with the line at the end of the main config.
    pub fn is_last(&self) -> bool {
        !self.positions.is_empty() && self.after().is_empty()
    }

    /// What the user has to do to enable Sway Things: the line and where it goes.
    pub fn instructions(&self) -> &Instructions {
        &self.instructions
    }

    /// The status in words, for the welcome screen, the banner and the CLI. While Sway Things is
    /// read-only, it ends with the line to add.
    pub fn message(&self) -> String {
        let explanation = self.explanation();
        if self.is_active() {
            explanation
        } else {
            format!("{explanation}\n\n    {}", self.instructions.line())
        }
    }

    /// [`IncludeStatus::message`] without the line to add, for the app, which shows the line on
    /// its own with a copy button. While Sway Things is read-only, it ends with a colon.
    pub fn explanation(&self) -> String {
        let places = self
            .lines
            .iter()
            .map(|include| self.place(&include.file, include.line))
            .collect::<Vec<_>>()
            .join(", ");
        match &self.inclusion {
            Inclusion::Active if self.is_last() => {
                format!("Sway Things is enabled: {places} includes its files.")
            }
            Inclusion::Active => format!(
                "Sway Things is enabled: {places} includes its files. {} of your config lines \
                 come after them and win where they set the same thing as Sway Things.",
                self.after().len()
            ),
            Inclusion::Partial { missing } => format!(
                "Not enabled yet: your config includes only some of the Sway Things files \
                 ({places}). Not included: {}. Use this line instead:",
                missing
                    .iter()
                    .map(|file| file.file_name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Inclusion::Unknown { unfollowed } => format!(
                "Not enabled yet: Sway Things can't follow part of your config:\n\n{}\n\nIf the \
                 Sway Things line is in there, add it to {} instead:",
                unfollowed
                    .iter()
                    .map(|diagnostic| format!("    {}", self.unfollowed(diagnostic)))
                    .collect::<Vec<_>>()
                    .join("\n"),
                self.instructions.config_display()
            ),
            Inclusion::Missing => self.instructions.explanation(),
        }
    }

    /// `file:line` for showing, with `~` for the home directory.
    fn place(&self, file: &Path, line: usize) -> String {
        let file = tilde(file, self.home.as_deref());
        if line == 0 {
            file
        } else {
            format!("{file}:{line}")
        }
    }

    fn unfollowed(&self, diagnostic: &Diagnostic) -> String {
        let place = self.place(&diagnostic.file, diagnostic.line);
        match &diagnostic.kind {
            DiagnosticKind::UnresolvedInclude { pattern, reason } => {
                format!("{place}: include {pattern} ({reason})")
            }
            DiagnosticKind::Unreadable { message } => format!("{place} ({message})"),
            DiagnosticKind::TooDeep => format!("{place} (includes are nested too deeply)"),
            DiagnosticKind::NotAFile => format!("{place} (not a regular file)"),
            _ => format!("{place} (too big to read)"),
        }
    }

    /// The permission to save settings and to write generated files.
    ///
    /// # Errors
    ///
    /// [`ReadOnly`] unless every generated file is included, with what to tell the user.
    pub fn write_access(&self) -> Result<WriteAccess, ReadOnly> {
        if self.is_active() {
            Ok(WriteAccess { _private: () })
        } else {
            Err(ReadOnly(Box::new(Reason {
                inclusion: self.inclusion.clone(),
                explanation: self.message(),
                instructions: self.instructions.clone(),
            })))
        }
    }
}

/// The permission to save settings and to write generated files, which only an
/// [`IncludeStatus`] that found the include line hands out. [`State::save`](crate::state::State)
/// and [`generate::write`](crate::generate::write) require it, so nothing is persisted while
/// sway wouldn't read it.
///
/// It says that the line was there when the config was last read. Get a new one from the
/// current status for each change instead of keeping it.
#[derive(Debug)]
pub struct WriteAccess {
    _private: (),
}

#[cfg(test)]
impl WriteAccess {
    /// For the tests of what requires it.
    pub(crate) fn for_tests() -> Self {
        Self { _private: () }
    }
}

/// Settings can't be changed, because the user's sway config doesn't include the generated
/// files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOnly(Box<Reason>);

/// Boxed, so that the error stays small in a `Result`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reason {
    inclusion: Inclusion,
    explanation: String,
    instructions: Instructions,
}

impl ReadOnly {
    /// Why: the line is missing, covers only some files, or may be where the parser can't look.
    pub fn inclusion(&self) -> &Inclusion {
        &self.0.inclusion
    }

    /// The whole explanation, as in [`IncludeStatus::message`].
    pub fn explanation(&self) -> &str {
        &self.0.explanation
    }

    /// The line to add and where.
    pub fn instructions(&self) -> &Instructions {
        &self.0.instructions
    }
}

impl fmt::Display for ReadOnly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Add the include line to change settings")
    }
}

impl std::error::Error for ReadOnly {}
