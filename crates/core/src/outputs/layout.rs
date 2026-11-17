// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The geometry of arranging outputs, for the canvas of the Displays page: no GTK, so it is tested
//! here.
//!
//! The layout is in logical pixels, as sway places outputs: an output's size is its mode, turned
//! by its transform and divided by its scale ([`logical_size`]). While an output is dragged,
//! [`snap`] pulls its edges and centers to those of the others when they come close, and lets go
//! again once the pointer moves on. When it is dropped, [`settle`] moves it to the nearest place
//! where it overlaps no other output and touches one, so the pointer can reach it: sway only moves
//! the pointer between outputs that share an edge. [`normalize`] moves the whole layout so that it
//! starts at 0,0.
//!
//! ```
//! use sway_things_core::outputs::Rect;
//! use sway_things_core::outputs::layout::{settle, snap};
//!
//! let left = Rect { x: 0, y: 0, width: 2560, height: 1440 };
//! // Dragged to a few pixels off the right edge, the output snaps to it.
//! let dragged = Rect { x: 2570, y: 6, width: 1920, height: 1080 };
//! let snapped = snap(dragged, &[left], 16);
//! assert_eq!((snapped.rect.x, snapped.rect.y), (2560, 0));
//! // Dropped mostly on top of the other output, it moves next to its nearest edge.
//! let dropped = Rect { x: 2000, y: 100, width: 1920, height: 1080 };
//! assert_eq!(settle(dropped, &[left]), Rect { x: 2560, ..dropped });
//! ```

use super::Rect;
use crate::command::output::{Mode, Transform};

/// A line the canvas draws while a dragged output is snapped to it: where its edge or center
/// lines up with another output's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Guide {
    /// A vertical line at this x.
    Vertical(i32),
    /// A horizontal line at this y.
    Horizontal(i32),
}

/// Where [`snap`] put a dragged output, and the lines it lines up with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapped {
    /// The output's place.
    pub rect: Rect,
    /// The lines it lines up with: at most one per direction.
    pub guides: Vec<Guide>,
}

/// The size of an output in the layout: its mode, turned by its transform, divided by its scale
/// and rounded down, as wlroots computes it (`wlr_output_effective_resolution`), but at least one
/// pixel.
pub fn logical_size(mode: Mode, scale: f64, transform: Transform) -> (i32, i32) {
    let (width, height) = if transform.is_sideways() {
        (mode.height(), mode.width())
    } else {
        (mode.width(), mode.height())
    };
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "wlroots truncates the same way; a mode is far below i32::MAX"
    )]
    // At least one pixel, so every output has an area.
    let divide = |pixels: u32| ((f64::from(pixels) / scale) as i32).max(1);
    (divide(width), divide(height))
}

/// Pulls `moving` to the edges and centers of `others` that are within `threshold` logical pixels,
/// on each axis on its own: left and right edges to the others' left and right edges (side by side
/// or aligned), centers to centers, and the same for top and bottom. The nearest candidate wins;
/// beyond the threshold the output stays where the pointer put it, so dragging on lets go of the
/// edge again.
pub fn snap(moving: Rect, others: &[Rect], threshold: i32) -> Snapped {
    // The places on one axis: start at start, at end, end at start, end at end, center at center.
    let candidates = |start: i32, size: i32, own: i32| {
        let end = start.saturating_add(size);
        let center = start.saturating_add(size / 2);
        [
            (start, start),
            (end, end),
            (start.saturating_sub(own), start),
            (end.saturating_sub(own), end),
            (center.saturating_sub(own / 2), center),
        ]
    };
    let x = nearest(
        moving.x,
        others
            .iter()
            .flat_map(|other| candidates(other.x, other.width, moving.width)),
        threshold,
    );
    let y = nearest(
        moving.y,
        others
            .iter()
            .flat_map(|other| candidates(other.y, other.height, moving.height)),
        threshold,
    );
    let mut guides = Vec::new();
    let mut rect = moving;
    if let Some((position, line)) = x {
        rect.x = position;
        guides.push(Guide::Vertical(line));
    }
    if let Some((position, line)) = y {
        rect.y = position;
        guides.push(Guide::Horizontal(line));
    }
    Snapped { rect, guides }
}

