// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A page in the content pane: header and body, scrollable, at a readable width.
//!
//! A page built from the registry has a section per registry section. The sections of per-device
//! settings start with a [`DeviceSelector`]: one for each run of sections of the same input type,
//! which picks the device all their rows are for. While no device of the type is connected and
//! nothing is stored for the type, the run shows its first heading and a hint instead of its rows.

use std::collections::BTreeSet;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::command::input::InputType;
use sway_things_core::registry::{Msg, Page, Registry, Scope};
use sway_things_core::view::Source;

use super::device_selector::none_connected;
use super::setting_row::tilde;
use super::{Clamp, DeviceSelector, PageHeader, Placeholder, SettingRow, TestArea};
use crate::i18n::tr;
use crate::model::Model;
use crate::style;

/// The widest a page's content gets, in pixels.
pub const MAXIMUM_WIDTH: i32 = 760;

mod imp {
    use std::cell::OnceCell;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct PageView {
        pub scroller: gtk::ScrolledWindow,
        pub content: gtk::Box,
        pub header: OnceCell<super::PageHeader>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PageView {
        const NAME: &'static str = "StPageView";
        type Type = super::PageView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-page");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for PageView {
        fn constructed(&self) {
            self.parent_constructed();
            self.content.set_orientation(gtk::Orientation::Vertical);
            self.content.add_css_class("st-page-content");
            let clamp = super::Clamp::new(&self.content, super::MAXIMUM_WIDTH);
            self.scroller
                .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
            self.scroller.set_child(Some(&clamp));
            self.scroller.set_parent(&*self.obj());
        }

        fn dispose(&self) {
            self.scroller.unparent();
        }
    }

    impl WidgetImpl for PageView {}
}

glib::wrapper! {
    /// A page of the content pane: its header and its body, scrolled vertically and at most
    /// [`MAXIMUM_WIDTH`] wide.
    pub struct PageView(ObjectSubclass<imp::PageView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PageView {
    /// A page with a header from `title` and `description`, and `body` below it.
    pub fn new(title: &str, description: &str, body: &impl IsA<gtk::Widget>) -> Self {
        let view: Self = glib::Object::new();
        let imp = view.imp();
        let header = PageHeader::new(title, description);
        imp.content.append(&header);
        imp.content.append(body);
        imp.header.get_or_init(|| header);
        view.update_property(&[gtk::accessible::Property::Label(title)]);
        view
    }

    /// The page view of `page` while it isn't built yet: its header and a placeholder.
    pub fn unbuilt(page: Page) -> Self {
        Self::new(
            tr(page.title()),
            tr(page.description()),
            &Placeholder::unbuilt(),
        )
    }

    /// The page view of `page` built from its settings in the registry: a section for each of
    /// their sections, with a row per setting, and the generated files the page writes to below.
    /// Per-device settings get a [`DeviceSelector`] per input type. `None` if the page has no
    /// settings with a row.
    pub fn for_settings(model: &Model, page: Page) -> Option<Self> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let mut section: Option<(Msg, gtk::ListBox)> = None;
        let mut groups: Vec<DeviceGroup> = Vec::new();
        let mut files = BTreeSet::new();
        for setting in Registry::builtin()
            .iter()
            .filter(|setting| setting.page() == page)
        {
            let Some(row) = SettingRow::new(model, setting, None) else {
                continue;
            };
            if let Some(mapping) = setting.sway() {
                files.insert(mapping.file());
            }
            let kind = match setting.scope() {
                Scope::PerDevice(kind) => Some(kind),
                Scope::Global | Scope::PerHost | Scope::PerOutput => None,
            };
            let list = match &section {
                Some((title, list)) if *title == setting.section() => list.clone(),
                _ => {
                    let heading = heading(tr(setting.section()), section.is_none());
                    let list = section_list(setting.section());
                    body.append(&heading);
                    let open = groups
                        .last_mut()
                        .filter(|group| group.open && Some(group.kind) == kind);
                    match (open, kind) {
                        (Some(group), _) => group.sections.push((heading, list.clone())),
                        (None, Some(kind)) => {
                            if let Some(last) = groups.last_mut() {
                                last.open = false;
                            }
                            let group = DeviceGroup::new(kind, heading, &list);
                            body.append(&group.hint);
                            groups.push(group);
                        }
                        (None, None) => {
                            if let Some(last) = groups.last_mut() {
                                last.open = false;
                            }
                        }
                    }
                    body.append(&list);
                    section = Some((setting.section(), list.clone()));
                    list
                }
            };
            if let Some(group) = groups
                .last_mut()
                .filter(|group| group.open && Some(group.kind) == kind)
            {
                group.rows.push(row.clone());
            }
            list.append(&list_row(&row));
        }
        section.as_ref()?;
        for group in groups {
            group.follow(model);
        }
        if page == Page::MouseTouchpad {
            body.append(&heading("Test area", false));
            body.append(&TestArea::new());
        }
        if let (Some(locations), false) = (model.locations(), files.is_empty()) {
            let paths: Vec<String> = files
                .into_iter()
                .map(|file| tilde(&locations.path(file)))
                .collect();
            let footer = gtk::Label::new(Some(&paths.join("\n")));
            footer.set_xalign(0.0);
            footer.set_wrap(true);
            footer.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            footer.set_selectable(true);
            // Selectable with the mouse, but not a stop for the keyboard, which would select it all.
            footer.set_focusable(false);
            footer.add_css_class("monospace");
            footer.add_css_class("st-page-footer");
            footer.add_css_class(style::DIM);
            footer.update_property(&[gtk::accessible::Property::Label(&format!(
                "Written to {}",
                paths.join(", ")
            ))]);
            body.append(&footer);
        }
        Some(Self::new(tr(page.title()), tr(page.description()), &body))
    }

    /// The header of the page.
    pub fn header(&self) -> Option<&PageHeader> {
        self.imp().header.get()
    }
}

/// The heading of a section titled `title`.
fn heading(title: &str, first: bool) -> gtk::Label {
    let heading = gtk::Label::builder()
        .label(title.to_uppercase())
        .xalign(0.0)
        .accessible_role(gtk::AccessibleRole::Heading)
        .build();
    heading.update_property(&[gtk::accessible::Property::Label(title)]);
    heading.add_css_class("st-section-heading");
    heading.add_css_class(style::DIM);
    if first {
        heading.add_css_class("st-first");
    }
    heading
}

/// The list for the rows of a section titled `title`.
fn section_list(title: Msg) -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("st-section");
    list.update_property(&[gtk::accessible::Property::Label(tr(title))]);
    list
}

/// `child` as a row of a section list. The row itself takes no focus; its controls do.
fn list_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let item = gtk::ListBoxRow::new();
    item.set_activatable(false);
    item.set_focusable(false);
    item.set_child(Some(child));
    item
}

