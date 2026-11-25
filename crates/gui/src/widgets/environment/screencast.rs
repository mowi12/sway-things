// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The output chooser of xdg-desktop-portal-wlr on the Environment page: the drop-down of
//! `environment.screencast_chooser` with the options whose tool isn't installed insensitive and
//! their reason; the display of `environment.screencast_output` picked from the connected ones;
//! and below the chooser, what keeps it from taking effect: a tool to install (the command to
//! copy), the user's own file that decides (read only, _Open in Editor_), a file of the system
//! Sway Things' file replaces, a session that isn't sway's.

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::apps::Dirs;
use sway_things_core::environment::screencast::{self, Chooser};
use sway_things_core::packages::Manager;
use sway_things_core::registry::environment::{SCREENCAST_CHOOSER, SCREENCAST_OUTPUT};
use sway_things_core::registry::{Kind, Setting};

use super::variables::editor_button;
use super::{DIRS, copy_button, tilde};
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;
use crate::style;
use crate::widgets::SettingRow;

// Messages used inside `glib::clone!` blocks, which xgettext can't look into: they are marked
// out here.
/// An option whose tool isn't installed.
const SLURP_MISSING: Msg = msg!("slurp isn't installed");
/// The menu without a launcher.
const MENU_MISSING: Msg = msg!("No launcher installed: fuzzel, wofi, rofi, bemenu or wmenu");
/// The empty choice of the display.
const NO_DISPLAY: Msg = msg!("None chosen");

/// The choices of the display to share: none, then the connected outputs.
pub fn choices(model: &Model, setting: &Setting) -> Option<Vec<(String, String)>> {
    (setting.key() == SCREENCAST_OUTPUT).then(|| output_choices(model))
}

fn output_choices(model: &Model) -> Vec<(String, String)> {
    let mut choices = vec![(String::new(), tr(NO_DISPLAY).to_owned())];
    choices.extend(model.output_choices());
    choices
}

/// Why `chooser` can't be picked here, if it can't: its tool isn't installed.
fn unavailable(chooser: Chooser, dirs: &Dirs) -> Option<&'static str> {
    match chooser {
        Chooser::Slurp if !screencast::has_slurp(dirs) => Some(tr(SLURP_MISSING)),
        Chooser::Menu if screencast::menu(dirs).is_none() => Some(tr(MENU_MISSING)),
        _ => None,
    }
}

/// Makes the rows of the output chooser follow the session: the options whose tool is missing
/// insensitive, the row locked while a file of the user's decides, the displays as they are
/// connected.
pub fn follow(model: &Model, row: &SettingRow) {
    let Some(setting) = row.setting() else {
        return;
    };
    match setting.key() {
        SCREENCAST_CHOOSER => {
            mark_unavailable(setting, row);
            let lock = |row: &SettingRow| {
                let users = DIRS.with(|dirs| dirs.as_ref().and_then(screencast::users_config));
                row.set_sensitive(users.is_none());
            };
            lock(row);
            let row = row.downgrade();
            model.connect_changed(move |_| {
                if let Some(row) = row.upgrade() {
                    lock(&row);
                }
            });
        }
        SCREENCAST_OUTPUT => {
            let row = row.downgrade();
            model.connect_changed(move |model| {
                if let Some(row) = row.upgrade() {
                    row.set_choices(&output_choices(model));
                }
            });
        }
        _ => {}
    }
}

/// Draws the options of the chooser's drop-down, those whose tool is missing insensitive with
/// the reason below their label.
fn mark_unavailable(setting: &Setting, row: &SettingRow) {
    let (Some(dropdown), Kind::Choice { choices, .. }) = (row.choice_dropdown(), setting.kind())
    else {
        return;
    };
    let ids: Vec<&'static str> = choices.iter().map(|choice| choice.id).collect();
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        let reason = gtk::Label::new(None);
        reason.set_xalign(0.0);
        reason.add_css_class(style::DIM);
        reason.add_css_class("caption");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
        content.append(&label);
        content.append(&reason);
        item.set_child(Some(&content));
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let title = item
            .item()
            .and_downcast::<gtk::StringObject>()
            .map(|object| object.string().to_string())
            .unwrap_or_default();
        let chooser = usize::try_from(item.position())
            .ok()
            .and_then(|index| ids.get(index))
            .map_or(Chooser::Default, |id| Chooser::from_id(id));
        let why = DIRS.with(|dirs| dirs.as_ref().and_then(|dirs| unavailable(chooser, dirs)));
        let Some(content) = item.child() else {
            return;
        };
        if let Some(label) = content.first_child().and_downcast::<gtk::Label>() {
            label.set_text(&title);
        }
        if let Some(reason) = content.last_child().and_downcast::<gtk::Label>() {
            reason.set_text(why.unwrap_or_default());
            reason.set_visible(why.is_some());
        }
        content.set_sensitive(why.is_none());
        item.set_activatable(why.is_none());
        item.set_selectable(why.is_none());
    });
    dropdown.set_list_factory(Some(&factory));
}

