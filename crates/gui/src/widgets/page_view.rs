// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A page in the content pane: header and body, scrollable, at a readable width.
//!
//! A page built from the registry has a section per registry section. The sections of per-device
//! settings start with a [`DeviceSelector`]: one for each run of sections of the same input type,
//! which picks the device all their rows are for. While no device of the type is connected and
//! nothing is stored for the type, the run shows its first heading and a hint instead of its rows.
//!
//! The Keyboard page adds its own widgets below some rows (the layouts, the XKB options), see
//! [`keyboard`].
//!
//! The settings of a directive with several values form a [`GroupRow`], collapsed, with their
//! rows below it.
//!
//! A deep link points to a row: [`PageView::point_to`] expands its group, scrolls it into view,
//! gives its control the keyboard focus and tints it with the accent for a moment.

use std::collections::BTreeSet;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::command::input::InputType;
use sway_things_core::registry::{ConfigFile, Msg, Page, Registry, Scope, Setting, SettingGroup};
use sway_things_core::ui_state;
use sway_things_core::view::Source;

use super::device_selector::none_connected;
use super::keyboard;
use super::setting_row::tilde;
use super::{Clamp, DeviceSelector, GroupRow, PageHeader, Placeholder, SettingRow, TestArea};
use crate::i18n::tr;
use crate::model::Model;
use crate::style;

/// The widest a page's content gets, in pixels.
pub const MAXIMUM_WIDTH: i32 = 760;

/// How long a row a deep link points to stays tinted before it fades back.
const HIGHLIGHT: Duration = Duration::from_millis(1500);

mod imp {
    use std::cell::{OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct PageView {
        pub scroller: gtk::ScrolledWindow,
        pub content: gtk::Box,
        pub header: OnceCell<super::PageHeader>,
        pub reset: OnceCell<gtk::Button>,
        /// The setting rows of a page built from the registry.
        pub rows: RefCell<Vec<super::SettingRow>>,
        /// The rows of the groups of settings on the page.
        pub groups: RefCell<Vec<super::GroupRow>>,
        /// Waits for the row a deep link points to to be laid out.
        pub pointing: RefCell<Option<gtk::TickCallbackId>>,
        /// The tinted row and the timeout that takes the tint away.
        pub highlight: RefCell<Option<(gtk::Widget, glib::SourceId)>>,
        /// The collapsible sections of the page.
        pub(super) sections: RefCell<Vec<super::Section>>,
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
    /// The rows of a group of settings go below a [`GroupRow`]. Per-device settings get a
    /// [`DeviceSelector`] per input type. `None` if the page has no settings with a row.
    pub fn for_settings(model: &Model, page: Page) -> Option<Self> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let mut sections = Vec::new();
        if page == Page::About {
            sections.push(generated_section(model, &body));
        }
        let mut first = page != Page::About;
        let mut section: Option<(Msg, gtk::ListBox)> = None;
        let mut groups: Vec<DeviceGroup> = Vec::new();
        let mut files = BTreeSet::new();
        let mut rows = Vec::new();
        let mut grouping = Grouping::default();
        for setting in Registry::builtin()
            .iter()
            .filter(|setting| setting.page() == page)
        {
            let row = match keyboard::choices(setting) {
                Some(choices) => SettingRow::with_choices(model, setting, None, &choices),
                None => SettingRow::new(model, setting, None),
            };
            let Some(row) = row else {
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
                    let new = Section::new(
                        model,
                        page,
                        setting.section().msgid(),
                        tr(setting.section()),
                        std::mem::take(&mut first),
                    );
                    let heading = new.header.clone().upcast::<gtk::Widget>();
                    let list = section_list(setting.section());
                    body.append(&new.header);
                    body.append(&new.body);
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
                            new.body.append(&group.hint);
                            groups.push(group);
                        }
                        (None, None) => {
                            if let Some(last) = groups.last_mut() {
                                last.open = false;
                            }
                        }
                    }
                    new.body.append(&list);
                    sections.push(new);
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
            grouping.add(model, setting, &row, &list, &mut groups);
            for widget in keyboard::below(model, &row) {
                list.append(&list_row(&widget));
            }
            rows.push(row);
        }
        grouping.close(model, &mut groups);
        section.as_ref()?;
        for group in groups {
            group.follow(model);
        }
        if page == Page::MouseTouchpad {
            sections.push(test_area_section(model, &body));
        }
        if let Some(footer) = footer(model, files) {
            body.append(&footer);
        }
        let view = Self::new(tr(page.title()), tr(page.description()), &body);
        view.imp().rows.replace(rows);
        view.imp().groups.replace(grouping.rows);
        view.imp().sections.replace(sections);
        view.add_page_reset(model, page);
        Some(view)
    }

