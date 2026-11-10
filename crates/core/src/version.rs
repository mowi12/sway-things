// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! sway and SwayFX versions, for settings and values that need a newer compositor than the oldest
//! supported one, or SwayFX at all.

use std::fmt;
use std::str::FromStr;

/// A sway version. SwayFX is compared by the sway version it is based on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SwayVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

impl SwayVersion {
    /// The oldest sway Sway Things supports.
    pub const OLDEST_SUPPORTED: Self = Self::new(1, 10, 0);

    /// The version `major.minor.patch`.
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl FromStr for SwayVersion {
    type Err = VersionError;

    /// Parses `major.minor` or `major.minor.patch`, e.g. `1.11` or `1.10.1`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let [major, minor, patch] = parse_version(text)?;
        Ok(Self::new(major, minor, patch))
    }
}

impl fmt::Display for SwayVersion {
    /// `1.11` for a `.0` release, `1.10.1` otherwise.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_version(f, self.major, self.minor, self.patch)
    }
}

/// A SwayFX version. SwayFX numbers its releases independently of sway (0.5 is based on sway
/// 1.10.1, 0.6 on 1.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SwayFxVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

impl SwayFxVersion {
    /// The oldest SwayFX Sway Things supports.
    pub const OLDEST_SUPPORTED: Self = Self::new(0, 5, 0);

    /// The version `major.minor.patch`.
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl FromStr for SwayFxVersion {
    type Err = VersionError;

    /// Parses `major.minor` or `major.minor.patch`, e.g. `0.5` or `0.5.3`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let [major, minor, patch] = parse_version(text)?;
        Ok(Self::new(major, minor, patch))
    }
}

impl fmt::Display for SwayFxVersion {
    /// `0.6` for a `.0` release, `0.5.2` otherwise.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_version(f, self.major, self.minor, self.patch)
    }
}

/// The string isn't a version of the form `major.minor` or `major.minor.patch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionError;

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a version like 1.10 or 1.10.1")
    }
}

impl std::error::Error for VersionError {}

fn parse_version(text: &str) -> Result<[u16; 3], VersionError> {
    let mut numbers = [0; 3];
    let mut parts = text.split('.');
    for (index, number) in numbers.iter_mut().enumerate() {
        match parts.next() {
            // Only digits: `parse` alone would also take a sign.
            Some(part) if !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) => {
                *number = part.parse().map_err(|_| VersionError)?;
            }
            // The patch number is optional.
            None if index == 2 => {}
            _ => return Err(VersionError),
        }
    }
    if parts.next().is_some() {
        return Err(VersionError);
    }
    Ok(numbers)
}

fn write_version(f: &mut fmt::Formatter<'_>, major: u16, minor: u16, patch: u16) -> fmt::Result {
    write!(f, "{major}.{minor}")?;
    if patch != 0 {
        write!(f, ".{patch}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{SwayFxVersion, SwayVersion, VersionError};

    #[test]
    fn versions_order_and_display() {
        assert!(SwayVersion::new(1, 10, 1) < SwayVersion::new(1, 11, 0));
        assert!(SwayVersion::new(1, 9, 9) < SwayVersion::OLDEST_SUPPORTED);
        assert_eq!(SwayVersion::new(1, 11, 0).to_string(), "1.11");
        assert_eq!(SwayVersion::new(1, 10, 1).to_string(), "1.10.1");
        assert!(SwayFxVersion::new(0, 5, 3) < SwayFxVersion::new(0, 6, 0));
        assert_eq!(SwayFxVersion::OLDEST_SUPPORTED.to_string(), "0.5");
        assert_eq!(SwayFxVersion::new(0, 5, 2).to_string(), "0.5.2");
    }

    #[test]
    fn versions_parse() {
        assert_eq!("1.10.1".parse(), Ok(SwayVersion::new(1, 10, 1)));
        assert_eq!("1.12".parse(), Ok(SwayVersion::new(1, 12, 0)));
        assert_eq!("1.12.0".parse(), Ok(SwayVersion::new(1, 12, 0)));
        assert_eq!("0.5.3".parse(), Ok(SwayFxVersion::new(0, 5, 3)));
        for text in [
            "", "1", "1.", ".1", "1.10.1.2", "1.x", "+1.10", "1.-1", "1.10-dev", "1 .10", "1.70000",
        ] {
            assert_eq!(text.parse::<SwayVersion>(), Err(VersionError), "{text:?}");
            assert_eq!(text.parse::<SwayFxVersion>(), Err(VersionError), "{text:?}");
        }
    }
}
