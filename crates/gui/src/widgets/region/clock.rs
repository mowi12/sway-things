// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialog that sets the clock by hand: a calendar, the hour and the minute, in the system's
//! time zone.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::region::zones::{self, LocalTime};

use crate::i18n::{msg, tr, trf};
use crate::style;

/// What the dialog calls with the date and time picked, once.
type Picked = Rc<RefCell<Option<Box<dyn FnOnce(LocalTime)>>>>;

/// Opens the dialog over `parent`, starting at the time it is now in `zone`; `picked` gets what
/// the user sets. Closing it otherwise sets nothing.
pub fn open(
    parent: Option<&gtk::Window>,
    zone: &str,
    picked: impl FnOnce(LocalTime) + 'static,
) -> gtk::Window {
    let (window, content) = super::dialog(parent, tr(msg!("Date & time")), false);

    let now = zones::local_now(zone);
    let calendar = gtk::Calendar::new();
    if let Ok(date) = glib::DateTime::from_utc(
        i32::from(now.year),
        i32::from(now.month),
        i32::from(now.day),
        12,
        0,
        0.0,
    ) {
        calendar.select_day(&date);
    }
    calendar.update_property(&[gtk::accessible::Property::Label(tr(msg!("Date")))]);
    content.append(&calendar);

    let hour = number(23, now.hour, tr(msg!("Hour")));
    let minute = number(59, now.minute, tr(msg!("Minute")));
    let time = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    time.set_halign(gtk::Align::Center);
    let label = gtk::Label::new(Some(tr(msg!("Time"))));
    label.add_css_class(style::DIM);
    label.set_margin_end(6);
    time.append(&label);
    time.append(&hour);
    time.append(&gtk::Label::new(Some(":")));
    time.append(&minute);
    content.append(&time);

    let note = gtk::Label::new(Some(&trf(
        msg!("In the time zone {zone}"),
        &[("zone", &zone)],
    )));
    note.add_css_class(style::DIM);
    note.set_wrap(true);
    content.append(&note);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_homogeneous(true);
    buttons.set_margin_top(8);
    let cancel = super::cancel_button(&window);
    let set = gtk::Button::with_label(tr(msg!("Set")));
    set.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&set);
    content.append(&buttons);
    window.set_default_widget(Some(&set));

    let picked: Picked = Rc::new(RefCell::new(Some(Box::new(picked))));
    set.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        calendar,
        #[weak]
        hour,
        #[weak]
        minute,
        move |_| {
            let date = calendar.date();
            let time = LocalTime {
                year: i16::try_from(date.year()).unwrap_or(1970),
                month: i8::try_from(date.month()).unwrap_or(1),
                day: i8::try_from(date.day_of_month()).unwrap_or(1),
                hour: i8::try_from(hour.value_as_int()).unwrap_or(0),
                minute: i8::try_from(minute.value_as_int()).unwrap_or(0),
            };
            if let Some(done) = picked.borrow_mut().take() {
                done(time);
            }
            window.close();
        }
    ));
    window.present();
    calendar.grab_focus();
    window
}

/// A spin button for 0 to `max`, at `value`, two digits wide, named `name`.
fn number(max: i8, value: i8, name: &str) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(0.0, f64::from(max), 1.0);
    spin.set_value(f64::from(value));
    spin.set_wrap(true);
    spin.set_width_chars(2);
    spin.set_orientation(gtk::Orientation::Vertical);
    spin.update_property(&[gtk::accessible::Property::Label(name)]);
    spin.connect_output(|spin| {
        spin.set_text(&format!("{:02}", spin.value_as_int()));
        glib::Propagation::Stop
    });
    spin
}