/// A run of sections of per-device settings of one input type, with the selector for the device
/// their rows are for.
struct DeviceGroup {
    kind: InputType,
    selector: DeviceSelector,
    /// Shown in place of the rows while there is nothing to set.
    hint: gtk::Label,
    /// The heading and the list of each section.
    sections: Vec<(gtk::Label, gtk::ListBox)>,
    rows: Vec<SettingRow>,
    /// Whether the sections being built still belong to the group.
    open: bool,
}

impl DeviceGroup {
    /// A group of type `kind` that starts with the section of `heading` and `list`. The selector
    /// becomes the first row of the list.
    fn new(kind: InputType, heading: gtk::Label, list: &gtk::ListBox) -> Self {
        let selector = DeviceSelector::new(kind);
        list.append(&list_row(&selector));
        let hint = gtk::Label::new(Some(&none_connected(kind)));
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        hint.add_css_class("st-section-hint");
        hint.add_css_class(style::DIM);
        hint.set_visible(false);
        Self {
            kind,
            selector,
            hint,
            sections: vec![(heading, list.clone())],
            rows: Vec::new(),
            open: true,
        }
    }

    /// Points the rows at the device picked, and keeps the device list and what is shown current
    /// with `model`.
    fn follow(self, model: &Model) {
        let rows: Vec<glib::WeakRef<SettingRow>> =
            self.rows.iter().map(ObjectExt::downgrade).collect();
        self.selector.connect_device_changed(glib::clone!(
            #[strong]
            rows,
            move |_, device| {
                for row in rows.iter().filter_map(glib::WeakRef::upgrade) {
                    row.set_device(device);
                }
            }
        ));
        let sections: Vec<(glib::WeakRef<gtk::Label>, glib::WeakRef<gtk::ListBox>)> = self
            .sections
            .iter()
            .map(|(heading, list)| (heading.downgrade(), list.downgrade()))
            .collect();
        let kind = self.kind;
        let update = glib::clone!(
            #[weak(rename_to = selector)]
            self.selector,
            #[weak(rename_to = hint)]
            self.hint,
            move |model: &Model| {
                let Some(devices) = model.devices() else {
                    return;
                };
                selector.update(&devices);
                let connected = devices.of_type(kind).next().is_some();
                let stored = rows
                    .iter()
                    .filter_map(glib::WeakRef::upgrade)
                    .filter_map(|row| row.setting())
                    .filter_map(|setting| model.view(setting, None))
                    .any(|view| matches!(view.source, Source::Stored { .. }));
                let shown = !model.devices_known() || connected || stored;
                for (index, (heading, list)) in sections.iter().enumerate() {
                    if let Some(heading) = heading.upgrade() {
                        heading.set_visible(shown || index == 0);
                    }
                    if let Some(list) = list.upgrade() {
                        list.set_visible(shown);
                    }
                }
                hint.set_visible(!shown);
            }
        );
        update(model);
        model.connect_changed(update);
    }
}
