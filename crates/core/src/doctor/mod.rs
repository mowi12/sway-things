// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checking the setup: why a setting doesn't take effect, for `sway-things doctor` and the app.
//!
//! [`Checkup::diagnose`] looks at everything between a stored value and sway: the compositor,
//! the user's sway config and its include line, the settings files, the generated files (do they
//! hold the stored settings, did someone edit them, does sway accept them) and the lines of the
//! user's config that set the same thing. The [`Diagnosis`] is a list of [`Finding`]s, each with
//! a [`Severity`], a stable [`Code`], a message and a hint how to fix it.
//!
//! [`Checkup::fix`] writes the generated files again: those that are missing or out of date
//! (`90-effects.conf` after switching between sway and SwayFX), and those changed outside only
//! when asked to. Like every write, it needs the [`WriteAccess`] of an active
//! [`IncludeStatus`], and it never touches a file Sway Things didn't make.
//!
//! [`Checkup::diagnose_with`] also tells which [`Step`] it starts, so a front end can show
//! progress while sway validates the generated files.
//!
//! Without a session, the compositor is the installed one (`sway --version`), and the generated
//! files are still validated with `sway --validate`: that is how to find out from a TTY why sway
//! doesn't start. If `sway` can't be run either, validation is skipped, not failed.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::conflict;
use crate::generate::{
    self, ConfigError, GeneratedFile, HEADER, InvalidLine, Locations, Manifest, Ownership, Report,
    Source, SwayValidator, Validate, WriteError,
};
use crate::include::{IncludeStatus, Inclusion, MainConfig, WriteAccess};
use crate::ipc::{Client, Compositor, IpcError};
use crate::registry::{ConfigFile, SwayMapping};
use crate::state::{ProblemKind, State};
use crate::sway_config::{Diagnostic, DiagnosticKind};
use crate::version::{SwayFxVersion, SwayVersion};

/// How bad a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Nothing to do.
    Ok,
    /// Worth knowing, but the stored settings still take effect as far as Sway Things can tell.
    Warning,
    /// Stored settings don't take effect, or sway may not start.
    Problem,
}

impl Severity {
    /// `ok`, `warning` or `problem`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warning => "warning",
            Self::Problem => "problem",
        }
    }
}

/// The part of the setup a finding is about, in the order they are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Section {
    /// The running or installed sway or SwayFX.
    Compositor,
    /// The user's sway config: which file, the include line, what the parser couldn't follow.
    Config,
    /// The settings files of Sway Things.
    Settings,
    /// The generated sway config files.
    Generated,
    /// Lines of the user's config that set a stored setting.
    Conflicts,
}

impl Section {
    /// Every section, in order.
    pub const ALL: [Self; 5] = [
        Self::Compositor,
        Self::Config,
        Self::Settings,
        Self::Generated,
        Self::Conflicts,
    ];

    /// The id for scripts: `compositor`, `config`, `settings`, `generated`, `conflicts`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Compositor => "compositor",
            Self::Config => "config",
            Self::Settings => "settings",
            Self::Generated => "generated",
            Self::Conflicts => "conflicts",
        }
    }

    /// The heading.
    pub fn title(self) -> &'static str {
        match self {
            Self::Compositor => "Compositor",
            Self::Config => "Sway config",
            Self::Settings => "Settings files",
            Self::Generated => "Generated files",
            Self::Conflicts => "Conflicts",
        }
    }
}

