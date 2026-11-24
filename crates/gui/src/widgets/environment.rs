// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Environment page's own parts, which [`PageView`](super::PageView) adds to the rows built
//! from the registry: the notice at the top when the session doesn't read environment.d, with
//! the line that makes it do so to copy.

use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::apps::Dirs;
use sway_things_core::environment::{self, session};
use sway_things_core::registry::Registry;

use super::notice::{self, Tone};
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The notice that the session doesn't read environment.d.
const NOT_READ: Msg = msg!(
    "Your session doesn't read environment.d: sway started without {names} from Sway Things. \
     Run this line before sway starts, in ~/.profile or in the script that starts sway; uwsm, \
     openSUSEway and Fedora's start-sway do it already."
);

thread_local! {
    /// The sway session the app runs in, read once: its environment doesn't change.
    static SESSION: Option<session::Session> = session::Session::of_this_session();
}

/// The names of the variables of Sway Things' file the session didn't get, although the file is
/// older than the session.
fn not_read(model: &Model) -> Vec<String> {
    let Some((state, dirs)) = model.state().zip(Dirs::from_env()) else {
        return Vec::new();
    };
    let Ok(written) = std::fs::metadata(environment::path(&dirs)).and_then(|meta| meta.modified())
    else {
        return Vec::new();
    };
    let ours = environment::variables(Registry::builtin(), &state);
    SESSION.with(|session| {
        session
            .as_ref()
            .map(|session| session::not_read(&ours, written, session))
            .unwrap_or_default()
    })
}

/// The notice above the page's rows that the session doesn't read environment.d, with the line
/// to run before sway starts and a button that copies it; hidden while the session reads it.
pub fn note(model: &Model) -> gtk::Widget {
    let said = notice::text("");
    let line = gtk::Label::new(Some(&session::import_line(&session::generator())));
    line.set_wrap(true);
    line.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    line.set_xalign(0.0);
    line.set_selectable(true);
    line.add_css_class("monospace");
    line.add_css_class(style::DIM);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&said);
    content.append(&line);
    let copy = copy_button(&line);
    let note = notice::notice(Tone::Warning, &content, Some(&copy));
    note.set_margin_bottom(12);
    let follow = glib::clone!(
        #[weak]
        note,
        #[weak]
        said,
        move |model: &Model| {
            let names = not_read(model);
            note.set_visible(!names.is_empty());
            said.set_text(&trf(NOT_READ, &[("names", &names.join(", "))]));
        }
    );
    follow(model);
    model.connect_changed(follow);
    note.upcast()
}

/// A button that copies the text of `line` and says so for a moment.
fn copy_button(line: &gtk::Label) -> gtk::Button {
    let copy = gtk::Button::with_label(tr(msg!("Copy Line")));
    // Each click says "Copied" for a while; only the latest click's timeout restores the label.
    let clicks = Rc::new(Cell::new(0_u32));
    copy.connect_clicked(glib::clone!(
        #[weak]
        line,
        move |button| {
            button.clipboard().set_text(&line.text());
            button.set_label(tr(msg!("Copied")));
            let click = clicks.get().wrapping_add(1);
            clicks.set(click);
            glib::timeout_add_seconds_local_once(
                2,
                glib::clone!(
                    #[weak]
                    button,
                    #[strong]
                    clicks,
                    move || {
                        if clicks.get() == click {
                            button.set_label(tr(msg!("Copy Line")));
                        }
                    }
                ),
            );
        }
    ));
    copy
}
