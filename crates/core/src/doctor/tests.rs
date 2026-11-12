// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::fs;
use std::future::{Future, ready};
use std::io;
use std::path::{Path, PathBuf};

use async_io::block_on;

use super::{
    Checkup, Code, Diagnosis, Finding, Fix, FixError, Found, Installed, Section, Severity, Step,
};
use crate::generate::tests::{SWAY_1_10, SWAYFX_0_5, TOUCHPAD};
use crate::generate::{ConfigError, HEADER, Locations, Outcome, Report, Validate};
use crate::include::{IncludeStatus, WriteAccess};
use crate::ipc::Compositor;
use crate::registry::{ConfigFile, Registry};
use crate::state::{Paths, State};
use crate::sway_config::Disk;
use crate::version::SwayVersion;

/// Accepts every file, reports `errors` for each, or can't run at all; and records what it was
/// asked to validate.
#[derive(Default)]
struct Validator {
    errors: Vec<ConfigError>,
    broken: bool,
    asked: RefCell<Vec<PathBuf>>,
}

impl Validate for Validator {
    fn validate(&self, staged: &Path) -> impl Future<Output = io::Result<Vec<ConfigError>>> {
        self.asked.borrow_mut().push(staged.to_owned());
        ready(if self.broken {
            Err(io::Error::new(io::ErrorKind::NotFound, "no sway here"))
        } else {
            Ok(self.errors.clone())
        })
    }
}

const INCLUDE: &str = "include {dir}/config/sway/sway-things/*.conf\n";
const GAPS: &str = "version = 1\n\n[windows.gaps]\ninner = 8\n";
const EFFECTS: &str = "version = 1\n\n[effects]\ncorner_radius = 8\n";

struct Setup {
    dir: tempfile::TempDir,
    state: State,
    locations: Locations,
    status: IncludeStatus,
    found: Found,
    validator: Validator,
}

impl Setup {
    /// `shared` as settings.toml and `config` as the sway config (`{dir}` stands for the
    /// temporary directory), in a session of sway 1.10.
    fn new(shared: &str, config: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("config/sway-things/settings.d")).unwrap();
        fs::write(root.join("config/sway-things/settings.toml"), shared).unwrap();
        let main = root.join("config/sway/config");
        fs::create_dir_all(main.parent().unwrap()).unwrap();
        fs::write(&main, config.replace("{dir}", &root.display().to_string())).unwrap();
        let paths = Paths::new(
            &root.join("config/sway-things"),
            &root.join("state/sway-things"),
            "laptop",
        )
        .unwrap();
        let locations = Locations::new(
            &root.join("config/sway/sway-things"),
            &root.join("state/sway-things"),
        );
        let status = IncludeStatus::detect(Some(&main), &Disk, &locations);
        Self {
            state: State::load(Registry::builtin(), paths),
            locations,
            status,
            found: Found::Session(SWAY_1_10),
            validator: Validator::default(),
            dir,
        }
    }

    fn checkup(&self) -> Checkup<'_, Validator> {
        Checkup {
            found: &self.found,
            state: &self.state,
            locations: &self.locations,
            status: &self.status,
            validator: &self.validator,
        }
    }

    fn diagnose(&self) -> Diagnosis {
        block_on(self.checkup().diagnose())
    }

    fn fix(&self, overwrite: bool) -> Result<Report, FixError> {
        block_on(self.checkup().fix(overwrite, &WriteAccess::for_tests()))
    }

    fn path(&self, file: ConfigFile) -> PathBuf {
        self.locations.path(file)
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("config/sway/config")
    }
}

/// The findings with `code`.
fn with(diagnosis: &Diagnosis, code: Code) -> Vec<&Finding> {
    diagnosis
        .findings()
        .iter()
        .filter(|finding| finding.code == code)
        .collect()
}

/// The one finding with `code`.
fn one(diagnosis: &Diagnosis, code: Code) -> &Finding {
    let found = with(diagnosis, code);
    assert_eq!(found.len(), 1, "{code:?} in {diagnosis:#?}");
    found[0]
}