/// The candidate `(position, line)` nearest to `position`, if one is within `threshold`.
fn nearest(
    position: i32,
    candidates: impl Iterator<Item = (i32, i32)>,
    threshold: i32,
) -> Option<(i32, i32)> {
    candidates
        .map(|(candidate, line)| {
            let distance = (i64::from(candidate) - i64::from(position)).abs();
            (distance, candidate, line)
        })
        .filter(|(distance, _, _)| *distance <= i64::from(threshold))
        .min_by_key(|(distance, _, _)| *distance)
        .map(|(_, candidate, line)| (candidate, line))
}

/// Whether a rectangle covers an area at all. An output always does; a rectangle without one
/// overlaps and touches nothing.
fn has_area(rect: Rect) -> bool {
    rect.width > 0 && rect.height > 0
}

/// Whether two rectangles cover a common area. Rectangles that only share an edge don't.
pub fn overlaps(a: Rect, b: Rect) -> bool {
    has_area(a)
        && has_area(b)
        && a.x < b.right()
        && b.x < a.right()
        && a.y < b.bottom()
        && b.y < a.bottom()
}

/// Whether two rectangles share a piece of an edge, so the pointer can move from one to the other.
/// Touching only at a corner doesn't count.
pub fn touches(a: Rect, b: Rect) -> bool {
    if !has_area(a) || !has_area(b) {
        return false;
    }
    let side_by_side =
        (a.right() == b.x || b.right() == a.x) && a.y < b.bottom() && b.y < a.bottom();
    let stacked = (a.bottom() == b.y || b.bottom() == a.y) && a.x < b.right() && b.x < a.right();
    side_by_side || stacked
}

/// The place for `moving`, dropped where it is, that overlaps none of `others` and touches at
/// least one of them, and where the whole layout hangs together if any such place does (moving
/// the output that joined two others away from them would cut them apart): where it is if that
/// is such a place, else the nearest place next to an edge of another output. Without others,
/// or for a rectangle without an area, where it is.
pub fn settle(moving: Rect, others: &[Rect]) -> Rect {
    let fits = |rect: Rect| {
        others.iter().all(|other| !overlaps(rect, *other))
            && others.iter().any(|other| touches(rect, *other))
    };
    let together = |rect: Rect| {
        let mut all: Vec<Rect> = others.to_vec();
        all.push(rect);
        is_connected(&all)
    };
    if others.is_empty() || !has_area(moving) || (fits(moving) && together(moving)) {
        return moving;
    }
    let distance = |rect: &Rect| {
        let dx = i128::from(rect.x) - i128::from(moving.x);
        let dy = i128::from(rect.y) - i128::from(moving.y);
        dx * dx + dy * dy
    };
    let candidates = candidates(moving, others);
    if let Some(best) = candidates
        .iter()
        .filter(|rect| fits(**rect) && together(**rect))
        .min_by_key(|rect| distance(rect))
    {
        return *best;
    }
    if fits(moving) {
        return moving;
    }
    candidates
        .into_iter()
        .filter(|rect| fits(*rect))
        .min_by_key(distance)
        // The side right of the output that reaches furthest right is always free, unless the
        // layout touches the end of sway's coordinates.
        .unwrap_or(moving)
}

/// The places next to each side of each of `others` for `moving`, as close to where it was
/// dropped as that side allows while the two share at least a quarter of the shorter edge, so
/// the pointer has somewhere to cross.
fn candidates(moving: Rect, others: &[Rect]) -> Vec<Rect> {
    let along = |position: i32, low: i32, high: i32| position.clamp(low, high.max(low));
    others
        .iter()
        .flat_map(|other| {
            let shared = |a: i32, b: i32| (a.min(b) / 4).max(1);
            let y_shared = shared(moving.height, other.height);
            let x_shared = shared(moving.width, other.width);
            let y = along(
                moving.y,
                other
                    .y
                    .saturating_sub(moving.height)
                    .saturating_add(y_shared),
                other.bottom().saturating_sub(y_shared),
            );
            let x = along(
                moving.x,
                other
                    .x
                    .saturating_sub(moving.width)
                    .saturating_add(x_shared),
                other.right().saturating_sub(x_shared),
            );
            [
                Rect {
                    x: other.x.saturating_sub(moving.width),
                    y,
                    ..moving
                },
                Rect {
                    x: other.right(),
                    y,
                    ..moving
                },
                Rect {
                    x,
                    y: other.y.saturating_sub(moving.height),
                    ..moving
                },
                Rect {
                    x,
                    y: other.bottom(),
                    ..moving
                },
            ]
        })
        .collect()
}