/// What a finding is, as a stable id for scripts and the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    /// A supported sway or SwayFX runs in this session.
    Session,
    /// No session runs; the installed compositor is known.
    NoSession,
    /// sway runs but can't be used: too old, or it doesn't answer like sway.
    SessionUnusable,
    /// No session runs, and the installed compositor is older than Sway Things supports.
    InstalledUnsupported,
    /// No session runs, and `sway --version` names no version Sway Things knows.
    VersionUnknown,
    /// No session runs, and `sway` can't be run.
    SwayMissing,
    /// The main config sway reads.
    MainConfig,
    /// sway finds no config of the user's.
    NoConfig,
    /// The include line is there, at the end.
    IncludeActive,
    /// The include line is there, but lines of the user's come after it.
    IncludeNotLast,
    /// The config includes only some generated files.
    IncludePartial,
    /// The include line wasn't found, but part of the config can't be followed.
    IncludeUnknown,
    /// The config doesn't include the generated files.
    IncludeMissing,
    /// Something in the user's config the parser couldn't follow.
    ConfigDiagnostic,
    /// The settings files load without problems.
    SettingsLoaded,
    /// A settings file can't be used at all.
    SettingsUnusable,
    /// An entry of a settings file is ignored.
    SettingsEntry,
    /// A stored setting isn't in the generated file: the compositor doesn't support it, or its
    /// value can't be turned into a command.
    SettingSkipped,
    /// A generated file holds what Sway Things generates.
    FileCurrent,
    /// A generated file doesn't exist and isn't needed.
    FileNotNeeded,
    /// A generated file that should hold settings doesn't exist.
    FileMissing,
    /// A generated file doesn't hold what Sway Things generates now.
    FileOutOfDate,
    /// A generated file was changed outside Sway Things.
    FileEdited,
    /// A file in the place of a generated file wasn't made by Sway Things.
    FileForeign,
    /// A generated file can't be read.
    FileUnreadable,
    /// A generated file was made by Sway Things, but without a compositor to generate for it
    /// can't be told whether it is up to date.
    FileUnchecked,
    /// sway accepts a generated file.
    FileValid,
    /// sway rejects a generated file.
    FileInvalid,
    /// The generated files couldn't be validated, because sway can't be run.
    ValidationSkipped,
    /// No line of the user's config sets a stored setting.
    NoConflicts,
    /// A line of the user's config comes after the generated file and wins over a stored setting.
    ConfigWins,
    /// A line of the user's config comes before the generated file and has no effect while the
    /// setting is stored.
    ConfigLineDead,
}

impl Code {
    /// The id for scripts, in snake case: `file_out_of_date`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::NoSession => "no_session",
            Self::SessionUnusable => "session_unusable",
            Self::InstalledUnsupported => "installed_unsupported",
            Self::VersionUnknown => "version_unknown",
            Self::SwayMissing => "sway_missing",
            Self::MainConfig => "main_config",
            Self::NoConfig => "no_config",
            Self::IncludeActive => "include_active",
            Self::IncludeNotLast => "include_not_last",
            Self::IncludePartial => "include_partial",
            Self::IncludeUnknown => "include_unknown",
            Self::IncludeMissing => "include_missing",
            Self::ConfigDiagnostic => "config_diagnostic",
            Self::SettingsLoaded => "settings_loaded",
            Self::SettingsUnusable => "settings_unusable",
            Self::SettingsEntry => "settings_entry",
            Self::SettingSkipped => "setting_skipped",
            Self::FileCurrent => "file_current",
            Self::FileNotNeeded => "file_not_needed",
            Self::FileMissing => "file_missing",
            Self::FileOutOfDate => "file_out_of_date",
            Self::FileEdited => "file_edited",
            Self::FileForeign => "file_foreign",
            Self::FileUnreadable => "file_unreadable",
            Self::FileUnchecked => "file_unchecked",
            Self::FileValid => "file_valid",
            Self::FileInvalid => "file_invalid",
            Self::ValidationSkipped => "validation_skipped",
            Self::NoConflicts => "no_conflicts",
            Self::ConfigWins => "config_wins",
            Self::ConfigLineDead => "config_line_dead",
        }
    }

    /// The section the finding belongs to.
    pub fn section(self) -> Section {
        match self {
            Self::Session
            | Self::NoSession
            | Self::SessionUnusable
            | Self::InstalledUnsupported
            | Self::VersionUnknown
            | Self::SwayMissing => Section::Compositor,
            Self::MainConfig
            | Self::NoConfig
            | Self::IncludeActive
            | Self::IncludeNotLast
            | Self::IncludePartial
            | Self::IncludeUnknown
            | Self::IncludeMissing
            | Self::ConfigDiagnostic => Section::Config,
            Self::SettingsLoaded
            | Self::SettingsUnusable
            | Self::SettingsEntry
            | Self::SettingSkipped => Section::Settings,
            Self::FileCurrent
            | Self::FileNotNeeded
            | Self::FileMissing
            | Self::FileOutOfDate
            | Self::FileEdited
            | Self::FileForeign
            | Self::FileUnreadable
            | Self::FileUnchecked
            | Self::FileValid
            | Self::FileInvalid
            | Self::ValidationSkipped => Section::Generated,
            Self::NoConflicts | Self::ConfigWins | Self::ConfigLineDead => Section::Conflicts,
        }
    }
}

/// What [`Checkup::fix`] does about a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fix {
    /// Writes the generated file again.
    Regenerate,
    /// Writes the generated file again, but only when asked to overwrite files changed outside
    /// Sway Things.
    Overwrite,
}

