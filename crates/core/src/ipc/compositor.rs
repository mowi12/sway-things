// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which compositor answers on the IPC socket.

use std::fmt;

use serde::Deserialize;

use crate::command::SwayCmd;
use crate::version::{SwayFxVersion, SwayVersion};

/// The compositor behind the IPC socket: sway, or SwayFX with the sway it is based on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compositor {
    /// sway.
    Sway {
        /// The version.
        version: SwayVersion,
    },
    /// SwayFX.
    SwayFx {
        /// The SwayFX version.
        version: SwayFxVersion,
        /// The sway version this SwayFX is based on.
        base: SwayVersion,
    },
}

impl Compositor {
    /// The sway version, or for SwayFX the one it is based on.
    pub fn sway_version(self) -> SwayVersion {
        match self {
            Self::Sway { version } | Self::SwayFx { base: version, .. } => version,
        }
    }

    /// The SwayFX version, or `None` on sway.
    pub fn swayfx_version(self) -> Option<SwayFxVersion> {
        match self {
            Self::Sway { .. } => None,
            Self::SwayFx { version, .. } => Some(version),
        }
    }

    /// Whether this is SwayFX.
    pub fn is_swayfx(self) -> bool {
        self.swayfx_version().is_some()
    }

    /// Checks that Sway Things supports the compositor: sway from
    /// [`SwayVersion::OLDEST_SUPPORTED`] on, SwayFX from [`SwayFxVersion::OLDEST_SUPPORTED`] on.
    ///
    /// # Errors
    ///
    /// [`Unmet`] names what is too old.
    pub fn check_supported(self) -> Result<(), Unmet> {
        self.check(
            SwayVersion::OLDEST_SUPPORTED,
            self.swayfx_version()
                .map(|_| SwayFxVersion::OLDEST_SUPPORTED),
        )
    }

    /// Checks that the compositor understands `command`: it is new enough, and it is SwayFX if
    /// the command is a SwayFX directive.
    ///
    /// # Errors
    ///
    /// [`Unmet`] names what the command needs.
    pub fn supports(self, command: &SwayCmd) -> Result<(), Unmet> {
        self.check(command.min_sway(), command.min_swayfx())
    }

    /// Checks the compositor against a minimum sway version and, for something only SwayFX
    /// has, a minimum SwayFX version.
    ///
    /// # Errors
    ///
    /// [`Unmet`] names the first requirement that isn't met.
    pub fn check(
        self,
        min_sway: SwayVersion,
        min_swayfx: Option<SwayFxVersion>,
    ) -> Result<(), Unmet> {
        match (min_swayfx, self.swayfx_version()) {
            (Some(needed), None) => return Err(Unmet::SwayFx { needed }),
            (Some(needed), Some(version)) if version < needed => {
                return Err(Unmet::SwayFx { needed });
            }
            _ => {}
        }
        if self.sway_version() < min_sway {
            return Err(Unmet::Sway { needed: min_sway });
        }
        Ok(())
    }

    /// Reads the compositor from the output of `sway --version`: `sway version 1.10.1`, or
    /// `swayfx version 0.5.3 (based on sway 1.11.0)`. A development build has more after the
    /// number (`1.12-dev-2a2a8ad (…)`), which is ignored.
    ///
    /// This is the compositor that is installed, for when none is running to ask over IPC.
    pub fn from_version_output(output: &str) -> Option<Self> {
        /// The version number at the start of `text`.
        fn number(text: &str) -> &str {
            let end = text
                .find(|char: char| !char.is_ascii_digit() && char != '.')
                .unwrap_or(text.len());
            text[..end].trim_end_matches('.')
        }
        let line = output.lines().next()?.trim();
        if let Some(rest) = line.strip_prefix("swayfx version ") {
            let (_, base) = rest.split_once("(based on sway ")?;
            // Older SwayFX wrote "based on sway version".
            let base = base.strip_prefix("version ").unwrap_or(base);
            Some(Self::SwayFx {
                version: number(rest).parse().ok()?,
                base: number(base).parse().ok()?,
            })
        } else {
            let version = number(line.strip_prefix("sway version ")?);
            Some(Self::Sway {
                version: version.parse().ok()?,
            })
        }
    }

    /// Reads the compositor from the reply to `GET_VERSION`.
    pub(super) fn from_reply(reply: &VersionReply) -> Result<Self, InvalidVersion> {
        let number = |number: i64| u16::try_from(number).map_err(|_| InvalidVersion);
        let (major, minor, patch) = (
            number(reply.major)?,
            number(reply.minor)?,
            number(reply.patch)?,
        );
        Ok(match &reply.sway_original_version {
            // Only SwayFX sends this field; its own version is in the numbers then.
            Some(base) => Self::SwayFx {
                version: SwayFxVersion::new(major, minor, patch),
                base: base.parse().map_err(|_| InvalidVersion)?,
            },
            None => Self::Sway {
                version: SwayVersion::new(major, minor, patch),
            },
        })
    }
}

