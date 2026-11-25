// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The arrangement of the outputs that are on, to drag into place.
//!
//! Each output is a toggle button (a "tile") at its place in the layout, scaled to fit the canvas,
//! with its number and its make and model. A click picks it; the button that is pressed is the
//! output the rows below are for. Dragging a tile moves it: it snaps to the edges and centers of
//! the others, with guide lines while it does, and when it is dropped it moves to the nearest
//! place where it overlaps no other output and shares an edge with one (see
//! [`sway_things_core::outputs::layout`]). Then the new layout, moved to start at 0,0, is applied:
//! every output that is on gets its position stored, so the layout stays as arranged.
//!
//! With the keyboard, Tab reaches the tiles and the arrow keys move the focused one by 10 logical
//! pixels, 100 with Shift, settled the same way.
//!
//! Until sway reports the new places, the tiles stay where they were dropped, so they don't jump
//! back for a moment.

mod drag;
mod geometry;
mod labels;
mod placing;
mod view;

use self::geometry::{details, keep_together, park, parked_size};
use self::labels::set_tile_labels;
pub(crate) use self::labels::short_label;
use self::view::View;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::glib::closure_local;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::outputs::layout::{self, Guide};
use sway_things_core::outputs::{MODE, Outputs, POSITION, Rect, SCALE, TRANSFORM};
use sway_things_core::registry::Value;
use sway_things_core::view::Source;

use crate::i18n::{msg, tr};
use crate::model::Model;

/// How close, in pixels on the screen, an edge has to come to snap.
const SNAP_DISTANCE: f64 = 12.0;
/// How far, in pixels on the screen, the pointer moves before a press becomes a drag.
const DRAG_START: f64 = 4.0;
/// How long a dropped layout is shown while sway doesn't report it yet.
const SETTLING: Duration = Duration::from_secs(3);
/// How long a changed size waits for what is stored for it to catch up.
const HOLD: Duration = Duration::from_secs(3);
/// How far an arrow key moves a tile, in logical pixels; with Shift ten times as far.
const NUDGE: i32 = 10;

/// Outputs and their places, by identifier.
type Layout = Vec<(String, Rect)>;

/// What the app stores for the size of each output (scale, rotation, mode), by identifier.
type Sizes = std::collections::BTreeMap<String, Vec<Option<Value>>>;

/// Outputs reported while a drag ran, with the number of each, by identifier.
type Deferred = (Outputs, Vec<(String, usize)>);

/// How far a guide line reaches beyond the tiles it runs along, in pixels.
const GUIDE_BEYOND: f64 = 12.0;
/// How thick a guide line is, in pixels.
const GUIDE_WIDTH: f64 = 2.0;
/// How wide the marker of the place between two tiles a dragged one goes into is, in pixels.
const SLOT_WIDTH: f64 = 4.0;
/// How big the grip in the corner of a tile is, in pixels.
const GRIP: i32 = 14;
/// How long the tiles slide to their new places after a drop.
const SLIDE: Duration = Duration::from_millis(200);

/// An output on the canvas.
#[derive(Debug, Clone)]
struct Tile {
    identifier: String,
    /// Its place in the layout, or for an output that is off, where it is parked.
    rect: Rect,
    /// Whether it is off: shown greyed out beside the others, and not dragged.
    off: bool,
    button: gtk::ToggleButton,
    /// The grip at its bottom right corner that scales the output.
    grip: gtk::Box,
    /// The scale the output runs with.
    scale: f64,
    /// The mode and more it shows under its name.
    details: String,
    /// The name sway gives the output (its connector), its number and its make and model.
    name: String,
    number: Option<usize>,
    label: String,
}

/// What [`Canvas::set_outputs`] shows of an output.
struct Shown {
    identifier: String,
    /// The name sway gives the output, its connector.
    name: String,
    /// Its place, while it is on.
    rect: Option<Rect>,
    label: String,
    /// The name on the tile.
    short: String,
    details: String,
    /// The scale it runs with.
    scale: f64,
    /// The size it is parked with while it is off.
    size: (i32, i32),
    parked: Option<Rect>,
}

