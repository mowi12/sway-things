// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Bar page's own parts, which [`PageView`](super::PageView) adds to the rows built from the
//! registry:
//!
//! - above the rows: the preview of the bar with a text summary ([`preview`]), and the card of
//!   the bar program with how the bar runs ([`backend`], [`status`]);
//! - in place of the item lists' rows: three lanes of items ([`lanes`]), which move by
//!   dragging, with Alt and the arrow keys or from each item's menu, get items from the picker
//!   ([`picker`]) and open an item's settings ([`dialog`], [`script`] for a command of the
//!   user's);
//! - above the height: the sizes; above the colors: the presets as cards ([`presets`]);
//! - below the displays: a check button per display; below the text color: its contrast; below
//!   the status command: the files Sway Things writes ([`files`]).
//!
//! The parts share a [`Ctx`] while the page is built: the model, and the changes Ctrl+Z (or a
//! note's _Undo_, [`toast`]) takes back.

mod backend;
mod dialog;
mod files;
mod lanes;
mod mimic;
mod picker;
mod presets;
mod preview;
mod script;
mod status;
mod takeover;
mod toast;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bar::{Modules, style};
use sway_things_core::registry::{Registry, Value, bar as keys};
use sway_things_core::theme;
use sway_things_core::view::Source;

use super::SettingRow;
use crate::i18n::{Msg, msg, tr, trf};
use crate::model::Model;

