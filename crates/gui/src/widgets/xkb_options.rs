// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The XKB options of the Keyboard page, below the row of the options setting: the common ones
//! as drop-downs and a switch ([`common`]), and [`OptionBrowser`], every option of the XKB rules
//! by group, with a search.
//!
//! They all change the one list of options, through the row of the options setting
//! ([`SettingRow::change`]), so each change goes through the safety net and a refusal shows at
//! that row. Options they don't know stay in the list as they are.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sway_things_core::registry::Value;

use super::SettingRow;
use super::keyboard::{follow, plain_row};
use crate::model::Model;
use crate::xkb;

/// What the Caps Lock key can do, and the option for it.
const CAPS_LOCK: &[(&str, Option<&str>)] = &[
    ("Caps Lock", None),
    ("Ctrl", Some("ctrl:nocaps")),
    ("Escape", Some("caps:escape")),
    ("Swap with Escape", Some("caps:swapescape")),
    ("Off", Some("caps:none")),
];

/// The option that swaps the left Ctrl and Alt keys.
const SWAP_CTRL_ALT: &str = "ctrl:swap_lalt_lctl";

/// The options of the row of the options setting.
fn options(row: &SettingRow) -> Vec<String> {
    match row.value() {
        Some(Value::List(options)) => options,
        _ => Vec::new(),
    }
}

/// Changes the options of `row` to `new`, if that is a change.
fn set_options(row: &SettingRow, new: Vec<String>) {
    if new != options(row) {
        row.change(Value::List(new));
    }
}

/// The rows for the common options: what Caps Lock does, swapping Ctrl and Alt, the Compose key
/// and the shortcut that switches layouts.
pub fn common(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let caps: Vec<(String, Option<String>)> = CAPS_LOCK
        .iter()
        .map(|(label, option)| ((*label).to_owned(), option.map(str::to_owned)))
        .collect();
    let known_caps: Vec<String> = caps
        .iter()
        .filter_map(|(_, option)| option.clone())
        .collect();
    vec![
        choice_row(
            model,
            row,
            "Caps Lock key",
            "What the Caps Lock key does",
            caps,
            move |option| known_caps.iter().any(|known| known == option),
        ),
        swap_row(model, row),
        choice_row(
            model,
            row,
            "Compose key",
            "Starts a sequence that types a special character, e.g. Compose, ', e for é",
            group_choices("compose", "Off"),
            |option| option.starts_with("compose:"),
        ),
        choice_row(
            model,
            row,
            "Switch layouts with",
            "The keys that switch to the next layout",
            group_choices("grp", "None"),
            |option| option.starts_with("grp:"),
        ),
    ]
}

/// The choices of the XKB option group `group`: `none` first, then its options.
fn group_choices(group: &str, none: &str) -> Vec<(String, Option<String>)> {
    std::iter::once((none.to_owned(), None))
        .chain(
            xkb::rules()
                .options_of(group)
                .iter()
                .filter(|option| !option.extra)
                .map(|option| (option.label().to_owned(), Some(option.name.clone()))),
        )
        .collect()
}

/// A row with a drop-down that picks one option of those `owns` (or none): the others stay.
fn choice_row(
    model: &Model,
    row: &SettingRow,
    title: &str,
    subtitle: &str,
    choices: Vec<(String, Option<String>)>,
    owns: impl Fn(&str) -> bool + 'static,
) -> gtk::Widget {
    let labels: Vec<&str> = choices.iter().map(|(label, _)| label.as_str()).collect();
    let labels = gtk::StringList::new(&labels);
    let dropdown = gtk::DropDown::new(
        Some(labels.clone()),
        Some(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            gtk::Expression::NONE,
            "string",
        )),
    );
    dropdown.set_enable_search(true);
    dropdown.set_search_match_mode(gtk::StringFilterMatchMode::Substring);
    dropdown.set_valign(gtk::Align::Center);
    dropdown.update_property(&[gtk::accessible::Property::Label(title)]);
    let options_of: Rc<RefCell<Vec<Option<String>>>> = Rc::new(RefCell::new(
        choices.into_iter().map(|(_, option)| option).collect(),
    ));
    let owns = Rc::new(owns);
    let updating = Rc::new(Cell::new(false));

    dropdown.connect_selected_notify(glib::clone!(
        #[weak]
        row,
        #[strong]
        options_of,
        #[strong]
        owns,
        #[strong]
        updating,
        move |dropdown| {
            if updating.get() {
                return;
            }
            let Some(chosen) = usize::try_from(dropdown.selected())
                .ok()
                .and_then(|index| options_of.borrow().get(index).cloned())
            else {
                return;
            };
            let mut new: Vec<String> = options(&row)
                .into_iter()
                .filter(|option| !owns(option))
                .collect();
            new.extend(chosen);
            set_options(&row, new);
        }
    ));

    let refresh = glib::clone!(
        #[weak]
        row,
        #[weak]
        dropdown,
        move || {
            let current = options(&row).into_iter().find(|option| owns(option));
            updating.set(true);
            let index = {
                let known = options_of
                    .borrow()
                    .iter()
                    .position(|option| *option == current);
                known.unwrap_or_else(|| {
                    // An option the rules don't list: shown by its name.
                    let name = current.clone().unwrap_or_default();
                    labels.append(&name);
                    options_of.borrow_mut().push(current.clone());
                    options_of.borrow().len() - 1
                })
            };
            if let Ok(index) = u32::try_from(index) {
                dropdown.set_selected(index);
            }
            updating.set(false);
            lock(&[dropdown.upcast_ref()], row.locked().as_deref());
        }
    );
    follow(model, row, refresh);
    plain_row(title, Some(subtitle), &dropdown).upcast()
}

