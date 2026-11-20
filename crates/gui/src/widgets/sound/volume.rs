// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A volume control: a mute button, a slider and the level in percent.
//!
//! Moving the slider sends the level at once, but never two requests at a time: while one is on
//! its way, only the newest level waits, and it is sent next. What the server reports while the
//! slider is being moved (and for a moment after) doesn't move it back, so a slow answer can't
//! make the slider jump under the pointer.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;

use crate::i18n::{msg, tr, trf};

/// How long after the last move of the slider the server's reports are ignored for it.
const HOLD: Duration = Duration::from_millis(800);

/// What a control does with a level (a fraction, 1.0 is 100 %) or a mute.
pub type Send<T> = Rc<dyn Fn(T) -> Pin<Box<dyn Future<Output = ()>>>>;

/// Whether the control is for something that plays or something that records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An output or a stream that plays: speaker icons.
    Playing,
    /// An input or a stream that records: microphone icons.
    Recording,
}

/// A mute button, a slider and the level in percent, in one line.
#[derive(Clone)]
pub struct VolumeControl {
    line: gtk::Box,
    mute: gtk::ToggleButton,
    scale: gtk::Scale,
    percent: gtk::Label,
    kind: Kind,
    state: Rc<State>,
}

#[derive(Default)]
struct State {
    /// Set while the control is updated from the server, so the change isn't sent back.
    updating: Cell<bool>,
    /// Until when the server's reports don't move the slider.
    held_until: Cell<Option<Instant>>,
    /// The newest level not sent yet, while a request is on its way.
    wanted: Cell<Option<f64>>,
    /// Whether a request is on its way.
    busy: Cell<bool>,
    level: RefCell<Option<Send<f64>>>,
    muted: RefCell<Option<Send<bool>>>,
    /// The mute button's name for screen readers.
    mute_name: RefCell<String>,
    /// The level the server reported last, shown once the slider is let go.
    reported: Cell<Option<f64>>,
}

impl VolumeControl {
    /// A control named `name` for screen readers ("Output volume").
    pub fn new(name: &str, kind: Kind) -> Self {
        let mute = gtk::ToggleButton::new();
        mute.add_css_class("flat");
        mute.set_valign(gtk::Align::Center);
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.set_width_request(140);
        scale.set_valign(gtk::Align::Center);
        scale.set_increments(0.05, 0.1);
        scale.update_property(&[gtk::accessible::Property::Label(name)]);
        let percent = gtk::Label::new(None);
        percent.set_width_chars(5);
        percent.set_xalign(1.0);
        percent.add_css_class("numeric");
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.set_hexpand(true);
        line.append(&mute);
        line.append(&scale);
        line.append(&percent);
        let control = Self {
            line,
            mute,
            scale,
            percent,
            kind,
            state: Rc::default(),
        };
        control.state.mute_name.replace(tr(msg!("Mute")).to_owned());
        control.connect();
        control.show(0.0, false, 1.0);
        control
    }

    /// The line to put into a row.
    pub fn widget(&self) -> &gtk::Box {
        &self.line
    }

    /// The slider, for tests.
    pub fn scale(&self) -> &gtk::Scale {
        &self.scale
    }

    /// The mute button, for tests.
    pub fn mute_button(&self) -> &gtk::ToggleButton {
        &self.mute
    }

    /// Names the slider `volume` and the mute button `mute` for screen readers, such as
    /// "Volume of Firefox" and "Mute Firefox".
    pub fn set_names(&self, volume: &str, mute: &str) {
        self.scale
            .update_property(&[gtk::accessible::Property::Label(volume)]);
        if *self.state.mute_name.borrow() != mute {
            self.state.mute_name.replace(mute.to_owned());
            self.show_level(self.scale.value(), self.mute.is_active());
        }
    }

    /// Sends a new level with `level` and a mute with `muted`.
    pub fn on_change(&self, level: Send<f64>, muted: Send<bool>) {
        self.state.level.replace(Some(level));
        self.state.muted.replace(Some(muted));
    }

    /// Shows the server's `level` (a fraction) and `muted`, with the slider going up to
    /// `loudest`; the slider keeps its place while it is being moved.
    pub fn show(&self, level: f64, muted: bool, loudest: f64) {
        let state = &self.state;
        state.reported.set(Some(level));
        state.updating.set(true);
        // While a level is on its way or was just set, the server's report may be older.
        let held = state.busy.get()
            || state
                .held_until
                .get()
                .is_some_and(|until| Instant::now() < until);
        // Above the loudest offered (set elsewhere), the slider reaches the level.
        let loudest = loudest.max(level).max(1.0);
        let adjustment = self.scale.adjustment();
        if (adjustment.upper() - loudest).abs() > f64::EPSILON {
            adjustment.set_upper(loudest);
            self.scale.clear_marks();
            if loudest > 1.0 {
                self.scale.add_mark(1.0, gtk::PositionType::Bottom, None);
            }
        }
        if !held {
            self.scale.set_value(level);
        }
        self.mute.set_active(muted);
        self.show_level(self.scale.value(), muted);
        state.updating.set(false);
    }