/// Below the chooser: what keeps it from taking effect, followed as the settings change.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    if row.setting().map(Setting::key) != Some(SCREENCAST_CHOOSER) {
        return Vec::new();
    }
    let notes = gtk::Box::new(gtk::Orientation::Vertical, 8);
    notes.add_css_class("st-screencast-notes");
    let sync = |notes: &gtk::Box| {
        if let Some(parent) = notes.parent() {
            parent.set_visible(notes.is_visible());
        }
    };
    notes.connect_parent_notify(sync);
    notes.connect_visible_notify(sync);
    let follow = glib::clone!(
        #[weak]
        notes,
        move |model: &Model| fill(&notes, model)
    );
    follow(model);
    model.connect_changed(follow);
    vec![notes.upcast()]
}

/// Fills `notes` for the settings of `model`, and hides it while there is nothing to say.
fn fill(notes: &gtk::Box, model: &Model) {
    while let Some(child) = notes.first_child() {
        notes.remove(&child);
    }
    let Some(dirs) = DIRS.with(Clone::clone) else {
        notes.set_visible(false);
        return;
    };
    let chooser = model
        .state()
        .map_or(Chooser::Default, |state| Chooser::of(&state));
    if let Some(users) = screencast::users_config(&dirs) {
        notes.append(&line(&trf(
            msg!("{file} is your own and decides how to choose: Sway Things doesn't change it."),
            &[("file", &tilde(&users))],
        )));
        let open = editor_button(model, users, 1);
        open.set_halign(gtk::Align::Start);
        notes.append(&open);
    } else {
        let manager = Manager::detect();
        let missing = |package: &str| manager.map(|manager| manager.install_command(&[package]));
        if !screencast::has_slurp(&dirs) {
            notes.append(&line(tr(msg!(
                "To ask by clicking a display, install slurp:"
            ))));
            if let Some(command) = missing(screencast::SLURP.package) {
                notes.append(&to_copy(&command));
            }
        }
        if screencast::menu(&dirs).is_none() {
            notes.append(&line(tr(msg!(
                "To pick from a menu, install a launcher such as fuzzel:"
            ))));
            if let Some(command) = screencast::MENUS
                .first()
                .and_then(|menu| missing(menu.program.package))
            {
                notes.append(&to_copy(&command));
            }
        }
        if chooser != Chooser::Default {
            if let Some(system) = screencast::shadowed(&dirs) {
                notes.append(&line(&trf(
                    msg!("Replaces {file} of the system for xdg-desktop-portal-wlr."),
                    &[("file", &system.display())],
                )));
            }
            if !dirs
                .desktops
                .iter()
                .any(|desktop| desktop == screencast::FILE_NAME)
            {
                notes.append(&line(tr(msg!(
                    "XDG_CURRENT_DESKTOP doesn't name sway, so xdg-desktop-portal-wlr doesn't \
                     read Sway Things' file."
                ))));
            }
        }
    }
    notes.set_visible(notes.first_child().is_some());
}

/// A line of text, dim.
fn line(said: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(said));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class(style::DIM);
    label.upcast()
}

/// A command to copy, in monospace, with a button that copies it.
fn to_copy(command: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(command));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    // A long command wraps instead of widening the page.
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.add_css_class("monospace");
    let copy = copy_button(&label);
    copy.set_valign(gtk::Align::Center);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("st-portals-copy");
    row.append(&label);
    row.append(&copy);
    row.upcast()
}
