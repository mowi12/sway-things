// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `doctor`: the core's checkup as text for people or JSON for scripts, and `--fix`.
//!
//! On a terminal, the text is colored and a spinner on stderr tells which check runs; see
//! [`Look`].

use std::borrow::Cow;
use std::fmt;
use std::io::Write;
use std::path::Path;

use anstyle::{AnsiColor, Style};
use clap::Args;
use serde_json::{Value as Json, json};
use sway_things_core::doctor::{
    Checkup, Diagnosis, Finding, Fix, FixError, Section, Severity, Step,
};
use sway_things_core::generate::{Outcome, Report, Validate};
use sway_things_core::i18n::{Msg, MsgPlural, fill, ntrf, tr, trf};
use sway_things_core::system::{self, Details};
use sway_things_core::{msg, msg_plural};

use crate::Exit;
use crate::spinner::Spinner;

/// The options of `doctor`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct DoctorArgs {
    /// Write the generated files that are missing or out of date. Never touches your own files
    #[arg(long)]
    pub fix: bool,
    /// With --fix, also replace generated files that were changed outside Sway Things
    #[arg(long, requires = "fix")]
    pub overwrite: bool,
    /// Print JSON for scripts
    #[arg(long)]
    pub json: bool,
}

/// What `--fix` did.
enum Fixed {
    /// Nothing could be fixed; `edited` if only files changed outside are left, which need
    /// `--overwrite`.
    Nothing { edited: bool },
    /// The include line is missing, so nothing is written.
    ReadOnly,
    /// The files were written, as far as sway accepted them.
    Done(Report),
    /// The files couldn't be written.
    Failed(FixError),
}

impl Fixed {
    /// The outcome as an id for scripts.
    fn id(&self) -> &'static str {
        match self {
            Self::Nothing { .. } => "nothing",
            Self::ReadOnly => "read_only",
            Self::Done(_) => "done",
            Self::Failed(_) => "failed",
        }
    }

    /// What happened, in a sentence, unless it is said per file: translated by `translate`, which
    /// is [`tr`] for people and [`Msg::msgid`] for scripts.
    fn message(&self, translate: fn(Msg) -> &'static str) -> Option<String> {
        let text = match self {
            Self::Nothing { edited: true } => msg!(
                "Only generated files that were changed outside Sway Things are left; add \
                 --overwrite to replace them."
            ),
            Self::Nothing { edited: false } => msg!("Nothing for --fix to do."),
            Self::ReadOnly => msg!(
                "Nothing was written: Sway Things is read-only until your config includes its \
                 files."
            ),
            Self::Done(_) => return None,
            Self::Failed(error) => {
                return Some(fill(
                    translate(msg!("The generated files couldn't be written: {error}.")),
                    &[("error", error)],
                ));
            }
        };
        Some(translate(text).to_owned())
    }
}

/// How the text report looks. The default is plain text without a spinner, as for a pipe.
#[derive(Debug, Default)]
pub struct Look {
    /// Color the headings, the status words, the summary and the commands, and dim paths,
    /// details and hints, with the 16 standard ANSI colors. Never for `--json`.
    pub color: bool,
    /// Tells which check runs, until the report is printed.
    pub spinner: Spinner,
}

/// Checks the setup of `checkup`, fixes what `args` asks for, and reports to `out` as plain text
/// or JSON, after the system details `system`, if given. Exits with [`Exit::Failure`] if the fix failed, [`Exit::Problems`] if a problem is
/// left, and [`Exit::Ok`] otherwise: warnings don't count.
pub async fn doctor<V: Validate>(
    checkup: &Checkup<'_, V>,
    system: Option<&Details>,
    args: &DoctorArgs,
    out: &mut dyn Write,
) -> Exit {
    doctor_with(checkup, system, args, out, &Look::default()).await
}

/// [`doctor`], with the text report as `look` says.
pub async fn doctor_with<V: Validate>(
    checkup: &Checkup<'_, V>,
    system: Option<&Details>,
    args: &DoctorArgs,
    out: &mut dyn Write,
    look: &Look,
) -> Exit {
    let spinner = &look.spinner;
    let fixed = if args.fix {
        Some(fix(checkup, args.overwrite, spinner).await)
    } else {
        None
    };
    let diagnosis = checkup
        .diagnose_with(|step| spinner.say(step_message(step)))
        .await;
    spinner.clear();
    if args.json {
        let _ = writeln!(out, "{}", json(&diagnosis, fixed.as_ref(), checkup, system));
    } else {
        let paint = Paint { color: look.color };
        if let Some(system) = system {
            system_text(system, paint, out);
        }
        text(&diagnosis, fixed.as_ref(), checkup, args, paint, out);
    }
    if matches!(fixed, Some(Fixed::Failed(_))) {
        Exit::Failure
    } else if diagnosis.severity() == Severity::Problem {
        Exit::Problems
    } else {
        Exit::Ok
    }
}

