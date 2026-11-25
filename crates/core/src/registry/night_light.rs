// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the night light on the Brightness & Night Light page: whether it is on, the tool
//! (wlsunset or gammastep), the schedule and the temperatures.
//!
//! None of them has a sway directive: `sway-things hook night-light` in `80-hooks.conf` starts
//! the tool with them while the night light is on ([`crate::night_light`]). The brightness of the
//! page is live and stores nothing.
//!
//! The keys, the defaults and the check of a time are defined here and re-exported by
//! [`crate::night_light`], so the registry stays the bottom layer.

use super::{Choice, Page, Setting, Unit, ValueError, When};
use crate::msg;

/// The key of the switch of the night light.
pub const ENABLED: &str = "night_light.enabled";
/// The key of the tool that makes the colors warmer: `wlsunset` or `gammastep`.
pub const TOOL: &str = "night_light.tool";
/// The key of when the night light is on: `sunset`, `times` or `always`.
pub const SCHEDULE: &str = "night_light.schedule";
/// The key of where the sun's times come from: `timezone` or `custom`.
pub const LOCATION: &str = "night_light.location";
/// The key of the latitude of a location entered by hand, in degrees north.
pub const LATITUDE: &str = "night_light.latitude";
/// The key of the longitude of a location entered by hand, in degrees east.
pub const LONGITUDE: &str = "night_light.longitude";
/// The key of the time the night starts with fixed times, `HH:MM`.
pub const FROM: &str = "night_light.from";
/// The key of the time the night ends with fixed times, `HH:MM`.
pub const TO: &str = "night_light.to";
/// The key of the color temperature at night, in kelvin.
pub const TEMPERATURE_NIGHT: &str = "night_light.temperature_night";
/// The key of the color temperature by day, in kelvin.
pub const TEMPERATURE_DAY: &str = "night_light.temperature_day";

/// The lowest color temperature offered, in kelvin.
pub const MIN_TEMPERATURE: i64 = 1000;
/// The highest color temperature offered, in kelvin: neutral daylight, no change.
pub const MAX_TEMPERATURE: i64 = 6500;
/// The default temperature at night: wlsunset's own.
pub const DEFAULT_NIGHT: i64 = 4000;

