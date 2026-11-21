// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The checks of the generated files: each against the manifest and what would be generated
//! now, and through `sway --validate`.

use std::collections::BTreeSet;

use super::{Checkup, Code, Finding, Fix, Severity, Step, validated};
use crate::generate::{GeneratedFile, HEADER, Manifest, Ownership, Validate};
use crate::i18n::Text;
use crate::msg;
use crate::registry::{ConfigFile, SwayMapping};

impl<V: Validate> Checkup<'_, V> {
    pub(super) async fn files(
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
    pub(super) fn ownership_finding(
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
}
