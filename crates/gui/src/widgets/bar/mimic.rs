// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview's waybar: the widgets waybar builds, with its names and classes, so the bar's own
//! stylesheet draws them, over a slice of the wallpaper.
//!
//! waybar puts each module in a `widget` (an event box) in the box of its lane
//! (`.modules-left`); a module is a label named after it (`#clock`, `#custom-media`, with the part
//! after `#` as its class), the workspaces a box of buttons, a group a box named after it with
//! the `widget`s of its members. A drawer shows only its first member until it opens, so the
//! preview shows that one. Items that only show while something happens (the binding mode, the
//! privacy dots) are left out, as on a quiet desktop. A command of the user's never runs here:
//! its item shows its format, or its name.
//!
//! The stylesheet ([`sway_things_core::bar::preview`]) is loaded into a provider of the preview
//! above the user's own `gtk.css`, after a rule that takes every property of the preview's
//! widgets back to its initial value, so neither the app's theme nor the user's GTK theme reaches
//! them; what GTK 4 can't parse is skipped quietly.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use sway_things_core::bar::preview::css_name;
use sway_things_core::bar::{Modules, catalog, kind_of, modules::members};
use sway_things_core::registry::wallpaper::IMAGE;
use sway_things_core::registry::{Registry, Value};
use sway_things_core::wallpaper::{self, COLLECTION};

use super::preview::{OPENS, has_icon_glyph, item_missing, item_sample, item_title, spoken};
use super::{Ctx, dialog};
use crate::i18n::trf;

/// How deep groups in groups are followed.
const MAX_DEPTH: u8 = 8;

mod imp {
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    /// A module's `widget`, as waybar's event box is named in CSS.
    #[derive(Debug, Default)]
    pub struct ModuleBox;

    #[glib::object_subclass]
    impl ObjectSubclass for ModuleBox {
        const NAME: &'static str = "StBarModule";
        type Type = super::ModuleBox;
        type ParentType = gtk::Box;

        fn class_init(class: &mut Self::Class) {
            class.set_css_name("widget");
        }
    }

    impl ObjectImpl for ModuleBox {}

    // A box only passes the focus on to its children; an item of the preview takes it itself,
    // so Tab goes from item to item.
    impl WidgetImpl for ModuleBox {
        fn grab_focus(&self) -> bool {
            let widget = self.obj();
            if !widget.is_focusable() {
                return self.parent_grab_focus();
            }
            match widget.root() {
                Some(root) => {
                    root.set_focus(Some(&*widget));
                    widget.is_focus()
                }
                None => false,
            }
        }

        fn focus(&self, direction: gtk::DirectionType) -> bool {
            let widget = self.obj();
            if !widget.is_focusable() {
                return self.parent_focus(direction);
            }
            !widget.is_focus() && self.grab_focus()
        }
    }
    impl BoxImpl for ModuleBox {}
}

glib::wrapper! {
    /// A module's `widget` in the preview: a box named `widget` in CSS.
    pub struct ModuleBox(ObjectSubclass<imp::ModuleBox>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl ModuleBox {
    /// An empty `widget`.
    fn new() -> Self {
        glib::Object::new()
    }
}

/// The rule before the bar's stylesheet: every property of the preview's widgets at its initial
/// value (inherited ones from the root), as in a waybar without a GTK theme.
pub(super) fn reset(root: &str) -> String {
    format!(".{root}, .{root} * {{ all: unset; }}\n")
}

/// The rule after it: the item with the keyboard focus is outlined.
pub(super) fn focus_ring(root: &str) -> String {
    format!(
        ".{root} widget:focus-visible {{ outline: 2px solid alpha(white, 0.9); \
         outline-offset: -2px; }}\n"
    )
}

/// A provider for the preview's stylesheet, above the user's own `gtk.css`, removed again when
/// `widget` is destroyed. What GTK 4 can't parse in it is skipped without a warning.
pub(super) fn provider_for(widget: &impl IsA<gtk::Widget>) -> gtk::CssProvider {
    let provider = gtk::CssProvider::new();
    provider.connect_parsing_error(|provider, _, _| {
        provider.stop_signal_emission_by_name("parsing-error");
    });
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 2,
        );
    }
    let removed = provider.clone();
    widget.as_ref().connect_destroy(move |_| {
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_remove_provider_for_display(&display, &removed);
        }
    });
    provider
}

