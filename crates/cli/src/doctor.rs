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

    /// What happened, in a sentence, unless it is said per file.
    fn message(&self) -> Option<String> {
        match self {
            Self::Nothing { edited: true } => Some(
                "Only generated files that were changed outside Sway Things are left; add \
                 --overwrite to replace them."
                    .to_owned(),
            ),
            Self::Nothing { edited: false } => Some("Nothing for --fix to do.".to_owned()),
            Self::ReadOnly => Some(
                "Nothing was written: Sway Things is read-only until your config includes its \
                 files."
                    .to_owned(),
            ),
            Self::Done(_) => None,
            Self::Failed(error) => {
                Some(format!("The generated files couldn't be written: {error}."))
            }
        }
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
/// or JSON. Exits with [`Exit::Failure`] if a problem is left or the fix failed, [`Exit::Ok`]
/// otherwise: warnings don't count.
pub async fn doctor<V: Validate>(
    checkup: &Checkup<'_, V>,
    args: &DoctorArgs,
    out: &mut dyn Write,
) -> Exit {
    doctor_with(checkup, args, out, &Look::default()).await
}

/// [`doctor`], with the text report as `look` says.
pub async fn doctor_with<V: Validate>(
    checkup: &Checkup<'_, V>,
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
        let _ = writeln!(out, "{}", json(&diagnosis, fixed.as_ref(), checkup));
    } else {
        let paint = Paint { color: look.color };
        text(&diagnosis, fixed.as_ref(), checkup, args, paint, out);
    }
    if diagnosis.severity() == Severity::Problem || matches!(fixed, Some(Fixed::Failed(_))) {
        Exit::Failure
    } else {
        Exit::Ok
    }
}

/// What the spinner says while `step` runs.
fn step_message(step: Step) -> Cow<'static, str> {
    match step {
        Step::Section(Section::Compositor) => "Checking the compositor…".into(),
        Step::Section(Section::Config) => "Checking your sway config…".into(),
        Step::Section(Section::Settings) => "Checking the settings files…".into(),
        Step::Section(Section::Generated) => "Checking the generated files…".into(),
        Step::Validate(file) => format!("Validating {} with sway…", file.file_name()).into(),
        Step::Section(Section::Conflicts) => {
            "Comparing your config with the stored settings…".into()
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
    spinner.say("Writing the generated files and validating them with sway…");
    match checkup.fix(overwrite, &access).await {
        Ok(report) => Fixed::Done(report),
        Err(error) => Fixed::Failed(error),
    }
}

/// The width of the status column.
const STATUS: usize = 7;

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

fn text<V>(
    diagnosis: &Diagnosis,
    fixed: Option<&Fixed>,
    checkup: &Checkup<'_, V>,
    args: &DoctorArgs,
    paint: Paint,
    out: &mut dyn Write,
) {
    let indent = " ".repeat(2 + STATUS + 2);
    if let Some(fixed) = fixed {
        let _ = writeln!(out, "{}", paint.paint(HEADING, "Fix"));
        if let Some(message) = fixed.message() {
            let _ = writeln!(out, "  {message}");
        }
        if let Fixed::Done(report) = fixed {
            for (file, outcome) in report {
                let path = checkup.locations.path(*file);
                let line = match outcome {
                    Outcome::Written => {
                        format!("wrote {}", paint.paint(DIM, &path.display().to_string()))
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
        let _ = writeln!(out, "{}", paint.paint(HEADING, section.title()));
        for finding in findings {
            finding_text(finding, &indent, paint, out);
        }
    }

    let _ = writeln!(out);
    let problems = diagnosis.count(Severity::Problem);
    let warnings = diagnosis.count(Severity::Warning);
    let summary = if problems == 0 && warnings == 0 {
        paint
            .paint(AnsiColor::Green.on_default().bold(), "Everything is fine.")
            .to_string()
    } else {
        let count = |count: usize, thing: &str, color: AnsiColor| {
            let style = if count == 0 {
                Style::new()
            } else {
                color.on_default().bold()
            };
            paint.paint(style, &counted(count, thing)).to_string()
        };
        format!(
            "{}, {}.",
            count(problems, "problem", AnsiColor::Red),
            count(warnings, "warning", AnsiColor::Yellow)
        )
    };
    let _ = writeln!(out, "{summary}");
    if !args.fix && diagnosis.can_fix(true) {
        let _ = writeln!(
            out,
            "Run `{}` to write the generated files again.",
            paint.paint(COMMAND, "sway-things doctor --fix")
        );
    }
}

fn finding_text(finding: &Finding, indent: &str, paint: Paint, out: &mut dyn Write) {
    let mut lines = finding.message.lines();
    let status = finding.severity.id();
    let _ = writeln!(
        out,
        "  {}{}  {}",
        paint.paint(severity_style(finding.severity), status),
        " ".repeat(STATUS.saturating_sub(status.len())),
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
    let (command, note) = match finding.fix {
        Some(Fix::Regenerate) => ("sway-things doctor --fix", ""),
        Some(Fix::Overwrite) => (
            "sway-things doctor --fix --overwrite",
            " (your changes to the file are lost)",
        ),
        None => return,
    };
    let _ = writeln!(
        out,
        "{indent}{} {}{}",
        paint.paint(DIM, "Fix:"),
        paint.paint(COMMAND, command),
        paint.paint(DIM, note)
    );
}

/// `line` after `indent`, in `style`; an empty line stays empty.
fn write_indented(out: &mut dyn Write, indent: &str, line: &str, style: Style, paint: Paint) {
    if line.is_empty() {
        let _ = writeln!(out);
    } else {
        let _ = writeln!(out, "{indent}{}", paint.paint(style, line));
    }
}

/// `1 problem`, `2 warnings`, `no problems`.
fn counted(count: usize, thing: &str) -> String {
    match count {
        0 => format!("no {thing}s"),
        1 => format!("1 {thing}"),
        count => format!("{count} {thing}s"),
    }
}

fn json<V>(diagnosis: &Diagnosis, fixed: Option<&Fixed>, checkup: &Checkup<'_, V>) -> Json {
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
            "message": fixed.message(),
            "files": files,
        })
    });
    json!({
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
