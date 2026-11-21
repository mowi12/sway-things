// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sections of About & Maintenance built by hand: the system details with their copy button
//! and the project's links, and the generated files.
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::network::NetworkManager;
use sway_things_core::registry::{Msg, Page};
use sway_things_core::sound::Sound;
use sway_things_core::system::{self, Details, Value};

use super::Section;
use crate::i18n::{msg, tr};
use crate::icons;
use crate::model::Model;
use crate::style;
use crate::widgets::list::list_row;
use crate::widgets::{NetworkBus, SoundServer};

/// The project's links below the "About" section: what they are called, their icon and where
/// they lead.
const PROJECT_LINKS: [(Msg, &str, &str); 3] = [
    (msg!("Website"), icons::WEBSITE, env!("CARGO_PKG_HOMEPAGE")),
    (
        msg!("Source Code (GitHub)"),
        icons::SOURCE_CODE,
        env!("CARGO_PKG_REPOSITORY"),
    ),
    (
        msg!("Report a Bug"),
        icons::BUG,
        concat!(env!("CARGO_PKG_REPOSITORY"), "/issues"),
    ),
];

/// The label of the button that copies the system details.
const COPY_DETAILS: Msg = msg!("Copy System Details");
/// Its label right after a click.
const COPIED: Msg = msg!("Copied");
/// How long it says so.
const COPIED_FOR: Duration = Duration::from_secs(2);

/// The "About" section of About & Maintenance: a row per line of the system details (Sway
/// Things, the compositor, GTK, the distribution, NetworkManager, the sound server, the kernel
/// and the tools the pages delegate to), a button that copies them for a bug report, then the project's links,
/// which open in the default browser. The details are read again each time the section shows.
pub(super) fn about_section(model: &Model, body: &gtk::Box) -> Section {
    let title = msg!("About");
    let section = Section::new(model, Page::About, title.msgid(), tr(title), true);
    body.append(&section.header);
    body.append(&section.body);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("st-section");
    list.update_property(&[gtk::accessible::Property::Label(tr(title))]);
    let details = Rc::new(RefCell::new(None::<Details>));
    // The labels; the values follow once the details are read.
    let labels = Details {
        compositor: None,
        gtk: None,
        distribution: None,
        network_manager: None,
        sound_server: None,
        kernel: None,
        tools: system::TOOLS.iter().map(|tool| (*tool, false)).collect(),
    }
    .lines();
    let mut values = Vec::new();
    for (label, _) in labels {
        let (row, value) = info_row(&label.translated(), "");
        values.push(value);
        list.append(&list_row(&row));
    }
    section.body.append(&list);
    let copy = copy_button(&details);
    section.body.append(&copy);
    let compositor = model.compositor();
    list.connect_map(glib::clone!(
        #[strong]
        details,
        #[weak]
        copy,
        move |list| {
            let window = list.root().and_downcast::<crate::window::Window>();
            let bus = window
                .as_ref()
                .map_or(NetworkBus::Off, crate::window::Window::network_bus);
            let server = window
                .as_ref()
                .map_or(SoundServer::Off, crate::window::Window::sound_server);
            let values = values.clone();
            let details = details.clone();
            glib::spawn_future_local(glib::clone!(
                #[weak]
                copy,
                async move {
                    let nm = match &bus {
                        NetworkBus::System => NetworkManager::connect().await.ok(),
                        NetworkBus::Address(address) => {
                            NetworkManager::connect_to(address).await.ok()
                        }
                        NetworkBus::Off => None,
                    };
                    let sound = match &server {
                        SoundServer::Session => Sound::connect().ok(),
                        SoundServer::Address(address) => Sound::connect_to(Some(address)).ok(),
                        SoundServer::Off => None,
                    };
                    let read = Details::read(
                        compositor,
                        Some(crate::app::gtk_version()),
                        nm.as_ref(),
                        sound.as_ref(),
                    )
                    .await;
                    for (label, (_, value)) in values.iter().zip(read.lines()) {
                        label.set_label(&shown(&value));
                    }
                    details.replace(Some(read));
                    copy.set_sensitive(true);
                }
            ));
        }
    ));
    let links = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    links.set_halign(gtk::Align::Center);
    links.add_css_class("st-about-links");
    for (label, icon, uri) in PROJECT_LINKS {
        links.append(&link_button(tr(label), icon, uri));
    }
    section.body.append(&links);
    section
}