/// The row with the switch that swaps the left Ctrl and Alt keys.
fn swap_row(model: &Model, row: &SettingRow) -> gtk::Widget {
    let title = "Swap left Ctrl and Alt";
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.update_property(&[gtk::accessible::Property::Label(title)]);
    let updating = Rc::new(Cell::new(false));
    switch.connect_state_set(glib::clone!(
        #[weak]
        row,
        #[strong]
        updating,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, on| {
            if !updating.get() {
                let mut new: Vec<String> = options(&row)
                    .into_iter()
                    .filter(|option| option != SWAP_CTRL_ALT)
                    .collect();
                if on {
                    new.push(SWAP_CTRL_ALT.to_owned());
                }
                set_options(&row, new);
            }
            glib::Propagation::Proceed
        }
    ));
    let refresh = glib::clone!(
        #[weak]
        row,
        #[weak]
        switch,
        move || {
            let on = options(&row).iter().any(|option| option == SWAP_CTRL_ALT);
            updating.set(true);
            switch.set_active(on);
            switch.set_state(on);
            updating.set(false);
            lock(&[switch.upcast_ref()], row.locked().as_deref());
        }
    );
    follow(model, row, refresh);
    plain_row(
        title,
        Some("For keyboards with Ctrl where others have Alt"),
        &switch,
    )
    .upcast()
}

/// Makes `widgets` insensitive with `reason` as tooltip, or sensitive without one.
fn lock(widgets: &[&gtk::Widget], reason: Option<&str>) {
    for widget in widgets {
        widget.set_sensitive(reason.is_none());
        widget.set_tooltip_text(reason);
    }
}

/// An option of the browser: its check button, its name and its group.
#[derive(Debug)]
struct OptionCheck {
    check: gtk::CheckButton,
    name: String,
    description: String,
    extra: bool,
    group: usize,
}

