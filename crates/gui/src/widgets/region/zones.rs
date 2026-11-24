// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The time zone dialog: a search field and the zones timedated knows, each with its city,
//! region, current offset and local time. Activating a zone picks it and closes the dialog.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::region::zones::Zone;

use crate::i18n::{msg, tr};
use crate::style;

/// What the dialog calls with the zone picked, once.
type Picked = Rc<RefCell<Option<Box<dyn FnOnce(String)>>>>;

/// Opens the dialog over `parent` with `zones`, `current` selected; `picked` gets the zone the
/// user activates. Closing it otherwise picks nothing.
pub fn open(
    parent: Option<&gtk::Window>,
    zones: &[String],
    current: &str,
    picked: impl FnOnce(String) + 'static,
) -> gtk::Window {
    let title = tr(msg!("Time zone"));
    let (window, content) = super::dialog(parent, title, true);
    window.set_default_size(460, 520);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(tr(msg!("Search for a city or region"))));
    search.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Search time zones"
    )))]);
    content.append(&search);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("st-section");
    list.update_property(&[gtk::accessible::Property::Label(title)]);
    let mut selected = None;
    let zones: Vec<Zone> = zones.iter().map(|name| Zone::new(name)).collect();
    for zone in &zones {
        let item = zone_row(zone);
        list.append(&item);
        if zone.name() == current {
            selected = Some(item);
        }
    }
    let placeholder = gtk::Label::new(Some(tr(msg!("No time zone matches the search"))));
    placeholder.add_css_class(style::DIM);
    placeholder.set_margin_top(12);
    placeholder.set_margin_bottom(12);
    list.set_placeholder(Some(&placeholder));
    let zones = Rc::new(zones);
    let query = Rc::new(RefCell::new(String::new()));
    {
        let zones = Rc::clone(&zones);
        let query = Rc::clone(&query);
        list.set_filter_func(move |item| {
            let index = usize::try_from(item.index()).unwrap_or(usize::MAX);
            zones
                .get(index)
                .is_some_and(|zone| zone.matches(&query.borrow()))
        });
    }
    search.connect_search_changed(glib::clone!(
        #[weak]
        list,
        move |entry| {
            entry.text().as_str().clone_into(&mut query.borrow_mut());
            list.invalidate_filter();
        }
    ));

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    content.append(&scroller);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&super::cancel_button(&window));
    content.append(&buttons);

    let picked: Picked = Rc::new(RefCell::new(Some(Box::new(picked))));
    list.connect_row_activated(glib::clone!(
        #[weak]
        window,
        #[strong]
        picked,
        move |_, item| {
            let index = usize::try_from(item.index()).unwrap_or(usize::MAX);
            if let (Some(zone), Some(done)) = (zones.get(index), picked.borrow_mut().take()) {
                done(zone.name().to_owned());
            }
            window.close();
        }
    ));
    // Enter in the search field picks the first zone that matches.
    search.connect_activate(glib::clone!(
        #[weak]
        list,
        move |_| {
            let mut index = 0;
            while let Some(item) = list.row_at_index(index) {
                if item.is_child_visible() {
                    item.activate();
                    return;
                }
                index += 1;
            }
        }
    ));
    window.present();
    if let Some(item) = selected {
        list.select_row(Some(&item));
        let scroll = item.clone();
        glib::idle_add_local_once(move || {
            scroll.grab_focus();
        });
    }
    search.grab_focus();
    window
}

/// The row of `zone`: its city above its region, the offset and the local time on the right.
fn zone_row(zone: &Zone) -> gtk::ListBoxRow {
    let city = gtk::Label::new(Some(&zone.city()));
    city.set_xalign(0.0);
    city.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let region = gtk::Label::new(Some(&zone.region()));
    region.set_xalign(0.0);
    region.set_ellipsize(gtk::pango::EllipsizeMode::End);
    region.add_css_class(style::DIM);
    region.set_visible(!zone.region().is_empty());
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(&city);
    text.append(&region);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.set_margin_top(6);
    line.set_margin_bottom(6);
    line.set_margin_start(8);
    line.set_margin_end(8);
    line.append(&text);
    if let Some(now) = zone.now() {
        let offset = gtk::Label::new(Some(&now.offset()));
        offset.add_css_class(style::DIM);
        offset.add_css_class("numeric");
        let time = gtk::Label::new(Some(&now.time()));
        time.add_css_class("numeric");
        line.append(&offset);
        line.append(&time);
    }
    let item = gtk::ListBoxRow::new();
    item.set_child(Some(&line));
    item.update_property(&[gtk::accessible::Property::Label(&zone.place())]);
    item
}
