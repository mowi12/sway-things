// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Time zones as people read them: the city and region of an IANA name, and the abbreviation,
//! offset and local time of a zone at a moment, from the system's tzdata
//! (`/usr/share/zoneinfo`, or `$TZDIR`).
//!
//! ```
//! use sway_things_core::region::zones;
//!
//! let zone = zones::Zone::new("America/Argentina/Buenos_Aires");
//! assert_eq!(zone.city(), "Buenos Aires");
//! assert_eq!(zone.region(), "America, Argentina");
//! assert_eq!(zones::offset_text(-3 * 3600), "UTC\u{2212}03:00");
//! ```

use jiff::Timestamp;
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;

/// A time zone by its IANA name, with the parts people read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Zone {
    name: String,
}

impl Zone {
    /// The zone named `name` (`Europe/Berlin`). Whether tzdata knows it isn't checked.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
        }
    }

    /// The IANA name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The city: the last part of the name, with spaces for underscores (`Buenos Aires`). A name
    /// without a region (`UTC`) is all city.
    pub fn city(&self) -> String {
        self.name
            .rsplit('/')
            .next()
            .unwrap_or(&self.name)
            .replace('_', " ")
    }

    /// The region: the parts before the city, joined with commas (`America, Argentina`); empty
    /// for a name without a region.
    pub fn region(&self) -> String {
        let mut parts: Vec<&str> = self.name.split('/').collect();
        parts.pop();
        parts.join(", ").replace('_', " ")
    }

    /// The city and the region, as the time zone row shows them (`Berlin, Europe`).
    pub fn place(&self) -> String {
        let region = self.region();
        if region.is_empty() {
            self.city()
        } else {
            format!("{}, {region}", self.city())
        }
    }

    /// What the zone is now, or `None` if tzdata doesn't have it.
    pub fn now(&self) -> Option<ZoneNow> {
        self.at(Timestamp::now())
    }

    /// What the zone is at `now`, or `None` if tzdata doesn't have it.
    pub fn at(&self, now: Timestamp) -> Option<ZoneNow> {
        let tz = TimeZone::get(&self.name).ok()?;
        let info = tz.to_offset_info(now);
        let datetime = tz.to_datetime(now);
        Some(ZoneNow {
            abbreviation: info.abbreviation().to_owned(),
            offset_seconds: info.offset().seconds(),
            hour: datetime.hour(),
            minute: datetime.minute(),
        })
    }

    /// Whether this zone's name or place has every word of `query`, in any case: the search of
    /// the time zone dialog. An empty query matches every zone.
    pub fn matches(&self, query: &str) -> bool {
        let haystack = format!("{} {}", self.name, self.place()).to_lowercase();
        query
            .to_lowercase()
            .split_whitespace()
            .all(|word| haystack.contains(word))
    }
}

/// What a zone is at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneNow {
    /// The abbreviation (`CET`, `CEST`), or the offset where tzdata has none (`-03`).
    pub abbreviation: String,
    /// The offset from UTC, in seconds east.
    pub offset_seconds: i32,
    /// The local hour, 0 to 23.
    pub hour: i8,
    /// The local minute.
    pub minute: i8,
}

impl ZoneNow {
    /// The offset as `UTC+01:00` ([`offset_text`]).
    pub fn offset(&self) -> String {
        offset_text(self.offset_seconds)
    }