/// A group of the browser.
#[derive(Debug)]
struct Group {
    expander: gtk::Expander,
    label: String,
    /// The "None" choice of a group whose options exclude each other.
    none: Option<gtk::CheckButton>,
}

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    use crate::model::Model;
    use crate::widgets::SettingRow;

    #[derive(Debug, Default)]
    pub struct OptionBrowser {
        pub(super) row: OnceCell<SettingRow>,
        pub(super) model: OnceCell<Model>,
        pub(super) expander: gtk::Expander,
        pub(super) search: gtk::SearchEntry,
        pub(super) groups_box: gtk::Box,
        pub(super) groups: RefCell<Vec<super::Group>>,
        pub(super) checks: RefCell<Vec<super::OptionCheck>>,
        /// Set while the check buttons show the options, so that isn't sent as a change.
        pub(super) updating: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OptionBrowser {
        const NAME: &'static str = "StOptionBrowser";
        type Type = super::OptionBrowser;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-option-browser");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for OptionBrowser {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for OptionBrowser {}
}

glib::wrapper! {
    /// Every XKB option of the rules, by group, with a search.
    pub struct OptionBrowser(ObjectSubclass<imp::OptionBrowser>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl OptionBrowser {
    /// The browser for the row of the options setting, `row`. Its groups are built when it is
    /// first opened.
    pub fn new(model: &Model, row: &SettingRow) -> Self {
        let browser: Self = glib::Object::new();
        let imp = browser.imp();
        let _ = imp.row.set(row.clone());
        let _ = imp.model.set(model.clone());

        imp.search.set_placeholder_text(Some("Search options"));
        imp.search
            .update_property(&[gtk::accessible::Property::Label("Search options")]);
        imp.search.connect_search_changed(glib::clone!(
            #[weak]
            browser,
            move |_| browser.filter()
        ));
        imp.groups_box.set_orientation(gtk::Orientation::Vertical);
        imp.groups_box.set_spacing(4);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_top(10);
        content.append(&imp.search);
        content.append(&imp.groups_box);

        let title = gtk::Label::new(Some("All XKB options"));
        title.add_css_class("st-row-title");
        imp.expander.set_label_widget(Some(&title));
        imp.expander.set_child(Some(&content));
        imp.expander.connect_expanded_notify(glib::clone!(
            #[weak]
            browser,
            move |expander| {
                if expander.is_expanded() {
                    browser.build_groups();
                }
            }
        ));
        imp.expander.set_parent(&browser);

        follow(
            model,
            row,
            glib::clone!(
                #[weak]
                browser,
                move || browser.refresh()
            ),
        );
        browser
    }

    /// The expander that opens the browser.
    pub fn expander(&self) -> &gtk::Expander {
        &self.imp().expander
    }

    /// The check button of the option `name`, once the browser was opened.
    pub fn check(&self, name: &str) -> Option<gtk::CheckButton> {
        self.imp()
            .checks
            .borrow()
            .iter()
            .find(|check| check.name == name)
            .map(|check| check.check.clone())
    }

    /// Builds the groups of the XKB rules, once.
    fn build_groups(&self) {
        let imp = self.imp();
        if !imp.groups.borrow().is_empty() {
            return;
        }
        let mut groups = Vec::new();
        let mut checks = Vec::new();
        for (index, group) in xkb::rules().option_groups.iter().enumerate() {
            let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
            list.set_margin_start(18);
            list.set_margin_bottom(6);
            let none = (!group.multiple).then(|| {
                let none = gtk::CheckButton::with_label("None");
                none.connect_toggled(glib::clone!(
                    #[weak(rename_to = browser)]
                    self,
                    move |none| {
                        if none.is_active() {
                            browser.choose(index, None);
                        }
                    }
                ));
                list.append(&none);
                none
            });
            for option in &group.options {
                let check = gtk::CheckButton::with_label(option.label());
                check.set_tooltip_text(Some(&option.name));
                if let Some(none) = &none {
                    check.set_group(Some(none));
                }
                let name = option.name.clone();
                check.connect_toggled(glib::clone!(
                    #[weak(rename_to = browser)]
                    self,
                    move |check| browser.toggled(index, &name, check.is_active())
                ));
                list.append(&check);
                checks.push(OptionCheck {
                    check,
                    name: option.name.clone(),
                    description: option.label().to_lowercase(),
                    extra: option.extra,
                    group: index,
                });
            }
            let expander = gtk::Expander::new(Some(group.item.label()));
            expander.set_child(Some(&list));
            imp.groups_box.append(&expander);
            groups.push(Group {
                expander,
                label: group.item.label().to_lowercase(),
                none,
            });
        }
        imp.groups.replace(groups);
        imp.checks.replace(checks);
        self.refresh();
        self.filter();
    }

    /// An option was turned on or off by the user.
    fn toggled(&self, group: usize, name: &str, on: bool) {
        let imp = self.imp();
        if imp.updating.get() {
            return;
        }
        let exclusive = imp
            .groups
            .borrow()
            .get(group)
            .is_some_and(|group| group.none.is_some());
        if exclusive {
            // Turning one on turns the one before off; that toggle sends nothing.
            if on {
                self.choose(group, Some(name));
            }
            return;
        }
        let Some(row) = imp.row.get() else { return };
        let mut new: Vec<String> = options(row)
            .into_iter()
            .filter(|option| option != name)
            .collect();
        if on {
            new.push(name.to_owned());
        }
        set_options(row, new);
    }

    /// Picks `name` (or with `None`, no option) in the group at `group`, whose options exclude
    /// each other.
    fn choose(&self, group: usize, name: Option<&str>) {
        let imp = self.imp();
        if imp.updating.get() {
            return;
        }
        let Some(row) = imp.row.get() else { return };
        let of_group: Vec<String> = imp
            .checks
            .borrow()
            .iter()
            .filter(|check| check.group == group)
            .map(|check| check.name.clone())
            .collect();
        let mut new: Vec<String> = options(row)
            .into_iter()
            .filter(|option| !of_group.contains(option))
            .collect();
        new.extend(name.map(str::to_owned));
        set_options(row, new);
    }

    /// Shows the options in effect and whether they can be changed.
    fn refresh(&self) {
        let imp = self.imp();
        let Some(row) = imp.row.get() else { return };
        let current = options(row);
        imp.updating.set(true);
        let checks = imp.checks.borrow();
        for check in checks.iter() {
            check.check.set_active(current.contains(&check.name));
        }
        for (index, group) in imp.groups.borrow().iter().enumerate() {
            if let Some(none) = &group.none {
                let any = checks
                    .iter()
                    .any(|check| check.group == index && current.contains(&check.name));
                if !any {
                    none.set_active(true);
                }
            }
        }
        imp.updating.set(false);
        let reason = row.locked();
        imp.groups_box.set_sensitive(reason.is_none());
        imp.groups_box.set_tooltip_text(reason.as_deref());
    }

    /// Shows the groups and options that match the search; without one, all but the exotic
    /// options. While searching, the groups with a match are open.
    fn filter(&self) {
        let imp = self.imp();
        let query = imp.search.text().trim().to_lowercase();
        let checks = imp.checks.borrow();
        for (index, group) in imp.groups.borrow().iter().enumerate() {
            let group_matches = !query.is_empty() && group.label.contains(&query);
            let mut any = false;
            for check in checks.iter().filter(|check| check.group == index) {
                let shown = if query.is_empty() {
                    !check.extra
                } else {
                    group_matches
                        || check.name.to_lowercase().contains(&query)
                        || check.description.contains(&query)
                };
                check.check.set_visible(shown);
                any |= shown;
            }
            group.expander.set_visible(any);
            if !query.is_empty() && any {
                group.expander.set_expanded(true);
            }
        }
    }
}
