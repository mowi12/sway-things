// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiles the icons of the app into a resource bundle, which `icons::register` puts into the
//! binary, so nothing has to be installed for them.

fn main() {
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/resources.gresource.xml",
        "sway-things.gresource",
    );
}
