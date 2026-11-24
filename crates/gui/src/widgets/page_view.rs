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
//! [`keyboard`]. The Cursor & Tablet page offers drop-downs for its themes, sizes and screens,
//! see [`cursor`].
//!
//! The settings of a directive with several values form a [`GroupRow`], collapsed, with their
//! rows below it.
//!
//! A deep link points to a row: [`PageView::point_to`] expands its group, scrolls it into view,
//! gives its control the keyboard focus and tints it with the accent for a moment.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::command::input::InputType;
use sway_things_core::registry::{ConfigFile, Msg, Page, Registry, Scope, Setting, SettingGroup};
use sway_things_core::ui_state;

use self::about::{about_section, generated_section};
use self::device_group::DeviceGroup;
use super::apps;
use super::brightness;
use super::displays::{self, Arrangement, OutputPicker};
use super::notifications;
use super::setting_row::tilde;
use super::{
    Clamp, GroupRow, PageHeader, Placeholder, SettingRow, ShortcutList, TestArea, WorkspaceMap,
};
use crate::i18n::{msg, msg_plural, ntr, ntrf, tr, trf};
use crate::model::Model;
use crate::style;
use crate::widgets::list::{list_row, section_list};
use sway_things_core::notifications::Daemon;

/// The widest a page's content gets, in pixels, with its margins.
pub const MAXIMUM_WIDTH: i32 = 760;

/// The smallest and the largest margin on each side of a page, in pixels.
pub(super) const MARGINS: (i32, i32) = (12, 32);

/// How long a row a deep link points to stays tinted before it fades back.
const HIGHLIGHT: Duration = Duration::from_millis(1500);

mod about;
mod device_group;
mod parts;

