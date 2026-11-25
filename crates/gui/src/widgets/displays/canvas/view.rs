// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How an arrangement of outputs maps onto an area, without GTK: the room around it, the view
//! that fits it in, where each tile goes, and the size the area asks for. The Displays canvas and
//! the Wallpaper page's preview both place their tiles with it, so the two show the outputs alike.

use sway_things_core::outputs::Rect;

/// The room around the arrangement in the area, in pixels.
pub(crate) const PADDING: f64 = 24.0;

/// The size an area that shows the arrangement asks for along `orientation`: minimum and natural,
/// as [`gtk::prelude::WidgetImpl::measure`] gives them.
pub(crate) fn measure(orientation: gtk::Orientation) -> (i32, i32, i32, i32) {
    match orientation {
        gtk::Orientation::Horizontal => (120, 600, -1, -1),
        _ => (200, 300, -1, -1),
    }
}

/// How the layout maps onto the area: area = offset + logical × scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct View {
    pub(crate) scale: f64,
    pub(crate) x: f64,
    pub(crate) y: f64,
}

impl View {
    /// The view that fits `bounds` into `width` × `height`, centered.
    pub(crate) fn fit(bounds: Rect, width: f64, height: f64) -> Self {
        let usable_width = (width - 2.0 * PADDING).max(1.0);
        let usable_height = (height - 2.0 * PADDING).max(1.0);
        let scale = (usable_width / f64::from(bounds.width.max(1)))
            .min(usable_height / f64::from(bounds.height.max(1)));
        Self {
            scale,
            x: (width - f64::from(bounds.width) * scale) / 2.0 - f64::from(bounds.x) * scale,
            y: (height - f64::from(bounds.height) * scale) / 2.0 - f64::from(bounds.y) * scale,
        }
    }

    /// The view of the outputs at `rects` in an area of `width` × `height`; `None` without any.
    pub(crate) fn of(rects: &[Rect], width: i32, height: i32) -> Option<Self> {
        Some(Self::fit(
            bounds(rects)?,
            f64::from(width),
            f64::from(height),
        ))
    }

    pub(crate) fn x(self, logical: i32) -> f64 {
        self.x + f64::from(logical) * self.scale
    }

    pub(crate) fn y(self, logical: i32) -> f64 {
        self.y + f64::from(logical) * self.scale
    }

    /// A distance in the area in logical pixels.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a distance on the screen divided by a positive scale fits easily"
    )]
    pub(crate) fn logical(self, canvas: f64) -> i32 {
        (canvas / self.scale).round() as i32
    }

    /// Where the tile of an output at `rect` goes: x, y, width and height in pixels, a pixel in
    /// from each edge, so tiles that touch stay apart on the screen.
    pub(crate) fn tile(self, rect: Rect) -> (f64, f64, f64, f64) {
        (
            self.x(rect.x) + 1.0,
            self.y(rect.y) + 1.0,
            (f64::from(rect.width) * self.scale - 2.0).max(1.0),
            (f64::from(rect.height) * self.scale - 2.0).max(1.0),
        )
    }
}

/// What the area shows of the outputs at `rects`: all of them, with room for half of the smallest
/// on every side, so an output can be dragged next to any edge without leaving the area, and the
/// tiles stay big enough to read. `None` without any.
pub(crate) fn bounds(rects: &[Rect]) -> Option<Rect> {
    let left = rects.iter().map(|rect| rect.x).min()?;
    let top = rects.iter().map(|rect| rect.y).min()?;
    let right = rects.iter().map(|rect| rect.right()).max()?;
    let bottom = rects.iter().map(|rect| rect.bottom()).max()?;
    let room_x = rects.iter().map(|rect| rect.width).min()? / 2;
    let room_y = rects.iter().map(|rect| rect.height).min()? / 2;
    Some(Rect {
        x: left.saturating_sub(room_x),
        y: top.saturating_sub(room_y),
        width: right
            .saturating_sub(left)
            .saturating_add(room_x.saturating_mul(2)),
        height: bottom
            .saturating_sub(top)
            .saturating_add(room_y.saturating_mul(2)),
    })
}

