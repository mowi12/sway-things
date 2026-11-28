// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Showing Sway Things' bar in place of the user's own, without editing their config, and back.
//!
//! Where the user's sway config starts their bar in a `bar { … }` block before our include, the
//! card offers _Use Sway Things' Bar_: it stores the IDs of their bars (`bar.take_over`) with
//! Waybar as the bar program, and reloads sway, which then runs Sway Things' waybar as their
//! first bar and nothing for the others ([`sway_things_core::bar::takeover`]). A banner says
//! that it starts, then, once Sway Things' waybar runs, asks to keep it for 15 seconds and
//! switches back by itself without _Keep_. It switches back at once if Sway Things' waybar ends,
//! or doesn't run 6 seconds after the reload, and says why, with what waybar wrote
//! ([`takeover::log_errors`]). _Switch Back to My Own Bar_ stays while it is shown: it removes
//! our lines and reloads, so their bar is back exactly as before.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant, SystemTime};

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bar::{WaybarFiles, process, takeover};
use sway_things_core::registry::{Value, bar as keys};

use super::{Ctx, follow};
use crate::i18n::{msg, tr, trf};

/// How long the banner waits to be kept.
const KEEP_FOR: Duration = Duration::from_secs(15);
/// How long after the reload Sway Things' waybar must run.
const STARTS_WITHIN: Duration = Duration::from_secs(6);
/// How long a reload may take to start: after that, an idle model counts as reloaded.
const RELOAD_STARTS: Duration = Duration::from_secs(2);

/// A switch to Sway Things' bar that waits to be kept.
#[derive(Debug, Clone)]
struct Trial {
    /// When it started, by the clock of the main loop and of the files.
    started: Instant,
    started_at: SystemTime,
    /// Whether the model was seen busy with the reload.
    busy_seen: bool,
    /// When sway was done reloading, once it was.
    reloaded: Option<Instant>,
    /// Since when Sway Things' waybar runs, once it does.
    running: Option<Instant>,
    /// The values before: the bar program and the bars taken over.
    backend: Value,
    taken: Vec<String>,
}

impl Trial {
    /// A trial that starts now, with the values before.
    fn new(backend: Value, taken: Vec<String>) -> Self {
        Self {
            started: Instant::now(),
            started_at: SystemTime::now(),
            busy_seen: false,
            reloaded: None,
            running: None,
            backend,
            taken,
        }
    }

    /// The whole seconds left to keep it, at least one, once Sway Things' waybar runs.
    fn left(&self) -> Option<u64> {
        self.running
            .map(|since| KEEP_FOR.saturating_sub(since.elapsed()).as_secs().max(1))
    }
}

/// The part of the card about the user's own bar.
struct Part {
    ctx: Weak<Ctx>,
    root: gtk::Box,
    /// The switch that waits to be kept.
    trial: RefCell<Option<Trial>>,
    /// What happened last, said until the next switch, and what waybar wrote about it.
    said: RefCell<Option<String>>,
    log: RefCell<Vec<String>>,
    /// What the part shows now, so it is rebuilt only where that changes.
    shown: RefCell<String>,
}

/// The part of the card that offers Sway Things' bar in place of the user's, or switches back.
pub fn section(ctx: &Rc<Ctx>) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.add_css_class("st-bar-takeover");
    let part = Rc::new(Part {
        ctx: Rc::downgrade(ctx),
        root: root.clone(),
        trial: RefCell::new(None),
        said: RefCell::new(None),
        log: RefCell::new(Vec::new()),
        shown: RefCell::new(String::new()),
    });
    let follower = Rc::downgrade(&part);
    follow(ctx.model(), &root, move || {
        if let Some(part) = follower.upgrade() {
            part.refresh();
        }
    });
    // The start and the countdown, twice a second while a switch waits to be kept.
    let ticker = Rc::downgrade(&part);
    glib::timeout_add_local(Duration::from_millis(500), move || {
        let Some(part) = ticker.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if part.trial.borrow().is_some() {
            part.refresh();
        }
        glib::ControlFlow::Continue
    });
    // The part lives as long as its widget.
    let kept = RefCell::new(Some(part));
    root.connect_destroy(move |_| {
        kept.take();
    });
    root
}

/// The bars of the user's sway config that can be taken over, and whether the first runs waybar.
fn takeable(ctx: &Ctx) -> (Vec<String>, bool) {
    let (Some(status), Some(locations)) = (ctx.model().status(), ctx.model().locations()) else {
        return (Vec::new(), false);
    };
    let bars = takeover::bars(status.config(), locations.directory());
    let theirs: Vec<&takeover::Bar> = bars.iter().filter(|bar| bar.can_take()).collect();
    let waybar = theirs
        .first()
        .is_some_and(|bar| bar.runs_waybar() || bar.taken());
    (theirs.iter().map(|bar| bar.id.clone()).collect(), waybar)
}

/// Whether the card offers Sway Things' bar in place of a bar of the user's.
pub fn offers(ctx: &Ctx) -> bool {
    !takeable(ctx).0.is_empty()
}

