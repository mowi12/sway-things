// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `doctor`: the core's checkup as text for people or JSON for scripts, and `--fix`.

use std::io::Write;

use clap::Args;
use serde_json::{Value as Json, json};
use sway_things_core::doctor::{Checkup, Diagnosis, Finding, Fix, FixError, Section, Severity};
use sway_things_core::generate::{Outcome, Report, Validate};

use crate::Exit;

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

/// Checks the setup of `checkup`, fixes what `args` asks for, and reports to `out`. Exits with
/// [`Exit::Failure`] if a problem is left or the fix failed, [`Exit::Ok`] otherwise: warnings
/// don't count.
pub async fn doctor<V: Validate>(
    checkup: &Checkup<'_, V>,
    args: &DoctorArgs,
    out: &mut dyn Write,
) -> Exit {
    let fixed = if args.fix {
        Some(fix(checkup, args.overwrite).await)
    } else {
        None
    };
    let diagnosis = checkup.diagnose().await;
    if args.json {
        let _ = writeln!(out, "{}", json(&diagnosis, fixed.as_ref(), checkup));
    } else {
        text(&diagnosis, fixed.as_ref(), checkup, args, out);
    }
    if diagnosis.severity() == Severity::Problem || matches!(fixed, Some(Fixed::Failed(_))) {
        Exit::Failure
    } else {
        Exit::Ok
    }
}

async fn fix<V: Validate>(checkup: &Checkup<'_, V>, overwrite: bool) -> Fixed {
    let before = checkup.diagnose().await;
    if !before.can_fix(overwrite) {
        return Fixed::Nothing {
            edited: before.can_fix(true),
        };
    }
    let Ok(access) = checkup.status.write_access() else {
        return Fixed::ReadOnly;
    };
    match checkup.fix(overwrite, &access).await {
        Ok(report) => Fixed::Done(report),
        Err(error) => Fixed::Failed(error),
    }
}

/// The width of the status column.
const STATUS: usize = 7;

fn text<V>(
    diagnosis: &Diagnosis,
    fixed: Option<&Fixed>,
    checkup: &Checkup<'_, V>,
    args: &DoctorArgs,
    out: &mut dyn Write,
) {
    let indent = " ".repeat(2 + STATUS + 2);
    if let Some(fixed) = fixed {
        let _ = writeln!(out, "Fix");
        if let Some(message) = fixed.message() {
            let _ = writeln!(out, "  {message}");
        }
        if let Fixed::Done(report) = fixed {
            for (file, outcome) in report {
                let path = checkup.locations.path(*file);
                let line = match outcome {
                    Outcome::Written => format!("wrote {}", path.display()),
                    Outcome::Unchanged | Outcome::NotCreated => continue,
                    _ => outcome
                        .problem(*file)
                        .unwrap_or_else(|| file.file_name().to_owned()),
                };
                let _ = writeln!(out, "  {line}");
                if let Outcome::Invalid(lines) = outcome {
                    for invalid in lines {
                        let _ = writeln!(out, "{indent}{}", crate::output::invalid_line(invalid));
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
        let _ = writeln!(out, "{}", section.title());
        for finding in findings {
            finding_text(finding, &indent, out);
        }
    }

    let _ = writeln!(out);
    let problems = diagnosis.count(Severity::Problem);
    let warnings = diagnosis.count(Severity::Warning);
    let summary = match (problems, warnings) {
        (0, 0) => "Everything is fine.".to_owned(),
        _ => format!(
            "{}, {}.",
            counted(problems, "problem"),
            counted(warnings, "warning")
        ),
    };
    let _ = writeln!(out, "{summary}");
    if !args.fix && diagnosis.can_fix(true) {
        let _ = writeln!(
            out,
            "Run `sway-things doctor --fix` to write the generated files again."
        );
    }
}

fn finding_text(finding: &Finding, indent: &str, out: &mut dyn Write) {
    let mut lines = finding.message.lines();
    let _ = writeln!(
        out,
        "  {:<STATUS$}  {}",
        finding.severity.id(),
        lines.next().unwrap_or_default()
    );
    for line in lines {
        write_indented(out, indent, line);
    }
    for detail in &finding.details {
        write_indented(out, &format!("{indent}  "), detail);
    }
    if let Some(hint) = &finding.hint {
        for line in hint.lines() {
            write_indented(out, indent, line);
        }
    }
    match finding.fix {
        Some(Fix::Regenerate) => write_indented(out, indent, "Fix: sway-things doctor --fix"),
        Some(Fix::Overwrite) => write_indented(
            out,
            indent,
            "Fix: sway-things doctor --fix --overwrite (your changes to the file are lost)",
        ),
        None => {}
    }
}

/// `line` after `indent`; an empty line stays empty.
fn write_indented(out: &mut dyn Write, indent: &str, line: &str) {
    if line.is_empty() {
        let _ = writeln!(out);
    } else {
        let _ = writeln!(out, "{indent}{line}");
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