mod imp {
    use std::cell::{OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct PageView {
        pub scroller: gtk::ScrolledWindow,
        pub content: gtk::Box,
        /// Around the content: its width and margins.
        pub clamp: OnceCell<super::super::Clamp>,
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
        /// On the Displays page, the arrangement and the picker of the output.
        pub(super) displays: RefCell<Option<(super::OutputPicker, super::Arrangement)>>,
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
            let clamp = super::Clamp::with_margins(
                &self.content,
                super::MAXIMUM_WIDTH - 2 * super::MARGINS.1,
                super::MARGINS,
            );
            // Narrower than its content can get, the page scrolls sideways instead of being cut
            // off.
            self.scroller
                .set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
            self.scroller.set_child(Some(&clamp));
            let _ = self.clamp.set(clamp);
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
    /// The width the content of the page needs, with the smallest margins: narrower, the page
    /// scrolls sideways.
    pub fn content_minimum(&self) -> i32 {
        self.imp()
            .clamp
            .get()
            .map_or(0, |clamp| clamp.measure(gtk::Orientation::Horizontal, -1).0)
    }

    /// Whether the content of the page fits in `width`, with the margins it gets there, without
    /// scrolling sideways.
    pub fn fits(&self, width: i32) -> bool {
        self.imp().clamp.get().is_none_or(|clamp| clamp.fits(width))
    }

    /// The narrowest the page can be to give its content `content_width` pixels, with the margins
    /// it gets there.
    pub fn width_for(&self, content_width: i32) -> i32 {
        self.imp()
            .clamp
            .get()
            .map_or(content_width, |clamp| clamp.width_for(content_width))
    }

    /// The content of the page: its header and its body, without the margins.
    pub fn content(&self) -> gtk::Box {
        self.imp().content.clone()
    }

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
        let mut first = true;
        let before = parts::before_rows(model, page, &body, &mut sections, &mut first);
        let mut section: Option<(Msg, gtk::ListBox)> = None;
        let mut groups: Vec<DeviceGroup> = Vec::new();
        let mut files: BTreeSet<ConfigFile> = parts::files(page).into_iter().collect();
        let mut rows = Vec::new();
        let mut grouping = Grouping::default();
        let mut lists: Vec<gtk::ListBox> = Vec::new();
        for setting in Registry::builtin()
            .iter()
            .filter(|setting| setting.page() == page)
        {
            // A setting can have a widget of its page in place of its row (a list of its own),
            // or both; with neither it isn't shown.
            let instead = parts::instead(model, setting);
            let row = parts::row(model, setting);
            if row.is_none() && instead.is_none() {
                continue;
            }
            if let Some(mapping) = setting.sway() {
                files.insert(mapping.file());
            }
            let kind = match setting.scope() {
                Scope::PerDevice(kind) => Some(kind),
                Scope::Global | Scope::PerHost | Scope::PerOutput | Scope::PerCollection => None,
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
                    let list = section_list(tr(setting.section()));
                    body.append(&new.header);
                    body.append(&new.body);
                    join_device_group(&mut groups, kind, heading, &list, &new.body);
                    new.body.append(&list);
                    sections.push(new);
                    section = Some((setting.section(), list.clone()));
                    lists.push(list.clone());
                    list
                }
            };
            if let Some(widget) = instead {
                // Above the section's rows.
                if let Some(body) = list.parent().and_downcast::<gtk::Box>() {
                    body.insert_child_after(&widget, list.prev_sibling().as_ref());
                }
            }
            let Some(row) = row else {
                continue;
            };
            if let Some(group) = groups
                .last_mut()
                .filter(|group| group.open && Some(group.kind) == kind)
            {
                group.rows.push(row.clone());
            }
            grouping.add(model, setting, &row, &list, &mut groups);
            for widget in parts::below(model, &row) {
                list.append(&list_row(&widget));
            }
            rows.push(row);
        }
        grouping.close(model, &mut groups);
        section.as_ref()?;
        // A section of only a widget in place of rows has no list to draw.
        for list in lists.iter().filter(|list| list.first_child().is_none()) {
            list.set_visible(false);
        }
        for group in groups {
            group.follow(model);
        }
        parts::after_rows(model, page, &body, &mut sections, before.daemon.as_ref());
        let displays = before
            .arrangement
            .map(|shown| follow_displays(model, shown, &rows));
        if let Some(footer) = footer(model, files) {
            body.append(&footer);
        }
        let view = Self::new(tr(page.title()), tr(page.description()), &body);
        view.imp().rows.replace(rows);
        view.imp().groups.replace(grouping.rows);
        view.imp().sections.replace(sections);
        view.imp().displays.replace(displays);
        view.add_page_reset(model, page);
        Some(view)
    }

