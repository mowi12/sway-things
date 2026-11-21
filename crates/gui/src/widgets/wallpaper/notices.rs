// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The notices about the lines of the user's config that start a wallpaper tool or script, which
//! may draw over the background set here.

use std::fmt::Write as _;

use gtk::prelude::*;
use sway_things_core::wallpaper::{self, Starter};

use super::Ui;
use crate::i18n::{msg, tr, trf};
use crate::model::Model;
use crate::widgets::notice::{self, Tone};
use crate::widgets::setting_row::tilde;

impl Ui {
    pub(super) fn refresh_notice(&self) {
        while let Some(child) = self.notices.first_child() {
            self.notices.remove(&child);
        }
        let found = self
            .model
            .status()
            .map(|status| wallpaper::starters(status.config()))
            .unwrap_or_default();
        self.notices.set_visible(!found.is_empty());
        // One notice per program, naming every line that starts it.
        let mut programs: Vec<(String, Vec<wallpaper::External>)> = Vec::new();
        for external in found {
            let program = program_of(&external.starter);
            match programs.iter_mut().find(|(other, _)| *other == program) {
                Some((_, lines)) => lines.push(external),
                None => programs.push((program, vec![external])),
            }
        }
        for (program, lines) in &programs {
            self.notices
                .append(&starter_notice(&self.model, program, lines));
        }
    }
}

/// The program a line starts, as the notice names it.
pub(super) fn program_of(starter: &Starter) -> String {
    match starter {
        Starter::Tool(tool) => (*tool).to_owned(),
        Starter::Script(script) => script.clone(),
    }
}

/// The notice about the lines of the user's config that start the wallpaper tool `program`,
/// with a button that opens the first of them in the editor.
pub(super) fn starter_notice(
    model: &Model,
    program: &str,
    lines: &[wallpaper::External],
) -> gtk::Box {
    // `~/.config/sway/config:14, 15`: the file once for lines in the same file.
    let mut place = String::new();
    let mut file = None;
    for external in lines {
        let this = &external.directive.file;
        if file == Some(this) {
            let _ = write!(place, ", {}", external.directive.line);
        } else {
            if !place.is_empty() {
                place.push_str(", ");
            }
            let _ = write!(place, "{}:{}", tilde(this), external.directive.line);
            file = Some(this);
        }
    }
    let text = notice::text(&trf(
        // TRANSLATORS: {program} is a wallpaper tool such as swww-daemon, {place} a file and line
        // such as ~/.config/sway/config:12.
        msg!(
            "Your config starts {program} ({place}). It sets a wallpaper of its own, which may \
             cover the one set here. To use this page, remove that line."
        ),
        &[("program", &program), ("place", &place)],
    ));
    let Some(external) = lines.first() else {
        return gtk::Box::new(gtk::Orientation::Horizontal, 12);
    };
    let open = gtk::Button::with_label(tr(msg!("Open config in editor")));
    let (file, line) = (
        external.directive.file.to_path_buf(),
        external.directive.line,
    );
    let weak_model = model.downgrade();
    let weak_text = text.downgrade();
    open.connect_clicked(move |button| {
        let Some(model) = weak_model.upgrade() else {
            return;
        };
        let weak_text = weak_text.clone();
        crate::editor::open(button, Some(&model), &file, Some(line), move |message| {
            if let (Some(message), Some(text)) = (message, weak_text.upgrade()) {
                text.set_text(message);
            }
        });
    });
    notice::notice(Tone::Warning, &text, Some(&open))
}
