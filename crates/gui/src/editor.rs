// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opening a file in the user's text editor: the command comes from the preference
//! `app.editor_command`, and [`sway_things_core::editor::resolve`] decides what runs.

use std::path::Path;

use gtk::prelude::*;
use gtk::{gio, glib};
use sway_things_core::editor::{self, Launch};
use sway_things_core::registry::{Registry, Value};

use crate::model::Model;

/// Opens `file` at `line` (counted from 1), if known, with the editor of `model`'s preference:
/// a program it names, `$VISUAL` or `$EDITOR` in a terminal, or the desktop's default application
/// for text files. `widget` is the one the action came from; its window is the parent of any
/// dialog. `on_result` gets a message for the user if the editor can't be started or fails, and
/// `None` once it opened the file: the default application started, or the program ended
/// without an error.
pub fn open(
    widget: &impl IsA<gtk::Widget>,
    model: Option<&Model>,
    file: &Path,
    line: Option<usize>,
    on_result: impl Fn(Option<&str>) + 'static,
) {
    let command = model.map(command).unwrap_or_default();
    let launch = match editor::resolve(&command, file, line, &|name| std::env::var_os(name)) {
        Ok(launch) => launch,
        Err(error) => {
            on_result(Some(&format!("The editor command {error}")));
            return;
        }
    };
    let run = match launch {
        Launch::Run(run) => run,
        Launch::DefaultApp => {
            let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(file)));
            let window = widget.as_ref().root().and_downcast::<gtk::Window>();
            launcher.launch(
                window.as_ref(),
                gio::Cancellable::NONE,
                move |result| match result {
                    Ok(()) => on_result(None),
                    Err(error) => on_result(Some(&format!("The config can't be opened: {error}"))),
                },
            );
            return;
        }
    };
    let program = run.program.to_string_lossy().into_owned();
    let mut child = match run.spawn() {
        Ok(child) => child,
        Err(error) => {
            on_result(Some(&format!("{program} can't be started: {error}")));
            return;
        }
    };
    // Waiting reaps the editor once it ends, so it doesn't stay behind as a zombie.
    glib::spawn_future_local(async move {
        match child.status().await {
            Ok(status) if status.success() => on_result(None),
            Ok(status) => on_result(Some(&format!("{program} ended with {status}"))),
            Err(error) => on_result(Some(&format!("{program} failed: {error}"))),
        }
    });
}

/// The editor command of `model`'s preference; empty if there is none.
fn command(model: &Model) -> String {
    Registry::builtin()
        .get(editor::KEY)
        .and_then(|setting| model.view(setting, None))
        .and_then(|view| match view.value {
            Value::Text(command) => Some(command),
            _ => None,
        })
        .unwrap_or_default()
}