/// The `widget` of item `name` of a lane, with what it shows and whether an icon font is
/// missing for it; `None` where waybar shows nothing on a quiet desktop.
pub(super) fn item(ctx: &Rc<Ctx>, modules: &Modules, name: &str) -> Option<(ModuleBox, bool)> {
    let mut iconless = false;
    let (widget, shown) = module(modules, name, 0, &mut iconless)?;
    let title = item_title(modules, name);
    let shown = if shown.is_empty() {
        title.clone()
    } else {
        shown
    };
    let described = trf(OPENS, &[("item", &title), ("value", &shown)]);
    widget.set_focusable(true);
    widget.set_accessible_role(gtk::AccessibleRole::Button);
    widget.set_tooltip_text(Some(&described));
    widget.update_property(&[gtk::accessible::Property::Label(&described)]);
    let open = {
        let weak = Rc::downgrade(ctx);
        let name = name.to_owned();
        move |widget: &gtk::Widget| {
            if let Some(ctx) = weak.upgrade() {
                dialog::open(&ctx, widget, &name);
            }
        }
    };
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_PRIMARY);
    {
        let open = open.clone();
        click.connect_released(move |gesture, _, _, _| {
            if let Some(widget) = gesture.widget() {
                open(&widget);
            }
        });
    }
    widget.add_controller(click);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(move |controller, key, _, _| {
        use gtk::gdk::Key;
        if matches!(
            key,
            Key::Return | Key::KP_Enter | Key::space | Key::KP_Space
        ) {
            if let Some(widget) = controller.widget() {
                open(&widget);
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    widget.add_controller(keys);
    Some((widget, iconless))
}

/// A waybar window of the bar's items `lanes` (left, center, right), at the top, drawn by the
/// rules of the class `class`: what a card of a look shows, built as the preview builds it, but
/// not to be clicked. `spacing` is waybar's space between items, `height` its height.
pub(super) fn mini_bar(
    modules: &Modules,
    lanes: [&[String]; 3],
    class: &str,
    spacing: i32,
    height: i32,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class(class);
    root.add_css_class("top");
    root.add_css_class("st-bar-picture");
    root.set_can_target(false);
    root.set_size_request(-1, height);
    let center = gtk::CenterBox::new();
    center.set_hexpand(true);
    let mut iconless = false;
    let boxes = [
        (lanes[0], "modules-left"),
        (lanes[1], "modules-center"),
        (lanes[2], "modules-right"),
    ]
    .map(|(names, lane)| {
        let items = gtk::Box::new(gtk::Orientation::Horizontal, spacing);
        items.add_css_class(lane);
        for name in names {
            if let Some((widget, _)) = module(modules, name, 0, &mut iconless) {
                items.append(&widget);
            }
        }
        items
    });
    let [left, middle, right] = boxes;
    center.set_start_widget(Some(&left));
    center.set_center_widget(Some(&middle));
    center.set_end_widget(Some(&right));
    root.append(&center);
    root
}

/// The `widget` of module `name` and the text it shows, at `depth` in groups.
fn module(
    modules: &Modules,
    name: &str,
    depth: u8,
    iconless: &mut bool,
) -> Option<(ModuleBox, String)> {
    let kind = kind_of(name);
    if depth > MAX_DEPTH || item_missing(name).is_some() || matches!(kind, "sway/mode" | "privacy")
    {
        return None;
    }
    let definition = modules.definition(name);
    let (id, class) = css_name(name);
    let outer = ModuleBox::new();
    let (content, shown): (gtk::Widget, String) = if kind.starts_with("group/") {
        let group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let members = definition.as_ref().map(members).unwrap_or_default();
        // A drawer shows its first member until it opens.
        let drawer = definition
            .as_ref()
            .is_some_and(|definition| definition.contains_key("drawer"));
        let shown_members = if drawer {
            &members[..members.len().min(1)]
        } else {
            &members[..]
        };
        let mut words = Vec::new();
        for member in shown_members {
            if let Some((widget, text)) = module(modules, member, depth + 1, iconless) {
                group.append(&widget);
                if !text.is_empty() {
                    words.push(text);
                }
            }
        }
        (group.upcast(), words.join(" "))
    } else {
        match kind {
            "sway/workspaces" => workspaces(definition.as_ref(), iconless),
            "tray" => {
                let spacing = definition
                    .as_ref()
                    .and_then(|definition| definition.get("spacing")?.as_i64())
                    .and_then(|spacing| i32::try_from(spacing.clamp(0, 64)).ok())
                    .unwrap_or(0);
                let size = icon_size(definition.as_ref(), "icon-size");
                let tray = gtk::Box::new(gtk::Orientation::Horizontal, spacing);
                for icon in [
                    "network-wireless-symbolic",
                    "audio-volume-high-symbolic",
                    "mail-unread-symbolic",
                ] {
                    let image = gtk::Image::from_icon_name(icon);
                    image.set_pixel_size(size);
                    tray.append(&image);
                }
                (tray.upcast(), String::new())
            }
            "image" => {
                // Its picture comes from a path or a command, which the preview doesn't read.
                let holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                let image = gtk::Image::from_icon_name("pan-start-symbolic");
                image.set_pixel_size(icon_size(definition.as_ref(), "size"));
                holder.append(&image);
                (holder.upcast(), String::new())
            }
            _ => {
                let text = if kind.starts_with("custom/") {
                    catalog::custom_sample(name, definition.as_ref())
                } else {
                    item_sample(modules, name).unwrap_or_else(|| item_title(modules, name))
                };
                let label = label(&text, iconless);
                // waybar marks every module so; the looks reach modules by it.
                label.add_css_class("module");
                if kind == "battery" {
                    label.add_css_class("discharging");
                    if let Some(state) = battery_state(definition.as_ref(), &text) {
                        label.add_css_class(&state);
                    }
                }
                (label.upcast(), spoken(&text))
            }
        }
    };
    content.set_widget_name(&id);
    if let Some(class) = class {
        content.add_css_class(&class);
    }
    outer.append(&content);
    Some((outer, shown))
}

/// The workspaces: three buttons, the first one focused, each as the definition's format shows
/// it; with what they show.
fn workspaces(
    definition: Option<&serde_json::Map<String, serde_json::Value>>,
    iconless: &mut bool,
) -> (gtk::Widget, String) {
    let workspaces = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let mut words = Vec::new();
    for (index, number) in ["1", "2", "3"].into_iter().enumerate() {
        let focused = index == 0;
        let text = catalog::workspace_label(definition, number, focused);
        let button = gtk::Button::new();
        button.set_child(Some(&label(&text, iconless)));
        button.set_can_target(false);
        button.set_focusable(false);
        if focused {
            button.add_css_class("focused");
            button.add_css_class("visible");
        }
        workspaces.append(&button);
        words.push(text);
    }
    (workspaces.upcast(), spoken(&words.join(" ")))
}

/// A label of the preview showing `text` as plain text; notes in `iconless` when its icon glyphs
/// can't be drawn.
fn label(text: &str, iconless: &mut bool) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    if has_icon_glyph(text) {
        *iconless |= label.create_pango_layout(Some(text)).unknown_glyphs_count() > 0;
    }
    label
}

/// The icon size option `key` of a definition, 16 by default.
fn icon_size(definition: Option<&serde_json::Map<String, serde_json::Value>>, key: &str) -> i32 {
    definition
        .and_then(|definition| definition.get(key)?.as_i64())
        .and_then(|size| i32::try_from(size.clamp(8, 64)).ok())
        .unwrap_or(16)
}

/// The class of the state a battery showing `text` (its first number, the charge) is in, as
/// waybar picks it: of the `states` whose value is at least the charge, the lowest.
fn battery_state(
    definition: Option<&serde_json::Map<String, serde_json::Value>>,
    text: &str,
) -> Option<String> {
    let digits: String = text
        .chars()
        .skip_while(|char| !char.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    let charge: i64 = digits.parse().ok()?;
    definition?
        .get("states")?
        .as_object()?
        .iter()
        .filter_map(|(state, value)| Some((state, value.as_i64()?)))
        .filter(|(_, value)| *value >= charge)
        .min_by_key(|(_, value)| *value)
        .map(|(state, _)| state.clone())
}

thread_local! {
    /// The wallpapers read so far, by path; `None` for one that can't be read.
    static WALLPAPERS: RefCell<HashMap<PathBuf, Option<gtk::gdk_pixbuf::Pixbuf>>> =
        RefCell::new(HashMap::new());
}

/// The picture the wallpaper settings show on every output, if one is known: the current picture
/// of its collection, or its image.
pub(super) fn wallpaper_picture(ctx: &Ctx) -> Option<PathBuf> {
    let registry = Registry::builtin();
    let text = |key| match ctx.model().view(registry.get(key)?, None)?.value {
        Value::Text(text) if !text.is_empty() => Some(text),
        _ => None,
    };
    if text(COLLECTION).is_some() {
        let state = ctx.model().state()?;
        return wallpaper::current(&state.paths().wallpaper_link(None));
    }
    text(IMAGE).map(|image| wallpaper::expand(&image, Some(&glib::home_dir())))
}

/// Draws `picture` in `area` as the top (or with `bottom`, the bottom) of a screen as wide as
/// the area, which the picture covers as swaybg's `fill` does; the area is hidden, so the
/// stand-in shows, without a picture or while it is read.
pub(super) fn draw_wallpaper(area: &gtk::DrawingArea, picture: Option<&Path>, bottom: bool) {
    let Some(path) = picture.map(Path::to_path_buf) else {
        area.set_visible(false);
        return;
    };
    let draw = move |area: &gtk::DrawingArea, pixbuf: Option<gtk::gdk_pixbuf::Pixbuf>| {
        let Some(pixbuf) = pixbuf else {
            area.set_visible(false);
            return;
        };
        area.set_visible(true);
        area.set_draw_func(move |_, cairo, width, height| {
            let (width, height) = (f64::from(width), f64::from(height));
            // A screen of 16:9 as wide as the area.
            let screen = width * 9.0 / 16.0;
            let (picture_width, picture_height) = (
                f64::from(pixbuf.width().max(1)),
                f64::from(pixbuf.height().max(1)),
            );
            let scale = (width / picture_width).max(screen / picture_height);
            let x = (width - picture_width * scale) / 2.0;
            let mut y = (screen - picture_height * scale) / 2.0;
            if bottom {
                y += height - screen;
            }
            cairo.translate(x, y);
            cairo.scale(scale, scale);
            cairo.set_source_pixbuf(&pixbuf, 0.0, 0.0);
            let _ = cairo.paint();
        });
        area.queue_draw();
    };
    let known = WALLPAPERS.with(|cache| cache.borrow().get(&path).cloned());
    if let Some(pixbuf) = known {
        draw(area, pixbuf);
        return;
    }
    area.set_visible(false);
    let weak = area.downgrade();
    glib::spawn_future_local(async move {
        let read = {
            let path = path.clone();
            gio::spawn_blocking(move || {
                let pixbuf =
                    gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&path, 1920, 1920, true).ok()?;
                Some((
                    pixbuf.width(),
                    pixbuf.height(),
                    pixbuf.rowstride(),
                    pixbuf.has_alpha(),
                    pixbuf.read_pixel_bytes(),
                ))
            })
            .await
        };
        let pixbuf = read
            .ok()
            .flatten()
            .map(|(width, height, stride, alpha, bytes)| {
                gtk::gdk_pixbuf::Pixbuf::from_bytes(
                    &bytes,
                    gtk::gdk_pixbuf::Colorspace::Rgb,
                    alpha,
                    8,
                    width,
                    height,
                    stride,
                )
            });
        WALLPAPERS.with(|cache| cache.borrow_mut().insert(path, pixbuf.clone()));
        if let Some(area) = weak.upgrade() {
            draw(&area, pixbuf);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_battery_is_in_the_lowest_state_above_its_charge() {
        let definition = serde_json::json!({"states": {"good": 95, "warning": 30, "critical": 10}});
        let definition = definition.as_object();
        assert_eq!(battery_state(definition, "80%"), Some("good".to_owned()));
        assert_eq!(
            battery_state(definition, "x 25 %"),
            Some("warning".to_owned())
        );
        assert_eq!(battery_state(definition, "100%"), None);
        assert_eq!(battery_state(None, "80%"), None);
    }
}
