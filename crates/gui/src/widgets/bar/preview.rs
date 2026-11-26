// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview at the top of the Bar page: the bar the settings set up, with sample values (the
//! clock shows the time now), over a slice of the wallpaper (the picture of the Wallpaper page,
//! else a stand-in). Each item opens its settings. Below it, the same bar in words, which is also
//! the preview's accessible description, so a screen reader gets everything the picture shows.
//!
//! For waybar, the preview builds the widgets waybar builds and draws them with the stylesheet
//! waybar gets, the user's own included ([`super::mimic`]). It is a close approximation, and
//! says so: waybar draws with GTK 3, so spacing can differ by a pixel or two, and a command of
//! the user's never runs here. With swaybar it shows what swaybar shows: the workspaces and the
//! text of the status command, in the colors of its look.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bar::catalog::{self, Works};
use sway_things_core::bar::preview::Extra;
use sway_things_core::bar::style::{self, Look};
use sway_things_core::bar::{Backend, Modules, groups, kind_of, modules::members};
use sway_things_core::command::Color;
use sway_things_core::registry::{Value, bar as keys};

use super::{Ctx, dialog, follow, mimic};
use crate::i18n::{Msg, msg, tr, trf};

/// A preview item that opens its settings.
pub(super) const OPENS: Msg = msg!("{item}: {value}. Opens its settings");
/// The summary of a lane.
const LANE: Msg = msg!("{lane}: {items}.");
/// An item with what it shows.
const WITH_VALUE: Msg = msg!("{item} ({value})");
/// A lane without items.
const NOTHING: Msg = msg!("nothing");

thread_local! {
    /// The number of the next preview, for the class its colors are given by.
    static NEXT: Cell<u32> = const { Cell::new(0) };
}

/// A fresh class name for a preview's colors.
pub(crate) fn fresh_class(prefix: &str) -> String {
    let number = NEXT.with(|next| {
        let number = next.get();
        next.set(number.wrapping_add(1));
        number
    });
    format!("{prefix}-{number}")
}

/// A style provider for the whole display, removed again when `widget` is destroyed.
pub(crate) fn provider_for(widget: &impl IsA<gtk::Widget>) -> gtk::CssProvider {
    let provider = gtk::CssProvider::new();
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
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

/// How a bar is drawn in a preview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shape {
    /// Its height in pixels; 0 fits the items.
    pub height: i64,
    /// The space between items.
    pub spacing: i64,
    /// The margins around it: top, right, bottom, left.
    pub margins: [i64; 4],
    /// How much smaller than the real bar it is drawn.
    pub scale: f64,
}