/// A place between two outputs that touch, where a dragged output can go: `before` is left of
/// (or above) `after`, as indexes into the outputs the drag sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// The output left of (or above) the place.
    pub before: usize,
    /// The output right of (or below) the place.
    pub after: usize,
    /// Whether the two stand one above the other rather than side by side.
    pub stacked: bool,
}

/// The slot between two of `others` that `moving` points at: its center near the edge the two
/// share, within their common span. `current`, the slot shown so far, stays while the center is
/// within half as far again, so the marker doesn't flicker on the edge of its zone.
pub fn slot_at(moving: Rect, others: &[Rect], current: Option<Slot>) -> Option<Slot> {
    if !has_area(moving) {
        return None;
    }
    let reach = |slot: Slot, stretch: i64| -> Option<i64> {
        let (a, b) = (others.get(slot.before)?, others.get(slot.after)?);
        let (a, b, moving) = if slot.stacked {
            (transposed(*a), transposed(*b), transposed(moving))
        } else {
            (*a, *b, moving)
        };
        if a.right() != b.x {
            return None;
        }
        let span = (a.y.max(b.y), a.bottom().min(b.bottom()));
        let center_x = i64::from(moving.x) * 2 + i64::from(moving.width);
        let center_y = i64::from(moving.y) * 2 + i64::from(moving.height);
        // Within the height the two share, and near their edge: a quarter of the narrowest of
        // the three, on either side (in half pixels, as the centers are).
        if center_y < i64::from(span.0) * 2 || center_y > i64::from(span.1) * 2 {
            return None;
        }
        let zone = i64::from(a.width.min(b.width).min(moving.width)) / 2 * stretch / 2;
        let distance = (center_x - i64::from(b.x) * 2).abs();
        (distance <= zone).then_some(distance)
    };
    if let Some(slot) = current.filter(|slot| reach(*slot, 3).is_some()) {
        return Some(slot);
    }
    let mut best: Option<(i64, Slot)> = None;
    for (before, a) in others.iter().enumerate() {
        for (after, b) in others.iter().enumerate() {
            for stacked in [false, true] {
                let touch = if stacked {
                    a.bottom() == b.y
                } else {
                    a.right() == b.x
                };
                if !touch {
                    continue;
                }
                let slot = Slot {
                    before,
                    after,
                    stacked,
                };
                if let Some(distance) = reach(slot, 2) {
                    if best.is_none_or(|(nearest, _)| distance < nearest) {
                        best = Some((distance, slot));
                    }
                }
            }
        }
    }
    best.map(|(_, slot)| slot)
}

/// `moving` put into `slot` between two of `others`: right after the first, its top lined up
/// with the nearer top of the two (its left with the nearer left, one above the other), and
/// everything from the second one on moved along by its size to make room. Then the gap where
/// `moving` came from is closed. The new place of `moving` and of each of `others`, in their
/// order; `None` if the layout wouldn't hang together without overlaps that way.
pub fn insert(moving: Rect, others: &[Rect], slot: Slot) -> Option<(Rect, Vec<Rect>)> {
    if slot.stacked {
        let turned: Vec<Rect> = others.iter().copied().map(transposed).collect();
        let (placed, moved) = insert_side_by_side(
            transposed(moving),
            &turned,
            Slot {
                stacked: false,
                ..slot
            },
        )?;
        Some((
            transposed(placed),
            moved.into_iter().map(transposed).collect(),
        ))
    } else {
        insert_side_by_side(moving, others, slot)
    }
}