/// That there is no finding with `code`.
fn none(diagnosis: &Diagnosis, code: Code) {
    assert_eq!(with(diagnosis, code).len(), 0, "{code:?} in {diagnosis:#?}");
}

fn codes(diagnosis: &Diagnosis) -> Vec<Code> {
    diagnosis
        .findings()
        .iter()
        .map(|finding| finding.code)
        .collect()
}

#[test]
fn progress_names_each_step_before_it_starts() {
    let setup = Setup::new(GAPS, INCLUDE);
    setup.fix(false).unwrap();
    let mut steps = Vec::new();
    let diagnosis = block_on(setup.checkup().diagnose_with(|step| steps.push(step)));
    assert_eq!(diagnosis, setup.diagnose());
    assert_eq!(
        steps,
        [
            Step::Section(Section::Compositor),
            Step::Section(Section::Config),
            Step::Section(Section::Settings),
            Step::Section(Section::Generated),
            Step::Validate(ConfigFile::Appearance),
            Step::Section(Section::Conflicts),
        ]
    );
}

#[test]
fn a_setup_that_works_is_fine() {
    let setup = Setup::new(GAPS, INCLUDE);
    setup.fix(false).unwrap();
    let diagnosis = setup.diagnose();
    assert_eq!(diagnosis.severity(), Severity::Ok, "{diagnosis:#?}");
    assert_eq!(
        codes(&diagnosis),
        [
            Code::Session,
            Code::MainConfig,
            Code::IncludeActive,
            Code::SettingsLoaded,
            Code::FileNotNeeded,
            Code::FileCurrent,
            Code::FileValid,
            Code::FileNotNeeded,
            Code::FileNotNeeded,
            Code::NoConflicts,
        ]
    );
    assert!(!diagnosis.can_fix(true));
    assert_eq!(
        one(&diagnosis, Code::Session).message,
        "sway 1.10.1 runs in this session."
    );
    let main = one(&diagnosis, Code::MainConfig);
    assert_eq!(main.file.as_deref(), Some(setup.config().as_path()));
    assert!(main.message.ends_with(" (loaded by the running sway)"));
    // Only the file that exists is validated, also after the fix validated it.
    assert_eq!(
        setup.validator.asked.borrow().last(),
        Some(&setup.path(ConfigFile::Appearance))
    );
    for finding in diagnosis.findings() {
        assert_eq!(finding.section(), finding.code.section());
    }
}

#[test]
fn a_missing_include_line_comes_with_the_instructions() {
    let setup = Setup::new(GAPS, "gaps inner 4\n");
    let diagnosis = setup.diagnose();
    assert_eq!(diagnosis.severity(), Severity::Problem);
    let include = one(&diagnosis, Code::IncludeMissing);
    assert_eq!(include.severity, Severity::Problem);
    let hint = include.hint.as_deref().unwrap();
    assert!(
        hint.ends_with(&format!(
            "\n\n    include {}/*.conf",
            setup.locations.directory().display()
        )),
        "{hint}"
    );
    assert_eq!(include.file.as_deref(), Some(setup.config().as_path()));
    // Nothing generated is in effect, so nothing conflicts with it.
    none(&diagnosis, Code::NoConflicts);
    none(&diagnosis, Code::ConfigWins);
    // The file is still missing, and fixing would write it once the line is there.
    assert_eq!(
        one(&diagnosis, Code::FileMissing).fix,
        Some(Fix::Regenerate)
    );

    let setup = Setup::new(
        GAPS,
        "include {dir}/config/sway/sway-things/10-input.conf\n",
    );
    let diagnosis = setup.diagnose();
    assert_eq!(
        one(&diagnosis, Code::IncludePartial).severity,
        Severity::Problem
    );
    let setup = Setup::new(GAPS, "include $(pwd)/*.conf\n");
    let diagnosis = setup.diagnose();
    one(&diagnosis, Code::IncludeUnknown);
    let unresolved = one(&diagnosis, Code::ConfigDiagnostic);
    assert_eq!(unresolved.severity, Severity::Warning);
    assert_eq!(unresolved.line, Some(1));
    assert!(
        unresolved
            .message
            .contains(":1: include $(pwd)/*.conf can't be followed: "),
        "{}",
        unresolved.message
    );
}

