// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The presets as cards above the colors: each card shows a small bar in the preset's look, its
//! name and a few words. The small bar is built and drawn as the preview is: the first of the
//! user's own items as waybar's widgets ([`mimic::mini_bar`]), with the stylesheet the look
//! would give waybar (the user's own stylesheet included), confined to the card, at its real
//! size and scaled down, over the wallpaper. A click sets the preset's style (colors, shape, sizes, corners, the
//! space around the bar) at once and never touches the items; a note offers _Undo_, and Ctrl+Z
//! works too. The card of the preset whose values the bar has is marked _In use_; once a value
//! changes, no card is. _Glass_ needs SwayFX for its blur and shows only there.

use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bar::style::{
    OWN, PRESET_KEYS, PRESETS, Preset, extra_css_active, preset_values,
};
use sway_things_core::registry::{Value, bar as keys};

use std::cell::RefCell;
use std::fmt::Write as _;

use sway_things_core::bar::Modules;

use super::preview::fresh_class;
use super::toast::Toast;
use super::{Ctx, follow, mimic};
use crate::i18n::{Msg, msg, tr, trf};

/// Said once a preset is applied.
const APPLIED: Msg = msg!("Applied the look “{preset}”. Your items stay as they are.");
/// The card in use.
const IN_USE: Msg = msg!("In use");
/// Said once a look turned the user's own stylesheet off.
const OWN_OFF: Msg =
    msg!("Applied the look “{preset}”. Your own stylesheet is off now, so this look shows.");
/// Said once the user's own style is back.
const OWN_ON: Msg = msg!("Your own stylesheet is on again.");

/// The user's own stylesheet as a look: their file alone, on ([`OWN`]).
static YOUR_STYLE: Preset = Preset {
    id: OWN,
    title: msg!("Your style"),
    description: msg!("Your own stylesheet"),
    swayfx: false,
};
/// How wide a card's bar is drawn at least before it is scaled down, in pixels; it fills the
/// card.
const BAR_WIDTH: i32 = 400;
/// How much smaller a card's bar is shown.
const SCALE: f64 = 0.55;

/// A card's small bar: where it goes, its class, and the look it shows.
struct Thumb {
    holder: gtk::Box,
    class: String,
    preset: &'static Preset,
}

/// The cards and the note after a click.
#[derive(Debug, Clone)]
pub struct Presets {
    root: gtk::Box,
}

impl Presets {
    /// The cards, following the settings.
    pub fn new(ctx: &Rc<Ctx>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("st-bar-presets");
        let title = gtk::Label::new(Some(tr(msg!("Start from a look"))));
        title.set_xalign(0.0);
        title.add_css_class("heading");
        root.append(&title);
        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_homogeneous(true);
        flow.set_min_children_per_line(1);
        flow.set_max_children_per_line(3);
        flow.set_column_spacing(8);
        flow.set_row_spacing(8);
        flow.update_property(&[gtk::accessible::Property::Label(tr(msg!("Looks")))]);
        root.append(&flow);
        let weak = Rc::downgrade(ctx);
        let toast = Toast::new(move || {
            if let Some(ctx) = weak.upgrade() {
                ctx.undo();
            }
        });
        root.append(toast.widget());
        let provider = mimic::provider_for(&root);
        let swayfx = ctx
            .model()
            .compositor()
            .is_some_and(sway_things_core::ipc::Compositor::is_swayfx);
        let picture = mimic::wallpaper_picture(ctx);
        let mut cards = Vec::new();
        let mut thumbs = Vec::new();
        // The user's own style first, shown while they have a stylesheet of their own.
        let looks = std::iter::once(&YOUR_STYLE)
            .chain(PRESETS.iter().filter(|preset| swayfx || !preset.swayfx));
        let mut own_card = None;
        for preset in looks {
            let class = fresh_class("st-wb-look");
            let card = card(ctx, preset, &toast, swayfx);
            mimic::draw_wallpaper(&card.wallpaper, picture.as_deref(), false);
            let child = gtk::FlowBoxChild::new();
            child.set_focusable(false);
            child.set_child(Some(&card.button));
            flow.append(&child);
            let holder = card.holder.clone();
            if preset.id == OWN {
                own_card = Some((child, card.description.clone()));
            }
            cards.push((preset.id, card));
            thumbs.push(Thumb {
                holder,
                class,
                preset,
            });
        }
        // The small bars follow the items and the user's stylesheet; rebuilt when they change.
        let drawn = RefCell::new(String::new());
        let weak = Rc::downgrade(ctx);
        follow(ctx.model(), &root, move || {
            let Some(ctx) = weak.upgrade() else { return };
            let extra = ctx
                .model()
                .state()
                .and_then(|state| sway_things_core::bar::style::extra_css(&state));
            if let Some((child, description)) = &own_card {
                child.set_visible(extra.is_some());
                if let Some(path) = &extra {
                    // The file's name, the whole path in the tooltip.
                    let name = std::path::Path::new(path)
                        .file_name()
                        .map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned());
                    description.set_text(&name);
                    description.set_tooltip_text(Some(&sway_things_core::bar::tilde(
                        std::path::Path::new(path),
                    )));
                }
            }
            draw_thumbs(&ctx, &thumbs, &provider, &drawn);
            let current = ctx.model().bar_preset();
            for (id, card) in &cards {
                card.select(current == Some(*id));
            }
            flow.set_sensitive(ctx.locked(keys::THEME).is_none() && ctx.writable());
        });
        Self { root }
    }

    /// The widget to show.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }
}