impl fmt::Display for Compositor {
    /// `sway 1.11` or `SwayFX 0.5.3 (based on sway 1.11)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sway { version } => write!(f, "sway {version}"),
            Self::SwayFx { version, base } => {
                write!(f, "SwayFX {version} (based on sway {base})")
            }
        }
    }
}

/// What the running compositor lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unmet {
    /// It is older than this sway (for SwayFX: based on an older one).
    Sway {
        /// The oldest sway that would do.
        needed: SwayVersion,
    },
    /// It isn't SwayFX, or an older one than this.
    SwayFx {
        /// The oldest SwayFX that would do.
        needed: SwayFxVersion,
    },
}

impl fmt::Display for Unmet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sway { needed } => write!(f, "requires sway {needed} or newer"),
            Self::SwayFx { needed } => write!(f, "requires SwayFX {needed} or newer"),
        }
    }
}

impl std::error::Error for Unmet {}

/// The reply to `GET_VERSION` doesn't hold version numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InvalidVersion;

/// The reply to `GET_VERSION`.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct VersionReply {
    pub(super) major: i64,
    pub(super) minor: i64,
    pub(super) patch: i64,
    pub(super) human_readable: String,
    /// The sway version SwayFX is based on. sway doesn't send it.
    pub(super) sway_original_version: Option<String>,
    pub(super) loaded_config_file_name: String,
}

#[cfg(test)]
mod tests {
    use super::{Compositor, InvalidVersion, Unmet, VersionReply};
    use crate::command::input::{DragLock, Input, InputSelector, InputType};
    use crate::command::{behavior, effects};
    use crate::version::{SwayFxVersion, SwayVersion};

    fn detect(json: &str) -> Result<Compositor, InvalidVersion> {
        Compositor::from_reply(&serde_json::from_str::<VersionReply>(json).unwrap())
    }

    const SWAY_1_10: Compositor = Compositor::Sway {
        version: SwayVersion::new(1, 10, 1),
    };
    const SWAYFX_0_5: Compositor = Compositor::SwayFx {
        version: SwayFxVersion::new(0, 5, 0),
        base: SwayVersion::new(1, 10, 1),
    };

    #[test]
    fn sway_and_swayfx_are_told_apart() {
        // Replies of sway 1.10.1 (Debian 13), a development build and SwayFX 0.5.3.
        let sway = detect(
            r#"{"human_readable": "1.10.1", "variant": "sway", "major": 1, "minor": 10,
                "patch": 1, "loaded_config_file_name": "/home/me/.config/sway/config"}"#,
        )
        .unwrap();
        assert_eq!(sway, SWAY_1_10);
        assert_eq!(sway.sway_version(), SwayVersion::new(1, 10, 1));
        assert_eq!(sway.swayfx_version(), None);
        assert!(!sway.is_swayfx());
        assert_eq!(sway.to_string(), "sway 1.10.1");

