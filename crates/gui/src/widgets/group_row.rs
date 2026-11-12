// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The row of a group of settings that render one sway directive together, like the five colors
//! of `client.focused`: the group's title, a summary of the members' values (a swatch per color),
//! the shared line of the sway config while `app.show_directives` is on, and a chevron. It
//! expands to the members' rows, indented below it, which don't repeat the line.
//!
//! The row takes the keyboard focus; `Enter` or `Space` expands and collapses it, like a click.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib};
use sway_things_core::registry::{Kind, SettingGroup, Value};

use super::SettingRow;
use super::setting_row::shows_directives;
use crate::i18n::tr;
use crate::model::Model;
use crate::style;

/// The style class of the list item of a member row.
pub const MEMBER: &str = "st-member";

/// A swatch of the summary and the color it shows.
type Swatch = (gtk::DrawingArea, Rc<Cell<gdk::RGBA>>);

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use sway_things_core::registry::SettingGroup;

    use crate::model::Model;

    #[derive(Debug)]
    pub struct GroupRow {
        pub(super) group: OnceCell<SettingGroup>,
        pub(super) model: OnceCell<Model>,
        pub(super) members: OnceCell<Vec<super::SettingRow>>,
        /// The list item the row is the child of, which takes the focus.
        pub(super) item: gtk::ListBoxRow,
        pub(super) expanded: Cell<bool>,
        pub(super) title: gtk::Label,
        pub(super) line: gtk::Label,
        pub(super) summary: gtk::Box,
        pub(super) summary_text: gtk::Label,
        pub(super) swatches: RefCell<Vec<super::Swatch>>,
        pub(super) chevron: gtk::Image,
        /// The list items of the members.
        pub(super) member_items: OnceCell<Vec<gtk::ListBoxRow>>,
    }

    impl Default for GroupRow {
        fn default() -> Self {
            Self {
                group: OnceCell::new(),
                model: OnceCell::new(),
                members: OnceCell::new(),
                // Roles are fixed at construction: the item expands and collapses like a button,
                // and the chevron says nothing the expanded state doesn't.
                item: gtk::ListBoxRow::builder()
                    .accessible_role(gtk::AccessibleRole::Button)
                    .build(),
                expanded: Cell::new(false),
                title: gtk::Label::default(),
                line: gtk::Label::default(),
                summary: gtk::Box::default(),
                summary_text: gtk::Label::default(),
                swatches: RefCell::default(),
                chevron: gtk::Image::builder()
                    .accessible_role(gtk::AccessibleRole::Presentation)
                    .build(),
                member_items: OnceCell::new(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GroupRow {
        const NAME: &'static str = "StGroupRow";
        type Type = super::GroupRow;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-group-row");
            klass.set_layout_manager_type::<gtk::BoxLayout>();
        }
    }

    impl ObjectImpl for GroupRow {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for GroupRow {}
}

glib::wrapper! {
    /// The expandable row of a group of settings.
    pub struct GroupRow(ObjectSubclass<imp::GroupRow>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl GroupRow {
    /// The row of `group`, with the rows of its `members`, collapsed. The members don't show the
    /// line of the sway config; the group shows it once.
    pub fn new(model: &Model, group: SettingGroup, members: Vec<SettingRow>) -> Self {
        let row: Self = glib::Object::new();
        let imp = row.imp();
        let _ = imp.group.set(group);
        let _ = imp.model.set(model.clone());
        for member in &members {
            member.set_grouped(true);
        }
        let _ = imp.members.set(members);
        row.build();
        row.set_expanded(false);
        model.connect_changed(glib::clone!(
            #[weak]
            row,
            move |_| row.refresh()
        ));
        row.refresh();
        row
    }

    fn build(&self) {
        let imp = self.imp();
        let Some(group) = imp.group.get().copied() else {
            return;
        };
        let title = tr(group.title());
        imp.title.set_text(title);
        imp.title.add_css_class("st-row-title");
        imp.line.add_css_class("monospace");
        imp.line.add_css_class("st-row-detail");
        imp.line.add_css_class(style::DIM);
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 3);
        texts.set_hexpand(true);
        texts.set_valign(gtk::Align::Center);
        for label in [&imp.title, &imp.line] {
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            texts.append(label);
        }

        imp.summary.set_spacing(4);
        imp.summary.set_valign(gtk::Align::Center);
        imp.summary_text.add_css_class(style::DIM);
        imp.summary.append(&imp.summary_text);
        let members = self.members();
        if members
            .iter()
            .filter_map(SettingRow::setting)
            .all(|setting| matches!(setting.kind(), Kind::Color { .. }))
        {
            imp.summary_text.set_visible(false);
            let swatches = members.iter().map(|_| swatch()).collect::<Vec<_>>();
            for (area, _) in &swatches {
                imp.summary.append(area);
            }
            imp.swatches.replace(swatches);
        }
        imp.chevron.set_valign(gtk::Align::Center);

        texts.set_parent(self);
        imp.summary.set_parent(self);
        imp.chevron.set_parent(self);
        if let Some(layout) = self.layout_manager().and_downcast::<gtk::BoxLayout>() {
            layout.set_spacing(12);
        }

        let item = &imp.item;
        item.set_child(Some(self));
        item.set_activatable(false);
        item.set_focusable(true);
        item.add_css_class("st-group");
        item.update_property(&[gtk::accessible::Property::Label(title)]);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = row)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| row.key_pressed(key, modifiers)
        ));
        item.add_controller(keys);
        let click = gtk::GestureClick::new();
        click.connect_released(glib::clone!(
            #[weak(rename_to = row)]
            self,
            move |gesture, _, _, _| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                row.toggle();
            }
        ));
        item.add_controller(click);

        let member_items = members
            .iter()
            .map(|member| {
                let item = gtk::ListBoxRow::new();
                item.set_activatable(false);
                item.set_focusable(false);
                item.add_css_class(MEMBER);
                item.set_child(Some(member));
                item
            })
            .collect();
        let _ = imp.member_items.set(member_items);
    }

    /// Toggles the row on `Enter` or `Space` without modifiers.
    fn key_pressed(&self, key: gdk::Key, modifiers: gdk::ModifierType) -> glib::Propagation {
        let toggles = matches!(
            key,
            gdk::Key::Return
                | gdk::Key::KP_Enter
                | gdk::Key::ISO_Enter
                | gdk::Key::space
                | gdk::Key::KP_Space
        );
        if toggles && (modifiers & gtk::accelerator_get_default_mod_mask()).is_empty() {
            self.toggle();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    }

    /// The list item of the row, which takes the focus.
    pub fn item(&self) -> &gtk::ListBoxRow {
        &self.imp().item
    }

    /// The list items to put into a section list: the row's, then those of the members.
    pub fn items(&self) -> Vec<gtk::ListBoxRow> {
        let mut items = vec![self.item().clone()];
        items.extend(self.imp().member_items.get().into_iter().flatten().cloned());
        items
    }

    /// The group of the row.
    pub fn group(&self) -> Option<SettingGroup> {
        self.imp().group.get().copied()
    }

    /// The rows of the members.
    pub fn members(&self) -> &[SettingRow] {
        self.imp().members.get().map_or(&[], Vec::as_slice)
    }

    /// Whether the members are shown.
    pub fn is_expanded(&self) -> bool {
        self.imp().expanded.get()
    }

    /// Shows or hides the members.
    pub fn set_expanded(&self, expanded: bool) {
        let imp = self.imp();
        imp.expanded.set(expanded);
        for item in imp.member_items.get().into_iter().flatten() {
            item.set_visible(expanded);
        }
        imp.chevron.set_icon_name(Some(if expanded {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        }));
        imp.item
            .update_state(&[gtk::accessible::State::Expanded(Some(expanded))]);
    }

    /// Expands the row if it is collapsed, and the other way round.
    pub fn toggle(&self) {
        self.set_expanded(!self.is_expanded());
    }

    /// The line of the sway config, if the row shows it.
    pub fn line(&self) -> Option<String> {
        let line = &self.imp().line;
        line.is_visible().then(|| line.text().to_string())
    }

    /// The colors of the swatches, in the order of the members.
    pub fn swatches(&self) -> Vec<gdk::RGBA> {
        self.imp()
            .swatches
            .borrow()
            .iter()
            .map(|(_, color)| color.get())
            .collect()
    }

    /// Shows the members' values and the line, for the device the members are for.
    pub fn refresh(&self) {
        let imp = self.imp();
        let Some(model) = imp.model.get() else {
            return;
        };
        let members = self.members();
        let views: Vec<_> = members
            .iter()
            .filter_map(|member| {
                let setting = member.setting()?;
                let view = model.view(setting, member.device().as_deref())?;
                Some((setting, view))
            })
            .collect();
        let line = views.first().and_then(|(_, view)| view.line.clone());
        imp.line.set_text(line.as_deref().unwrap_or_default());
        imp.line
            .set_visible(line.is_some() && shows_directives(model));

        let texts: Vec<String> = views
            .iter()
            .map(|(setting, view)| value_text(setting.kind(), &view.value))
            .collect();
        imp.summary_text.set_text(&texts.join(" · "));
        let named: Vec<String> = views
            .iter()
            .zip(&texts)
            .map(|((setting, _), text)| format!("{} {text}", tr(setting.title())))
            .collect();
        imp.item
            .update_property(&[gtk::accessible::Property::Description(&named.join(", "))]);
        for ((area, color), (_, view)) in imp.swatches.borrow().iter().zip(&views) {
            if let Value::Color(value) = &view.value {
                if let Ok(rgba) = gdk::RGBA::parse(value.to_string()) {
                    color.set(rgba);
                    area.queue_draw();
                }
            }
            area.set_tooltip_text(Some(&view.value.to_string()));
        }
    }
}

/// A swatch of 16 by 16 pixels with rounded corners and a faint outline in the text color, so a
/// color close to the background still shows.
fn swatch() -> Swatch {
    let color = Rc::new(Cell::new(gdk::RGBA::TRANSPARENT));
    let area = gtk::DrawingArea::builder()
        .content_width(16)
        .content_height(16)
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    area.set_draw_func(glib::clone!(
        #[strong]
        color,
        move |area, cr, width, height| {
            let (width, height) = (f64::from(width), f64::from(height));
            let radius = 4.0;
            cr.new_sub_path();
            cr.arc(
                width - radius - 0.5,
                radius + 0.5,
                radius,
                -90f64.to_radians(),
                0.0,
            );
            cr.arc(
                width - radius - 0.5,
                height - radius - 0.5,
                radius,
                0.0,
                90f64.to_radians(),
            );
            cr.arc(
                radius + 0.5,
                height - radius - 0.5,
                radius,
                90f64.to_radians(),
                180f64.to_radians(),
            );
            cr.arc(
                radius + 0.5,
                radius + 0.5,
                radius,
                180f64.to_radians(),
                270f64.to_radians(),
            );
            cr.close_path();
            let fill = color.get();
            cr.set_source_rgba(
                f64::from(fill.red()),
                f64::from(fill.green()),
                f64::from(fill.blue()),
                f64::from(fill.alpha()),
            );
            let _ = cr.fill_preserve();
            let outline = area.color();
            cr.set_source_rgba(
                f64::from(outline.red()),
                f64::from(outline.green()),
                f64::from(outline.blue()),
                0.35,
            );
            cr.set_line_width(1.0);
            let _ = cr.stroke();
        }
    ));
    (area, color)
}

/// `value` as the summary shows it.
fn value_text(kind: &Kind, value: &Value) -> String {
    match (kind, value) {
        (Kind::Bool { .. }, Value::Bool(true)) => "On".to_owned(),
        (Kind::Bool { .. }, Value::Bool(false)) => "Off".to_owned(),
        (Kind::Choice { choices, .. }, Value::Choice(id)) => choices
            .iter()
            .find(|choice| choice.id == id)
            .map_or_else(|| id.clone(), |choice| tr(choice.label).to_owned()),
        _ => value.to_string(),
    }
}
