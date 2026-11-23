// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a page adds to the rows built from the registry, page by page, in one place: the parts
//! before and after its rows, the choices a row of it offers and what the row follows, and the
//! widgets below a row. [`PageView::for_settings`](super::PageView::for_settings) asks here and
//! names no page itself.

use gtk::prelude::*;
use sway_things_core::outputs;
use sway_things_core::registry::{ConfigFile, Page, Setting};

use super::{
    Section, ShowDaemon, about_section, add_apps_parts, add_workspace_map, arrangement_section,
    dnd_section, follow_daemon, generated_section, shortcuts_section, test_area_section,
};
use crate::model::Model;
use crate::widgets::displays::{self, Arrangement};
use crate::widgets::{SettingRow, apps, capture, cursor, keyboard, look, notifications, shortcuts};

/// What the parts above the rows hand to the rest of the page.
#[derive(Default)]
pub(super) struct Before {
    /// The arrangement of the displays, on the Displays page.
    pub arrangement: Option<Arrangement>,
    /// Where the Do Not Disturb section says which daemon the page is about, on the
    /// Notifications page.
    pub daemon: Option<ShowDaemon>,
}

/// The parts of `page` above its rows: the system details and the generated files on About &
/// Maintenance, the list of bindings on Keyboard Shortcuts, the note of Apps Look & Feel, the
/// arrangement of the displays, Do Not Disturb on Notifications. `first` says whether the next
/// section is the first of the page; a part that is a section takes it.
pub(super) fn before_rows(
    model: &Model,
    page: Page,
    body: &gtk::Box,
    sections: &mut Vec<Section>,
    first: &mut bool,
) -> Before {
    match page {
        Page::About => {
            sections.push(about_section(model, body));
            sections.push(generated_section(model, body));
            *first = false;
            Before::default()
        }
        Page::KeyboardShortcuts => {
            sections.push(shortcuts_section(model, body, std::mem::take(first)));
            Before::default()
        }
        Page::AppsLookFeel => {
            body.append(&look::note());
            Before::default()
        }
        Page::Displays => Before {
            arrangement: Some(arrangement_section(
                model,
                body,
                std::mem::take(first),
                sections,
            )),
            daemon: None,
        },
        Page::Notifications => Before {
            arrangement: None,
            daemon: Some(dnd_section(model, body, std::mem::take(first), sections)),
        },
        _ => Before::default(),
    }
}

/// The parts of `page` below its rows: the test area of Mouse & Touchpad, the map of the
/// displays on Workspaces, the note above the default apps and the list of what starts at login
/// on Apps & Autostart; on Notifications, the sections of mako and swaync follow the daemon the
/// page is about (`daemon`, from [`before_rows`]).
pub(super) fn after_rows(
    model: &Model,
    page: Page,
    body: &gtk::Box,
    sections: &mut Vec<Section>,
    daemon: Option<&ShowDaemon>,
) {
    match page {
        Page::MouseTouchpad => sections.push(test_area_section(model, body)),
        Page::Workspaces => add_workspace_map(model, sections),
        Page::AppsAutostart => add_apps_parts(model, sections),
        Page::Notifications => follow_daemon(sections, daemon),
        _ => {}
    }
}

/// The row of `setting` on its page, for the whole session or input type: a drop-down where the
/// page offers choices of its own (the keyboard models, an output's modes, the cursor and GTK
/// themes, the screens, the installed apps), following what they depend on. A setting of Keyboard
/// Shortcuts or Apps & Autostart may have no row ([`shortcuts::has_row`], [`apps::has_row`]).
pub(super) fn row(model: &Model, setting: &'static Setting) -> Option<SettingRow> {
    if !shortcuts::has_row(model, setting) || !apps::has_row(setting) {
        return None;
    }
    if setting.key() == outputs::MODE {
        return SettingRow::with_choices(model, setting, None, &displays::mode_choices(None));
    }
    let choices = keyboard::choices(setting)
        .or_else(|| cursor::choices(model, setting))
        .or_else(|| look::choices(setting))
        .or_else(|| apps::choices(setting));
    let row = match choices {
        Some(choices) => SettingRow::with_choices(model, setting, None, &choices),
        None => SettingRow::new(model, setting, None),
    }?;
    cursor::follow(model, &row);
    look::follow(&row);
    capture::follow(model, &row);
    apps::follow(&row);
    Some(row)
}

/// The widgets of a page below `row`, in the row's list: the Keyboard page's layout list and
/// option browser, what the keys of a media key switch replace on Keyboard Shortcuts, what the
/// keys of the terminal and the launcher do now on Apps & Autostart, the notes of Screenshots &
/// Clipboard, the lists of swaync's widgets and buttons on Notifications.
pub(super) fn below(model: &Model, row: &SettingRow) -> Vec<gtk::Widget> {
    let mut widgets = keyboard::below(model, row);
    widgets.extend(shortcuts::below(model, row));
    widgets.extend(apps::below(model, row));
    widgets.extend(capture::below(model, row));
    widgets.extend(notifications::below(model, row));
    widgets
}

/// The generated files `page` writes to besides those of its sway directives, for its footer:
/// the bindings and hooks of Screenshots & Clipboard.
pub(super) fn files(page: Page) -> Vec<ConfigFile> {
    capture::files(page)
}