/// A tile scaled with its grip: its identifier, the size it had, its new place and since when.
type Scaled = (String, (i32, i32), Rect, Instant);

/// Where a tile is on the canvas: x, y, width and height in pixels.
type Placed = (f64, f64, f64, f64);

/// Where the tiles slide from, and since when.
type Slide = (std::collections::HashMap<String, Placed>, Instant);

/// A drag in progress.
#[derive(Debug, Clone)]
struct Drag {
    identifier: String,
    /// Where the tile was when the drag started.
    start: Rect,
    /// Where it is now, snapped.
    rect: Rect,
    guides: Vec<Guide>,
    /// Whether the pointer moved far enough to count as a drag.
    moving: bool,
    /// Whether the tile snaps on each axis now.
    snapped: (bool, bool),
    /// How often the pointer pulled the tile out of a snap on each axis. After two, that axis
    /// no longer snaps in this drag: the magnet backs off.
    escapes: (u8, u8),
    /// The place between two tiles it goes into if dropped now: the others make room then.
    slot: Option<layout::Slot>,
    /// While it is over a slot: where the others go to make room, as they are drawn (by
    /// identifier). Worked out when the slot changes, so they move once, not with each motion.
    room: Vec<(String, Rect)>,
    /// For a drag of the grip: the scale the output ran with, and the one it gets.
    scaling: Option<(f64, f64)>,
}

/// How often the pointer has to pull a tile out of a snap before that axis stops snapping.
const ESCAPES: u8 = 2;
/// How many arrangements Ctrl+Z goes back.
const UNDO_STEPS: usize = 20;

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::sync::OnceLock;
    use std::time::Instant;

    use gtk::glib;
    use gtk::glib::subclass::Signal;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::outputs::Rect;

    use crate::model::Model;

    #[derive(Debug, Default)]
    pub struct Canvas {
        pub(super) model: OnceCell<Model>,
        pub(super) tiles: RefCell<Vec<super::Tile>>,
        pub(super) selected: RefCell<Option<String>>,
        pub(super) drag: RefCell<Option<super::Drag>>,
        /// The view while a drag runs, so the canvas doesn't rescale under the pointer.
        pub(super) frozen: Cell<Option<super::View>>,
        /// Set while a risky change waits: the tiles can be picked, not moved or scaled.
        pub(super) locked: Cell<bool>,
        /// The displays that show another display's picture, and what their tiles say about it:
        /// parked beside the others like one that is off.
        pub(super) showing: RefCell<Vec<(String, String)>>,
        /// A layout that was applied and isn't reported by sway yet, and since when.
        pub(super) dropped: RefCell<Option<(super::Layout, Instant)>>,
        /// Set while the tiles are updated, so that isn't taken as the user's choice.
        pub(super) updating: Cell<bool>,
        /// The arrangements before the last ones the user made, for Ctrl+Z.
        pub(super) history: RefCell<Vec<Vec<(String, Rect)>>>,
        /// Outputs reported while a drag ran, with their numbers, to show once it ends.
        pub(super) deferred: RefCell<Option<super::Deferred>>,
        /// The guide lines: a vertical and a horizontal one, in the accent color.
        pub(super) guides: OnceCell<(gtk::Box, gtk::Box)>,
        /// The layout and what the app stored for the sizes of its outputs (scale, rotation,
        /// mode) as of the last change that didn't wait to be kept: a size that changed with
        /// what is stored changed through the app.
        pub(super) baseline: RefCell<Option<(super::Layout, super::Sizes)>>,
        /// Since when the baseline is held for what is stored to catch up.
        pub(super) held_since: Cell<Option<Instant>>,
        /// Since when a stored size waits for sway to report the new size.
        pub(super) stored_at: Cell<Option<Instant>>,
        /// Where each tile was last placed on the canvas.
        pub(super) placed: RefCell<std::collections::HashMap<String, super::Placed>>,
        /// The places the tiles slide from after a change of the layout, and since when.
        pub(super) slide: RefCell<Option<super::Slide>>,
        /// A tile scaled with its grip: its identifier, the size it had, the size it gets, and
        /// since when. It is drawn at the new size until sway reports a size for it.
        pub(super) scaled: RefCell<Option<super::Scaled>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Canvas {
        const NAME: &'static str = "StDisplayCanvas";
        type Type = super::Canvas;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-display-canvas");
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }
    }

    impl ObjectImpl for Canvas {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                // The output picked on the canvas, by its identifier.
                vec![
                    Signal::builder("selected")
                        .param_types([String::static_type()])
                        .build(),
                ]
            })
        }

        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Canvas {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            super::view::measure(orientation)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            self.obj().place_tiles(width, height);
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            // The tiles with their grips, the dragged one last so it is on top, then the guide
            // lines.
            let dragged = self
                .drag
                .borrow()
                .as_ref()
                .map(|drag| drag.identifier.clone());
            let tiles = self.tiles.borrow();
            let mut last = None;
            for tile in tiles.iter() {
                if dragged.as_deref() == Some(tile.identifier.as_str()) {
                    last = Some(tile);
                } else {
                    widget.snapshot_child(&tile.button, snapshot);
                    widget.snapshot_child(&tile.grip, snapshot);
                }
            }
            if let Some(tile) = last {
                widget.snapshot_child(&tile.button, snapshot);
                widget.snapshot_child(&tile.grip, snapshot);
            }
            if let Some((vertical, horizontal)) = self.guides.get() {
                widget.snapshot_child(vertical, snapshot);
                widget.snapshot_child(horizontal, snapshot);
            }
        }
    }
}