#[test]
fn lines_after_the_include_and_conflicts_are_listed() {
    let setup = Setup::new(
        GAPS,
        &format!("gaps inner 2\n{INCLUDE}gaps inner 4\nfocus_follows_mouse no\n"),
    );
    setup.fix(false).unwrap();
    let diagnosis = setup.diagnose();
    // Warnings only: the settings still work where the user's lines don't touch them.
    assert_eq!(diagnosis.severity(), Severity::Warning, "{diagnosis:#?}");
    let config = setup.config().display().to_string();
    let after = one(&diagnosis, Code::IncludeNotLast);
    assert!(after.message.contains(" 2 of your config lines "));
    assert_eq!(
        after.details,
        [
            format!("{config}:3: gaps inner 4"),
            format!("{config}:4: focus_follows_mouse no")
        ]
    );
    assert_eq!(after.line, Some(2));
    assert!(after.hint.as_deref().unwrap().starts_with("Move `include "));

    let wins = one(&diagnosis, Code::ConfigWins);
    assert_eq!(wins.severity, Severity::Warning);
    assert_eq!(wins.key, Some("windows.gaps.inner"));
    assert_eq!(
        (wins.file.as_deref(), wins.line),
        (Some(setup.config().as_path()), Some(3))
    );
    assert_eq!(
        wins.message,
        format!(
            "windows.gaps.inner: your config wins: {config}:3 comes after the generated file \
             (gaps inner 4)."
        )
    );
    let dead = one(&diagnosis, Code::ConfigLineDead);
    assert_eq!(dead.severity, Severity::Ok);
    assert_eq!(dead.line, Some(1));
    none(&diagnosis, Code::NoConflicts);
}

#[test]
fn many_lines_after_the_include_are_cut_short() {
    let lines: String = (1..=12).fold(String::new(), |lines, gap| {
        lines + &format!("gaps outer {gap}\n")
    });
    let setup = Setup::new(GAPS, &format!("{INCLUDE}{lines}"));
    let diagnosis = setup.diagnose();
    let after = one(&diagnosis, Code::IncludeNotLast);
    assert_eq!(after.details.len(), 11);
    assert_eq!(after.details[10], "… and 2 more");
}

#[test]
fn per_device_conflicts_name_the_device() {
    let setup = Setup::new(
        &format!("version = 1\n\n[device.\"{TOUCHPAD}\".input.touchpad]\ntap = true\n"),
        &format!("{INCLUDE}input \"{TOUCHPAD}\" tap disabled\n"),
    );
    let diagnosis = setup.diagnose();
    let wins = one(&diagnosis, Code::ConfigWins);
    assert_eq!(wins.device.as_deref(), Some(TOUCHPAD));
    assert!(
        wins.message.starts_with(&format!(
            "input.touchpad.tap (device {TOUCHPAD}): your config wins: "
        )),
        "{}",
        wins.message
    );
}

#[test]
fn problems_of_the_settings_files_are_reported() {
    let setup = Setup::new("version = 1\n[windows.gaps\n", INCLUDE);
    let diagnosis = setup.diagnose();
    let unusable = one(&diagnosis, Code::SettingsUnusable);
    assert_eq!(unusable.severity, Severity::Problem);
    assert!(
        unusable.message.contains("not valid TOML"),
        "{}",
        unusable.message
    );
    none(&diagnosis, Code::SettingsLoaded);

    let setup = Setup::new(
        "version = 1\n\n[windows.gaps]\ninner = \"wide\"\nwide = 3\n",
        INCLUDE,
    );
    let diagnosis = setup.diagnose();
    let entries = with(&diagnosis, Code::SettingsEntry);
    assert_eq!(entries.len(), 2, "{diagnosis:#?}");
    assert!(
        entries
            .iter()
            .all(|entry| entry.severity == Severity::Warning)
    );
    assert!(
        entries[0]
            .file
            .as_deref()
            .unwrap()
            .ends_with("settings.toml")
    );
    assert_eq!(diagnosis.severity(), Severity::Warning);
}

