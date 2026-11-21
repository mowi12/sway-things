// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The card of the bar program above the rows: which bar Sway Things sets up, in one line and
//! plain words, with _Change…_ (or _Set Up a Bar…_ while none is), and below it how the bar runs
//! ([`super::status`]).
//!
//! _Change…_ opens a dialog with the three choices, each with what it can do and, for Waybar,
//! whether it is installed and its version. Before a switch it says what stays and what the
//! other program can't show; nothing is dropped: every setting stays saved, and switching back
//! brings it all back. Without a choice the user's own bar stays as it is, which is why Sway
//! Things never picks a program by itself.

use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use sway_things_core::bar::process;
use sway_things_core::registry::{Value, bar as keys};

use super::preview::item_title;
use super::{Ctx, follow, status, takeover};
use crate::i18n::{Msg, msg, msg_plural, tr, trf};

/// The version of Waybar that is installed.
const INSTALLED: Msg = msg!("Installed: Waybar {version}");
/// What swaybar won't show.
const SWAYBAR_DROPS: Msg = msg!(
    "Not shown by swaybar: {items}, islands, transparency, rounded corners and the space around \
     the bar. They stay saved for when you switch back."
);
/// The button that confirms.
const USE: Msg = msg!("Use {program}");

/// A choice of the dialog.
struct Program {
    id: &'static str,
    title: Msg,
    description: Msg,
}

/// The choices, in the order the dialog shows them.
const PROGRAMS: [Program; 3] = [
    Program {
        id: "waybar",
        title: msg!("Waybar"),
        description: msg!(
            "Many items, icons, tooltips, click actions and full styling. A program of its own; \
             Sway Things writes its config, and yours stays as it is."
        ),
    },
    Program {
        id: "swaybar",
        title: msg!("swaybar"),
        description: msg!(
            "Built into sway and very light. Text only: the workspaces, the binding mode and \
             the text of a status program, with little styling."
        ),
    },
    Program {
        id: "none",
        title: msg!("None: keep your own bar"),
        description: msg!(
            "Sway Things sets up no bar, and whatever bar your sway config starts stays as it is."
        ),
    },
];

/// The bar program set up now.
fn current(ctx: &Ctx) -> &'static str {
    match ctx.value(keys::BACKEND) {
        Some(Value::Choice(id)) if id == "waybar" => "waybar",
        Some(Value::Choice(id)) if id == "swaybar" => "swaybar",
        _ => "none",
    }
}

/// The card of the bar program, following the settings.
pub fn card(ctx: &Rc<Ctx>) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.add_css_class("st-bar-program");
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let icon = gtk::Image::from_icon_name("preferences-desktop-display-symbolic");
    icon.set_icon_size(gtk::IconSize::Large);
    icon.set_valign(gtk::Align::Center);
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    head.append(&icon);
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(None);
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.add_css_class("heading");
    let subtitle = gtk::Label::new(None);
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.add_css_class(crate::style::DIM);
    texts.append(&title);
    texts.append(&subtitle);
    head.append(&texts);
    let change = gtk::Button::new();
    change.set_valign(gtk::Align::Center);
    let weak = Rc::downgrade(ctx);
    change.connect_clicked(move |button| {
        if let Some(ctx) = weak.upgrade() {
            open(&ctx, button.upcast_ref());
        }
    });
    head.append(&change);
    card.append(&head);
    card.append(&takeover::section(ctx));
    let status = status::Status::new(ctx.model());
    let details = gtk::Expander::new(Some(tr(msg!("Details"))));
    details.add_css_class("st-bar-details");
    details.set_child(Some(status.widget()));
    card.append(&details);
    // The state in one line under the program; details open by themselves when something needs
    // the user, once.
    let told = subtitle.downgrade();
    let shown = details.downgrade();
    let opened = Rc::new(std::cell::Cell::new(false));
    let lines = status.clone();
    status.connect_state(move |state, attention, details_say_more| {
        if let Some(subtitle) = told.upgrade() {
            subtitle.set_text(state);
            if attention {
                subtitle.add_css_class("st-attention");
            } else {
                subtitle.remove_css_class("st-attention");
            }
        }
        if let Some(details) = shown.upgrade() {
            details.set_visible(!lines.is_empty());
            if details_say_more && !opened.replace(true) {
                details.set_expanded(true);
            }
        }
    });
    status.update(ctx.model());
    let weak = Rc::downgrade(ctx);
    follow(ctx.model(), &card, move || {
        let Some(ctx) = weak.upgrade() else { return };
        let (heading, button) = match current(&ctx) {
            "waybar" => (msg!("Bar program: Waybar"), msg!("Change…")),
            "swaybar" => (msg!("Bar program: swaybar"), msg!("Change…")),
            _ => (msg!("No bar is set up here"), msg!("Set Up a Bar…")),
        };
        title.set_text(tr(heading));
        change.set_label(tr(button));
        // Where the user's own bar can be replaced, that is the suggested way.
        if current(&ctx) == "none" && !takeover::offers(&ctx) {
            change.add_css_class("suggested-action");
        } else {
            change.remove_css_class("suggested-action");
        }
        change.set_sensitive(ctx.writable());
    });
    card
}

