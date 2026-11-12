// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The keyboard layouts of the Keyboard page, below the row of the layouts setting: a row per
//! layout with its variant, reordered by drag and drop or with Alt+↑ and Alt+↓, removed with ✕
//! or Delete, and _Add layout…_, a popover that searches the layouts and variants of the XKB
//! rules.
//!
//! The layouts and the variants are two settings. Every change tries both as one change through
//! the safety net ([`Model::try_risky_all`]), so a layout and its variant stay together. The last
//! layout can't be removed. The list follows the device the row of the layouts is for.

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib};
use sway_things_core::registry::{Registry, Value};

use super::SettingRow;
use crate::model::Model;
use crate::style;
use crate::xkb;

/// The key of the layouts.
pub const LAYOUTS: &str = "input.keyboard.xkb_layout";
/// The key of the variants, one for each layout.
pub const VARIANTS: &str = "input.keyboard.xkb_variant";

/// The layouts and variants (as many as layouts) a list shows, and why they are locked.
type Shown = (Vec<String>, Vec<String>, Option<String>);

/// What the drag handle and the keyboard offer, for its tooltip and screen readers.
const HOW_TO_MOVE: &str =
    "Drag to reorder, or press Alt+Up or Alt+Down. Delete removes the layout.";

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    use crate::model::Model;
    use crate::widgets::SettingRow;

    #[derive(Debug, Default)]
    pub struct LayoutList {
        pub(super) row: OnceCell<SettingRow>,
        pub(super) model: OnceCell<Model>,
        pub(super) list: gtk::ListBox,
        pub(super) add: gtk::MenuButton,
        pub(super) search: gtk::SearchEntry,
        pub(super) results: gtk::ListBox,
        /// The layout and the variant of each row of `results`.
        pub(super) choices: RefCell<Vec<(String, String)>>,
        /// The layouts and variants (as many as layouts) the rows show, and why they are locked.
        pub(super) shown: RefCell<Option<super::Shown>>,
        /// The row to give the focus once the list is built again, after a move by keyboard.
        pub(super) focus: Cell<Option<usize>>,
        /// Set while the rows are built, so their drop-downs don't send changes.
        pub(super) updating: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LayoutList {
        const NAME: &'static str = "StLayoutList";
        type Type = super::LayoutList;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("st-layout-list");
            klass.set_layout_manager_type::<gtk::BoxLayout>();
        }
    }

    impl ObjectImpl for LayoutList {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for LayoutList {}
}

