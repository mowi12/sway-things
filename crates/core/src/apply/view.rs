// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The value in effect for a row and for `sway-things get`: the view of the stored values
//! ([`SettingView`]) with what is in effect besides the settings files, a risky change that
//! waits to be kept and the displays kanshi sets up. One function, so the app and the command
//! line show the same.

use super::Pending;
use crate::outputs::{Outputs, POSITION};
use crate::registry::{Setting, Value, Values};
use crate::state::State;
use crate::view::SettingView;

/// What is in effect besides the settings files.
#[derive(Debug, Clone, Copy, Default)]
pub struct InEffect<'a> {
    /// A risky change that waits to be kept or reverted: not stored, but in effect.
    pub pending: Option<&'a Pending>,
    /// While kanshi sets up the displays, the outputs: no line of ours is in the sway config
    /// meanwhile, and a value for every display is stored for each connected one.
    pub kanshi: Option<&'a Outputs>,
}

/// `view` of `setting` (for `device`, or the whole type) with what `effect` puts in effect:
///
/// - the value a waiting risky change tries, and the config line it would make;
/// - the place a display got along with that change;
/// - while kanshi sets up the displays, no config line for a per-output setting, and for every
///   display the value the connected ones share, if they all have the same.
pub fn in_effect(
    mut view: SettingView,
    setting: &'static Setting,
    device: Option<&str>,
    state: &State,
    effect: &InEffect<'_>,
) -> SettingView {
    let pending = effect.pending.filter(|pending| pending.device() == device);
    if let Some((pending, value)) = pending.and_then(|pending| {
        let tried = pending.values().find(|tried| tried.key == setting.key())?;
        Some((pending, &tried.value))
    }) {
        view.value = value.clone();
        let tried = Tried {
            values: pending
                .values()
                .map(|tried| (tried.key, &tried.value))
                .collect(),
            under: state.values(device),
        };
        if setting.sway().is_some() {
            view.line = state.registry().config_line(setting, &tried, device).ok();
        }
    }
    if setting.key() == POSITION {
        let along = effect
            .pending
            .zip(device)
            .and_then(|(pending, device)| pending.along(device));
        if let Some(place) = along {
            view.value = place.clone();
        }
    }
    if let Some(outputs) = effect.kanshi {
        if setting.sets_up_display() {
            view.line = None;
            if device.is_none() {
                let mut each = outputs
                    .iter()
                    .filter(|output| output.is_connected() && output.is_enabled())
                    .map(|output| state.get(setting.key(), Some(output.identifier())));
                if let Some(Some(first)) = each.next() {
                    if each.all(|value| value == Some(first)) {
                        view.value = first.clone();
                    }
                }
            }
        }
    }
    view
}

/// The tried values of a risky change, and the stored values for the other keys.
struct Tried<'a, V> {
    values: Vec<(&'static str, &'a Value)>,
    under: V,
}

impl<V: Values> Values for Tried<'_, V> {
    fn get(&self, key: &str) -> Option<&Value> {
        self.values
            .iter()
            .find(|(tried, _)| *tried == key)
            .map(|(_, value)| *value)
            .or_else(|| self.under.get(key))
    }
}

#[cfg(test)]
mod tests {
    use super::{InEffect, in_effect};
    use crate::outputs::{Outputs, SCALE};
    use crate::registry::{Registry, Value};
    use crate::state::{Paths, State};
    use crate::view::{SettingView, Source};

    const TWO: &str = r#"[
      {"name": "DP-1", "active": true, "primary": false, "make": "A", "model": "B",
       "serial": "C", "modes": []},
      {"name": "DP-2", "active": true, "primary": false, "make": "D", "model": "E",
       "serial": "F", "modes": []}
    ]"#;

    fn default_view() -> SettingView {
        SettingView {
            value: Value::Decimal(1.0),
            source: Source::Default,
            line: Some("output * scale 1".to_owned()),
            locked: None,
            changed_elsewhere: None,
        }
    }

    #[test]
    fn while_kanshi_sets_up_the_displays_every_display_shows_their_shared_value_and_no_line() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.toml"),
            "[display.\"A B C\".output]\nscale = 2.0\n[display.\"D E F\".output]\nscale = 2.0\n",
        )
        .unwrap();
        let state = State::load(
            Registry::builtin(),
            Paths::new(dir.path(), dir.path(), "host").unwrap(),
        );
        let setting = Registry::builtin().get(SCALE).unwrap();
        let found: Vec<crate::ipc::types::Output> = serde_json::from_str(TWO).unwrap();
        let outputs = Outputs::new(&found);

        // Without kanshi: the view as it is.
        let view = in_effect(default_view(), setting, None, &state, &InEffect::default());
        assert_eq!(view.value, Value::Decimal(1.0));
        assert!(view.line.is_some());

        // With kanshi: the value both displays have, and no line of ours in the sway config.
        let kanshi = InEffect {
            pending: None,
            kanshi: Some(&outputs),
        };
        let view = in_effect(default_view(), setting, None, &state, &kanshi);
        assert_eq!(view.value, Value::Decimal(2.0));
        assert_eq!(view.line, None);

        // Different values: nothing they share to show.
        std::fs::write(
            dir.path().join("settings.toml"),
            "[display.\"A B C\".output]\nscale = 2.0\n[display.\"D E F\".output]\nscale = 1.5\n",
        )
        .unwrap();
        let state = State::load(
            Registry::builtin(),
            Paths::new(dir.path(), dir.path(), "host").unwrap(),
        );
        let view = in_effect(default_view(), setting, None, &state, &kanshi);
        assert_eq!(view.value, Value::Decimal(1.0));
    }
}
