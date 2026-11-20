// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dragging a tile: snapping, the guides, settling it next to the others, the slide of the
//! others out of the way, dropping and arranging.
use std::time::Instant;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::outputs::layout::Guide;
use sway_things_core::outputs::{Rect, SCALE, layout};
use sway_things_core::registry::Value;

use super::geometry::{percent, scale_for_grip, scale_with_grip, take_slot};
use super::labels::tile_labels;
use super::{
    Canvas, DRAG_START, Drag, ESCAPES, Layout, Placed, SETTLING, SLIDE, SNAP_DISTANCE, Tile,
    UNDO_STEPS,
};
use crate::i18n::{msg, tr};

impl Canvas {
    /// Moves tiles with the pointer: a press on a tile that moves far enough becomes a drag.
    pub(super) fn add_drag(&self) {
        let gesture = gtk::GestureDrag::new();
        // Before the buttons: a press that becomes a drag isn't a click.
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        gesture.connect_drag_begin(glib::clone!(
            #[weak(rename_to = canvas)]
            self,
            move |_, x, y| canvas.begin_drag(x, y)
        ));
        gesture.connect_drag_update(glib::clone!(
            #[weak(rename_to = canvas)]
            self,
            move |gesture, dx, dy| canvas.update_drag(gesture, dx, dy)
        ));
        gesture.connect_drag_end(glib::clone!(
            #[weak(rename_to = canvas)]
            self,
            move |_, _, _| canvas.end_drag()
        ));
        gesture.connect_cancel(glib::clone!(
            #[weak(rename_to = canvas)]
            self,
            move |_, _| canvas.cancel_drag()
        ));
        self.add_controller(gesture);
    }

    pub(super) fn begin_drag(&self, x: f64, y: f64) {
        let imp = self.imp();
        // Locked: a press only picks the tile.
        if imp.locked.get() {
            return;
        }
        let grabbed_tile = self.grip_at(x, y);
        let grabbed = grabbed_tile.is_some();
        let Some(tile) = grabbed_tile
            .or_else(|| self.tile_at(x, y))
            .filter(|tile| !tile.off)
        else {
            return;
        };
        let rect = self.shown_rect(&tile);
        imp.frozen.set(self.view(self.width(), self.height()));
        imp.drag.replace(Some(Drag {
            identifier: tile.identifier,
            start: rect,
            rect,
            guides: Vec::new(),
            moving: false,
            snapped: (false, false),
            escapes: (0, 0),
            slot: None,
            room: Vec::new(),
            scaling: grabbed.then_some((tile.scale, tile.scale)),
        }));
    }

    /// The tile whose grip is under the point `x`, `y` of the canvas.
    pub(super) fn grip_at(&self, x: f64, y: f64) -> Option<Tile> {
        let picked = WidgetExt::pick(self, x, y, gtk::PickFlags::DEFAULT)?;
        self.imp()
            .tiles
            .borrow()
            .iter()
            .find(|tile| picked == *tile.grip.upcast_ref::<gtk::Widget>())
            .cloned()
    }

    /// Shows the mode on the tile of `identifier` again, after its grip showed the scale.
    pub(super) fn restore_details(&self, identifier: &str) {
        let tiles = self.imp().tiles.borrow();
        if let Some(details) = tiles
            .iter()
            .find(|tile| tile.identifier == identifier)
            .and_then(|tile| Some((tile_labels(&tile.button)?.2, tile.details.clone())))
        {
            details.0.set_text(&details.1);
        }
    }

    /// Shows `scale` on the tile of `identifier` while its grip is dragged.
    pub(super) fn show_scale(&self, identifier: &str, scale: f64) {
        let tiles = self.imp().tiles.borrow();
        let Some(tile) = tiles.iter().find(|tile| tile.identifier == identifier) else {
            return;
        };
        let Some(content) = tile.button.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let details = std::iter::successors(content.first_child(), WidgetExt::next_sibling)
            .filter_map(|child| child.downcast::<gtk::Label>().ok())
            .nth(2);
        if let Some(details) = details {
            details.set_text(&percent(scale));
        }
    }

    /// A grip for a tile: dragging it scales the output, as the tile keeps its proportions.
    pub(super) fn grip(&self) -> gtk::Box {
        let grip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        grip.add_css_class("st-display-grip");
        grip.set_cursor_from_name(Some("nwse-resize"));
        grip.set_tooltip_text(Some(tr(msg!("Drag to scale"))));
        grip.set_visible(false);
        grip.set_parent(self);
        grip
    }

    /// The tile under the point `x`, `y` of the canvas.
    pub(super) fn tile_at(&self, x: f64, y: f64) -> Option<Tile> {
        let picked = WidgetExt::pick(self, x, y, gtk::PickFlags::DEFAULT)?;
        self.imp()
            .tiles
            .borrow()
            .iter()
            .find(|tile| {
                picked == *tile.button.upcast_ref::<gtk::Widget>()
                    || picked.is_ancestor(&tile.button)
            })
            .cloned()
    }

