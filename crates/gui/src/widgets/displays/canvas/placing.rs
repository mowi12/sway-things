// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the tiles, the guides and the slot are drawn: the view that fits the arrangement into the
//! canvas, and placing each part in it.
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::outputs::layout::Guide;
use sway_things_core::outputs::{Rect, layout};

use super::labels::{fit_labels, tile_labels};
use super::{Canvas, GRIP, GUIDE_BEYOND, GUIDE_WIDTH, SLOT_WIDTH, View};

impl Canvas {
    /// The view of the current layout in a canvas of `width` × `height`.
    pub(super) fn view(&self, width: i32, height: i32) -> Option<View> {
        if let Some(frozen) = self.imp().frozen.get() {
            return Some(frozen);
        }
        let rects: Vec<Rect> = self
            .imp()
            .tiles
            .borrow()
            .iter()
            .map(|tile| self.shown_rect(tile))
            .collect();
        let left = rects.iter().map(|rect| rect.x).min()?;
        let top = rects.iter().map(|rect| rect.y).min()?;
        let right = rects.iter().map(|rect| rect.right()).max()?;
        let bottom = rects.iter().map(|rect| rect.bottom()).max()?;
        // Room around the layout for half of the smallest output on every side, so an output
        // can be dragged next to any edge without leaving the canvas (the view stays as it is
        // while dragging), and the tiles stay big enough to read.
        let room_x = rects.iter().map(|rect| rect.width).min()? / 2;
        let room_y = rects.iter().map(|rect| rect.height).min()? / 2;
        let bounds = Rect {
            x: left.saturating_sub(room_x),
            y: top.saturating_sub(room_y),
            width: right
                .saturating_sub(left)
                .saturating_add(room_x.saturating_mul(2)),
            height: bottom
                .saturating_sub(top)
                .saturating_add(room_y.saturating_mul(2)),
        };
        Some(View::fit(bounds, f64::from(width), f64::from(height)))
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "positions on the canvas are within its few hundred pixels"
    )]
    pub(super) fn place_tiles(&self, width: i32, height: i32) {
        let Some(view) = self.view(width, height) else {
            return;
        };
        let imp = self.imp();
        for tile in imp.tiles.borrow().iter() {
            let rect = self.drawn_rect(tile);
            // A small gap, so tiles that touch stay apart on the screen.
            let x = view.x(rect.x) + 1.0;
            let y = view.y(rect.y) + 1.0;
            let w = (f64::from(rect.width) * view.scale - 2.0).max(1.0);
            let h = (f64::from(rect.height) * view.scale - 2.0).max(1.0);
            let dragged = imp
                .drag
                .borrow()
                .as_ref()
                .is_some_and(|drag| drag.moving && drag.identifier == tile.identifier);
            let (x, y, w, h) = if dragged {
                (x, y, w, h)
            } else {
                self.sliding(&tile.identifier, (x, y, w, h))
            };
            imp.placed
                .borrow_mut()
                .insert(tile.identifier.clone(), (x, y, w, h));
            let (w, h) = (w.round() as i32, h.round() as i32);
            // The grip sits in the bottom right corner of the picked tile, if it is on and big
            // enough: one grip at a time keeps the others' names clear.
            let picked = imp.selected.borrow().as_deref() == Some(tile.identifier.as_str());
            let grip_shown =
                picked && !tile.off && !imp.locked.get() && w > GRIP * 3 && h > GRIP * 3;
            tile.grip.set_visible(grip_shown);
            if grip_shown {
                tile.grip.size_allocate(
                    &gtk::Allocation::new(
                        x.round() as i32 + w - GRIP - 2,
                        y.round() as i32 + h - GRIP - 2,
                        GRIP,
                        GRIP,
                    ),
                    -1,
                );
            }
            // While its grip is dragged: the scale it gets, and the details that say so.
            let scaling = imp
                .drag
                .borrow()
                .as_ref()
                .filter(|drag| drag.identifier == tile.identifier)
                .and_then(|drag| drag.scaling)
                .map(|(_, scale)| scale);
            let (full, scale) = match (scaling, tile_labels(&tile.button)) {
                (Some(scale), Some((_, _, details))) => (details.text().to_string(), scale),
                _ => (tile.details.clone(), tile.scale),
            };
            fit_labels(&tile.button, (w, h), grip_shown, &full, scale);
            tile.button.size_allocate(
                &gtk::Allocation::new(x.round() as i32, y.round() as i32, w, h),
                -1,
            );
        }
        self.place_guides(view);
    }

    /// Places the guide lines of the drag, from the dragged tile to the tiles it lines up with
    /// and a little beyond; hides those that aren't needed.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "positions on the canvas are within its few hundred pixels"
    )]
    pub(super) fn place_guides(&self, view: View) {
        let imp = self.imp();
        let Some((vertical, horizontal)) = imp.guides.get() else {
            return;
        };
        let drag = imp.drag.borrow();
        let drag = drag.as_ref().filter(|drag| drag.moving);
        if let Some((drag, slot)) = drag.and_then(|drag| Some((drag, drag.slot?))) {
            self.place_slot(view, &drag.identifier, slot, vertical, horizontal);
            return;
        }
        let rects: Vec<Rect> = self.layout().into_iter().map(|(_, rect)| rect).collect();
        let mut shown = (false, false);
        for guide in drag.map(|drag| drag.guides.as_slice()).unwrap_or_default() {
            let Some(drag) = drag else { break };
            // The tiles the line runs along: the dragged one and those with an edge or their
            // center on it.
            let along: Vec<Rect> = rects
                .iter()
                .copied()
                .filter(|rect| match guide {
                    Guide::Vertical(x) => {
                        [rect.x, rect.right(), rect.x + rect.width / 2].contains(x)
                    }
                    Guide::Horizontal(y) => {
                        [rect.y, rect.bottom(), rect.y + rect.height / 2].contains(y)
                    }
                })
                .chain(std::iter::once(drag.rect))
                .collect();
            let (line, allocation) = match guide {
                Guide::Vertical(x) => {
                    let top = along.iter().map(|rect| rect.y).min().unwrap_or(0);
                    let bottom = along.iter().map(|rect| rect.bottom()).max().unwrap_or(0);
                    shown.0 = true;
                    (
                        vertical,
                        (
                            view.x(*x) - GUIDE_WIDTH / 2.0,
                            view.y(top) - GUIDE_BEYOND,
                            GUIDE_WIDTH,
                            view.y(bottom) - view.y(top) + 2.0 * GUIDE_BEYOND,
                        ),
                    )
                }
                Guide::Horizontal(y) => {
                    let left = along.iter().map(|rect| rect.x).min().unwrap_or(0);
                    let right = along.iter().map(|rect| rect.right()).max().unwrap_or(0);
                    shown.1 = true;
                    (
                        horizontal,
                        (
                            view.x(left) - GUIDE_BEYOND,
                            view.y(*y) - GUIDE_WIDTH / 2.0,
                            view.x(right) - view.x(left) + 2.0 * GUIDE_BEYOND,
                            GUIDE_WIDTH,
                        ),
                    )
                }
            };
            let (x, y, width, height) = allocation;
            line.set_visible(true);
            line.size_allocate(
                &gtk::Allocation::new(
                    x.round() as i32,
                    y.round() as i32,
                    (width.round() as i32).max(1),
                    (height.round() as i32).max(1),
                ),
                -1,
            );
        }
        vertical.set_visible(shown.0);
        horizontal.set_visible(shown.1);
    }

    /// Shows the marker of `slot`, the place between two tiles the dragged one goes into: a
    /// bar along the edge the two share, in the accent color.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "positions on the canvas are within its few hundred pixels"
    )]
    pub(super) fn place_slot(
        &self,
        view: View,
        dragged: &str,
        slot: layout::Slot,
        vertical: &gtk::Box,
        horizontal: &gtk::Box,
    ) {
        let others: Vec<Rect> = self
            .layout()
            .into_iter()
            .filter(|(identifier, _)| identifier != dragged)
            .map(|(_, rect)| rect)
            .collect();
        let (Some(a), Some(b)) = (others.get(slot.before), others.get(slot.after)) else {
            vertical.set_visible(false);
            horizontal.set_visible(false);
            return;
        };
        let (line, other, (x, y, width, height)) = if slot.stacked {
            let left = view.x(a.x.min(b.x));
            let right = view.x(a.right().max(b.right()));
            (
                horizontal,
                vertical,
                (
                    left,
                    view.y(b.y) - SLOT_WIDTH / 2.0,
                    right - left,
                    SLOT_WIDTH,
                ),
            )
        } else {
            let top = view.y(a.y.min(b.y));
            let bottom = view.y(a.bottom().max(b.bottom()));
            (
                vertical,
                horizontal,
                (
                    view.x(b.x) - SLOT_WIDTH / 2.0,
                    top,
                    SLOT_WIDTH,
                    bottom - top,
                ),
            )
        };
        other.set_visible(false);
        line.set_visible(true);
        line.size_allocate(
            &gtk::Allocation::new(
                x.round() as i32,
                y.round() as i32,
                (width.round() as i32).max(1),
                (height.round() as i32).max(1),
            ),
            -1,
        );
    }
}