/// The hour and minute of a time written `HH:MM` (or `H:MM`), 24-hour; `None` for anything else.
pub fn parse_time(text: &str) -> Option<(u32, u32)> {
    let (hour, minute) = text.split_once(':')?;
    let digits = |part: &str, len: std::ops::RangeInclusive<usize>| {
        (len.contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    let hour = digits(hour, 1..=2)?;
    let minute = digits(minute, 2..=2)?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// The check of a time setting: `HH:MM`, 24-hour.
///
/// # Errors
///
/// [`ValueError::NotATime`].
pub fn check_time(text: &str) -> Result<(), ValueError> {
    parse_time(text).map(|_| ()).ok_or(ValueError::NotATime)
}

const TOOLS: &[Choice] = &[
    Choice::new("wlsunset", msg!("wlsunset")),
    Choice::new("gammastep", msg!("gammastep")),
];

const SCHEDULES: &[Choice] = &[
    Choice::new("sunset", msg!("Sunset to sunrise")),
    Choice::new("times", msg!("Fixed times")),
    Choice::new("always", msg!("Always on")),
];

const LOCATIONS: &[Choice] = &[
    Choice::new("timezone", msg!("From the time zone")),
    Choice::new("custom", msg!("Entered by hand")),
];

pub(super) fn settings() -> Vec<Setting> {
    let mut settings = schedule();
    settings.extend(temperatures());
    settings
}

/// The switch, the tool and when the night light is on.
fn schedule() -> Vec<Setting> {
    let section = msg!("Night light");
    let page = Page::BrightnessNightLight;
    vec![
        Setting::toggle(ENABLED, page, false)
            .section(section)
            .title(msg!("Night light"))
            .subtitle(msg!(
                "Warmer colors in the evening and at night, with wlsunset or gammastep"
            ))
            .keywords(&[
                msg!("night light"),
                msg!("night shift"),
                msg!("blue light"),
                msg!("color temperature"),
                msg!("redshift"),
                msg!("wlsunset"),
                msg!("gammastep"),
            ])
            .build(),
        Setting::choice(TOOL, page, "wlsunset", TOOLS)
            .section(section)
            .title(msg!("Tool"))
            .subtitle(msg!("The program that changes the colors of the displays"))
            .depends_on(ENABLED, When::On)
            .build(),
        Setting::choice(SCHEDULE, page, "sunset", SCHEDULES)
            .section(section)
            .title(msg!("Schedule"))
            .depends_on(ENABLED, When::On)
            .keywords(&[msg!("sunset"), msg!("sunrise"), msg!("time")])
            .build(),
        Setting::choice(LOCATION, page, "timezone", LOCATIONS)
            .section(section)
            .title(msg!("Location"))
            .subtitle(msg!(
                "Sunset and sunrise depend on where you are; the time zone gives its main city. \
                 Nothing leaves this computer"
            ))
            .depends_on(ENABLED, When::On)
            .keywords(&[msg!("location"), msg!("coordinates"), msg!("time zone")])
            .build(),
        Setting::decimal(LATITUDE, page, 0.0, -90.0, 90.0)
            .section(section)
            .title(msg!("Latitude"))
            .subtitle(msg!("Degrees north; south is negative"))
            .slider(-90.0, 90.0, 0.01)
            .depends_on(ENABLED, When::On)
            .keywords(&[msg!("location"), msg!("coordinates")])
            .build(),
        Setting::decimal(LONGITUDE, page, 0.0, -180.0, 180.0)
            .section(section)
            .title(msg!("Longitude"))
            .subtitle(msg!("Degrees east; west is negative"))
            .slider(-180.0, 180.0, 0.01)
            .depends_on(ENABLED, When::On)
            .keywords(&[msg!("location"), msg!("coordinates")])
            .build(),
        Setting::text(FROM, page, "20:00")
            .section(section)
            .title(msg!("From"))
            .subtitle(msg!("When the night starts, as HH:MM"))
            .check_text(check_time)
            .depends_on(ENABLED, When::On)
            .keywords(&[msg!("time"), msg!("evening")])
            .build(),
        Setting::text(TO, page, "07:00")
            .section(section)
            .title(msg!("To"))
            .subtitle(msg!("When the night ends, as HH:MM"))
            .check_text(check_time)
            .depends_on(ENABLED, When::On)
            .keywords(&[msg!("time"), msg!("morning")])
            .build(),
    ]
}

/// The temperatures, at night and by day.
fn temperatures() -> Vec<Setting> {
    let section = msg!("Night light");
    let page = Page::BrightnessNightLight;
    vec![
        Setting::integer(
            TEMPERATURE_NIGHT,
            page,
            DEFAULT_NIGHT,
            MIN_TEMPERATURE,
            MAX_TEMPERATURE,
        )
        .section(section)
        .title(msg!("Night temperature"))
        .subtitle(msg!(
            "Lower is warmer and more orange; 6500 K changes nothing"
        ))
        .slider(1000.0, 6500.0, 100.0)
        .unit(Unit::Kelvin)
        .depends_on(ENABLED, When::On)
        .keywords(&[msg!("warm"), msg!("kelvin"), msg!("color temperature")])
        .build(),
        Setting::integer(
            TEMPERATURE_DAY,
            page,
            MAX_TEMPERATURE,
            MIN_TEMPERATURE,
            MAX_TEMPERATURE,
        )
        .section(section)
        .title(msg!("Day temperature"))
        .subtitle(msg!("6500 K leaves the colors as they are by day"))
        .slider(1000.0, 6500.0, 100.0)
        .unit(Unit::Kelvin)
        .depends_on(ENABLED, When::On)
        .keywords(&[msg!("kelvin"), msg!("color temperature")])
        .build(),
    ]
}

#[cfg(test)]
mod tests {
    use super::{check_time, parse_time};

    #[test]
    fn times_are_24_hour_with_two_digit_minutes() {
        assert_eq!(parse_time("20:00"), Some((20, 0)));
        assert_eq!(parse_time("7:05"), Some((7, 5)));
        assert_eq!(parse_time("07:05"), Some((7, 5)));
        assert_eq!(parse_time("23:59"), Some((23, 59)));
        for wrong in [
            "24:00", "12:60", "12:5", "1200", "", ":30", "12:30:00", "+1:30", "ab:cd",
        ] {
            assert_eq!(parse_time(wrong), None, "{wrong}");
            assert!(check_time(wrong).is_err(), "{wrong}");
        }
    }
}