    pub(super) fn update_drag(&self, gesture: &gtk::GestureDrag, dx: f64, dy: f64) {
        let imp = self.imp();
        let Some(view) = imp.frozen.get() else {
            return;
        };
        let (all, other_ids, others): (Layout, Vec<String>, Vec<Rect>) = {
            let drag = imp.drag.borrow();
            let Some(drag) = drag.as_ref() else {
                return;
            };
            let all = self.layout();
            let (ids, rects) = all
                .iter()
                .filter(|(identifier, _)| *identifier != drag.identifier)
                .cloned()
                .unzip();
            (all, ids, rects)
        };
        let mut guard = imp.drag.borrow_mut();
        let Some(drag) = guard.as_mut() else {
            return;
        };
        if !drag.moving {
            if dx.hypot(dy) < DRAG_START {
                return;
            }
            drag.moving = true;
            gesture.set_state(gtk::EventSequenceState::Claimed);
        }
        if let Some((from, _)) = drag.scaling {
            let scale = scale_for_grip(drag.start, from, view.logical(dx), view.logical(dy));
            scale_with_grip(drag, &all, from, scale);
            let identifier = drag.identifier.clone();
            drop(guard);
            self.show_scale(&identifier, scale);
            self.queue_allocate();
            self.queue_draw();
            return;
        }
        let moved = Rect {
            x: drag.start.x.saturating_add(view.logical(dx)),
            y: drag.start.y.saturating_add(view.logical(dy)),
            ..drag.start
        };
        // Near the edge between two tiles: a marker says it goes in between, the others slide
        // aside to make room, and the tile follows the pointer freely.
        let slot_changed = take_slot(drag, moved, &other_ids, &others);
        if drag.slot.is_some() {
            drag.rect = moved;
            drag.guides.clear();
            drag.snapped = (false, false);
            drop(guard);
            if slot_changed {
                // The others slide aside to make room, or back.
                self.start_slide();
            }
            self.queue_allocate();
            self.queue_draw();
            return;
        }
        if slot_changed {
            drop(guard);
            self.start_slide();
            guard = imp.drag.borrow_mut();
        }
        let Some(drag) = guard.as_mut() else {
            return;
        };
        let snapped = layout::snap(moved, &others, view.logical(SNAP_DISTANCE));
        let on_x = snapped
            .guides
            .iter()
            .any(|guide| matches!(guide, Guide::Vertical(_)));
        let on_y = snapped
            .guides
            .iter()
            .any(|guide| matches!(guide, Guide::Horizontal(_)));
        if drag.snapped.0 && !on_x {
            drag.escapes.0 = drag.escapes.0.saturating_add(1);
        }
        if drag.snapped.1 && !on_y {
            drag.escapes.1 = drag.escapes.1.saturating_add(1);
        }
        let free_x = drag.escapes.0 >= ESCAPES;
        let free_y = drag.escapes.1 >= ESCAPES;
        drag.snapped = (on_x && !free_x, on_y && !free_y);
        drag.rect = Rect {
            x: if free_x { moved.x } else { snapped.rect.x },
            y: if free_y { moved.y } else { snapped.rect.y },
            ..moved
        };
        drag.guides = snapped
            .guides
            .into_iter()
            .filter(|guide| match guide {
                Guide::Vertical(_) => !free_x,
                Guide::Horizontal(_) => !free_y,
            })
            .collect();
        drop(guard);
        self.queue_allocate();
        self.queue_draw();
    }

    /// Shows the outputs reported while a drag ran.
    pub(super) fn show_deferred(&self) {
        let deferred = self.imp().deferred.take();
        if let Some((outputs, numbers)) = deferred {
            self.set_outputs(&outputs, |identifier| {
                numbers
                    .iter()
                    .find(|(known, _)| known == identifier)
                    .map(|(_, number)| *number)
            });
        }
    }

    pub(super) fn end_drag(&self) {
        let imp = self.imp();
        let drag = imp.drag.take();
        imp.frozen.set(None);
        // An output that came or went meanwhile ends the drag where the layout is now.
        let replaced = imp.deferred.borrow().as_ref().is_some_and(|(outputs, _)| {
            let now: Vec<String> = self.layout().into_iter().map(|(id, _)| id).collect();
            let reported: Vec<String> = outputs
                .enabled()
                .map(|output| output.identifier().to_owned())
                .collect();
            now != reported
        });
        if replaced {
            if drag.as_ref().is_some_and(|drag| !drag.room.is_empty()) {
                self.start_slide();
            }
            self.show_deferred();
            return;
        }
        self.show_deferred();
        let Some(drag) = drag.filter(|drag| drag.moving) else {
            self.queue_allocate();
            return;
        };
        if let Some((from, scale)) = drag.scaling {
            // The tile shows its mode again; once sway reports the new size, the layout flows
            // along with it (`follow_size_changes`), as for a scale set in the row.
            self.restore_details(&drag.identifier);
            if (scale - from).abs() > f64::EPSILON {
                if let Some(model) = imp.model.get() {
                    model.set(SCALE, Some(&drag.identifier), Value::Decimal(scale));
                }
                let size = (drag.start.width, drag.start.height);
                imp.scaled.replace(Some((
                    drag.identifier.clone(),
                    size,
                    drag.rect,
                    Instant::now(),
                )));
            }
            self.queue_allocate();
            return;
        }
        let layout = self.layout();
        self.drop_at(&drag.identifier, drag.rect, drag.slot, &layout);
        self.pick(&drag.identifier);
    }

