// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The checks of Screenshots & Clipboard: a part that is on while a program it runs is missing,
//! and a clipboard history the user's config starts besides ours.

use super::{Checkup, Code, Finding, Severity, place};
use crate::capture::tools::{self, Program};
use crate::generate::Validate;
use crate::i18n::Text;
use crate::msg;

impl<V: Validate> Checkup<'_, V> {
    /// The parts of Screenshots & Clipboard that are on while a program they run is missing
    /// ([`tools::missing_for`]): their keys do nothing but say so.
    pub(super) fn tools(missing: Vec<(&'static str, Vec<Program>)>, findings: &mut Vec<Finding>) {
        for (key, programs) in missing {
            let names: Vec<&str> = programs.iter().map(|program| program.name).collect();
            let packages = tools::packages(&programs);
            findings.push(
                Finding::new(
                    Code::ToolMissing,
                    Severity::Problem,
                    Text::new(msg!("{key} is on, but {programs} isn't installed."))
                        .arg("key", key)
                        .arg("programs", names.join(", ")),
                )
                .with_hint(
                    Text::new(msg!(
                        "Install the packages {packages} with your package manager."
                    ))
                    .arg("packages", packages.join(", ")),
                )
                .for_setting(key, None),
            );
        }
    }

    /// The clipboard history is on while a line of the user's config starts one of its own:
    /// Sway Things then starts none, and the page's settings don't apply to it.
    pub(super) fn history_twice(&self, findings: &mut Vec<Finding>) {
        let key = crate::capture::HISTORY;
        if !crate::capture::is_on(self.state, key) {
            return;
        }
        let Some(line) = crate::capture::clipboard::own_history(&self.status.config().directives)
        else {
            return;
        };
        findings.push(
            Finding::new(
                Code::HistoryTwice,
                Severity::Warning,
                Text::new(msg!(
                    "{key} is on, but your config starts a clipboard history of its own at \
                     {place}, so Sway Things starts none."
                ))
                .arg("key", key)
                .arg("place", place(&line.file, line.line)),
            )
            .with_hint(msg!(
                "Remove that line to have Sway Things keep the history, or switch the history off."
            ))
            .at(&line.file, line.line)
            .for_setting(key, None),
        );
    }
}
