// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Date & Time section: the time zone with _Change…_, automatic time, and the clock with
//! _Set…_ while automatic time is off.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::region::zones::{LocalTime, Zone};
use sway_things_core::region::{Bus, Change, Clock, Error, Timedate, geo};

use super::{Help, subtitle};
use crate::i18n::{msg, tr, trf};
use crate::widgets::Placeholder;
use crate::widgets::list::{heading, list_row, row, section};
use crate::widgets::live::follow_changes;

/// How long to collect a burst of changes before reading timedated again.
const THROTTLE: Duration = Duration::from_millis(100);
/// How often the section looks for timedated while it isn't there, and reads whether the clock
/// is synchronized (timedated doesn't report that as a change).
const RETRY: Duration = Duration::from_secs(5);

/// The widgets and state of the section. Cheap to clone; every clone shares them.
#[derive(Clone)]
pub struct TimeSection {
    inner: Rc<Inner>,
}

struct Inner {
    /// The list of rows, hidden while timedated isn't there.
    list: gtk::ListBox,
    /// Shown instead of the list while timedated isn't there.
    missing: Placeholder,
    zone_subtitle: gtk::Label,
    zone_button: gtk::Button,
    ntp: gtk::Switch,
    ntp_subtitle: gtk::Label,
    time_subtitle: gtk::Label,
    time_button: gtk::Button,
    help: Help,
    timedate: RefCell<Option<Timedate>>,
    /// The bus of timedated, where geoclue is too.
    bus: RefCell<Bus>,
    clock: RefCell<Option<Clock>>,
    /// Set while a change waits for timedated (and polkit), so the controls wait too.
    busy: Cell<bool>,
    /// What automatic time was switched to while that change waits: the switch shows it.
    wanted_ntp: Cell<Option<bool>>,
    /// Set while the switch is moved to timedated's state, so that isn't taken for a change.
    updating: Cell<bool>,
    /// Updates the clock's row at every full second while the page is on screen.
    tick: RefCell<Option<glib::SourceId>>,
    /// The automatic time zone's lookup while it runs.
    detecting: RefCell<Option<glib::JoinHandle<()>>>,
    /// Why the automatic time zone last set nothing, as the help shows it.
    detect_help: RefCell<Option<String>>,
    tasks: RefCell<Vec<glib::JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        for task in self.tasks.take() {
            task.abort();
        }
        if let Some(task) = self.detecting.take() {
            task.abort();
        }
        if let Some(tick) = self.tick.take() {
            tick.remove();
        }
    }
}

impl TimeSection {
    /// The section, appended to `content`; `first` is the page's first section.
    pub fn new(content: &gtk::Box, first: bool) -> Self {
        let title = tr(msg!("Date & Time"));
        content.append(&heading(title, first));
        let list = gtk::ListBox::new();
        section(&list, title);
        let missing = Placeholder::new(
            "preferences-system-time-symbolic",
            tr(msg!("The system clock isn't available")),
            tr(msg!(
                "Changing the time zone and the clock needs systemd-timedated, which this system \
                 doesn't have."
            )),
        );
        missing.set_visible(false);
        let inner = Inner {
            list,
            missing,
            zone_subtitle: subtitle(),
            zone_button: gtk::Button::with_label(tr(msg!("Change…"))),
            ntp: gtk::Switch::new(),
            ntp_subtitle: subtitle(),
            time_subtitle: subtitle(),
            time_button: gtk::Button::with_label(tr(msg!("Set…"))),
            help: Help::new(),
            timedate: RefCell::default(),
            bus: RefCell::default(),
            clock: RefCell::default(),
            busy: Cell::new(false),
            wanted_ntp: Cell::new(None),
            updating: Cell::new(false),
            tick: RefCell::default(),
            detecting: RefCell::default(),
            detect_help: RefCell::default(),
            tasks: RefCell::default(),
        };
        let section = Self {
            inner: Rc::new(inner),
        };
        section.build(content);
        section
    }