glib::wrapper! {
    /// The outputs that are on, at their places, to pick and to drag.
    pub struct Canvas(ObjectSubclass<imp::Canvas>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Canvas {
    /// An empty canvas that applies the layout through `model`.
    pub fn new(model: &Model) -> Self {
        let canvas: Self = glib::Object::new();
        let _ = canvas.imp().model.set(model.clone());
        canvas.set_hexpand(true);
        canvas.set_overflow(gtk::Overflow::Hidden);
        canvas.update_property(&[gtk::accessible::Property::Label(tr(msg!(
            "Arrangement of the displays"
        )))]);
        let guide = |name: &str| {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            line.add_css_class("st-display-guide");
            line.set_can_target(false);
            line.set_visible(false);
            line.update_property(&[gtk::accessible::Property::Label(name)]);
            line.set_parent(&canvas);
            line
        };
        let _ = canvas.imp().guides.set((
            guide(tr(msg!("Vertical guide"))),
            guide(tr(msg!("Horizontal guide"))),
        ));
        canvas.add_drag();
        canvas
    }

    /// The tiles' buttons, in the order of the outputs.
    pub fn tiles(&self) -> Vec<(String, gtk::ToggleButton)> {
        self.imp()
            .tiles
            .borrow()
            .iter()
            .map(|tile| (tile.identifier.clone(), tile.button.clone()))
            .collect()
    }

    /// Where each output that is on is shown, in logical pixels.
    pub fn layout(&self) -> Vec<(String, Rect)> {
        self.imp()
            .tiles
            .borrow()
            .iter()
            .filter(|tile| !tile.off)
            .map(|tile| (tile.identifier.clone(), self.shown_rect(tile)))
            .collect()
    }

    /// Shows the connected outputs of `outputs`, numbered as `number` says (by identifier): those
    /// that are on at their places, those that are off greyed out right of them.
    ///
    /// When one output that is on changed its size (a new scale, rotation or mode) while the
    /// outputs were arranged without gaps, the outputs right of it and below it move along, so
    /// the layout stays without overlaps and gaps.
    pub fn set_outputs(&self, outputs: &Outputs, number: impl Fn(&str) -> Option<usize>) {
        let imp = self.imp();
        if imp.drag.borrow().is_some() {
            // The tiles stay as they are under the pointer; the outputs are shown once the drag
            // ends.
            let numbers = outputs
                .iter()
                .filter_map(|output| {
                    let identifier = output.identifier().to_owned();
                    number(&identifier).map(|number| (identifier, number))
                })
                .collect();
            imp.deferred.replace(Some((outputs.clone(), numbers)));
            return;
        }
        let mut shown: Vec<Shown> = outputs
            .iter()
            .filter(|output| output.is_connected())
            .map(|output| {
                let showing = imp
                    .showing
                    .borrow()
                    .iter()
                    .find(|(id, _)| id == output.identifier())
                    .map(|(_, what)| what.clone());
                Shown {
                    identifier: output.identifier().to_owned(),
                    name: output.name().to_owned(),
                    rect: output
                        .rect()
                        .filter(|_| output.is_enabled() && showing.is_none()),
                    label: output.label(),
                    short: short_label(output),
                    details: showing.unwrap_or_else(|| details(output)),
                    scale: output.scale().unwrap_or(1.0),
                    size: parked_size(output),
                    parked: None,
                }
            })
            .collect();
        park(&mut shown);
        let reported: Vec<(String, Rect, String)> = shown
            .iter()
            .filter_map(|shown| Some((shown.identifier.clone(), shown.rect?, shown.label.clone())))
            .collect();
        self.forget_dropped_if_reported(&reported);
        let same = {
            let tiles = imp.tiles.borrow();
            tiles.len() == shown.len()
                && tiles
                    .iter()
                    .zip(&shown)
                    .all(|(tile, shown)| tile.identifier == shown.identifier)
        };
        if !same {
            for tile in imp.tiles.take() {
                tile.button.unparent();
                tile.grip.unparent();
            }
            let tiles = shown
                .iter()
                .map(|shown| Tile {
                    identifier: shown.identifier.clone(),
                    rect: Rect::default(),
                    off: false,
                    button: self.tile_button(&shown.identifier),
                    grip: self.grip(),
                    scale: 1.0,
                    details: String::new(),
                    name: String::new(),
                    number: None,
                    label: String::new(),
                })
                .collect();
            imp.tiles.replace(tiles);
        }
        for (tile, shown) in imp.tiles.borrow_mut().iter_mut().zip(&shown) {
            tile.off = shown.rect.is_none();
            tile.scale = shown.scale;
            tile.details.clone_from(&shown.details);
            tile.name.clone_from(&shown.name);
            tile.label.clone_from(&shown.label);
            tile.number = number(&tile.identifier);
            tile.rect = shown.rect.or(shown.parked).unwrap_or_default();
            let number = number(&tile.identifier).map_or_else(String::new, |n| n.to_string());
            set_tile_labels(
                &tile.button,
                &number,
                &shown.label,
                &shown.short,
                &shown.details,
                tile.rect,
                tile.off,
            );
        }
        self.show_selected();
        self.queue_allocate();
        self.queue_draw();
        self.follow_size_changes();
    }

    /// Compares the layout with the one before the last change the app stored: when outputs
    /// changed their size with what the app stores for them, the layout flows along. While a risky
    /// change waits, the layout before it stays the one to compare with, so a change that is
    /// reverted moves nothing and one that is kept moves the others then.
    fn follow_size_changes(&self) {
        let imp = self.imp();
        // While a change is sent or waits to be kept, the layout before it stays the one to
        // compare with: sway's report and what the app stores arrive in either order.
        let Some(model) = imp.model.get() else {
            return;
        };
        if model.is_busy() {
            return;
        }
        if model.pending_change().is_some() {
            self.move_along_with_trial(model);
            return;
        }
        let layout = self.layout();
        let baseline = imp.baseline.take();
        let Some((before, sizes_before)) = baseline else {
            imp.baseline.replace(Some((layout, self.stored_sizes())));
            return;
        };
        let sizes = self.stored_sizes();
        if before == layout {
            // Nothing moved or resized yet. A stored size counts for the report of sway that
            // follows it, for a moment; one that no new size followed (a mode of the same size)
            // counts no longer after that, so a later change from elsewhere isn't taken for it.
            if sizes != sizes_before && imp.stored_at.get().is_none() {
                imp.stored_at.set(Some(Instant::now()));
                self.recheck_after(HOLD);
            }
            let stale = imp.stored_at.get().is_some_and(|at| at.elapsed() >= HOLD);
            if stale {
                imp.stored_at.set(None);
                imp.baseline.replace(Some((before, sizes)));
            } else {
                imp.baseline.replace(Some((before, sizes_before)));
            }
            return;
        }
        let resized = keep_together(&before, &layout).map(|(resized, _)| resized);
        let stored_yet = resized.as_ref().is_some_and(|resized| {
            resized
                .iter()
                .any(|id| sizes_before.get(id) != sizes.get(id))
        });
        // One output changed its size, but what is stored for it hasn't caught up yet (sway
        // reported a change of the command line before the settings file was read again): the
        // layout before stays the one to compare with, for a moment.
        let held = imp.held_since.get();
        if resized.is_some() && !stored_yet && held.is_none_or(|since| since.elapsed() < HOLD) {
            if held.is_none() {
                imp.held_since.set(Some(Instant::now()));
                self.recheck_after(HOLD);
            }
            imp.baseline.replace(Some((before, sizes_before)));
            return;
        }
        imp.held_since.set(None);
        imp.stored_at.set(None);
        self.follow_resize(&before, &sizes_before, &sizes);
        imp.baseline.replace(Some((self.layout(), sizes)));
    }

    /// While the trial of a display's new size (a mode or a rotation) waits: the others move along
    /// so that what touched before still touches, now and not only once it is kept. The layout
    /// before the trial stays the one to compare with, so Revert puts back all of it.
    fn move_along_with_trial(&self, model: &Model) {
        let before = self
            .imp()
            .baseline
            .borrow()
            .as_ref()
            .map(|(before, _)| before.clone());
        let Some(before) = before else {
            return;
        };
        let layout = self.layout();
        // A display switched off in the trial: the gap it leaves closes, so nothing floats.
        if before.len() == layout.len() + 1 {
            let vacated = before
                .iter()
                .find(|(id, _)| layout.iter().all(|(shown, _)| shown != id))
                .map(|(_, rect)| *rect);
            let rects: Vec<Rect> = layout.iter().map(|(_, rect)| *rect).collect();
            let was_tidy =
                layout::is_tidy(&before.iter().map(|(_, rect)| *rect).collect::<Vec<_>>());
            if let Some(closed) = vacated
                .filter(|_| was_tidy && !layout::is_tidy(&rects))
                .and_then(|vacated| layout::close_gap(&rects, vacated))
            {
                let places: Vec<(String, Value)> = layout
                    .iter()
                    .zip(closed)
                    .filter(|((_, now), new)| (now.x, now.y) != (new.x, new.y))
                    .map(|((id, _), new)| (id.clone(), Value::Text(format!("{},{}", new.x, new.y))))
                    .collect();
                if !places.is_empty() {
                    model.move_along(places);
                }
            }
            return;
        }
        // From the places before the trial, with the sizes now: another value tried in its place
        // (back to the one before, say) puts the others where that one needs them, not where
        // the first one did.
        let sized: Vec<(String, Rect)> = layout
            .iter()
            .map(|(id, now)| {
                let was = before
                    .iter()
                    .find(|(known, _)| known == id)
                    .map_or(*now, |(_, was)| *was);
                (
                    id.clone(),
                    Rect {
                        x: was.x,
                        y: was.y,
                        ..*now
                    },
                )
            })
            .collect();
        let arranged = keep_together(&before, &sized).map_or(sized, |(_, arranged)| arranged);
        let places: Vec<(String, Value)> = arranged
            .iter()
            .filter(|(id, rect)| {
                layout
                    .iter()
                    .any(|(shown, now)| shown == id && (now.x, now.y) != (rect.x, rect.y))
            })
            .map(|(id, rect)| (id.clone(), Value::Text(format!("{},{}", rect.x, rect.y))))
            .collect();
        if !places.is_empty() {
            model.move_along(places);
        }
    }

    /// Looks at the sizes again after `delay`, so what is held or stored doesn't count longer
    /// than that when nothing else changes.
    fn recheck_after(&self, delay: Duration) {
        glib::timeout_add_local_once(
            delay + Duration::from_millis(50),
            glib::clone!(
                #[weak(rename_to = canvas)]
                self,
                move || canvas.follow_size_changes()
            ),
        );
    }

    /// What the app stores for the size of each output that is on, its own or for every output:
    /// its scale, rotation and mode.
    fn stored_sizes(&self) -> Sizes {
        let Some(model) = self.imp().model.get() else {
            return Sizes::new();
        };
        let registry = sway_things_core::registry::Registry::builtin();
        self.layout()
            .into_iter()
            .map(|(identifier, _)| {
                let values = [SCALE, TRANSFORM, MODE]
                    .iter()
                    .map(|key| {
                        // Its own value or the one for every output, as long as it is stored.
                        let setting = registry.get(key)?;
                        let view = model.view(setting, Some(&identifier))?;
                        matches!(view.source, Source::Stored { .. }).then_some(view.value)
                    })
                    .collect();
                (identifier, values)
            })
            .collect()
    }

    /// Flows the layout ([`layout::reflow`]) after one or more outputs changed their size since
    /// `before`, if the layout had neither gaps nor overlaps and now has, and the change came
    /// through the app: what it stores or tries for one of their sizes changed too. A change from
    /// elsewhere (`swaymsg`) is left as it is.
    fn follow_resize(&self, before: &[(String, Rect)], sizes_before: &Sizes, sizes: &Sizes) {
        let after = self.layout();
        if self.imp().dropped.borrow().is_some() || before.len() != after.len() {
            return;
        }
        let Some((resized, arranged)) = keep_together(before, &after) else {
            return;
        };
        // Only what the app changed: what it stores for one of them changed too.
        if resized
            .iter()
            .all(|id| sizes_before.get(id) == sizes.get(id))
        {
            return;
        }
        self.show_dropped(&arranged);
        self.apply(&arranged);
    }

    /// Each connected output's name (its connector), number, make and model, and whether it is
    /// off, as the tiles show them.
    pub fn numbered(&self) -> Vec<(String, usize, String, bool)> {
        self.imp()
            .tiles
            .borrow()
            .iter()
            .filter_map(|tile| {
                let number = tile.number?;
                Some((tile.name.clone(), number, tile.label.clone(), tile.off))
            })
            .collect()
    }

    /// Presses the tile of the output `identifier`, or none.
    pub fn select(&self, identifier: Option<&str>) {
        self.imp().selected.replace(identifier.map(str::to_owned));
        self.show_selected();
    }

    /// Calls `callback` with the identifier of the output picked on the canvas.
    pub fn connect_selected<F: Fn(&Self, &str) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "selected",
            false,
            closure_local!(move |canvas: &Self, identifier: &str| callback(canvas, identifier)),
        )
    }

    /// Moves the output `identifier` so that its top left corner is at `x`, `y` in the layout,
    /// as a drop there would: settled next to the others, then applied.
    pub fn move_output(&self, identifier: &str, x: i32, y: i32) {
        let layout = self.layout();
        let Some((_, rect)) = layout.iter().find(|(id, _)| id == identifier) else {
            return;
        };
        let moved = Rect { x, y, ..*rect };
        self.drop_at(identifier, moved, None, &layout);
    }

    /// Puts the outputs side by side, in their order from left to right, with their tops in
    /// line, and applies that.
    pub fn arrange_side_by_side(&self) {
        let mut layout = self.layout();
        layout.sort_by_key(|(_, rect)| (rect.x, rect.y));
        let mut x = 0_i32;
        let arranged: Vec<(String, Rect)> = layout
            .into_iter()
            .map(|(identifier, rect)| {
                let placed = Rect { x, y: 0, ..rect };
                x = x.saturating_add(rect.width);
                (identifier, placed)
            })
            .collect();
        self.arrange(&arranged, true);
    }

    fn tile_button(&self, identifier: &str) -> gtk::ToggleButton {
        let number = gtk::Label::new(None);
        number.add_css_class("st-display-number");
        let name = gtk::Label::new(None);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_wrap(true);
        name.set_lines(2);
        name.set_justify(gtk::Justification::Center);
        name.set_max_width_chars(1);
        name.add_css_class("st-display-name");
        let details = gtk::Label::new(None);
        details.set_ellipsize(gtk::pango::EllipsizeMode::End);
        details.set_max_width_chars(1);
        details.add_css_class("st-display-details");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
        content.set_valign(gtk::Align::Center);
        content.append(&number);
        content.append(&name);
        content.append(&details);
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&content));
        // What doesn't fit the tile stays inside it: `fit_labels` leaves out what doesn't fit.
        button.set_overflow(gtk::Overflow::Hidden);
        button.add_css_class("st-display-tile");
        button.set_parent(self);
        let identifier = identifier.to_owned();
        button.connect_toggled(glib::clone!(
            #[weak(rename_to = canvas)]
            self,
            #[strong]
            identifier,
            move |button| {
                if canvas.imp().updating.get() {
                    return;
                }
                if button.is_active() {
                    canvas.pick(&identifier);
                } else {
                    // The picked output stays picked: a second click doesn't let go of it.
                    canvas.show_selected();
                }
            }
        ));
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = canvas)]
            self,
            #[strong]
            identifier,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| canvas.nudge(&identifier, key, modifiers)
        ));
        button.add_controller(keys);
        button
    }

    /// Locks the layout while a risky change waits (so Revert puts back only that change): the
    /// tiles can still be picked, but not moved, nudged or scaled. The grip goes meanwhile.
    pub fn set_locked(&self, locked: bool) {
        if self.imp().locked.replace(locked) != locked {
            self.queue_allocate();
        }
    }

    /// Notes the displays that show another display's picture (by identifier, with what their
    /// tile says): they stand apart, so they are parked beside the others like ones that are off.
    pub fn set_showing(&self, showing: Vec<(String, String)>) {
        self.imp().showing.replace(showing);
    }

    /// Picks the output `identifier` and tells.
    fn pick(&self, identifier: &str) {
        self.imp().selected.replace(Some(identifier.to_owned()));
        self.show_selected();
        self.emit_by_name::<()>("selected", &[&identifier.to_owned()]);
    }

    /// Presses the tile of the picked output and releases the others.
    fn show_selected(&self) {
        let imp = self.imp();
        let selected = imp.selected.borrow().clone();
        imp.updating.set(true);
        for tile in imp.tiles.borrow().iter() {
            tile.button
                .set_active(selected.as_deref() == Some(tile.identifier.as_str()));
        }
        imp.updating.set(false);
        // The grip goes with the pick.
        self.queue_allocate();
    }

    /// Moves the output `identifier` with an arrow key.
    fn nudge(
        &self,
        identifier: &str,
        key: gtk::gdk::Key,
        modifiers: gtk::gdk::ModifierType,
    ) -> glib::Propagation {
        if self.imp().locked.get() {
            return glib::Propagation::Proceed;
        }
        let step = if modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
            NUDGE * 10
        } else {
            NUDGE
        };
        let (dx, dy) = match key {
            gtk::gdk::Key::Left => (-step, 0),
            gtk::gdk::Key::Right => (step, 0),
            gtk::gdk::Key::Up => (0, -step),
            gtk::gdk::Key::Down => (0, step),
            _ => return glib::Propagation::Proceed,
        };
        let layout = self.layout();
        if let Some((_, rect)) = layout.iter().find(|(id, _)| id == identifier) {
            let moved = Rect {
                x: rect.x.saturating_add(dx),
                y: rect.y.saturating_add(dy),
                ..*rect
            };
            self.drop_at(identifier, moved, None, &layout);
        }
        glib::Propagation::Stop
    }

    /// Where `tile` is shown: where it is dragged, where it was dropped, or where sway has it.
    fn shown_rect(&self, tile: &Tile) -> Rect {
        let imp = self.imp();
        if let Some(drag) = imp.drag.borrow().as_ref() {
            if drag.identifier == tile.identifier && drag.moving {
                return drag.rect;
            }
        }
        if let Some((dropped, _)) = imp.dropped.borrow().as_ref() {
            if let Some((_, rect)) = dropped.iter().find(|(id, _)| *id == tile.identifier) {
                return *rect;
            }
        }
        tile.rect
    }

    /// Where a tile is drawn: where it is shown, or for a tile just scaled with its grip, at its
    /// new size until sway reports a size for it (or for [`SETTLING`]), so it doesn't snap back
    /// in between. Only the drawing: the layout is what sway reports.
    fn drawn_rect(&self, tile: &Tile) -> Rect {
        // Making room for a tile dragged over a slot.
        if let Some(drag) = self.imp().drag.borrow().as_ref() {
            if let Some((_, rect)) = drag.room.iter().find(|(id, _)| *id == tile.identifier) {
                return *rect;
            }
        }
        let shown = self.shown_rect(tile);
        let scaled = self.imp().scaled.borrow();
        match scaled.as_ref() {
            Some((identifier, size, rect, since))
                if *identifier == tile.identifier
                    && (shown.width, shown.height) == *size
                    && since.elapsed() < SETTLING =>
            {
                Rect {
                    x: shown.x,
                    y: shown.y,
                    ..*rect
                }
            }
            _ => shown,
        }
    }

    /// Forgets the dropped layout once sway reports it, or after a while.
    fn forget_dropped_if_reported(&self, enabled: &[(String, Rect, String)]) {
        let imp = self.imp();
        let done = imp
            .dropped
            .borrow()
            .as_ref()
            .is_some_and(|(dropped, since)| {
                since.elapsed() > SETTLING
                    || dropped.iter().all(|(identifier, rect)| {
                        enabled
                            .iter()
                            .any(|(id, reported, _)| id == identifier && reported == rect)
                    })
            });
        if done {
            imp.dropped.replace(None);
        }
    }

    /// Ctrl+Z for an ancestor of the canvas (the page): goes back to the arrangement before the
    /// last one, wherever the focus is on the page. A field that has the focus undoes its own
    /// typing first.
    pub fn undo_shortcut(&self) -> gtk::EventControllerKey {
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = canvas)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                let undo = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK)
                    && matches!(key, gtk::gdk::Key::z | gtk::gdk::Key::Z);
                if undo && canvas.undo() {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        keys
    }

    /// Goes back to the arrangement before the last one the user made. Returns whether there
    /// was one.
    pub fn undo(&self) -> bool {
        if !self.is_sensitive() || self.imp().locked.get() {
            return false;
        }
        let previous = self.imp().history.borrow_mut().pop();
        let Some(previous) = previous else {
            return false;
        };
        // Only the outputs that are still on.
        let current: Vec<String> = self.layout().into_iter().map(|(id, _)| id).collect();
        let previous: Vec<(String, Rect)> = previous
            .into_iter()
            .filter(|(id, _)| current.contains(id))
            .collect();
        if previous.len() != current.len() {
            return false;
        }
        self.arrange(&previous, false);
        true
    }

    /// Stores the position of every output of `arranged` that isn't stored as it is.
    fn apply(&self, arranged: &[(String, Rect)]) {
        let Some(model) = self.imp().model.get() else {
            return;
        };
        let Some(setting) = sway_things_core::registry::Registry::builtin().get(POSITION) else {
            return;
        };
        for (identifier, rect) in arranged {
            let value = Value::Text(format!("{},{}", rect.x, rect.y));
            let stored = model.stored_in(setting, Some(identifier)).is_some();
            let current = model.view(setting, Some(identifier)).map(|view| view.value);
            if !stored || current.as_ref() != Some(&value) {
                model.set(POSITION, Some(identifier), value);
            }
        }
    }
}

impl Default for Canvas {
    fn default() -> Self {
        glib::Object::new()
    }
}