    /// Adds "Reset page…" to the header: after asking, it removes every stored value of `page`
    /// that can't lock the user out. It can only be pressed while there is such a value.
    fn add_page_reset(&self, model: &Model, page: Page) {
        let button = gtk::Button::with_label("Reset page…");
        button.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |button| confirm_page_reset(button, &model, page)
        ));
        let follow = glib::clone!(
            #[weak]
            button,
            move |model: &Model| {
                let writable = model.status().is_some_and(|status| status.is_active());
                button.set_sensitive(writable && model.page_reset(page).0 > 0);
            }
        );
        follow(model);
        model.connect_changed(follow);
        if let Some(header) = self.header() {
            header.set_end(&button);
        }
        let _ = self.imp().reset.set(button);
    }

    /// The "Reset page…" button, on a page built from the registry.
    pub fn reset_button(&self) -> Option<&gtk::Button> {
        self.imp().reset.get()
    }

    /// The header of the page.
    pub fn header(&self) -> Option<&PageHeader> {
        self.imp().header.get()
    }

    /// The sections of the page: each one's id, header button and whether it is expanded.
    pub fn sections(&self) -> Vec<(String, gtk::Button, bool)> {
        self.imp()
            .sections
            .borrow()
            .iter()
            .map(|section| {
                (
                    section.id.clone(),
                    section.header.clone(),
                    section.body.get_visible(),
                )
            })
            .collect()
    }

    /// The groups of settings on the page.
    pub fn groups(&self) -> Vec<GroupRow> {
        self.imp().groups.borrow().clone()
    }

    /// The group of settings with `id`, if the page has it.
    pub fn group(&self, id: &str) -> Option<GroupRow> {
        self.imp()
            .groups
            .borrow()
            .iter()
            .find(|group| group.group().is_some_and(|group| group.id() == id))
            .cloned()
    }

    /// The group `row` is a member of.
    fn group_of(&self, row: &SettingRow) -> Option<GroupRow> {
        self.imp()
            .groups
            .borrow()
            .iter()
            .find(|group| group.members().contains(row))
            .cloned()
    }

    /// The row of the setting with `key`, if the page shows one: the rows of an input type
    /// without devices are hidden. A member of a collapsed group counts as shown, since pointing
    /// to it expands the group.
    pub fn row(&self, key: &str) -> Option<SettingRow> {
        let rows = self.imp().rows.borrow();
        let row = rows
            .iter()
            .find(|row| row.setting().is_some_and(|setting| setting.key() == key))?;
        let start = match self.group_of(row) {
            Some(group) => group.item().clone().upcast::<gtk::Widget>(),
            None => row.clone().upcast(),
        };
        // A collapsed section counts as shown: pointing to the row opens it.
        let sections = self.imp().sections.borrow();
        let collapsible = |widget: &gtk::Widget| {
            sections
                .iter()
                .any(|section| section.body.upcast_ref::<gtk::Widget>() == widget)
        };
        let mut widget = Some(start);
        while let Some(shown) = widget {
            // Each widget's own `visible`: `is_visible` would also ask its parents.
            if !shown.get_visible() && !collapsible(&shown) {
                return None;
            }
            if shown == *self.upcast_ref::<gtk::Widget>() {
                break;
            }
            widget = shown.parent();
        }
        Some(row.clone())
    }

    /// Points to `row`, as a deep link does: expands the group it is a member of, tints it with
    /// the accent, and once it is laid out, scrolls it into view and gives its control the
    /// keyboard focus. The tint fades after a moment, or goes at once without animations.
    pub fn point_to(&self, row: &SettingRow) {
        let imp = self.imp();
        // A collapsed section opens for the moment, which isn't stored as the user's choice.
        if let Some(section) = imp
            .sections
            .borrow()
            .iter()
            .find(|section| row.is_ancestor(&section.body))
        {
            section.show(true);
        }
        if let Some(group) = self.group_of(row) {
            group.set_expanded(true);
        }
        let tinted = row.parent().unwrap_or_else(|| row.clone().upcast());
        if let Some((old, timeout)) = imp.highlight.take() {
            old.remove_css_class(style::HIGHLIGHT);
            timeout.remove();
        }
        tinted.add_css_class(style::HIGHLIGHT);
        let timeout = glib::timeout_add_local_once(
            HIGHLIGHT,
            glib::clone!(
                #[weak(rename_to = view)]
                self,
                move || {
                    if let Some((tinted, _)) = view.imp().highlight.take() {
                        tinted.remove_css_class(style::HIGHLIGHT);
                    }
                }
            ),
        );
        imp.highlight.replace(Some((tinted.clone(), timeout)));

        if let Some(old) = imp.pointing.take() {
            old.remove();
        }
        // A page that was just built, or a window that isn't shown yet, has no layout to scroll
        // in: wait for the frame after the row got its place.
        let tick = row.add_tick_callback(glib::clone!(
            #[weak(rename_to = view)]
            self,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move |row, _| {
                if !row.is_mapped() || row.height() == 0 {
                    return glib::ControlFlow::Continue;
                }
                view.imp().pointing.take();
                if let Some(viewport) = view.imp().scroller.child().and_downcast::<gtk::Viewport>()
                {
                    viewport.scroll_to(&tinted, None);
                }
                if let Some(control) = row.controls().first() {
                    if !control.grab_focus() {
                        control.child_focus(gtk::DirectionType::TabForward);
                    }
                }
                glib::ControlFlow::Break
            }
        ));
        imp.pointing.replace(Some(tick));
    }
}