    fn show_level(&self, level: f64, muted: bool) {
        show_level(
            &self.percent,
            &self.mute,
            self.kind,
            &self.state,
            level,
            muted,
        );
    }

    /// Connects the slider and the mute button. The handlers hold the widgets weakly, so a
    /// control whose row goes is freed with it.
    fn connect(&self) {
        let (percent, mute, kind) = (self.percent.downgrade(), self.mute.downgrade(), self.kind);
        let state = Rc::clone(&self.state);
        self.scale.connect_value_changed(move |scale| {
            let (Some(percent), Some(mute)) = (percent.upgrade(), mute.upgrade()) else {
                return;
            };
            let level = scale.value();
            show_level(&percent, &mute, kind, &state, level, mute.is_active());
            if state.updating.get() {
                return;
            }
            state.held_until.set(Some(Instant::now() + HOLD));
            send_level(&state, level, scale.downgrade());
        });
        let (percent, scale) = (self.percent.downgrade(), self.scale.downgrade());
        let state = Rc::clone(&self.state);
        self.mute.connect_toggled(move |mute| {
            let (Some(percent), Some(scale)) = (percent.upgrade(), scale.upgrade()) else {
                return;
            };
            show_level(
                &percent,
                mute,
                kind,
                &state,
                scale.value(),
                mute.is_active(),
            );
            if state.updating.get() {
                return;
            }
            let send = state.muted.borrow().clone();
            if let Some(send) = send {
                glib::spawn_future_local(send(mute.is_active()));
            }
        });
    }
}

/// Shows `level` (a fraction) in percent in `percent`, and `muted` on `mute`.
fn show_level(
    percent: &gtk::Label,
    mute: &gtk::ToggleButton,
    kind: Kind,
    state: &State,
    level: f64,
    muted: bool,
) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a level is 0 … 1.5 or a little more, a few hundred percent at most"
    )]
    let shown = (level * 100.0).round() as i64;
    percent.set_text(&trf(msg!("{percent} %"), &[("percent", &shown)]));
    mute.set_icon_name(icon(kind, level, muted));
    // The button's name stays; its pressed state says whether it is muted.
    let tooltip = if muted {
        tr(msg!("Unmute"))
    } else {
        tr(msg!("Mute"))
    };
    mute.set_tooltip_text(Some(tooltip));
    mute.update_property(&[gtk::accessible::Property::Label(&state.mute_name.borrow())]);
}

/// Sends `level` with `state`'s sender, or keeps it for after the request on its way.
fn send_level(state: &Rc<State>, level: f64, scale: glib::WeakRef<gtk::Scale>) {
    if state.busy.get() {
        state.wanted.set(Some(level));
        return;
    }
    let Some(send) = state.level.borrow().clone() else {
        return;
    };
    state.busy.set(true);
    let state = Rc::clone(state);
    glib::spawn_future_local(async move {
        send(level).await;
        while let Some(level) = state.wanted.take() {
            send(level).await;
        }
        state.busy.set(false);
        // The protection lasts a moment after the last answer, for the report that follows it.
        state.held_until.set(Some(Instant::now() + HOLD));
        // Then the slider shows what the server has, also when it refused the last level and
        // reported nothing new.
        glib::timeout_future(HOLD).await;
        let held = state.busy.get()
            || state
                .held_until
                .get()
                .is_some_and(|until| Instant::now() < until);
        if let (false, Some(scale), Some(reported)) = (held, scale.upgrade(), state.reported.get())
        {
            state.updating.set(true);
            scale.set_value(reported);
            state.updating.set(false);
        }
    });
}

/// The icon for `level` (a fraction) and `muted`, by `kind`.
fn icon(kind: Kind, level: f64, muted: bool) -> &'static str {
    match kind {
        Kind::Playing if muted || level <= 0.0 => "audio-volume-muted-symbolic",
        Kind::Playing if level < 0.34 => "audio-volume-low-symbolic",
        Kind::Playing if level < 0.67 => "audio-volume-medium-symbolic",
        Kind::Playing => "audio-volume-high-symbolic",
        Kind::Recording if muted || level <= 0.0 => "microphone-sensitivity-muted-symbolic",
        Kind::Recording if level < 0.34 => "microphone-sensitivity-low-symbolic",
        Kind::Recording if level < 0.67 => "microphone-sensitivity-medium-symbolic",
        Kind::Recording => "microphone-sensitivity-high-symbolic",
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, icon};

    #[test]
    fn the_icon_follows_the_level_and_the_mute() {
        assert_eq!(
            icon(Kind::Playing, 0.5, true),
            "audio-volume-muted-symbolic"
        );
        assert_eq!(
            icon(Kind::Playing, 0.0, false),
            "audio-volume-muted-symbolic"
        );
        assert_eq!(icon(Kind::Playing, 0.2, false), "audio-volume-low-symbolic");
        assert_eq!(
            icon(Kind::Playing, 1.4, false),
            "audio-volume-high-symbolic"
        );
        assert_eq!(
            icon(Kind::Recording, 0.5, false),
            "microphone-sensitivity-medium-symbolic"
        );
    }
}
