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

mod files;
mod finding;
mod found;
mod kanshi;
#[cfg(test)]
mod tests;

pub use finding::{Code, Diagnosis, Finding, Fix, Section, Severity, Step};
pub use found::{FixError, Found, Installed};

use std::path::Path;

use crate::conflict;
use crate::generate::{
    self, ConfigError, GeneratedFile, InvalidLine, Locations, Report, Source, Validate,
};
use crate::gsettings::{self, GSettingsError, GSettingsStore};
use crate::i18n::{Msg, Text};
use crate::include::{IncludeStatus, Inclusion, WriteAccess};
use crate::registry::{ConfigFile, Scope};
use crate::state::{ProblemKind, State};
use crate::sway_config::{Diagnostic, DiagnosticKind};
use crate::version::{SwayFxVersion, SwayVersion};
use crate::{msg, msg_plural};

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
    /// Where settings are copied to GSettings, checked for the stored ones.
    pub gsettings: &'a dyn GSettingsStore,
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
        for (key, error) in gsettings::check_stored(self.state, self.gsettings) {
            findings.push(
                Finding::new(
                    Code::SettingNotCopied,
                    Severity::Warning,
                    Text::new(msg!("{setting}: {reason}."))
                        .arg("setting", key)
                        .text("reason", error.text()),
                )
                .with_hint(gsettings_hint(&error))
                .for_setting(key, None),
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
/// What to do about a value GSettings can't take. Sway Things writes it again when sway starts
/// or reloads.
fn gsettings_hint(error: &GSettingsError) -> Msg {
    match error {
        GSettingsError::NoSchema { .. } | GSettingsError::NoKey { .. } => msg!(
            "Install the package gsettings-desktop-schemas, or a newer one. Sway Things writes the \
             value when sway starts or reloads."
        ),
        GSettingsError::NotWritable { .. } => msg!(
            "The system's administrator locked the key. Reset the setting, or ask them to unlock \
             it."
        ),
        GSettingsError::NotKept { .. } => msg!(
            "Install dconf (the package dconf-service or dconf), then log in again. Sway Things \
             writes the value when sway starts or reloads."
        ),
        GSettingsError::WrongType { .. } | GSettingsError::Rejected { .. } => msg!(
            "The installed schema doesn't take this value; it may be older than Sway Things \
             expects. Pick another value or reset the setting."
        ),
    }
}

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
