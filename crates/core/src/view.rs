// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a row of the app shows for a setting: the value in effect, where it comes from, the line
//! of the sway config that sets it, and why it can't be changed, if it can't.
//!
//! The value in effect is the one Sway Things stores; without one, the value the user's own config
//! sets; without that, sway's default. A row and the command line show the same, because both
//! come from [`SettingView::of`].
//!
//! ```
//! use std::path::Path;
//! use sway_things_core::generate::Locations;
//! use sway_things_core::include::IncludeStatus;
//! use sway_things_core::ipc::Compositor;
//! use sway_things_core::registry::{Registry, Value};
//! use sway_things_core::state::{Paths, State};
//! use sway_things_core::sway_config::MemoryFiles;
//! use sway_things_core::version::SwayVersion;
//! use sway_things_core::view::{SettingView, Source};
//!
//! # let dir = tempfile::tempdir()?;
//! let state = State::load(Registry::builtin(), Paths::new(dir.path(), dir.path(), "host")?);
//! let locations = Locations::new(&dir.path().join("sway-things"), dir.path());
//! let files = MemoryFiles::new().file("/config", "gaps inner 6\n");
//! let status = IncludeStatus::detect(Some(Path::new("/config")), &files, &locations);
//! let sway = Compositor::Sway { version: SwayVersion::new(1, 10, 1) };
//!
//! let gaps = Registry::builtin().get("windows.gaps.inner").ok_or("no such setting")?;
//! let view = SettingView::of(gaps, None, &state, &status, sway);
//! assert_eq!(view.value, Value::Integer(6));
//! assert!(matches!(view.source, Source::Config { line: 1, .. }));
//! assert_eq!(view.line.as_deref(), Some("gaps inner 6"));
//! // The config doesn't include the generated files, so nothing can be changed yet.
//! assert!(view.locked.is_some());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::command::input::InputSelector;
use crate::conflict::Report;
use crate::include::IncludeStatus;
use crate::ipc::{Compositor, Unmet};
use crate::registry::{Setting, Value, Values};
use crate::state::{Layer, State};

/// Where the value a row shows comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Sway Things stores it, in this file. `for_type` if it is the value of the device's input
    /// type, shown for a device that has none of its own.
    Stored {
        /// The settings file.
        layer: Layer,
        /// Whether it is the value of the input type.
        for_type: bool,
    },
    /// The user's own config sets it, at this line.
    Config {
        /// The file.
        file: PathBuf,
        /// The number of the line, counted from 1.
        line: usize,
    },
    /// Nothing sets it: sway's default, or the default Sway Things writes for a setting whose
    /// directive it stores other values of.
    Default,
}

/// Why a setting can't be changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locked {
    /// The user's config doesn't include the generated files (or not all of them); the text says
    /// what to do.
    ReadOnly(String),
    /// The running compositor doesn't support the setting.
    Unsupported(Unmet),
}

impl fmt::Display for Locked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The explanation is long and the same for every setting; the app shows it once.
            Self::ReadOnly(_) => f.write_str("Add the include line to change settings"),
            Self::Unsupported(Unmet::Sway { needed }) => write!(f, "Requires sway {needed}"),
            Self::Unsupported(Unmet::SwayFx { needed }) => write!(f, "Requires SwayFX {needed}"),
        }
    }
}

/// What a row shows for a setting.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingView {
    /// The value in effect.
    pub value: Value,
    /// Where it comes from.
    pub source: Source,
    /// The line of the sway config that sets it, as the app shows it (`pointer_accel 0.3`), or
    /// `None` for a preference of Sway Things itself.
    pub line: Option<String>,
    /// Why it can't be changed, or `None` if it can.
    pub locked: Option<Locked>,
}