/// The integer `key` of `preset`, else `default`.
fn preset_integer(preset: &Preset, key: &str, default: i64) -> i64 {
    preset_values(preset.id)
        .iter()
        .find(|(preset_key, _)| *preset_key == key)
        .and_then(|(_, value)| match value {
            Value::Integer(number) => Some(*number),
            _ => None,
        })
        .unwrap_or(default)
}

/// Builds the small bar of each card from the first of the user's items and loads the rules of
/// every look, each confined to its card; only when the items or the rules changed.
fn draw_thumbs(ctx: &Ctx, thumbs: &[Thumb], provider: &gtk::CssProvider, drawn: &RefCell<String>) {
    let modules = ctx
        .model()
        .bar_modules()
        .and_then(Result::ok)
        .unwrap_or_default();
    let take = |key, count: usize| ctx.list(key).into_iter().take(count).collect::<Vec<_>>();
    let left = take(keys::MODULES_LEFT, 2);
    let center = take(keys::MODULES_CENTER, 1);
    let right: Vec<String> = {
        let all = ctx.list(keys::MODULES_RIGHT);
        all[all.len().saturating_sub(2)..].to_vec()
    };
    let mut rules = String::new();
    for thumb in thumbs {
        rules.push_str(&mimic::reset(&thumb.class));
        if let Some(css) = ctx
            .model()
            .bar_preset_css(Some(thumb.preset.id), &thumb.class)
        {
            rules.push_str(&css.css);
        }
    }
    let _ = writeln!(
        rules,
        ".st-bar-scale {{ transform: scale({SCALE}); transform-origin: 0 0; }}"
    );
    let key = format!("{left:?}{center:?}{right:?}{}{rules}", modules.text());
    if *drawn.borrow() == key {
        return;
    }
    drawn.replace(key);
    provider.load_from_string(&rules);
    for thumb in thumbs {
        fill_thumb(ctx, thumb, &modules, [&left, &center, &right]);
    }
}

/// Puts the small bar of `thumb` with the items `lanes` in its place: in the sizes of its look,
/// or for the user's own style, in the sizes set now.
fn fill_thumb(ctx: &Ctx, thumb: &Thumb, modules: &Modules, lanes: [&[String]; 3]) {
    let integer = |key: &str, default| {
        let number = if thumb.preset.id == OWN {
            match ctx.value(key) {
                Some(Value::Integer(number)) => number,
                _ => default,
            }
        } else {
            preset_integer(thumb.preset, key, default)
        };
        i32::try_from(number.clamp(0, 200)).unwrap_or(0)
    };
    let bar = mimic::mini_bar(
        modules,
        lanes,
        &thumb.class,
        integer(keys::SPACING, 0),
        integer(keys::HEIGHT, 30),
    );
    bar.set_margin_top(integer(keys::MARGIN_TOP, 0));
    bar.set_margin_start(integer(keys::MARGIN_LEFT, 0));
    bar.set_margin_end(integer(keys::MARGIN_RIGHT, 0));
    bar.set_hexpand(true);
    while let Some(child) = thumb.holder.first_child() {
        thumb.holder.remove(&child);
    }
    thumb.holder.append(&bar);
}

