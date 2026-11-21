// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The note about kanshi above the canvas: what kanshi does with the arrangement, the line its
//! config needs, and the warnings when the user's profiles come first.
use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use sway_things_core::generate;
use sway_things_core::outputs::{Output, Outputs, kanshi};

use crate::i18n::{msg, tr, trf};
use crate::model::Model;
use crate::style;

/// The note about kanshi above the canvas, hidden: an icon, what is said with the line to put
/// in the kanshi config on a line of its own under it, and the button that copies the line and
/// says so for a moment.
pub(super) fn kanshi_note() -> gtk::Box {
    let note = gtk::Label::new(None);
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.set_hexpand(true);
    note.set_selectable(true);
    let copy = gtk::Button::with_label(tr(msg!("Copy Line")));
    copy.set_valign(gtk::Align::Center);
    copy.set_visible(false);
    // Each click says "Copied" for a while; only the latest click's timeout restores the label.
    let clicks = Rc::new(Cell::new(0_u32));
    copy.connect_clicked(move |button| {
        let Some(line) = button.tooltip_text() else {
            return;
        };
        button.clipboard().set_text(&line);
        button.set_label(tr(msg!("Copied")));
        let click = clicks.get().wrapping_add(1);
        clicks.set(click);
        glib::timeout_add_seconds_local_once(
            2,
            glib::clone!(
                #[weak]
                button,
                #[strong]
                clicks,
                move || {
                    if clicks.get() == click {
                        button.set_label(tr(msg!("Copy Line")));
                    }
                }
            ),
        );
    });
    // The line for the kanshi config, on a line of its own under what it is for.
    let line = gtk::Label::new(None);
    line.set_wrap(true);
    line.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    line.set_xalign(0.0);
    line.set_selectable(true);
    line.add_css_class("monospace");
    line.add_css_class(style::DIM);
    let said = gtk::Box::new(gtk::Orientation::Vertical, 6);
    said.set_hexpand(true);
    said.append(&note);
    said.append(&line);
    let kanshi = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    kanshi.add_css_class("st-display-warning");
    kanshi.append(&gtk::Image::from_icon_name("dialog-information-symbolic"));
    kanshi.append(&said);
    kanshi.append(&copy);
    kanshi.set_visible(false);
    kanshi
}

/// What the note says (`said`, with whether something is to do about it) with how to do it:
/// the line to put at the top of the kanshi config, to copy. Sway Things never edits that file
/// and gives no command for it either.
pub(super) fn with_the_way(
    said: Option<(String, bool)>,
    look: &crate::model::KanshiLook,
) -> (Option<String>, Option<String>) {
    match said {
        Some((text, true)) => (
            Some(format!(
                "{text} {}",
                tr(msg!(
                    "Put this line at the top of your kanshi config, above your profiles, and \
                     take a line further down that includes it away:"
                ))
            )),
            Some(look.include_line.clone()),
        ),
        Some((text, false)) => (Some(text), None),
        None => (None, None),
    }
}

/// The identifier of the display `criteria` names among `outputs`, or `criteria` itself.
fn identified(outputs: &Outputs, criteria: &str) -> String {
    outputs.find(criteria).map_or_else(
        || criteria.to_owned(),
        |output| output.identifier().to_owned(),
    )
}

/// The note while sway sets up the displays: what kanshi would add, if anything, and whether
/// it is a warning.
fn without_kanshi(look: &crate::model::KanshiLook) -> Option<(String, bool)> {
    if look.installed && look.include_missing {
        Some((
            tr(msg!(
                "So that each combination of displays is remembered, your kanshi config needs \
                 the line of Sway Things at its top, above your own profiles."
            ))
            .to_owned(),
            true,
        ))
    } else if !look.installed {
        Some((
            tr(msg!(
                "With kanshi installed, each combination of displays is remembered and comes \
                 back on its own."
            ))
            .to_owned(),
            false,
        ))
    } else {
        None
    }
}