/// What the spinner says while `step` runs.
fn step_message(step: Step) -> Cow<'static, str> {
    match step {
        Step::Section(Section::Compositor) => tr(msg!("Checking the compositor…")).into(),
        Step::Section(Section::Config) => tr(msg!("Checking your sway config…")).into(),
        Step::Section(Section::Settings) => tr(msg!("Checking the settings files…")).into(),
        Step::Section(Section::Generated) => tr(msg!("Checking the generated files…")).into(),
        Step::Validate(file) => trf(
            msg!("Validating {file} with sway…"),
            &[("file", &file.file_name())],
        )
        .into(),
        Step::Section(Section::Conflicts) => {
            tr(msg!("Comparing your config with the stored settings…")).into()
        }
    }
}

async fn fix<V: Validate>(checkup: &Checkup<'_, V>, overwrite: bool, spinner: &Spinner) -> Fixed {
    let before = checkup
        .diagnose_with(|step| spinner.say(step_message(step)))
        .await;
    if !before.can_fix(overwrite) {
        return Fixed::Nothing {
            edited: before.can_fix(true),
        };
    }
    let Ok(access) = checkup.status.write_access() else {
        return Fixed::ReadOnly;
    };
    spinner.say(tr(msg!(
        "Writing the generated files and validating them with sway…"
    )));
    match checkup.fix(overwrite, &access).await {
        Ok(report) => Fixed::Done(report),
        Err(error) => Fixed::Failed(error),
    }
}

/// The status word of `severity`, padded to the width of the longest.
fn status_word(severity: Severity) -> String {
    let word = |severity| {
        tr(match severity {
            // TRANSLATORS: A status word of `sway-things doctor`, in a column before each finding.
            Severity::Ok => msg!("ok"),
            // TRANSLATORS: A status word of `sway-things doctor`, in a column before each finding.
            Severity::Warning => msg!("warning"),
            // TRANSLATORS: A status word of `sway-things doctor`, in a column before each finding.
            Severity::Problem => msg!("problem"),
        })
    };
    let width = [Severity::Ok, Severity::Warning, Severity::Problem]
        .into_iter()
        .map(|severity| word(severity).chars().count())
        .max()
        .unwrap_or_default();
    format!("{:width$}", word(severity))
}

/// Section headings.
const HEADING: Style = Style::new().bold();
/// Paths, details and hints: worth less attention than the message.
const DIM: Style = Style::new().dimmed();
/// A command to run.
const COMMAND: Style = AnsiColor::Cyan.on_default();

/// The style of a status word.
fn severity_style(severity: Severity) -> Style {
    match severity {
        Severity::Ok => AnsiColor::Green.on_default(),
        Severity::Warning => AnsiColor::Yellow.on_default(),
        Severity::Problem => AnsiColor::Red.on_default(),
    }
}

/// Styles text with ANSI escape codes, or leaves it as it is.
#[derive(Debug, Clone, Copy)]
struct Paint {
    color: bool,
}

impl Paint {
    fn paint(self, style: Style, text: &str) -> Painted<'_> {
        Painted {
            style: if self.color { style } else { Style::new() },
            text,
        }
    }

    /// `text` with every mention of `file` dimmed, with the line number after it, if any.
    fn dim_path(self, text: &str, file: Option<&Path>) -> String {
        let Some(file) = file.filter(|_| self.color) else {
            return text.to_owned();
        };
        let path = file.display().to_string();
        if path.is_empty() {
            return text.to_owned();
        }
        let mut result = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find(&path) {
            let after = &rest[start + path.len()..];
            let digits = after.strip_prefix(':').map_or(0, |line| {
                line.bytes().take_while(u8::is_ascii_digit).count()
            });
            let end = start + path.len() + if digits > 0 { 1 + digits } else { 0 };
            result.push_str(&rest[..start]);
            result.push_str(&self.paint(DIM, &rest[start..end]).to_string());
            rest = &rest[end..];
        }
        result.push_str(rest);
        result
    }
}

