// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Following the user's sway config: the include status, kept current while files change.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::IncludeStatus;
use crate::generate::Locations;
use crate::sway_config::{Disk, default_path, default_paths};
use crate::watch::{self, WatchError, Watcher};

/// Which file is the main sway config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MainConfig {
    /// This file: the one the running sway loaded, as `GET_VERSION` names it.
    Path(PathBuf),
    /// The one sway finds without a session
    /// ([`default_path`](crate::sway_config::default_path)), looked up again whenever a file
    /// changes, so a config the user creates is found.
    Default,
}

impl MainConfig {
    fn path(&self) -> Option<PathBuf> {
        match self {
            Self::Path(path) => Some(path.clone()),
            Self::Default => default_path(&Disk),
        }
    }
}

/// The files and the directories to watch.
type Targets = (BTreeSet<PathBuf>, BTreeSet<PathBuf>);

/// The [`IncludeStatus`] of the user's sway config, read again whenever the config changes: the
/// main config, every file it includes, and the directories its includes point into, so that a
/// file a glob newly matches is noticed too.
///
/// It reports only when the status differs from the last one, which also covers every change to
/// the user's directives. The writes of Sway Things to its own files change nothing, so they are
/// not reported.
#[derive(Debug)]
pub struct IncludeWatch {
    main: MainConfig,
    locations: Locations,
    debounce: Duration,
    status: IncludeStatus,
    watcher: Watcher,
    targets: Targets,
}

impl IncludeWatch {
    /// Reads the config and starts watching it.
    ///
    /// # Errors
    ///
    /// [`WatchError`] if the system refuses to watch.
    pub fn new(main: MainConfig, locations: Locations) -> Result<Self, WatchError> {
        Self::with_debounce(main, locations, watch::DEBOUNCE)
    }

    fn with_debounce(
        main: MainConfig,
        locations: Locations,
        debounce: Duration,
    ) -> Result<Self, WatchError> {
        let status = IncludeStatus::detect(main.path().as_deref(), &Disk, &locations);
        let targets = targets(&main, &status);
        let watcher = Watcher::with_directories(targets.0.clone(), targets.1.clone(), debounce)?;
        let mut watch = Self {
            main,
            locations,
            debounce,
            status,
            watcher,
            targets,
        };
        // What changed between reading and watching.
        watch.refresh();
        Ok(watch)
    }

    /// The status as of the last report.
    pub fn status(&self) -> &IncludeStatus {
        &self.status
    }

    /// Waits until the status changed and returns the new one. `None` if watching stopped
    /// unexpectedly.
    pub async fn changed(&mut self) -> Option<&IncludeStatus> {
        loop {
            self.watcher.changed().await?;
            if self.refresh() {
                return Some(&self.status);
            }
        }
    }

    /// Like [`IncludeWatch::changed`], but blocks the thread.
    pub fn changed_blocking(&mut self) -> Option<&IncludeStatus> {
        loop {
            self.watcher.changed_blocking()?;
            if self.refresh() {
                return Some(&self.status);
            }
        }
    }

    /// The new status if files changed since the last call and the status with them.
    pub fn try_changed(&mut self) -> Option<&IncludeStatus> {
        let mut reported = false;
        while self.watcher.try_changed().is_some() {
            reported = true;
        }
        (reported && self.refresh()).then_some(&self.status)
    }

    /// Reads the config again and watches the files it consists of now. Returns whether the
    /// status changed.
    fn refresh(&mut self) -> bool {
        let mut status = self.detect();
        let targets = targets(&self.main, &status);
        if targets != self.targets {
            // If the system refuses, the old watches stay; the next report tries again.
            if let Ok(watcher) =
                Watcher::with_directories(targets.0.clone(), targets.1.clone(), self.debounce)
            {
                self.watcher = watcher;
                self.targets = targets;
                // What changed in the new files before they were watched.
                status = self.detect();
            }
        }
        let changed = status != self.status;
        self.status = status;
        changed
    }

    fn detect(&self) -> IncludeStatus {
        IncludeStatus::detect(self.main.path().as_deref(), &Disk, &self.locations)
    }
}

