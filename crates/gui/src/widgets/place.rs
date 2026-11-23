// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The place on a page while a part of it is built again: how far it is scrolled, which widget
//! has the focus and where that widget is on screen.
//!
//! A part that builds its widgets again after a change (the list of bindings, the cards of the
//! Workspaces map) takes the [`Place`] before it removes them and puts it back after:
//!
//! - The focus doesn't fall out of the page. A focused widget that is removed makes GTK move the
//!   focus to the next widget of the window that takes it, before the next frame, which scrolls
//!   the page there. So the focus is taken off before the widgets go, and given to the new
//!   widget that stands for the old one, or to the one after it.
//! - The page stays where it was: once the new widgets are laid out, it is scrolled so that the
//!   widget with the focus is where the old one was on screen. Without a focus in the page, it
//!   keeps how far it was scrolled.
//!
//! A change can build a part again more than once before the next frame (the model says it
//! changed when a change is queued and when it is applied). The place taken first counts until
//! the page is laid out: the widgets built in between have no position yet.

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;

/// The place on the page around a widget, taken before a part of it is built again.
#[derive(Debug)]
pub struct Place {
    scroller: gtk::ScrolledWindow,
    /// How far the page was scrolled.
    value: f64,
    /// The widget with the focus, if it is on the page.
    focus: Option<gtk::Widget>,
    /// Where the widget with the focus was in the page's content, from its top.
    y: Option<f64>,
    /// Whether it was taken while the page waited for its layout after another change.
    waiting: bool,
}

/// A place waiting for the next layout of its page.
#[derive(Debug)]
struct Pending {
    scroller: glib::WeakRef<gtk::ScrolledWindow>,
    value: f64,
    y: Option<f64>,
    /// The widget to keep where the focus was.
    anchor: Option<gtk::Widget>,
}

thread_local! {
    /// The places waiting for the next layout of their page.
    static PENDING: RefCell<Vec<Pending>> = const { RefCell::new(Vec::new()) };
}

impl Place {
    /// The place of the page that `widget` is on: `None` if it isn't in a scrolled window.
    pub fn of(widget: &impl IsA<gtk::Widget>) -> Option<Self> {
        let scroller = widget
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()?;
        let focus = widget
            .root()
            .and_then(|root| root.focus())
            .filter(|focus| focus.is_ancestor(&scroller));
        let pending = PENDING.with(|pending| {
            pending
                .borrow()
                .iter()
                .find(|place| place.scroller.upgrade().as_ref() == Some(&scroller))
                .map(|place| (place.value, place.y))
        });
        let waiting = pending.is_some();
        let (value, y) = pending.unwrap_or_else(|| {
            let y = focus.as_ref().and_then(|focus| content_y(&scroller, focus));
            (scroller.vadjustment().value(), y)
        });
        Some(Self {
            scroller,
            value,
            focus,
            y,
            waiting,
        })
    }

    /// Keeps `widget` in place of the widget with the focus, which is in it: the row of a
    /// focused button, whose new row takes the focus.
    pub fn keep(&mut self, widget: &gtk::Widget) {
        if !self.waiting {
            self.y = content_y(&self.scroller, widget);
        }
    }

    /// The widget with the focus on the page, if there is one.
    pub fn focus(&self) -> Option<&gtk::Widget> {
        self.focus.as_ref()
    }

    /// Takes the focus off the page while its widgets are built again, if the focused widget is
    /// in `part`, so that GTK doesn't move it elsewhere when the widget goes.
    pub fn release(&self, part: &impl IsA<gtk::Widget>) {
        if let Some(focus) = &self.focus {
            if focus == part.upcast_ref() || focus.is_ancestor(part) {
                if let Some(root) = focus.root() {
                    root.set_focus(None::<&gtk::Widget>);
                }
            }
        }
    }

    /// Puts the place back: gives the focus to `focus`, the widget that stands for the one that
    /// had it (or to that one, if it is still on the page), and once the page is laid out again,
    /// scrolls it so that the focused widget is where the old one was on screen.
    pub fn restore(self, focus: Option<gtk::Widget>) {
        self.put(focus, true);
    }

    /// Puts the place back after the widget with the focus went: gives the focus to `next`, the
    /// one after it, and keeps how far the page is scrolled, so the rows after it move up.
    pub fn restore_after_removal(self, next: Option<gtk::Widget>) {
        self.put(next, false);
    }

    fn put(self, focus: Option<gtk::Widget>, keep_focus_in_place: bool) {
        let Self {
            scroller,
            value,
            focus: old,
            y,
            waiting: _,
        } = self;
        let y = y.filter(|_| keep_focus_in_place);
        let anchor = focus.or_else(|| old.filter(|old| old.is_ancestor(&scroller)));
        if let Some(anchor) = &anchor {
            if !anchor.has_focus() {
                anchor.grab_focus();
            }
        }
        let place = Pending {
            scroller: scroller.downgrade(),
            value,
            y,
            anchor,
        };
        let waiting = PENDING.with(|pending| {
            let mut pending = pending.borrow_mut();
            pending.retain(|place| place.scroller.upgrade().is_some());
            let found = pending
                .iter_mut()
                .find(|other| other.scroller.upgrade().as_ref() == Some(&scroller));
            if let Some(other) = found {
                *other = place;
                true
            } else {
                pending.push(place);
                false
            }
        });
        if !waiting {
            after_layout(&scroller);
        }
    }
}

/// Where `widget` is in the content of `scroller`, from its top: the same however far it is
/// scrolled. `None` while it isn't laid out.
fn content_y(scroller: &gtk::ScrolledWindow, widget: &gtk::Widget) -> Option<f64> {
    let content = match scroller.child().and_downcast::<gtk::Viewport>() {
        Some(viewport) => viewport.child()?,
        None => scroller.child()?,
    };
    let point = widget.compute_point(&content, &gtk::graphene::Point::new(0.0, 0.0))?;
    Some(f64::from(point.y()))
}

/// Puts back the place waiting for `scroller` once its page is laid out next. Without a frame
/// clock (the window isn't shown), at once.
fn after_layout(scroller: &gtk::ScrolledWindow) {
    let Some(clock) = scroller.frame_clock() else {
        put_back(scroller);
        return;
    };
    // The window lays out in its own handler of this signal, which was connected before.
    let handler = std::rc::Rc::new(RefCell::new(None::<glib::SignalHandlerId>));
    let weak = scroller.downgrade();
    let id = clock.connect_layout(glib::clone!(
        #[strong]
        handler,
        move |clock| {
            if let Some(id) = handler.borrow_mut().take() {
                clock.disconnect(id);
            }
            if let Some(scroller) = weak.upgrade() {
                put_back(&scroller);
            }
        }
    ));
    handler.replace(Some(id));
    scroller.queue_allocate();
}

/// Scrolls `scroller` so that the widget of the place waiting for it is where the focus was, or
/// as far as it was without one.
fn put_back(scroller: &gtk::ScrolledWindow) {
    let place = PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        let index = pending
            .iter()
            .position(|place| place.scroller.upgrade().as_ref() == Some(scroller))?;
        Some(pending.swap_remove(index))
    });
    let Some(place) = place else { return };
    let moved = place
        .anchor
        .as_ref()
        .and_then(|anchor| content_y(scroller, anchor))
        .zip(place.y)
        .map_or(0.0, |(now, was)| now - was);
    scroller.vadjustment().set_value(place.value + moved);
}
