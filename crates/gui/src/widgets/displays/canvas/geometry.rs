// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The geometry of the canvas, without GTK: where an output is parked, keeping the arrangement
//! together, scaling with the grip and making room for a slot.

use sway_things_core::command::output::Transform;
use sway_things_core::outputs::{Output, Rect, layout};

use super::{Drag, Layout, Shown};
use crate::i18n::{msg, tr, trf};

/// The scale for the grip of a tile `start` (logical pixels) at `from` moved by `dx`, `dy`
/// logical pixels: the movement along the tile's diagonal resizes it, keeping its proportions as
/// a scale does; across the diagonal it does nothing. Common scales (100 % to 200 % in quarters)
/// catch it within 4 %; else it goes in steps of 5 %, from 50 % to 300 % as the row allows.
pub(super) fn scale_for_grip(start: Rect, from: f64, dx: i32, dy: i32) -> f64 {
    let (width, height) = (
        f64::from(start.width.max(1)),
        f64::from(start.height.max(1)),
    );
    let along =
        (f64::from(dx) * width + f64::from(dy) * height) / (width * width + height * height);
    let grown = (1.0 + along).max(0.05);
    let scale = (from / grown).clamp(0.5, 3.0);
    let common = [1.0, 1.25, 1.5, 1.75, 2.0]
        .into_iter()
        .find(|common| (scale - common).abs() <= 0.04);
    common.unwrap_or_else(|| (scale * 20.0).round() / 20.0)
}

/// A logical length of `length` at `from` at the scale `to`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a length of a few thousand pixels times a ratio of scales between 0.5 and 3 fits"
)]
pub(super) fn resized(length: i32, from: f64, to: f64) -> i32 {
    ((f64::from(length) * from / to).round() as i32).max(1)
}

/// The tile of `drag`, dragged by its grip, at `scale` (from `from`), and the others of `all`
/// where they go once the new size is in effect.
pub(super) fn scale_with_grip(drag: &mut Drag, all: &[(String, Rect)], from: f64, scale: f64) {
    drag.scaling = Some((from, scale));
    drag.rect = Rect {
        width: resized(drag.start.width, from, scale),
        height: resized(drag.start.height, from, scale),
        ..drag.start
    };
    drag.room = grip_room(all, drag);
}

/// Where the others of `all` go while the tile of `drag` is scaled with its grip: flowed along
/// with its new size, so what touched still touches ([`keep_together`]).
pub(super) fn grip_room(all: &[(String, Rect)], drag: &Drag) -> Vec<(String, Rect)> {
    let with = |rect: Rect| -> Layout {
        all.iter()
            .map(|(id, shown)| {
                let rect = if *id == drag.identifier { rect } else { *shown };
                (id.clone(), rect)
            })
            .collect()
    };
    keep_together(&with(drag.start), &with(drag.rect))
        .map(|(_, arranged)| {
            arranged
                .into_iter()
                .filter(|(id, _)| *id != drag.identifier)
                .collect()
        })
        .unwrap_or_default()
}

/// Notes the slot between two of `others` (by identifier in `ids`) that `drag`, moved to `moved`,
/// is over now, and where the others go to make room for it. Says whether the slot changed: the
/// others move only then, so they don't shift with every motion of the pointer.
pub(super) fn take_slot(drag: &mut Drag, moved: Rect, ids: &[String], others: &[Rect]) -> bool {
    let start = drag.start;
    // Only a slot it can go into: the marker never promises what the drop doesn't do.
    let slot = layout::slot_at(moved, others, drag.slot)
        .filter(|slot| layout::insert(moved, start, others, *slot).is_some());
    if slot == drag.slot {
        return false;
    }
    drag.slot = slot;
    drag.room = match slot {
        Some(slot) => {
            let (_, made_room) = layout::place(moved, start, others, Some(slot));
            ids.iter().cloned().zip(made_room).collect()
        }
        None => Vec::new(),
    };
    true
}

/// Parks the outputs of `shown` that are off in a column right of those that are on.
pub(super) fn park(shown: &mut [Shown]) {
    let right = shown
        .iter()
        .filter_map(|shown| shown.rect)
        .map(Rect::right)
        .max()
        .unwrap_or(0);
    let top = shown
        .iter()
        .filter_map(|shown| shown.rect)
        .map(|rect| rect.y)
        .min()
        .unwrap_or(0);
    let gap = shown.iter().map(|shown| shown.size.0).max().unwrap_or(0) / 8;
    let mut y = top;
    for off in shown.iter_mut().filter(|shown| shown.rect.is_none()) {
        off.parked = Some(Rect {
            x: right.saturating_add(gap),
            y,
            width: off.size.0,
            height: off.size.1,
        });
        y = y.saturating_add(off.size.1).saturating_add(gap);
    }
}

