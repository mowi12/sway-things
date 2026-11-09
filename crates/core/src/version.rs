// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! sway and SwayFX versions, for settings and values that need a newer compositor than the oldest
//! supported one, or SwayFX at all.

use std::fmt;

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

impl fmt::Display for SwayFxVersion {
    /// `0.6` for a `.0` release, `0.5.2` otherwise.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_version(f, self.major, self.minor, self.patch)
    }
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
    use super::{SwayFxVersion, SwayVersion};

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
}