/// A card of a look and its parts.
struct Card {
    button: gtk::Button,
    /// The _In use_ pill and the check badge over the small bar.
    pill: gtk::Box,
    badge: gtk::Box,
    title: gtk::Label,
    /// Where its small bar goes, and the wallpaper under it.
    holder: gtk::Box,
    wallpaper: gtk::DrawingArea,
    description: gtk::Label,
}

impl Card {
    /// Marks the card as the look in use, or not: an accent outline, a check badge, a bold
    /// title, an _In use_ pill, and the state for screen readers.
    fn select(&self, used: bool) {
        self.pill.set_visible(used);
        self.badge.set_visible(used);
        if used {
            self.button.add_css_class("st-current");
            self.title.add_css_class("st-current");
        } else {
            self.button.remove_css_class("st-current");
            self.title.remove_css_class("st-current");
        }
        self.button
            .update_state(&[gtk::accessible::State::Pressed(if used {
                gtk::AccessibleTristate::True
            } else {
                gtk::AccessibleTristate::False
            })]);
        self.button
            .update_state(&[gtk::accessible::State::Selected(Some(used))]);
    }
}

/// The small bar of a card of `preset`: the box it goes in at real size, scaled down by the
/// rule of `.st-bar-scale` and cut to the card, over the wallpaper's area; with the check badge
/// of the look in use. Returns the thumbnail, the box, the area and the badge.
fn thumbnail(preset: &Preset) -> (gtk::Overlay, gtk::Box, gtk::DrawingArea, gtk::Box) {
    // At real size in `holder`, scaled down by the rule of `.st-bar-scale`, and cut to the
    // card by `clip`, over a slice of the wallpaper.
    let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
    holder.add_css_class("st-bar-scale");
    holder.set_halign(gtk::Align::Start);
    holder.set_valign(gtk::Align::Start);
    let clip = gtk::ScrolledWindow::new();
    clip.set_policy(gtk::PolicyType::External, gtk::PolicyType::External);
    clip.set_has_frame(false);
    clip.set_can_target(false);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "small pixel sizes, far below i32::MAX"
    )]
    let scaled = |pixels: i32| (f64::from(pixels) * SCALE).round() as i32;
    let height = i32::try_from(
        (preset_integer(preset, keys::HEIGHT, 30) + preset_integer(preset, keys::MARGIN_TOP, 0))
            .clamp(0, 400),
    )
    .unwrap_or(30);
    clip.set_min_content_width(scaled(BAR_WIDTH));
    clip.set_min_content_height(scaled(height));
    clip.set_max_content_height(scaled(height));
    clip.set_child(Some(&holder));
    // The bar is as wide as the card once scaled down.
    let width = std::cell::Cell::new(0);
    let fill = holder.downgrade();
    clip.add_tick_callback(move |clip, _| {
        let now = clip.width();
        if now > 0 && width.replace(now) != now {
            if let Some(holder) = fill.upgrade() {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a widget's width, far below i32::MAX"
                )]
                holder.set_size_request((f64::from(now) / SCALE).round() as i32, -1);
            }
        }
        glib::ControlFlow::Continue
    });
    let wallpaper = gtk::DrawingArea::new();
    let stage = gtk::Overlay::new();
    stage.add_css_class("st-bar-backdrop");
    stage.add_css_class("st-bar-thumb");
    stage.set_overflow(gtk::Overflow::Hidden);
    stage.set_child(Some(&wallpaper));
    let over = gtk::Box::new(gtk::Orientation::Vertical, 0);
    over.append(&clip);
    stage.add_overlay(&over);
    stage.set_measure_overlay(&over, true);
    // The check badge of the look in use, over the small bar.
    let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    badge.add_css_class("st-bar-check");
    badge.set_halign(gtk::Align::End);
    badge.set_valign(gtk::Align::Start);
    let tick = gtk::Image::from_icon_name("object-select-symbolic");
    tick.set_accessible_role(gtk::AccessibleRole::Presentation);
    badge.append(&tick);
    badge.set_visible(false);
    stage.add_overlay(&badge);
    (stage, holder, wallpaper, badge)
}