#[cfg(test)]
mod tests {
    use super::{PADDING, View, bounds};
    use sway_things_core::outputs::Rect;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// The tiles of `rects` in an area of `width` × `height`.
    fn tiles(rects: &[Rect], width: i32, height: i32) -> Vec<(f64, f64, f64, f64)> {
        let view = View::of(rects, width, height).unwrap_or(View {
            scale: 0.0,
            x: 0.0,
            y: 0.0,
        });
        rects.iter().map(|rect| view.tile(*rect)).collect()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn nothing_has_no_view() {
        assert_eq!(bounds(&[]), None);
        assert_eq!(View::of(&[], 600, 300), None);
    }

    #[test]
    fn the_room_is_half_the_smallest_output() {
        let side_by_side = [rect(0, 0, 2560, 1440), rect(2560, 0, 1920, 1080)];
        assert_eq!(bounds(&side_by_side), Some(rect(-960, -540, 6400, 2520)));
    }

    #[test]
    fn side_by_side_outputs_touch_with_a_gap_of_two_pixels() {
        let shown = tiles(
            &[rect(0, 0, 1920, 1080), rect(1920, 0, 1920, 1080)],
            600,
            300,
        );
        let [(ax, ay, aw, ah), (bx, by, _, _)] = shown.as_slice() else {
            panic!("{shown:?}");
        };
        assert!(near(*ax + *aw + 2.0, *bx), "{shown:?}");
        assert!(near(*ay, *by), "{shown:?}");
        // Same proportions as the screen, but for the gap.
        assert!(
            near((*aw + 2.0) / (*ah + 2.0), 1920.0 / 1080.0),
            "{shown:?}"
        );
    }

    #[test]
    fn an_output_above_keeps_its_offset() {
        // A laptop below the middle of a wider monitor.
        let shown = tiles(
            &[rect(0, 0, 2560, 1440), rect(320, 1440, 1920, 1200)],
            600,
            300,
        );
        let [(ax, ay, aw, ah), (bx, by, bw, _)] = shown.as_slice() else {
            panic!("{shown:?}");
        };
        let scale = (*aw + 2.0) / 2560.0;
        assert!(near(*bx - *ax, 320.0 * scale), "{shown:?}");
        assert!(near(*ay + *ah + 2.0, *by), "{shown:?}");
        assert!(near(*bw + 2.0, 1920.0 * scale), "{shown:?}");
    }

    #[test]
    fn a_rotated_output_and_mixed_scales_keep_their_logical_sizes() {
        // A monitor at 1.5 (2560 × 1440 → 1707 × 960) beside one turned on its side.
        let shown = tiles(
            &[rect(0, 0, 1707, 960), rect(1707, 0, 1080, 1920)],
            600,
            300,
        );
        let [(_, _, aw, ah), (_, _, bw, bh)] = shown.as_slice() else {
            panic!("{shown:?}");
        };
        assert!(
            near((*bw + 2.0) / (*bh + 2.0), 1080.0 / 1920.0),
            "{shown:?}"
        );
        assert!(near((*ah + 2.0) / (*bh + 2.0), 960.0 / 1920.0), "{shown:?}");
        assert!(
            near((*aw + 2.0) / (*bw + 2.0), 1707.0 / 1080.0),
            "{shown:?}"
        );
    }

    #[test]
    fn the_arrangement_is_centered_within_the_padding() {
        let rects = [rect(0, 0, 1920, 1080)];
        let view = View::of(&rects, 600, 300).unwrap_or(View {
            scale: 0.0,
            x: 0.0,
            y: 0.0,
        });
        let area = bounds(&rects).unwrap_or_default();
        // The height limits: the bounds fill it but for the padding, and center across.
        assert!(near(
            f64::from(area.height) * view.scale,
            300.0 - 2.0 * PADDING
        ));
        let left = view.x(area.x);
        let right = view.x(area.right());
        assert!(near(left, 600.0 - right), "{left} {right}");
    }
}
