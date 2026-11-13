// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `sway-things-gui`: the GTK4 settings app of Sway Things.

fn main() -> gtk::glib::ExitCode {
    // The translations of the app's texts, in the user's language.
    sway_things_core::i18n::init();
    sway_things_gui::app::run()
}