impl Fix {
    /// `regenerate` or `overwrite`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Regenerate => "regenerate",
            Self::Overwrite => "overwrite",
        }
    }
}

/// One thing the checkup found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// What it is.
    pub code: Code,
    /// How bad it is.
    pub severity: Severity,
    /// What was found, in a sentence.
    pub message: String,
    /// More lines that belong to it: the lines sway rejects, the user's lines after the
    /// include.
    pub details: Vec<String>,
    /// How to fix it, if there is something to do that [`Finding::fix`] doesn't cover. Can be
    /// several lines.
    pub hint: Option<String>,
    /// The file it is about.
    pub file: Option<PathBuf>,
    /// The line in [`Finding::file`], counted from 1.
    pub line: Option<usize>,
    /// The setting it is about.
    pub key: Option<&'static str>,
    /// The input device of [`Finding::key`], or `None` for the whole session or input type.
    pub device: Option<String>,
    /// What [`Checkup::fix`] does about it, if anything.
    pub fix: Option<Fix>,
}

impl Finding {
    fn new(code: Code, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            details: Vec::new(),
            hint: None,
            file: None,
            line: None,
            key: None,
            device: None,
            fix: None,
        }
    }

    /// The section it belongs to.
    pub fn section(&self) -> Section {
        self.code.section()
    }

    fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    fn with_details(mut self, details: Vec<String>) -> Self {
        self.details = details;
        self
    }

    fn in_file(mut self, file: &Path) -> Self {
        self.file = Some(file.to_owned());
        self
    }

    /// In `file` at `line`; 0 for the whole file.
    fn at(mut self, file: &Path, line: usize) -> Self {
        self.file = Some(file.to_owned());
        self.line = (line > 0).then_some(line);
        self
    }

    fn for_setting(mut self, key: &'static str, device: Option<&str>) -> Self {
        self.key = Some(key);
        self.device = device.map(str::to_owned);
        self
    }

    fn fixed_by(mut self, fix: Option<Fix>) -> Self {
        self.fix = fix;
        self
    }
}

/// What the checkup is about to do, for showing progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The checks of this section start.
    Section(Section),
    /// `sway --validate` checks this generated file, which can take a moment.
    Validate(ConfigFile),
}

/// Everything the checkup found, in the order of the [`Section`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    findings: Vec<Finding>,
}

impl Diagnosis {
    /// The findings, section by section.
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// The worst severity of the findings: [`Severity::Ok`] when everything is fine.
    pub fn severity(&self) -> Severity {
        self.findings
            .iter()
            .map(|finding| finding.severity)
            .max()
            .unwrap_or(Severity::Ok)
    }

    /// How many findings have `severity`.
    pub fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    }

    /// Whether [`Checkup::fix`] can do something about a finding, with `overwrite` or without.
    pub fn can_fix(&self, overwrite: bool) -> bool {
        self.findings.iter().any(|finding| match finding.fix {
            Some(Fix::Regenerate) => true,
            Some(Fix::Overwrite) => overwrite,
            None => false,
        })
    }
}

/// The installed `sway`, as `sway --version` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    /// This compositor.
    Version(Compositor),
    /// `sway` runs, but doesn't print a version of sway or SwayFX that Sway Things can read.
    Unknown,
    /// `sway` can't be run, with the error.
    Missing(String),
}

impl Installed {
    /// Runs `sway --version` through `validator`.
    pub async fn of(validator: &SwayValidator) -> Self {
        match validator.compositor().await {
            Ok(Some(compositor)) => Self::Version(compositor),
            Ok(None) => Self::Unknown,
            Err(error) => Self::Missing(error.to_string()),
        }
    }
}

/// The compositor the checkup is for: the one in this session, or the installed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// A supported sway or SwayFX runs in this session.
    Session(Compositor),
    /// No session runs.
    NoSession {
        /// Why, e.g. `SWAYSOCK isn't set`.
        reason: String,
        /// The installed compositor.
        installed: Installed,
    },
    /// A session runs, but Sway Things can't use it: the compositor is too old, or it doesn't
    /// answer like sway does.
    Unusable {
        /// What went wrong.
        error: String,
        /// The installed compositor.
        installed: Installed,
    },
}

