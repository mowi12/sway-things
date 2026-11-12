// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Watching files for changes made outside Sway Things.
//!
//! A [`Watcher`] watches a set of files, whether they exist yet or not, and reports which of them
//! may have changed: written in place, replaced by a rename (how editors and `git` save), created
//! or deleted, also along with their directory. It can also watch directories for files that
//! appear in them, change or go away. Bursts of events become one report once the files have been
//! quiet for the debounce time.
//!
//! The watcher only says that a file may have changed. Whoever reads the file decides whether it
//! did, so a report after one's own write is harmless.
//!
//! It runs on a thread of its own and hands over reports through a channel, so it needs no
//! particular executor: await [`Watcher::changed`] on any of them (for example the glib main
//! loop), or block on [`Watcher::changed_blocking`].
//!
//! ```no_run
//! use sway_things_core::watch::{DEBOUNCE, Watcher};
//!
//! let watcher = Watcher::new(["/home/me/.config/sway/config".into()], DEBOUNCE)?;
//! while let Some(files) = watcher.changed_blocking() {
//!     println!("changed: {files:?}");
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

/// A debounce time that fits files people edit: long enough for an editor or `git` to finish a
/// save, short enough to feel instant.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// A burst that never pauses is reported after this many debounce times at the latest.
const MAX_DEBOUNCES: u32 = 10;

enum Message {
    Event(notify::Result<notify::Event>),
    Stop,
}

/// Watches a set of files. Dropping it stops watching.
#[derive(Debug)]
pub struct Watcher {
    reports: async_channel::Receiver<Vec<PathBuf>>,
    stop: mpsc::Sender<Message>,
}

impl Watcher {
    /// Starts watching `files`. Changes are reported once no watched file has had an event for
    /// `debounce`.
    ///
    /// A file doesn't have to exist: its directory is watched, or the nearest directory above it
    /// that exists. For a file that is a symbolic link, the file it points to is watched too.
    ///
    /// # Errors
    ///
    /// [`WatchError`] if the system refuses to watch, e.g. because the user has no inotify
    /// watches left.
    pub fn new(
        files: impl IntoIterator<Item = PathBuf>,
        debounce: Duration,
    ) -> Result<Self, WatchError> {
        Self::with_directories(files, [], debounce)
    }

    /// Like [`Watcher::new`], and also watches `directories`: one of them is reported when a
    /// file directly in it is created, changed, renamed or deleted, or the directory itself is.
    /// That is how to follow a glob such as `config.d/*`, whose files aren't known in advance.
    ///
    /// # Errors
    ///
    /// [`WatchError`] if the system refuses to watch.
    pub fn with_directories(
        files: impl IntoIterator<Item = PathBuf>,
        directories: impl IntoIterator<Item = PathBuf>,
        debounce: Duration,
    ) -> Result<Self, WatchError> {
        let (sender, receiver) = mpsc::channel();
        let events = sender.clone();
        let notifier = notify::recommended_watcher(move |event| {
            // The receiver is gone once the watcher is dropped; nobody is listening then.
            let _ = events.send(Message::Event(event));
        })
        .map_err(WatchError)?;
        let mut watches = Watches {
            watcher: notifier,
            files: files
                .into_iter()
                .map(|path| Watched::new(path, false))
                .chain(directories.into_iter().map(|path| Watched::new(path, true)))
                .collect(),
            directories: BTreeSet::new(),
        };
        watches.sync().map_err(WatchError)?;

        let (reports, reported) = async_channel::unbounded();
        thread::Builder::new()
            .name("sway-things-watch".to_owned())
            .spawn(move || run(watches, debounce, &receiver, &reports))
            .map_err(|error| WatchError(notify::Error::io(error)))?;
        Ok(Self {
            reports: reported,
            stop: sender,
        })
    }

    /// Waits for the next report: the watched files and directories that may have changed, as the
    /// paths given to [`Watcher::new`], sorted. `None` if watching stopped unexpectedly.
    pub async fn changed(&self) -> Option<Vec<PathBuf>> {
        self.reports.recv().await.ok()
    }

    /// Like [`Watcher::changed`], but blocks the thread.
    pub fn changed_blocking(&self) -> Option<Vec<PathBuf>> {
        self.reports.recv_blocking().ok()
    }

    /// The next report if there is one already.
    pub fn try_changed(&self) -> Option<Vec<PathBuf>> {
        self.reports.try_recv().ok()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // The thread may be gone already.
        let _ = self.stop.send(Message::Stop);
    }
}