/// The note for displays that the user's own kanshi profile `name` (quoted) sets up.
fn set_up_by_theirs(model: &Model, name: &str) -> String {
    if model.adopts_profiles() {
        trf(
            msg!(
                "These displays are set up by your kanshi profile {name}, and Sway Things places \
                 the workspaces on them. Change something here to arrange them here instead."
            ),
            &[("name", &name)],
        )
    } else {
        trf(
            msg!(
                "These displays are set up by your kanshi profile {name}. Change something here \
                 to arrange them here instead."
            ),
            &[("name", &name)],
        )
    }
}

/// What the note about kanshi says for the displays `outputs` connected now, the line for the
/// kanshi config with it, and whether it is a warning. From what the model saw when it last looked:
/// nothing is read from the machine here.
pub(super) fn kanshi_says(
    model: &Model,
    outputs: &Outputs,
) -> (Option<String>, Option<String>, bool) {
    // What the model saw when it last looked: nothing is read from the machine here.
    let look = model.kanshi();
    let displays = model.displays_backend();
    // A profile of the user's that kanshi applies instead of ours, for these displays.
    let connected: Vec<&str> = outputs
        .iter()
        .filter(|output| output.is_connected())
        .map(Output::identifier)
        .collect();
    let overriding = kanshi::overriding(&look.before_ours, &connected, |criteria| {
        identified(outputs, criteria)
    });
    // Without a combination set up here, the first of the user's profiles after the line that
    // matches sets the displays up.
    let theirs: Option<String> = (model.arranged_here() == Some(false))
        .then(|| {
            kanshi::overriding(&look.after_ours, &connected, |criteria| {
                identified(outputs, criteria)
            })
            .into_iter()
            .next()
        })
        .flatten();
    // The line belongs at the very top: any profile above it wins where it matches.
    let above: Vec<&str> = look
        .before_ours
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let warns = displays.is_kanshi() && !above.is_empty();
    // Each name in quotes of its own.
    let quoted = |names: &[&str]| {
        names
            .iter()
            .map(|name| format!("\u{201c}{name}\u{201d}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    // Someone else's file in the place of ours: nothing set here reaches kanshi.
    if let Some(file) = &look.foreign {
        let text = trf(
            msg!(
                "Nothing set here is remembered: {file} wasn\u{2019}t made by Sway Things, so \
                 it is left alone and kanshi never sees the arrangement. Move that file away, \
                 then change something here."
            ),
            &[("file", file)],
        );
        return (Some(text), None, true);
    }
    let said = match displays {
        generate::Displays::Kanshi { .. } if warns && overriding.is_empty() => Some((
            trf(
                msg!(
                    "The line of Sway Things isn\u{2019}t at the top of your kanshi config: \
                     {name} comes before it and wins over the arrangement made here whenever \
                     its displays are connected."
                ),
                &[("name", &quoted(&above))],
            ),
            true,
        )),
        generate::Displays::Kanshi { .. } if warns => {
            let names: Vec<&str> = overriding.iter().map(String::as_str).collect();
            Some((
                trf(
                    msg!(
                        "Changes here don\u{2019}t last: your kanshi profile {name} sets up \
                         these displays, as it comes before the line of Sway Things in your \
                         kanshi config."
                    ),
                    &[("name", &quoted(&names))],
                ),
                true,
            ))
        }
        // Nothing set up here for these displays: the user's own profile does it.
        generate::Displays::Kanshi { .. } if theirs.is_some() => Some((
            set_up_by_theirs(model, &quoted(&[theirs.as_deref().unwrap_or_default()])),
            false,
        )),
        generate::Displays::Kanshi { start } => {
            let text = if start && !look.runs {
                msg!(
                    "Each combination of displays is remembered: kanshi brings this \
                     arrangement back whenever these displays are connected together. kanshi \
                     starts with your next login."
                )
            } else {
                msg!(
                    "Each combination of displays is remembered: kanshi brings this \
                     arrangement back whenever these displays are connected together."
                )
            };
            Some((tr(text).to_owned(), false))
        }
        generate::Displays::Sway => without_kanshi(&look),
    };
    let (text, line) = with_the_way(said, &look);
    (text, line, warns)
}
