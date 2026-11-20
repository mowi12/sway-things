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
use crate::i18n::{Msg, Text};
use crate::include::{IncludeStatus, Inclusion, MainConfig, WriteAccess};
use crate::ipc::{Client, Compositor, IpcError};
use crate::registry::{ConfigFile, Scope, SwayMapping};
use crate::state::{ProblemKind, State};
use crate::sway_config::{Diagnostic, DiagnosticKind};
use crate::version::{SwayFxVersion, SwayVersion};
use crate::{msg, msg_plural};

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
    pub fn title(self) -> Msg {
        match self {
            Self::Compositor => msg!("Compositor"),
            Self::Config => msg!("Sway config"),
            Self::Settings => msg!("Settings files"),
            Self::Generated => msg!("Generated files"),
            Self::Conflicts => msg!("Conflicts"),
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
    /// kanshi is installed, but its config doesn't include the profiles of Sway Things.
    KanshiIncludeMissing,
    /// A file in the place of the kanshi profiles of Sway Things wasn't made by Sway Things.
    KanshiFileForeign,
    /// A profile of the user's kanshi config covers a combination Sway Things remembers, and
    /// comes first.
    KanshiProfileOverlap,
    /// sway and kanshi can't run `sway-things` for the hooks, or run another build of it.
    HookProgram,
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
            Self::KanshiIncludeMissing => "kanshi_include_missing",
            Self::KanshiFileForeign => "kanshi_file_foreign",
            Self::KanshiProfileOverlap => "kanshi_profile_overlap",
            Self::HookProgram => "hook_program",
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
            | Self::ConfigDiagnostic
            | Self::KanshiIncludeMissing
            | Self::KanshiFileForeign
            | Self::KanshiProfileOverlap
            | Self::HookProgram => Section::Config,
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
    pub message: Text,
    /// More lines that belong to it: the lines sway rejects, the user's lines after the
    /// include.
    pub details: Vec<Text>,
    /// How to fix it, if there is something to do that [`Finding::fix`] doesn't cover. Can be
    /// several lines.
    pub hint: Option<Text>,
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
    fn new(code: Code, severity: Severity, message: impl Into<Text>) -> Self {
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

    fn with_hint(mut self, hint: impl Into<Text>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    fn with_details(mut self, details: Vec<Text>) -> Self {
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
    Missing(Text),
}

impl Installed {
    /// Runs `sway --version` through `validator`.
    pub async fn of(validator: &SwayValidator) -> Self {
        match validator.compositor().await {
            Ok(Some(compositor)) => Self::Version(compositor),
            Ok(None) => Self::Unknown,
            Err(error) => Self::Missing(Text::data(error.to_string())),
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
        reason: Text,
        /// The installed compositor.
        installed: Installed,
    },
    /// A session runs, but Sway Things can't use it: the compositor is too old, or it doesn't
    /// answer like sway does.
    Unusable {
        /// What went wrong.
        error: Text,
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
                reason: msg!("SWAYSOCK isn't set").into(),
                installed,
            },
            IpcError::NotRunning { socket, .. } => Self::NoSession {
                reason: Text::new(msg!("nothing answers on {socket}"))
                    .arg("socket", socket.display()),
                installed,
            },
            error => Self::Unusable {
                error: error.text(),
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
    fn sway_missing(&self) -> Option<&Text> {
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

impl FixError {
    /// Why nothing was written, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::NoCompositor => msg!(
                "it isn't known which compositor to generate for: no sway session runs, and \
                 `sway --version` names none"
            )
            .into(),
            Self::Write(error) => error.text(),
        }
    }
}

impl fmt::Display for FixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
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
    /// The outputs of the session, if one runs: a line of the user's that names an output by
    /// its connector competes with a value stored by its make, model and serial.
    pub outputs: Option<&'a crate::outputs::Outputs>,
    /// Who sets up the displays: with kanshi, no `output` lines are generated.
    pub displays: generate::Displays,
    /// kanshi's files, if kanshi is installed: what is checked of them.
    pub kanshi: Option<crate::outputs::kanshi::KanshiFiles>,
    /// The `$PATH` sway and kanshi run `sway-things` from for the hooks; `None` for a checkup
    /// that doesn't look at the machine.
    pub path: Option<std::ffi::OsString>,
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
        self.kanshi(&mut findings);
        let generated = self.found.compositor().map(|compositor| {
            generate::generate_for(self.state.registry(), self.state, compositor, self.displays)
        });
        // Only where something runs a hook: a line of the generated config, or kanshi's profiles.
        let hooks = self.displays.is_kanshi()
            || generated.as_ref().is_some_and(|files| {
                files
                    .iter()
                    .any(|file| file.text().contains("sway-things hook "))
            });
        if let Some(path) = self.path.as_ref().filter(|_| hooks) {
            Self::hook_program(
                Some(path.as_os_str()),
                std::env::current_exe().ok().as_deref(),
                &mut findings,
            );
        }
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
        let files =
            generate::generate_for(self.state.registry(), self.state, compositor, self.displays);
        let overwrite: &[ConfigFile] = if overwrite { &ConfigFile::ALL } else { &[] };
        generate::write(&files, self.locations, self.validator, overwrite, access)
            .await
            .map_err(FixError::Write)
    }

    fn compositor(&self, findings: &mut Vec<Finding>) {
        let supported = Text::new(msg!(
            "Sway Things needs sway {sway} or SwayFX {swayfx} or newer."
        ))
        .arg("sway", SwayVersion::OLDEST_SUPPORTED)
        .arg("swayfx", SwayFxVersion::OLDEST_SUPPORTED);
        let installed = match self.found {
            Found::Session(compositor) => {
                findings.push(Finding::new(
                    Code::Session,
                    Severity::Ok,
                    Text::new(msg!("{compositor} runs in this session."))
                        .text("compositor", compositor.text()),
                ));
                return;
            }
            Found::Unusable { error, installed } => {
                findings.push(
                    Finding::new(
                        Code::SessionUnusable,
                        Severity::Problem,
                        Text::new(msg!("The running sway can't be used: {error}."))
                            .text("error", error.clone()),
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
                        Text::new(msg!(
                            "No sway session runs ({reason}). The installed compositor is \
                             {compositor}."
                        ))
                        .text("reason", reason.clone())
                        .text("compositor", compositor.text()),
                    ),
                    Installed::Unknown => Finding::new(
                        Code::VersionUnknown,
                        Severity::Warning,
                        Text::new(msg!(
                            "No sway session runs ({reason}), and `sway --version` names no \
                             version of sway or SwayFX."
                        ))
                        .text("reason", reason.clone()),
                    )
                    .with_hint(msg!(
                        "Without the version, the generated files can't be checked against the \
                         compositor. Check which program `sway` is on your PATH."
                    )),
                    Installed::Missing(error) => Finding::new(
                        Code::SwayMissing,
                        Severity::Problem,
                        Text::new(msg!(
                            "No sway session runs ({reason}), and sway can't be run: {error}."
                        ))
                        .text("reason", reason.clone())
                        .text("error", error.clone()),
                    )
                    .with_hint(msg!(
                        "Install sway or SwayFX, or check your PATH. Without it, the generated \
                         files can't be validated."
                    )),
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
                        Text::new(msg!(
                            "The installed {compositor} is not supported: Sway Things \
                             {requirement}."
                        ))
                        .text("compositor", compositor.text())
                        .text("requirement", unmet.text()),
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
                let message = if matches!(self.found, Found::Session(_)) {
                    msg!("Main config: {file} (loaded by the running sway)")
                } else {
                    msg!("Main config: {file}")
                };
                findings.push(
                    Finding::new(
                        Code::MainConfig,
                        Severity::Ok,
                        Text::new(message).arg("file", main.display()),
                    )
                    .in_file(main),
                );
            }
            None => findings.push(Finding::new(
                Code::NoConfig,
                Severity::Problem,
                msg!("sway finds no config file, so it starts with its defaults."),
            )),
        }
        let first = status.lines().first();
        let include = include_finding(status);
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
            findings.push(Finding::new(Code::SettingsLoaded, Severity::Ok, {
                let count = self.state.entries().len();
                Text::count(
                    msg_plural!(
                        "The settings files load without problems ({count} stored value).",
                        "The settings files load without problems ({count} stored values)."
                    ),
                    count,
                )
                .arg("count", count)
            }));
        }
        for problem in problems {
            let finding = match problem.kind {
                ProblemKind::Unreadable { .. }
                | ProblemKind::Syntax { .. }
                | ProblemKind::UnsupportedVersion { .. } => Finding::new(
                    Code::SettingsUnusable,
                    Severity::Problem,
                    problem.text(),
                )
                .with_hint(msg!(
                    "Nothing in this file is in effect, and Sway Things doesn't write it until it \
                     is fixed."
                )),
                _ => Finding::new(Code::SettingsEntry, Severity::Warning, problem.text())
                    .with_hint(msg!(
                        "The entry is ignored and stays in the file. Fix or remove it."
                    )),
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
                    Text::new(msg!("{setting} has no effect: {reason}."))
                        .text("setting", setting_name(source))
                        .arg("reason", reason),
                )
                .with_hint(msg!(
                    "The value stays stored and is a comment in the generated file. It takes \
                     effect on a compositor that supports it; reset the setting to remove it."
                ))
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
                            Text::new(msg!("{file} can't be read: {error}."))
                                .arg("file", file.file_name())
                                .arg("error", error),
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
                    unvalidated.get_or_insert_with(|| Text::data(error.to_string()));
                }
            }
        }
        let reason = missing_sway.cloned().or(unvalidated);
        if let (Some(reason), true) = (reason, checked > 0) {
            findings.push(
                Finding::new(
                    Code::ValidationSkipped,
                    Severity::Warning,
                    Text::new(msg!(
                        "The generated files weren't checked with sway --validate: sway can't be \
                         run ({reason})."
                    ))
                    .text("reason", reason),
                )
                .with_hint(msg!("Install sway or SwayFX, or check your PATH.")),
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
                Text::new(msg!("{name} isn't needed: no stored setting goes into it."))
                    .arg("name", name),
            ),
            Ownership::Absent => Finding::new(
                Code::FileMissing,
                Severity::Problem,
                Text::new(msg!(
                    "{name} is missing, so the settings that go into it have no effect."
                ))
                .arg("name", name),
            )
            .fixed_by(regenerate),
            Ownership::Current | Ownership::Ours if !known => Finding::new(
                Code::FileUnchecked,
                Severity::Warning,
                Text::new(msg!(
                    "{name} was written by Sway Things; without knowing the compositor, it can't \
                     be told whether it is up to date."
                ))
                .arg("name", name),
            ),
            Ownership::Current => Finding::new(
                Code::FileCurrent,
                Severity::Ok,
                Text::new(msg!("{name} is up to date.")).arg("name", name),
            ),
            Ownership::Ours => Finding::new(
                Code::FileOutOfDate,
                Severity::Problem,
                match compositor {
                    Some(compositor) => Text::new(msg!(
                        "{file} is out of date: it doesn't hold what Sway Things generates from \
                         the stored settings for {compositor}."
                    ))
                    .arg("file", name)
                    .text("compositor", compositor.text()),
                    None => Text::new(msg!(
                        "{file} is out of date: it doesn't hold what Sway Things generates from \
                         the stored settings."
                    ))
                    .arg("file", name),
                },
            )
            .with_hint(msg!(
                "This happens after switching between sway and SwayFX, or after editing the \
                 settings files while Sway Things didn't run."
            ))
            .fixed_by(regenerate),
            Ownership::Edited => Finding::new(
                Code::FileEdited,
                Severity::Problem,
                Text::new(msg!("{name} was changed outside Sway Things.")).arg("name", name),
            )
            .with_hint(msg!(
                "Move what you changed into your own config: fixing the file replaces it with \
                 what Sway Things generates."
            ))
            .fixed_by(compositor.map(|_| Fix::Overwrite)),
            Ownership::Foreign => Finding::new(
                Code::FileForeign,
                Severity::Problem,
                Text::new(msg!(
                    "{name} wasn't made by Sway Things, so it is never written."
                ))
                .arg("name", name),
            )
            .with_hint(
                Text::new(msg!(
                    "Move it out of {directory}: the include line reads every *.conf file there, \
                     and Sway Things needs the name."
                ))
                .arg("directory", self.locations.directory().display()),
            ),
        }
    }

    /// Whether sway and kanshi can run `sway-things`: the generated config and kanshi's profiles
    /// run it by name for the hooks (the display guard, the workspaces, the pictures), so it
    /// must be in a directory of `path` (`$PATH`). One that isn't the program `this` (the one
    /// running now) is another build, which may not know the hooks of this one.
    fn hook_program(
        path: Option<&std::ffi::OsStr>,
        this: Option<&std::path::Path>,
        findings: &mut Vec<Finding>,
    ) {
        let Some(found) = crate::delegate::find_program("sway-things", path) else {
            findings.push(
                Finding::new(
                    Code::HookProgram,
                    Severity::Problem,
                    Text::new(msg!(
                        "sway and kanshi can't run sway-things: it isn't in a directory of \
                         $PATH. The hooks in the generated config do nothing, so workspaces \
                         aren't placed, pictures don't start and no display is switched on \
                         when none is."
                    )),
                )
                .with_hint(msg!(
                    "Install Sway Things, or put the directory of sway-things into $PATH \
                     before sway starts."
                )),
            );
            return;
        };
        let same = |one: &std::path::Path, other: &std::path::Path| match (
            one.canonicalize(),
            other.canonicalize(),
        ) {
            (Ok(one), Ok(other)) => one == other,
            _ => one == other,
        };
        // Only told apart from a `sway-things` itself: the app is another program.
        let Some(this) =
            this.filter(|this| this.file_name().is_some_and(|name| name == "sway-things"))
        else {
            return;
        };
        if !same(&found, this) {
            findings.push(
                Finding::new(
                    Code::HookProgram,
                    Severity::Warning,
                    Text::new(msg!(
                        "sway and kanshi run {found} for the hooks, not this build ({this}). An \
                         older one doesn't know every hook, so workspaces may not be placed and \
                         pictures may not start after a reload."
                    ))
                    .arg("found", found.display())
                    .arg("this", this.display()),
                )
                .with_hint(msg!(
                    "Install this build, or put its directory first in $PATH before sway starts."
                )),
            );
        }
    }

    /// What keeps kanshi from bringing back the remembered combinations: its config without
    /// our include, a file of someone else's in the place of ours, a profile of the user's that
    /// covers a remembered combination before ours. Nothing without kanshi.
    fn kanshi(&self, findings: &mut Vec<Finding>) {
        use crate::outputs::kanshi::Inclusion;
        let Some(files) = &self.kanshi else {
            return;
        };
        if files.inclusion() == Inclusion::Missing {
            findings.push(
                Finding::new(
                    Code::KanshiIncludeMissing,
                    Severity::Warning,
                    Text::new(msg!(
                        "{file} doesn't include the profiles of Sway Things, so combinations of \
                         displays aren't remembered."
                    ))
                    .arg("file", files.config.display()),
                )
                .with_hint(files.how_to_include()),
            );
        }
        {
            if files.ours_is_foreign() {
                findings.push(
                    Finding::new(
                        Code::KanshiFileForeign,
                        Severity::Problem,
                        Text::new(msg!(
                            "{file} wasn't made by Sway Things, so nothing is written there and \
                             kanshi never sees what is set in the app."
                        ))
                        .arg("file", files.ours.display()),
                    )
                    .with_hint(msg!(
                        "Move it away, so Sway Things can write its profiles there."
                    )),
                );
            }
        }
        let displays_path = self.state.paths().displays();
        let profiles = match crate::outputs::profiles::Profiles::load(displays_path) {
            Ok(profiles) => profiles,
            Err(error) => {
                findings.push(
                    Finding::new(
                        Code::KanshiFileForeign,
                        Severity::Warning,
                        Text::new(msg!("{file} can't be used: {error}."))
                            .arg("file", displays_path.display())
                            .arg("error", error),
                    )
                    .with_hint(msg!(
                        "Fix or move it; meanwhile the displays are set up from the sway config."
                    )),
                );
                return;
            }
        };
        self.kanshi_file(files, &profiles, findings);
        self.kanshi_order(files, &profiles, findings);
    }

    /// Whether the file of ours for kanshi says what the remembered displays do: one that is
    /// missing makes kanshi reject its whole config, and one that stays behind means nothing set
    /// in the app reaches kanshi.
    fn kanshi_file(
        &self,
        files: &crate::outputs::kanshi::KanshiFiles,
        profiles: &crate::outputs::profiles::Profiles,
        findings: &mut Vec<Finding>,
    ) {
        if !self.displays.is_kanshi() || files.ours_is_foreign() {
            return;
        }
        let places = crate::outputs::workspaces::Placing::of(self.state);
        let expected = crate::outputs::profiles::kanshi_config(profiles, places);
        let finding = match std::fs::read_to_string(&files.ours) {
            Ok(text) if text == expected => return,
            Ok(_) => Finding::new(
                Code::KanshiFileForeign,
                Severity::Problem,
                Text::new(msg!(
                    "{file} is out of date: it doesn't hold what Sway Things remembers for the \
                     displays, so kanshi sets them up as before."
                ))
                .arg("file", files.ours.display()),
            ),
            Err(_) => Finding::new(
                Code::KanshiFileForeign,
                Severity::Problem,
                Text::new(msg!(
                    "{file} is missing, and kanshi rejects its whole config over an include it \
                     can't find: no display is set up by kanshi."
                ))
                .arg("file", files.ours.display()),
            ),
        };
        findings.push(finding.with_hint(msg!(
            "Run: sway-things hook displays (it writes the file again and has kanshi read it)"
        )));
    }

    /// The user's kanshi profiles that come before our include, which kanshi tries first: one
    /// that covers a remembered combination, and the include not being at the top at all.
    fn kanshi_order(
        &self,
        files: &crate::outputs::kanshi::KanshiFiles,
        profiles: &crate::outputs::profiles::Profiles,
        findings: &mut Vec<Finding>,
    ) {
        let Ok(config) = std::fs::read_to_string(&files.config) else {
            return;
        };
        // Only a profile before our include can win; a display named by its connector is
        // matched with the displays connected now.
        let named = |criteria: &str| {
            self.outputs
                .and_then(|outputs| outputs.find(criteria))
                .map_or_else(
                    || criteria.to_owned(),
                    |output| output.identifier().to_owned(),
                )
        };
        // Those that cover nothing remembered yet still win once their displays are arranged.
        let mut above: Vec<String> = Vec::new();
        for (name, outputs) in files.profiles_before_ours(&config) {
            use crate::outputs::kanshi::{ANY, ONE};
            let wildcard = outputs
                .iter()
                .any(|criteria| criteria == ONE || criteria == ANY);
            let mut sorted: Vec<String> = outputs.iter().map(|criteria| named(criteria)).collect();
            sorted.sort_unstable();
            let covers = (wildcard && profiles.iter().next().is_some())
                || profiles.iter().any(|profile| {
                    profile
                        .combination()
                        .iter()
                        .copied()
                        .eq(sorted.iter().map(String::as_str))
                });
            if covers {
                findings.push(
                    Finding::new(
                        Code::KanshiProfileOverlap,
                        Severity::Warning,
                        Text::new(msg!(
                            "The profile {name} in {file} comes before the include and covers \
                             displays Sway Things remembers; kanshi applies the first profile that \
                             matches."
                        ))
                        .arg("name", name)
                        .arg("file", files.config.display()),
                    )
                    .with_hint(files.how_to_include()),
                );
            } else {
                above.push(name);
            }
        }
        if !above.is_empty() {
            findings.push(
                Finding::new(
                    Code::KanshiProfileOverlap,
                    Severity::Warning,
                    Text::new(msg!(
                        "The include of Sway Things isn't at the top of {file}: {name} comes \
                         before it and wins for its displays, whatever is arranged for them on \
                         the Displays page."
                    ))
                    .arg("name", above.join(", "))
                    .arg("file", files.config.display()),
                )
                .with_hint(files.how_to_include()),
            );
        }
    }

    fn conflicts(&self, findings: &mut Vec<Finding>) {
        if self.status.lines().is_empty() {
            // Nothing generated is in effect, so nothing of the user's competes with it.
            return;
        }
        let before = findings.len();
        for entry in self.state.entries() {
            // kanshi sets up the displays: no line of ours competes with the user's.
            if self.displays.is_kanshi() && entry.setting.scope() == Scope::PerOutput {
                continue;
            }
            let Some(report) = conflict::Report::check(
                self.status,
                entry.setting,
                entry.device,
                conflict::Known {
                    devices: None,
                    outputs: self.outputs,
                },
            ) else {
                continue;
            };
            let name = match entry.device {
                Some(device) => Text::new(msg!("{key} (device {device})"))
                    .arg("key", entry.setting.key())
                    .arg("device", device),
                None => Text::data(entry.setting.key()),
            };
            if let Some(line) = report.overriding() {
                findings.push(
                    Finding::new(
                        Code::ConfigWins,
                        Severity::Warning,
                        Text::new(msg!(
                            "{setting}: your config wins: {place} comes after the generated file \
                             ({line})."
                        ))
                        .text("setting", name.clone())
                        .arg("place", place(line.file(), line.line()))
                        .arg("line", line.text()),
                    )
                    .with_hint(msg!(
                        "Remove or change that line, or reset the setting to let your config \
                         decide it."
                    ))
                    .at(line.file(), line.line())
                    .for_setting(entry.setting.key(), entry.device),
                );
            }
            for line in report.overridden() {
                findings.push(
                    Finding::new(
                        Code::ConfigLineDead,
                        Severity::Ok,
                        Text::new(msg!(
                            "{setting}: {place} ({line}) has no effect while Sway Things stores \
                             the setting."
                        ))
                        .text("setting", name.clone())
                        .arg("place", place(line.file(), line.line()))
                        .arg("line", line.text()),
                    )
                    .with_hint(msg!(
                        "Remove the line, or reset the setting to let the line take effect again."
                    ))
                    .at(line.file(), line.line())
                    .for_setting(entry.setting.key(), entry.device),
                );
            }
        }
        if findings.len() == before {
            findings.push(Finding::new(
                Code::NoConflicts,
                Severity::Ok,
                msg!("No line of your config sets a stored setting."),
            ));
        }
    }
}

/// The finding about the include line of `status`.
fn include_finding(status: &IncludeStatus) -> Finding {
    let first = status.lines().first();
    match status.inclusion() {
        Inclusion::Active if status.is_last() => {
            Finding::new(Code::IncludeActive, Severity::Ok, status.explanation())
        }
        Inclusion::Active => {
            const SHOWN: usize = 10;
            let after = status.after();
            let mut details: Vec<Text> = after
                .iter()
                .take(SHOWN)
                .map(|directive| {
                    Text::data(format!(
                        "{}: {}",
                        place(&directive.file, directive.line),
                        directive.text
                    ))
                })
                .collect();
            if after.len() > SHOWN {
                let more = after.len() - SHOWN;
                details.push(
                    Text::count(
                        msg_plural!("… and {count} more", "… and {count} more"),
                        more,
                    )
                    .arg("count", more),
                );
            }
            let hint = match first {
                Some(include) => Text::new(msg!(
                    "Move `include {pattern}` to the end of {file}, unless those lines are \
                     meant to win. The conflicts below name the settings they change."
                ))
                .arg("pattern", &include.pattern)
                .arg("file", include.file.display()),
                None => msg!("Move the include line to the end of your config.").into(),
            };
            Finding::new(
                Code::IncludeNotLast,
                Severity::Warning,
                Text::count(
                    msg_plural!(
                        "The include line isn't last: {count} of your config lines comes \
                         after it and wins where it sets the same thing as Sway Things.",
                        "The include line isn't last: {count} of your config lines come after \
                         it and win where they set the same thing as Sway Things."
                    ),
                    after.len(),
                )
                .arg("count", after.len()),
            )
            .with_details(details)
            .with_hint(hint)
        }
        Inclusion::Partial { .. } => Finding::new(
            Code::IncludePartial,
            Severity::Problem,
            msg!("Sway Things is read-only: your config includes only some of its files."),
        )
        .with_hint(status.message()),
        Inclusion::Unknown { .. } => Finding::new(
            Code::IncludeUnknown,
            Severity::Problem,
            msg!(
                "Sway Things is read-only: it can't follow part of your config to find the \
             include line."
            ),
        )
        .with_hint(status.message()),
        Inclusion::Missing => Finding::new(
            Code::IncludeMissing,
            Severity::Problem,
            msg!(
                "Sway Things is read-only: your config doesn't include its files, so nothing it \
             generates is in effect."
            ),
        )
        .with_hint(status.message()),
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
            Text::new(msg!("sway --validate accepts {name}.")).arg("name", name),
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
        msg!(
            "The file holds what Sway Things generates for the stored settings: reset the \
             settings named above, and please report this as a bug."
        )
    } else {
        msg!("Fixing the file replaces it with what Sway Things generates, if sway accepts that.")
    };
    Finding::new(
        Code::FileInvalid,
        Severity::Problem,
        Text::new(msg!(
            "sway --validate rejects {name}; sway reports errors for it at every start."
        ))
        .arg("name", name),
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
fn setting_name(source: &Source) -> Text {
    let keys = source.keys.join(", ");
    match &source.device {
        Some(device) => Text::new(msg!("{key} (device {device})"))
            .arg("key", keys)
            .arg("device", device),
        None => Text::data(keys),
    }
}

/// A line sway rejected: `line 3: message (from windows.gaps.inner)`.
fn invalid_line(line: &InvalidLine) -> Text {
    // sway's message stays as it is.
    let text = match (line.line, &line.source) {
        (0, None) => return Text::data(&line.message),
        (0, Some(_)) => Text::new(msg!("{message} (from {settings})")),
        (_, None) => Text::new(msg!("line {number}: {message}")),
        (_, Some(_)) => Text::new(msg!("line {number}: {message} (from {settings})")),
    };
    let text = text.arg("number", line.line).arg("message", &line.message);
    match &line.source {
        Some(source) => text.text("settings", setting_name(source)),
        None => text,
    }
}

/// What the parser couldn't follow in the user's config.
fn diagnostic_finding(diagnostic: &Diagnostic) -> Finding {
    let mut severity = Severity::Warning;
    let mut hint = None;
    let place = place(&diagnostic.file, diagnostic.line);
    let message = match &diagnostic.kind {
        DiagnosticKind::Unreadable { message } => {
            severity = Severity::Problem;
            Text::new(msg!("{place}: can't be read: {error}.")).arg("error", message)
        }
        DiagnosticKind::UnresolvedInclude { pattern, reason } => {
            hint = Some(msg!(
                "Sway Things never runs commands to read your config, so it doesn't know what \
                 that include sets."
            ));
            Text::new(msg!(
                "{place}: include {pattern} can't be followed: {reason}."
            ))
            .arg("pattern", pattern)
            .text("reason", reason.text())
        }
        DiagnosticKind::IncludeNotFound { path } => Text::new(msg!(
            "{place}: include: {path} doesn't exist, so sway skips it."
        ))
        .arg("path", path.display()),
        DiagnosticKind::AlreadyIncluded { path } => Text::new(msg!(
            "{place}: include: {path} was read already, so sway doesn't read it again."
        ))
        .arg("path", path.display()),
        DiagnosticKind::NotAFile => {
            severity = Severity::Problem;
            hint = Some(msg!(
                "Sway Things reads only regular files, as a device, FIFO or socket might never \
                 end, so it doesn't know what sway would read from it."
            ));
            msg!("{place}: isn't a regular file, so it wasn't read.").into()
        }
        DiagnosticKind::InvalidInclude => {
            msg!("{place}: include needs exactly one path, so sway rejects it.").into()
        }
        DiagnosticKind::TooDeep => {
            msg!("{place}: includes are nested too deeply to follow.").into()
        }
        DiagnosticKind::UnmatchedBrace => msg!("{place}: a }} closes no block.").into(),
        DiagnosticKind::UnclosedBlock => {
            msg!("{place}: a block is still open at the end of the file.").into()
        }
        DiagnosticKind::BlockTooDeep => msg!(
            "{place}: blocks are nested too deeply; this one was skipped with everything in it."
        )
        .into(),
        DiagnosticKind::TooBig => {
            msg!("{place}: the config is too big; the rest wasn't read.").into()
        }
    };
    let finding = Finding::new(
        Code::ConfigDiagnostic,
        severity,
        message.arg("place", place),
    )
    .at(&diagnostic.file, diagnostic.line);
    match hint {
        Some(hint) => finding.with_hint(hint),
        None => finding,
    }
}
