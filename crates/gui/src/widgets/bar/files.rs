// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files Sway Things writes for the bar, at the end of the page: _Show Files…_ opens them
//! read-only, a tab each (Waybar's `config.jsonc`, `style.css` and `theme.css`), with the path,
//! _Copy Path_ and the text as Sway Things writes it for the settings now. With swaybar the
//! bar's lines are in `75-bar.conf`, which the _Generated config_ window shows.

use std::rc::Rc;

use gtk::prelude::*;
use sway_things_core::bar::tilde;
use sway_things_core::registry::{ConfigFile, Value, bar as keys};

use super::{Ctx, follow};
use crate::i18n::{msg, tr};

/// The row with _Show Files…_.
pub fn row(ctx: &Rc<Ctx>) -> gtk::Widget {
    let title = gtk::Label::new(Some(tr(msg!("Files Sway Things writes"))));
    title.set_xalign(0.0);
    let subtitle = gtk::Label::new(None);
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.add_css_class(crate::style::DIM);
    subtitle.add_css_class("st-row-detail");
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    texts.append(&title);
    texts.append(&subtitle);
    let show = gtk::Button::with_label(tr(msg!("Show Files…")));
    show.set_valign(gtk::Align::Center);
    let weak = Rc::downgrade(ctx);
    show.connect_clicked(move |button| {
        let Some(ctx) = weak.upgrade() else { return };
        if matches!(ctx.value(keys::BACKEND), Some(Value::Choice(id)) if id == "swaybar") {
            if let Some(window) = button.root().and_downcast::<crate::window::Window>() {
                window.show_generated(Some(ConfigFile::Bar));
            }
        } else {
            open(&ctx, button.upcast_ref());
        }
    });
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.add_css_class("st-bar-files");
    line.append(&texts);
    line.append(&show);
    let weak = Rc::downgrade(ctx);
    follow(ctx.model(), &line, move || {
        let Some(ctx) = weak.upgrade() else { return };
        let swaybar =
            matches!(ctx.value(keys::BACKEND), Some(Value::Choice(id)) if id == "swaybar");
        subtitle.set_text(&if swaybar {
            tr(msg!("swaybar's lines, in 75-bar.conf")).to_owned()
        } else {
            ctx.model()
                .waybar_files()
                .map(|files| tilde(&files.dir))
                .unwrap_or_default()
        });
    });
    line.upcast()
}

/// Opens Waybar's files, read-only, over the window of `anchor`.
fn open(ctx: &Ctx, anchor: &gtk::Widget) {
    let window = super::dialog::window(anchor, tr(msg!("Waybar's Files")), 760, 620);
    let notebook = gtk::Notebook::new();
    notebook.set_vexpand(true);
    for (path, text) in ctx.model().bar_texts() {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let page = gtk::Box::new(gtk::Orientation::Vertical, 8);
        page.add_css_class("st-bar-file");
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let location = gtk::Label::new(Some(&tilde(&path)));
        location.set_xalign(0.0);
        location.set_hexpand(true);
        location.set_selectable(true);
        location.set_wrap(true);
        location.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        location.add_css_class("monospace");
        location.add_css_class(crate::style::DIM);
        let copy = gtk::Button::with_label(tr(msg!("Copy Path")));
        let copied = path.display().to_string();
        copy.connect_clicked(move |button| button.clipboard().set_text(&copied));
        head.append(&location);
        head.append(&copy);
        page.append(&head);
        let buffer = gtk::TextBuffer::new(None);
        buffer.set_text(&text);
        let view = gtk::TextView::with_buffer(&buffer);
        view.set_editable(false);
        view.set_monospace(true);
        view.add_css_class("st-generated-text");
        view.update_property(&[gtk::accessible::Property::Label(&name)]);
        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_child(Some(&view));
        page.append(&scrolled);
        notebook.append_page(&page, Some(&gtk::Label::new(Some(&name))));
    }
    let note = gtk::Label::new(Some(tr(msg!(
        "What Sway Things writes for the settings now. Change the bar on this page; hand edits \
         of these files are overwritten. Your own Waybar config is never written."
    ))));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class(crate::style::DIM);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.add_css_class("st-bar-dialog-content");
    content.append(&note);
    content.append(&notebook);
    window.set_child(Some(&content));
    window.present();
}