/// One step Ctrl+Z takes back: the settings it changed with their values before, `None` where
/// nothing was stored.
type Step = Vec<(&'static str, Option<Value>)>;

/// The class of the item of the preview the lanes point at.
pub(crate) const MARKED: &str = "st-bar-marked";

/// What the parts of one Bar page share.
#[derive(Debug)]
pub(crate) struct Ctx {
    model: Model,
    /// The steps Ctrl+Z takes back, each with the module definitions before it, where it
    /// changed them too (groups).
    undo: RefCell<Vec<(Step, Option<Modules>)>>,
    anchors: RefCell<Vec<(&'static str, gtk::Widget)>>,
    /// The widget that says what happened to a screen reader: the lanes.
    voice: RefCell<glib::WeakRef<gtk::Widget>>,
    /// The items of the preview by name, to mark the one the lanes point at.
    shown: RefCell<Vec<(String, glib::WeakRef<gtk::Widget>)>>,
    /// The item the lanes point at (a group under the pointer or with the focus there).
    marked: RefCell<Option<String>>,
}

thread_local! {
    /// The parts of the Bar page being built.
    static BUILDING: RefCell<Option<Rc<Ctx>>> = const { RefCell::new(None) };
}

impl Ctx {
    /// The context of the page being built, made on first use.
    fn current(model: &Model) -> Rc<Self> {
        BUILDING.with(|building| {
            building
                .borrow_mut()
                .get_or_insert_with(|| {
                    Rc::new(Self {
                        model: model.clone(),
                        undo: RefCell::new(Vec::new()),
                        anchors: RefCell::new(Vec::new()),
                        voice: RefCell::new(glib::WeakRef::new()),
                        shown: RefCell::new(Vec::new()),
                        marked: RefCell::new(None),
                    })
                })
                .clone()
        })
    }

    /// The model.
    pub(crate) fn model(&self) -> &Model {
        &self.model
    }

    /// The value of bar setting `key` in effect.
    pub(crate) fn value(&self, key: &str) -> Option<Value> {
        let setting = Registry::builtin().get(key)?;
        self.model.view(setting, None).map(|view| view.value)
    }

    /// The list of setting `key`.
    pub(crate) fn list(&self, key: &str) -> Vec<String> {
        match self.value(key) {
            Some(Value::List(items)) => items,
            _ => Vec::new(),
        }
    }

    /// Why setting `key` can't be changed now, if it can't.
    pub(crate) fn locked(&self, key: &str) -> Option<String> {
        let setting = Registry::builtin().get(key)?;
        self.model
            .view(setting, None)
            .and_then(|view| view.locked.map(|locked| locked.text().translated()))
    }

    /// Whether the settings can be written: the include line is there.
    pub(crate) fn writable(&self) -> bool {
        self.model.status().is_some_and(|status| status.is_active())
    }

    /// What `keys` hold now, to take a change of them back.
    pub(crate) fn snapshot(&self, keys: &[&'static str]) -> Step {
        keys.iter()
            .filter_map(|key| {
                let setting = Registry::builtin().get(key)?;
                let view = self.model.view(setting, None)?;
                let stored = matches!(view.source, Source::Stored { .. });
                Some((*key, stored.then_some(view.value)))
            })
            .collect()
    }

    /// Remembers `step` for Ctrl+Z.
    pub(crate) fn remember(&self, step: Step) {
        self.remember_with(step, None);
    }

    /// Remembers `step` for Ctrl+Z, with `modules`, the module definitions before it.
    pub(crate) fn remember_with(&self, step: Step, modules: Option<Modules>) {
        let mut undo = self.undo.borrow_mut();
        undo.push((step, modules));
        // A long session of changes needs no endless history.
        if undo.len() > 50 {
            undo.remove(0);
        }
    }

    /// Takes the last change back. Says whether there was one.
    pub(crate) fn undo(&self) -> bool {
        let Some((step, modules)) = self.undo.borrow_mut().pop() else {
            return false;
        };
        // The definitions first: the lists may name groups only they define.
        if let Some(modules) = modules {
            self.model.set_bar_modules(modules);
        }
        for (key, value) in step {
            match value {
                Some(value) => self.model.set(key, None, value),
                None => self.model.reset(key, None),
            }
        }
        true
    }

    /// Lets `widget` say what happens on the page to a screen reader.
    pub(crate) fn set_voice(&self, widget: &impl IsA<gtk::Widget>) {
        self.voice.replace(widget.as_ref().downgrade());
    }

    /// Says `text` to a screen reader.
    pub(crate) fn announce(&self, text: &str) {
        if let Some(widget) = self.voice.borrow().upgrade() {
            widget.announce(text, gtk::AccessibleAnnouncementPriority::Medium);
        }
    }

    /// The module definitions of the bar; none while `bar.jsonc` can't be read.
    pub(crate) fn modules(&self) -> Modules {
        self.model
            .bar_modules()
            .and_then(Result::ok)
            .unwrap_or_default()
    }

    /// Notes the items the preview shows now, by name.
    pub(crate) fn set_shown(&self, shown: Vec<(String, gtk::Widget)>) {
        self.shown.replace(
            shown
                .into_iter()
                .map(|(name, widget)| (name, widget.downgrade()))
                .collect(),
        );
        let marked = self.marked.borrow().clone();
        self.mark(marked.as_deref());
    }

    /// Marks item `name` in the preview (a drawer opens), or with `None` none.
    pub(crate) fn mark(&self, name: Option<&str>) {
        self.marked.replace(name.map(ToOwned::to_owned));
        for (shown, widget) in self.shown.borrow().iter() {
            if let Some(widget) = widget.upgrade() {
                let on = Some(shown.as_str()) == name;
                if on {
                    widget.add_css_class(MARKED);
                } else {
                    widget.remove_css_class(MARKED);
                }
                if let Some(revealer) = mimic::drawer_of(&widget) {
                    revealer.set_reveal_child(on);
                }
            }
        }
    }

    /// Lets `widget` stand for the row of `key`, which the page doesn't show.
    fn anchor(&self, key: &'static str, widget: &impl IsA<gtk::Widget>) {
        self.anchors
            .borrow_mut()
            .push((key, widget.as_ref().clone()));
    }
}

/// Adds the parts above the rows to `body`: the preview and the card of the bar program, and
/// Ctrl+Z for the changes of the page.
pub fn before(model: &Model, body: &gtk::Box) {
    BUILDING.with(|building| building.replace(None));
    let ctx = Ctx::current(model);
    let preview = preview::Preview::new(&ctx);
    body.append(preview.widget());
    let card = backend::card(&ctx);
    body.append(&card);
    ctx.anchor(keys::BACKEND, &card);
    let shortcuts = gtk::ShortcutController::new();
    let weak = Rc::downgrade(&ctx);
    shortcuts.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("<Control>z"),
        Some(gtk::CallbackAction::new(move |_, _| {
            if weak.upgrade().is_some_and(|ctx| ctx.undo()) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        })),
    ));
    body.add_controller(shortcuts);
    // The page keeps its parts' context as long as it lives.
    let keep = RefCell::new(Some(ctx));
    body.connect_destroy(move |_| drop(keep.take()));
}

/// The widgets that stand for the rows the page doesn't show, once it is built.
pub fn finish() -> Vec<(&'static str, gtk::Widget)> {
    BUILDING
        .with(|building| building.borrow_mut().take())
        .map(|ctx| ctx.anchors.take())
        .unwrap_or_default()
}

/// Whether the page shows setting `key` in a way of its own instead of its row.
pub fn is_hidden(key: &str) -> bool {
    matches!(
        key,
        keys::BACKEND
            | keys::TAKE_OVER
            | keys::MODULES_LEFT
            | keys::MODULES_CENTER
            | keys::MODULES_RIGHT
    )
}

/// The widgets the Bar page shows above `row`.
pub fn above(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let Some(setting) = row.setting() else {
        return Vec::new();
    };
    let ctx = Ctx::current(model);
    match setting.key() {
        keys::HEIGHT => vec![sizes(&ctx)],
        keys::THEME => vec![presets::Presets::new(&ctx).widget().clone().upcast()],
        keys::MODIFIER => {
            shown_while_held(&ctx, row);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Shows the row of the key that shows the bar (`row`, once in its list) only while the bar
/// shows while a key is held.
fn shown_while_held(ctx: &Rc<Ctx>, row: &SettingRow) {
    let apply = {
        let weak = Rc::downgrade(ctx);
        let row = row.downgrade();
        move || {
            let (Some(ctx), Some(row)) = (weak.upgrade(), row.upgrade()) else {
                return;
            };
            let held =
                matches!(ctx.value(keys::VISIBILITY), Some(Value::Choice(id)) if id == "hold");
            if let Some(item) = row.parent() {
                item.set_visible(held);
            }
        }
    };
    let on_parent = apply.clone();
    row.connect_parent_notify(move |_| on_parent());
    follow(ctx.model(), row, apply);
}

/// The widgets the Bar page shows below `row`.
pub fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let Some(setting) = row.setting() else {
        return Vec::new();
    };
    let ctx = Ctx::current(model);
    match setting.key() {
        keys::MODULES_LEFT => {
            let lanes = lanes::Lanes::new(&ctx);
            for key in [
                keys::MODULES_LEFT,
                keys::MODULES_CENTER,
                keys::MODULES_RIGHT,
            ] {
                ctx.anchor(key, lanes.widget());
            }
            vec![lanes.widget().clone().upcast()]
        }
        keys::OUTPUTS => vec![outputs(model, row)],
        keys::TEXT => vec![contrast(&ctx)],
        keys::STATUS_COMMAND => vec![files::row(&ctx)],
        _ => Vec::new(),
    }
}

/// Calls `refresh` now and whenever `model` changes, while `widget` exists.
pub(crate) fn follow(model: &Model, widget: &impl IsA<gtk::Widget>, refresh: impl Fn() + 'static) {
    refresh();
    let weak = widget.as_ref().downgrade();
    let handler = Rc::new(RefCell::new(None));
    let id = model.connect_changed(move |_| {
        if weak.upgrade().is_some() {
            refresh();
        }
    });
    handler.replace(Some(id));
    widget.as_ref().connect_destroy(glib::clone!(
        #[weak]
        model,
        move |_| {
            if let Some(id) = handler.take() {
                model.disconnect(id);
            }
        }
    ));
}

/// The list a list setting's row shows.
pub(crate) fn list_of(row: &SettingRow) -> Vec<String> {
    match row.value() {
        Some(Value::List(items)) => items,
        _ => Vec::new(),
    }
}

/// A check button per display, connected or named in the list, below the displays row: the
/// bar shows on those checked, or on all while none is.
fn outputs(model: &Model, row: &SettingRow) -> gtk::Widget {
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_max_children_per_line(4);
    flow.set_row_spacing(4);
    flow.set_column_spacing(12);
    flow.add_css_class("st-bar-outputs");
    flow.update_property(&[gtk::accessible::Property::Label(tr(msg!(
        "Displays that show the bar"
    )))]);
    let updating = Rc::new(std::cell::Cell::new(false));
    let refresh = glib::clone!(
        #[weak]
        flow,
        #[weak]
        row,
        #[weak]
        model,
        #[strong]
        updating,
        move || {
            updating.set(true);
            while let Some(child) = flow.first_child() {
                flow.remove(&child);
            }
            let chosen = list_of(&row);
            let mut names: Vec<String> = model
                .outputs()
                .map(|outputs| {
                    outputs
                        .iter()
                        .filter(|output| output.is_connected())
                        .map(|output| output.name().to_owned())
                        .collect()
                })
                .unwrap_or_default();
            for name in &chosen {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            for name in names {
                let check = gtk::CheckButton::with_label(&name);
                check.set_active(chosen.contains(&name));
                check.set_sensitive(row.locked().is_none());
                check.connect_toggled(glib::clone!(
                    #[weak]
                    row,
                    #[strong]
                    updating,
                    move |check| {
                        if updating.get() {
                            return;
                        }
                        let mut list = list_of(&row);
                        list.retain(|item| *item != name);
                        if check.is_active() {
                            list.push(name.clone());
                        }
                        row.change(Value::List(list));
                    }
                ));
                flow.append(&check);
            }
            updating.set(false);
        }
    );
    follow(model, &flow, refresh);
    flow.upcast()
}

/// A size of the bar: its height and the size of its text, in pixels.
struct Size {
    title: Msg,
    height: i64,
    text: u32,
}

/// The sizes above the height.
const SIZES: [Size; 4] = [
    Size {
        title: msg!("Small"),
        height: 24,
        text: 11,
    },
    Size {
        title: msg!("Medium"),
        height: 30,
        text: 13,
    },
    Size {
        title: msg!("Large"),
        height: 38,
        text: 16,
    },
    Size {
        title: msg!("Huge"),
        height: 48,
        text: 20,
    },
];

/// The sizes as linked buttons: each sets the height and the size of the font together, which
/// is what makes a bar easy to read.
fn sizes(ctx: &Rc<Ctx>) -> gtk::Widget {
    let title = gtk::Label::new(Some(tr(msg!("Size"))));
    title.set_xalign(0.0);
    let subtitle = gtk::Label::new(Some(tr(msg!("The height and the text size together"))));
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.add_css_class(crate::style::DIM);
    subtitle.add_css_class("st-row-detail");
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    texts.append(&title);
    texts.append(&subtitle);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    buttons.add_css_class("linked");
    buttons.set_valign(gtk::Align::Center);
    buttons.set_accessible_role(gtk::AccessibleRole::Group);
    buttons.update_property(&[gtk::accessible::Property::Label(tr(msg!("Size")))]);
    let mut toggles: Vec<gtk::ToggleButton> = Vec::new();
    for size in &SIZES {
        let button = gtk::ToggleButton::with_label(tr(size.title));
        if let Some(first) = toggles.first() {
            button.set_group(Some(first));
        }
        let (height, text) = (size.height, size.text);
        let weak = Rc::downgrade(ctx);
        button.connect_clicked(move |button| {
            let Some(ctx) = weak.upgrade() else { return };
            if !button.is_active() || size_of(&ctx) == Some((height, text)) {
                return;
            }
            let font = match ctx.value(keys::FONT) {
                Some(Value::Text(font)) => font,
                _ => String::new(),
            };
            ctx.remember(ctx.snapshot(&[keys::HEIGHT, keys::FONT]));
            ctx.model().set(keys::HEIGHT, None, Value::Integer(height));
            ctx.model().set(
                keys::FONT,
                None,
                Value::Text(style::font_with_size(&font, Some(text))),
            );
        });
        buttons.append(&button);
        toggles.push(button);
    }
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.add_css_class("st-bar-sizes");
    line.append(&texts);
    line.append(&buttons);
    let weak = Rc::downgrade(ctx);
    follow(ctx.model(), &line, move || {
        let Some(ctx) = weak.upgrade() else { return };
        let current = size_of(&ctx);
        for (toggle, size) in toggles.iter().zip(&SIZES) {
            toggle.set_active(current == Some((size.height, size.text)));
        }
        buttons.set_sensitive(ctx.locked(keys::HEIGHT).is_none() && ctx.writable());
    });
    line.upcast()
}

/// The height and the text size in pixels the bar has now, if both are set.
fn size_of(ctx: &Ctx) -> Option<(i64, u32)> {
    let Some(Value::Integer(height)) = ctx.value(keys::HEIGHT) else {
        return None;
    };
    let Some(Value::Text(font)) = ctx.value(keys::FONT) else {
        return None;
    };
    let size = style::font_size_px(&font)?;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a font size in pixels, checked to be in range"
    )]
    let size = (size > 0.0 && size < 1000.0).then(|| size.round() as u32)?;
    Some((height, size))
}

/// The contrast of the text on the bar's background below the text color, with a warning where
/// it is hard to read.
fn contrast(ctx: &Rc<Ctx>) -> gtk::Widget {
    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    icon.set_valign(gtk::Align::Start);
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_wrap(true);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.add_css_class("st-bar-contrast");
    line.append(&icon);
    line.append(&label);
    let weak = Rc::downgrade(ctx);
    let shown = line.clone();
    follow(ctx.model(), &line, move || {
        let Some(ctx) = weak.upgrade() else { return };
        let custom = matches!(ctx.value(keys::THEME), Some(Value::Choice(id)) if id == "custom");
        let Some(look) = ctx.model().bar_look(None).filter(|_| custom) else {
            shown.set_visible(false);
            return;
        };
        shown.set_visible(true);
        let ratio = format!("{:.1}", contrast_ratio(&look));
        let readable = contrast_ratio(&look) >= 4.5;
        icon.set_visible(!readable);
        label.set_text(&if readable {
            trf(READABLE, &[("ratio", &ratio)])
        } else {
            trf(UNREADABLE, &[("ratio", &ratio)])
        });
    });
    line.upcast()
}

/// The contrast of readable text.
const READABLE: Msg = msg!("Text on the background: {ratio}:1, easy to read");
/// The contrast of text that is hard to read.
const UNREADABLE: Msg = msg!(
    "Text on the background: {ratio}:1, hard to read. Pick a lighter text or a darker \
     background, or the other way round."
);

/// The contrast of `look`'s text on its background, the background laid over black where it is
/// transparent (a dark wallpaper is the common case).
pub(crate) fn contrast_ratio(look: &style::Look) -> f64 {
    let rgb = |color: sway_things_core::command::Color| {
        let [red, green, blue] = color.channels();
        theme::Rgb::from_u8(red, green, blue)
    };
    let background = look.colors.background;
    let alpha = f64::from(background.alpha().unwrap_or(0xff)) / 255.0 * look.opacity;
    let fill = theme::Rgb::from_u8(0, 0, 0).under(rgb(background), alpha.clamp(0.0, 1.0));
    theme::ratio(rgb(look.colors.text), fill)
}