impl SettingView {
    /// What a row shows for `setting`, for the whole session or input type, or for the input
    /// `device` with this identifier.
    pub fn of(
        setting: &'static Setting,
        device: Option<&str>,
        state: &State,
        status: &IncludeStatus,
        compositor: Compositor,
    ) -> Self {
        let registry = state.registry();
        let key = setting.key();
        let stored = state.values(device);
        let group: Vec<&Setting> = registry.group(setting).collect();
        let group_stored = group
            .iter()
            .any(|member| stored.get(member.key()).is_some());

        // What the user's config sets, for the settings of the directive, if Sway Things stores
        // none of them: one line of theirs sets all of them.
        let mut users = BTreeMap::new();
        let mut user_line = None;
        if !group_stored {
            if let Some(line) = Report::check(status, setting, device, None)
                .as_ref()
                .and_then(Report::user_value)
            {
                for member in &group {
                    if let Some(Ok(value)) = member.read(&line.directive) {
                        users.insert(member.key().to_owned(), value);
                    }
                }
                if users.contains_key(key) {
                    user_line = Some((line.file().to_owned(), line.line()));
                }
            }
        }

        let (value, source) = if let Some(value) = stored.get(key) {
            let own = state.layer(key, device);
            let layer = own
                .or_else(|| state.layer(key, None))
                .unwrap_or(Layer::Shared);
            let source = Source::Stored {
                layer,
                for_type: own.is_none(),
            };
            (value.clone(), source)
        } else if let (Some(value), Some((file, line))) = (users.get(key), user_line) {
            (value.clone(), Source::Config { file, line })
        } else {
            (setting.default_value(), Source::Default)
        };

        let selector = device.and_then(|device| InputSelector::device(device).ok());
        let line = setting.sway().and_then(|_| {
            registry
                .config_line(
                    setting,
                    &Over {
                        over: &users,
                        under: &stored,
                    },
                    selector.as_ref(),
                )
                .ok()
        });

        let requirements = setting.requirements();
        let locked = match compositor.check(requirements.min_sway, requirements.min_swayfx) {
            Err(unmet) => Some(Locked::Unsupported(unmet)),
            Ok(()) => status
                .write_access()
                .err()
                .map(|read_only| Locked::ReadOnly(read_only.explanation().to_owned())),
        };

        Self {
            value,
            source,
            line,
            locked,
        }
    }
}

/// The values in `over`, and those of `under` for the other keys.
struct Over<'a, V> {
    over: &'a BTreeMap<String, Value>,
    under: &'a V,
}