impl Found {
    /// Looks for the session in `$SWAYSOCK`, and without a usable one asks the installed `sway`
    /// through `validator`. Also returns the main config: the one the running sway loaded, or
    /// the one sway would find.
    pub async fn detect(validator: &SwayValidator) -> (Self, MainConfig) {
        let error = match Client::connect().await {
            Ok(mut client) => {
                let main = match client.version().await {
                    Ok(version) if !version.loaded_config_file_name.is_empty() => {
                        MainConfig::Path(version.loaded_config_file_name.into())
                    }
                    _ => MainConfig::Default,
                };
                return (Self::Session(client.compositor()), main);
            }
            Err(error) => error,
        };
        let installed = Installed::of(validator).await;
        let found = match error {
            IpcError::NoSession => Self::NoSession {
                reason: "SWAYSOCK isn't set".to_owned(),
                installed,
            },
            IpcError::NotRunning { socket, .. } => Self::NoSession {
                reason: format!("nothing answers on {}", socket.display()),
                installed,
            },
            error => Self::Unusable {
                error: error.to_string(),
                installed,
            },
        };
        (found, MainConfig::Default)
    }

    /// The compositor to generate for: the running one, else the installed one. `None` if
    /// neither is known.
    pub fn compositor(&self) -> Option<Compositor> {
        match self {
            Self::Session(compositor) => Some(*compositor),
            Self::NoSession { installed, .. } | Self::Unusable { installed, .. } => match installed
            {
                Installed::Version(compositor) => Some(*compositor),
                Installed::Unknown | Installed::Missing(_) => None,
            },
        }
    }

    /// Why `sway` can't be run, when no session runs. In a session, it is tried anyway.
    fn sway_missing(&self) -> Option<&str> {
        match self {
            Self::NoSession {
                installed: Installed::Missing(error),
                ..
            } => Some(error),
            _ => None,
        }
    }
}

/// Why [`Checkup::fix`] couldn't write the generated files.
#[derive(Debug)]
pub enum FixError {
    /// Neither a session nor `sway --version` says which compositor to generate for.
    NoCompositor,
    /// A file can't be written, or sway can't be run to validate it.
    Write(WriteError),
}

impl fmt::Display for FixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCompositor => f.write_str(
                "it isn't known which compositor to generate for: no sway session runs, and \
                 `sway --version` names none",
            ),
            Self::Write(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for FixError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoCompositor => None,
            Self::Write(error) => Some(error),
        }
    }
}

/// What the checkup looks at.
#[derive(Debug)]
pub struct Checkup<'a, V> {
    /// The compositor.
    pub found: &'a Found,
    /// The settings.
    pub state: &'a State,
    /// Where the generated files are.
    pub locations: &'a Locations,
    /// The user's sway config and its include line.
    pub status: &'a IncludeStatus,
    /// What validates the generated files: `sway --validate`.
    pub validator: &'a V,
}

