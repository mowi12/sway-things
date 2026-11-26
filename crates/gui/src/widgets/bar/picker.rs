// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The item picker: every item the bar can show, by category, with a search. Each row has the
//! item's icon, name, what it does and a sample of what it shows, and badges for what stands in
//! its way: _Not available here_ (no battery, no backlight: the row can't be picked), _Needs …_
//! (a program that isn't installed), _Uses the network_, _In the bar_ (picking it adds another
//! one). Enter or a click adds the item to the lane the picker was opened for.
//!
//! Templates are items with a script of their own, copied on request to Sway Things' waybar
//! directory. _Your own command…_ opens the form for a command of the user's.

use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use serde_json::{Map, Value as Json};
use sway_things_core::bar::catalog::{self, Category, Module, Need, Works};
use sway_things_core::bar::templates::{self, TEMPLATES, Template};
use sway_things_core::bar::{Modules, tilde};

use super::lanes::{self, lane_name};
use super::{Ctx, dialog};
use crate::i18n::{Msg, msg, tr, trf};

/// The picker's title.
const TITLE: Msg = msg!("Add an Item to the {lane}");
/// Said once an item is added.
const ADDED: Msg = msg!("Added {item} to the {lane}");
/// A copy of a template's script the user changed stays.
const SCRIPT_KEPT: Msg = msg!("Your copy of the script stays");
/// Where it is, and that the item runs it.
const SCRIPT_KEPT_DETAIL: Msg = msg!(
    "{file} isn't the script as Sway Things ships it, so it stays as it is and the new item runs \
     it. Move it away to get the template's script."
);
/// Why a template couldn't be added.
const TEMPLATE_FAILED: Msg = msg!("The template couldn't be added");
/// What an item shows, as a sample.
const SAMPLE: Msg = msg!("Shows: {sample}");

/// What a row of the picker adds.
#[derive(Debug, Clone, Copy)]
enum Pick {
    Module(&'static Module),
    Template(&'static Template),
    Command,
}

/// A category in the picker: its heading, its list, and each row with the text the search looks
/// in.
type Group = (gtk::Label, gtk::ListBox, Vec<(gtk::ListBoxRow, String)>);

/// Opens the picker for lane `key`, over the window of `anchor`.
#[expect(
    clippy::too_many_lines,
    reason = "the parts of one dialog, read best in order"
)]
pub fn open(ctx: &Rc<Ctx>, anchor: &gtk::Widget, key: &'static str) {
    let title = trf(TITLE, &[("lane", &lane_name(key))]);
    let window = dialog::window(anchor, &title, 560, 680);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let header = gtk::Box::new(gtk::Orientation::Vertical, 12);
    header.add_css_class("st-bar-picker-header");
    let heading = gtk::Label::new(Some(&title));
    heading.add_css_class("title-2");
    heading.set_xalign(0.0);
    heading.set_accessible_role(gtk::AccessibleRole::Heading);
    header.append(&heading);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(tr(msg!("Search items"))));
    search.update_property(&[gtk::accessible::Property::Label(tr(msg!("Search items")))]);
    header.append(&search);
    root.append(&header);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("st-bar-picker-body");
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_hscrollbar_policy(gtk::PolicyType::Never);
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&body));
    root.append(&scrolled);
    window.set_child(Some(&root));
    let modules = ctx
        .model()
        .bar_modules()
        .and_then(Result::ok)
        .unwrap_or_default();
    let mut groups: Vec<Group> = Vec::new();
    for category in Category::ALL {
        let picks = picks(category);
        if picks.is_empty() {
            continue;
        }
        let label = crate::widgets::list::heading(tr(category.title()), groups.is_empty());
        body.append(&label);
        let list = crate::widgets::list::section_list(tr(category.title()));
        list.set_activate_on_single_click(true);
        let mut rows = Vec::new();
        for pick in picks {
            let (row, haystack) = pick_row(ctx, &modules, pick);
            list.append(&row);
            rows.push((row, haystack));
        }
        let weak = Rc::downgrade(ctx);
        let picked_window = window.downgrade();
        let anchor = anchor.clone();
        let row_picks = picks_by_row(&rows, category);
        list.connect_row_activated(move |_, row| {
            let (Some(ctx), Some(window)) = (weak.upgrade(), picked_window.upgrade()) else {
                return;
            };
            if let Some((_, pick)) = row_picks.iter().find(|(shown, _)| shown == row) {
                window.set_visible(false);
                add(&ctx, &anchor, key, *pick);
                // Closed once the list is done with the click: closing destroys the list.
                glib::idle_add_local_once(move || window.close());
            }
        });
        body.append(&list);
        groups.push((label, list, rows));
    }
    let nothing = gtk::Label::new(Some(tr(msg!("No item matches the search."))));
    nothing.add_css_class(crate::style::DIM);
    nothing.set_margin_top(24);
    nothing.set_visible(false);
    body.append(&nothing);
    let groups = Rc::new(groups);
    let filter_groups = groups.clone();
    search.connect_search_changed(move |search| {
        let words: Vec<String> = search
            .text()
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let mut any = false;
        for (label, list, rows) in filter_groups.iter() {
            let mut shown = false;
            for (row, haystack) in rows {
                let matches = words.iter().all(|word| haystack.contains(word.as_str()));
                row.set_visible(matches);
                shown |= matches;
            }
            label.set_visible(shown);
            list.set_visible(shown);
            any |= shown;
        }
        nothing.set_visible(!any);
    });
    search.connect_activate(move |_| {
        // Enter in the search picks the first item that matches.
        let first = groups.iter().find_map(|(_, _, rows)| {
            rows.iter()
                .find(|(row, _)| row.is_visible() && row.is_sensitive())
                .map(|(row, _)| row.clone())
        });
        if let Some(row) = first {
            row.emit_by_name::<()>("activate", &[]);
        }
    });
    GtkWindowExt::set_focus(&window, Some(&search));
    window.present();
}