/// A watched file or directory.
struct Watched {
    /// The path as given.
    path: PathBuf,
    /// The file the path leads to through symbolic links, while it exists.
    target: Option<PathBuf>,
    /// Whether it is a directory, watched for the files in it.
    directory: bool,
}

impl Watched {
    fn new(path: PathBuf, directory: bool) -> Self {
        Self {
            path,
            target: None,
            directory,
        }
    }

    /// Whether an event at `event_path` can mean that the file changed: the event is about the
    /// file itself, the file it links to, or a directory above it that appeared or went away.
    /// For a directory, also an event about a file directly in it.
    fn is_affected_by(&self, event_path: &Path) -> bool {
        let inside = |directory: &Path| self.directory && event_path.parent() == Some(directory);
        self.path.starts_with(event_path)
            || inside(&self.path)
            || self
                .target
                .as_deref()
                .is_some_and(|target| target == event_path || inside(target))
    }
}

/// The directories the system watches for the files.
struct Watches {
    watcher: RecommendedWatcher,
    files: Vec<Watched>,
    directories: BTreeSet<PathBuf>,
}

impl Watches {
    /// Watches the directories the files are in right now. Directories are watched, not files:
    /// a watch on a file is lost when the file is replaced, and can't see it being created.
    fn sync(&mut self) -> notify::Result<()> {
        let mut wanted = BTreeSet::new();
        for file in &mut self.files {
            file.target = file
                .path
                .canonicalize()
                .ok()
                .filter(|target| *target != file.path);
            let paths = [Some(&file.path), file.target.as_ref()];
            for path in paths.into_iter().flatten() {
                // A file is seen from its directory, a directory from itself.
                let Some(start) = (if file.directory {
                    Some(path.as_path())
                } else {
                    path.parent()
                }) else {
                    continue;
                };
                // The nearest directory that exists sees the missing ones being created.
                if let Some(directory) = start.ancestors().find(|directory| directory.is_dir()) {
                    wanted.insert(directory.to_owned());
                }
            }
        }
        for directory in self.directories.difference(&wanted) {
            // A directory that was deleted is no longer watched anyway.
            let _ = self.watcher.unwatch(directory);
        }
        let mut result = Ok(());
        let mut watched = BTreeSet::new();
        for directory in wanted {
            if self.directories.contains(&directory) {
                watched.insert(directory);
                continue;
            }
            match self.watcher.watch(&directory, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    watched.insert(directory);
                }
                Err(error) => result = Err(error),
            }
        }
        self.directories = watched;
        result
    }

    /// Adds the files `message` is about to `changed`.
    fn collect(&self, message: notify::Result<notify::Event>, changed: &mut BTreeSet<PathBuf>) {
        match message {
            // Reading a file doesn't change it.
            Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
            Ok(event) if !event.need_rescan() => {
                for file in &self.files {
                    if event.paths.iter().any(|path| file.is_affected_by(path)) {
                        changed.insert(file.path.clone());
                    }
                }
            }
            // Events were lost: any file may have changed.
            Ok(_) | Err(_) => changed.extend(self.files.iter().map(|file| file.path.clone())),
        }
    }
}

/// The watcher thread: collects events until the files are quiet, then reports.
fn run(
    mut watches: Watches,
    debounce: Duration,
    messages: &mpsc::Receiver<Message>,
    reports: &async_channel::Sender<Vec<PathBuf>>,
) {
    let mut changed = BTreeSet::new();
    while let Ok(Message::Event(event)) = messages.recv() {
        watches.collect(event, &mut changed);
        if changed.is_empty() {
            continue;
        }
        let started = Instant::now();
        while started.elapsed() < debounce * MAX_DEBOUNCES {
            match messages.recv_timeout(debounce) {
                Ok(Message::Event(event)) => watches.collect(event, &mut changed),
                Ok(Message::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
            }
        }
        // Before the report: a file created in a directory that is only watched from now on
        // causes no event, but whoever gets the report reads the files after this point.
        // A directory that can't be watched now is tried again at the next report.
        let _ = watches.sync();
        let report = std::mem::take(&mut changed).into_iter().collect();
        if reports.send_blocking(report).is_err() {
            return;
        }
    }
}

/// The system refuses to watch files.
#[derive(Debug)]
pub struct WatchError(notify::Error);

impl fmt::Display for WatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "can't watch files: {}", self.0)
    }
}

