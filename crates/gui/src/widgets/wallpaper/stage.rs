// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The area the display tiles of the preview stand in: each tile at its output's place, fitted
//! into the area as the Displays canvas fits the arrangement ([`View`]).

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::outputs::Rect;

use crate::widgets::displays::View;

mod imp {
    use std::cell::RefCell;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct Stage {
        /// The tiles and the places of their outputs.
        pub(super) tiles: RefCell<Vec<super::Tile>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Stage {
        const NAME: &'static str = "StWallpaperStage";
        type Type = super::Stage;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-wallpaper-stage");
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }
    }

    impl ObjectImpl for Stage {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Stage {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            crate::widgets::displays::measure(orientation)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            self.obj().place_tiles(width, height);
        }
    }
}

/// A tile and the place of its output.
type Tile = (gtk::Button, Rect);

glib::wrapper! {
    /// The display tiles of the Wallpaper page, placed like the output layout.
    pub struct Stage(ObjectSubclass<imp::Stage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Stage {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl Stage {
    /// Shows `tiles` in place of the ones shown: each a button and the place of its output.
    pub fn set_tiles(&self, tiles: Vec<Tile>) {
        for (tile, _) in self.imp().tiles.take() {
            tile.unparent();
        }
        for (tile, _) in &tiles {
            tile.set_parent(self);
        }
        self.imp().tiles.replace(tiles);
        self.queue_allocate();
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "positions in the preview are within its few hundred pixels"
    )]
    fn place_tiles(&self, width: i32, height: i32) {
        let tiles = self.imp().tiles.borrow();
        let rects: Vec<Rect> = tiles.iter().map(|(_, rect)| *rect).collect();
        let Some(view) = View::of(&rects, width, height) else {
            return;
        };
        for (tile, rect) in tiles.iter() {
            let (x, y, w, h) = view.tile(*rect);
            tile.size_allocate(
                &gtk::Allocation::new(
                    x.round() as i32,
                    y.round() as i32,
                    w.round() as i32,
                    h.round() as i32,
                ),
                -1,
            );
        }
    }
}
