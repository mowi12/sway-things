// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `GtkApplication` of Sway Things.

use gtk::prelude::*;
use gtk::{gio, glib};

use crate::model::Model;
use crate::style;
use crate::window::Window;

/// The application id: the D-Bus name and the sway `app_id` of the window, so users can match it in
/// `for_window` rules.
pub const APP_ID: &str = "io.github.mowi12.SwayThings";

/// Builds the application: loads the style sheet on startup and opens the window on activation.
pub fn build() -> gtk::Application {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::default())
        .build();
    app.connect_startup(|_| style::load());
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        // Loading connects to sway; the application stays up until the window is there.
        let hold = app.hold();
        glib::spawn_future_local(glib::clone!(
            #[strong]
            app,
            async move {
                let model = Model::load().await;
                Window::new(&app, &model).present();
                drop(hold);
            }
        ));
    });
    app
}

/// Runs the application until its last window closes.
pub fn run() -> glib::ExitCode {
    build().run()
}
