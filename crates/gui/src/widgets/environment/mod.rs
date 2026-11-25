// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Environment page's own parts, which [`PageView`](super::PageView) adds to the rows built
//! from the registry: the notice at the top when the session doesn't read environment.d, with
//! the line that makes it do so to copy; the list of variables in place of the row of
//! `environment.variables` ([`variables`]); the Portals row with the check of the XDG desktop
//! portals in place of the row of `environment.portal_config` ([`portals`]); below a row of a setting that writes variables (also
//! on other pages), the line of another environment.d file that replaces one.

mod portals;
mod variables;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::apps::Dirs;
use sway_things_core::environment::files::{self, Line, Origin};
use sway_things_core::environment::{self, session};
use sway_things_core::registry::environment::{PORTAL_CONFIG, VARIABLES};
use sway_things_core::registry::{Registry, Setting};

use super::SettingRow;
use super::notice::{self, Tone};
use super::setting_row::tilde;
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;

pub use variables::VariableList;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// The notice that the session doesn't read environment.d.
const NOT_READ: Msg = msg!(
    "Your session doesn't read environment.d: sway started without {names} from Sway Things. \
     Run this line before sway starts, in ~/.profile or in the script that starts sway; uwsm, \
     openSUSEway and Fedora's start-sway do it already."
);
/// A variable of a setting that a later file sets again.
const SET_AGAIN: Msg = msg!("{name} is set again in {place}, which wins over this setting");

thread_local! {
    /// The sway session the app runs in, read once: its environment doesn't change.
    static SESSION: Option<session::Session> = session::Session::of_this_session();
    /// The XDG directories of this session, read once.
    static DIRS: Option<Dirs> = Dirs::from_env();
    /// The environment of the systemd user manager, which the values of environment.d expand
    /// with: empty until it answered.
    static BASE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Whether `setting` gets a row of its own: the variables are a list, the portal configuration
/// is made in the dialog of the Portals row ([`instead`]).
pub fn has_row(setting: &Setting) -> bool {
    setting.key() != VARIABLES && setting.key() != PORTAL_CONFIG
}

/// The list of variables in place of the row of `environment.variables`, the Portals row in
/// place of that of `environment.portal_config`.
pub fn instead(model: &Model, setting: &Setting) -> Option<gtk::Widget> {
    match setting.key() {
        VARIABLES => Some(VariableList::new(model).widget().clone().upcast()),
        PORTAL_CONFIG => Some(portals::row(model)),
        _ => None,
    }
}

/// Every line of environment.d as the settings in `model` would make Sway Things' file, which
/// line replaces each, and the value each gives, expanded.
#[derive(Debug, Default)]
pub(crate) struct Listing {
    /// The lines, in the order systemd reads them.
    pub lines: Vec<Line>,
    /// For each line, the index of the line that replaces its value.
    pub overridden_by: Vec<Option<usize>>,
    /// For each line, its value expanded.
    pub expanded: Vec<String>,
}

impl Listing {
    /// The listing for the settings of `model`.
    pub fn of(model: &Model) -> Self {
        let Some(state) = model.state() else {
            return Self::default();
        };
        let ours = environment::variables(Registry::builtin(), &state);
        let lines = DIRS
            .with(|dirs| dirs.as_ref().map(|dirs| files::of_session(dirs, &ours)))
            .unwrap_or_default();
        let overridden_by = files::overridden_by(&lines);
        let expanded = BASE.with(|base| files::expanded(&lines, &base.borrow()));
        Self {
            lines,
            overridden_by,
            expanded,
        }
    }

    /// Where `line` is, to show: the file with `~` for the home directory, and the line.
    pub fn place(line: &Line) -> String {
        format!("{}:{}", tilde(&line.file), line.line)
    }

    /// The line of Sway Things' file that sets `name`, by index.
    pub fn ours(&self, name: &str) -> Option<usize> {
        self.lines
            .iter()
            .position(|line| line.origin == Origin::Ours && line.name == name)
    }

    /// What replaces the line at `index`, in words, if another line does.
    pub fn replaced(&self, index: usize) -> Option<String> {
        let by = self.overridden_by.get(index).copied().flatten()?;
        let line = self.lines.get(by)?;
        Some(if line.origin == Origin::Ours {
            tr(msg!("Overridden by Sway Things")).to_owned()
        } else {
            trf(
                msg!("Overridden by {place}"),
                &[("place", &Self::place(line))],
            )
        })
    }
}

/// A dialog window titled `title` over the window of `from`, which Escape closes. Returns it and
/// its content, which has the title as its heading.
pub(super) fn dialog_window(from: &gtk::Widget, title: &str) -> (gtk::Window, gtk::Box) {
    let window = gtk::Window::builder()
        .modal(true)
        .resizable(true)
        .destroy_with_parent(true)
        .default_width(460)
        .title(title)
        .build();
    window.set_transient_for(from.root().and_downcast::<gtk::Window>().as_ref());
    window.add_css_class("st-dialog");
    window.add_css_class("st-environment-dialog");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(24);
    content.set_margin_bottom(16);
    content.set_margin_start(20);
    content.set_margin_end(20);
    let heading = gtk::Label::new(Some(title));
    heading.set_wrap(true);
    heading.add_css_class("title-2");
    heading.set_accessible_role(gtk::AccessibleRole::Heading);
    content.append(&heading);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .child(&content)
        .build();
    window.set_child(Some(&scroller));
    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(|widget, _| {
            if let Some(window) = widget.downcast_ref::<gtk::Window>() {
                window.close();
            }
            glib::Propagation::Stop
        })),
    ));
    window.add_controller(escape);
    (window, content)
}

/// Asks the user manager for its environment once, then calls `then`.
fn load_base(then: impl FnOnce() + 'static) {
    glib::spawn_future_local(async move {
        if let Some(base) = files::user_manager().await {
            BASE.with(|known| *known.borrow_mut() = base);
        }
        then();
    });
}

/// Below the row of a setting that writes environment variables: which line of another file
/// replaces one of them, so the setting has no effect; hidden while none does.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let Some(setting) = row.setting() else {
        return Vec::new();
    };
    let Some(&mapping) = setting.environment() else {
        return Vec::new();
    };
    if setting.key() == VARIABLES {
        return Vec::new();
    }
    let label = notice::text("");
    label.add_css_class(style::DIM);
    let sign = gtk::Image::from_icon_name("dialog-warning-symbolic");
    sign.add_css_class("st-environment-warning");
    sign.set_valign(gtk::Align::Start);
    let note = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    note.add_css_class("st-environment-note");
    note.append(&sign);
    note.append(&label);
    let sync = |note: &gtk::Box| {
        if let Some(parent) = note.parent() {
            parent.set_visible(note.is_visible());
        }
    };
    note.connect_parent_notify(sync);
    note.connect_visible_notify(sync);
    let follow = glib::clone!(
        #[weak]
        note,
        #[weak]
        label,
        move |model: &Model| {
            let listing = Listing::of(model);
            let said: Vec<String> = listing
                .lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.origin == Origin::Ours && mapping.owns(&line.name))
                .filter_map(|(index, line)| {
                    let by = listing.overridden_by.get(index).copied().flatten()?;
                    let later = listing.lines.get(by)?;
                    Some(trf(
                        SET_AGAIN,
                        &[("name", &line.name), ("place", &Listing::place(later))],
                    ))
                })
                .collect();
            label.set_text(&said.join("\n"));
            note.set_visible(!said.is_empty());
        }
    );
    follow(model);
    model.connect_changed(follow);
    vec![note.upcast()]
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