/// The second line of a tile: the size of the mode and a turn, or that the output is off.
pub(super) fn details(output: &Output) -> String {
    if !output.is_enabled() {
        return tr(msg!("Off")).to_owned();
    }
    let Some(mode) = output.mode() else {
        return String::new();
    };
    let turned = output
        .transform()
        .filter(|transform| *transform != Transform::Normal)
        .map(|transform| format!(" \u{b7} {}", turn(transform)))
        .unwrap_or_default();
    let scaled = output
        .scale()
        .filter(|scale| (scale - 1.0).abs() > 0.001)
        .map(|scale| format!(" \u{b7} {}", percent(scale)))
        .unwrap_or_default();
    format!("{} \u{d7} {}{scaled}{turned}", mode.width(), mode.height())
}

/// A scale as a percentage, as people read it: _150 %_.
pub(super) fn percent(scale: f64) -> String {
    trf(
        // TRANSLATORS: A display's scale as a percentage, such as 150 %.
        msg!("{percent}\u{a0}%"),
        &[("percent", &format!("{:.0}", scale * 100.0))],
    )
}

/// A transform as people read it.
pub(super) fn turn(transform: Transform) -> &'static str {
    match transform {
        Transform::Normal => "",
        Transform::R90 => "90\u{b0}",
        Transform::R180 => "180\u{b0}",
        Transform::R270 => "270\u{b0}",
        Transform::Flipped => tr(msg!("Flipped")),
        Transform::Flipped90 => tr(msg!("Flipped, 90\u{b0}")),
        Transform::Flipped180 => tr(msg!("Flipped, 180\u{b0}")),
        Transform::Flipped270 => tr(msg!("Flipped, 270\u{b0}")),
    }
}

/// The size an output that is off is shown with: its largest mode, or a common one.
pub(super) fn parked_size(output: &Output) -> (i32, i32) {
    output
        .modes()
        .iter()
        .max_by_key(|mode| u64::from(mode.width()) * u64::from(mode.height()))
        .map_or((1920, 1080), |mode| {
            layout::logical_size(*mode, 1.0, Transform::Normal)
        })
}

/// The outputs of `after` that changed their size since `before`, and the layout `after` flowed
/// so that what touched still touches ([`layout::reflow`]). `None` unless the same outputs are
/// on, only sizes changed, and the layout had neither gaps nor overlaps before but has now.
pub(super) fn keep_together(
    before: &[(String, Rect)],
    after: &[(String, Rect)],
) -> Option<(Vec<String>, Layout)> {
    let same = before.len() == after.len()
        && before
            .iter()
            .zip(after)
            .all(|((old, _), (new, _))| old == new);
    if !same {
        return None;
    }
    let resized: Vec<String> = before
        .iter()
        .zip(after)
        .filter(|((_, old), (_, new))| (old.width, old.height) != (new.width, new.height))
        .map(|((id, _), _)| id.clone())
        .collect();
    if resized.is_empty() {
        return None;
    }
    let old: Vec<Rect> = before.iter().map(|(_, rect)| *rect).collect();
    let new: Vec<Rect> = after.iter().map(|(_, rect)| *rect).collect();
    let flowed = layout::reflow(&old, &new)?;
    let arranged = after.iter().map(|(id, _)| id.clone()).zip(flowed).collect();
    Some((resized, arranged))
}

#[cfg(test)]
mod tests {
    use super::{resized, scale_for_grip};
    use sway_things_core::outputs::Rect;

    const TILE: Rect = Rect {
        x: 0,
        y: 0,
        width: 1600,
        height: 900,
    };

    #[test]
    fn the_grip_scales_along_the_diagonal() {
        // Not moved: the scale it had.
        assert!((scale_for_grip(TILE, 1.0, 0, 0) - 1.0).abs() < 1e-9);
        // A third smaller along the diagonal: 150 %.
        let scale = scale_for_grip(TILE, 1.0, -533, -300);
        assert!((scale - 1.5).abs() < 1e-9, "{scale}");
        // Across the diagonal: nothing.
        let scale = scale_for_grip(TILE, 1.0, 300, -533);
        assert!((scale - 1.0).abs() < 1e-9, "{scale}");
        // Near a common scale it catches; else steps of 5 %.
        let scale = scale_for_grip(TILE, 1.0, -330, -186);
        assert!((scale - 1.25).abs() < 1e-9, "{scale}");
        let scale = scale_for_grip(TILE, 1.0, -380, -214);
        assert!((scale - 1.3).abs() < 1e-9, "{scale}");
        // Within 50 % and 300 %.
        assert!((scale_for_grip(TILE, 1.0, 5000, 5000) - 0.5).abs() < 1e-9);
        assert!((scale_for_grip(TILE, 1.0, -1590, -895) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_length_follows_the_scale() {
        assert_eq!(resized(2560, 1.0, 2.0), 1280);
        assert_eq!(resized(1280, 2.0, 1.0), 2560);
        assert_eq!(resized(1, 1.0, 3.0), 1);
    }
}
