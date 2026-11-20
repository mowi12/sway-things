// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The modes an output offers, grouped as the Displays page offers them: a resolution per size,
//! then the refresh rates of the chosen one. The page only adds the labels.

use super::Output;
use crate::command::output::Mode;

/// The modes of `output`, and `current` and the mode in use if it doesn't list them, largest
/// and fastest first.
pub fn known(output: Option<&Output>, current: Option<Mode>) -> Vec<Mode> {
    let mut modes: Vec<Mode> = output
        .map(|output| output.modes().to_vec())
        .unwrap_or_default();
    let extra = current.into_iter().chain(output.and_then(Output::mode));
    for mode in extra {
        let listed = modes
            .iter()
            .any(|known| known.to_string() == mode.to_string());
        if !listed {
            modes.push(mode);
        }
    }
    modes.sort_by_key(|mode| {
        std::cmp::Reverse((
            u64::from(mode.width()) * u64::from(mode.height()),
            mode.width(),
            mode.refresh().unwrap_or(0),
        ))
    });
    modes
}

/// A mode per size `output` offers, largest first, each the one with the highest refresh rate;
/// the size of `current` is `current` itself, so the entry in use is the mode in use.
pub fn sizes(output: Option<&Output>, current: Option<Mode>) -> Vec<Mode> {
    let modes = known(output, current);
    let mut sizes: Vec<(u32, u32)> = Vec::new();
    for mode in &modes {
        if !sizes.contains(&(mode.width(), mode.height())) {
            sizes.push((mode.width(), mode.height()));
        }
    }
    sizes
        .into_iter()
        .filter_map(|(width, height)| {
            current
                .filter(|mode| (mode.width(), mode.height()) == (width, height))
                .or_else(|| {
                    modes
                        .iter()
                        .copied()
                        .filter(|mode| (mode.width(), mode.height()) == (width, height))
                        .max_by_key(|mode| mode.refresh().unwrap_or(0))
                })
        })
        .collect()
}

/// The modes of the size of `current` on `output`, one per refresh rate, highest first.
pub fn rates(output: Option<&Output>, current: Mode) -> Vec<Mode> {
    let mut rates: Vec<Mode> = Vec::new();
    for known in known(output, Some(current)) {
        let same_size = (known.width(), known.height()) == (current.width(), current.height());
        if same_size && !rates.iter().any(|rate| rate.refresh() == known.refresh()) {
            rates.push(known);
        }
    }
    rates.sort_by_key(|rate| std::cmp::Reverse(rate.refresh().unwrap_or(0)));
    rates
}