impl<V: Validate> Checkup<'_, V> {
    /// Checks everything. Nothing is written; the only program run is the validator.
    pub async fn diagnose(&self) -> Diagnosis {
        self.diagnose_with(|_| {}).await
    }

    /// Checks everything like [`Checkup::diagnose`], and calls `progress` with each [`Step`]
    /// before it starts.
    pub async fn diagnose_with(&self, mut progress: impl FnMut(Step)) -> Diagnosis {
        let mut findings = Vec::new();
        progress(Step::Section(Section::Compositor));
        self.compositor(&mut findings);
        progress(Step::Section(Section::Config));
        self.config(&mut findings);
        let generated = self
            .found
            .compositor()
            .map(|compositor| generate::generate(self.state.registry(), self.state, compositor));
        progress(Step::Section(Section::Settings));
        self.settings(generated.as_deref(), &mut findings);
        progress(Step::Section(Section::Generated));
        self.files(generated.as_deref(), &mut findings, &mut progress)
            .await;
        progress(Step::Section(Section::Conflicts));
        self.conflicts(&mut findings);
        Diagnosis { findings }
    }

    /// Writes the generated files for the compositor in [`Checkup::found`], each validated before
    /// it replaces the old one: those that are missing or out of date, and with `overwrite` those
    /// that were changed outside Sway Things. Files Sway Things didn't make are never written, and
    /// neither are the settings files.
    ///
    /// # Errors
    ///
    /// [`FixError::NoCompositor`] if it isn't known what to generate for, and
    /// [`FixError::Write`] if a file can't be written or sway can't be run to validate it.
    pub async fn fix(&self, overwrite: bool, access: &WriteAccess) -> Result<Report, FixError> {
        let compositor = self.found.compositor().ok_or(FixError::NoCompositor)?;
        let files = generate::generate(self.state.registry(), self.state, compositor);
        let overwrite: &[ConfigFile] = if overwrite { &ConfigFile::ALL } else { &[] };
        generate::write(&files, self.locations, self.validator, overwrite, access)
            .await
            .map_err(FixError::Write)
    }

    fn compositor(&self, findings: &mut Vec<Finding>) {
        let supported = format!(
            "Sway Things needs sway {} or SwayFX {} or newer.",
            SwayVersion::OLDEST_SUPPORTED,
            SwayFxVersion::OLDEST_SUPPORTED
        );
        let installed = match self.found {
            Found::Session(compositor) => {
                findings.push(Finding::new(
                    Code::Session,
                    Severity::Ok,
                    format!("{compositor} runs in this session."),
                ));
                return;
            }
            Found::Unusable { error, installed } => {
                findings.push(
                    Finding::new(
                        Code::SessionUnusable,
                        Severity::Problem,
                        format!("The running sway can't be used: {error}."),
                    )
                    .with_hint(supported.clone()),
                );
                installed
            }
            Found::NoSession { reason, installed } => {
                let finding = match installed {
                    Installed::Version(compositor) => Finding::new(
                        Code::NoSession,
                        Severity::Ok,
                        format!(
                            "No sway session runs ({reason}). The installed compositor is \
                             {compositor}."
                        ),
                    ),
                    Installed::Unknown => Finding::new(
                        Code::VersionUnknown,
                        Severity::Warning,
                        format!(
                            "No sway session runs ({reason}), and `sway --version` names no \
                             version of sway or SwayFX."
                        ),
                    )
                    .with_hint(
                        "Without the version, the generated files can't be checked against the \
                         compositor. Check which program `sway` is on your PATH.",
                    ),
                    Installed::Missing(error) => Finding::new(
                        Code::SwayMissing,
                        Severity::Problem,
                        format!("No sway session runs ({reason}), and sway can't be run: {error}."),
                    )
                    .with_hint(
                        "Install sway or SwayFX, or check your PATH. Without it, the generated \
                         files can't be validated.",
                    ),
                };
                findings.push(finding);
                installed
            }
        };
        if let Installed::Version(compositor) = installed {
            if let Err(unmet) = compositor.check_supported() {
                findings.push(
                    Finding::new(
                        Code::InstalledUnsupported,
                        Severity::Problem,
                        format!(
                            "The installed {compositor} is not supported: Sway Things {unmet}."
                        ),
                    )
                    .with_hint(supported),
                );
            }
        }
    }

    fn config(&self, findings: &mut Vec<Finding>) {
        let status = self.status;
        match status.main() {
            Some(main) => {
                let loaded = if matches!(self.found, Found::Session(_)) {
                    " (loaded by the running sway)"
                } else {
                    ""
                };
                findings.push(
                    Finding::new(
                        Code::MainConfig,
                        Severity::Ok,
                        format!("Main config: {}{loaded}", main.display()),
                    )
                    .in_file(main),
                );
            }
            None => findings.push(Finding::new(
                Code::NoConfig,
                Severity::Problem,
                "sway finds no config file, so it starts with its defaults.",
            )),
        }
        let first = status.lines().first();
        let include = match status.inclusion() {
            Inclusion::Active if status.is_last() => {
                Finding::new(Code::IncludeActive, Severity::Ok, status.explanation())
            }
            Inclusion::Active => {
                const SHOWN: usize = 10;
                let after = status.after();
                let mut details: Vec<String> = after
                    .iter()
                    .take(SHOWN)
                    .map(|directive| {
                        format!(
                            "{}: {}",
                            place(&directive.file, directive.line),
                            directive.text
                        )
                    })
                    .collect();
                if after.len() > SHOWN {
                    details.push(format!("… and {} more", after.len() - SHOWN));
                }
                let hint = match first {
                    Some(include) => format!(
                        "Move `include {}` to the end of {}, unless those lines are meant to \
                         win. The conflicts below name the settings they change.",
                        include.pattern,
                        include.file.display()
                    ),
                    None => "Move the include line to the end of your config.".to_owned(),
                };
                Finding::new(
                    Code::IncludeNotLast,
                    Severity::Warning,
                    format!(
                        "The include line isn't last: {} of your config lines come after it and \
                         win where they set the same thing as Sway Things.",
                        after.len()
                    ),
                )
                .with_details(details)
                .with_hint(hint)
            }
            Inclusion::Partial { .. } => Finding::new(
                Code::IncludePartial,
                Severity::Problem,
                "Sway Things is read-only: your config includes only some of its files.",
            )
            .with_hint(status.message()),
            Inclusion::Unknown { .. } => Finding::new(
                Code::IncludeUnknown,
                Severity::Problem,
                "Sway Things is read-only: it can't follow part of your config to find the \
                 include line.",
            )
            .with_hint(status.message()),
            Inclusion::Missing => Finding::new(
                Code::IncludeMissing,
                Severity::Problem,
                "Sway Things is read-only: your config doesn't include its files, so nothing it \
                 generates is in effect.",
            )
            .with_hint(status.message()),
        };
        let include = match first {
            Some(line) => include.at(&line.file, line.line),
            None => include.in_file(status.instructions().config()),
        };
        findings.push(include);
        for diagnostic in &status.config().diagnostics {
            findings.push(diagnostic_finding(diagnostic));
        }
    }

    fn settings(&self, generated: Option<&[GeneratedFile]>, findings: &mut Vec<Finding>) {
        let problems = self.state.problems();
        if problems.is_empty() {
            findings.push(Finding::new(
                Code::SettingsLoaded,
                Severity::Ok,
                match self.state.entries().len() {
                    1 => "The settings files load without problems (1 stored value).".to_owned(),
                    count => {
                        format!("The settings files load without problems ({count} stored values).")
                    }
                },
            ));
        }
        for problem in problems {
            let finding = match problem.kind {
                ProblemKind::Unreadable { .. }
                | ProblemKind::Syntax { .. }
                | ProblemKind::UnsupportedVersion { .. } => Finding::new(
                    Code::SettingsUnusable,
                    Severity::Problem,
                    problem.to_string(),
                )
                .with_hint(
                    "Nothing in this file is in effect, and Sway Things doesn't write it until it \
                     is fixed.",
                ),
                _ => Finding::new(Code::SettingsEntry, Severity::Warning, problem.to_string())
                    .with_hint("The entry is ignored and stays in the file. Fix or remove it."),
            };
            findings.push(finding.in_file(&problem.path));
        }
        let skipped = generated
            .into_iter()
            .flatten()
            .flat_map(GeneratedFile::lines)
            .filter_map(|line| Some((&line.source, line.skipped.as_deref()?)));
        for (source, reason) in skipped {
            let Some(&key) = source.keys.first() else {
                continue;
            };
            findings.push(
                Finding::new(
                    Code::SettingSkipped,
                    Severity::Warning,
                    format!("{} has no effect: {reason}.", setting_name(source)),
                )
                .with_hint(
                    "The value stays stored and is a comment in the generated file. It takes \
                     effect on a compositor that supports it; reset the setting to remove it.",
                )
                .for_setting(key, source.device.as_deref()),
            );
        }
    }

    async fn files(
        &self,
        generated: Option<&[GeneratedFile]>,
        findings: &mut Vec<Finding>,
        progress: &mut impl FnMut(Step),
    ) {
        let manifest = Manifest::load(self.locations.manifest());
        let needed: BTreeSet<ConfigFile> = self
            .state
            .entries()
            .iter()
            .filter_map(|entry| entry.setting.sway())
            .map(SwayMapping::file)
            .collect();
        let missing_sway = self.found.sway_missing();
        let mut unvalidated = None;
        let mut checked = 0;
        for file in ConfigFile::ALL {
            let path = self.locations.path(file);
            let content =
                generated.and_then(|files| files.iter().find(|generated| generated.file() == file));
            // Without a compositor there is no content to compare; the header stands in, so only
            // who made the file is told.
            let text = content.map_or_else(|| HEADER.to_owned(), GeneratedFile::text);
            let ownership = match manifest.ownership(&path, &text) {
                Ok(ownership) => ownership,
                Err(error) => {
                    findings.push(
                        Finding::new(
                            Code::FileUnreadable,
                            Severity::Problem,
                            format!("{} can't be read: {error}.", file.file_name()),
                        )
                        .in_file(&path),
                    );
                    continue;
                }
            };
            let finding = self.ownership_finding(file, ownership, content.is_some(), &needed);
            let fix = finding.fix;
            findings.push(finding.in_file(&path));
            if matches!(ownership, Ownership::Absent | Ownership::Foreign) {
                continue;
            }
            checked += 1;
            if missing_sway.is_some() {
                continue;
            }
            progress(Step::Validate(file));
            match self.validator.validate(&path).await {
                Ok(errors) => {
                    // Only the file as generated now tells which settings a line comes from.
                    let current = content.filter(|_| ownership == Ownership::Current);
                    findings.push(validated(file, errors, current, fix).in_file(&path));
                }
                Err(error) => {
                    unvalidated.get_or_insert_with(|| error.to_string());
                }
            }
        }
        let reason = missing_sway.map(str::to_owned).or(unvalidated);
        if let (Some(reason), true) = (reason, checked > 0) {
            findings.push(
                Finding::new(
                    Code::ValidationSkipped,
                    Severity::Warning,
                    format!(
                        "The generated files weren't checked with sway --validate: sway can't be \
                         run ({reason})."
                    ),
                )
                .with_hint("Install sway or SwayFX, or check your PATH."),
            );
        }
    }

    /// What `ownership` means for the generated `file`, which holds content if `known` (the
    /// compositor is known) and has to exist if it is `needed`.
    fn ownership_finding(
        &self,
        file: ConfigFile,
        ownership: Ownership,
        known: bool,
        needed: &BTreeSet<ConfigFile>,
    ) -> Finding {
        let name = file.file_name();
        let compositor = self.found.compositor();
        let regenerate = compositor.map(|_| Fix::Regenerate);
        match ownership {
            Ownership::Absent if !needed.contains(&file) => Finding::new(
                Code::FileNotNeeded,
                Severity::Ok,
                format!("{name} isn't needed: no stored setting goes into it."),
            ),
            Ownership::Absent => Finding::new(
                Code::FileMissing,
                Severity::Problem,
                format!("{name} is missing, so the settings that go into it have no effect."),
            )
            .fixed_by(regenerate),
            Ownership::Current | Ownership::Ours if !known => Finding::new(
                Code::FileUnchecked,
                Severity::Warning,
                format!(
                    "{name} was written by Sway Things; without knowing the compositor, it can't \
                     be told whether it is up to date."
                ),
            ),
            Ownership::Current => Finding::new(
                Code::FileCurrent,
                Severity::Ok,
                format!("{name} is up to date."),
            ),
            Ownership::Ours => Finding::new(
                Code::FileOutOfDate,
                Severity::Problem,
                format!(
                    "{name} is out of date: it doesn't hold what Sway Things generates from the \
                     stored settings{}.",
                    compositor
                        .map(|compositor| format!(" for {compositor}"))
                        .unwrap_or_default()
                ),
            )
            .with_hint(
                "This happens after switching between sway and SwayFX, or after editing the \
                 settings files while Sway Things didn't run.",
            )
            .fixed_by(regenerate),
            Ownership::Edited => Finding::new(
                Code::FileEdited,
                Severity::Problem,
                format!("{name} was changed outside Sway Things."),
            )
            .with_hint(
                "Move what you changed into your own config: fixing the file replaces it with \
                 what Sway Things generates.",
            )
            .fixed_by(compositor.map(|_| Fix::Overwrite)),
            Ownership::Foreign => Finding::new(
                Code::FileForeign,
                Severity::Problem,
                format!("{name} wasn't made by Sway Things, so it is never written."),
            )
            .with_hint(format!(
                "Move it out of {}: the include line reads every *.conf file there, and Sway \
                 Things needs the name.",
                self.locations.directory().display()
            )),
        }
    }

    fn conflicts(&self, findings: &mut Vec<Finding>) {
        if self.status.lines().is_empty() {
            // Nothing generated is in effect, so nothing of the user's competes with it.
            return;
        }
        let before = findings.len();
        for entry in self.state.entries() {
            let Some(report) =
                conflict::Report::check(self.status, entry.setting, entry.device, None)
            else {
                continue;
            };
            let name = match entry.device {
                Some(device) => format!("{} (device {device})", entry.setting.key()),
                None => entry.setting.key().to_owned(),
            };
            if let Some(line) = report.overriding() {
                findings.push(
                    Finding::new(
                        Code::ConfigWins,
                        Severity::Warning,
                        format!(
                            "{name}: your config wins: {} comes after the generated file ({}).",
                            place(line.file(), line.line()),
                            line.text()
                        ),
                    )
                    .with_hint(
                        "Remove or change that line, or reset the setting to let your config \
                         decide it.",
                    )
                    .at(line.file(), line.line())
                    .for_setting(entry.setting.key(), entry.device),
                );
            }
            for line in report.overridden() {
                findings.push(
                    Finding::new(
                        Code::ConfigLineDead,
                        Severity::Ok,
                        format!(
                            "{name}: {} ({}) has no effect while Sway Things stores the setting.",
                            place(line.file(), line.line()),
                            line.text()
                        ),
                    )
                    .with_hint(
                        "Remove the line, or reset the setting to let the line take effect again.",
                    )
                    .at(line.file(), line.line())
                    .for_setting(entry.setting.key(), entry.device),
                );
            }
        }
        if findings.len() == before {
            findings.push(Finding::new(
                Code::NoConflicts,
                Severity::Ok,
                "No line of your config sets a stored setting.",
            ));
        }
    }
}