impl std::error::Error for WatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::Watcher;

    const DEBOUNCE: Duration = Duration::from_millis(100);

    /// The pause between the writes of a burst.
    const BURST_PAUSE: Duration = Duration::from_millis(10);

    /// A debounce far longer than [`BURST_PAUSE`], so a busy machine can't split a burst.
    const BURST_DEBOUNCE: Duration = Duration::from_millis(500);

    /// A directory with a watcher on `settings.toml` and `settings.d/laptop.toml` in it.
    struct Fixture {
        dir: tempfile::TempDir,
        watcher: Watcher,
        debounce: Duration,
    }

    impl Fixture {
        fn new(prepare: impl FnOnce(&Path)) -> Self {
            Self::with_debounce(DEBOUNCE, prepare)
        }

        fn with_debounce(debounce: Duration, prepare: impl FnOnce(&Path)) -> Self {
            let dir = tempfile::tempdir().unwrap();
            prepare(dir.path());
            let watcher = Watcher::new(
                [
                    dir.path().join("settings.toml"),
                    dir.path().join("settings.d/laptop.toml"),
                ],
                debounce,
            )
            .unwrap();
            Self {
                dir,
                watcher,
                debounce,
            }
        }

        fn shared(&self) -> PathBuf {
            self.dir.path().join("settings.toml")
        }

        fn host(&self) -> PathBuf {
            self.dir.path().join("settings.d/laptop.toml")
        }

        /// The next report, which must come.
        fn report(&self) -> Vec<PathBuf> {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(report) = self.watcher.try_changed() {
                    return report;
                }
                assert!(Instant::now() < deadline, "no report");
                thread::sleep(Duration::from_millis(5));
            }
        }

        /// Asserts that nothing more is reported.
        fn quiet(&self) {
            thread::sleep(self.debounce * 4);
            assert_eq!(self.watcher.try_changed(), None);
        }
    }

    #[test]
    fn a_burst_of_writes_is_one_report() {
        let fixture = Fixture::with_debounce(BURST_DEBOUNCE, |dir| {
            fs::write(dir.join("settings.toml"), "a = 0\n").unwrap();
        });
        fixture.quiet();
        for round in 1..=5 {
            fs::write(fixture.shared(), format!("a = {round}\n")).unwrap();
            thread::sleep(BURST_PAUSE);
        }
        assert_eq!(fixture.report(), [fixture.shared()]);
        // By the time of the report the files are quiet, so it is safe to read them.
        assert_eq!(fs::read_to_string(fixture.shared()).unwrap(), "a = 5\n");
        fixture.quiet();

        // Writes that are further apart are reported one by one.
        fs::write(fixture.shared(), "a = 6\n").unwrap();
        assert_eq!(fixture.report(), [fixture.shared()]);
        fs::write(fixture.shared(), "a = 7\n").unwrap();
        assert_eq!(fixture.report(), [fixture.shared()]);
        fixture.quiet();
    }

    #[test]
    fn a_file_replaced_by_a_rename_is_reported() {
        let fixture = Fixture::new(|dir| fs::write(dir.join("settings.toml"), "a = 0\n").unwrap());
        // How editors and git save: write a new file, then rename it over the old one.
        for round in 1..=2 {
            let temporary = fixture.dir.path().join(".settings.toml.tmp");
            fs::write(&temporary, format!("a = {round}\n")).unwrap();
            fs::rename(&temporary, fixture.shared()).unwrap();
            assert_eq!(fixture.report(), [fixture.shared()]);
            fixture.quiet();
        }
        // And how some editors keep a backup: move the old file away first.
        fs::rename(fixture.shared(), fixture.dir.path().join("settings.toml~")).unwrap();
        fs::write(fixture.shared(), "a = 3\n").unwrap();
        assert_eq!(fixture.report(), [fixture.shared()]);
        fixture.quiet();
    }

    #[test]
    fn creating_and_deleting_a_file_is_reported() {
        let fixture = Fixture::new(|_| {});
        fs::write(fixture.shared(), "a = 1\n").unwrap();
        assert_eq!(fixture.report(), [fixture.shared()]);
        fs::remove_file(fixture.shared()).unwrap();
        assert_eq!(fixture.report(), [fixture.shared()]);
        fixture.quiet();
    }

    #[test]
    fn a_file_in_a_directory_that_appears_later_is_reported() {
        let fixture = Fixture::new(|_| {});
        fs::create_dir(fixture.dir.path().join("settings.d")).unwrap();
        fs::write(fixture.host(), "a = 1\n").unwrap();
        assert_eq!(fixture.report(), [fixture.host()]);
        // Whether the file was created before or after its directory got watched, the report
        // came after it.
        assert_eq!(fs::read_to_string(fixture.host()).unwrap(), "a = 1\n");
        while fixture.watcher.try_changed().is_some() {}
        fixture.quiet();

        // The new directory is watched now.
        fs::write(fixture.host(), "a = 2\n").unwrap();
        assert_eq!(fixture.report(), [fixture.host()]);

        fs::remove_dir_all(fixture.dir.path().join("settings.d")).unwrap();
        assert_eq!(fixture.report(), [fixture.host()]);
        fixture.quiet();

        // And again once it is back.
        fs::create_dir(fixture.dir.path().join("settings.d")).unwrap();
        assert_eq!(fixture.report(), [fixture.host()]);
        fs::write(fixture.host(), "a = 3\n").unwrap();
        assert_eq!(fixture.report(), [fixture.host()]);
    }

    #[test]
    fn other_files_and_reads_are_not_reported() {
        let fixture = Fixture::new(|dir| {
            fs::write(dir.join("settings.toml"), "a = 0\n").unwrap();
            fs::create_dir(dir.join("settings.d")).unwrap();
        });
        fs::write(fixture.dir.path().join("notes.txt"), "x").unwrap();
        fs::write(fixture.dir.path().join(".settings.toml.swp"), "x").unwrap();
        fs::write(fixture.dir.path().join("settings.d/desktop.toml"), "x").unwrap();
        fs::read_to_string(fixture.shared()).unwrap();
        fixture.quiet();
    }

    #[test]
    fn both_files_changing_together_are_one_report() {
        let fixture = Fixture::new(|dir| fs::create_dir(dir.join("settings.d")).unwrap());
        fs::write(fixture.host(), "a = 1\n").unwrap();
        fs::write(fixture.shared(), "a = 1\n").unwrap();
        assert_eq!(fixture.report(), [fixture.host(), fixture.shared()]);
        fixture.quiet();
    }

    #[test]
    fn a_linked_file_is_watched_where_it_really_is() {
        let dotfiles = tempfile::tempdir().unwrap();
        let real = dotfiles.path().join("settings.toml");
        fs::write(&real, "a = 0\n").unwrap();
        let fixture = Fixture::new(|dir| {
            std::os::unix::fs::symlink(&real, dir.join("settings.toml")).unwrap();
        });
        fs::write(&real, "a = 1\n").unwrap();
        assert_eq!(fixture.report(), [fixture.shared()]);
        // Replaced in the repository, as `git checkout` does.
        let temporary = dotfiles.path().join("settings.toml.new");
        fs::write(&temporary, "a = 2\n").unwrap();
        fs::rename(&temporary, &real).unwrap();
        assert_eq!(fixture.report(), [fixture.shared()]);
        fixture.quiet();
    }

    #[test]
    fn a_watched_directory_reports_the_files_in_it() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let drop_ins = dir.path().join("config.d");
        fs::write(&config, "").unwrap();
        let watcher =
            Watcher::with_directories([config.clone()], [drop_ins.clone()], DEBOUNCE).unwrap();
        let report = || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(report) = watcher.try_changed() {
                    return report;
                }
                assert!(Instant::now() < deadline, "no report");
                thread::sleep(Duration::from_millis(5));
            }
        };
        let quiet = || {
            thread::sleep(DEBOUNCE * 4);
            assert_eq!(watcher.try_changed(), None);
        };

        // The directory appears, then files in it come, change and go.
        fs::create_dir(&drop_ins).unwrap();
        assert_eq!(report(), std::slice::from_ref(&drop_ins));
        fs::write(drop_ins.join("10-new.conf"), "a\n").unwrap();
        assert_eq!(report(), std::slice::from_ref(&drop_ins));
        fs::write(drop_ins.join("10-new.conf"), "b\n").unwrap();
        assert_eq!(report(), std::slice::from_ref(&drop_ins));
        fs::rename(drop_ins.join("10-new.conf"), drop_ins.join("20-new.conf")).unwrap();
        assert_eq!(report(), std::slice::from_ref(&drop_ins));
        fs::remove_file(drop_ins.join("20-new.conf")).unwrap();
        assert_eq!(report(), std::slice::from_ref(&drop_ins));
        quiet();

        // Not what is deeper, and not the files next to it.
        fs::create_dir(drop_ins.join("deeper")).unwrap();
        assert_eq!(report(), std::slice::from_ref(&drop_ins));
        fs::write(drop_ins.join("deeper/file"), "").unwrap();
        fs::write(dir.path().join("other"), "").unwrap();
        quiet();
        fs::write(&config, "changed\n").unwrap();
        assert_eq!(report(), [config]);

        fs::remove_dir_all(&drop_ins).unwrap();
        assert_eq!(report(), [drop_ins]);
        quiet();
    }
}
