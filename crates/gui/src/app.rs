// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `GtkApplication` of Sway Things.
//!
//! The application is unique on the session bus: starting `sway-things-gui` again hands its
//! command line to the running instance, which opens the link and presents its window. The link
//! is the only argument, a page id or the key of a setting (`sway-things-gui keyboard`,
//! `sway-things-gui input.keyboard.repeat_rate`). The action `app.open` opens a link too, e.g.
//! `gapplication action io.github.mowi12.SwayThings open "'keyboard'"`; an empty link only
//! presents the window.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use sway_things_core::registry::{Link, Registry};

use crate::model::Model;
use crate::style;
use crate::theme;
use crate::window::Window;

/// The application id: the D-Bus name and the sway `app_id` of the window, so users can match it in
/// `for_window` rules.
pub const APP_ID: &str = "io.github.mowi12.SwayThings";

/// The exit status for wrong arguments, as the CLI uses it.
const USAGE: u8 = 2;

/// The window while it loads: the link to open once it is there.
#[derive(Debug, Default)]
struct Loading {
    /// Whether the window is being built.
    started: Cell<bool>,
    /// The link asked for last while the window loads.
    link: RefCell<Option<Link<'static>>>,
}

/// Builds the application: loads the style sheet and follows the light/dark preference on
/// startup, and opens the window, at a link if one is given, on activation, on a command line and
/// with the action `app.open`.
pub fn build() -> gtk::Application {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let loading = Rc::new(Loading::default());
    app.connect_startup(glib::clone!(
        #[strong]
        loading,
        move |app| {
            style::load();
            theme::follow_color_scheme();
            let open = gio::ActionEntry::builder("open")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(glib::clone!(
                    #[strong]
                    loading,
                    move |app: &gtk::Application, _, parameter| {
                        let text = parameter.and_then(String::from_variant).unwrap_or_default();
                        if text.is_empty() {
                            present(app, &loading, None);
                            return;
                        }
                        match Link::parse(&text, Registry::builtin()) {
                            Ok(link) => present(app, &loading, Some(link)),
                            Err(error) => eprintln!("sway-things-gui: {error}"),
                        }
                    }
                ))
                .build();
            app.add_action_entries([open]);
        }
    ));
    app.connect_activate(glib::clone!(
        #[strong]
        loading,
        move |app| present(app, &loading, None)
    ));
    app.connect_command_line(|app, command_line| {
        let arguments = command_line.arguments();
        let links: Vec<String> = arguments
            .iter()
            .skip(1)
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        match links.as_slice() {
            [] => {
                app.activate();
                glib::ExitCode::SUCCESS
            }
            [text] => match Link::parse(text, Registry::builtin()) {
                Ok(_) => {
                    app.activate_action("open", Some(&text.to_variant()));
                    glib::ExitCode::SUCCESS
                }
                Err(error) => {
                    command_line.printerr_literal(&format!("error: {error}\n"));
                    glib::ExitCode::new(USAGE)
                }
            },
            _ => {
                command_line.printerr_literal(
                    "error: one link at most: sway-things-gui [<page id or setting key>]\n",
                );
                glib::ExitCode::new(USAGE)
            }
        }
    });
    app
}

/// Presents the window of `app` at `link`, if one is given. The first time, it builds the window
/// once the settings are loaded; a link asked for meanwhile is opened when it is there.
fn present(app: &gtk::Application, loading: &Rc<Loading>, link: Option<Link<'static>>) {
    if let Some(window) = app
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<Window>().ok())
    {
        if let Some(link) = link {
            window.open_link(&link);
        }
        window.present();
        return;
    }
    if link.is_some() {
        loading.link.replace(link);
    }
    if loading.started.replace(true) {
        return;
    }
    // Loading connects to sway; the application stays up until the window is there.
    let hold = app.hold();
    glib::spawn_future_local(glib::clone!(
        #[strong]
        app,
        #[strong]
        loading,
        async move {
            let model = Model::load().await;
            let window = Window::new(&app, &model);
            if let Some(link) = loading.link.take() {
                window.open_link(&link);
            }
            window.present();
            loading.started.set(false);
            drop(hold);
        }
    ));
}

/// Runs the application until its last window closes.
pub fn run() -> glib::ExitCode {
    build().run()
}