/// The CSS that draws a bar of `look` and `shape` inside the widgets of class `class`: the bar
/// (`.st-bar-strip`), its items (`.st-bar-item`) and the focused workspace (`.st-bar-focused`).
/// Every value is a number, a color or an escaped string.
pub(crate) fn css(class: &str, look: &Look, shape: Shape) -> String {
    #[expect(clippy::cast_precision_loss, reason = "pixels, far below 2^52")]
    let px = |value: i64| format!("{:.1}px", value as f64 * shape.scale);
    let colors = look.colors;
    let text = style::css_color(colors.text, 1.0);
    let background = style::css_color(colors.background, look.opacity);
    let mut font = String::new();
    match look.font.as_ref() {
        Some(described) => {
            let families = style::css_font_family(&described.family);
            if !families.is_empty() {
                let _ = write!(font, "font-family: {families};");
            }
            let size =
                described.size.map_or(
                    13.0,
                    |(size, unit)| if unit == "px" { size } else { size * 4.0 / 3.0 },
                );
            let _ = write!(font, "font-size: {:.1}px;", size * shape.scale);
            if let Some(weight) = described.weight {
                let _ = write!(font, "font-weight: {weight};");
            }
            if described.italic {
                font.push_str("font-style: italic;");
            }
        }
        None => {
            let _ = write!(font, "font-size: {:.1}px;", 13.0 * shape.scale);
        }
    }
    let [top, right, bottom, left] = shape.margins;
    let height = if shape.height > 0 {
        format!("min-height: {};", px(shape.height))
    } else {
        String::new()
    };
    let radius = px(look.radius.min(999));
    let (strip_background, item_background, border) = if look.pills {
        (
            "transparent".to_owned(),
            background.clone(),
            format!("1px solid {}", style::css_color(colors.text, 0.1)),
        )
    } else {
        (
            background.clone(),
            "transparent".to_owned(),
            "none".to_owned(),
        )
    };
    let strip_radius = if look.pills {
        "0".to_owned()
    } else {
        radius.clone()
    };
    // In a pill, the focused workspace sits 3 px inside it and is wider, as in the stylesheet.
    let (focused_margin, focused_padding) = if look.pills {
        (px(3), px(look.padding + 4))
    } else {
        ("0".to_owned(), px((look.padding / 2).max(2)))
    };
    format!(
        ".{class} .st-bar-strip {{ background-color: {strip_background}; color: {text}; \
         border-radius: {strip_radius}; margin: {} {} {} {}; {height} {font} }}\n\
         .{class} .st-bar-strip label {{ color: {text}; }}\n\
         .{class} .st-bar-strip button.st-bar-item {{ background: {item_background}; \
         color: {text}; border: {border}; border-radius: {radius}; box-shadow: none; \
         min-height: 0; min-width: 0; padding: 0 {}; margin: 0 {}; }}\n\
         .{class} .st-bar-strip button.st-bar-item:hover {{ background-color: {hover}; }}\n\
         .{class} .st-bar-strip .st-bar-focused {{ background-color: {focused}; \
         color: {on_focused}; border-radius: {radius}; margin: {focused_margin} 0; \
         padding: 0 {focused_padding}; font-weight: bold; }}\n\
         .{class} .st-bar-strip .st-bar-workspace {{ padding: 0 {}; font-weight: bold; }}\n",
        px(top),
        px(right),
        px(bottom),
        px(left),
        px(look.padding),
        px(shape.spacing.max(0) / 2 + i64::from(look.pills) * 2),
        px((look.padding / 2).max(2)),
        hover = style::css_color(colors.hover, 1.0),
        focused = style::css_color(colors.focused, 1.0),
        on_focused = style::css_color(style::text_on(colors.focused, colors.text), 1.0),
    )
}

/// The look of swaybar's own colors (`default_bar_config` in sway's `config/bar.c`).
fn swaybar_look(font: Option<style::CssFont>) -> Look {
    Look {
        colors: style::Colors {
            background: Color::rgb(0, 0, 0),
            text: Color::rgb(0xff, 0xff, 0xff),
            hover: Color::rgb(0x5f, 0x67, 0x6a),
            focused: Color::rgb(0x28, 0x55, 0x77),
            warning: Color::rgb(0xf0, 0x93, 0x2b),
            critical: Color::rgb(0xf5, 0x3c, 0x3c),
            urgent: Color::rgb(0x90, 0x00, 0x00),
        },
        opacity: 1.0,
        pills: false,
        padding: 6,
        radius: 0,
        font,
    }
}

/// The name an item has on the page: the catalog's title, with the instance (`bat2`), or the
/// name after `custom/` or `group/`.
pub(crate) fn item_title(_modules: &Modules, name: &str) -> String {
    let kind = kind_of(name);
    let instance = name.split_once('#').map(|(_, instance)| instance);
    let base = if let Some(custom) = kind.strip_prefix("custom/") {
        sway_things_core::bar::templates::find(custom).map_or_else(
            || custom.to_owned(),
            |template| tr(template.title).to_owned(),
        )
    } else if kind.starts_with("group/") {
        return group_title(name);
    } else {
        catalog::find(name).map_or_else(|| kind.to_owned(), |module| tr(module.title).to_owned())
    };
    match instance {
        Some(instance) => format!("{base} ({instance})"),
        None => base,
    }
}

/// The name of group `name` on the page: its name after `group/` in words.
pub(crate) fn group_title(name: &str) -> String {
    groups::title(name)
}

/// The time now: hour, minute, second.
pub(crate) fn now() -> (u32, u32, u32) {
    glib::DateTime::now_local().map_or((0, 0, 0), |now| {
        (
            u32::try_from(now.hour()).unwrap_or(0),
            u32::try_from(now.minute()).unwrap_or(0),
            u32::try_from(now.second()).unwrap_or(0),
        )
    })
}

