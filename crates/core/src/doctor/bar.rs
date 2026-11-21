// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bar's part of the checkup: the files Sway Things writes for waybar, whether waybar is
//! installed and runs them, and bars of the user's own config next to ours. Nothing is reported
//! while Sway Things doesn't set up a bar.

use super::{Checkup, Code, Finding, Fix, Severity};
use crate::bar::process::{self, Starter};
use crate::bar::{Backend, Modules, WaybarFiles, waybar};
use crate::generate::Validate;
use crate::i18n::Text;
use crate::include::WriteAccess;
use crate::msg;

impl<V: Validate> Checkup<'_, V> {
    /// The findings about the bar.
    pub(super) fn bar(&self, findings: &mut Vec<Finding>) {
        let backend = Backend::of(self.state);
        if backend == Backend::None {
            return;
        }
        let generated = self.locations.directory();
        for (file, line) in process::swaybars(self.status.config(), generated) {
            findings.push(
                Finding::new(
                    Code::BarOther,
                    Severity::Warning,
                    msg!("Your sway config sets up a swaybar of its own, next to the bar of Sway Things."),
                )
                .at(&file, line)
                .with_hint(msg!(
                    "Remove that bar block, or set the bar program to “Not set up here”."
                )),
            );
        }
        if backend == Backend::Swaybar {
            findings.push(Finding::new(
                Code::BarRunning,
                Severity::Ok,
                msg!("swaybar is set up in 75-bar.conf."),
            ));
            return;
        }
        // Only a checkup that looks at the machine asks for programs and processes.
        let looks = self.path.is_some();
        if looks && crate::delegate::find_program("waybar", self.path.as_deref()).is_none() {
            findings.push(
                Finding::new(
                    Code::BarWaybarMissing,
                    Severity::Problem,
                    msg!("waybar is the bar, but it isn't installed."),
                )
                .with_hint(msg!(
                    "Install the waybar package, or choose swaybar on the Bar page."
                )),
            );
        }
        let Some(files) = WaybarFiles::of(self.state) else {
            return;
        };
        if !self.bar_files(&files, findings) {
            return;
        }
        if !looks || !matches!(self.found, super::Found::Session(_)) {
            return;
        }
        self.bar_running(&files, findings);
    }

    /// Which waybar runs, what went wrong when it was last started, and a line that may start
    /// it.
    fn bar_running(&self, files: &WaybarFiles, findings: &mut Vec<Finding>) {
        let generated = self.locations.directory();
        let running = process::running();
        let ours = running.iter().filter(|waybar| waybar.runs(files)).count();
        let starter = Starter::detect(self.status.config(), generated, &running, files);
        if let Some(problem) = process::last_problem() {
            findings.push(
                Finding::new(Code::BarRunning, Severity::Problem, problem.text())
                    .with_hint(msg!("Run sway-things hook waybar to start it again.")),
            );
        }
        if let Starter::Maybe(line) = &starter {
            findings.push(
                Finding::new(
                    Code::BarRunning,
                    Severity::Warning,
                    msg!(
                        "A line of your sway config may start waybar; Sway Things can't tell, so \
                         it doesn't start a second one."
                    ),
                )
                .at(&line.file, line.line)
                .with_hint(
                    Text::new(msg!("To use the bar of the Bar page there: {line}")).text(
                        "line",
                        Text::data(process::handover(&starter, files).unwrap_or_default()),
                    ),
                ),
            );
        }
        let finding = if ours > 0 {
            Finding::new(
                Code::BarRunning,
                Severity::Ok,
                msg!("waybar runs with the config of Sway Things."),
            )
        } else if running.is_empty() {
            Finding::new(Code::BarRunning, Severity::Warning, msg!("No waybar runs."))
                .with_hint(msg!("It starts with sway; or run sway-things hook waybar."))
        } else {
            let hint = process::handover(&starter, files).map_or_else(
                || {
                    Text::new(msg!(
                        "Start waybar with -c and -s pointing at the files of Sway Things."
                    ))
                },
                |line| {
                    Text::new(msg!("Change the line that starts it to: {line}"))
                        .text("line", Text::data(line))
                },
            );
            Finding::new(
                Code::BarRunning,
                Severity::Warning,
                msg!("waybar runs, but with another config than the one of Sway Things."),
            )
            .with_hint(hint)
        };
        findings.push(finding);
    }

    /// The findings about the files for waybar; `false` if the module definitions can't be read.
    fn bar_files(&self, files: &WaybarFiles, findings: &mut Vec<Finding>) -> bool {
        let modules = match Modules::path_of(self.state).map(|path| Modules::load(&path)) {
            Some(Ok(modules)) => modules,
            Some(Err(error)) => {
                findings.push(Finding::new(Code::BarFile, Severity::Problem, error.text()));
                return false;
            }
            None => Modules::default(),
        };
        for (path, comment, text) in
            waybar::texts(files, self.state, &modules, Some(self.status.config()))
        {
            let finding = match std::fs::read_to_string(&path) {
                Ok(old) if old == text => Finding::new(
                    Code::BarFile,
                    Severity::Ok,
                    Text::new(msg!("{file} is up to date.")).arg("file", path.display()),
                ),
                Ok(_) if !WaybarFiles::is_ours_or_missing(&path, comment) => Finding::new(
                    Code::BarFile,
                    Severity::Problem,
                    Text::new(msg!(
                        "{file} wasn't written by Sway Things, so waybar doesn't get the bar of the Bar page."
                    ))
                    .arg("file", path.display()),
                )
                .with_hint(msg!("Move it away, so Sway Things can write it.")),
                Ok(_) => Finding::new(
                    Code::BarFile,
                    Severity::Warning,
                    Text::new(msg!("{file} doesn't hold the bar as it is set."))
                        .arg("file", path.display()),
                )
                .fixed_by(Some(Fix::Regenerate)),
                Err(_) => Finding::new(
                    Code::BarFile,
                    Severity::Warning,
                    Text::new(msg!("{file} is missing.")).arg("file", path.display()),
                )
                .fixed_by(Some(Fix::Regenerate)),
            };
            findings.push(finding.in_file(&path));
        }
        true
    }

    /// Writes the files for waybar, while waybar is the bar (for `--fix`).
    pub(super) fn fix_bar(&self, access: &WriteAccess) {
        if Backend::of(self.state) != Backend::Waybar {
            return;
        }
        let (Some(files), Some(path)) = (WaybarFiles::of(self.state), Modules::path_of(self.state))
        else {
            return;
        };
        if let Ok(modules) = Modules::load(&path) {
            let _ = waybar::write(
                &files,
                self.state,
                &modules,
                Some(self.status.config()),
                access,
            );
        }
    }
}