        let development = detect(
            r#"{"human_readable": "1.12-dev-2a2a8ad (Nov  7 2026, branch 'master')",
                "variant": "sway", "major": 1, "minor": 12, "patch": 0,
                "loaded_config_file_name": ""}"#,
        )
        .unwrap();
        assert_eq!(development.to_string(), "sway 1.12");

        let swayfx = detect(
            r#"{"human_readable": "0.5.3", "sway_original_version": "1.11.0",
                "variant": "sway", "major": 0, "minor": 5, "patch": 3,
                "loaded_config_file_name": "/etc/sway/config"}"#,
        )
        .unwrap();
        assert_eq!(
            swayfx,
            Compositor::SwayFx {
                version: SwayFxVersion::new(0, 5, 3),
                base: SwayVersion::new(1, 11, 0),
            }
        );
        assert_eq!(swayfx.sway_version(), SwayVersion::new(1, 11, 0));
        assert_eq!(swayfx.swayfx_version(), Some(SwayFxVersion::new(0, 5, 3)));
        assert!(swayfx.is_swayfx());
        assert_eq!(swayfx.to_string(), "SwayFX 0.5.3 (based on sway 1.11)");
    }

    #[test]
    fn the_installed_compositor_is_read_from_sway_version() {
        let read = Compositor::from_version_output;
        assert_eq!(read("sway version 1.10.1\n"), Some(SWAY_1_10));
        assert_eq!(
            read("sway version 1.12"),
            Some(Compositor::Sway {
                version: SwayVersion::new(1, 12, 0)
            })
        );
        assert_eq!(
            read("sway version 1.12-dev-2a2a8ad (Nov  7 2026, branch 'master')\n"),
            Some(Compositor::Sway {
                version: SwayVersion::new(1, 12, 0)
            })
        );
        assert_eq!(
            read("swayfx version 0.6 (based on sway 1.12.0)\n"),
            Some(Compositor::SwayFx {
                version: SwayFxVersion::new(0, 6, 0),
                base: SwayVersion::new(1, 12, 0)
            })
        );
        assert_eq!(
            read("swayfx version 0.5.3-1a2b3c4 (Nov  7 2026) (based on sway version 1.11.0)"),
            Some(Compositor::SwayFx {
                version: SwayFxVersion::new(0, 5, 3),
                base: SwayVersion::new(1, 11, 0)
            })
        );
        for output in [
            "",
            "sway",
            "sway version",
            "sway version dev",
            "swayfx version 0.6",
            "swayfx version 0.6 (based on sway )",
            "i3 version 4.23",
        ] {
            assert_eq!(read(output), None, "{output:?}");
        }
    }

    #[test]
    fn broken_version_replies_are_rejected() {
        for json in [
            r#"{"human_readable": "", "major": -1, "minor": 10, "patch": 0,
                "loaded_config_file_name": ""}"#,
            r#"{"human_readable": "", "major": 1, "minor": 70000, "patch": 0,
                "loaded_config_file_name": ""}"#,
            r#"{"human_readable": "", "sway_original_version": "unknown", "major": 0,
                "minor": 5, "patch": 0, "loaded_config_file_name": ""}"#,
        ] {
            assert_eq!(detect(json), Err(InvalidVersion), "{json}");
        }
    }

    #[test]
    fn old_compositors_are_not_supported() {
        let sway = |minor, patch| Compositor::Sway {
            version: SwayVersion::new(1, minor, patch),
        };
        assert_eq!(sway(10, 0).check_supported(), Ok(()));
        assert_eq!(sway(12, 0).check_supported(), Ok(()));
        assert_eq!(
            sway(9, 0).check_supported(),
            Err(Unmet::Sway {
                needed: SwayVersion::OLDEST_SUPPORTED
            })
        );
        assert_eq!(SWAYFX_0_5.check_supported(), Ok(()));
        let old_swayfx = Compositor::SwayFx {
            version: SwayFxVersion::new(0, 4, 0),
            base: SwayVersion::new(1, 9, 0),
        };
        let unmet = old_swayfx.check_supported().unwrap_err();
        assert_eq!(
            unmet,
            Unmet::SwayFx {
                needed: SwayFxVersion::OLDEST_SUPPORTED
            }
        );
        assert_eq!(unmet.to_string(), "requires SwayFX 0.5 or newer");
        assert_eq!(
            sway(9, 0).check_supported().unwrap_err().to_string(),
            "requires sway 1.10 or newer"
        );
    }

    #[test]
    fn commands_are_checked_against_the_compositor() {
        let touchpad = Input::new(InputSelector::Type(InputType::Touchpad));
        let sticky = touchpad.drag_lock(DragLock::EnabledSticky);
        let rounded = effects::corner_radius(8).unwrap();
        let animated = effects::animation_duration_ms(200).unwrap();
        let plain = behavior::tiling_drag(true);
        let sway_1_11 = Compositor::Sway {
            version: SwayVersion::new(1, 11, 0),
        };
        let swayfx_0_6 = Compositor::SwayFx {
            version: SwayFxVersion::new(0, 6, 0),
            base: SwayVersion::new(1, 12, 0),
        };

        for compositor in [SWAY_1_10, sway_1_11, SWAYFX_0_5, swayfx_0_6] {
            assert_eq!(compositor.supports(&plain), Ok(()));
        }
        // A value that needs a newer sway; SwayFX counts by the sway it is based on.
        let needs_1_11 = Err(Unmet::Sway {
            needed: SwayVersion::new(1, 11, 0),
        });
        assert_eq!(SWAY_1_10.supports(&sticky), needs_1_11);
        assert_eq!(SWAYFX_0_5.supports(&sticky), needs_1_11);
        assert_eq!(sway_1_11.supports(&sticky), Ok(()));
        assert_eq!(swayfx_0_6.supports(&sticky), Ok(()));
        // SwayFX directives.
        assert_eq!(
            sway_1_11.supports(&rounded),
            Err(Unmet::SwayFx {
                needed: SwayFxVersion::new(0, 5, 0)
            })
        );
        assert_eq!(SWAYFX_0_5.supports(&rounded), Ok(()));
        assert_eq!(
            SWAYFX_0_5.supports(&animated),
            Err(Unmet::SwayFx {
                needed: SwayFxVersion::new(0, 6, 0)
            })
        );
        assert_eq!(swayfx_0_6.supports(&animated), Ok(()));
    }
}