/// [`insert`] for a slot between outputs side by side.
fn insert_side_by_side(moving: Rect, others: &[Rect], slot: Slot) -> Option<(Rect, Vec<Rect>)> {
    let (a, b) = (*others.get(slot.before)?, *others.get(slot.after)?);
    let y = if (moving.y - a.y).abs() <= (moving.y - b.y).abs() {
        a.y
    } else {
        b.y
    };
    let placed = Rect {
        x: a.right(),
        y,
        ..moving
    };
    let moved: Vec<Rect> = others
        .iter()
        .map(|rect| {
            if rect.x >= b.x {
                Rect {
                    x: rect.x.saturating_add(moving.width),
                    ..*rect
                }
            } else {
                *rect
            }
        })
        .collect();
    let (placed, moved) = close_gaps(placed, &moved);
    let mut all = moved.clone();
    all.push(placed);
    let fine = is_connected(&all)
        && all
            .iter()
            .enumerate()
            .all(|(index, a)| all.iter().skip(index + 1).all(|b| !overlaps(*a, *b)));
    fine.then_some((placed, moved))
}

/// `placed` and `others` with each gap between outputs side by side closed: from left to right,
/// an output whose left neighbors all end before it moves left to the nearest right edge.
fn close_gaps(placed: Rect, others: &[Rect]) -> (Rect, Vec<Rect>) {
    let mut all: Vec<Rect> = others.to_vec();
    all.push(placed);
    let mut order: Vec<usize> = (0..all.len()).collect();
    order.sort_by_key(|&index| all.get(index).map(|rect| (rect.x, rect.y)));
    for &index in &order {
        let Some(current) = all.get(index).copied() else {
            continue;
        };
        let edge = all
            .iter()
            .filter(|other| {
                other.right() <= current.x
                    && other.y < current.bottom()
                    && current.y < other.bottom()
            })
            .map(|other| other.right())
            .max();
        if let (Some(edge), Some(rect)) = (edge, all.get_mut(index)) {
            rect.x = edge;
        }
    }
    let placed = all.pop().unwrap_or(placed);
    (placed, all)
}

/// `rect` with x and y swapped, and width and height.
fn transposed(rect: Rect) -> Rect {
    Rect {
        x: rect.y,
        y: rect.x,
        width: rect.height,
        height: rect.width,
    }
}

/// The layout `before`, after some of its rectangles changed their size to those in `after`
/// (same order, same places): each rectangle moves by as much as the ones it stood against on
/// its left and above it grew or shrank, so what touched still touches, and the layout starts at
/// 0,0. `None` unless only sizes changed, `before` hung together without overlaps, `after`
/// doesn't, and the moved layout does.
pub fn reflow(before: &[Rect], after: &[Rect]) -> Option<Vec<Rect>> {
    let fine = |rects: &[Rect]| {
        is_connected(rects)
            && rects
                .iter()
                .enumerate()
                .all(|(index, a)| rects.iter().skip(index + 1).all(|b| !overlaps(*a, *b)))
    };
    let same_places = before.len() == after.len()
        && before
            .iter()
            .zip(after)
            .all(|(old, new)| (old.x, old.y) == (new.x, new.y));
    if !same_places || !fine(before) || fine(after) {
        return None;
    }
    let mut moved: Vec<Rect> = after.to_vec();
    // Left to right: each starts where the new right edges of those on its left end.
    let mut order: Vec<usize> = (0..before.len()).collect();
    order.sort_by_key(|&index| before.get(index).map(|rect| (rect.x, rect.y)));
    for &index in &order {
        let (Some(old), Some(current)) = (before.get(index).copied(), moved.get(index).copied())
        else {
            continue;
        };
        let x = before
            .iter()
            .enumerate()
            .filter(|(_, left)| {
                left.right() == old.x && left.y < old.bottom() && old.y < left.bottom()
            })
            .filter_map(|(other, _)| moved.get(other).map(|rect| rect.right()))
            .max();
        if let (Some(x), Some(rect)) = (x, moved.get_mut(index)) {
            *rect = Rect { x, ..current };
        }
    }
    // Top to bottom, the same.
    order.sort_by_key(|&index| before.get(index).map(|rect| (rect.y, rect.x)));
    for &index in &order {
        let (Some(old), Some(current)) = (before.get(index).copied(), moved.get(index).copied())
        else {
            continue;
        };
        let y = before
            .iter()
            .enumerate()
            .filter(|(_, above)| {
                above.bottom() == old.y && above.x < old.right() && old.x < above.right()
            })
            .filter_map(|(other, _)| moved.get(other).map(|rect| rect.bottom()))
            .max();
        if let (Some(y), Some(rect)) = (y, moved.get_mut(index)) {
            *rect = Rect { y, ..current };
        }
    }
    normalize(&mut moved);
    fine(&moved).then_some(moved)
}

