// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Snapshots of the settings files.
//!
//! A snapshot is a directory named after the time it was taken, with the settings files at the
//! same relative paths as in the config directory:
//!
//! ```text
//! history/
//! ├── 20261110T142355Z/
//! │   ├── settings.toml
//! │   └── settings.d/laptop.toml
//! └── 20261110T143102Z/
//!     └── settings.toml
//! ```
//!
//! A file that didn't exist at that time is missing from the snapshot.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How many snapshots are kept. Older ones are deleted.
pub(super) const LIMIT: usize = 50;

/// A file of a snapshot: its path relative to the config directory, and its content if it
/// existed.
pub(super) type File = (PathBuf, Option<String>);

/// A snapshot of the settings files, taken before a save replaced them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    id: String,
    taken_at: SystemTime,
}

impl Snapshot {
    /// The name of the snapshot, e.g. `20261110T142355Z`: the UTC time it was taken, with a
    /// counter (`-02`) if several were taken within a second. Newer snapshots sort after older
    /// ones.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// When the snapshot was taken, to the second.
    pub fn taken_at(&self) -> SystemTime {
        self.taken_at
    }
}

/// Saves `files` as a snapshot taken at `now` and deletes the oldest snapshots beyond `limit`.
/// Nothing is saved if no file exists, or if the newest snapshot holds the same contents.
pub(super) fn take(
    history: &Path,
    files: &[File],
    now: SystemTime,
    limit: usize,
) -> io::Result<()> {
    if files.iter().all(|(_, content)| content.is_none()) {
        return Ok(());
    }
    let snapshots = list(history)?;
    if let Some(newest) = snapshots.first() {
        let paths: Vec<&Path> = files.iter().map(|(path, _)| path.as_path()).collect();
        let contents = read(history, newest.id(), &paths)?;
        if contents.iter().eq(files.iter().map(|(_, content)| content)) {
            return Ok(());
        }
    }

    // Written under another name first, so a snapshot is complete or not there.
    fs::create_dir_all(history)?;
    let temporary = history.join(format!(".{}.tmp", std::process::id()));
    if temporary.exists() {
        fs::remove_dir_all(&temporary)?;
    }
    for (path, content) in files {
        if let Some(content) = content {
            let path = temporary.join(path);
            if let Some(directory) = path.parent() {
                fs::create_dir_all(directory)?;
            }
            fs::write(path, content)?;
        }
    }
    let timestamp = timestamp(now);
    let id = (1..100)
        .map(|counter| match counter {
            1 => timestamp.clone(),
            _ => format!("{timestamp}-{counter:02}"),
        })
        .find(|id| !history.join(id).exists())
        .ok_or_else(|| io::Error::other("too many snapshots within one second"))?;
    fs::rename(&temporary, history.join(id))?;

    // The new snapshot is one more than `snapshots`, which is newest first.
    for old in snapshots.iter().skip(limit.saturating_sub(1)) {
        fs::remove_dir_all(history.join(old.id()))?;
    }
    Ok(())
}

/// The snapshots, newest first. Anything else in the directory is ignored.
pub(super) fn list(history: &Path) -> io::Result<Vec<Snapshot>> {
    let entries = match fs::read_dir(history) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut snapshots = Vec::new();
    for entry in entries {
        let entry = entry?;
        let snapshot = entry.file_name().to_str().and_then(parse_id);
        if let Some(snapshot) = snapshot.filter(|_| entry.path().is_dir()) {
            snapshots.push(snapshot);
        }
    }
    snapshots.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(snapshots)
}

/// The contents of the files at `paths` in the snapshot `id`; `None` for a file that didn't
/// exist when the snapshot was taken.
///
/// # Errors
///
/// `NotFound` if there is no such snapshot. `id` is checked to be a snapshot name, so it can't
/// point anywhere else.
pub(super) fn read(history: &Path, id: &str, paths: &[&Path]) -> io::Result<Vec<Option<String>>> {
    let snapshot = history.join(id);
    if parse_id(id).is_none() || !snapshot.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no such snapshot"));
    }
    paths
        .iter()
        .map(|path| match fs::read_to_string(snapshot.join(path)) {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        })
        .collect()
}

