// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a page adds to the rows built from the registry, page by page, in one place: the parts
//! before and after its rows, the choices a row of it offers and what the row follows, and the
//! widgets below a row. [`PageView::for_settings`](super::PageView::for_settings) asks here and
//! names no page itself.

use gtk::prelude::*;
use sway_things_core::outputs;
use sway_things_core::registry::{Page, Setting};

use super::{
    Section, about_section, add_workspace_map, arrangement_section, generated_section,
    test_area_section,
};
use crate::model::Model;
use crate::widgets::displays::{self, Arrangement};
use crate::widgets::{SettingRow, cursor, keyboard, look, wallpaper};

/// The parts of `page` above its rows: the system details and the generated files on About &
/// Maintenance, the note of Apps Look & Feel, the arrangement of the displays. `first` says
/// whether the next section is the first of the page; a part that is a section takes it.
pub(super) fn before_rows(
    model: &Model,
    page: Page,
    body: &gtk::Box,
    sections: &mut Vec<Section>,
    first: &mut bool,
) -> Option<Arrangement> {
    match page {
        Page::About => {
            sections.push(about_section(model, body));
            sections.push(generated_section(model, body));
            *first = false;
            None
        }
        Page::AppsLookFeel => {
            body.append(&look::note());
            None
        }
        Page::Displays => Some(arrangement_section(
            model,
            body,
            std::mem::take(first),
            sections,
        )),
        _ => None,
    }
}

/// The parts of `page` below its rows: the test area of Mouse & Touchpad, the map of the
/// displays on Workspaces.
pub(super) fn after_rows(model: &Model, page: Page, body: &gtk::Box, sections: &mut Vec<Section>) {
    match page {
        Page::MouseTouchpad => sections.push(test_area_section(model, body)),
        Page::Workspaces => add_workspace_map(model, sections),
        _ => {}
    }
}

/// The row of `setting` on its page, for the whole session or input type: a drop-down where the
/// page offers choices of its own (the keyboard models, an output's modes, the cursor and GTK
/// themes, the screens), following what they depend on.
pub(super) fn row(model: &Model, setting: &'static Setting) -> Option<SettingRow> {
    if setting.key() == outputs::MODE {
        return SettingRow::with_choices(model, setting, None, &displays::mode_choices(None));
    }
    let choices = keyboard::choices(setting)
        .or_else(|| cursor::choices(model, setting))
        .or_else(|| look::choices(setting));
    let row = match choices {
        Some(choices) => SettingRow::with_choices(model, setting, None, &choices),
        None => SettingRow::new(model, setting, None),
    }?;
    cursor::follow(model, &row);
    look::follow(&row);
    Some(row)
}

/// The widgets of a page below `row`, in the row's list: the Keyboard page's layout list and
/// option browser, the Wallpaper page's pictures and its notice about wallpaper tools.
pub(super) fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    keyboard::below(model, row)
        .into_iter()
        .chain(wallpaper::below(model, row))
        .collect()
}
