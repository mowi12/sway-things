// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The locks of the session, so two commands, hooks or the app don't do one thing at once.
//!
//! A lock is a file in `$XDG_RUNTIME_DIR`, made only if it isn't there, holding a token of its
//! own (the process number, a stamp and a counter). It is removed when dropped, only while it
//! still holds that token. One whose process is gone, or that is older than [`Lock::STALE`], was
//! left behind and is taken over: moved aside first (only one process can move a file), and put
//! back if what was moved was another, newer lock. A lock whose holder still runs is never taken
//! over before then, however long it is held (a region being selected for a recording).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// Why a lock can't be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockError {
    /// `XDG_RUNTIME_DIR` isn't set, so there is no place for it.
    NoRuntimeDir,
    /// Another process holds it, and didn't let go in time.
    Busy,
}

/// A lock of the session (see [the module](self)).
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
    token: String,
}

/// How long the waiting ways to take a lock wait between two tries.
const RETRY: Duration = Duration::from_millis(100);

impl Lock {
    /// How old a lock may get before it counts as left behind, whoever holds it.
    pub const STALE: Duration = Duration::from_secs(600);

    /// Takes the lock in the file `name` of `$XDG_RUNTIME_DIR`, without waiting.
    ///
    /// # Errors
    ///
    /// [`LockError::NoRuntimeDir`] without `XDG_RUNTIME_DIR`, [`LockError::Busy`] while another
    /// process holds it.
    pub fn take(name: &str) -> Result<Self, LockError> {
        Self::take_in(&runtime_dir()?, name).ok_or(LockError::Busy)
    }

    /// Takes the lock `name` as [`Self::take`] does, blocking the thread while another process
    /// holds it, at most `wait`.
    ///
    /// # Blocking
    ///
    /// Retries until the other process gives up the lock, for up to `wait`.
    ///
    /// # Errors
    ///
    /// [`LockError::NoRuntimeDir`] without `XDG_RUNTIME_DIR`, [`LockError::Busy`] if it is still
    /// held after `wait`.
    pub fn wait(name: &str, wait: Duration) -> Result<Self, LockError> {
        let dir = runtime_dir()?;
        let until = Instant::now() + wait;
        loop {
            if let Some(lock) = Self::take_in(&dir, name) {
                return Ok(lock);
            }
            if Instant::now() >= until {
                return Err(LockError::Busy);
            }
            std::thread::sleep(RETRY);
        }
    }

    /// Takes the lock `name` as [`Self::take`] does, waiting without blocking while another
    /// process holds it, at most `wait`.
    ///
    /// # Errors
    ///
    /// [`LockError::NoRuntimeDir`] without `XDG_RUNTIME_DIR`, [`LockError::Busy`] if it is still
    /// held after `wait`.
    pub async fn wait_for(name: &str, wait: Duration) -> Result<Self, LockError> {
        let dir = runtime_dir()?;
        let until = Instant::now() + wait;
        loop {
            if let Some(lock) = Self::take_in(&dir, name) {
                return Ok(lock);
            }
            if Instant::now() >= until {
                return Err(LockError::Busy);
            }
            async_io::Timer::after(RETRY).await;
        }
    }

    /// Takes the lock in the file `name` of `dir`; `None` while another process holds it.
    pub(crate) fn take_in(dir: &Path, name: &str) -> Option<Self> {
        static TAKEN: AtomicU64 = AtomicU64::new(0);
        let path = dir.join(name);
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let count = TAKEN.fetch_add(1, Ordering::Relaxed);
        let token = format!("{} {stamp} {count}", std::process::id());
        // Twice: once more after a lock left behind was moved aside.
        for _ in 0..2 {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                if file.write_all(token.as_bytes()).is_err() {
                    let _ = std::fs::remove_file(&path);
                    return None;
                }
                return Some(Self { path, token });
            }
            let held = std::fs::read_to_string(&path).ok()?;
            if !left_behind(&path, &held) {
                return None;
            }
            let aside = dir.join(format!("{name}.{}", token.replace(' ', "-")));
            if std::fs::rename(&path, &aside).is_err() {
                // Someone else moved it first.
                continue;
            }
            let moved = std::fs::read_to_string(&aside).unwrap_or_default();
            if moved != held {
                // Another process took it over and made a new lock meanwhile: put back (never
                // over a lock that is there again).
                let _ = std::fs::hard_link(&aside, &path);
                let _ = std::fs::remove_file(&aside);
                return None;
            }
            let _ = std::fs::remove_file(&aside);
        }
        None
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Only while it is still ours: a lock taken over meanwhile belongs to someone else.
        if std::fs::read_to_string(&self.path).is_ok_and(|held| held == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// `$XDG_RUNTIME_DIR`, where the locks are.
fn runtime_dir() -> Result<PathBuf, LockError> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .ok_or(LockError::NoRuntimeDir)
}

/// Whether the lock at `path` holding `held` was left behind: its process is gone, or it is
/// older than [`Lock::STALE`] (or empty and older than ten seconds: its process ended before it
/// wrote its token).
fn left_behind(path: &Path, held: &str) -> bool {
    let age = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .unwrap_or_default();
    if age > Lock::STALE {
        return true;
    }
    match held.split(' ').next().filter(|pid| !pid.is_empty()) {
        Some(pid) => {
            pid.bytes().all(|byte| byte.is_ascii_digit()) && !Path::new("/proc").join(pid).exists()
        }
        None => age > Duration::from_secs(10),
    }
}

#[cfg(test)]
mod tests {
    use super::Lock;

    #[test]
    fn a_lock_is_held_once_and_taken_over_only_when_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lock");
        let held = Lock::take_in(dir.path(), "test.lock").unwrap();
        // A live holder (this process): nobody else gets it.
        assert!(Lock::take_in(dir.path(), "test.lock").is_none());
        drop(held);
        assert!(!path.exists());
        // Left behind by a process that is gone: taken over.
        std::fs::write(&path, "4294967295 1 0").unwrap();
        let taken = Lock::take_in(dir.path(), "test.lock").unwrap();
        assert_ne!(std::fs::read_to_string(&path).unwrap(), "4294967295 1 0");
        // Taken over meanwhile by someone else: dropping ours leaves theirs.
        std::fs::write(&path, "1 2 3").unwrap();
        drop(taken);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "1 2 3");
        // Nothing else is left in the directory.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_lock_held_long_is_kept_while_its_holder_runs() {
        use std::time::{Duration, SystemTime};
        let age = |path: &std::path::Path| {
            let file = std::fs::File::options().write(true).open(path).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(20))
                .unwrap();
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lock");
        // Held for longer than ten seconds (a region being selected) by a process that runs.
        let held = Lock::take_in(dir.path(), "test.lock").unwrap();
        age(&path);
        assert!(Lock::take_in(dir.path(), "test.lock").is_none());
        // The same lock once its holder is gone: taken over.
        let mut gone = std::process::Command::new("true").spawn().unwrap();
        let pid = gone.id();
        gone.wait().unwrap();
        std::fs::write(&path, format!("{pid} 1 0")).unwrap();
        age(&path);
        let taken = Lock::take_in(dir.path(), "test.lock").unwrap();
        drop(held);
        assert!(path.exists());
        drop(taken);
        assert!(!path.exists());
    }
}