/// Whether a waybar with the config `files` of Sway Things runs.
fn ours_runs(files: Option<&WaybarFiles>) -> bool {
    files.is_some_and(|files| !process::ours(files).is_empty())
}

/// Stores `backend` with the bars `taken` over and reloads sway, which starts the bar they say.
fn switch(ctx: &Ctx, backend: Value, taken: Vec<String>) {
    let model = ctx.model();
    model.set(keys::TAKE_OVER, None, Value::List(taken));
    model.set(keys::BACKEND, None, backend);
    model.reload_sway();
}

/// A label that wraps.
fn wrapped(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label
}

impl Part {
    /// Brings the part to the settings now and the switch that waits, if one does: whether
    /// Sway Things' waybar runs is read off the main context first.
    fn refresh(self: &Rc<Self>) {
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        if self.trial.borrow().is_none() {
            self.show(&ctx, None);
            return;
        }
        let files = ctx.model().waybar_files();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let runs = gtk::gio::spawn_blocking(move || ours_runs(files.as_ref()))
                .await
                .unwrap_or(false);
            let Some(part) = weak.upgrade() else {
                return;
            };
            if let Some(ctx) = part.ctx.upgrade() {
                part.show(&ctx, Some(runs));
            }
        });
    }

    /// Shows the part, after following the switch that waits with whether our waybar `runs`.
    fn show(self: &Rc<Self>, ctx: &Rc<Ctx>, runs: Option<bool>) {
        if let Some(runs) = runs {
            self.check_trial(ctx, runs);
        }
        let backend = ctx
            .value(keys::BACKEND)
            .unwrap_or(Value::Choice("none".to_owned()));
        let taken = ctx.list(keys::TAKE_OVER);
        let active = backend == Value::Choice("waybar".to_owned()) && !taken.is_empty();
        let trial = self.trial.borrow().clone();
        let key = format!(
            "{backend:?}|{taken:?}|{:?}|{:?}",
            trial.as_ref().map(Trial::left),
            self.said.borrow()
        );
        if self.shown.replace(key.clone()) == key {
            return;
        }
        while let Some(child) = self.root.first_child() {
            self.root.remove(&child);
        }
        if let Some(text) = self.said.borrow().as_ref() {
            let label = wrapped(text);
            label.add_css_class("st-attention");
            self.root.append(&label);
            let log = self.log.borrow();
            if !log.is_empty() {
                let lines = wrapped(&log.join("\n"));
                lines.add_css_class("monospace");
                lines.add_css_class(crate::style::DIM);
                lines.set_selectable(true);
                let details = gtk::Expander::new(Some(tr(msg!("What Waybar said"))));
                details.set_child(Some(&lines));
                self.root.append(&details);
            }
        }
        if let Some(trial) = trial {
            self.banner(&trial);
        } else if active {
            self.switch_back(ctx);
        } else if backend != Value::Choice("swaybar".to_owned()) {
            self.offer(ctx);
        }
    }

    /// Follows a switch that waits to be kept: notes when sway reloaded and when Sway Things'
    /// waybar runs, and switches back when the time to keep it is up, when it ends, or when it
    /// doesn't run [`STARTS_WITHIN`] after the reload (at once if the launcher says it can't run
    /// waybar): the values before come back, and the page says why.
    fn check_trial(&self, ctx: &Ctx, runs: bool) {
        let reason = {
            let mut trial = self.trial.borrow_mut();
            let Some(trial) = trial.as_mut() else {
                return;
            };
            let busy = ctx.model().is_busy();
            trial.busy_seen |= busy;
            if trial.reloaded.is_none()
                && !busy
                && (trial.busy_seen || trial.started.elapsed() >= RELOAD_STARTS)
            {
                trial.reloaded = Some(Instant::now());
            }
            if runs && trial.running.is_none() {
                trial.running = Some(Instant::now());
            }
            let errors = || takeover::log_errors(trial.started_at, 8);
            match (trial.running, trial.reloaded) {
                (Some(_), _) if !runs => {
                    self.log.replace(errors());
                    msg!("Switched back to your own bar: Sway Things' Waybar ended.")
                }
                (Some(since), _) if since.elapsed() >= KEEP_FOR => {
                    self.log.replace(Vec::new());
                    msg!("Switched back to your own bar: Sway Things' bar wasn't kept.")
                }
                (None, Some(reloaded)) => {
                    // sway ran the launcher (it wrote its log), but no waybar of ours runs a
                    // moment later: it ended at once.
                    let ended = takeover::log_written(trial.started_at).is_some_and(|written| {
                        written
                            .elapsed()
                            .is_ok_and(|elapsed| elapsed >= Duration::from_secs(1))
                    });
                    if !ended && reloaded.elapsed() < STARTS_WITHIN {
                        return;
                    }
                    let log = errors();
                    let reason = if ended {
                        msg!(
                            "Switched back to your own bar: Sway Things' Waybar ended right after \
                             it started. What Waybar said is below."
                        )
                    } else {
                        msg!(
                            "Switched back to your own bar: Sway Things' Waybar didn't start. Is \
                             waybar installed?"
                        )
                    };
                    self.log.replace(log);
                    reason
                }
                _ => return,
            }
        };
        if let Some(before) = self.trial.take() {
            switch(ctx, before.backend, before.taken);
        }
        self.said.replace(Some(tr(reason).to_owned()));
    }

    /// The banner of a switch that waits to be kept.
    fn banner(self: &Rc<Self>, trial: &Trial) {
        let banner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        banner.add_css_class("st-bar-toast");
        let text = wrapped(&match trial.left() {
            Some(seconds) => trf(
                msg!("Keep this bar? Switching back in {seconds} s"),
                &[("seconds", &seconds)],
            ),
            None => tr(msg!("Starting Sway Things' bar…")).to_owned(),
        });
        text.set_hexpand(true);
        banner.append(&text);
        let keep = gtk::Button::with_label(tr(msg!("Keep")));
        keep.add_css_class("suggested-action");
        keep.set_valign(gtk::Align::Center);
        // Kept once it shows.
        keep.set_sensitive(trial.running.is_some());
        let back = gtk::Button::with_label(tr(msg!("Switch Back")));
        back.set_valign(gtk::Align::Center);
        banner.append(&keep);
        banner.append(&back);
        let weak = Rc::downgrade(self);
        keep.connect_clicked(move |_| {
            if let Some(part) = weak.upgrade() {
                part.trial.replace(None);
                part.said.replace(None);
                part.log.replace(Vec::new());
                part.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        back.connect_clicked(move |_| {
            let Some(part) = weak.upgrade() else { return };
            if let (Some(ctx), Some(before)) = (part.ctx.upgrade(), part.trial.take()) {
                switch(&ctx, before.backend, before.taken);
            }
            part.said.replace(Some(
                tr(msg!(
                    "Your own bar is back, exactly as your sway config sets it."
                ))
                .to_owned(),
            ));
            part.refresh();
        });
        self.root.append(&banner);
    }

    /// While Sway Things' bar shows in place of the user's: _Switch Back to My Own Bar_.
    fn switch_back(self: &Rc<Self>, ctx: &Ctx) {
        let text = wrapped(tr(msg!(
            "Sway Things' bar shows in place of your own. Your sway config and your own Waybar \
             config stay as they are."
        )));
        text.add_css_class(crate::style::DIM);
        self.root.append(&text);
        let back = gtk::Button::with_label(tr(msg!("Switch Back to My Own Bar")));
        back.set_halign(gtk::Align::Start);
        back.set_sensitive(ctx.writable());
        let weak = Rc::downgrade(self);
        back.connect_clicked(move |_| {
            let Some(part) = weak.upgrade() else { return };
            if let Some(ctx) = part.ctx.upgrade() {
                switch(&ctx, Value::Choice("none".to_owned()), Vec::new());
            }
            part.said.replace(Some(
                tr(msg!(
                    "Your own bar is back, exactly as your sway config sets it."
                ))
                .to_owned(),
            ));
            part.refresh();
        });
        self.root.append(&back);
    }

    /// Where the user's sway config starts a bar of theirs that can be taken over: what that
    /// means, and _Use Sway Things' Bar_.
    fn offer(self: &Rc<Self>, ctx: &Ctx) {
        let (theirs, waybar) = takeable(ctx);
        if theirs.is_empty() {
            return;
        }
        self.root.append(&wrapped(tr(if waybar {
            msg!(
                "Your bar: your own Waybar, started by your sway config. Sway Things can show its \
                 bar instead: your config and your own Waybar config stay untouched, and you can \
                 switch back at any time."
            )
        } else {
            msg!(
                "Your bar: started by your sway config. Sway Things can show its bar instead: \
                 your config stays untouched, and you can switch back at any time."
            )
        })));
        let use_ours = gtk::Button::with_label(tr(msg!("Use Sway Things' Bar")));
        use_ours.add_css_class("suggested-action");
        use_ours.set_halign(gtk::Align::Start);
        let installed = process::installed();
        use_ours.set_sensitive(ctx.writable() && installed);
        if !installed {
            self.root
                .append(&wrapped(tr(msg!("Install the waybar package first."))));
        }
        let weak = Rc::downgrade(self);
        use_ours.connect_clicked(move |_| {
            let Some(part) = weak.upgrade() else { return };
            let Some(ctx) = part.ctx.upgrade() else {
                return;
            };
            let before = Trial::new(
                ctx.value(keys::BACKEND)
                    .unwrap_or(Value::Choice("none".to_owned())),
                ctx.list(keys::TAKE_OVER),
            );
            switch(&ctx, Value::Choice("waybar".to_owned()), theirs.clone());
            part.trial.replace(Some(before));
            part.said.replace(None);
            part.log.replace(Vec::new());
            part.refresh();
        });
        self.root.append(&use_ours);
    }
}