/// What item `name` shows in the preview, if it shows text of its own: not a command of the
/// user's, whose output only a run tells, nor the workspaces, which the preview draws.
pub(crate) fn item_sample(modules: &Modules, name: &str) -> Option<String> {
    if name.starts_with("custom/") || kind_of(name) == "sway/workspaces" {
        return None;
    }
    catalog::sample(name, modules.definition(name).as_ref(), now())
}

/// `text` for words: without the icons of icon fonts (the private use areas of Unicode, which
/// a screen reader can't say), and with single spaces.
pub(crate) fn spoken(text: &str) -> String {
    text.chars()
        .filter(|char| {
            !matches!(u32::from(*char), 0xE000..=0xF8FF | 0xF_0000..=0xF_FFFD | 0x10_0000..=0x10_FFFD)
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Why item `name` can't work here, if it can't.
pub(crate) fn item_missing(name: &str) -> Option<String> {
    match catalog::find(name)?.need.check() {
        Works::Yes => None,
        Works::No(need) => Some(tr(need.missing()).to_owned()),
    }
}

/// The preview and its summary.
#[derive(Debug, Clone)]
pub struct Preview {
    root: gtk::Box,
}

impl Preview {
    /// The preview, following the settings.
    pub fn new(ctx: &Rc<Ctx>) -> Self {
        let class = fresh_class("st-bar-preview");
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("st-bar-preview");
        root.add_css_class(&class);
        // The wallpaper, drawn under the bar and the slice beside it.
        let stage = gtk::Overlay::new();
        stage.add_css_class("st-bar-backdrop");
        stage.set_overflow(gtk::Overflow::Hidden);
        let wallpaper = gtk::DrawingArea::new();
        stage.set_child(Some(&wallpaper));
        let backdrop = gtk::Box::new(gtk::Orientation::Vertical, 0);
        stage.add_overlay(&backdrop);
        stage.set_measure_overlay(&backdrop, true);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_propagate_natural_height(true);
        scroller.set_propagate_natural_width(true);
        scroller.set_has_frame(false);
        // A slice of the wallpaper below a bar at the top, above one at the bottom.
        let above = gtk::Box::new(gtk::Orientation::Vertical, 0);
        above.add_css_class("st-bar-slice");
        let below = gtk::Box::new(gtk::Orientation::Vertical, 0);
        below.add_css_class("st-bar-slice");
        backdrop.append(&above);
        backdrop.append(&scroller);
        backdrop.append(&below);
        // The bar is often wider than the page: a scrollbar of its own at the bottom edge of the
        // preview, never over the items, and it pans with a drag and the wheel too.
        let scrollbar =
            gtk::Scrollbar::new(gtk::Orientation::Horizontal, Some(&scroller.hadjustment()));
        scrollbar.add_css_class("st-bar-scrollbar");
        scrollbar.set_visible(false);
        backdrop.append(&scrollbar);
        let vertical_bar = Rc::new(Cell::new(false));
        pan(&scroller, &vertical_bar);
        let strip = gtk::CenterBox::new();
        strip.add_css_class("st-bar-strip");
        strip.set_accessible_role(gtk::AccessibleRole::Group);
        strip.update_property(&[gtk::accessible::Property::Label(tr(msg!("Bar preview")))]);
        // waybar's window: `window#waybar` of its stylesheet.
        let bar_class = fresh_class("st-wb");
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        bar.add_css_class(&bar_class);
        // A picture of the user's bar, in the colors of their stylesheet (the contrast checks
        // leave it out).
        bar.add_css_class("st-bar-picture");
        bar.set_accessible_role(gtk::AccessibleRole::Group);
        bar.update_property(&[gtk::accessible::Property::Label(tr(msg!("Bar preview")))]);
        let bar_lanes = gtk::CenterBox::new();
        bar_lanes.set_hexpand(true);
        bar.append(&bar_lanes);
        scroller.set_child(Some(&strip));
        root.append(&stage);
        let caption = gtk::Label::new(None);
        caption.set_xalign(0.0);
        caption.set_wrap(true);
        caption.add_css_class(crate::style::DIM);
        caption.add_css_class("st-row-detail");
        root.append(&caption);
        // The caption says when the bar is wider than the preview.
        let caption_text = Rc::new(RefCell::new(String::new()));
        let wider = Rc::new(Cell::new(false));
        let adjustment = scroller.hadjustment();
        {
            let (caption, caption_text, wider) =
                (caption.downgrade(), caption_text.clone(), wider.clone());
            let (scrollbar, vertical_bar) = (scrollbar.downgrade(), vertical_bar.clone());
            adjustment.connect_changed(move |adjustment| {
                let overflows = adjustment.upper() > adjustment.page_size() + 1.0;
                if let Some(scrollbar) = scrollbar.upgrade() {
                    scrollbar.set_visible(overflows && !vertical_bar.get());
                }
                if wider.replace(overflows) != overflows {
                    if let Some(caption) = caption.upgrade() {
                        set_caption(&caption, &caption_text.borrow(), overflows);
                    }
                }
            });
        }
        // The bar in words: the preview's accessible description, kept off the screen.
        let summary = gtk::Label::new(None);
        summary.add_css_class("st-bar-summary");
        summary.set_visible(false);
        root.append(&summary);
        let provider = provider_for(&root);
        let bar_provider = mimic::provider_for(&root);
        let weak = Rc::downgrade(ctx);
        let parts = Parts {
            wallpaper,
            shown_wallpaper: RefCell::new(None),
            bar,
            bar_class,
            bar_lanes,
            bar_provider,
            backdrop,
            slices: [above, below],
            scroller,
            vertical_bar,
            strip,
            caption,
            summary,
        };
        follow(ctx.model(), &root, move || {
            if let Some(ctx) = weak.upgrade() {
                let text = refresh(&ctx, &class, &provider, &parts);
                set_caption(&parts.caption, &text, wider.get());
                caption_text.replace(text);
            }
        });
        Self { root }
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }
}

/// The widgets of the preview.
struct Parts {
    /// The wallpaper under the bar.
    wallpaper: gtk::DrawingArea,
    /// The picture it draws and whether at the bottom, once drawn.
    shown_wallpaper: RefCell<Option<(Option<PathBuf>, bool)>>,
    /// waybar's window, its class and the box of its lanes.
    bar: gtk::Box,
    bar_class: String,
    bar_lanes: gtk::CenterBox,
    /// The stylesheet of waybar's window.
    bar_provider: gtk::CssProvider,
    backdrop: gtk::Box,
    /// The wallpaper above and below the bar.
    slices: [gtk::Box; 2],
    scroller: gtk::ScrolledWindow,
    /// Whether the bar stands at the left or right, so it scrolls up and down.
    vertical_bar: Rc<Cell<bool>>,
    strip: gtk::CenterBox,
    caption: gtk::Label,
    summary: gtk::Label,
}

/// How far one step of a mouse wheel moves the bar, in pixels.
const WHEEL_STEP: f64 = 60.0;
/// How far the pointer moves before a press on the preview becomes a drag that pans it.
const DRAG_THRESHOLD: f64 = 8.0;

/// Lets the bar in `scroller` pan sideways with a drag (mouse or touch; a press that moves less
/// than [`DRAG_THRESHOLD`] stays a click on the item under it) and with a vertical wheel, since
/// there is nothing to scroll up and down; the item with the keyboard focus scrolls into view.
/// A bar at the left or right (`vertical`) keeps the scrolled window's own scrolling.
fn pan(scroller: &gtk::ScrolledWindow, vertical: &Rc<Cell<bool>>) {
    let drag = gtk::GestureDrag::new();
    drag.set_name(Some("st-bar-pan"));
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    let start = Rc::new(Cell::new(0.0));
    let panning = Rc::new(Cell::new(false));
    {
        let (start, panning, scroller) = (start.clone(), panning.clone(), scroller.downgrade());
        drag.connect_drag_begin(move |_, _, _| {
            panning.set(false);
            if let Some(scroller) = scroller.upgrade() {
                start.set(scroller.hadjustment().value());
            }
        });
    }
    {
        let (vertical, scroller) = (vertical.clone(), scroller.downgrade());
        let panning = panning.clone();
        drag.connect_drag_update(move |gesture, offset_x, _| {
            let Some(scroller) = scroller.upgrade() else {
                return;
            };
            if vertical.get() {
                return;
            }
            if !panning.get() {
                if offset_x.abs() < DRAG_THRESHOLD {
                    return;
                }
                // From now on it is a drag: the item under the pointer gets no click.
                panning.set(true);
                gesture.set_state(gtk::EventSequenceState::Claimed);
            }
            scroller.hadjustment().set_value(start.get() - offset_x);
        });
    }
    scroller.add_controller(drag);
    let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    wheel.set_name(Some("st-bar-wheel"));
    wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (vertical, scroller) = (vertical.clone(), scroller.downgrade());
        wheel.connect_scroll(move |controller, _, delta_y| {
            let Some(scroller) = scroller.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if vertical.get() || delta_y == 0.0 {
                return glib::Propagation::Proceed;
            }
            // A wheel's steps, or a touchpad's pixels.
            let step = if controller.unit() == gtk::gdk::ScrollUnit::Wheel {
                WHEEL_STEP
            } else {
                1.0
            };
            let adjustment = scroller.hadjustment();
            adjustment.set_value(adjustment.value() + delta_y * step);
            glib::Propagation::Stop
        });
    }
    scroller.add_controller(wheel);
}

/// Shows `text` in `caption`, and that the bar is wider than the preview if `wider`.
fn set_caption(caption: &gtk::Label, text: &str, wider: bool) {
    if wider {
        caption.set_text(&format!("{text} {}", tr(WIDER)));
    } else {
        caption.set_text(text);
    }
}

/// Said when the bar is wider than the preview.
const WIDER: Msg = msg!("Scroll sideways to see all of it.");
/// The items of the bar, counted.
const COUNTED: Msg = msg!("{count} items: {left} left, {center} center, {right} right.");
/// Commands of the user's, which the preview doesn't run.
const COMMANDS: Msg = msg!("Your scripts don't run here: their items show a sample or their name.");
/// The user's own stylesheet couldn't be read.
const UNREADABLE: Msg = msg!("Your stylesheet {file} can't be read, so the preview leaves it out.");
/// Icons that need a font that isn't installed.
const NO_ICON_FONT: Msg =
    msg!("Its icons need Font Awesome or a Nerd Font, which isn't installed: Waybar shows boxes.");

/// Draws the preview of the bar the settings set up now; the caption to show.
#[expect(
    clippy::too_many_lines,
    reason = "the parts of one picture, read best in order"
)]
fn refresh(ctx: &Rc<Ctx>, class: &str, provider: &gtk::CssProvider, parts: &Parts) -> String {
    let Parts {
        wallpaper,
        shown_wallpaper,
        bar,
        bar_class,
        bar_lanes,
        bar_provider,
        backdrop,
        slices,
        scroller,
        strip,
        summary,
        ..
    } = parts;
    let backend = match ctx.value(keys::BACKEND) {
        Some(Value::Choice(id)) if id == "waybar" => Backend::Waybar,
        Some(Value::Choice(id)) if id == "swaybar" => Backend::Swaybar,
        _ => Backend::None,
    };
    let integer = |key| match ctx.value(key) {
        Some(Value::Integer(number)) => number,
        _ => 0,
    };
    let position = match ctx.value(keys::POSITION) {
        Some(Value::Choice(id)) => id,
        _ => "top".to_owned(),
    };
    let vertical = matches!(position.as_str(), "left" | "right") && backend != Backend::Swaybar;
    let Some(mut look) = ctx.model().bar_look(None) else {
        return String::new();
    };
    let theme = match ctx.value(keys::THEME) {
        Some(Value::Choice(id)) => id,
        _ => String::new(),
    };
    if backend == Backend::Swaybar && matches!(theme.as_str(), "waybar" | "plain") {
        look = swaybar_look(look.font.clone());
    }
    let waybar = backend != Backend::Swaybar;
    // The wallpaper under the bar, read again only when it changes.
    let picture = mimic::wallpaper_picture(ctx);
    let wanted = (picture, position == "bottom");
    if shown_wallpaper.borrow().as_ref() != Some(&wanted) {
        mimic::draw_wallpaper(wallpaper, wanted.0.as_deref(), wanted.1);
        shown_wallpaper.replace(Some(wanted));
    }
    let margins = [
        integer(keys::MARGIN_TOP),
        integer(keys::MARGIN_RIGHT),
        integer(keys::MARGIN_BOTTOM),
        integer(keys::MARGIN_LEFT),
    ];
    let mut extra = Extra::None;
    if waybar {
        provider.load_from_string("");
        let preview_css = ctx.model().bar_preview_css(bar_class);
        let css = preview_css
            .as_ref()
            .map_or("", |preview_css| preview_css.css.as_str());
        bar_provider.load_from_string(&format!(
            "{}{css}{}",
            mimic::reset(bar_class),
            mimic::focus_ring(bar_class)
        ));
        extra = preview_css.map_or(Extra::None, |preview_css| preview_css.extra);
    } else {
        bar_provider.load_from_string("");
        let shape = Shape {
            height: integer(keys::HEIGHT),
            spacing: 0,
            margins: [0; 4],
            scale: 1.0,
        };
        provider.load_from_string(&css(class, &look, shape));
    }
    // The bar at its edge of the stand-in desktop.
    let orientation = if vertical {
        gtk::Orientation::Vertical
    } else {
        gtk::Orientation::Horizontal
    };
    strip.set_orientation(orientation);
    bar.set_orientation(orientation);
    bar_lanes.set_orientation(orientation);
    for side in ["top", "bottom", "left", "right"] {
        if side == position {
            bar.add_css_class(side);
        } else {
            bar.remove_css_class(side);
        }
    }
    // waybar's margins keep the window off the edges; its height is the window's.
    let [top, right, bottom, left] =
        margins.map(|margin| i32::try_from(margin.clamp(0, 200)).unwrap_or(0));
    bar.set_margin_top(top);
    bar.set_margin_end(right);
    bar.set_margin_bottom(bottom);
    bar.set_margin_start(left);
    let height = i32::try_from(integer(keys::HEIGHT).clamp(0, 400)).unwrap_or(0);
    if vertical {
        bar.set_size_request(height.max(-1), -1);
    } else {
        bar.set_size_request(-1, if height > 0 { height } else { -1 });
    }
    let shown: &gtk::Widget = if waybar {
        bar.upcast_ref()
    } else {
        strip.upcast_ref()
    };
    if scroller.child().as_ref() != Some(shown) {
        scroller.set_child(Some(shown));
    }
    // Tab through the items: the focused one scrolls into view.
    if let Some(viewport) = scroller.child().and_downcast::<gtk::Viewport>() {
        viewport.set_scroll_to_focus(true);
    }
    backdrop.set_orientation(if vertical {
        gtk::Orientation::Horizontal
    } else {
        gtk::Orientation::Vertical
    });
    backdrop.set_size_request(-1, if vertical { 240 } else { -1 });
    let [above, below] = slices;
    above.set_visible(!vertical && position == "bottom");
    below.set_visible(!vertical && position != "bottom");
    parts.vertical_bar.set(vertical);
    // Across, the scrollbar of the preview stands for the scrolled window's own.
    scroller.set_policy(
        if vertical {
            gtk::PolicyType::Never
        } else {
            gtk::PolicyType::External
        },
        if vertical {
            gtk::PolicyType::Automatic
        } else {
            gtk::PolicyType::Never
        },
    );
    scroller.set_hexpand(!vertical);
    scroller.set_vexpand(vertical);
    let (halign, valign) = match position.as_str() {
        "bottom" => (gtk::Align::Fill, gtk::Align::End),
        "left" if vertical => (gtk::Align::Start, gtk::Align::Fill),
        "right" if vertical => (gtk::Align::End, gtk::Align::Fill),
        _ => (gtk::Align::Fill, gtk::Align::Start),
    };
    scroller.set_halign(halign);
    scroller.set_valign(valign);
    let modules = ctx
        .model()
        .bar_modules()
        .and_then(Result::ok)
        .unwrap_or_default();
    let lanes = [
        (keys::MODULES_LEFT, tr(msg!("Left")), "modules-left"),
        (keys::MODULES_CENTER, tr(msg!("Center")), "modules-center"),
        (keys::MODULES_RIGHT, tr(msg!("Right")), "modules-right"),
    ];
    let spacing = i32::try_from(integer(keys::SPACING).clamp(0, 100)).unwrap_or(0);
    let mut sentences = Vec::new();
    let mut boxes = Vec::new();
    let mut counts = Vec::new();
    let mut no_icon_font = false;
    let mut commands = false;
    let mut shown_items = Vec::new();
    for (key, lane, lane_class) in lanes {
        let items = gtk::Box::new(orientation, if waybar { spacing } else { 0 });
        if waybar {
            items.add_css_class(lane_class);
        }
        let mut words = Vec::new();
        let names = if waybar {
            ctx.list(key)
        } else {
            swaybar_items(ctx, key)
        };
        for name in names {
            let title = item_title(&modules, &name);
            let sample = if waybar {
                item_sample(&modules, &name)
            } else {
                swaybar_sample(ctx, &name)
            };
            let said = sample
                .as_deref()
                .map(spoken)
                .filter(|value| !value.is_empty());
            words.push(match &said {
                Some(value) if *value != title => {
                    trf(WITH_VALUE, &[("item", &title), ("value", value)])
                }
                _ => title.clone(),
            });
            if waybar {
                commands |= has_command(&modules, &name, 0);
                if let Some((widget, iconless)) = mimic::item(ctx, &modules, &name) {
                    no_icon_font |= iconless;
                    items.append(&widget);
                    shown_items.push((name.clone(), widget.upcast()));
                }
            } else {
                let (button, iconless) = item_button(ctx, &name, &title, sample.as_deref());
                no_icon_font |= iconless;
                items.append(&button);
            }
        }
        counts.push(words.len());
        if words.is_empty() {
            words.push(tr(NOTHING).to_owned());
        }
        sentences.push(trf(LANE, &[("lane", &lane), ("items", &words.join(", "))]));
        boxes.push(items);
    }
    ctx.set_shown(shown_items);
    let mut boxes = boxes.into_iter();
    let lanes_box = if waybar { bar_lanes } else { strip };
    lanes_box.set_start_widget(boxes.next().as_ref());
    lanes_box.set_center_widget(boxes.next().as_ref());
    lanes_box.set_end_widget(boxes.next().as_ref());
    let words = sentences.join(" ");
    summary.set_text(&words);
    shown.update_property(&[gtk::accessible::Property::Description(&words)]);
    let mut caption = tr(match backend {
        Backend::None => msg!("The bar Sway Things would set up, with sample values."),
        Backend::Swaybar => msg!("swaybar: workspaces and the status command's text."),
        Backend::Waybar => msg!("Sample values; select an item to change it."),
    })
    .to_owned();
    if waybar && commands {
        caption = format!("{caption} {}", tr(COMMANDS));
    }
    if waybar {
        let [left, center, right] = [0, 1, 2].map(|at| counts.get(at).copied().unwrap_or(0));
        caption = format!(
            "{caption} {}",
            trf(
                COUNTED,
                &[
                    ("count", &(left + center + right)),
                    ("left", &left),
                    ("center", &center),
                    ("right", &right),
                ],
            )
        );
    }
    if let Extra::Unreadable(path) = extra {
        let path = sway_things_core::bar::tilde(&path);
        caption = format!("{caption} {}", trf(UNREADABLE, &[("file", &path)]));
    }
    if no_icon_font {
        caption = format!("{caption} {}", tr(NO_ICON_FONT));
    }
    caption
}