/// Opens the dialog of the bar program over the window of `anchor`.
#[expect(
    clippy::too_many_lines,
    reason = "the parts of one dialog, read best in order"
)]
pub fn open(ctx: &Rc<Ctx>, anchor: &gtk::Widget) {
    let window = super::dialog::window(anchor, tr(msg!("Bar Program")), 520, -1);
    window.set_resizable(false);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.add_css_class("st-bar-dialog-content");
    let heading = gtk::Label::new(Some(tr(msg!("Which bar should Sway Things set up?"))));
    heading.add_css_class("title-2");
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    heading.set_accessible_role(gtk::AccessibleRole::Heading);
    content.append(&heading);
    let list = crate::widgets::list::section_list(tr(msg!("Bar program")));
    list.set_accessible_role(gtk::AccessibleRole::RadioGroup);
    let now = current(ctx);
    let mut first: Option<gtk::CheckButton> = None;
    let mut radios = Vec::new();
    let version = gtk::Label::new(None);
    for program in &PROGRAMS {
        let radio = gtk::CheckButton::new();
        if let Some(first) = &first {
            radio.set_group(Some(first));
        } else {
            first = Some(radio.clone());
        }
        radio.set_active(program.id == now);
        radio.set_valign(gtk::Align::Start);
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        texts.set_hexpand(true);
        let title = gtk::Label::new(Some(tr(program.title)));
        title.set_xalign(0.0);
        title.add_css_class("heading");
        let description = gtk::Label::new(Some(tr(program.description)));
        description.set_xalign(0.0);
        description.set_wrap(true);
        description.add_css_class(crate::style::DIM);
        texts.append(&title);
        texts.append(&description);
        if program.id == "waybar" {
            version.set_xalign(0.0);
            version.add_css_class("st-row-detail");
            texts.append(&version);
        }
        radio.update_property(&[
            gtk::accessible::Property::Label(tr(program.title)),
            gtk::accessible::Property::Description(tr(program.description)),
        ]);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        line.add_css_class("st-bar-choice");
        line.append(&radio);
        line.append(&texts);
        // A click anywhere on the line picks it.
        let click = gtk::GestureClick::new();
        let picked = radio.downgrade();
        click.connect_released(move |_, _, _, _| {
            if let Some(radio) = picked.upgrade() {
                radio.set_active(true);
            }
        });
        line.add_controller(click);
        list.append(&crate::widgets::list::list_row(&line));
        radios.push((program, radio));
    }
    content.append(&list);
    let summary = crate::widgets::notice::text("");
    content.append(&crate::widgets::notice::notice(
        crate::widgets::notice::Tone::Info,
        &summary,
        None,
    ));
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label(tr(msg!("Cancel")));
    let closing = window.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(window) = closing.upgrade() {
            window.close();
        }
    });
    let confirm = gtk::Button::new();
    confirm.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&confirm);
    content.append(&buttons);
    window.set_child(Some(&content));
    window.set_default_widget(Some(&confirm));

    let installed = process::installed();
    version.set_text(tr(if installed {
        msg!("Installed")
    } else {
        msg!("Not installed: install the waybar package first")
    }));
    if installed {
        let shown = version.downgrade();
        glib::spawn_future_local(async move {
            if let Ok(Some(found)) = gio::spawn_blocking(process::version).await {
                if let Some(label) = shown.upgrade() {
                    label.set_text(&trf(INSTALLED, &[("version", &found)]));
                }
            }
        });
    }
    let radios = Rc::new(radios);
    let picked = {
        let radios = radios.clone();
        move || -> &'static str {
            radios
                .iter()
                .find(|(_, radio)| radio.is_active())
                .map_or("none", |(program, _)| program.id)
        }
    };
    let update = {
        let weak = Rc::downgrade(ctx);
        let picked = picked.clone();
        let summary = summary.downgrade();
        let confirm = confirm.downgrade();
        move || {
            let (Some(ctx), Some(summary), Some(confirm)) =
                (weak.upgrade(), summary.upgrade(), confirm.upgrade())
            else {
                return;
            };
            let target = picked();
            let program = PROGRAMS
                .iter()
                .find(|program| program.id == target)
                .map_or("", |program| tr(program.title));
            confirm.set_label(&if target == current(&ctx) {
                tr(msg!("Keep")).to_owned()
            } else {
                trf(USE, &[("program", &program)])
            });
            summary.set_text(&what_happens(&ctx, target, installed));
        }
    };
    update();
    for (_, radio) in radios.iter() {
        let update = update.clone();
        radio.connect_toggled(move |radio| {
            if radio.is_active() {
                update();
            }
        });
    }
    let weak = Rc::downgrade(ctx);
    let closing = window.downgrade();
    confirm.connect_clicked(move |_| {
        let Some(ctx) = weak.upgrade() else { return };
        let target = picked();
        if target != current(&ctx) {
            ctx.remember(ctx.snapshot(&[keys::BACKEND]));
            ctx.model()
                .set(keys::BACKEND, None, Value::Choice(target.to_owned()));
        }
        if let Some(window) = closing.upgrade() {
            window.close();
        }
    });
    if let Some((_, radio)) = radios.iter().find(|(_, radio)| radio.is_active()) {
        GtkWindowExt::set_focus(&window, Some(radio));
    }
    window.present();
}