/// The paths of the generated `files` a page writes to, for its footer. `None` without files or
/// without the locations of the generated files.
fn footer(model: &Model, files: BTreeSet<ConfigFile>) -> Option<gtk::Box> {
    let locations = model.locations()?;
    if files.is_empty() {
        return None;
    }
    let footer = gtk::Box::new(gtk::Orientation::Vertical, 2);
    footer.add_css_class("st-page-footer");
    for file in files {
        let path = tilde(&locations.path(file));
        let label = gtk::Label::new(Some(&path));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_selectable(true);
        // Selectable with the mouse, but not a stop for the keyboard, which would select it all.
        label.set_focusable(false);
        label.add_css_class("monospace");
        label.add_css_class(style::DIM);
        label.update_property(&[gtk::accessible::Property::Label(&format!(
            "Written to {path}"
        ))]);
        let show = gtk::Button::with_label("Show");
        show.add_css_class("flat");
        show.set_valign(gtk::Align::Center);
        show.set_tooltip_text(Some(&format!(
            "Show what Sway Things writes to {}",
            file.file_name()
        )));
        show.update_property(&[gtk::accessible::Property::Label(&format!(
            "Show {}",
            file.file_name()
        ))]);
        show.connect_clicked(move |button| {
            if let Some(window) = button.root().and_downcast::<crate::window::Window>() {
                window.show_generated(Some(file));
            }
        });
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.append(&label);
        line.append(&show);
        footer.append(&line);
    }
    Some(footer)
}

/// The "Test area" section of Mouse & Touchpad, to try the pointer and scroll settings.
fn test_area_section(model: &Model, body: &gtk::Box) -> Section {
    let section = Section::new(model, Page::MouseTouchpad, "Test area", "Test area", false);
    section.body.append(&TestArea::new());
    body.append(&section.header);
    body.append(&section.body);
    section
}

