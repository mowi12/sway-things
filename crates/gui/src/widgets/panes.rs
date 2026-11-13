// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The two panes of the window: the sidebar and the content pane, side by side, or one at a time
//! while the window is narrow.
//!
//! The window is narrow below a threshold, or below the width both panes need side by side if
//! that is more (larger fonts). The pane with the keyboard focus stays when the window gets
//! narrow; without the focus in either pane, the content pane does. The other pane is hidden
//! (unmapped), so `Tab` never reaches it.

use gtk::glib;
use gtk::glib::closure_local;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use std::cell::{Cell, RefCell};
    use std::sync::OnceLock;

    use gtk::glib;
    use gtk::glib::subclass::Signal;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug)]
    pub struct Panes {
        pub sidebar: RefCell<Option<gtk::Widget>>,
        pub separator: gtk::Separator,
        pub content: RefCell<Option<gtk::Widget>>,
        /// The width below which the panes are shown one at a time, in pixels.
        pub threshold: Cell<i32>,
        /// Whether the panes are shown one at a time.
        pub narrow: Cell<bool>,
        /// Which pane is shown while narrow: the content pane, or the sidebar.
        pub content_shown: Cell<bool>,
    }

    impl Default for Panes {
        fn default() -> Self {
            Self {
                sidebar: RefCell::default(),
                separator: gtk::Separator::new(gtk::Orientation::Vertical),
                content: RefCell::default(),
                threshold: Cell::new(0),
                narrow: Cell::new(false),
                content_shown: Cell::new(true),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Panes {
        const NAME: &'static str = "StPanes";
        type Type = super::Panes;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-panes");
        }
    }

    impl ObjectImpl for Panes {
        fn dispose(&self) {
            for child in [self.sidebar.take(), self.content.take()]
                .into_iter()
                .flatten()
            {
                child.unparent();
            }
            self.separator.unparent();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    // The panes switched between side by side and one at a time; the argument
                    // is whether they are shown one at a time now.
                    Signal::builder("narrow-changed")
                        .param_types([bool::static_type()])
                        .build(),
                ]
            })
        }
    }

    impl WidgetImpl for Panes {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let (Some(sidebar), Some(content)) = self.children() else {
                return (0, 0, -1, -1);
            };
            if orientation == gtk::Orientation::Horizontal {
                // As narrow as either pane alone, as wide as both side by side.
                let (sidebar_minimum, sidebar_natural, _, _) = sidebar.measure(orientation, -1);
                let (content_minimum, content_natural, _, _) = content.measure(orientation, -1);
                let (_, separator, _, _) = self.separator.measure(orientation, -1);
                let natural =
                    (sidebar_natural + separator + content_natural).max(self.threshold.get());
                return (sidebar_minimum.max(content_minimum), natural, -1, -1);
            }
            // As high as the higher pane, so showing the other one keeps the height.
            let widths = (for_size >= 0)
                .then(|| self.widths(for_size))
                .flatten()
                .unwrap_or((for_size, for_size));
            let (sidebar_minimum, sidebar_natural, _, _) = sidebar.measure(orientation, widths.0);
            let (content_minimum, content_natural, _, _) = content.measure(orientation, widths.1);
            (
                sidebar_minimum.max(content_minimum),
                sidebar_natural.max(content_natural),
                -1,
                -1,
            )
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let (Some(sidebar), Some(content)) = self.children() else {
                return;
            };
            let widths = self.widths(width);
            let narrow = widths.is_none();
            if narrow != self.narrow.get() {
                if narrow {
                    // The pane with the focus stays.
                    let focus = self.obj().root().and_then(|root| root.focus());
                    let in_sidebar = focus.is_some_and(|focus| focus.is_ancestor(&sidebar));
                    self.content_shown.set(!in_sidebar);
                }
                self.narrow.set(narrow);
                self.obj().show_children();
                // Outside the layout: the handlers may change what is shown.
                let panes = self.obj().downgrade();
                glib::idle_add_local_once(move || {
                    if let Some(panes) = panes.upgrade() {
                        let narrow = panes.is_narrow();
                        panes.emit_by_name::<()>("narrow-changed", &[&narrow]);
                    }
                });
            }
            match widths {
                Some((sidebar_width, content_width)) => {
                    let separator = width - sidebar_width - content_width;
                    sidebar.allocate(sidebar_width, height, baseline, None);
                    self.separator
                        .allocate(separator, height, baseline, Some(at(sidebar_width)));
                    content.allocate(
                        content_width,
                        height,
                        baseline,
                        Some(at(sidebar_width + separator)),
                    );
                }
                None if self.content_shown.get() => content.allocate(width, height, baseline, None),
                None => sidebar.allocate(width, height, baseline, None),
            }
        }
    }

    impl Panes {
        pub fn children(&self) -> (Option<gtk::Widget>, Option<gtk::Widget>) {
            (self.sidebar.borrow().clone(), self.content.borrow().clone())
        }

        /// The widths of the sidebar and the content pane side by side in `width`, or `None` if
        /// the panes are shown one at a time there.
        fn widths(&self, width: i32) -> Option<(i32, i32)> {
            let (Some(sidebar), Some(content)) = self.children() else {
                return None;
            };
            let orientation = gtk::Orientation::Horizontal;
            let (sidebar_minimum, sidebar_natural, _, _) = sidebar.measure(orientation, -1);
            let (content_minimum, _, _, _) = content.measure(orientation, -1);
            let (_, separator, _, _) = self.separator.measure(orientation, -1);
            let needed = sidebar_minimum + separator + content_minimum;
            if width < self.threshold.get().max(needed) {
                return None;
            }
            // The sidebar asks for a fixed width; the content pane takes the rest.
            let sidebar_width = sidebar_natural
                .min(width - separator - content_minimum)
                .max(sidebar_minimum);
            Some((sidebar_width, width - separator - sidebar_width))
        }
    }

    /// The transform that moves a child `x` pixels to the right.
    #[expect(
        clippy::cast_precision_loss,
        reason = "pixel offsets are far below 2^24, where f32 starts to round"
    )]
    fn at(x: i32) -> gtk::gsk::Transform {
        gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(x as f32, 0.0))
    }
}

