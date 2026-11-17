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

use std::time::{Duration, Instant};

use gtk::glib;
use gtk::glib::closure_local;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::command::output::Transform;
use sway_things_core::outputs::layout::{self, Guide};
use sway_things_core::outputs::{MODE, Output, Outputs, POSITION, Rect, SCALE, TRANSFORM};
use sway_things_core::registry::Value;
use sway_things_core::view::Source;

use crate::model::Model;

/// How close, in pixels on the screen, an edge has to come to snap.
const SNAP_DISTANCE: f64 = 12.0;
/// How far, in pixels on the screen, the pointer moves before a press becomes a drag.
const DRAG_START: f64 = 4.0;
/// The room around the layout in the canvas, in pixels.
const PADDING: f64 = 24.0;
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

/// An output on the canvas.
#[derive(Debug, Clone)]
struct Tile {
    identifier: String,
    /// Its place in the layout, or for an output that is off, where it is parked.
    rect: Rect,
    /// Whether it is off: shown greyed out beside the others, and not dragged.
    off: bool,
    button: gtk::ToggleButton,
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
    details: String,
    /// The size it is parked with while it is off.
    size: (i32, i32),
    parked: Option<Rect>,
}

/// Parks the outputs of `shown` that are off in a column right of those that are on.
fn park(shown: &mut [Shown]) {
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
fn details(output: &Output) -> String {
    if !output.is_enabled() {
        return "Off".to_owned();
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
        .map(|scale| format!(" \u{b7} {:.0}\u{a0}%", scale * 100.0))
        .unwrap_or_default();
    format!("{} \u{d7} {}{scaled}{turned}", mode.width(), mode.height())
}

/// A transform as people read it.
fn turn(transform: Transform) -> &'static str {
    match transform {
        Transform::Normal => "",
        Transform::R90 => "90\u{b0}",
        Transform::R180 => "180\u{b0}",
        Transform::R270 => "270\u{b0}",
        Transform::Flipped => "Flipped",
        Transform::Flipped90 => "Flipped, 90\u{b0}",
        Transform::Flipped180 => "Flipped, 180\u{b0}",
        Transform::Flipped270 => "Flipped, 270\u{b0}",
    }
}

/// The size an output that is off is shown with: its largest mode, or a common one.
fn parked_size(output: &Output) -> (i32, i32) {
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
fn keep_together(
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

/// How the layout maps onto the canvas: canvas = offset + logical × scale.
#[derive(Debug, Clone, Copy, PartialEq)]
struct View {
    scale: f64,
    x: f64,
    y: f64,
}

impl View {
    /// The view that fits `bounds` into `width` × `height`, centered.
    fn fit(bounds: Rect, width: f64, height: f64) -> Self {
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

    fn x(self, logical: i32) -> f64 {
        self.x + f64::from(logical) * self.scale
    }

    fn y(self, logical: i32) -> f64 {
        self.y + f64::from(logical) * self.scale
    }

    /// A distance on the canvas in logical pixels.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a distance on the screen divided by a positive scale fits easily"
    )]
    fn logical(self, canvas: f64) -> i32 {
        (canvas / self.scale).round() as i32
    }
}

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
            match orientation {
                gtk::Orientation::Horizontal => (120, 600, -1, -1),
                _ => (180, 260, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            self.obj().place_tiles(width, height);
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            // The tiles, the dragged one last so it is on top, then the guide lines.
            let dragged = self
                .drag
                .borrow()
                .as_ref()
                .map(|drag| drag.identifier.clone());
            let tiles = self.tiles.borrow();
            let mut last = None;
            for tile in tiles.iter() {
                if dragged.as_deref() == Some(tile.identifier.as_str()) {
                    last = Some(tile.button.clone());
                } else {
                    widget.snapshot_child(&tile.button, snapshot);
                }
            }
            if let Some(button) = last {
                widget.snapshot_child(&button, snapshot);
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
        canvas.update_property(&[gtk::accessible::Property::Label(
            "Arrangement of the displays",
        )]);
        let guide = |name: &str| {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            line.add_css_class("st-display-guide");
            line.set_can_target(false);
            line.set_visible(false);
            line.update_property(&[gtk::accessible::Property::Label(name)]);
            line.set_parent(&canvas);
            line
        };
        let _ = canvas
            .imp()
            .guides
            .set((guide("Vertical guide"), guide("Horizontal guide")));
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
            .map(|output| Shown {
                identifier: output.identifier().to_owned(),
                name: output.name().to_owned(),
                rect: output.rect().filter(|_| output.is_enabled()),
                label: output.label(),
                details: details(output),
                size: parked_size(output),
                parked: None,
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
            }
            let tiles = shown
                .iter()
                .map(|shown| Tile {
                    identifier: shown.identifier.clone(),
                    rect: Rect::default(),
                    off: false,
                    button: self.tile_button(&shown.identifier),
                    name: String::new(),
                    number: None,
                    label: String::new(),
                })
                .collect();
            imp.tiles.replace(tiles);
        }
        for (tile, shown) in imp.tiles.borrow_mut().iter_mut().zip(&shown) {
            tile.off = shown.rect.is_none();
            tile.name.clone_from(&shown.name);
            tile.label.clone_from(&shown.label);
            tile.number = number(&tile.identifier);
            tile.rect = shown.rect.or(shown.parked).unwrap_or_default();
            let number = number(&tile.identifier).map_or_else(String::new, |n| n.to_string());
            set_tile_labels(
                &tile.button,
                &number,
                &shown.label,
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
        let waits = imp
            .model
            .get()
            .is_some_and(|model| model.pending_change().is_some() || model.is_busy());
        if waits {
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
        self.drop_at(identifier, moved, &layout);
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
        details.set_wrap(true);
        details.set_lines(2);
        details.set_justify(gtk::Justification::Center);
        details.set_max_width_chars(1);
        details.add_css_class("st-display-details");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
        content.set_valign(gtk::Align::Center);
        content.append(&number);
        content.append(&name);
        content.append(&details);
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&content));
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
    }

    /// Moves the output `identifier` with an arrow key.
    fn nudge(
        &self,
        identifier: &str,
        key: gtk::gdk::Key,
        modifiers: gtk::gdk::ModifierType,
    ) -> glib::Propagation {
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
            self.drop_at(identifier, moved, &layout);
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

    /// The view of the current layout in a canvas of `width` × `height`.
    fn view(&self, width: i32, height: i32) -> Option<View> {
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
        let bounds = Rect {
            x: left,
            y: top,
            width: right.saturating_sub(left),
            height: bottom.saturating_sub(top),
        };
        Some(View::fit(bounds, f64::from(width), f64::from(height)))
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "positions on the canvas are within its few hundred pixels"
    )]
    fn place_tiles(&self, width: i32, height: i32) {
        let Some(view) = self.view(width, height) else {
            return;
        };
        for tile in self.imp().tiles.borrow().iter() {
            let rect = self.shown_rect(tile);
            // A small gap, so tiles that touch stay apart on the screen.
            let x = view.x(rect.x) + 1.0;
            let y = view.y(rect.y) + 1.0;
            let w = (f64::from(rect.width) * view.scale - 2.0).max(1.0);
            let h = (f64::from(rect.height) * view.scale - 2.0).max(1.0);
            let (min_w, _, _, _) = tile.button.measure(gtk::Orientation::Horizontal, -1);
            let (min_h, _, _, _) = tile.button.measure(gtk::Orientation::Vertical, -1);
            tile.button.size_allocate(
                &gtk::Allocation::new(
                    x.round() as i32,
                    y.round() as i32,
                    (w.round() as i32).max(min_w),
                    (h.round() as i32).max(min_h),
                ),
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
    fn place_guides(&self, view: View) {
        let imp = self.imp();
        let Some((vertical, horizontal)) = imp.guides.get() else {
            return;
        };
        let drag = imp.drag.borrow();
        let drag = drag.as_ref().filter(|drag| drag.moving);
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

    /// Moves tiles with the pointer: a press on a tile that moves far enough becomes a drag.
    fn add_drag(&self) {
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

    fn begin_drag(&self, x: f64, y: f64) {
        let imp = self.imp();
        let Some(tile) = self.tile_at(x, y).filter(|tile| !tile.off) else {
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
        }));
    }

    /// The tile under the point `x`, `y` of the canvas.
    fn tile_at(&self, x: f64, y: f64) -> Option<Tile> {
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

    fn update_drag(&self, gesture: &gtk::GestureDrag, dx: f64, dy: f64) {
        let imp = self.imp();
        let Some(view) = imp.frozen.get() else {
            return;
        };
        let others: Vec<Rect> = {
            let drag = imp.drag.borrow();
            let Some(drag) = drag.as_ref() else {
                return;
            };
            self.layout()
                .into_iter()
                .filter(|(identifier, _)| *identifier != drag.identifier)
                .map(|(_, rect)| rect)
                .collect()
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
        let moved = Rect {
            x: drag.start.x.saturating_add(view.logical(dx)),
            y: drag.start.y.saturating_add(view.logical(dy)),
            ..drag.start
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
    fn show_deferred(&self) {
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

    fn end_drag(&self) {
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
            self.show_deferred();
            return;
        }
        self.show_deferred();
        let Some(drag) = drag.filter(|drag| drag.moving) else {
            self.queue_allocate();
            return;
        };
        let layout = self.layout();
        self.drop_at(&drag.identifier, drag.rect, &layout);
        self.pick(&drag.identifier);
    }

    fn cancel_drag(&self) {
        let imp = self.imp();
        imp.drag.replace(None);
        imp.frozen.set(None);
        self.show_deferred();
        self.queue_allocate();
        self.queue_draw();
    }

    /// Settles the output `identifier` at `rect` among the others of `layout`, moves the layout
    /// to start at 0,0, shows it and applies it: every output gets its position stored.
    fn drop_at(&self, identifier: &str, rect: Rect, layout: &[(String, Rect)]) {
        let others: Vec<Rect> = layout
            .iter()
            .filter(|(id, _)| id != identifier)
            .map(|(_, rect)| *rect)
            .collect();
        let settled = layout::settle(rect, &others);
        let identifiers: Vec<String> = layout.iter().map(|(id, _)| id.clone()).collect();
        let mut rects: Vec<Rect> = layout
            .iter()
            .map(|(id, other)| if id == identifier { settled } else { *other })
            .collect();
        layout::normalize(&mut rects);
        let arranged: Vec<(String, Rect)> = identifiers.into_iter().zip(rects).collect();
        self.arrange(&arranged, true);
    }

    /// Shows `arranged` and applies it; with `remember`, Ctrl+Z goes back to the arrangement
    /// before.
    fn arrange(&self, arranged: &[(String, Rect)], remember: bool) {
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
        self.show_dropped(arranged);
        self.apply(arranged);
    }

    /// Shows `arranged` until sway reports it, or for [`SETTLING`] if it doesn't.
    fn show_dropped(&self, arranged: &[(String, Rect)]) {
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
        if !self.is_sensitive() {
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

/// Shows `number`, `label` and `details` on a tile, and says it all to screen readers.
fn set_tile_labels(
    button: &gtk::ToggleButton,
    number: &str,
    label: &str,
    details: &str,
    rect: Rect,
    off: bool,
) {
    let Some(content) = button.child().and_downcast::<gtk::Box>() else {
        return;
    };
    let labels: Vec<gtk::Label> =
        std::iter::successors(content.first_child(), WidgetExt::next_sibling)
            .filter_map(|child| child.downcast::<gtk::Label>().ok())
            .collect();
    if let [shown_number, name, shown_details] = labels.as_slice() {
        shown_number.set_text(number);
        name.set_text(label);
        shown_details.set_text(details);
    }
    if off {
        button.add_css_class("st-off");
    } else {
        button.remove_css_class("st-off");
    }
    button.set_tooltip_text(Some(label));
    let description = if off {
        format!("Display {number}, {label}, off")
    } else {
        format!(
            "Display {number}, {label}, {details}, at {}, {}",
            rect.x, rect.y
        )
    };
    button.update_property(&[gtk::accessible::Property::Label(&description)]);
}

impl Default for Canvas {
    fn default() -> Self {
        glib::Object::new()
    }
}