/// `YYYYMMDDTHHMMSSZ` in UTC. A time before 1970 counts as 1970.
fn timestamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, second_of_day) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        second_of_day / 3600,
        second_of_day % 3600 / 60,
        second_of_day % 60
    )
}

/// Reads a snapshot name: a timestamp as written by [`timestamp`], and an optional counter.
fn parse_id(id: &str) -> Option<Snapshot> {
    let (stamp, counter) = match id.split_once('-') {
        Some((stamp, counter)) => (stamp, Some(counter)),
        None => (id, None),
    };
    if counter.is_some_and(|counter| {
        counter.len() != 2 || !counter.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return None;
    }
    let bytes = stamp.as_bytes();
    if bytes.len() != 16 || bytes[8] != b'T' || bytes[15] != b'Z' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u64> {
        let digits = stamp.get(range)?;
        digits
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| digits.parse().ok())?
    };
    let (year, month, day) = (number(0..4)?, number(4..6)?, number(6..8)?);
    let (hour, minute, second) = (number(9..11)?, number(11..13)?, number(13..15)?);
    if year < 1970
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    Some(Snapshot {
        id: id.to_owned(),
        taken_at: UNIX_EPOCH + Duration::from_secs(seconds),
    })
}

/// The date of the day `days` after 1970-01-01, in the Gregorian calendar. This is Howard
/// Hinnant's `civil_from_days`, for days from 1970 on.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    // Day 0 of this count is 0000-03-01: years start in March, so a leap day is the last day.
    let days = days + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// The inverse of [`civil_from_days`], for dates from 1970 on.
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = year - u64::from(month <= 2);
    let era = year / 400;
    let year_of_era = year % 400;
    let month_from_march = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::{File, civil_from_days, days_from_civil, list, parse_id, read, take, timestamp};

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn files(shared: Option<&str>, host: Option<&str>) -> [File; 2] {
        [
            (PathBuf::from("settings.toml"), shared.map(Into::into)),
            (
                PathBuf::from("settings.d/laptop.toml"),
                host.map(Into::into),
            ),
        ]
    }

    fn ids(history: &Path) -> Vec<String> {
        list(history)
            .unwrap()
            .iter()
            .map(|snapshot| snapshot.id().to_owned())
            .collect()
    }

    #[test]
    fn timestamps_are_utc_dates() {
        for (seconds, expected) in [
            (0, "19700101T000000Z"),
            (951_782_400, "20000229T000000Z"),
            (1_709_251_199, "20240229T235959Z"),
            (1_709_251_200, "20240301T000000Z"),
            (1_794_320_635, "20261110T142355Z"),
            (4_102_444_799, "20991231T235959Z"),
        ] {
            assert_eq!(timestamp(at(seconds)), expected);
            let snapshot = parse_id(expected).unwrap();
            assert_eq!(snapshot.taken_at(), at(seconds));
            assert_eq!(snapshot.id(), expected);
        }
        assert_eq!(
            timestamp(UNIX_EPOCH - Duration::from_secs(5)),
            "19700101T000000Z"
        );
        // Every day of 140 years converts back to itself.
        for days in 0..(140 * 366) {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
        }
    }

    #[test]
    fn only_snapshot_names_are_ids() {
        assert_eq!(
            parse_id("20261110T142355Z-02").unwrap().taken_at(),
            at(1_794_320_635)
        );
        for id in [
            "",
            "..",
            "../20261110T142355Z",
            "20261110T142355",
            "20261110 142355Z",
            "20261310T142355Z",
            "20261100T142355Z",
            "20261110T242355Z",
            "19691231T235959Z",
            "20261110T142355Z-2",
            "20261110T142355Z-0a",
            "20261110T142355Z-02-03",
            "2026111٠T142355Z",
            ".1234.tmp",
        ] {
            assert_eq!(parse_id(id), None, "{id:?}");
        }
    }

    #[test]
    fn a_snapshot_holds_the_files_that_existed() {
        let dir = tempfile::tempdir().unwrap();
        let history = dir.path().join("state/history");
        assert_eq!(ids(&history), Vec::<String>::new());

        // Nothing existed: nothing to keep.
        take(&history, &files(None, None), at(1_794_320_635), 50).unwrap();
        assert!(!history.exists());

        take(
            &history,
            &files(Some("a = 1\n"), None),
            at(1_794_320_635),
            50,
        )
        .unwrap();
        take(
            &history,
            &files(Some("a = 2\n"), Some("b = 1\n")),
            at(1_794_321_062),
            50,
        )
        .unwrap();
        assert_eq!(ids(&history), ["20261110T143102Z", "20261110T142355Z"]);
        assert_eq!(
            fs::read_to_string(history.join("20261110T143102Z/settings.d/laptop.toml")).unwrap(),
            "b = 1\n"
        );
        assert!(!history.join("20261110T142355Z/settings.d").exists());

        let paths = [
            Path::new("settings.toml"),
            Path::new("settings.d/laptop.toml"),
        ];
        assert_eq!(
            read(&history, "20261110T142355Z", &paths).unwrap(),
            [Some("a = 1\n".to_owned()), None]
        );
        assert_eq!(
            read(&history, "20261110T143102Z", &paths).unwrap(),
            [Some("a = 2\n".to_owned()), Some("b = 1\n".to_owned())]
        );
        for id in ["20251110T143102Z", "../history/20261110T143102Z", ""] {
            assert_eq!(
                read(&history, id, &paths).unwrap_err().kind(),
                std::io::ErrorKind::NotFound
            );
        }
    }

    #[test]
    fn the_same_contents_are_kept_once() {
        let dir = tempfile::tempdir().unwrap();
        let history = dir.path();
        take(history, &files(Some("a = 1\n"), None), at(100), 50).unwrap();
        take(history, &files(Some("a = 1\n"), None), at(200), 50).unwrap();
        assert_eq!(ids(history), ["19700101T000140Z"]);
        // A file that appears makes it a new state, even if empty.
        take(history, &files(Some("a = 1\n"), Some("")), at(300), 50).unwrap();
        // Going back to an older state is a new snapshot too.
        take(history, &files(Some("a = 1\n"), None), at(400), 50).unwrap();
        assert_eq!(ids(history).len(), 3);
    }

    #[test]
    fn snapshots_within_a_second_get_a_counter() {
        let dir = tempfile::tempdir().unwrap();
        let history = dir.path();
        for content in ["a = 1\n", "a = 2\n", "a = 3\n"] {
            take(history, &files(Some(content), None), at(100), 50).unwrap();
        }
        assert_eq!(
            ids(history),
            [
                "19700101T000140Z-03",
                "19700101T000140Z-02",
                "19700101T000140Z"
            ]
        );
    }

    #[test]
    fn the_oldest_snapshots_beyond_the_limit_are_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let history = dir.path();
        // Not ours: never listed, never deleted.
        fs::write(history.join("notes.txt"), "keep").unwrap();
        fs::create_dir(history.join("backup")).unwrap();
        fs::write(history.join("20261110T142355Z"), "a file, not a snapshot").unwrap();

        for second in 0..5 {
            let content = format!("a = {second}\n");
            take(history, &files(Some(&content), None), at(second), 3).unwrap();
        }
        assert_eq!(
            ids(history),
            ["19700101T000004Z", "19700101T000003Z", "19700101T000002Z"]
        );
        let mut names: Vec<_> = fs::read_dir(history)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "19700101T000002Z",
                "19700101T000003Z",
                "19700101T000004Z",
                "20261110T142355Z",
                "backup",
                "notes.txt"
            ]
        );
    }
}