/// What sway said about the generated `file`: the `errors` it found, traced to the settings
/// behind each line if the file holds the `current` content. `fix` is what fixing the file does.
fn validated(
    file: ConfigFile,
    errors: Vec<ConfigError>,
    current: Option<&GeneratedFile>,
    fix: Option<Fix>,
) -> Finding {
    let name = file.file_name();
    if errors.is_empty() {
        return Finding::new(
            Code::FileValid,
            Severity::Ok,
            format!("sway --validate accepts {name}."),
        );
    }
    let details = errors
        .into_iter()
        .map(|error| {
            invalid_line(&InvalidLine {
                source: current
                    .and_then(|content| content.source_of_line(error.line))
                    .cloned(),
                line: error.line,
                message: error.message,
            })
        })
        .collect();
    let hint = if current.is_some() {
        "The file holds what Sway Things generates for the stored settings: reset the settings \
         named above, and please report this as a bug."
    } else {
        "Fixing the file replaces it with what Sway Things generates, if sway accepts that."
    };
    Finding::new(
        Code::FileInvalid,
        Severity::Problem,
        format!("sway --validate rejects {name}; sway reports errors for it at every start."),
    )
    .with_details(details)
    .with_hint(hint)
    .fixed_by(fix)
}

/// `file:line`, or the file alone for line 0.
fn place(file: &Path, line: usize) -> String {
    if line == 0 {
        file.display().to_string()
    } else {
        format!("{}:{line}", file.display())
    }
}

