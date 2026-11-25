// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview of the displays, each a tile with its wallpaper placed like the output layout,
//! and what the edited display shows: its picture, collection or color, the fit and the color.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use sway_things_core::ipc::Client;
use sway_things_core::outputs::{Output, Rect};
use sway_things_core::registry::Value;
use sway_things_core::registry::wallpaper::{COLOR, FILL};
use sway_things_core::state::State;
use sway_things_core::wallpaper::{self, Collection, Moment};

use super::collections::describe;
use super::colors::{color_of, rgba, solid};
use super::images::{own_size_into, set_frame_color, thumbnail_into, when_readable};
use super::{FITS, Kind, Look, Timers, Ui};
use crate::i18n::{msg, tr, trf};

/// The size of a screen whose size isn't known, in pixels.
const SCREEN: (i32, i32) = (1920, 1080);

impl Ui {
    /// The card with the preview, what the edited display shows, the fit and the color.
    pub(super) fn current_card(self: &Rc<Self>) -> gtk::Box {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 14);
        card.add_css_class("st-wallpaper-card");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let heading = gtk::Label::new(Some(tr(msg!("Displays"))));
        heading.add_css_class("st-section-heading");
        heading.set_xalign(0.0);
        heading.set_hexpand(true);
        let same_label = gtk::Label::new(Some(tr(msg!("Same wallpaper on all displays"))));
        same_label.set_wrap(true);
        same_label.set_xalign(1.0);
        self.same
            .update_property(&[gtk::accessible::Property::Label(tr(msg!(
                "Same wallpaper on all displays"
            )))]);
        header.append(&heading);
        header.append(&same_label);
        header.append(&self.same);
        card.append(&header);

        card.append(&self.preview);