/// What to watch for `status`: the files the config consists of or could consist of, and the
/// directories its includes point into.
fn targets(main: &MainConfig, status: &IncludeStatus) -> Targets {
    let mut files: BTreeSet<PathBuf> = match main {
        MainConfig::Path(path) => BTreeSet::from([path.clone()]),
        MainConfig::Default => default_paths(&Disk).into_iter().collect(),
    };
    files.insert(status.instructions().config().to_owned());
    let config = status.config();
    files.extend(config.files.iter().map(|file| file.to_path_buf()));
    let mut directories = BTreeSet::new();
    for path in config.includes.iter().flat_map(|include| &include.paths) {
        // Also a path that doesn't exist: the file may appear.
        files.insert(path.clone());
        // Where a glob in the last part of the path finds its files.
        directories.extend(path.parent().map(Path::to_owned));
    }
    (files, directories)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{IncludeWatch, MainConfig};
    use crate::generate::Locations;
    use crate::include::Inclusion;

    const DEBOUNCE: Duration = Duration::from_millis(100);

    /// A sway config directory with a watch on its `config`.
    struct Fixture {
        dir: tempfile::TempDir,
        watch: IncludeWatch,
    }

    impl Fixture {
        fn new(main: &str, prepare: impl FnOnce(&Path)) -> Self {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("config"), main).unwrap();
            prepare(dir.path());
            let locations =
                Locations::new(&dir.path().join("sway-things"), &dir.path().join("state"));
            let main = MainConfig::Path(dir.path().join("config"));
            let watch = IncludeWatch::with_debounce(main, locations, DEBOUNCE).unwrap();
            Self { dir, watch }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        /// The include line for the generated files.
        fn line(&self) -> String {
            self.watch.status().instructions().line().to_owned()
        }

        /// The next change of the status, which must come.
        fn changed(&mut self) -> Inclusion {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = self.watch.try_changed() {
                    return status.inclusion().clone();
                }
                assert!(Instant::now() < deadline, "no change");
                thread::sleep(Duration::from_millis(5));
            }
        }

        /// Asserts that no change is reported.
        fn quiet(&mut self) {
            thread::sleep(DEBOUNCE * 4);
            assert!(self.watch.try_changed().is_none());
        }
    }

    #[test]
    fn the_status_follows_the_line_in_the_main_config() {
        let mut fixture = Fixture::new("gaps inner 8\n", |_| {});
        assert_eq!(fixture.watch.status().inclusion(), &Inclusion::Missing);
        let line = fixture.line();
        fixture.quiet();

        fs::write(fixture.path("config"), format!("gaps inner 8\n{line}\n")).unwrap();
        assert_eq!(fixture.changed(), Inclusion::Active);
        assert!(fixture.watch.status().is_last());

        // A line of the user after it is a change too: it wins from now on.
        fs::write(fixture.path("config"), format!("{line}\ngaps inner 8\n")).unwrap();
        assert_eq!(fixture.changed(), Inclusion::Active);
        assert_eq!(fixture.watch.status().after().len(), 1);

        // Saved again without a change, as editors do.
        fs::write(fixture.path("config"), format!("{line}\ngaps inner 8\n")).unwrap();
        fixture.quiet();

        // Replaced by a rename, without the line.
        fs::write(fixture.path("config.tmp"), "gaps inner 8\n").unwrap();
        fs::rename(fixture.path("config.tmp"), fixture.path("config")).unwrap();
        assert_eq!(fixture.changed(), Inclusion::Missing);
        fixture.quiet();
    }

    #[test]
    fn the_status_follows_included_files() {
        let mut fixture = Fixture::new("include config.d/*.conf\ninclude extra\n", |_| {});
        let line = fixture.line();
        fixture.quiet();

        // A file the glob newly matches, in a directory that didn't exist.
        fs::create_dir(fixture.path("config.d")).unwrap();
        fs::write(fixture.path("config.d/50-things.conf"), format!("{line}\n")).unwrap();
        assert_eq!(fixture.changed(), Inclusion::Active);
        // The new file is watched now.
        fs::write(fixture.path("config.d/50-things.conf"), "# gone\n").unwrap();
        assert_eq!(fixture.changed(), Inclusion::Missing);

        // An included file that didn't exist, and one more level of includes.
        fs::write(fixture.path("extra"), "include deeper\n").unwrap();
        fs::write(fixture.path("deeper"), format!("{line}\n")).unwrap();
        assert_eq!(fixture.changed(), Inclusion::Active);
        fs::remove_file(fixture.path("deeper")).unwrap();
        assert_eq!(fixture.changed(), Inclusion::Missing);
        fixture.quiet();
    }

    #[test]
    fn writing_the_generated_files_is_no_change() {
        let mut fixture = Fixture::new("", |_| {});
        let line = fixture.line();
        fs::write(fixture.path("config"), format!("{line}\n")).unwrap();
        assert_eq!(fixture.changed(), Inclusion::Active);

        fs::create_dir(fixture.path("sway-things")).unwrap();
        fs::write(
            fixture.path("sway-things/10-input.conf"),
            "# Generated\ninput type:touchpad tap enabled\n",
        )
        .unwrap();
        fixture.quiet();
        assert!(fixture.watch.status().is_last());

        // A file of the user in that directory is part of their config.
        fs::write(fixture.path("sway-things/99-mine.conf"), "gaps inner 8\n").unwrap();
        assert_eq!(fixture.changed(), Inclusion::Active);
        assert_eq!(fixture.watch.status().after().len(), 1);
    }
}