    /// The local time, `21:35`.
    pub fn time(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

/// `seconds` east of UTC as `UTC+01:00`, `UTC−03:30` (with a minus sign) or `UTC` for none.
pub fn offset_text(seconds: i32) -> String {
    if seconds == 0 {
        return "UTC".to_owned();
    }
    let sign = if seconds < 0 { '\u{2212}' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    format!("UTC{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The local date and time in `zone` at `now`, or `None` if tzdata doesn't have the zone.
pub fn local(zone: &str, now: Timestamp) -> Option<DateTime> {
    Some(TimeZone::get(zone).ok()?.to_datetime(now))
}

/// The local date and time in `zone` now, to start the clock's dialog with; in UTC if tzdata
/// doesn't have the zone.
pub fn local_now(zone: &str) -> LocalTime {
    let now = Timestamp::now();
    let datetime = local(zone, now).unwrap_or_else(|| TimeZone::UTC.to_datetime(now));
    LocalTime {
        year: datetime.year(),
        month: datetime.month(),
        day: datetime.day(),
        hour: datetime.hour(),
        minute: datetime.minute(),
    }
}

/// A date and time picked in the dialog: what the clock is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    /// The year.
    pub year: i16,
    /// The month, 1 to 12.
    pub month: i8,
    /// The day of the month.
    pub day: i8,
    /// The hour, 0 to 23.
    pub hour: i8,
    /// The minute.
    pub minute: i8,
}

impl LocalTime {
    /// The date and time, or `None` if it doesn't exist (February 30).
    fn datetime(self) -> Option<DateTime> {
        let date = Date::new(self.year, self.month, self.day).ok()?;
        let time = Time::new(self.hour, self.minute, 0, 0).ok()?;
        Some(date.to_datetime(time))
    }

    /// The moment it is in `zone`, in microseconds since the epoch, for timedated's `SetTime`:
    /// a time skipped when the clocks go forward counts as the time after the gap, one that comes
    /// twice as the first. `None` if the date doesn't exist or tzdata doesn't have the zone.
    pub fn micros_in(self, zone: &str) -> Option<i64> {
        let tz = TimeZone::get(zone).ok()?;
        let timestamp = tz
            .to_ambiguous_timestamp(self.datetime()?)
            .compatible()
            .ok()?;
        Some(timestamp.as_microsecond())
    }

    /// As `timedatectl set-time` takes it: `2026-11-24 21:35:00`.
    pub fn text(self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:00",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split_into_city_and_region() {
        let berlin = Zone::new("Europe/Berlin");
        assert_eq!(berlin.city(), "Berlin");
        assert_eq!(berlin.region(), "Europe");
        assert_eq!(berlin.place(), "Berlin, Europe");
        let utc = Zone::new("UTC");
        assert_eq!(utc.city(), "UTC");
        assert_eq!(utc.region(), "");
        assert_eq!(utc.place(), "UTC");
        assert_eq!(
            Zone::new("America/North_Dakota/New_Salem").place(),
            "New Salem, America, North Dakota"
        );
    }

    #[test]
    fn search_takes_every_word_in_any_case() {
        let zone = Zone::new("America/Argentina/Buenos_Aires");
        assert!(zone.matches(""));
        assert!(zone.matches("buenos"));
        assert!(zone.matches("aires ARGENTINA"));
        assert!(zone.matches("buenos_aires"));
        assert!(!zone.matches("berlin"));
        assert!(!zone.matches("buenos berlin"));
    }

    #[test]
    fn offsets_read_like_utc_plus_hours_and_minutes() {
        assert_eq!(offset_text(0), "UTC");
        assert_eq!(offset_text(3600), "UTC+01:00");
        assert_eq!(offset_text(5 * 3600 + 45 * 60), "UTC+05:45");
        assert_eq!(offset_text(-(3 * 3600 + 30 * 60)), "UTC\u{2212}03:30");
    }

    #[test]
    fn zones_report_abbreviation_offset_and_time() {
        // 2026-01-15 12:00 UTC, in winter in Berlin.
        let winter: Timestamp = "2026-01-15T12:00:00Z".parse().unwrap();
        let Some(now) = Zone::new("Europe/Berlin").at(winter) else {
            // A system without tzdata.
            return;
        };
        assert_eq!(now.abbreviation, "CET");
        assert_eq!(now.offset(), "UTC+01:00");
        assert_eq!(now.time(), "13:00");
        let summer: Timestamp = "2026-07-15T12:00:00Z".parse().unwrap();
        let now = Zone::new("Europe/Berlin").at(summer).unwrap();
        assert_eq!(now.abbreviation, "CEST");
        assert_eq!(now.time(), "14:00");
        assert_eq!(Zone::new("Mars/Olympus_Mons").at(winter), None);
    }

    #[test]
    fn a_picked_local_time_becomes_the_moment_in_the_zone() {
        let picked = LocalTime {
            year: 2026,
            month: 11,
            day: 24,
            hour: 21,
            minute: 35,
        };
        assert_eq!(picked.text(), "2026-11-24 21:35:00");
        let Some(micros) = picked.micros_in("Europe/Berlin") else {
            return;
        };
        let utc: Timestamp = "2026-11-24T20:35:00Z".parse().unwrap();
        assert_eq!(micros, utc.as_microsecond());
        assert_eq!(picked.micros_in("Mars/Olympus_Mons"), None);
        let missing = LocalTime { day: 31, ..picked };
        assert_eq!(missing.micros_in("Europe/Berlin"), None);
        let local = local("Europe/Berlin", utc).unwrap();
        assert_eq!((local.hour(), local.minute()), (21, 35));
    }
}