/// The card of `preset`.
fn card(ctx: &Rc<Ctx>, preset: &'static Preset, toast: &Toast, swayfx: bool) -> Card {
    let (thumb, holder, wallpaper, badge) = thumbnail(preset);
    let title = gtk::Label::new(Some(tr(preset.title)));
    title.set_xalign(0.0);
    title.add_css_class("heading");
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    pill.add_css_class("st-bar-in-use");
    pill.set_valign(gtk::Align::Center);
    pill.append(&gtk::Label::new(Some(tr(IN_USE))));
    pill.set_visible(false);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    heading.append(&title);
    heading.append(&pill);
    let description = gtk::Label::new(Some(tr(preset.description)));
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.set_lines(2);
    description.set_max_width_chars(22);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    description.add_css_class(crate::style::DIM);
    description.add_css_class("st-row-detail");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&thumb);
    content.append(&heading);
    content.append(&description);
    let button = gtk::Button::new();
    button.add_css_class("st-bar-preset");
    button.set_child(Some(&content));
    button.update_property(&[
        gtk::accessible::Property::Label(tr(preset.title)),
        gtk::accessible::Property::Description(tr(preset.description)),
    ]);
    let weak = Rc::downgrade(ctx);
    let toast = toast.clone();
    button.connect_clicked(move |_| {
        if let Some(ctx) = weak.upgrade() {
            let was_own = ctx
                .model()
                .state()
                .is_some_and(|state| extra_css_active(&state).is_some());
            apply(&ctx, preset, swayfx);
            let said = if preset.id == OWN {
                tr(OWN_ON).to_owned()
            } else if was_own {
                trf(OWN_OFF, &[("preset", &tr(preset.title))])
            } else {
                trf(APPLIED, &[("preset", &tr(preset.title))])
            };
            toast.show(&said);
        }
    });
    Card {
        button,
        pill,
        badge,
        title,
        holder,
        wallpaper,
        description,
    }
}

/// Stores the values of `preset` and resets the other settings a preset may set, as one step
/// of Ctrl+Z.
fn apply(ctx: &Ctx, preset: &Preset, swayfx: bool) {
    if preset.id == OWN {
        // Only the user's own stylesheet, on again.
        ctx.remember(ctx.snapshot(&[keys::THEME, keys::EXTRA_CSS_ON]));
        ctx.model()
            .set(keys::THEME, None, Value::Choice("plain".to_owned()));
        ctx.model().set(keys::EXTRA_CSS_ON, None, Value::Bool(true));
        return;
    }
    // Blur needs SwayFX; on sway the rest of the preset still applies.
    let mut touched: Vec<&'static str> = PRESET_KEYS
        .iter()
        .copied()
        .filter(|key| swayfx || *key != keys::BLUR)
        .collect();
    // A look shows as it is: the user's own stylesheet goes off, and Ctrl+Z turns it on again.
    let own = ctx
        .model()
        .state()
        .is_some_and(|state| extra_css_active(&state).is_some());
    if own {
        touched.push(keys::EXTRA_CSS_ON);
    }
    ctx.remember(ctx.snapshot(&touched));
    if own {
        ctx.model()
            .set(keys::EXTRA_CSS_ON, None, Value::Bool(false));
        touched.pop();
    }
    let values = preset_values(preset.id);
    for key in touched {
        match values.iter().find(|(preset_key, _)| *preset_key == key) {
            Some((_, value)) => ctx.model().set(key, None, value.clone()),
            None => ctx.model().reset(key, None),
        }
    }
}