glib::wrapper! {
    /// The sidebar and the content pane side by side, or below a threshold, one at a time.
    pub struct Panes(ObjectSubclass<imp::Panes>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Panes {
    /// `sidebar` and `content` side by side while the panes are at least `threshold` pixels wide,
    /// and one at a time below.
    pub fn new(
        sidebar: &impl IsA<gtk::Widget>,
        content: &impl IsA<gtk::Widget>,
        threshold: i32,
    ) -> Self {
        let panes: Self = glib::Object::new();
        let imp = panes.imp();
        imp.threshold.set(threshold);
        sidebar.set_parent(&panes);
        imp.separator.set_parent(&panes);
        content.set_parent(&panes);
        imp.sidebar.replace(Some(sidebar.clone().upcast()));
        imp.content.replace(Some(content.clone().upcast()));
        panes
    }

    /// The width below which the panes are shown one at a time, in pixels.
    pub fn threshold(&self) -> i32 {
        self.imp().threshold.get()
    }

    /// Changes the width below which the panes are shown one at a time.
    pub fn set_threshold(&self, threshold: i32) {
        self.imp().threshold.set(threshold);
        self.queue_resize();
    }

    /// Whether the panes are shown one at a time.
    pub fn is_narrow(&self) -> bool {
        self.imp().narrow.get()
    }

    /// Whether the sidebar is shown: always side by side, and while narrow, unless the content
    /// pane is.
    pub fn is_sidebar_shown(&self) -> bool {
        !self.is_narrow() || !self.imp().content_shown.get()
    }

    /// Whether the content pane is shown: always side by side, and while narrow, unless the
    /// sidebar is.
    pub fn is_content_shown(&self) -> bool {
        !self.is_narrow() || self.imp().content_shown.get()
    }

    /// Shows the sidebar instead of the content pane while narrow; the choice stays for the next
    /// time the window gets narrow.
    pub fn show_sidebar(&self) {
        self.show(false);
    }

    /// Shows the content pane instead of the sidebar while narrow.
    pub fn show_content(&self) {
        self.show(true);
    }

    fn show(&self, content: bool) {
        let imp = self.imp();
        if imp.content_shown.replace(content) != content {
            self.show_children();
            self.queue_allocate();
        }
    }

    /// Maps the panes that are shown and unmaps the others, so the keyboard focus can only reach
    /// what is shown.
    fn show_children(&self) {
        let imp = self.imp();
        let (sidebar, content) = imp.children();
        imp.separator.set_child_visible(!self.is_narrow());
        if let Some(sidebar) = sidebar {
            sidebar.set_child_visible(self.is_sidebar_shown());
        }
        if let Some(content) = content {
            content.set_child_visible(self.is_content_shown());
        }
    }

    /// Calls `callback` with whether the panes are shown one at a time, each time that changes.
    pub fn connect_narrow_changed<F: Fn(&Self, bool) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "narrow-changed",
            false,
            closure_local!(move |panes: &Self, narrow: bool| callback(panes, narrow)),
        )
    }
}