    pub(super) fn cancel_drag(&self) {
        let imp = self.imp();
        if let Some(drag) = imp.drag.replace(None) {
            self.restore_details(&drag.identifier);
            // The others go back from where they made room.
            if !drag.room.is_empty() {
                self.start_slide();
            }
        }
        imp.frozen.set(None);
        self.show_deferred();
        self.queue_allocate();
        self.queue_draw();
    }

    /// Settles the output `identifier` at `rect` among the others of `layout`, moves the layout
    /// to start at 0,0, shows it and applies it: every output gets its position stored.
    pub(super) fn drop_at(
        &self,
        identifier: &str,
        rect: Rect,
        slot: Option<layout::Slot>,
        layout: &[(String, Rect)],
    ) {
        let others: Vec<Rect> = layout
            .iter()
            .filter(|(id, _)| id != identifier)
            .map(|(_, rect)| *rect)
            .collect();
        // Where it stood: what the layout says while nothing is dragged any more.
        let origin = layout
            .iter()
            .find(|(id, _)| id == identifier)
            .map_or(rect, |(_, origin)| *origin);
        let (settled, others) = layout::place(rect, origin, &others, slot);
        let identifiers: Vec<String> = layout.iter().map(|(id, _)| id.clone()).collect();
        let mut others = others.into_iter();
        let mut rects: Vec<Rect> = layout
            .iter()
            .map(|(id, other)| {
                if id == identifier {
                    settled
                } else {
                    others.next().unwrap_or(*other)
                }
            })
            .collect();
        layout::normalize(&mut rects);
        let arranged: Vec<(String, Rect)> = identifiers.into_iter().zip(rects).collect();
        self.arrange(&arranged, true);
    }

    /// Shows `arranged` and applies it; with `remember`, Ctrl+Z goes back to the arrangement
    /// before.
    pub(super) fn arrange(&self, arranged: &[(String, Rect)], remember: bool) {
        let imp = self.imp();
        let before = self.layout();
        if before == *arranged {
            self.queue_allocate();
            return;
        }
        if remember {
            let mut history = imp.history.borrow_mut();
            history.push(before);
            if history.len() > UNDO_STEPS {
                history.remove(0);
            }
        }
        self.start_slide();
        self.show_dropped(arranged);
        self.apply(arranged);
    }

    /// Lets the tiles slide from where they are to their new places, unless the user turned
    /// animations off.
    pub(super) fn start_slide(&self) {
        let imp = self.imp();
        let animated =
            gtk::Settings::default().is_none_or(|settings| settings.is_gtk_enable_animations());
        if !animated {
            return;
        }
        let from = imp.placed.borrow().clone();
        let running = imp.slide.replace(Some((from, Instant::now()))).is_some();
        if running {
            return;
        }
        self.add_tick_callback(|canvas, _| {
            let imp = canvas.imp();
            canvas.queue_allocate();
            let done = imp
                .slide
                .borrow()
                .as_ref()
                .is_none_or(|(_, since)| since.elapsed() >= SLIDE);
            if done {
                imp.slide.replace(None);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    /// Where `identifier`, placed at `target` now, is shown while the tiles slide.
    pub(super) fn sliding(&self, identifier: &str, target: Placed) -> Placed {
        let slide = self.imp().slide.borrow();
        let Some((from, since)) = slide.as_ref() else {
            return target;
        };
        let Some(start) = from.get(identifier) else {
            return target;
        };
        let t = (since.elapsed().as_secs_f64() / SLIDE.as_secs_f64()).min(1.0);
        // Ease out: fast at first, gently at the end.
        let eased = 1.0 - (1.0 - t).powi(3);
        let lerp = |a: f64, b: f64| a + (b - a) * eased;
        (
            lerp(start.0, target.0),
            lerp(start.1, target.1),
            lerp(start.2, target.2),
            lerp(start.3, target.3),
        )
    }

    /// Shows `arranged` until sway reports it, or for [`SETTLING`] if it doesn't.
    pub(super) fn show_dropped(&self, arranged: &[(String, Rect)]) {
        let since = Instant::now();
        self.imp().dropped.replace(Some((arranged.to_vec(), since)));
        glib::timeout_add_local_once(
            SETTLING,
            glib::clone!(
                #[weak(rename_to = canvas)]
                self,
                move || {
                    let imp = canvas.imp();
                    let expired = imp
                        .dropped
                        .borrow()
                        .as_ref()
                        .is_some_and(|(_, dropped)| *dropped == since);
                    if expired {
                        imp.dropped.replace(None);
                        canvas.queue_allocate();
                    }
                }
            ),
        );
        self.queue_allocate();
        self.queue_draw();
    }
}