#[test]
fn files_that_are_missing_or_out_of_date_are_fixed() {
    let setup = Setup::new(GAPS, INCLUDE);
    let diagnosis = setup.diagnose();
    let missing = one(&diagnosis, Code::FileMissing);
    assert_eq!(missing.severity, Severity::Problem);
    assert_eq!(missing.fix, Some(Fix::Regenerate));
    assert_eq!(
        missing.file.as_deref(),
        Some(setup.path(ConfigFile::Appearance).as_path())
    );
    assert!(diagnosis.can_fix(false));

    let report = setup.fix(false).unwrap();
    assert!(report.contains(&(ConfigFile::Appearance, Outcome::Written)));
    assert!(report.contains(&(ConfigFile::Input, Outcome::NotCreated)));
    assert_eq!(setup.diagnose().severity(), Severity::Ok);

    // Changed in the settings file while Sway Things didn't run.
    fs::write(
        setup.dir.path().join("config/sway-things/settings.toml"),
        "version = 1\n\n[windows.gaps]\ninner = 9\n",
    )
    .unwrap();
    let setup = Setup {
        state: State::load(Registry::builtin(), setup.state.paths().clone()),
        ..setup
    };
    let diagnosis = setup.diagnose();
    let stale = one(&diagnosis, Code::FileOutOfDate);
    assert_eq!(stale.fix, Some(Fix::Regenerate));
    assert!(
        stale
            .message
            .ends_with("from the stored settings for sway 1.10.1."),
        "{}",
        stale.message
    );
    setup.fix(false).unwrap();
    assert!(
        fs::read_to_string(setup.path(ConfigFile::Appearance))
            .unwrap()
            .ends_with("\ngaps inner 9\n")
    );
    assert_eq!(setup.diagnose().severity(), Severity::Ok);
}

#[test]
fn switching_from_swayfx_to_sway_is_detected_and_fixed() {
    let mut setup = Setup::new(EFFECTS, INCLUDE);
    setup.found = Found::Session(SWAYFX_0_5);
    setup.fix(false).unwrap();
    let effects = setup.path(ConfigFile::Effects);
    assert!(
        fs::read_to_string(&effects)
            .unwrap()
            .ends_with("\ncorner_radius 8\n")
    );
    assert_eq!(setup.diagnose().severity(), Severity::Ok);

    // sway is back: the file still holds the SwayFX directive, which sway rejects.
    setup.found = Found::NoSession {
        reason: "SWAYSOCK isn't set".to_owned(),
        installed: Installed::Version(SWAY_1_10),
    };
    setup.validator.errors = vec![ConfigError {
        line: 3,
        message: "Unknown/invalid command 'corner_radius'".to_owned(),
    }];
    let diagnosis = setup.diagnose();
    assert_eq!(
        one(&diagnosis, Code::NoSession).message,
        "No sway session runs (SWAYSOCK isn't set). The installed compositor is sway 1.10.1."
    );
    assert_eq!(
        one(&diagnosis, Code::FileOutOfDate).fix,
        Some(Fix::Regenerate)
    );
    let invalid = one(&diagnosis, Code::FileInvalid);
    assert_eq!(invalid.severity, Severity::Problem);
    assert_eq!(invalid.fix, Some(Fix::Regenerate));
    // The file isn't what would be generated now, so its lines can't be traced to settings.
    assert_eq!(
        invalid.details,
        ["line 3: Unknown/invalid command 'corner_radius'"]
    );
    let skipped = one(&diagnosis, Code::SettingSkipped);
    assert_eq!(skipped.key, Some("effects.corner_radius"));
    assert_eq!(
        skipped.message,
        "effects.corner_radius has no effect: requires SwayFX 0.5 or newer."
    );

    setup.validator.errors.clear();
    let report = setup.fix(false).unwrap();
    assert!(report.contains(&(ConfigFile::Effects, Outcome::Written)));
    assert_eq!(
        fs::read_to_string(&effects).unwrap(),
        format!("{HEADER}# effects.corner_radius: requires SwayFX 0.5 or newer\n")
    );
    let diagnosis = setup.diagnose();
    assert_eq!(diagnosis.severity(), Severity::Warning, "{diagnosis:#?}");
    one(&diagnosis, Code::FileCurrent);
}