/// Text in a style; the plain style and empty text add no escape codes.
struct Painted<'a> {
    style: Style,
    text: &'a str,
}

impl fmt::Display for Painted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.text.is_empty() {
            return Ok(());
        }
        write!(
            f,
            "{}{}{}",
            self.style.render(),
            self.text,
            self.style.render_reset()
        )
    }
}

/// The system details, under their heading, then a blank line.
fn system_text(system: &Details, paint: Paint, out: &mut dyn Write) {
    let _ = writeln!(out, "{}", paint.paint(HEADING, tr(system::heading())));
    for (label, value) in system.lines() {
        let _ = writeln!(out, "  {label}: {}", value.text());
    }
    let _ = writeln!(out);
}

/// The system details for scripts: a value or `null` each, and whether each tool is installed.
fn system_json(system: &Details) -> Json {
    let tools: serde_json::Map<String, Json> = system
        .tools
        .iter()
        .map(|(tool, installed)| (tool.program.to_owned(), Json::Bool(*installed)))
        .collect();
    json!({
        "sway_things": system::SWAY_THINGS_VERSION,
        "compositor": system.compositor.map(|compositor| compositor.to_string()),
        "gtk": system.gtk,
        "distribution": system.distribution,
        "network_manager": system.network_manager,
        "kernel": system.kernel,
        "tools": tools,
    })
}