/// The "Generated config" section of About & Maintenance: a row that opens the window with the
/// files Sway Things writes.
fn generated_section(model: &Model, body: &gtk::Box) -> Section {
    let section = Section::new(
        model,
        Page::About,
        "Generated config",
        "Generated config",
        true,
    );
    body.append(&section.header);
    body.append(&section.body);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("st-section");
    list.update_property(&[gtk::accessible::Property::Label("Generated config")]);
    let title = gtk::Label::new(Some("Generated config"));
    title.set_xalign(0.0);
    let subtitle = gtk::Label::new(Some(
        "What Sway Things writes for your sway config to include, and which files were changed \
         outside it",
    ));
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.add_css_class(style::DIM);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(&title);
    text.append(&subtitle);
    let show = gtk::Button::with_label("Show\u{2026}");
    show.set_valign(gtk::Align::Center);
    show.update_property(&[gtk::accessible::Property::Label("Show generated config")]);
    show.connect_clicked(|button| {
        if let Some(window) = button.root().and_downcast::<crate::window::Window>() {
            window.show_generated(None);
        }
    });
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.append(&text);
    row.append(&show);
    list.append(&list_row(&row));
    section.body.append(&list);
    section
}

/// Puts the rows of a page into their lists, the members of a group below a [`GroupRow`].
#[derive(Default)]
struct Grouping {
    /// The group being built, its members and their list: the members of a group are next to
    /// each other, in one section.
    open: Option<(SettingGroup, Vec<SettingRow>, gtk::ListBox)>,
    /// The rows of the groups built.
    rows: Vec<GroupRow>,
}

impl Grouping {
    /// Adds the `row` of `setting` to `list`, or to its group.
    fn add(
        &mut self,
        model: &Model,
        setting: &Setting,
        row: &SettingRow,
        list: &gtk::ListBox,
        devices: &mut [DeviceGroup],
    ) {
        match (setting.group(), &mut self.open) {
            (Some(group), Some((open, members, _))) if *open == group => {
                members.push(row.clone());
            }
            (Some(group), _) => {
                self.close(model, devices);
                self.open = Some((group, vec![row.clone()], list.clone()));
            }
            (None, _) => {
                self.close(model, devices);
                list.append(&list_row(row));
            }
        }
    }

    /// Puts the group being built into its list. Its row follows the device picked for its
    /// members.
    fn close(&mut self, model: &Model, devices: &mut [DeviceGroup]) {
        let Some((group, members, list)) = self.open.take() else {
            return;
        };
        let row = GroupRow::new(model, group, members);
        for item in row.items() {
            list.append(&item);
        }
        if let Some(device_group) = devices.last_mut().filter(|device_group| {
            row.members()
                .iter()
                .any(|member| device_group.rows.contains(member))
        }) {
            device_group.group_rows.push(row.clone());
        }
        self.rows.push(row);
    }
}

/// Asks whether to reset `page`, naming what stays, and resets it on yes.
fn confirm_page_reset(button: &gtk::Button, model: &Model, page: Page) {
    let (removed, kept) = model.page_reset(page);
    if removed == 0 {
        return;
    }
    let values = if removed == 1 { "value" } else { "values" };
    let names: Vec<String> = kept.iter().map(|setting| setting_name(setting)).collect();
    let names = names.join(", ");
    let kept_note = match kept.len() {
        0 => String::new(),
        1 => format!("\n\n{names} can lock you out, so it keeps its value; reset it from its row."),
        _ => format!(
            "\n\n{names} can lock you out, so they keep their values; reset them from their rows."
        ),
    };
    let detail = format!(
        "Sway Things no longer sets them: your config or sway\u{2019}s defaults decide \
         again.{kept_note}"
    );
    let dialog = gtk::AlertDialog::builder()
        .message(format!(
            "Reset {removed} stored {values} of {}?",
            tr(page.title())
        ))
        .detail(detail)
        .buttons(["Cancel", "Reset"])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    let window = button.root().and_downcast::<gtk::Window>();
    dialog.choose(
        window.as_ref(),
        gtk::gio::Cancellable::NONE,
        glib::clone!(
            #[weak]
            model,
            move |answer| {
                if answer == Ok(1) {
                    model.reset_page(page);
                }
            }
        ),
    );
}