    fn build(&self, content: &gtk::Box) {
        let inner = &self.inner;
        let zone_title = tr(msg!("Time zone"));
        inner.zone_button.set_valign(gtk::Align::Center);
        inner
            .zone_button
            .update_property(&[gtk::accessible::Property::Label(tr(msg!(
                "Change the time zone"
            )))]);
        inner.list.append(&list_row(&row(
            &gtk::Label::new(Some(zone_title)),
            &inner.zone_subtitle,
            &[inner.zone_button.upcast_ref()],
        )));
        let weak = Rc::downgrade(&self.inner);
        inner.zone_button.connect_clicked(move |button| {
            if let Some(inner) = weak.upgrade() {
                Self { inner }.pick_zone(button);
            }
        });

        let ntp_title = tr(msg!("Set time automatically"));
        inner.ntp.set_valign(gtk::Align::Center);
        inner
            .ntp
            .update_property(&[gtk::accessible::Property::Label(ntp_title)]);
        inner.list.append(&list_row(&row(
            &gtk::Label::new(Some(ntp_title)),
            &inner.ntp_subtitle,
            &[inner.ntp.upcast_ref()],
        )));
        let weak = Rc::downgrade(&self.inner);
        inner.ntp.connect_state_set(move |_, on| {
            if let Some(inner) = weak.upgrade() {
                if !inner.updating.get() {
                    Self { inner }.set_ntp(on);
                }
            }
            glib::Propagation::Stop
        });

        let time_title = tr(msg!("Date & time"));
        inner.time_button.set_valign(gtk::Align::Center);
        inner
            .time_button
            .update_property(&[gtk::accessible::Property::Label(tr(msg!(
                "Set the date and time"
            )))]);
        inner.list.append(&list_row(&row(
            &gtk::Label::new(Some(time_title)),
            &inner.time_subtitle,
            &[inner.time_button.upcast_ref()],
        )));
        let weak = Rc::downgrade(&self.inner);
        inner.time_button.connect_clicked(move |button| {
            if let Some(inner) = weak.upgrade() {
                Self { inner }.pick_time(button);
            }
        });

        content.append(&inner.missing);
        content.append(&inner.list);
        content.append(&inner.help.root);
        self.render();
    }

    /// Connects to timedated on `bus` and follows it until the section goes.
    pub fn follow(&self, bus: Bus) {
        self.inner.bus.replace(bus.clone());
        let weak = Rc::downgrade(&self.inner);
        let task = glib::spawn_future_local(async move {
            let timedate = match Timedate::connect(&bus).await {
                Ok(timedate) => timedate,
                Err(error) => {
                    if let Some(inner) = weak.upgrade() {
                        Self { inner }.show(Err(error));
                    }
                    return;
                }
            };
            if let Some(inner) = weak.upgrade() {
                inner.timedate.replace(Some(timedate.clone()));
            }
            let changes = timedate.changes().await;
            if !refresh(&weak).await {
                return;
            }
            let reads = weak.clone();
            let again = glib::spawn_future_local(async move {
                loop {
                    glib::timeout_future(RETRY).await;
                    if !refresh(&reads).await {
                        return;
                    }
                }
            });
            if let Some(inner) = weak.upgrade() {
                inner.tasks.borrow_mut().push(again);
            }
            if let Ok(changes) = changes {
                follow_changes(changes, THROTTLE, || refresh(&weak)).await;
            }
        });
        self.inner.tasks.borrow_mut().push(task);
    }

    /// Puts `row` (the automatic time zone) below the time zone's row.
    pub fn insert_row(&self, row: &crate::widgets::SettingRow) {
        self.inner
            .list
            .insert(&crate::widgets::list::list_row(row), 1);
    }

    /// Finds the zone where the computer is and sets it if the system has another (polkit may
    /// ask for the password), as the automatic time zone does while the page is open. Why it set
    /// none shows in the section's help, with what to run. A lookup still running is stopped.
    pub fn detect(&self) {
        self.cancel_detect();
        let bus = self.inner.bus.borrow().clone();
        let weak = Rc::downgrade(&self.inner);
        let task = glib::spawn_future_local(async move {
            let timedate = match weak
                .upgrade()
                .and_then(|inner| inner.timedate.borrow().clone())
            {
                Some(timedate) => timedate,
                None => match Timedate::connect(&bus).await {
                    Ok(timedate) => timedate,
                    Err(_) => return,
                },
            };
            let found = geo::detect(&bus, &timedate).await;
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.detecting.replace(None);
            let why = found.as_ref().err().map(|error| error.text().translated());
            let command = found.as_ref().err().and_then(geo::DetectError::command);
            inner.help.show(why.as_deref(), command.as_deref());
            inner.detect_help.replace(why);
            refresh(&Rc::downgrade(&inner)).await;
        });
        self.inner.detecting.replace(Some(task));
    }

    /// Stops the automatic time zone's lookup, silently. Whether one was running.
    pub fn cancel_detect(&self) -> bool {
        let task = self.inner.detecting.take();
        if let Some(task) = &task {
            task.abort();
        }
        task.is_some()
    }

    /// Hides why the automatic time zone last set nothing, if the help still shows it.
    pub fn forget_detect(&self) {
        let inner = &self.inner;
        if let Some(why) = inner.detect_help.take() {
            if inner.help.text() == why {
                inner.help.show(None, None);
            }
        }
    }

