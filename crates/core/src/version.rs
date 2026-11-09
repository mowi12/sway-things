// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! sway versions, for settings and values that need a newer sway than the oldest supported one.

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
        write!(f, "{}.{}", self.major, self.minor)?;
        if self.patch != 0 {
            write!(f, ".{}", self.patch)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SwayVersion;

    #[test]
    fn versions_order_and_display() {
        assert!(SwayVersion::new(1, 10, 1) < SwayVersion::new(1, 11, 0));
        assert!(SwayVersion::new(1, 9, 9) < SwayVersion::OLDEST_SUPPORTED);
        assert_eq!(SwayVersion::new(1, 11, 0).to_string(), "1.11");
        assert_eq!(SwayVersion::new(1, 10, 1).to_string(), "1.10.1");
    }
}