#[test]
fn a_rejected_file_names_the_settings_behind_its_lines() {
    let mut setup = Setup::new(GAPS, INCLUDE);
    setup.fix(false).unwrap();
    setup.validator.errors = vec![ConfigError {
        line: 3,
        message: "no".to_owned(),
    }];
    let diagnosis = setup.diagnose();
    let invalid = one(&diagnosis, Code::FileInvalid);
    assert_eq!(invalid.details, ["line 3: no (from windows.gaps.inner)"]);
    assert_eq!(invalid.fix, None);
    assert!(
        invalid
            .hint
            .as_deref()
            .unwrap()
            .contains("report this as a bug")
    );
}

#[test]
fn files_changed_outside_are_only_overwritten_when_asked() {
    let setup = Setup::new(GAPS, INCLUDE);
    setup.fix(false).unwrap();
    let appearance = setup.path(ConfigFile::Appearance);
    fs::write(&appearance, format!("{HEADER}gaps inner 1\n")).unwrap();
    let diagnosis = setup.diagnose();
    let edited = one(&diagnosis, Code::FileEdited);
    assert_eq!(edited.fix, Some(Fix::Overwrite));
    assert!(!diagnosis.can_fix(false));
    assert!(diagnosis.can_fix(true));

    let report = setup.fix(false).unwrap();
    assert!(report.contains(&(ConfigFile::Appearance, Outcome::EditedOutside)));
    assert!(
        fs::read_to_string(&appearance)
            .unwrap()
            .ends_with("gaps inner 1\n")
    );
    let report = setup.fix(true).unwrap();
    assert!(report.contains(&(ConfigFile::Appearance, Outcome::Written)));
    assert_eq!(setup.diagnose().severity(), Severity::Ok);
}

#[test]
fn files_sway_things_didnt_make_are_never_touched() {
    let setup = Setup::new(GAPS, INCLUDE);
    let behavior = setup.path(ConfigFile::Behavior);
    fs::create_dir_all(behavior.parent().unwrap()).unwrap();
    fs::write(&behavior, "focus_follows_mouse no\n").unwrap();
    let diagnosis = setup.diagnose();
    let foreign = one(&diagnosis, Code::FileForeign);
    assert_eq!(foreign.severity, Severity::Problem);
    assert_eq!(foreign.fix, None);
    // Not ours, so not validated either.
    assert!(!setup.validator.asked.borrow().contains(&behavior));

    let report = setup.fix(true).unwrap();
    assert!(report.contains(&(ConfigFile::Behavior, Outcome::Foreign)));
    assert_eq!(
        fs::read_to_string(&behavior).unwrap(),
        "focus_follows_mouse no\n"
    );
}