/// The rows of `rows` with what they add, in the order of `category`'s picks.
fn picks_by_row(
    rows: &[(gtk::ListBoxRow, String)],
    category: Category,
) -> Vec<(gtk::ListBoxRow, Pick)> {
    rows.iter()
        .map(|(row, _)| row.clone())
        .zip(picks(category))
        .collect()
}

/// What the picker offers in `category`: the catalog's items, the templates, and in _Custom_
/// the user's own command first.
fn picks(category: Category) -> Vec<Pick> {
    let mut picks = Vec::new();
    if category == Category::Custom {
        picks.push(Pick::Command);
    }
    picks.extend(
        catalog::MODULES
            .iter()
            .filter(|module| !matches!(module.name, "custom" | "group"))
            .filter(|module| catalog::category(module.name) == category)
            .map(Pick::Module),
    );
    picks.extend(
        TEMPLATES
            .iter()
            .filter(|template| template.category() == category)
            .map(Pick::Template),
    );
    picks
}

/// The row of `pick` and the lowercase text the search looks in.
#[expect(
    clippy::too_many_lines,
    reason = "the parts of one row, read best in order"
)]
fn pick_row(ctx: &Ctx, modules: &Modules, pick: Pick) -> (gtk::ListBoxRow, String) {
    let (name, title, description, icon) = match pick {
        Pick::Module(module) => (
            module.name.to_owned(),
            tr(module.title),
            tr(module.description),
            catalog::icon(module.name),
        ),
        Pick::Template(template) => (
            template.module_name(),
            tr(template.title),
            tr(template.description),
            "utilities-terminal-symbolic",
        ),
        Pick::Command => (
            "custom/".to_owned(),
            tr(msg!("Your own command…")),
            tr(msg!(
                "Shows what a program or script of yours prints, every few seconds or as it goes"
            )),
            "utilities-terminal-symbolic",
        ),
    };
    let mut badges: Vec<(String, bool)> = Vec::new();
    let mut available = true;
    if let Pick::Module(module) = pick {
        if let Works::No(need) = module.need.check() {
            match need {
                Need::Battery | Need::Backlight | Need::Bluetooth | Need::Thermal => {
                    available = false;
                    badges.push((
                        trf(
                            msg!("Not available here: {reason}"),
                            &[("reason", &tr(need.missing()))],
                        ),
                        true,
                    ));
                }
                Need::Program(program) => badges.push((
                    trf(msg!("Needs {programs}"), &[("programs", &program)]),
                    true,
                )),
                _ => badges.push((tr(need.missing()).to_owned(), true)),
            }
        }
    }
    if let Pick::Template(template) = pick {
        let missing = template.missing();
        if !missing.is_empty() {
            badges.push((
                trf(
                    msg!("Needs {programs}"),
                    &[("programs", &missing.join(", "))],
                ),
                true,
            ));
        }
        if template.uses_network() {
            badges.push((tr(msg!("Uses the network")).to_owned(), false));
        }
    }
    if !matches!(pick, Pick::Command) && lanes::taken(ctx, &name) {
        badges.push((tr(msg!("In the bar: adds another")).to_owned(), false));
    }
    let sample = match pick {
        Pick::Module(module) => catalog::sample(
            module.name,
            modules.definition(module.name).as_ref(),
            super::preview::now(),
        ),
        _ => None,
    };
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.add_css_class("st-bar-pick");
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(24);
    image.set_valign(gtk::Align::Start);
    image.set_accessible_role(gtk::AccessibleRole::Presentation);
    content.append(&image);
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.add_css_class("heading");
    texts.append(&title_label);
    let description_label = gtk::Label::new(Some(description));
    description_label.set_xalign(0.0);
    description_label.set_wrap(true);
    description_label.add_css_class(crate::style::DIM);
    texts.append(&description_label);
    if let Some(sample) = &sample {
        let label = gtk::Label::new(Some(&trf(SAMPLE, &[("sample", sample)])));
        label.set_xalign(0.0);
        label.add_css_class("st-row-detail");
        texts.append(&label);
    }
    if !badges.is_empty() {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.set_halign(gtk::Align::Start);
        line.set_margin_top(4);
        for (text, attention) in &badges {
            let badge = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            badge.add_css_class("st-bar-badge");
            if *attention {
                badge.add_css_class("st-attention");
                let warning = gtk::Image::from_icon_name("dialog-warning-symbolic");
                warning.set_accessible_role(gtk::AccessibleRole::Presentation);
                badge.append(&warning);
            }
            badge.append(&gtk::Label::new(Some(text)));
            line.append(&badge);
        }
        texts.append(&line);
    }
    content.append(&texts);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    row.set_sensitive(available);
    let mut words = vec![title.to_owned(), description.to_owned()];
    words.extend(badges.iter().map(|(text, _)| text.clone()));
    row.update_property(&[
        gtk::accessible::Property::Label(title),
        gtk::accessible::Property::Description(&words[1..].join(". ")),
    ]);
    words.push(name);
    words.push(
        tr(match pick {
            Pick::Module(module) => catalog::category(module.name).title(),
            Pick::Template(template) => template.category().title(),
            Pick::Command => Category::Custom.title(),
        })
        .to_owned(),
    );
    (row, words.join(" ").to_lowercase())
}