    /// Updates the clock's row at every full second, while the page is on screen.
    pub fn start_ticking(&self) {
        if self.inner.tick.borrow().is_some() {
            return;
        }
        self.inner
            .tick
            .replace(Some(next_tick(Rc::downgrade(&self.inner))));
        self.render_time();
    }

    /// Stops updating the clock's row.
    pub fn stop_ticking(&self) {
        if let Some(source) = self.inner.tick.take() {
            source.remove();
        }
    }

    /// Shows what timedated reports, or that it isn't there.
    fn show(&self, clock: Result<Clock, Error>) {
        let inner = &self.inner;
        match clock {
            Ok(clock) => {
                inner.clock.replace(Some(clock));
                inner.missing.set_visible(false);
                inner.list.set_visible(true);
            }
            Err(Error::Unavailable(_)) => {
                inner.clock.replace(None);
                inner.missing.set_visible(true);
                inner.list.set_visible(false);
                inner.help.show(None, None);
            }
            Err(error) => {
                inner.help.show(Some(&error.text().translated()), None);
            }
        }
        self.render();
    }

    /// Puts the state into the rows.
    fn render(&self) {
        let inner = &self.inner;
        let clock = inner.clock.borrow().clone().unwrap_or_default();
        let busy = inner.busy.get();
        let zone = Zone::new(&clock.timezone);
        let zone_text = if clock.timezone.is_empty() {
            tr(msg!("Not set")).to_owned()
        } else {
            match zone.now() {
                Some(now) => trf(
                    // TRANSLATORS: The time zone row: the city and region, the abbreviation and
                    // the offset, such as "Berlin, Europe · CET (UTC+01:00)".
                    msg!("{place} · {abbreviation} ({offset})"),
                    &[
                        ("place", &zone.place()),
                        ("abbreviation", &now.abbreviation),
                        ("offset", &now.offset()),
                    ],
                ),
                None => zone.place(),
            }
        };
        inner.zone_subtitle.set_text(&zone_text);
        inner.zone_button.set_sensitive(!busy);

        inner.updating.set(true);
        let ntp = inner.wanted_ntp.get().unwrap_or(clock.ntp);
        inner.ntp.set_active(ntp);
        inner.ntp.set_state(ntp);
        inner.updating.set(false);
        inner.ntp.set_sensitive(!busy && clock.can_ntp);
        let ntp_text = if !clock.can_ntp {
            tr(msg!(
                "No time service is installed, such as systemd-timesyncd or chrony"
            ))
        } else if !clock.ntp {
            tr(msg!("Off: the clock keeps the time it is set to"))
        } else if clock.synchronized {
            tr(msg!("Synchronized with a time server"))
        } else {
            tr(msg!("Not synchronized yet"))
        };
        inner.ntp_subtitle.set_text(ntp_text);
        inner.time_button.set_sensitive(!busy && !clock.ntp);
        inner.time_button.set_tooltip_text(
            clock
                .ntp
                .then(|| tr(msg!("Turn off Set time automatically first"))),
        );
        self.render_time();
    }

    /// Shows the date and time in the system's time zone, in the user's formats.
    fn render_time(&self) {
        let inner = &self.inner;
        let zone = inner
            .clock
            .borrow()
            .as_ref()
            .map(|clock| clock.timezone.clone())
            .unwrap_or_default();
        let tz = glib::TimeZone::from_identifier(Some(zone.as_str()))
            .unwrap_or_else(glib::TimeZone::utc);
        let text = glib::DateTime::now(&tz)
            .and_then(|now| now.format("%x %X"))
            .map(|text| text.to_string())
            .unwrap_or_default();
        if inner.time_subtitle.text() != text {
            inner.time_subtitle.set_text(&text);
        }
    }

    /// Opens the time zone dialog; the zone picked is set.
    fn pick_zone(&self, button: &gtk::Button) {
        let Some(timedate) = self.inner.timedate.borrow().clone() else {
            return;
        };
        let current = self
            .inner
            .clock
            .borrow()
            .as_ref()
            .map(|clock| clock.timezone.clone())
            .unwrap_or_default();
        let parent = button.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(&self.inner);
        let task = glib::spawn_future_local(async move {
            let zones = match timedate.zones().await {
                Ok(zones) => zones,
                Err(error) => {
                    if let Some(inner) = weak.upgrade() {
                        inner.help.show(Some(&error.text().translated()), None);
                    }
                    return;
                }
            };
            super::zones::open(parent.as_ref(), &zones, &current, move |zone| {
                if let Some(inner) = weak.upgrade() {
                    Self { inner }.change(Change::TimeZone(zone));
                }
            });
        });
        self.inner.tasks.borrow_mut().push(task);
    }

