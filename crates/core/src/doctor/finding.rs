// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the checkup reports: findings with their severity, section, code and fix, and the
//! diagnosis that holds them.

use std::path::{Path, PathBuf};

use crate::i18n::{Msg, Text};
use crate::msg;
use crate::registry::ConfigFile;

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
    /// A profile of the user's kanshi config that Sway Things copies to place the workspaces
    /// keeps a line that may place them too, or is in a file the copies don't reach.
    KanshiProfileCopy,
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
    /// A stored setting can't be copied to GSettings, so apps that read it there don't get it.
    SettingNotCopied,
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
            Self::KanshiProfileCopy => "kanshi_profile_copy",
            Self::HookProgram => "hook_program",
            Self::SettingNotCopied => "setting_not_copied",
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
            | Self::KanshiProfileCopy
            | Self::HookProgram => Section::Config,
            Self::SettingsLoaded
            | Self::SettingsUnusable
            | Self::SettingsEntry
            | Self::SettingSkipped
            | Self::SettingNotCopied => Section::Settings,
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
    pub(super) fn new(code: Code, severity: Severity, message: impl Into<Text>) -> Self {
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

    pub(super) fn with_hint(mut self, hint: impl Into<Text>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub(super) fn with_details(mut self, details: Vec<Text>) -> Self {
        self.details = details;
        self
    }

    pub(super) fn in_file(mut self, file: &Path) -> Self {
        self.file = Some(file.to_owned());
        self
    }

    /// In `file` at `line`; 0 for the whole file.
    pub(super) fn at(mut self, file: &Path, line: usize) -> Self {
        self.file = Some(file.to_owned());
        self.line = (line > 0).then_some(line);
        self
    }

    pub(super) fn for_setting(mut self, key: &'static str, device: Option<&str>) -> Self {
        self.key = Some(key);
        self.device = device.map(str::to_owned);
        self
    }

    pub(super) fn fixed_by(mut self, fix: Option<Fix>) -> Self {
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
    pub(super) findings: Vec<Finding>,
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