glib::wrapper! {
    /// The layouts of a keyboard, or of all keyboards, with their variants.
    pub struct LayoutList(ObjectSubclass<imp::LayoutList>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LayoutList {
    /// The list for the row of the layouts setting, `row`.
    pub fn new(model: &Model, row: &SettingRow) -> Self {
        let list: Self = glib::Object::new();
        let imp = list.imp();
        let _ = imp.row.set(row.clone());
        let _ = imp.model.set(model.clone());
        if let Some(layout) = list.layout_manager().and_downcast::<gtk::BoxLayout>() {
            layout.set_orientation(gtk::Orientation::Vertical);
            layout.set_spacing(10);
        }
        imp.list.set_selection_mode(gtk::SelectionMode::None);
        imp.list.add_css_class("st-layouts");
        imp.list
            .update_property(&[gtk::accessible::Property::Label("Layouts")]);
        imp.list.set_parent(&list);
        list.build_add();
        imp.add.set_parent(&list);

        model.connect_changed(glib::clone!(
            #[weak]
            list,
            move |_| list.refresh()
        ));
        row.connect_device_changed(glib::clone!(
            #[weak]
            list,
            move |_| list.refresh()
        ));
        list.refresh();
        list
    }

    /// The layouts and the variants (as many as there are layouts) the list shows.
    pub fn shown(&self) -> (Vec<String>, Vec<String>) {
        self.imp()
            .shown
            .borrow()
            .as_ref()
            .map(|(layouts, variants, _)| (layouts.clone(), variants.clone()))
            .unwrap_or_default()
    }

    /// The rows of the layouts, in order.
    pub fn rows(&self) -> Vec<gtk::ListBoxRow> {
        let mut rows = Vec::new();
        let mut child = self.imp().list.first_child();
        while let Some(widget) = child {
            if let Ok(row) = widget.clone().downcast::<gtk::ListBoxRow>() {
                rows.push(row);
            }
            child = widget.next_sibling();
        }
        rows
    }

    /// The _Add layout…_ button.
    pub fn add_button(&self) -> &gtk::MenuButton {
        &self.imp().add
    }

    /// Adds `layout` with `variant` (empty for none) at the end, through the safety net.
    pub fn add(&self, layout: &str, variant: &str) {
        let (mut layouts, mut variants) = self.shown();
        layouts.push(layout.to_owned());
        variants.push(variant.to_owned());
        self.change(layouts, variants);
    }

    /// Moves the layout at `from` with its variant to `to`, through the safety net.
    pub fn move_layout(&self, from: usize, to: usize) {
        let (mut layouts, mut variants) = self.shown();
        if from >= layouts.len() || to >= layouts.len() || from == to {
            return;
        }
        let layout = layouts.remove(from);
        let variant = variants.remove(from);
        layouts.insert(to, layout);
        variants.insert(to, variant);
        self.imp().focus.set(Some(to));
        self.change(layouts, variants);
    }

    /// Removes the layout at `index` with its variant, through the safety net. The last layout
    /// stays.
    pub fn remove(&self, index: usize) {
        let (mut layouts, mut variants) = self.shown();
        if layouts.len() < 2 || index >= layouts.len() {
            return;
        }
        layouts.remove(index);
        variants.remove(index);
        self.imp()
            .focus
            .set(Some(index.min(layouts.len().saturating_sub(1))));
        self.change(layouts, variants);
    }

    /// Gives the layout at `index` the variant `variant`, through the safety net.
    pub fn set_variant(&self, index: usize, variant: &str) {
        let (layouts, mut variants) = self.shown();
        if let Some(old) = variants.get_mut(index) {
            variant.clone_into(old);
        }
        self.change(layouts, variants);
    }

    /// Tries `layouts` with `variants` as one change: only the settings that change.
    fn change(&self, layouts: Vec<String>, mut variants: Vec<String>) {
        let imp = self.imp();
        let (Some(row), Some(model)) = (imp.row.get(), imp.model.get()) else {
            return;
        };
        let (old_layouts, mut old_variants) = self.shown();
        // Rendering pads the variants again; stored, they end with the last one there is.
        trim(&mut variants);
        trim(&mut old_variants);
        let mut changes = Vec::new();
        if layouts != old_layouts {
            changes.push((LAYOUTS, Some(Value::List(layouts))));
        }
        if variants != old_variants {
            changes.push((VARIANTS, Some(Value::List(variants))));
        }
        if changes.is_empty() {
            return;
        }
        row.set_error(None);
        model.try_risky_all(row.device().as_deref(), changes);
    }

    /// Shows the layouts and variants in effect for the device of the row, unless they are shown.
    fn refresh(&self) {
        let imp = self.imp();
        let (Some(row), Some(model)) = (imp.row.get(), imp.model.get()) else {
            return;
        };
        let Some(variants_setting) = Registry::builtin().get(VARIANTS) else {
            return;
        };
        let device = row.device();
        let Some(Value::List(layouts)) = row.value() else {
            return;
        };
        let mut variants = match model
            .view(variants_setting, device.as_deref())
            .map(|view| view.value)
        {
            Some(Value::List(variants)) => variants,
            _ => Vec::new(),
        };
        variants.resize(layouts.len(), String::new());
        let locked = row.locked();
        let shown = Some((layouts, variants, locked));
        if *imp.shown.borrow() == shown {
            return;
        }
        imp.shown.replace(shown);
        self.build_rows();
    }

    /// Builds a row for each layout shown.
    fn build_rows(&self) {
        let imp = self.imp();
        while let Some(child) = imp.list.first_child() {
            imp.list.remove(&child);
        }
        let Some((layouts, variants, locked)) = imp.shown.borrow().clone() else {
            return;
        };
        imp.updating.set(true);
        let count = layouts.len();
        for (index, (layout, variant)) in layouts.iter().zip(&variants).enumerate() {
            let item = self.layout_row(index, count, layout, variant);
            imp.list.append(&item);
        }
        imp.updating.set(false);
        let sensitive = locked.is_none();
        for widget in [imp.list.upcast_ref::<gtk::Widget>(), imp.add.upcast_ref()] {
            widget.set_sensitive(sensitive);
            widget.set_tooltip_text(locked.as_deref());
        }
        if let Some(index) = imp.focus.take() {
            if let Some(item) = self.rows().get(index) {
                item.grab_focus();
            }
        }
    }

    /// The row of the layout at `index` of `count`: a drag handle, its name, its variant and a
    /// remove button.
    fn layout_row(
        &self,
        index: usize,
        count: usize,
        layout: &str,
        variant: &str,
    ) -> gtk::ListBoxRow {
        let label = xkb::layout_label(layout);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);

        let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        handle.set_tooltip_text(Some(HOW_TO_MOVE));
        handle.set_cursor_from_name(Some("grab"));
        handle.set_accessible_role(gtk::AccessibleRole::Presentation);
        content.append(&handle);

        let name = gtk::Label::new(Some(&label));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_wrap(true);
        name.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        content.append(&name);

        let variants = variant_choices(layout, variant);
        let labels: Vec<&str> = variants.iter().map(|(_, label)| label.as_str()).collect();
        let dropdown = gtk::DropDown::from_strings(&labels);
        dropdown.set_valign(gtk::Align::Center);
        dropdown.update_property(&[gtk::accessible::Property::Label(&format!(
            "Variant of {label}"
        ))]);
        if let Some(selected) = variants
            .iter()
            .position(|(name, _)| name == variant)
            .and_then(|position| u32::try_from(position).ok())
        {
            dropdown.set_selected(selected);
        }
        let names: Vec<String> = variants.into_iter().map(|(name, _)| name).collect();
        dropdown.connect_selected_notify(glib::clone!(
            #[weak(rename_to = list)]
            self,
            move |dropdown| {
                if list.imp().updating.get() {
                    return;
                }
                let selected = usize::try_from(dropdown.selected()).ok();
                if let Some(name) = selected.and_then(|selected| names.get(selected)) {
                    list.set_variant(index, name);
                }
            }
        ));
        content.append(&dropdown);

        let remove = gtk::Button::from_icon_name("window-close-symbolic");
        remove.add_css_class("flat");
        remove.set_valign(gtk::Align::Center);
        remove.update_property(&[gtk::accessible::Property::Label(&format!("Remove {label}"))]);
        if count < 2 {
            remove.set_sensitive(false);
            remove.set_tooltip_text(Some("A keyboard needs at least one layout"));
        } else {
            remove.set_tooltip_text(Some("Remove the layout"));
        }
        remove.connect_clicked(glib::clone!(
            #[weak(rename_to = list)]
            self,
            move |_| list.remove(index)
        ));
        content.append(&remove);

        let item = gtk::ListBoxRow::new();
        item.set_activatable(false);
        item.set_child(Some(&content));
        item.update_property(&[
            gtk::accessible::Property::Label(&format!("{label}, layout {} of {count}", index + 1)),
            gtk::accessible::Property::Description(HOW_TO_MOVE),
        ]);
        self.add_moves(&item, &handle, index);
        item
    }

    /// Moving the layout at `index`: by dragging its handle onto another row, and with Alt+↑,
    /// Alt+↓ and Delete on its row.
    fn add_moves(&self, item: &gtk::ListBoxRow, handle: &gtk::Image, index: usize) {
        let Ok(dragged) = u32::try_from(index) else {
            return;
        };
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::MOVE);
        source.set_content(Some(&gdk::ContentProvider::for_value(&dragged.to_value())));
        source.connect_drag_begin(glib::clone!(
            #[weak]
            item,
            move |source, _| {
                let icon = gtk::WidgetPaintable::new(Some(&item));
                source.set_icon(Some(&icon), 0, 0);
            }
        ));
        handle.add_controller(source);

        let target = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
        target.connect_drop(glib::clone!(
            #[weak(rename_to = list)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                let Some(from) = value
                    .get::<u32>()
                    .ok()
                    .and_then(|from| usize::try_from(from).ok())
                else {
                    return false;
                };
                list.move_layout(from, index);
                true
            }
        ));
        item.add_controller(target);

        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = list)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);
                match key {
                    gdk::Key::Up | gdk::Key::KP_Up if alt => {
                        if let Some(to) = index.checked_sub(1) {
                            list.move_layout(index, to);
                        }
                        glib::Propagation::Stop
                    }
                    gdk::Key::Down | gdk::Key::KP_Down if alt => {
                        list.move_layout(index, index + 1);
                        glib::Propagation::Stop
                    }
                    gdk::Key::Delete | gdk::Key::KP_Delete => {
                        list.remove(index);
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            }
        ));
        item.add_controller(keys);
    }

    /// _Add layout…_ and its popover: a search entry over the results, which show the layouts,
    /// and while searching the layouts and variants that match.
    fn build_add(&self) {
        let imp = self.imp();
        imp.add.set_label("Add layout…");
        imp.add.set_halign(gtk::Align::Start);
        imp.add.set_always_show_arrow(false);

        imp.search.set_placeholder_text(Some("Search layouts"));
        imp.search
            .update_property(&[gtk::accessible::Property::Label("Search layouts")]);
        imp.results.set_selection_mode(gtk::SelectionMode::Browse);
        imp.results.set_activate_on_single_click(true);
        imp.results
            .update_property(&[gtk::accessible::Property::Label("Layouts")]);
        imp.results.set_filter_func(glib::clone!(
            #[weak(rename_to = list)]
            self,
            #[upgrade_or]
            false,
            move |result| list.is_result_shown(result)
        ));
        imp.results.connect_row_activated(glib::clone!(
            #[weak(rename_to = list)]
            self,
            move |_, result| list.choose(result)
        ));
        imp.search.connect_search_changed(glib::clone!(
            #[weak(rename_to = list)]
            self,
            move |_| list.imp().results.invalidate_filter()
        ));
        imp.search.connect_activate(glib::clone!(
            #[weak(rename_to = list)]
            self,
            move |_| {
                if let Some(first) = list.first_result() {
                    list.choose(&first);
                }
            }
        ));
        imp.search.set_key_capture_widget(Some(&imp.results));

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_min_content_height(280);
        scroller.set_max_content_height(360);
        scroller.set_min_content_width(320);
        scroller.set_propagate_natural_height(true);
        scroller.set_child(Some(&imp.results));

        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.append(&imp.search);
        content.append(&scroller);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&content));
        popover.connect_show(glib::clone!(
            #[weak(rename_to = list)]
            self,
            move |_| {
                list.fill_results();
                list.imp().search.grab_focus();
            }
        ));
        popover.connect_closed(glib::clone!(
            #[weak(rename_to = list)]
            self,
            move |_| list.imp().search.set_text("")
        ));
        imp.add.set_popover(Some(&popover));
    }

    /// Fills the results with every layout and variant of the XKB rules, once.
    fn fill_results(&self) {
        let imp = self.imp();
        if !imp.choices.borrow().is_empty() {
            return;
        }
        let mut choices = Vec::new();
        for layout in &xkb::rules().layouts {
            imp.results
                .append(&result_row(layout.item.label(), &layout.item.name));
            choices.push((layout.item.name.clone(), String::new()));
            for variant in &layout.variants {
                imp.results.append(&result_row(
                    variant.label(),
                    &format!("{}({})", layout.item.name, variant.name),
                ));
                choices.push((layout.item.name.clone(), variant.name.clone()));
            }
        }
        imp.choices.replace(choices);
        imp.results.invalidate_filter();
    }

    /// Whether the result row `result` matches the search: without one, the layouts that aren't
    /// exotic; while searching, every layout and variant that matches.
    fn is_result_shown(&self, result: &gtk::ListBoxRow) -> bool {
        let imp = self.imp();
        let query = imp.search.text().trim().to_lowercase();
        let choices = imp.choices.borrow();
        let Some((layout, variant)) = usize::try_from(result.index())
            .ok()
            .and_then(|index| choices.get(index))
        else {
            return false;
        };
        let Some(known) = xkb::rules().layout(layout) else {
            return false;
        };
        if query.is_empty() {
            return variant.is_empty() && !known.item.extra;
        }
        if variant.is_empty() {
            xkb::matches(&known.item, &query)
        } else {
            known
                .variant(variant)
                .is_some_and(|item| xkb::matches(item, &query))
        }
    }

    /// The first result the search shows.
    fn first_result(&self) -> Option<gtk::ListBoxRow> {
        let mut child = self.imp().results.first_child();
        while let Some(widget) = child {
            if let Ok(result) = widget.clone().downcast::<gtk::ListBoxRow>() {
                if result.is_child_visible() && self.is_result_shown(&result) {
                    return Some(result);
                }
            }
            child = widget.next_sibling();
        }
        None
    }

    /// Adds the layout and variant of the result row `result`, and closes the popover.
    fn choose(&self, result: &gtk::ListBoxRow) {
        let imp = self.imp();
        let choice = usize::try_from(result.index())
            .ok()
            .and_then(|index| imp.choices.borrow().get(index).cloned());
        imp.add.popdown();
        if let Some((layout, variant)) = choice {
            self.add(&layout, &variant);
        }
    }
}