/// "Section › Title", or the title alone where they are the same.
fn setting_name(setting: &Setting) -> String {
    let (section, title) = (tr(setting.section()), tr(setting.title()));
    if section == title {
        title.to_owned()
    } else {
        format!("{section} › {title}")
    }
}

/// A section of a page: a header with its title and a chevron, which collapses and expands the
/// body below it (a click, `Enter` or `Space`). The model remembers the choice.
#[derive(Debug, Clone)]
pub(crate) struct Section {
    /// The id of the section (from its English title), as the model stores it.
    id: String,
    header: gtk::Button,
    chevron: gtk::Image,
    body: gtk::Box,
}

impl Section {
    /// The section titled `title` (the English source title, for its id) and `shown` (the title
    /// as shown) on `page`, collapsed if the model says so. The first one on a page has no space
    /// above it.
    fn new(model: &Model, page: Page, title: &str, shown: &str, first: bool) -> Self {
        let label = gtk::Label::builder()
            .label(shown.to_uppercase())
            .xalign(0.0)
            .hexpand(true)
            .build();
        label.add_css_class("st-section-heading");
        label.add_css_class(style::DIM);
        let chevron = gtk::Image::from_icon_name("pan-down-symbolic");
        chevron.add_css_class(style::DIM);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        content.append(&label);
        content.append(&chevron);
        let header = gtk::Button::builder()
            .child(&content)
            .css_classes(["flat", "st-section-header"])
            .build();
        if first {
            header.add_css_class("st-first");
        }
        header.update_property(&[gtk::accessible::Property::Label(shown)]);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let section = Self {
            id: ui_state::section_id(title),
            header,
            chevron,
            body,
        };
        section.show(!model.is_collapsed(page, &section.id));
        let toggled = section.clone();
        section.header.connect_clicked(glib::clone!(
            #[weak]
            model,
            move |_| {
                let expanded = !toggled.body.get_visible();
                toggled.show(expanded);
                model.set_collapsed(page, &toggled.id, !expanded);
            }
        ));
        section
    }

    /// Shows the body if `expanded`, otherwise only the header.
    fn show(&self, expanded: bool) {
        self.body.set_visible(expanded);
        self.chevron.set_icon_name(Some(if expanded {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        }));
        self.header
            .update_state(&[gtk::accessible::State::Expanded(Some(expanded))]);
    }
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
    /// The header and the list of each section.
    sections: Vec<(gtk::Widget, gtk::ListBox)>,
    rows: Vec<SettingRow>,
    /// The groups of settings among the rows, whose summary follows the device too.
    group_rows: Vec<GroupRow>,
    /// Whether the sections being built still belong to the group.
    open: bool,
}

impl DeviceGroup {
    /// A group of type `kind` that starts with the section of `heading` and `list`. The selector
    /// becomes the first row of the list.
    fn new(kind: InputType, heading: gtk::Widget, list: &gtk::ListBox) -> Self {
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
            group_rows: Vec::new(),
            open: true,
        }
    }

    /// Points the rows at the device picked, and keeps the device list and what is shown current
    /// with `model`.
    fn follow(self, model: &Model) {
        let rows: Vec<glib::WeakRef<SettingRow>> =
            self.rows.iter().map(ObjectExt::downgrade).collect();
        let group_rows: Vec<glib::WeakRef<GroupRow>> =
            self.group_rows.iter().map(ObjectExt::downgrade).collect();
        self.selector.connect_device_changed(glib::clone!(
            #[strong]
            rows,
            move |_, device| {
                for row in rows.iter().filter_map(glib::WeakRef::upgrade) {
                    row.set_device(device);
                }
                for group in group_rows.iter().filter_map(glib::WeakRef::upgrade) {
                    group.refresh();
                }
            }
        ));
        let sections: Vec<(glib::WeakRef<gtk::Widget>, glib::WeakRef<gtk::ListBox>)> = self
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
