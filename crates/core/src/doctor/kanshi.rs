// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The checks of the hooks and of kanshi: whether sway and kanshi can run `sway-things`, kanshi's
//! file of ours and the order of the user's profiles.

use super::{Checkup, Code, Finding, Severity};
use crate::generate::Validate;
use crate::i18n::Text;
use crate::msg;

impl<V: Validate> Checkup<'_, V> {
    /// Whether sway and kanshi can run `sway-things`: the generated config and kanshi's profiles
    /// run it by name for the hooks (the display guard, the workspaces, the pictures), so it
    /// must be in a directory of `path` (`$PATH`). One that isn't the program `this` (the one
    /// running now) is another build, which may not know the hooks of this one.
    pub(super) fn hook_program(
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
    pub(super) fn kanshi(&self, findings: &mut Vec<Finding>) {
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
    pub(super) fn kanshi_file(
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
    pub(super) fn kanshi_order(
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
}