    /// Adds "Reset page…" to the header: after asking, it removes every stored value of `page`
    /// that can't lock the user out. It can only be pressed while there is such a value. Pages
    /// with a body of their own add it themselves.
    pub fn add_page_reset(&self, model: &Model, page: Page) {
        let button = gtk::Button::with_label(tr(msg!("Reset page…")));
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

    /// The arrangement of the displays, on the Displays page.
    pub fn arrangement(&self) -> Option<Arrangement> {
        self.imp()
            .displays
            .borrow()
            .as_ref()
            .map(|(_, arrangement)| arrangement.clone())
    }

    /// The picker of the output the rows are for, on the Displays page.
    pub fn output_picker(&self) -> Option<OutputPicker> {
        self.imp()
            .displays
            .borrow()
            .as_ref()
            .map(|(picker, _)| picker.clone())
    }

    /// The "Reset page…" button, on a page that has one.
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

    /// Makes `rows` the rows of settings the page shows, for a page with a body of its own: search
    /// and deep links find them.
    pub fn set_rows(&self, rows: Vec<SettingRow>) {
        self.imp().rows.replace(rows);
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
        label.update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("Written to {path}"),
            &[("path", &path)],
        ))]);
        let show = gtk::Button::with_label(tr(msg!("Show")));
        show.add_css_class("flat");
        show.set_valign(gtk::Align::Center);
        show.set_tooltip_text(Some(&trf(
            msg!("Show what Sway Things writes to {file}"),
            &[("file", &file.file_name())],
        )));
        show.update_property(&[gtk::accessible::Property::Label(&trf(
            msg!("Show {file}"),
            &[("file", &file.file_name())],
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

/// The "Arrangement" section of Displays, the first on the page if `first`, added to `body` and
/// `sections`. The page starts with it; its rows follow the output picked there.
fn arrangement_section(
    model: &Model,
    body: &gtk::Box,
    first: bool,
    sections: &mut Vec<Section>,
) -> Arrangement {
    let arrangement = Arrangement::new(model);
    let title = msg!("Arrangement");
    let section = Section::new(model, Page::Displays, title.msgid(), tr(title), first);
    section.body.append(arrangement.widget());
    // Ctrl+Z goes back to the arrangement before, wherever the focus is on the page.
    body.add_controller(arrangement.canvas().undo_shortcut());
    body.append(&section.header);
    body.append(&section.body);
    sections.push(section);
    arrangement
}

/// The "Brightness" section at the top of Brightness & Night Light: the backlight, the external
/// monitors and the keyboard light, live.
pub(super) fn brightness_section(model: &Model, body: &gtk::Box, first: bool) -> Section {
    let title = msg!("Brightness");
    let section = Section::new(
        model,
        Page::BrightnessNightLight,
        title.msgid(),
        tr(title),
        first,
    );
    section.body.append(&brightness::panel());
    body.append(&section.header);
    body.append(&section.body);
    section
}

/// Where the Notifications page's panel says which daemon the page is about.
pub(super) type ShowDaemon = Rc<RefCell<Option<Box<dyn Fn(Option<Daemon>)>>>>;

/// The "Do Not Disturb" section at the top of the Notifications page, with the daemon that
/// runs. Returns where the panel says which daemon the page is about.
pub(super) fn dnd_section(
    model: &Model,
    body: &gtk::Box,
    first: bool,
    sections: &mut Vec<Section>,
) -> ShowDaemon {
    let shown: ShowDaemon = Rc::default();
    let title = msg!("Do Not Disturb");
    let section = Section::new(model, Page::Notifications, title.msgid(), tr(title), first);
    let target = Rc::clone(&shown);
    section
        .body
        .append(&notifications::panel(model, move |daemon| {
            if let Some(show) = target.borrow().as_ref() {
                show(daemon);
            }
        }));
    body.append(&section.header);
    body.append(&section.body);
    sections.push(section);
    shown
}

/// Shows the sections of mako's and swaync's settings for the daemon the page is about: the
/// one that runs, else the one chosen to start; all of them while neither is known.
pub(super) fn follow_daemon(sections: &[Section], shown: Option<&ShowDaemon>) {
    let Some(shown) = shown else {
        return;
    };
    let mako = ui_state::section_id(msg!("mako").msgid());
    let swaync = ui_state::section_id("swaync");
    let parts: Vec<(Daemon, gtk::Button, gtk::Box)> = sections
        .iter()
        .filter_map(|section| {
            let daemon = if section.id == mako {
                Daemon::Mako
            } else if section.id.starts_with(&swaync) {
                Daemon::Swaync
            } else {
                return None;
            };
            Some((daemon, section.header.clone(), section.body.clone()))
        })
        .collect();
    shown.replace(Some(Box::new(move |daemon: Option<Daemon>| {
        for (of, header, body) in &parts {
            let visible = match daemon {
                Some(Daemon::Mako | Daemon::Swaync) => daemon == Some(*of),
                _ => true,
            };
            header.set_visible(visible);
            body.set_visible(visible);
        }
    })));
}

/// Lets the Displays page follow its outputs: the picker before the rows.
fn follow_displays(
    model: &Model,
    arrangement: Arrangement,
    rows: &[SettingRow],
) -> (displays::OutputPicker, Arrangement) {
    let picker = displays::follow(model, &arrangement, rows);
    (picker, arrangement)
}

/// The map of the displays with their workspaces, below the rows of the Workspaces page's
/// "Placement" section.
fn add_workspace_map(model: &Model, sections: &[Section]) {
    let placement = msg!("Placement").msgid();
    let id = ui_state::section_id(placement);
    if let Some(section) = sections.iter().find(|section| section.id == id) {
        section.body.append(WorkspaceMap::new(model).widget());
    }
}

/// The note above the default apps while no app reads them, and the list of everything that
/// starts with the session above the row of Apps & Autostart's "Autostart" section.
fn add_apps_parts(model: &Model, sections: &[Section]) {
    let find = |title: Msg| {
        let id = ui_state::section_id(title.msgid());
        sections.iter().find(|section| section.id == id)
    };
    if let Some(section) = find(msg!("Default apps")) {
        section.body.prepend(&apps::desktop_note());
    }
    if let Some(section) = find(msg!("Autostart")) {
        section
            .body
            .prepend(apps::AutostartList::new(model).widget());
    }
}

/// The "Shortcuts" section of Keyboard Shortcuts: every binding, searchable, with the button
/// that adds one, the first on the page if `first`. The page starts with it, above its rows.
fn shortcuts_section(model: &Model, body: &gtk::Box, first: bool) -> Section {
    let title = msg!("Shortcuts");
    let section = Section::new(
        model,
        Page::KeyboardShortcuts,
        title.msgid(),
        tr(title),
        first,
    );
    section.body.append(ShortcutList::new(model).widget());
    body.append(&section.header);
    body.append(&section.body);
    section
}

/// The "Test area" section of Mouse & Touchpad, to try the pointer and scroll settings.
fn test_area_section(model: &Model, body: &gtk::Box) -> Section {
    let title = msg!("Test area");
    let section = Section::new(model, Page::MouseTouchpad, title.msgid(), tr(title), false);
    section.body.append(&TestArea::new());
    body.append(&section.header);
    body.append(&section.body);
    section
}

/// Adds a new section, with its `heading` and `list`, to the open group of the input type
/// `kind`, or starts a group for it (with its hint in `body`); a section of settings for the
/// whole session closes the open group.
fn join_device_group(
    groups: &mut Vec<DeviceGroup>,
    kind: Option<InputType>,
    heading: gtk::Widget,
    list: &gtk::ListBox,
    body: &gtk::Box,
) {
    let open = groups
        .last_mut()
        .filter(|group| group.open && Some(group.kind) == kind);
    match (open, kind) {
        (Some(group), _) => group.sections.push((heading, list.clone())),
        (None, Some(kind)) => {
            if let Some(last) = groups.last_mut() {
                last.open = false;
            }
            let group = DeviceGroup::new(kind, heading, list);
            body.append(&group.hint);
            groups.push(group);
        }
        (None, None) => {
            if let Some(last) = groups.last_mut() {
                last.open = false;
            }
        }
    }
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
    let names: Vec<String> = kept.iter().map(|setting| setting_name(setting)).collect();
    let names = names.join(", ");
    let mut detail = ntr(
        msg_plural!(
            "Sway Things no longer sets it: your config or sway’s defaults decide again.",
            "Sway Things no longer sets them: your config or sway’s defaults decide again."
        ),
        removed,
    )
    .to_owned();
    if !kept.is_empty() {
        let kept_note = ntrf(
            msg_plural!(
                "{settings} can lock you out, so it keeps its value; reset it from its row.",
                "{settings} can lock you out, so they keep their values; reset them from their \
                 rows."
            ),
            kept.len(),
            &[("settings", &names)],
        );
        detail = format!("{detail}\n\n{kept_note}");
    }
    let dialog = gtk::AlertDialog::builder()
        .message(ntrf(
            msg_plural!(
                "Reset {count} stored value of {page}?",
                "Reset {count} stored values of {page}?"
            ),
            removed,
            &[("count", &removed), ("page", &tr(page.title()))],
        ))
        .detail(detail)
        .buttons([tr(msg!("Cancel")), tr(msg!("Reset"))])
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