/// Adds what `pick` stands for to lane `key`.
fn add(ctx: &Rc<Ctx>, anchor: &gtk::Widget, key: &'static str, pick: Pick) {
    let (name, title) = match pick {
        Pick::Command => {
            dialog::new_command(ctx, anchor, key);
            return;
        }
        Pick::Module(module) => {
            let name = lanes::free_name(ctx, module.name);
            // Another instance starts from the definition of the item it copies.
            if name != module.name {
                if let Some(Ok(mut modules)) = ctx.model().bar_modules() {
                    if let Some(definition) = modules.definition(module.name) {
                        modules.set(&name, definition);
                        ctx.model().set_bar_modules(modules);
                    }
                }
            }
            (name, tr(module.title).to_owned())
        }
        Pick::Template(template) => {
            let Some(definition) = install(ctx, anchor, template) else {
                return;
            };
            let name = lanes::free_name(ctx, &template.module_name());
            if let Some(Ok(mut modules)) = ctx.model().bar_modules() {
                modules.set(&name, definition);
                ctx.model().set_bar_modules(modules);
            }
            (name, tr(template.title).to_owned())
        }
    };
    lanes::add_item(ctx, key, &name);
    let text = trf(ADDED, &[("item", &title), ("lane", &lane_name(key))]);
    ctx.announce(&text);
}

/// Copies `template`'s script, if it has one; its module's definition, or `None` if the script
/// couldn't be copied (said in a dialog).
fn install(ctx: &Ctx, anchor: &gtk::Widget, template: &Template) -> Option<Map<String, Json>> {
    let (files, status) = (ctx.model().waybar_files()?, ctx.model().status()?);
    let installed = status
        .write_access()
        .map_err(|read_only| read_only.text().translated())
        .and_then(|access| {
            template
                .install(&files, &access)
                .map_err(|error| error.text().translated())
        });
    let window = anchor.root().and_downcast::<gtk::Window>();
    match installed {
        Err(error) => {
            gtk::AlertDialog::builder()
                .message(tr(TEMPLATE_FAILED))
                .detail(error)
                .modal(true)
                .build()
                .show(window.as_ref());
            return None;
        }
        Ok(templates::Installed::Kept(path)) => {
            gtk::AlertDialog::builder()
                .message(tr(SCRIPT_KEPT))
                .detail(trf(SCRIPT_KEPT_DETAIL, &[("file", &tilde(&path))]))
                .modal(true)
                .build()
                .show(window.as_ref());
        }
        Ok(_) => {}
    }
    Some(template.definition(&files))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_item_and_template_is_offered_once() {
        let mut count = 0;
        for category in Category::ALL {
            count += picks(category).len();
        }
        let modules = catalog::MODULES
            .iter()
            .filter(|module| !matches!(module.name, "custom" | "group"))
            .count();
        assert_eq!(count, modules + TEMPLATES.len() + 1);
    }
}
