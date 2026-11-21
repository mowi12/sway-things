// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing the generated files: validated by sway first, then replaced atomically.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_process::Command;

use super::{GeneratedFile, Manifest, Ownership, Source};
use crate::atomic;
use crate::i18n::Text;
use crate::include::WriteAccess;
use crate::ipc::Compositor;
use crate::msg;
use crate::registry::ConfigFile;
pub use crate::state::Locations;

/// An error sway found in a config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// The number of the line, counted from 1.
    pub line: usize,
    /// sway's message.
    pub message: String,
}

/// Checks a staged config file before it replaces the old one.
pub trait Validate {
    /// Validates the config file at `staged` as a main config of its own and returns the errors
    /// in it; none if it is valid.
    ///
    /// # Errors
    ///
    /// The I/O error if the file can't be checked at all.
    fn validate(&self, staged: &Path) -> impl Future<Output = io::Result<Vec<ConfigError>>>;
}

/// Validates with `sway --validate`, which works without a running session.
///
/// sway ignores errors in included files, so each generated file is validated as the main config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwayValidator {
    program: OsString,
}

impl SwayValidator {
    /// Validates with the `sway` on `PATH`. SwayFX installs itself under that name too.
    pub fn new() -> Self {
        Self::with_program("sway")
    }

    /// Validates with another sway binary.
    pub fn with_program(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl SwayValidator {
    /// The compositor this binary is, from `sway --version`: the one to generate for when no
    /// session is running to ask over IPC. `None` if the output isn't a version of sway or
    /// SwayFX.
    ///
    /// # Errors
    ///
    /// The I/O error if the binary can't be run.
    pub async fn compositor(&self) -> io::Result<Option<Compositor>> {
        let output = Command::new(&self.program)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .await?;
        Ok(Compositor::from_version_output(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }
}

impl Default for SwayValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl Validate for SwayValidator {
    async fn validate(&self, staged: &Path) -> io::Result<Vec<ConfigError>> {
        // An argument array, never a shell.
        let output = Command::new(&self.program)
            .arg("--validate")
            .arg("--config")
            .arg(staged)
            .stdin(Stdio::null())
            .output()
            .await?;
        let log = String::from_utf8_lossy(&output.stderr);
        let mut errors: Vec<ConfigError> = log
            .lines()
            .filter_map(|line| parse_error(line, staged))
            .collect();
        if errors.is_empty() && !output.status.success() {
            // sway refused the file without naming a line.
            errors.push(ConfigError {
                line: 0,
                message: log.trim().to_owned(),
            });
        }
        Ok(errors)
    }
}

/// Reads a line of sway's log like
/// `00:00:00.004 [ERROR] [sway/config.c:865] Error on line 3 'gaps inner x': Invalid … (/path)`.
pub(super) fn parse_error(log_line: &str, config: &Path) -> Option<ConfigError> {
    let (_, error) = log_line.split_once("Error on line ")?;
    let (line, rest) = error.split_once(' ')?;
    let line = line.parse().ok()?;
    // sway appends the path of the config in parentheses.
    let rest = rest
        .strip_suffix(&format!(" ({})", config.display()))
        .unwrap_or(rest);
    // The config line is quoted and may itself contain `': `, so the message starts after the
    // last one.
    let message = rest.rsplit_once("': ").map_or(rest, |(_, message)| message);
    Some(ConfigError {
        line,
        message: message.to_owned(),
    })
}

/// An error in a generated file, with the settings behind the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidLine {
    /// The number of the line, counted from 1; 0 if sway didn't name one.
    pub line: usize,
    /// sway's message.
    pub message: String,
    /// The settings the line comes from, if it is a line below the header.
    pub source: Option<Source>,
}

/// What happened to a generated file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The file was validated and written.
    Written,
    /// The file already held the content.
    Unchanged,
    /// The file has no content and doesn't exist, so it wasn't created.
    NotCreated,
    /// The file was changed outside Sway Things and was left alone. Writing again with the file
    /// in `overwrite` replaces it.
    EditedOutside,
    /// The file exists but wasn't generated by Sway Things. It is never written.
    Foreign,
    /// sway rejected the new content. The old file is still in place.
    Invalid(Vec<InvalidLine>),
}

impl Outcome {
    /// What went wrong with `file`, in words for the app and the command line, or `None` if
    /// nothing did.
    pub fn problem(&self, file: ConfigFile) -> Option<Text> {
        let name = file.file_name();
        match self {
            Self::Written | Self::Unchanged | Self::NotCreated => None,
            Self::EditedOutside => Some(
                Text::new(msg!(
                    "{file} was changed outside Sway Things, so it was left alone"
                ))
                .arg("file", name),
            ),
            Self::Foreign => Some(
                Text::new(msg!(
                    "{file} wasn't made by Sway Things, so it is never written"
                ))
                .arg("file", name),
            ),
            Self::Invalid(lines) => {
                // sway's message stays as it is.
                let text = match lines.first() {
                    None => Text::new(msg!("sway rejected the new {file}, so the old one stays")),
                    Some(InvalidLine { line: 0, .. }) => Text::new(msg!(
                        "sway rejected the new {file}, so the old one stays: {message}"
                    )),
                    Some(_) => Text::new(msg!(
                        "sway rejected the new {file}, so the old one stays: line {number}: \
                         {message}"
                    )),
                };
                let text = text.arg("file", name);
                Some(match lines.first() {
                    Some(line) => text.arg("number", line.line).arg("message", &line.message),
                    None => text,
                })
            }
        }
    }
}

/// The files in the order they were given, each with what happened to it.
pub type Report = Vec<(ConfigFile, Outcome)>;

/// A generated file as it is on disk, next to what Sway Things would write now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnDisk {
    /// Which file this is.
    pub file: ConfigFile,
    /// Where it is.
    pub path: PathBuf,
    /// How the file relates to what Sway Things would write, or why that can't be told.
    pub ownership: Result<Ownership, String>,
    /// The text of the file, if it exists and can be read; bytes that aren't UTF-8 are replaced.
    pub content: Option<String>,
}

/// Reads the generated `files` as they are in `locations`, without writing anything: for "Show
/// generated config", which marks the files that were changed outside Sway Things.
pub fn on_disk(files: &[GeneratedFile], locations: &Locations) -> Vec<OnDisk> {
    let manifest = Manifest::load(locations.manifest());
    files
        .iter()
        .map(|file| {
            let path = locations.path(file.file());
            let ownership = manifest
                .ownership(&path, &file.text())
                .map_err(|error| error.to_string());
            let content = fs::read(&path)
                .ok()
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
            OnDisk {
                file: file.file(),
                path,
                ownership,
                content,
            }
        })
        .collect()
}

/// Writes `files` to their places in `locations`.
///
/// For each file whose content changed: the new content is staged next to it, `validator` checks
/// the staged file, and only a valid one replaces the old file, atomically. Then the manifest
/// records its hash. A file that is a symbolic link stays one.
///
/// A file is only written if Sway Things owns it ([`Ownership`]): one that was changed outside is
/// left alone and reported, unless it is listed in `overwrite` (after asking the user); one
/// without the header that isn't in the manifest is never written. A file without content is not
/// created, but an existing one is emptied down to the header.
///
/// Writing needs the [`WriteAccess`] of an [`IncludeStatus`](crate::include::IncludeStatus):
/// while the user's sway config doesn't include the generated files, none is written.
///
/// # Errors
///
/// [`WriteError`] if a file or the manifest can't be read or written, or the validator can't
/// run. Files written before the failing one stay written and recorded.
pub async fn write(
    files: &[GeneratedFile],
    locations: &Locations,
    validator: &impl Validate,
    overwrite: &[ConfigFile],
    _access: &WriteAccess,
) -> Result<Report, WriteError> {
    let mut manifest = Manifest::load(locations.manifest());
    let mut report = Vec::with_capacity(files.len());
    let mut failure = None;
    for file in files {
        match write_file(file, locations, validator, overwrite, &mut manifest).await {
            Ok(outcome) => report.push((file.file(), outcome)),
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    // Also after a failure: what was written before it is on disk.
    let saved = manifest.save().map_err(|source| WriteError::Io {
        path: locations.manifest().to_owned(),
        source,
    });
    match failure {
        Some(error) => Err(error),
        None => saved.map(|()| report),
    }
}

async fn write_file(
    file: &GeneratedFile,
    locations: &Locations,
    validator: &impl Validate,
    overwrite: &[ConfigFile],
    manifest: &mut Manifest,
) -> Result<Outcome, WriteError> {
    let path = locations.path(file.file());
    let io_error = |source| WriteError::Io {
        path: path.clone(),
        source,
    };
    let text = file.text();
    match manifest.ownership(&path, &text).map_err(io_error)? {
        Ownership::Absent if file.is_empty() => {
            manifest.forget(&path);
            return Ok(Outcome::NotCreated);
        }
        Ownership::Current => {
            manifest.record(&path, &text).map_err(io_error)?;
            return Ok(Outcome::Unchanged);
        }
        Ownership::Foreign => return Ok(Outcome::Foreign),
        Ownership::Edited if !overwrite.contains(&file.file()) => {
            return Ok(Outcome::EditedOutside);
        }
        Ownership::Absent | Ownership::Ours | Ownership::Edited => {}
    }

    // Hidden and not `*.conf`: the include in the user's config never matches the staged file.
    let staged = atomic::stage(&path, &text, "staged").map_err(io_error)?;
    let errors = validator
        .validate(staged.path())
        .await
        .map_err(WriteError::Validator)?;
    if !errors.is_empty() {
        let invalid = errors
            .into_iter()
            .map(|error| InvalidLine {
                source: file.source_of_line(error.line).cloned(),
                line: error.line,
                message: error.message,
            })
            .collect();
        return Ok(Outcome::Invalid(invalid));
    }
    // Exactly the bytes sway checked replace the old file; only then are they recorded.
    staged.commit().map_err(io_error)?;
    manifest.record(&path, &text).map_err(io_error)?;
    Ok(Outcome::Written)
}

/// Why the generated files couldn't be written.
#[derive(Debug)]
pub enum WriteError {
    /// A generated file or the manifest can't be read or written.
    Io {
        /// The file.
        path: PathBuf,
        /// The error of the operating system.
        source: io::Error,
    },
    /// The validator can't run, e.g. because `sway` isn't installed. Nothing was replaced
    /// without validation.
    Validator(io::Error),
}

impl WriteError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Io { path, source } => Text::new(msg!("{file}: {error}"))
                .arg("file", path.display())
                .arg("error", source),
            Self::Validator(source) => {
                Text::new(msg!("can't run sway to validate: {error}")).arg("error", source)
            }
        }
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } | Self::Validator(source) => Some(source),
        }
    }
}