/// What switching to `target` keeps and what it can't show, in words.
fn what_happens(ctx: &Ctx, target: &str, installed: bool) -> String {
    let now = current(ctx);
    if target == now {
        return tr(msg!("This is the bar program set up now.")).to_owned();
    }
    match target {
        "swaybar" => {
            let modules = ctx
                .model()
                .bar_modules()
                .and_then(Result::ok)
                .unwrap_or_default();
            let items: Vec<String> = super::lanes::LANES
                .iter()
                .flat_map(|(lane, _)| ctx.list(lane))
                .filter(|name| !matches!(name.as_str(), "sway/workspaces" | "sway/mode"))
                .map(|name| item_title(&modules, &name))
                .collect();
            let items = if items.is_empty() {
                tr(msg!("no items")).to_owned()
            } else if items.len() > 4 {
                crate::i18n::ntrf(
                    msg_plural!(
                        "{first} and {count} more item",
                        "{first} and {count} more items"
                    ),
                    items.len() - 3,
                    &[
                        ("first", &items[..3].join(", ")),
                        ("count", &(items.len() - 3)),
                    ],
                )
            } else {
                items.join(", ")
            };
            format!(
                "{} {}",
                tr(msg!(
                    "Kept: the position (top or bottom), height, font and colors, and the \
                     workspaces."
                )),
                trf(SWAYBAR_DROPS, &[("items", &items)])
            )
        }
        "waybar" if !installed => tr(msg!(
            "Waybar isn't installed yet: install the waybar package. Until then, the bar can't \
             start; everything set here stays saved."
        ))
        .to_owned(),
        "waybar" => tr(msg!(
            "Waybar shows your items, look and placement. Sway Things starts it with its own \
             files; your own Waybar config stays as it is."
        ))
        .to_owned(),
        _ => tr(msg!(
            "Sway Things stops setting up a bar: a Waybar it started ends, and swaybar hides \
             until sway reloads. Every setting here stays saved for later."
        ))
        .to_owned(),
    }
}