/// The settings behind a generated line: `windows.gaps.inner`, with the device if it is for one.
fn setting_name(source: &Source) -> String {
    let keys = source.keys.join(", ");
    match &source.device {
        Some(device) => format!("{keys} (device {device})"),
        None => keys,
    }
}

/// A line sway rejected: `line 3: message (from windows.gaps.inner)`.
fn invalid_line(line: &InvalidLine) -> String {
    let place = match line.line {
        0 => String::new(),
        number => format!("line {number}: "),
    };
    let source = line
        .source
        .as_ref()
        .map(|source| format!(" (from {})", setting_name(source)))
        .unwrap_or_default();
    format!("{place}{}{source}", line.message)
}

/// What the parser couldn't follow in the user's config.
fn diagnostic_finding(diagnostic: &Diagnostic) -> Finding {
    let mut severity = Severity::Warning;
    let mut hint = None;
    let what = match &diagnostic.kind {
        DiagnosticKind::Unreadable { message } => {
            severity = Severity::Problem;
            format!("can't be read: {message}")
        }
        DiagnosticKind::UnresolvedInclude { pattern, reason } => {
            hint = Some(
                "Sway Things never runs commands to read your config, so it doesn't know what \
                 that include sets.",
            );
            format!("include {pattern} can't be followed: {reason}")
        }
        DiagnosticKind::IncludeNotFound { path } => {
            format!(
                "include: {} doesn't exist, so sway skips it",
                path.display()
            )
        }
        DiagnosticKind::AlreadyIncluded { path } => format!(
            "include: {} was read already, so sway doesn't read it again",
            path.display()
        ),
        DiagnosticKind::InvalidInclude => {
            "include needs exactly one path, so sway rejects it".into()
        }
        DiagnosticKind::TooDeep => "includes are nested too deeply to follow".to_owned(),
        DiagnosticKind::UnmatchedBrace => "a } closes no block".to_owned(),
        DiagnosticKind::UnclosedBlock => "a block is still open at the end of the file".to_owned(),
        DiagnosticKind::TooBig => "the config is too big; the rest wasn't read".to_owned(),
    };
    let finding = Finding::new(
        Code::ConfigDiagnostic,
        severity,
        format!("{}: {what}.", place(&diagnostic.file, diagnostic.line)),
    )
    .at(&diagnostic.file, diagnostic.line);
    match hint {
        Some(hint) => finding.with_hint(hint),
        None => finding,
    }
}
