// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The location of the time zone: the coordinates of its main city, from the time zone database
//! (`zone1970.tab`, else `zone.tab`), for the sun's times without asking anyone where the
//! computer is. Reads files; nothing leaves the computer.

use std::path::{Path, PathBuf};

/// Where the time zone database is.
const ZONEINFO: &str = "/usr/share/zoneinfo";

/// A place on Earth, in degrees: north and east are positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinates {
    /// Degrees north, −90 to 90.
    pub latitude: f64,
    /// Degrees east, −180 to 180.
    pub longitude: f64,
}

/// The name of this computer's time zone (`Europe/Berlin`): from `TZ` if it names a zone, else
/// from where `/etc/localtime` points into the database.
pub fn zone() -> Option<String> {
    if let Some(tz) = std::env::var_os("TZ").and_then(|tz| tz.into_string().ok()) {
        let tz = tz.strip_prefix(':').unwrap_or(&tz);
        if let Some(zone) =
            zone_of_path(Path::new(tz)).or_else(|| is_zone_name(tz).then(|| tz.to_owned()))
        {
            return Some(zone);
        }
    }
    let target = std::fs::read_link("/etc/localtime").ok()?;
    zone_of_path(&target)
}

/// The zone a path into the database names: what follows `zoneinfo/`.
pub fn zone_of_path(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    let (_, zone) = text.rsplit_once("zoneinfo/")?;
    // `posix/Europe/Berlin` and `right/Europe/Berlin` are the same zone.
    let zone = zone
        .strip_prefix("posix/")
        .or_else(|| zone.strip_prefix("right/"))
        .unwrap_or(zone);
    is_zone_name(zone).then(|| zone.to_owned())
}

/// Whether `text` looks like a zone name: `Area/Location`, without dots or blanks.
fn is_zone_name(text: &str) -> bool {
    text.contains('/')
        && !text.starts_with('/')
        && !text.contains("..")
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+'))
}

/// The coordinates of this computer's time zone, if the database has them.
pub fn of_time_zone() -> Option<Coordinates> {
    of_zone(&zone()?, Path::new(ZONEINFO))
}

/// The coordinates of `zone` in the tables of the database at `zoneinfo`.
pub fn of_zone(zone: &str, zoneinfo: &Path) -> Option<Coordinates> {
    ["zone1970.tab", "zone.tab"]
        .iter()
        .map(|table| -> PathBuf { zoneinfo.join(table) })
        .filter_map(|table| std::fs::read_to_string(table).ok())
        .find_map(|text| in_table(&text, zone))
}

/// The coordinates of `zone` in the text of `zone1970.tab` or `zone.tab`: tab-separated lines of
/// country codes, coordinates and zone.
pub fn in_table(text: &str, zone: &str) -> Option<Coordinates> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let mut fields = line.split('\t');
            let _codes = fields.next()?;
            let coordinates = fields.next()?;
            (fields.next()? == zone).then(|| parse_iso6709(coordinates))?
        })
}

/// Coordinates written as in the time zone tables (ISO 6709): `±DDMM±DDDMM` or
/// `±DDMMSS±DDDMMSS`.
pub fn parse_iso6709(text: &str) -> Option<Coordinates> {
    let split = text.get(1..)?.find(['+', '-'])? + 1;
    let (latitude, longitude) = text.split_at_checked(split)?;
    let latitude = degrees(latitude, 2)?;
    let longitude = degrees(longitude, 3)?;
    ((-90.0..=90.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude)).then_some(
        Coordinates {
            latitude,
            longitude,
        },
    )
}

/// The degrees of `±DD[D]MM[SS]`, with `width` digits of degrees.
fn degrees(text: &str, width: usize) -> Option<f64> {
    let (sign, digits) = text.split_at_checked(1)?;
    let sign = match sign {
        "+" => 1.0,
        "-" => -1.0,
        _ => return None,
    };
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let part = |range: std::ops::Range<usize>| -> Option<f64> {
        digits.get(range)?.parse::<u32>().ok().map(f64::from)
    };
    let whole = part(0..width)?;
    let minutes = part(width..width + 2)?;
    let seconds = match digits.len() - width {
        2 => 0.0,
        4 => part(width + 2..width + 4)?,
        _ => return None,
    };
    Some(sign * (whole + minutes / 60.0 + seconds / 3600.0))
}