/// Whether item `name`, or a member of it, shows a command of the user's.
fn has_command(modules: &Modules, name: &str, depth: u8) -> bool {
    if name.starts_with("custom/") {
        return true;
    }
    depth < 8
        && modules.definition(name).is_some_and(|definition| {
            members(&definition)
                .iter()
                .any(|member| has_command(modules, member, depth + 1))
        })
}

/// What swaybar shows in place of lane `key`: the workspaces and the binding mode on the left,
/// the status command's text on the right.
fn swaybar_items(ctx: &Ctx, key: &str) -> Vec<String> {
    match key {
        keys::MODULES_LEFT => vec!["sway/workspaces".to_owned(), "sway/mode".to_owned()],
        keys::MODULES_RIGHT => match ctx.value(keys::STATUS_COMMAND) {
            Some(Value::Text(command)) if !command.is_empty() => vec!["status".to_owned()],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// What swaybar shows for its item `name`.
fn swaybar_sample(ctx: &Ctx, name: &str) -> Option<String> {
    match name {
        "status" => match ctx.value(keys::STATUS_COMMAND) {
            Some(Value::Text(command)) => Some(command),
            _ => None,
        },
        "sway/mode" => Some("resize".to_owned()),
        _ => None,
    }
}

/// Whether `widget`'s font has every glyph of `text`.
fn drawable(widget: &impl IsA<gtk::Widget>, text: &str) -> bool {
    widget
        .as_ref()
        .create_pango_layout(Some(text))
        .unknown_glyphs_count()
        == 0
}

/// Whether `text` has an icon of an icon font (a character of Unicode's private use areas).
pub(super) fn has_icon_glyph(text: &str) -> bool {
    text.chars()
        .any(|char| matches!(u32::from(char), 0xE000..=0xF8FF | 0xF_0000..=0x10_FFFD))
}

/// The button of item `name` in the preview: the workspaces as numbers with the first one
/// focused, the tray as dots, else its sample text or its title. Where the sample's icon needs a
/// font that isn't installed, or there is no text, the item's symbolic icon stands in; says
/// whether a font was missing.
fn item_button(
    ctx: &Rc<Ctx>,
    name: &str,
    title: &str,
    sample: Option<&str>,
) -> (gtk::Button, bool) {
    let button = gtk::Button::new();
    button.add_css_class("st-bar-item");
    button.set_focus_on_click(false);
    let kind = catalog::find(name).map(|module| module.name);
    let mut iconless = false;
    let shown = match kind {
        Some("sway/workspaces") => {
            let workspaces = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            for (index, number) in ["1", "2", "3"].iter().enumerate() {
                let label = gtk::Label::new(Some(number));
                label.add_css_class(if index == 0 {
                    "st-bar-focused"
                } else {
                    "st-bar-workspace"
                });
                workspaces.append(&label);
            }
            button.set_child(Some(&workspaces));
            "1 2 3".to_owned()
        }
        Some("tray") => {
            let label = gtk::Label::new(Some("● ● ●"));
            button.set_child(Some(&label));
            tr(msg!("app icons")).to_owned()
        }
        _ => {
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
            // Items whose formats show an icon with their value get the item's symbol, in
            // place of an icon font's glyph, which may be missing.
            let valued = matches!(
                kind,
                Some(
                    "battery"
                        | "pulseaudio"
                        | "wireplumber"
                        | "network"
                        | "backlight"
                        | "bluetooth"
                        | "cpu"
                        | "memory"
                        | "temperature"
                        | "disk"
                        | "power-profiles-daemon"
                        | "idle_inhibitor"
                        | "keyboard-state"
                        | "mpris"
                        | "mpd"
                )
            );
            let (text, icon) = match sample {
                Some(sample) if has_icon_glyph(sample) => {
                    iconless |= !drawable(&button, sample);
                    if valued || iconless {
                        (spoken(sample), true)
                    } else {
                        (sample.to_owned(), false)
                    }
                }
                Some(sample) => (sample.to_owned(), valued),
                None => (title.to_owned(), name != "status"),
            };
            if icon {
                let image = gtk::Image::from_icon_name(catalog::icon(name));
                image.set_accessible_role(gtk::AccessibleRole::Presentation);
                content.append(&image);
            }
            if !text.is_empty() {
                content.append(&gtk::Label::new(Some(&text)));
            }
            button.set_child(Some(&content));
            if text.is_empty() {
                title.to_owned()
            } else {
                text
            }
        }
    };
    let described = trf(OPENS, &[("item", &title), ("value", &shown)]);
    button.set_tooltip_text(Some(&described));
    button.update_property(&[gtk::accessible::Property::Label(&described)]);
    if name == "status" {
        // swaybar's status text has no settings of its own here.
        button.set_sensitive(false);
        return (button, iconless);
    }
    let name = name.to_owned();
    let weak = Rc::downgrade(ctx);
    button.connect_clicked(move |button| {
        if let Some(ctx) = weak.upgrade() {
            dialog::open(&ctx, button.upcast_ref(), &name);
        }
    });
    (button, iconless)
}
