// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The indicators of a row: the badges, the reset button and machine toggle, the conflict with
//! the user's config and its popover, and the note under the row.
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::conflict::{Report, UserLine};
use sway_things_core::outputs;
use sway_things_core::registry::{Apply, Scope};
use sway_things_core::state::Layer;
use sway_things_core::view::{Locked, SettingView, Source};

use super::super::device_selector::all_of_type;
use super::SettingRow;
use crate::i18n::{msg, tr, trf};
use crate::style;
use crate::widgets::group_row;
use sway_things_core::registry::Setting;

/// The note under the line: where the value comes from, or why it can't be changed. For one
/// `device` without a value of its own, that it is the value of all devices of the type.
pub(super) fn note(view: &SettingView, setting: &Setting, device: Option<&str>) -> String {
    let mut parts = Vec::new();
    match (&view.source, setting.scope(), device) {
        (Source::Config { file, line }, _, _) => {
            parts.push(trf(
                msg!("From {file}:{line}"),
                &[("file", &tilde(file)), ("line", line)],
            ));
        }
        (
            Source::Stored { for_type: true, .. } | Source::Default,
            Scope::PerDevice(kind),
            Some(_),
        ) => {
            parts.push(trf(
                msg!("Same as {devices}"),
                &[("devices", &all_of_type(kind))],
            ));
        }
        (Source::Stored { for_type: true, .. }, Scope::PerOutput, Some(_)) => {
            parts.push(tr(msg!("Same as all displays")).to_owned());
        }
        // Where sway picks a value itself: the mode, the place and the scale of an output.
        (Source::Session, _, _)
            if matches!(
                setting.key(),
                outputs::MODE | outputs::POSITION | outputs::SCALE
            ) =>
        {
            parts.push(tr(msg!("Chosen by sway")).to_owned());
        }
        _ => {}
    }
    // The setting this one needs ("Turn on Blur first"). What the compositor lacks is a badge,
    // and the include line is explained once for the whole window, not on every row.
    if let Some(Locked::Needs(text)) = &view.locked {
        parts.push(text.translated());
    }
    if let Some(other) = &view.changed_elsewhere {
        parts.push(trf(
            msg!(
                "Apps use {value} now, set by another tool. Sway Things sets its own again when \
                 sway starts or reloads."
            ),
            &[("value", &group_row::value_text(setting, other))],
        ));
    }
    parts.join(" · ")
}

/// How a line of the user's config conflicts with the stored value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Conflict {
    /// It comes after the generated files and decides.
    Wins,
    /// It comes before them and is no longer in effect.
    Overridden,
}

/// The line of the user's config that conflicts with a stored value in `report`: the one that
/// wins over it, else the last one it made dead.
pub(super) fn conflict_line(report: &Report) -> Option<(Conflict, &UserLine)> {
    report
        .overriding()
        .map(|line| (Conflict::Wins, line))
        .or_else(|| {
            report
                .overridden()
                .last()
                .map(|line| (Conflict::Overridden, line))
        })
}

/// A badge under the line: an icon and a text. One that needs attention is marked as such.
pub(super) fn badge(icon: &str, text: &str, attention: bool) -> gtk::Box {
    let badge = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    badge.add_css_class("st-badge");
    if attention {
        badge.add_css_class("st-attention");
    }
    // The icon at the size of the text; a symbolic icon is drawn sharp at whole pixels.
    let image = gtk::Image::builder()
        .icon_name(icon)
        .pixel_size(12)
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    badge.append(&image);
    badge.append(&gtk::Label::new(Some(text)));
    badge
}

