// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The check of the program that draws the wallpaper: sway starts `swaybg_command` for the
//! `output … bg` lines, and the user's config can name none or one that isn't there.

use std::path::Path;

use super::{Checkup, Code, Finding, Severity, place};
use crate::generate::Validate;
use crate::i18n::Text;
use crate::msg;
use crate::wallpaper;

impl<V: Validate> Checkup<'_, V> {
    /// Whether sway can draw the stored wallpaper: not with `swaybg_command -`, nor with a
    /// program that isn't installed. Only while a wallpaper is stored; whether a program is in
    /// `$PATH` only for a checkup that looks at the machine ([`Checkup::path`]).
    pub(super) fn swaybg(&self, findings: &mut Vec<Finding>) {
        let stored = self
            .state
            .entries()
            .iter()
            .any(|entry| entry.setting.key().starts_with("wallpaper."));
        if !stored {
            return;
        }
        let Some(line) = wallpaper::swaybg_command(self.status.config()) else {
            return;
        };
        let Some(program) = line.args.first() else {
            return;
        };
        let at = place(&line.file, line.line);
        let message = if program == "-" {
            Text::new(msg!(
                "Your config sets swaybg_command - ({place}), so sway starts no program for the \
                 wallpaper and doesn't show the one set in Sway Things."
            ))
            .arg("place", &at)
        } else if self.is_missing(program) {
            Text::new(msg!(
                "Your config sets swaybg_command {program} ({place}), but it can't be found, so \
                 sway doesn't show the wallpaper set in Sway Things."
            ))
            .arg("program", program)
            .arg("place", &at)
        } else {
            return;
        };
        findings.push(
            Finding::new(Code::SwaybgCommand, Severity::Problem, message)
                .at(&line.file, line.line)
                .with_hint(msg!(
                    "Take the line out, so sway uses swaybg, or name a program that is installed."
                )),
        );
    }

    /// Whether `program` of `swaybg_command` can't be run: an absolute path that isn't a file,
    /// or a name in no directory of `$PATH`. Anything else (`~/…`, a relative path) isn't
    /// judged.
    fn is_missing(&self, program: &str) -> bool {
        if program.starts_with('/') {
            return !Path::new(program).is_file();
        }
        if program.contains('/') {
            return false;
        }
        self.path.as_ref().is_some_and(|path| {
            crate::delegate::find_program(program, Some(path.as_os_str())).is_none()
        })
    }
}