    /// Opens the dialog for the date and time; what is picked is set.
    fn pick_time(&self, button: &gtk::Button) {
        let zone = self
            .inner
            .clock
            .borrow()
            .as_ref()
            .map(|clock| clock.timezone.clone())
            .unwrap_or_default();
        let parent = button.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(&self.inner);
        let shown = zone.clone();
        super::clock::open(parent.as_ref(), &shown, move |picked| {
            if let Some(inner) = weak.upgrade() {
                Self { inner }.set_time(&zone, picked);
            }
        });
    }

    fn set_ntp(&self, on: bool) {
        self.inner.wanted_ntp.set(Some(on));
        self.change(Change::Ntp(on));
    }

    fn set_time(&self, zone: &str, picked: LocalTime) {
        let Some(micros) = picked.micros_in(zone) else {
            self.inner
                .help
                .show(Some(tr(msg!("This date and time doesn't exist."))), None);
            return;
        };
        self.run(Change::Time(picked.text()), Some(micros));
    }

    fn change(&self, change: Change) {
        self.run(change, None);
    }

    /// Asks timedated for `change` (`micros` for a new time) and shows what came of it.
    fn run(&self, change: Change, micros: Option<i64>) {
        let Some(timedate) = self.inner.timedate.borrow().clone() else {
            return;
        };
        self.inner.busy.set(true);
        self.inner.help.show(None, None);
        self.render();
        let weak = Rc::downgrade(&self.inner);
        let task = glib::spawn_future_local(async move {
            let result = match (&change, micros) {
                (Change::TimeZone(zone), _) => timedate.set_timezone(zone).await,
                (Change::Ntp(on), _) => timedate.set_ntp(*on).await,
                (Change::Time(_), Some(micros)) => timedate.set_time(micros).await,
                (Change::Time(_), None) => Ok(()),
            };
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.busy.set(false);
            inner.wanted_ntp.set(None);
            if let Err(error) = &result {
                let command = matches!(error, Error::Denied(_)).then(|| change.command());
                inner
                    .help
                    .show(Some(&error.text().translated()), command.as_deref());
            }
            let section = Self { inner };
            section.render();
            refresh(&Rc::downgrade(&section.inner)).await;
        });
        self.inner.tasks.borrow_mut().push(task);
    }

    /// The text of the time zone row, for tests.
    pub fn zone_text(&self) -> String {
        self.inner.zone_subtitle.text().to_string()
    }

    /// The switch of automatic time, for tests.
    pub fn ntp_switch(&self) -> &gtk::Switch {
        &self.inner.ntp
    }

    /// The text under automatic time, for tests.
    pub fn ntp_text(&self) -> String {
        self.inner.ntp_subtitle.text().to_string()
    }

    /// The button that sets the clock, for tests.
    pub fn time_button(&self) -> &gtk::Button {
        &self.inner.time_button
    }

    /// The button that opens the time zone dialog, for tests.
    pub fn zone_button(&self) -> &gtk::Button {
        &self.inner.zone_button
    }

    /// Whether the section shows that timedated isn't there, for tests.
    pub fn missing_shown(&self) -> bool {
        self.inner.missing.is_visible()
    }

    /// The help shown after a failed change, for tests.
    pub fn help(&self) -> &Help {
        &self.inner.help
    }
}

/// Reads timedated again and shows it. False once the section is gone.
async fn refresh(weak: &std::rc::Weak<Inner>) -> bool {
    let Some(timedate) = weak
        .upgrade()
        .and_then(|inner| inner.timedate.borrow().clone())
    else {
        return weak.upgrade().is_some();
    };
    let clock = timedate.clock().await;
    let Some(inner) = weak.upgrade() else {
        return false;
    };
    TimeSection { inner }.show(clock);
    true
}

/// Updates the clock's row right after the next full second of the system clock, then arms the
/// next update the same way: a fixed one-second interval would drift from the second boundary
/// and show each second late.
fn next_tick(weak: std::rc::Weak<Inner>) -> glib::SourceId {
    const SECOND: i64 = 1_000_000;
    let into_second = glib::real_time().rem_euclid(SECOND);
    let wait = u64::try_from(SECOND - into_second).unwrap_or(0);
    glib::timeout_add_local_once(
        Duration::from_micros(wait) + Duration::from_millis(2),
        move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let section = TimeSection { inner };
            section.render_time();
            // The source that fired is gone; only its id is replaced.
            let next = next_tick(Rc::downgrade(&section.inner));
            section.inner.tick.replace(Some(next));
        },
    )
}