#[test]
fn without_sway_validation_is_skipped_not_failed() {
    let mut setup = Setup::new(GAPS, INCLUDE);
    setup.fix(false).unwrap();
    setup.found = Found::NoSession {
        reason: "SWAYSOCK isn't set".to_owned(),
        installed: Installed::Missing("No such file or directory (os error 2)".to_owned()),
    };
    setup.validator.asked.borrow_mut().clear();
    let diagnosis = setup.diagnose();
    let missing = one(&diagnosis, Code::SwayMissing);
    assert_eq!(missing.severity, Severity::Problem);
    let skipped = one(&diagnosis, Code::ValidationSkipped);
    assert_eq!(skipped.severity, Severity::Warning);
    none(&diagnosis, Code::FileInvalid);
    assert_eq!(*setup.validator.asked.borrow(), [] as [PathBuf; 0]);
    // Without a compositor, a file of ours can't be compared.
    assert_eq!(
        one(&diagnosis, Code::FileUnchecked).severity,
        Severity::Warning
    );
    assert!(matches!(setup.fix(false), Err(FixError::NoCompositor)));

    // In a session sway is tried, and a failure to run it is a skip as well.
    setup.found = Found::Session(SWAY_1_10);
    setup.validator.broken = true;
    let diagnosis = setup.diagnose();
    let skipped = one(&diagnosis, Code::ValidationSkipped);
    assert!(
        skipped.message.contains("(no sway here)"),
        "{}",
        skipped.message
    );
    assert_eq!(diagnosis.severity(), Severity::Warning);
}

#[test]
fn compositors_sway_things_cant_use_are_problems() {
    let mut setup = Setup::new(GAPS, INCLUDE);
    setup.found = Found::Unusable {
        error: "sway 1.9 is not supported: Sway Things requires sway 1.10 or newer".to_owned(),
        installed: Installed::Unknown,
    };
    let diagnosis = setup.diagnose();
    assert_eq!(
        one(&diagnosis, Code::SessionUnusable).severity,
        Severity::Problem
    );
    assert_eq!(one(&diagnosis, Code::FileMissing).fix, None);

    setup.found = Found::NoSession {
        reason: "SWAYSOCK isn't set".to_owned(),
        installed: Installed::Unknown,
    };
    let diagnosis = setup.diagnose();
    assert_eq!(
        one(&diagnosis, Code::VersionUnknown).severity,
        Severity::Warning
    );

    let old = Compositor::Sway {
        version: SwayVersion::new(1, 9, 0),
    };
    setup.found = Found::NoSession {
        reason: "SWAYSOCK isn't set".to_owned(),
        installed: Installed::Version(old),
    };
    let diagnosis = setup.diagnose();
    one(&diagnosis, Code::NoSession);
    assert_eq!(
        one(&diagnosis, Code::InstalledUnsupported).message,
        "The installed sway 1.9 is not supported: Sway Things requires sway 1.10 or newer."
    );
}

#[test]
fn no_config_and_unreadable_files_are_problems() {
    let mut setup = Setup::new(GAPS, INCLUDE);
    setup.status = IncludeStatus::detect(None, &Disk, &setup.locations);
    let diagnosis = setup.diagnose();
    one(&diagnosis, Code::NoConfig);
    one(&diagnosis, Code::IncludeMissing);

    let main = setup.dir.path().join("config/sway/gone");
    setup.status = IncludeStatus::detect(Some(&main), &Disk, &setup.locations);
    let diagnosis = setup.diagnose();
    let unreadable = one(&diagnosis, Code::ConfigDiagnostic);
    assert_eq!(unreadable.severity, Severity::Problem);

    // A directory in the place of a generated file.
    fs::create_dir_all(setup.path(ConfigFile::Input)).unwrap();
    let diagnosis = setup.diagnose();
    one(&diagnosis, Code::FileUnreadable);
}

#[test]
fn ids_are_stable() {
    assert_eq!(Code::FileOutOfDate.id(), "file_out_of_date");
    assert_eq!(Code::ConfigWins.section().id(), "conflicts");
    assert_eq!(Severity::Problem.id(), "problem");
    assert_eq!(Fix::Overwrite.id(), "overwrite");
    assert!(Severity::Ok < Severity::Warning && Severity::Warning < Severity::Problem);
}