/// Moves every rectangle by the same amount so that the leftmost starts at x 0 and the topmost
/// at y 0, as sway's own layout does.
pub fn normalize(rects: &mut [Rect]) {
    let Some(left) = rects.iter().map(|rect| rect.x).min() else {
        return;
    };
    let top = rects.iter().map(|rect| rect.y).min().unwrap_or(0);
    for rect in rects {
        rect.x = rect.x.saturating_sub(left);
        rect.y = rect.y.saturating_sub(top);
    }
}

/// Whether every rectangle can be reached from every other one through shared edges.
pub fn is_connected(rects: &[Rect]) -> bool {
    let Some(start) = rects.first() else {
        return true;
    };
    let mut reached = vec![false; rects.len()];
    if let Some(start_reached) = reached.first_mut() {
        *start_reached = true;
    }
    let mut stack = vec![*start];
    while let Some(current) = stack.pop() {
        for (index, rect) in rects.iter().enumerate() {
            if let Some(reached) = reached.get_mut(index) {
                if !*reached && touches(current, *rect) {
                    *reached = true;
                    stack.push(*rect);
                }
            }
        }
    }
    reached.into_iter().all(|reached| reached)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Guide, is_connected, logical_size, normalize, overlaps, settle, snap, touches};
    use crate::command::output::{Mode, Transform};
    use crate::outputs::Rect;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn logical_sizes_are_turned_and_scaled_as_wlroots_does() {
        let mode = Mode::new(1920, 1080, Some(60_010)).unwrap();
        assert_eq!(logical_size(mode, 1.0, Transform::Normal), (1920, 1080));
        // The laptop panel at 1.3: 1476 x 830, as sway reports it.
        assert_eq!(
            logical_size(mode, 1.299_999_952_316_284_2, Transform::Normal),
            (1476, 830)
        );
        assert_eq!(logical_size(mode, 2.0, Transform::R90), (540, 960));
        assert_eq!(logical_size(mode, 1.0, Transform::Flipped270), (1080, 1920));
        assert_eq!(logical_size(mode, 1.0, Transform::R180), (1920, 1080));
        assert_eq!(logical_size(mode, 0.0, Transform::Normal), (1920, 1080));
    }

    #[test]
    fn a_dragged_output_snaps_to_edges_and_centers() {
        let left = rect(0, 0, 2560, 1440);
        // Right of the other, tops aligned.
        let snapped = snap(rect(2575, -10, 1920, 1080), &[left], 16);
        assert_eq!(snapped.rect, rect(2560, 0, 1920, 1080));
        assert_eq!(
            snapped.guides,
            [Guide::Vertical(2560), Guide::Horizontal(0)]
        );
        // Bottoms aligned.
        let snapped = snap(rect(2560, 355, 1920, 1080), &[left], 16);
        assert_eq!(snapped.rect.y, 360);
        assert_eq!(snapped.guides[1], Guide::Horizontal(1440));
        // Centers aligned.
        let snapped = snap(rect(2560, 175, 1920, 1080), &[left], 16);
        assert_eq!(snapped.rect.y, 180);
        assert_eq!(snapped.guides[1], Guide::Horizontal(720));
        // Below, left edges aligned.
        let snapped = snap(rect(-12, 1450, 1920, 1080), &[left], 16);
        assert_eq!((snapped.rect.x, snapped.rect.y), (0, 1440));
        // Too far away on one axis: that axis stays free.
        let snapped = snap(rect(2600, 500, 1920, 1080), &[left], 16);
        assert_eq!(snapped.rect, rect(2600, 500, 1920, 1080));
        assert_eq!(snapped.guides, [] as [super::Guide; 0]);
        // The nearest of two candidates wins.
        let other = rect(4480, 0, 1000, 1000);
        let snapped = snap(rect(2562, 600, 1920, 1080), &[left, other], 16);
        assert_eq!(snapped.rect.x, 2560);
    }

    #[test]
    fn overlapping_and_touching_are_told_apart() {
        let a = rect(0, 0, 100, 100);
        assert!(overlaps(a, rect(50, 50, 100, 100)));
        assert!(!overlaps(a, rect(100, 0, 100, 100)));
        assert!(touches(a, rect(100, 50, 100, 100)));
        assert!(touches(a, rect(-20, 100, 50, 10)));
        // Only a corner: the pointer can't cross.
        assert!(!touches(a, rect(100, 100, 100, 100)));
        assert!(!touches(a, rect(101, 0, 100, 100)));
    }

    #[test]
    fn a_dropped_output_moves_to_the_nearest_free_place() {
        let left = rect(0, 0, 2560, 1440);
        // Where it is is fine.
        let fine = rect(2560, 100, 1920, 1080);
        assert_eq!(settle(fine, &[left]), fine);
        // On top of the other: next to its nearest edge.
        assert_eq!(
            settle(rect(2000, 100, 1920, 1080), &[left]),
            rect(2560, 100, 1920, 1080)
        );
        assert_eq!(
            settle(rect(200, 1300, 1920, 1080), &[left]),
            rect(200, 1440, 1920, 1080)
        );
        // Away from every other one: back to the nearest edge, sharing a quarter of it.
        assert_eq!(
            settle(rect(3000, 2000, 1000, 500), &[left]),
            rect(2560, 1315, 1000, 500)
        );
        // Without others, anywhere.
        let alone = rect(-50, 70, 10, 10);
        assert_eq!(settle(alone, &[]), alone);
        // Between two outputs, a free side is found.
        let right = rect(2560, 0, 1920, 1080);
        let settled = settle(rect(1000, 0, 1920, 1080), &[left, right]);
        assert!(!overlaps(settled, left) && !overlaps(settled, right));
        assert!(touches(settled, left) || touches(settled, right));
    }

    #[test]
    fn a_dropped_output_keeps_the_layout_together_where_it_can() {
        // A | B | C: B dropped below A would cut C off; next to C or between them it doesn't.
        let a = rect(0, 0, 1000, 500);
        let c = rect(2000, 0, 1000, 500);
        let settled = settle(rect(100, 500, 1000, 500), &[a, c]);
        assert!(is_connected(&[a, c, settled]), "{settled:?}");
        assert!(!overlaps(settled, a) && !overlaps(settled, c));
    }

    #[test]
    fn a_resized_layout_flows_so_what_touched_still_touches() {
        // Two side by side, a third below the first; the first and the second get a scale of 2.
        let before = [
            rect(0, 0, 2560, 1440),
            rect(2560, 0, 1920, 1080),
            rect(0, 1440, 1920, 1080),
        ];
        let after = [
            rect(0, 0, 1280, 720),
            rect(2560, 0, 960, 540),
            rect(0, 1440, 1920, 1080),
        ];
        let flowed = super::reflow(&before, &after).unwrap();
        assert_eq!(
            flowed,
            [
                rect(0, 0, 1280, 720),
                rect(1280, 0, 960, 540),
                rect(0, 720, 1920, 1080)
            ]
        );
        // Nothing to do while the layout still hangs together, or when places changed too.
        assert_eq!(super::reflow(&before, &before), None);
        let moved = [rect(10, 0, 1280, 720), after[1], after[2]];
        assert_eq!(super::reflow(&before, &moved), None);
    }

    #[test]
    fn an_output_dropped_between_two_makes_room_for_itself() {
        use super::{Slot, insert, slot_at};
        let a = rect(0, 0, 1000, 500);
        let b = rect(1000, 0, 1000, 500);
        // Its center near the edge of A and B: that slot.
        let slot = Slot {
            before: 0,
            after: 1,
            stacked: false,
        };
        assert_eq!(slot_at(rect(650, 50, 800, 400), &[a, b], None), Some(slot));
        // Far from the edge: none; a little beyond the zone, the shown one stays.
        assert_eq!(slot_at(rect(100, 50, 800, 400), &[a, b], None), None);
        assert_eq!(slot_at(rect(380, 50, 800, 400), &[a, b], None), None);
        assert_eq!(
            slot_at(rect(380, 50, 800, 400), &[a, b], Some(slot)),
            Some(slot)
        );
        // Put between them, lined up with their tops; B moves along.
        let (placed, moved) = insert(rect(650, 50, 800, 400), &[a, b], slot).unwrap();
        assert_eq!(placed, rect(1000, 0, 800, 400));
        assert_eq!(moved, [a, rect(1800, 0, 1000, 500)]);

        // A | B | C | D, B dropped between C and D: the gap where B was closes.
        let c = rect(2000, 0, 1000, 500);
        let d = rect(3000, 0, 1000, 500);
        let between = Slot {
            before: 1,
            after: 2,
            stacked: false,
        };
        let (placed, moved) = insert(b, &[a, c, d], between).unwrap();
        assert_eq!(
            moved,
            [a, rect(1000, 0, 1000, 500), rect(3000, 0, 1000, 500)]
        );
        assert_eq!(placed, rect(2000, 0, 1000, 500));

        // One above the other works the same.
        let top = rect(0, 0, 1000, 500);
        let bottom = rect(0, 500, 1000, 500);
        let stacked = Slot {
            before: 0,
            after: 1,
            stacked: true,
        };
        assert_eq!(
            slot_at(rect(0, 300, 1000, 400), &[top, bottom], None),
            Some(stacked)
        );
        let (placed, moved) = insert(rect(0, 300, 1000, 400), &[top, bottom], stacked).unwrap();
        assert_eq!(placed, rect(0, 500, 1000, 400));
        assert_eq!(moved, [top, rect(0, 900, 1000, 500)]);
    }

    #[test]
    fn extreme_rectangles_neither_overflow_nor_count() {
        let empty = rect(50, 50, 0, 0);
        let screen = rect(0, 0, 100, 100);
        assert!(!overlaps(empty, screen) && !touches(empty, screen));
        assert!(!touches(rect(0, 0, 0, 100), screen));
        assert_eq!(settle(empty, &[screen]), empty);
        // Coordinates at the end of i32: nothing overflows.
        let far = rect(i32::MAX - 10, i32::MIN + 10, 1920, 1080);
        let near = rect(i32::MIN, i32::MAX - 5, 2560, 1440);
        let _ = snap(far, &[near], i32::MAX);
        let _ = settle(far, &[near]);
        let mut both = [far, near];
        normalize(&mut both);
    }

    #[test]
    fn a_layout_is_normalized_and_checked_for_gaps() {
        let mut rects = [rect(-1440, 300, 1440, 2560), rect(0, 304, 1920, 1080)];
        normalize(&mut rects);
        assert_eq!(rects, [rect(0, 0, 1440, 2560), rect(1440, 4, 1920, 1080)]);
        assert!(is_connected(&rects));
        assert!(!is_connected(&[rect(0, 0, 10, 10), rect(20, 0, 10, 10)]));
        assert!(is_connected(&[]));
        normalize(&mut []);
    }

    proptest! {
        /// Wherever an output is dropped, it ends up next to the others without overlapping any.
        #[test]
        fn a_settled_output_overlaps_nothing_and_touches_something(
            others in proptest::collection::vec(
                (-3000_i32..3000, -3000_i32..3000, 100_i32..3000, 100_i32..3000),
                1..4,
            ),
            dropped in (-6000_i32..6000, -6000_i32..6000, 100_i32..3000, 100_i32..3000),
        ) {
            // A layout without overlaps to start from: the others in a row.
            let mut x = 0;
            let others: Vec<Rect> = others
                .into_iter()
                .map(|(_, y, width, height)| {
                    let placed = rect(x, y.clamp(-50, 50), width, height);
                    x += width;
                    placed
                })
                .collect();
            let moving = rect(dropped.0, dropped.1, dropped.2, dropped.3);
            let settled = settle(moving, &others);
            prop_assert_eq!((settled.width, settled.height), (moving.width, moving.height));
            prop_assert!(others.iter().all(|other| !overlaps(settled, *other)));
            prop_assert!(others.iter().any(|other| touches(settled, *other)));
        }
    }
}