impl<V: Values> Values for Over<'_, V> {
    fn get(&self, key: &str) -> Option<&Value> {
        self.over.get(key).or_else(|| self.under.get(key))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Locked, SettingView, Source};
    use crate::command::Color;
    use crate::generate::Locations;
    use crate::include::IncludeStatus;
    use crate::ipc::{Compositor, Unmet};
    use crate::registry::{Registry, Value};
    use crate::state::{Layer, State};
    use crate::sway_config::MemoryFiles;
    use crate::version::{SwayFxVersion, SwayVersion};

    const MAIN: &str = "/home/me/.config/sway/config";
    const INCLUDE: &str = "include ~/.config/sway/sway-things/*.conf\n";
    const SWAY: Compositor = Compositor::Sway {
        version: SwayVersion::new(1, 10, 1),
    };
    const TOUCHPAD: &str = "1267:12377:ELAN1300:00_04F3:3059_Touchpad";

    struct Setup {
        _dir: tempfile::TempDir,
        state: State,
    }

    fn setup(shared: &str) -> Setup {
        let (dir, state) = crate::generate::tests::state(shared, "version = 1\n");
        Setup { _dir: dir, state }
    }

    fn status(main: &str) -> IncludeStatus {
        let locations = Locations::new(
            Path::new("/home/me/.config/sway/sway-things"),
            Path::new("/home/me/.local/state/sway-things"),
        );
        let files = MemoryFiles::new().var("HOME", "/home/me").file(MAIN, main);
        IncludeStatus::detect(Some(Path::new(MAIN)), &files, &locations)
    }

    fn view(setup: &Setup, key: &str, device: Option<&str>, main: &str) -> SettingView {
        let setting = Registry::builtin().get(key).unwrap();
        SettingView::of(setting, device, &setup.state, &status(main), SWAY)
    }

    #[test]
    fn a_stored_value_wins_over_the_users_config() {
        let setup = setup("version = 1\n\n[windows.gaps]\ninner = 8\n");
        let view = view(
            &setup,
            "windows.gaps.inner",
            None,
            &format!("gaps inner 4\n{INCLUDE}"),
        );
        assert_eq!(view.value, Value::Integer(8));
        assert_eq!(
            view.source,
            Source::Stored {
                layer: Layer::Shared,
                for_type: false
            }
        );
        assert_eq!(view.line.as_deref(), Some("gaps inner 8"));
        assert_eq!(view.locked, None);
    }

    #[test]
    fn without_a_stored_value_the_users_config_and_then_the_default_show() {
        let setup = setup("version = 1\n");
        let view = view(
            &setup,
            "input.touchpad.tap",
            None,
            &format!("{INCLUDE}input type:touchpad tap enabled\n"),
        );
        assert_eq!(view.value, Value::Bool(true));
        assert_eq!(
            view.source,
            Source::Config {
                file: MAIN.into(),
                line: 2
            }
        );
        assert_eq!(view.line.as_deref(), Some("tap enabled"));

        let view = super::tests::view(&setup, "input.touchpad.tap", None, INCLUDE);
        assert_eq!(
            (view.value, view.source),
            (Value::Bool(false), Source::Default)
        );
        assert_eq!(view.line.as_deref(), Some("tap disabled"));
    }

    #[test]
    fn a_users_client_line_shows_for_every_color_unless_one_is_stored() {
        let main = format!("client.focused #111111 #222222 #333333\n{INCLUDE}");
        let setup = setup("version = 1\n");
        let view = view(&setup, "windows.focused.child_border", None, &main);
        assert_eq!(view.value, Value::Color(Color::rgb(0x22, 0x22, 0x22)));
        assert_eq!(
            view.line.as_deref(),
            Some("client.focused #111111 #222222 #333333 #2e9ef4 #222222")
        );
        // With one color stored, Sway Things writes the whole line: the others are its defaults.
        let setup = super::tests::setup("version = 1\n\n[windows.focused]\ntext = \"#000000\"\n");
        let view = super::tests::view(&setup, "windows.focused.child_border", None, &main);
        assert_eq!(view.value, Value::Color(Color::rgb(0x28, 0x55, 0x77)));
        assert_eq!(view.source, Source::Default);
        assert_eq!(
            view.line.as_deref(),
            Some("client.focused #4c7899 #285577 #000000 #2e9ef4 #285577")
        );
    }

    #[test]
    fn a_device_shows_its_own_value_or_that_of_its_type() {
        let setup = setup(&format!(
            "version = 1\n\n[input.touchpad]\ntap = true\npointer_accel = 0.5\n\n\
             [device.\"{TOUCHPAD}\".input.touchpad]\npointer_accel = -0.2\n"
        ));
        let accel = view(
            &setup,
            "input.touchpad.pointer_accel",
            Some(TOUCHPAD),
            INCLUDE,
        );
        assert_eq!(accel.value, Value::Decimal(-0.2));
        assert!(matches!(
            accel.source,
            Source::Stored {
                for_type: false,
                ..
            }
        ));
        let tap = view(&setup, "input.touchpad.tap", Some(TOUCHPAD), INCLUDE);
        assert_eq!(tap.value, Value::Bool(true));
        assert!(matches!(tap.source, Source::Stored { for_type: true, .. }));
    }

    #[test]
    fn rows_are_locked_without_the_include_and_on_a_compositor_too_old() {
        let setup = setup("version = 1\n");
        let view = view(&setup, "windows.gaps.inner", None, "gaps inner 4\n");
        assert!(matches!(view.locked, Some(Locked::ReadOnly(_))));
        assert_eq!(
            view.locked.as_ref().map(ToString::to_string).as_deref(),
            Some("Add the include line to change settings")
        );
        // The value still shows.
        assert_eq!(view.value, Value::Integer(4));

        let radius = Registry::builtin().get("effects.corner_radius").unwrap();
        let view = SettingView::of(radius, None, &setup.state, &status(INCLUDE), SWAY);
        let locked = view.locked.unwrap();
        assert_eq!(
            locked,
            Locked::Unsupported(Unmet::SwayFx {
                needed: SwayFxVersion::OLDEST_SUPPORTED
            })
        );
        assert_eq!(locked.to_string(), "Requires SwayFX 0.5");
    }

    #[test]
    fn preferences_of_sway_things_have_no_line() {
        let setup = setup("version = 1\n\n[app]\nsafety_net_timeout = 30\n");
        let view = view(&setup, "app.safety_net_timeout", None, INCLUDE);
        assert_eq!(view.value, Value::Integer(30));
        assert_eq!(view.line, None);
        assert_eq!(view.locked, None);
    }
}