/// The button that copies `details` once they are read, and says so for a moment.
pub(super) fn copy_button(details: &Rc<RefCell<Option<Details>>>) -> gtk::Button {
    let copy = gtk::Button::with_label(tr(COPY_DETAILS));
    copy.set_halign(gtk::Align::Center);
    copy.add_css_class("st-about-copy");
    copy.set_sensitive(false);
    // Each click says "Copied" for a while; only the latest click's timeout restores the label.
    let clicks = Rc::new(Cell::new(0_u32));
    copy.connect_clicked(glib::clone!(
        #[strong]
        details,
        move |button| {
            let Some(text) = details.borrow().as_ref().map(Details::text) else {
                return;
            };
            button.clipboard().set_text(&text);
            button.set_label(tr(COPIED));
            let click = clicks.get().wrapping_add(1);
            clicks.set(click);
            glib::timeout_add_local_once(
                COPIED_FOR,
                glib::clone!(
                    #[weak]
                    button,
                    #[strong]
                    clicks,
                    move || {
                        if clicks.get() == click {
                            button.set_label(tr(COPY_DETAILS));
                        }
                    }
                ),
            );
        }
    ));
    copy
}

/// A value of the system details as a row shows it, capitalized like a label.
pub(super) fn shown(value: &Value) -> String {
    match value {
        Value::Text(text) => text.clone(),
        Value::Translated(text) => text.translated(),
        // TRANSLATORS: A value in the About section: a version that couldn't be found.
        Value::Unknown => tr(msg!("Unknown")).to_owned(),
        // TRANSLATORS: A value in the About section: a service that isn't running.
        Value::NotRunning => tr(msg!("Not running")).to_owned(),
        // TRANSLATORS: A value in the About section: a tool that is installed.
        Value::Installed => tr(msg!("Installed")).to_owned(),
        // TRANSLATORS: A value in the About section: a tool that isn't installed.
        Value::NotInstalled => tr(msg!("Not installed")).to_owned(),
    }
}

/// A row with `title` on the left and `value` on the right, selectable for bug reports, and the
/// label of the value.
pub(super) fn info_row(title: &str, value: &str) -> (gtk::Box, gtk::Label) {
    let title = gtk::Label::new(Some(title));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let value = gtk::Label::new(Some(value));
    value.set_xalign(1.0);
    value.set_selectable(true);
    value.set_wrap(true);
    value.add_css_class(style::DIM);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.append(&title);
    row.append(&value);
    (row, value)
}

/// A flat button with `icon` and `label` that opens `uri` in the default browser.
pub(super) fn link_button(label: &str, icon: &str, uri: &'static str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&gtk::Label::new(Some(label)));
    let button = gtk::Button::builder()
        .child(&content)
        .css_classes(["flat"])
        .tooltip_text(uri)
        .build();
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button.connect_clicked(move |button| {
        let window = button.root().and_downcast::<gtk::Window>();
        gtk::UriLauncher::new(uri).launch(
            window.as_ref(),
            gtk::gio::Cancellable::NONE,
            move |result| {
                if let Err(error) = result {
                    glib::g_warning!("sway-things", "{uri} can't be opened: {error}");
                }
            },
        );
    });
    button
}

/// The "Generated config" section of About & Maintenance: a row that opens the window with the
/// files Sway Things writes.
pub(super) fn generated_section(model: &Model, body: &gtk::Box) -> Section {
    let title = msg!("Generated config");
    let section = Section::new(model, Page::About, title.msgid(), tr(title), false);
    body.append(&section.header);
    body.append(&section.body);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("st-section");
    list.update_property(&[gtk::accessible::Property::Label(tr(title))]);
    let title = gtk::Label::new(Some(tr(title)));
    title.set_xalign(0.0);
    let subtitle = gtk::Label::new(Some(tr(msg!(
        "What Sway Things writes for your sway config to include, and which files were changed \
         outside it"
    ))));
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.add_css_class(style::DIM);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(&title);
    text.append(&subtitle);
    let show = gtk::Button::with_label(tr(msg!("Show…")));
    show.set_valign(gtk::Align::Center);
    show.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Show generated config"
    )))]);
    show.connect_clicked(|button| {
        if let Some(window) = button.root().and_downcast::<crate::window::Window>() {
            window.show_generated(None);
        }
    });
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.append(&text);
    row.append(&show);
    list.append(&list_row(&row));
    section.body.append(&list);
    section
}