/// A result of the search: the description, and the name sway takes below it.
fn result_row(label: &str, name: &str) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(8);
    content.set_margin_end(8);
    let title = gtk::Label::new(Some(label));
    title.set_xalign(0.0);
    title.set_wrap(true);
    let detail = gtk::Label::new(Some(name));
    detail.set_xalign(0.0);
    detail.add_css_class("monospace");
    detail.add_css_class(style::DIM);
    content.append(&title);
    content.append(&detail);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    row.update_property(&[gtk::accessible::Property::Label(label)]);
    row
}

/// The variants to pick for `layout`: none first, then those of the XKB rules. `current` is
/// added if the rules don't know it.
fn variant_choices(layout: &str, current: &str) -> Vec<(String, String)> {
    let mut choices = vec![(String::new(), "Standard".to_owned())];
    if let Some(known) = xkb::rules().layout(layout) {
        choices.extend(
            known
                .variants
                .iter()
                .map(|variant| (variant.name.clone(), variant.label().to_owned())),
        );
    }
    if !choices.iter().any(|(name, _)| name == current) {
        choices.push((current.to_owned(), xkb::variant_label(layout, current)));
    }
    choices
}

/// Drops the empty variants at the end.
fn trim(variants: &mut Vec<String>) {
    while variants.last().is_some_and(String::is_empty) {
        variants.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::{trim, variant_choices};

    #[test]
    fn variants_end_with_the_last_one_there_is() {
        let mut variants = vec![String::new(), "nodeadkeys".to_owned(), String::new()];
        trim(&mut variants);
        assert_eq!(variants, ["", "nodeadkeys"]);
    }

    #[test]
    fn an_unknown_variant_is_still_offered() {
        let choices = variant_choices("no-such-layout", "custom");
        assert_eq!(choices.first().map(|(name, _)| name.as_str()), Some(""));
        assert!(choices.iter().any(|(name, _)| name == "custom"));
    }
}