        let info = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&self.title);
        text.append(&self.detail);
        text.append(&self.more);
        text.append(&self.origin);
        info.append(&self.kind);
        info.append(&text);
        info.append(&self.next);
        info.append(&self.use_default);
        card.append(&info);
        card.append(&self.warning);

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let fits = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fits.add_css_class("linked");
        fits.update_property(&[gtk::accessible::Property::Label(tr(msg!("Fit")))]);
        for fit in &self.fits {
            fits.append(fit);
        }
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        controls.append(&fits);
        controls.append(&spacer);
        controls.append(&self.color);
        card.append(&controls);

        let weak = Rc::downgrade(self);
        self.same.connect_active_notify(move |switch| {
            if let Some(ui) = weak.upgrade() {
                ui.switch_same(switch.is_active());
            }
        });
        let weak = Rc::downgrade(self);
        self.next.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.next_picture();
            }
        });
        let weak = Rc::downgrade(self);
        self.use_default.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.use_default();
            }
        });
        for (fit, (id, _)) in self.fits.iter().zip(FITS) {
            let weak = Rc::downgrade(self);
            fit.connect_toggled(move |button| {
                if let (Some(ui), true) = (weak.upgrade(), button.is_active()) {
                    if !ui.syncing.get() {
                        ui.take_over();
                        ui.set(FILL, Value::Choice(id.to_owned()));
                    }
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.color.connect_rgba_notify(move |button| {
            if let Some(ui) = weak.upgrade() {
                if !ui.syncing.get() {
                    ui.take_over();
                    ui.set(COLOR, Value::Color(color_of(&button.rgba())));
                }
            }
        });
        card
    }

    /// The tiles of the displays, placed like the output layout.
    pub(super) fn refresh_preview(self: &Rc<Self>, outputs: &[Output], separate: bool) {
        let selected = self.selected.borrow().clone();
        let tiles = arrange(outputs)
            .into_iter()
            .map(|(output, rect)| {
                let name = output.map(|output| output.identifier().to_owned());
                let shows = if separate { name.as_deref() } else { None };
                let number = name.as_deref().and_then(|name| self.number(name));
                let tile = self.tile(output, shows, number);
                let editing = separate && name.is_some() && name == selected;
                if editing {
                    tile.add_css_class("st-editing");
                }
                if separate {
                    if let Some(name) = name {
                        let weak = Rc::downgrade(self);
                        tile.connect_clicked(move |_| {
                            if let Some(ui) = weak.upgrade() {
                                ui.selected.replace(Some(name.clone()));
                                ui.refresh();
                            }
                        });
                    }
                } else {
                    tile.set_can_target(false);
                }
                (tile, rect)
            })
            .collect();
        self.preview.set_tiles(tiles);
    }

    /// The number of the output `identifier`, as the Displays page shows it: its place among
    /// the outputs, counting from 1.
    fn number(&self, identifier: &str) -> Option<usize> {
        self.model
            .outputs()?
            .iter()
            .position(|output| output.identifier() == identifier)
            .map(|index| index + 1)
    }

    /// The tile of `output` (`None`: every output) with the wallpaper of `shows` and `number`.
    pub(super) fn tile(
        &self,
        output: Option<&Output>,
        shows: Option<&str>,
        number: Option<usize>,
    ) -> gtk::Button {
        let picture = gtk::Picture::new();
        picture.set_can_shrink(true);
        picture.add_css_class("st-wallpaper-screen");
        let color = self.color_value(shows);
        picture.set_paintable(Some(&solid(color)));
        // Stretched, so the one pixel stays inside the tile.
        picture.set_content_fit(gtk::ContentFit::Fill);
        // How swaybg puts the picture on the screen: scaled, or at its own size.
        let fill = match self.value(FILL, shows) {
            Some(Value::Choice(fill)) if fill == "fit" => Ok(gtk::ContentFit::Contain),
            Some(Value::Choice(fill)) if fill == "stretch" => Ok(gtk::ContentFit::Fill),
            Some(Value::Choice(fill)) if fill == "center" => Err(false),
            Some(Value::Choice(fill)) if fill == "tile" => Err(true),
            _ => Ok(gtk::ContentFit::Cover),
        };
        // Without a session, a common screen.
        let pixels = output
            .and_then(Output::pixels)
            .and_then(|(width, height)| {
                Some((i32::try_from(width).ok()?, i32::try_from(height).ok()?))
            })
            .unwrap_or(SCREEN);
        let path = self.picture_of(shows);
        let screen = gtk::Overlay::new();
        screen.set_child(Some(&picture));
        screen.set_overflow(gtk::Overflow::Hidden);
        screen.add_css_class("st-wallpaper-screen");
        match (path, fill) {
            (Some(path), Ok(fit)) => {
                let image = gtk::Picture::new();
                image.set_content_fit(fit);
                image.set_can_shrink(true);
                thumbnail_into(&image, &path);
                screen.add_overlay(&image);
            }
            (Some(path), Err(repeat)) => {
                let area = gtk::DrawingArea::new();
                own_size_into(&area, &path, pixels, repeat);
                screen.add_overlay(&area);
            }
            (None, _) => {}
        }
        if let Look::Collection(name, _) = self.look(shows) {
            let label = gtk::Label::new(Some(&name));
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let icon = gtk::Image::from_icon_name(Kind::Collection.icon());
            icon.set_accessible_role(gtk::AccessibleRole::Presentation);
            let badge = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            badge.add_css_class("st-wallpaper-badge");
            badge.set_halign(gtk::Align::Start);
            badge.set_valign(gtk::Align::Start);
            badge.append(&icon);
            badge.append(&label);
            screen.add_overlay(&badge);
        }
        let name = match output {
            Some(output) if output.label() != output.name() => {
                format!("{} · {}", output.label(), output.name())
            }
            Some(output) => output.name().to_owned(),
            None => tr(msg!("All displays")).to_owned(),
        };
        // The number of the display, as on the Displays page, small in the corner.
        if let Some(number) = number {
            let label = gtk::Label::new(Some(&number.to_string()));
            label.add_css_class("st-wallpaper-number");
            label.set_halign(gtk::Align::End);
            label.set_valign(gtk::Align::Start);
            label.set_can_target(false);
            screen.add_overlay(&label);
        }
        let tile = gtk::Button::new();
        tile.set_child(Some(&screen));
        tile.set_overflow(gtk::Overflow::Hidden);
        tile.add_css_class("flat");
        tile.add_css_class("st-wallpaper-tile");
        tile.set_tooltip_text(Some(&name));
        let description = match number {
            Some(number) => trf(
                msg!("Display {number}, {name}"),
                &[("number", &number), ("name", &name)],
            ),
            None => name,
        };
        tile.update_property(&[gtk::accessible::Property::Label(&description)]);
        tile
    }

    /// The name and details of what the edited output shows, the fit and the color.
    pub(super) fn refresh_current(self: &Rc<Self>, target: Option<&str>) {
        let look = self.look(target);
        let color = self.color_value(target);
        let (kind, title, detail, more) = match &look {
            Look::Picture(image) => {
                let path = Path::new(image);
                let name = path
                    .file_name()
                    .map_or_else(|| image.clone(), |name| name.to_string_lossy().into_owned());
                let folder = path
                    .parent()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_default();
                let detail = if folder.is_empty() {
                    tr(msg!("Picture")).to_owned()
                } else {
                    // TRANSLATORS: What a display shows: one picture, from {folder}.
                    trf(msg!("Picture · {folder}"), &[("folder", &folder)])
                };
                (Kind::Picture, name, detail, String::new())
            }
            Look::Collection(name, current) => {
                let picture = current
                    .as_ref()
                    .and_then(|current| current.file_name())
                    .map(|name| name.to_string_lossy().into_owned());
                let detail = match picture {
                    // TRANSLATORS: What a display shows: a collection of pictures, showing
                    // {picture} now.
                    Some(picture) => {
                        trf(msg!("Collection · now {picture}"), &[("picture", &picture)])
                    }
                    None => tr(msg!("Collection")).to_owned(),
                };
                let more = self
                    .model
                    .state()
                    .map(|state| describe(&Collection::load(&state, name)))
                    .unwrap_or_default();
                (Kind::Collection, name.clone(), detail, more)
            }
            Look::Color => (
                Kind::Color,
                tr(msg!("Solid color")).to_owned(),
                // TRANSLATORS: What a display shows: one color, {color} like #285577.
                trf(msg!("Color · {color}"), &[("color", &color.to_string())]),
                String::new(),
            ),
        };
        self.kind.set_icon_name(Some(kind.icon()));
        self.title.set_text(&title);
        self.detail.set_text(&detail);
        self.detail.set_visible(!detail.is_empty());
        self.more.set_text(&more);
        self.more.set_visible(!more.is_empty());
        self.next.set_visible(matches!(look, Look::Collection(..)));
        // The default is shown: say where it comes from; else offer to go back to it.
        let own = !self.own_keys(target).is_empty();
        self.origin.set_visible(!own);
        self.use_default.set_visible(own);
        if own {
            self.use_default
                .set_tooltip_text(Some(&self.fallback_origin(target)));
        } else {
            self.origin.set_text(&self.fallback_origin(target));
        }
        self.check_picture(self.picture_of(target));
        let fill = match self.value(FILL, target) {
            Some(Value::Choice(fill)) => fill,
            _ => "fill".to_owned(),
        };
        for (button, (id, _)) in self.fits.iter().zip(FITS) {
            button.set_active(id == fill);
            button.set_sensitive(!matches!(look, Look::Color));
        }
        self.color.set_rgba(&rgba(color));
        set_frame_color(color);
    }

    /// Says below what the edited output shows if its picture `path` doesn't exist or can't be
    /// read, so swaybg can't show it either.
    fn check_picture(self: &Rc<Self>, path: Option<PathBuf>) {
        if *self.checked.borrow() == path {
            return;
        }
        self.checked.replace(path.clone());
        self.warning.set_visible(false);
        let Some(path) = path else { return };
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        if !path.exists() {
            self.warning.set_text(&trf(
                msg!("{picture} doesn\u{2019}t exist, so the screen stays black."),
                &[("picture", &name)],
            ));
            self.warning.set_visible(true);
            return;
        }
        let weak = Rc::downgrade(self);
        when_readable(&path.clone(), move |readable| {
            let Some(ui) = weak.upgrade() else { return };
            if readable || ui.checked.borrow().as_ref() != Some(&path) {
                return;
            }
            ui.warning.set_text(&trf(
                msg!(
                    "{picture} can\u{2019}t be read, so swaybg likely can\u{2019}t show it \
                     either. Pictures in WebP, AVIF or JPEG XL need an image loader for \
                     gdk-pixbuf, like webp-pixbuf-loader."
                ),
                &[("picture", &name)],
            ));
            ui.warning.set_visible(true);
        });
    }

    /// Switches between editing every output and one output at a time. Back to every output,
    /// the values of single outputs go, after asking.
    pub(super) fn switch_same(self: &Rc<Self>, same: bool) {
        if self.syncing.get() {
            return;
        }
        if !same {
            self.separate.set(Some(true));
            self.refresh();
            return;
        }
        let own = self
            .model
            .state()
            .map(|state| own_outputs(&state))
            .unwrap_or_default();
        if own.is_empty() {
            self.separate.set(Some(false));
            self.refresh();
            return;
        }
        let dialog = gtk::AlertDialog::builder()
            .message(tr(msg!("Use the same wallpaper on all displays?")))
            .detail(tr(msg!(
                "The wallpapers set for single displays are removed, and every display shows the \
                 wallpaper set for all of them."
            )))
            .buttons([tr(msg!("Cancel")), tr(msg!("Use the Same Wallpaper"))])
            .cancel_button(0)
            .default_button(1)
            .modal(true)
            .build();
        let window = self.body.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(self);
        dialog.choose(window.as_ref(), gio::Cancellable::NONE, move |answer| {
            let Some(ui) = weak.upgrade() else { return };
            if answer == Ok(1) {
                for (key, output) in &own {
                    ui.model.reset(key, Some(output));
                }
                ui.separate.set(Some(false));
            }
            ui.refresh();
        });
    }

    /// Shows the next picture of the edited output's collection.
    pub(super) fn next_picture(self: &Rc<Self>) {
        let Some(state) = self.model.state() else {
            return;
        };
        let target = self.target();
        // An output without a collection of its own shows the one of every output.
        let output = target.filter(|output| wallpaper::shown_on(&state, Some(output)).is_some());
        let weak = Rc::downgrade(self);
        self.next.set_sensitive(false);
        glib::spawn_future_local(async move {
            let mut client = Client::connect().await.ok();
            let result =
                wallpaper::next(client.as_mut(), &state, output.as_deref(), Moment::Now).await;
            let Some(ui) = weak.upgrade() else { return };
            ui.next.set_sensitive(true);
            if let Err(error) = result {
                ui.error.set_text(&error.text().translated());
                ui.error.set_visible(true);
            }
            ui.refresh();
        });
    }

    /// Starts the timers of the collections again when what they change or when changed.
    pub(super) fn follow_timers(&self) {
        let Some(state) = self.model.state() else {
            return;
        };
        let now = timers(&state);
        if self.timers.borrow().as_ref() == Some(&now) {
            return;
        }
        self.timers.replace(Some(now));
        if !wallpaper::has_timers() || std::env::var_os("SWAYSOCK").is_none() {
            return;
        }
        let error = self.error.downgrade();
        glib::spawn_future_local(async move {
            if let Err(failure) = wallpaper::reschedule(&state, &wallpaper::program()).await {
                if let Some(error) = error.upgrade() {
                    error.set_text(&failure.text().translated());
                    error.set_visible(true);
                }
            }
        });
    }
}

/// Where the tiles of `outputs` go in the preview: at their places in the output layout. Without
/// a session, when no output has a place, they stand side by side at the size of their screens;
/// without outputs, one tile stands for all of them.
pub(super) fn arrange(outputs: &[Output]) -> Vec<(Option<&Output>, Rect)> {
    let placed: Vec<(Option<&Output>, Rect)> = outputs
        .iter()
        .filter_map(|output| Some((Some(output), output.rect()?)))
        .collect();
    if !placed.is_empty() {
        return placed;
    }
    if outputs.is_empty() {
        return vec![(None, screen_at(0, SCREEN))];
    }
    let mut x = 0;
    outputs
        .iter()
        .map(|output| {
            let size = output
                .pixels()
                .and_then(|(width, height)| {
                    Some((i32::try_from(width).ok()?, i32::try_from(height).ok()?))
                })
                .unwrap_or(SCREEN);
            let rect = screen_at(x, size);
            x = x.saturating_add(size.0);
            (Some(output), rect)
        })
        .collect()
}

/// A screen of `width` × `height` at `x` on the top edge.
fn screen_at(x: i32, (width, height): (i32, i32)) -> Rect {
    Rect {
        x,
        y: 0,
        width,
        height,
    }
}

/// The outputs with a wallpaper value of their own: each key and output.
pub(super) fn own_outputs(state: &State) -> Vec<(&'static str, String)> {
    state
        .entries()
        .into_iter()
        .filter(|entry| entry.setting.key().starts_with("wallpaper."))
        .filter_map(|entry| Some((entry.setting.key(), entry.device?.to_owned())))
        .collect()
}

/// The outputs whose collection changes by a timer, with when.
pub(super) fn timers(state: &State) -> Timers {
    wallpaper::shown(state)
        .into_iter()
        .map(|shown| (shown.output, shown.collection.change))
        .collect()
}