fn text<V>(
    diagnosis: &Diagnosis,
    fixed: Option<&Fixed>,
    checkup: &Checkup<'_, V>,
    args: &DoctorArgs,
    paint: Paint,
    out: &mut dyn Write,
) {
    let status_width = status_word(Severity::Ok).chars().count();
    let indent = " ".repeat(2 + status_width + 2);
    if let Some(fixed) = fixed {
        // TRANSLATORS: The heading of what `sway-things doctor --fix` did.
        let _ = writeln!(out, "{}", paint.paint(HEADING, tr(msg!("Fix"))));
        if let Some(message) = fixed.message(tr) {
            let _ = writeln!(out, "  {message}");
        }
        if let Fixed::Done(report) = fixed {
            for (file, outcome) in report {
                let path = checkup.locations.path(*file);
                let line = match outcome {
                    Outcome::Written => {
                        let path = path.display().to_string();
                        trf(msg!("wrote {file}"), &[("file", &paint.paint(DIM, &path))])
                    }
                    Outcome::Unchanged | Outcome::NotCreated => continue,
                    _ => outcome
                        .problem(*file)
                        .unwrap_or_else(|| file.file_name().to_owned()),
                };
                let _ = writeln!(out, "  {line}");
                if let Outcome::Invalid(lines) = outcome {
                    for invalid in lines {
                        let invalid = crate::output::invalid_line(invalid);
                        let _ = writeln!(out, "{indent}{}", paint.paint(DIM, &invalid));
                    }
                }
            }
        }
        let _ = writeln!(out);
    }

    let mut first = true;
    for section in Section::ALL {
        let findings: Vec<&Finding> = diagnosis
            .findings()
            .iter()
            .filter(|finding| finding.section() == section)
            .collect();
        if findings.is_empty() {
            continue;
        }
        if !first {
            let _ = writeln!(out);
        }
        first = false;
        let _ = writeln!(out, "{}", paint.paint(HEADING, tr(section.title())));
        for finding in findings {
            finding_text(finding, &indent, paint, out);
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "{}", summary(diagnosis, paint));
    if !args.fix && diagnosis.can_fix(true) {
        let command = paint.paint(COMMAND, "sway-things doctor --fix");
        let _ = writeln!(
            out,
            "{}",
            trf(
                msg!("Run `{command}` to write the generated files again."),
                &[("command", &command)]
            )
        );
    }
}

/// The last line of the report: how many problems and warnings there are.
fn summary(diagnosis: &Diagnosis, paint: Paint) -> String {
    let problems = diagnosis.count(Severity::Problem);
    let warnings = diagnosis.count(Severity::Warning);
    if problems == 0 && warnings == 0 {
        return paint
            .paint(
                AnsiColor::Green.on_default().bold(),
                tr(msg!("Everything is fine.")),
            )
            .to_string();
    }
    let counted = |count: usize, none: Msg, some: MsgPlural, color: AnsiColor| {
        if count == 0 {
            tr(none).to_owned()
        } else {
            let text = ntrf(some, count, &[("count", &count)]);
            paint.paint(color.on_default().bold(), &text).to_string()
        }
    };
    let problems = counted(
        problems,
        msg!("no problems"),
        msg_plural!("{count} problem", "{count} problems"),
        AnsiColor::Red,
    );
    let warnings = counted(
        warnings,
        msg!("no warnings"),
        msg_plural!("{count} warning", "{count} warnings"),
        AnsiColor::Yellow,
    );
    // TRANSLATORS: The summary of `sway-things doctor`: "2 problems, no warnings."
    trf(
        msg!("{problems}, {warnings}."),
        &[("problems", &problems), ("warnings", &warnings)],
    )
}

fn finding_text(finding: &Finding, indent: &str, paint: Paint, out: &mut dyn Write) {
    let mut lines = finding.message.lines();
    let status = status_word(finding.severity);
    let (word, padding) = status.split_at(status.trim_end().len());
    let _ = writeln!(
        out,
        "  {}{padding}  {}",
        paint.paint(severity_style(finding.severity), word),
        paint.dim_path(lines.next().unwrap_or_default(), finding.file.as_deref())
    );
    for line in lines {
        write_indented(out, indent, line, Style::new(), paint);
    }
    for detail in &finding.details {
        write_indented(out, &format!("{indent}  "), detail, DIM, paint);
    }
    if let Some(hint) = &finding.hint {
        for line in hint.lines() {
            write_indented(out, indent, line, DIM, paint);
        }
    }
    let (command, text) = match finding.fix {
        // TRANSLATORS: Before a command that fixes a finding of `sway-things doctor`.
        Some(Fix::Regenerate) => ("sway-things doctor --fix", msg!("Fix: {command}")),
        Some(Fix::Overwrite) => (
            "sway-things doctor --fix --overwrite",
            msg!("Fix: {command} (your changes to the file are lost)"),
        ),
        None => return,
    };
    // The command keeps its own style; the text around it is dimmed.
    let text = tr(text);
    let line = match text.split_once("{command}") {
        Some((before, after)) => format!(
            "{}{}{}",
            paint.paint(DIM, before),
            paint.paint(COMMAND, command),
            paint.paint(DIM, after)
        ),
        None => paint.paint(DIM, text).to_string(),
    };
    let _ = writeln!(out, "{indent}{line}");
}

/// `line` after `indent`, in `style`; an empty line stays empty.
fn write_indented(out: &mut dyn Write, indent: &str, line: &str, style: Style, paint: Paint) {
    if line.is_empty() {
        let _ = writeln!(out);
    } else {
        let _ = writeln!(out, "{indent}{}", paint.paint(style, line));
    }
}

fn json<V>(
    diagnosis: &Diagnosis,
    fixed: Option<&Fixed>,
    checkup: &Checkup<'_, V>,
    system: Option<&Details>,
) -> Json {
    let findings: Vec<Json> = diagnosis
        .findings()
        .iter()
        .map(|finding| {
            json!({
                "section": finding.section().id(),
                "code": finding.code.id(),
                "status": finding.severity.id(),
                "message": finding.message,
                "details": finding.details,
                "hint": finding.hint,
                "file": finding.file.as_ref().map(|file| file.display().to_string()),
                "line": finding.line,
                "key": finding.key,
                "device": finding.device,
                "fix": finding.fix.map(Fix::id),
            })
        })
        .collect();
    let fix = fixed.map(|fixed| {
        let files: Vec<Json> = match fixed {
            Fixed::Done(report) => report
                .iter()
                .map(|(file, outcome)| {
                    let errors: Vec<String> = match outcome {
                        Outcome::Invalid(lines) => {
                            lines.iter().map(crate::output::invalid_line).collect()
                        }
                        _ => Vec::new(),
                    };
                    json!({
                        "file": checkup.locations.path(*file).display().to_string(),
                        "outcome": outcome_id(outcome),
                        "errors": errors,
                    })
                })
                .collect(),
            _ => Vec::new(),
        };
        json!({
            "result": fixed.id(),
            "message": fixed.message(Msg::msgid),
            "files": files,
        })
    });
    json!({
        "system": system.map(system_json),
        "status": diagnosis.severity().id(),
        "problems": diagnosis.count(Severity::Problem),
        "warnings": diagnosis.count(Severity::Warning),
        "findings": findings,
        "fix": fix,
    })
}

fn outcome_id(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Written => "written",
        Outcome::Unchanged => "unchanged",
        Outcome::NotCreated => "not_created",
        Outcome::EditedOutside => "edited_outside",
        Outcome::Foreign => "foreign",
        Outcome::Invalid(_) => "invalid",
    }
}