/// `path` with the home directory written as `~`.
pub(crate) fn tilde(path: &std::path::Path) -> String {
    let home = glib::home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

impl SettingRow {
    /// Shows the reset button, the toggle, the badges and the ⋮ menu as `view` and the stored
    /// value ask.
    pub(super) fn refresh_indicators(
        &self,
        setting: &'static Setting,
        device: Option<&str>,
        view: &SettingView,
    ) {
        let imp = self.imp();
        let Some(model) = imp.model.get() else { return };
        // While kanshi sets up the displays, each of their values is remembered for the
        // combination as it is: there is no value of all displays to go back to, and nothing
        // only for this machine.
        let layer = model
            .stored_in(setting, device)
            .filter(|_| !(model.kanshi_runs() && setting.scope() == Scope::PerOutput));
        let read_only = matches!(view.locked, Some(Locked::ReadOnly(_)));
        imp.reset.set_child_visible(layer.is_some());
        imp.reset.set_sensitive(!read_only);
        imp.reset
            .set_tooltip_text(Some(&match (setting.scope(), device) {
                (Scope::PerDevice(kind), Some(_)) => trf(
                    msg!("Use the value of {devices}"),
                    &[("devices", &all_of_type(kind))],
                ),
                (Scope::PerOutput, Some(_)) => tr(msg!("Use the value of all displays")).to_owned(),
                _ => tr(msg!("Reset to default")).to_owned(),
            }));
        imp.machine.set_child_visible(layer.is_some());
        imp.machine.set_sensitive(!read_only);
        imp.updating.set(true);
        imp.machine.set_active(layer == Some(Layer::Host));
        imp.updating.set(false);

        while let Some(badge) = imp.badges.first_child() {
            imp.badges.remove(&badge);
        }
        let users = model.report(setting, device);
        // Room for the ⋮ menu wherever it could appear once a value is stored.
        imp.conflict
            .set_visible(users.as_ref().and_then(conflict_line).is_some());
        let report = users.filter(|_| layer.is_some());
        let found = report.as_ref().and_then(conflict_line);
        match &found {
            Some((Conflict::Wins, line)) => {
                imp.badges.append(&badge(
                    "dialog-warning-symbolic",
                    tr(msg!("Your config wins")),
                    true,
                ));
                imp.conflict_title.set_text(tr(msg!("Your config wins")));
                imp.conflict_text.set_text(tr(msg!(
                    "This line comes after the include line, so it decides after a reload."
                )));
                self.show_conflict(line);
            }
            Some((Conflict::Overridden, line)) => {
                imp.badges.append(&badge(
                    "action-unavailable-symbolic",
                    tr(msg!("Overridden")),
                    false,
                ));
                imp.conflict_title
                    .set_text(tr(msg!("Overridden in your config")));
                imp.conflict_text.set_text(tr(msg!(
                    "Sway Things’ value comes later and wins; this line is no longer in effect."
                )));
                self.show_conflict(line);
            }
            None => {}
        }
        if let Some(locked @ Locked::Unsupported(_)) = &view.locked {
            imp.badges.append(&badge(
                "changes-prevent-symbolic",
                &locked.text().translated(),
                false,
            ));
        }
        match setting.apply() {
            Apply::Reload => imp.badges.append(&badge(
                "view-refresh-symbolic",
                tr(msg!("Applies after reload")),
                false,
            )),
            Apply::Relogin => imp.badges.append(&badge(
                "system-log-out-symbolic",
                tr(msg!("Applies after re-login")),
                false,
            )),
            Apply::Live | Apply::ToolReload | Apply::Risky => {}
        }
        imp.conflict.set_child_visible(found.is_some());
        imp.conflict.set_sensitive(!read_only);
        // The user's config can't win over a setting that can lock the user out: the safety
        // net only resets it.
        imp.let_config_win
            .set_visible(setting.apply() != Apply::Risky);
        imp.report
            .replace(found.is_some().then_some(report).flatten());
        imp.badges.set_visible(imp.badges.first_child().is_some());
    }

    pub(super) fn show_conflict(&self, line: &UserLine) {
        let imp = self.imp();
        imp.conflict_place
            .set_text(&format!("{}:{}", tilde(line.file()), line.line()));
        imp.conflict_line.set_text(line.text());
    }
}

impl SettingRow {
    /// The reset button, the "This machine only" toggle and the ⋮ menu of a conflict, after the
    /// control.
    ///
    /// They keep their room in the row while they are hidden (shown or hidden with
    /// `set_child_visible`), so the control never moves when they appear: a slider being dragged
    /// would otherwise shrink under the pointer once the first change stores a value. The ⋮ menu
    /// keeps its room only in rows whose setting the user's config sets too, the only rows where
    /// it can appear.
    pub(super) fn build_indicators(&self, title: &str) {
        let imp = self.imp();
        imp.reset.set_icon_name("edit-undo-symbolic");
        imp.reset
            .update_property(&[gtk::accessible::Property::Label(&format!("Reset {title}"))]);
        imp.reset.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            self,
            move |_| row.reset()
        ));
        imp.machine.set_icon_name("computer-symbolic");
        imp.machine
            .set_tooltip_text(Some(tr(msg!("This machine only"))));
        imp.machine
            .update_property(&[gtk::accessible::Property::Label(&trf(
                msg!("{title}: this machine only"),
                &[("title", &title)],
            ))]);
        imp.machine.connect_toggled(glib::clone!(
            #[weak(rename_to = row)]
            self,
            move |toggle| row.move_to(toggle.is_active())
        ));
        imp.conflict.set_icon_name("view-more-symbolic");
        imp.conflict
            .set_tooltip_text(Some(tr(msg!("Your config sets this too"))));
        imp.conflict
            .update_property(&[gtk::accessible::Property::Label(&trf(
                msg!("{title}: your config sets this too"),
                &[("title", &title)],
            ))]);
        imp.conflict.set_popover(Some(&self.conflict_popover()));
        for button in [
            imp.reset.upcast_ref::<gtk::Widget>(),
            imp.machine.upcast_ref(),
            imp.conflict.upcast_ref(),
        ] {
            button.set_valign(gtk::Align::Center);
            button.set_child_visible(false);
            imp.controls.append(button);
        }
    }

    /// The menu behind ⋮: the line of the user's config, then what to do about it.
    pub(super) fn conflict_popover(&self) -> gtk::Popover {
        let imp = self.imp();
        let details = gtk::Box::new(gtk::Orientation::Vertical, 4);
        details.add_css_class("st-conflict-details");
        imp.conflict_title.add_css_class("st-row-title");
        imp.conflict_place.add_css_class("monospace");
        imp.conflict_place.add_css_class(style::DIM);
        imp.conflict_line.add_css_class("monospace");
        imp.conflict_line.set_selectable(true);
        for label in [
            &imp.conflict_title,
            &imp.conflict_place,
            &imp.conflict_line,
            &imp.conflict_text,
        ] {
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_max_width_chars(48);
            details.append(label);
        }
        let popover = gtk::Popover::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&details);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        imp.let_config_win.set_label(tr(msg!("Let my config win")));
        let open = gtk::Button::with_label(tr(msg!("Open config in editor")));
        let copy = gtk::Button::with_label(tr(msg!("Copy line")));
        for button in [&imp.let_config_win, &open, &copy] {
            button.add_css_class("flat");
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_xalign(0.0);
            }
            content.append(button);
        }
        imp.let_config_win.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                row.let_config_win();
            }
        ));
        open.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                row.open_config();
            }
        ));
        copy.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                let line = row.imp().conflict_line.text();
                row.clipboard().set_text(&line);
            }
        ));
        popover.set_child(Some(&content));
        popover
    }
}
