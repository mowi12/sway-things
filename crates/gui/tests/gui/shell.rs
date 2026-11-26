// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window around the pages: the sidebar from the keyboard, links, search, narrow mode and the
//! scroll lines.

use super::*;
/// Arrow keys move the focus through the sidebar without opening pages; Enter opens one.
pub(crate) fn check_sidebar_keyboard() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let sidebar = window.sidebar();
    let list = sidebar.list();

    ensure(
        window.page() == Page::ALL.first().copied(),
        "the first page isn't open",
    )?;
    ensure(sidebar.focus_selected(), "the sidebar can't take the focus")?;

    list.emit_move_cursor(gtk::MovementStep::DisplayLines, 1, false, false);
    let focused = list.focus_child().and_downcast::<gtk::ListBoxRow>();
    ensure(
        focused.as_ref().map(gtk::ListBoxRow::index) == Some(1),
        "Down doesn't move the focus to the second page",
    )?;
    ensure(
        window.page() == Page::ALL.first().copied(),
        "moving the focus opened a page",
    )?;

    list.emit_activate_cursor_row();
    ensure(
        window.page() == Page::ALL.get(1).copied(),
        "Enter doesn't open the focused page",
    )?;
    ensure(
        sidebar.selected() == Page::ALL.get(1).copied(),
        "the opened page isn't marked in the sidebar",
    )?;

    // Down from the last page of a group lands on the first page of the next, past the line and
    // the heading; only the groups after the first start below a line.
    let boundary = Page::ALL
        .windows(2)
        .position(|pair| pair[0].group() != pair[1].group())
        .and_then(|index| i32::try_from(index).ok())
        .ok_or_else(|| "the sidebar has only one group".to_owned())?;
    ensure(
        list.row_at_index(boundary)
            .is_some_and(|row| row.grab_focus()),
        "the last page of the first group can't take the focus",
    )?;
    list.emit_move_cursor(gtk::MovementStep::DisplayLines, 1, false, false);
    let focused = list.focus_child().and_downcast::<gtk::ListBoxRow>();
    ensure(
        focused.as_ref().map(gtk::ListBoxRow::index) == Some(boundary + 1),
        "Down doesn't skip the group separator and heading",
    )?;
    let has_separator = |index: i32| {
        list.row_at_index(index)
            .and_then(|row| row.header())
            .and_then(|header| header.first_child())
            .is_some_and(|child| child.is::<gtk::Separator>())
    };
    ensure(!has_separator(0), "the first group starts below a line")?;
    ensure(
        has_separator(boundary + 1),
        "the second group doesn't start below a line",
    )?;
    ensure(
        !has_separator(1),
        "a line separates two pages of the same group",
    )?;

    let mut unnamed = Vec::new();
    let mut index = 0;
    while let Some(row) = list.row_at_index(index) {
        if !row.is_focusable() {
            unnamed.push(format!("row {index} can't take the focus"));
        }
        index += 1;
    }
    ensure(
        usize::try_from(index).ok() == Some(Page::ALL.len()),
        "the sidebar doesn't list every page",
    )?;
    ensure(unnamed.is_empty(), &unnamed.join(", "))?;
    window.destroy();
    Ok(())
}

/// Esc in the content pane moves the focus back to the open page in the sidebar.
pub(crate) fn check_escape() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    window.show_page(Page::Keyboard);
    ensure(
        !window.back_to_sidebar(),
        "Esc acted with the focus in the sidebar",
    )?;

    // Unbuilt pages have nothing to focus, so a stand-in takes the focus in the content pane.
    let content = window
        .content()
        .ok_or_else(|| "  no content pane".to_owned())?;
    content.set_focusable(true);
    ensure(
        content.grab_focus(),
        "the content pane can't take the focus",
    )?;
    ensure(
        window.back_to_sidebar(),
        "Esc didn't go back to the sidebar",
    )?;
    let focused = gtk::prelude::GtkWindowExt::focus(&window);
    let row = focused.and_downcast::<gtk::ListBoxRow>();
    ensure(
        row.map(|row| row.index())
            == Page::ALL
                .iter()
                .position(|p| *p == Page::Keyboard)
                .and_then(|i| i32::try_from(i).ok()),
        "the focus isn't on the open page's row",
    )?;
    window.destroy();
    Ok(())
}

/// The checks of the deep links.
pub(crate) fn check_links() -> Vec<(String, Outcome)> {
    vec![
        ("deep links: page and setting".to_owned(), check_open_link()),
        (
            "deep links: a hidden row only opens the page".to_owned(),
            check_open_hidden_link(),
        ),
        (
            "deep links: a member of a group expands it".to_owned(),
            check_open_group_link(),
        ),
    ]
}

/// A link to a page opens it; a link to a setting opens its page, tints its row and gives its
/// control the focus, and the tint goes again.
pub(crate) fn check_open_link() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let registry = Registry::builtin();

    let link = Link::parse("windows", registry).map_err(|error| error.to_string())?;
    window.open_link(&link);
    ensure(
        window.page() == Some(Page::Windows),
        "  the page didn't open",
    )?;

    let key = "input.keyboard.repeat_rate";
    let link = Link::parse(key, registry).map_err(|error| error.to_string())?;
    window.open_link(&link);
    ensure(
        window.page() == Some(Page::Keyboard),
        "  the setting's page didn't open",
    )?;
    let row = row(&window, key)?;
    let tinted = || {
        row.parent()
            .is_some_and(|item| item.has_css_class(style::HIGHLIGHT))
    };
    ensure(tinted(), "  the row isn't tinted")?;
    let focused = run_until(Duration::from_secs(5), || {
        gtk::prelude::GtkWindowExt::focus(&window).is_some_and(|focus| focus.is_ancestor(&row))
    });
    ensure(focused, "  the row's control didn't get the focus")?;
    ensure(
        run_until(Duration::from_secs(5), || !tinted()),
        "  the tint didn't go",
    )?;
    window.destroy();
    Ok(())
}

/// A link to a setting of an input type without devices opens the page and points to nothing.
pub(crate) fn check_open_hidden_link() -> Outcome {
    let sway = obedient_with_inputs(SWAY_1_10, format!("[{KEYBOARD}, {MOUSE}]"));
    let fixture = live_fixture(&sway, "version = 1\n")?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let link = Link::parse("input.touchpad.tap", Registry::builtin())
        .map_err(|error| error.to_string())?;
    window.open_link(&link);
    ensure(
        window.page() == Some(Page::MouseTouchpad),
        "  the page didn't open",
    )?;
    let row = row(&window, "input.touchpad.tap")?;
    ensure(
        row.parent()
            .is_none_or(|item| !item.has_css_class(style::HIGHLIGHT)),
        "  a hidden row is tinted",
    )?;
    window.destroy();
    Ok(())
}

/// The checks of the search in the sidebar.
pub(crate) fn check_searches() -> Vec<(String, Outcome)> {
    vec![
        (
            "search: a result opens its setting".to_owned(),
            check_search(),
        ),
        (
            "search: pages are found by name".to_owned(),
            check_search_pages(),
        ),
        (
            "search: a member of a group".to_owned(),
            check_search_group(),
        ),
        (
            "search: the sidebar keeps its width".to_owned(),
            check_search_width(),
        ),
    ]
}

/// A member of a group is found by the group's title, its result says where it is, and opening
/// it expands the group and tints the member's row.
pub(crate) fn check_search_group() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let sidebar = window.sidebar();
    sidebar.search("focused window border");
    let first = sidebar
        .results()
        .row_at_index(0)
        .ok_or("  \"focused window border\" finds nothing")?;
    let texts: Vec<String> = all::<gtk::Label>(first.upcast_ref())
        .iter()
        .map(|label| label.label().to_string())
        .collect();
    ensure(
        texts == ["Title bar border", "Windows › Focused window"],
        &format!("  the first result reads {texts:?}"),
    )?;
    first.activate();
    ensure(
        window.page() == Some(Page::Windows),
        "  the result's page didn't open",
    )?;
    ensure(
        group(&window, "windows.focused")?.is_expanded(),
        "  the result didn't expand its group",
    )?;
    let row = row(&window, "windows.focused.border")?;
    ensure(
        row.parent()
            .is_some_and(|item| item.is_visible() && item.has_css_class(style::HIGHLIGHT)),
        "  the member's row isn't shown and tinted",
    )?;
    window.destroy();
    Ok(())
}

/// Searching shows results instead of the pages; a result opens its page and tints and focuses
/// its row. Esc goes from the page back to the result, then ends the search.
pub(crate) fn check_search() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let sidebar = window.sidebar();
    let results = sidebar.results();

    ensure(!sidebar.is_searching(), "  the sidebar starts searching")?;
    sidebar.search("DPI");
    ensure(sidebar.is_searching(), "  the results aren't shown")?;
    ensure(
        results.row_at_index(1).is_some(),
        "  \"DPI\" doesn't find the pointer speed of mice and touchpads",
    )?;

    ensure(
        sidebar.focus_selected(),
        "  the results can't take the focus",
    )?;
    let focused = results.focus_child().and_downcast::<gtk::ListBoxRow>();
    ensure(
        focused.as_ref().map(gtk::ListBoxRow::index) == Some(0),
        "  the first result doesn't get the focus",
    )?;
    results.emit_activate_cursor_row();
    ensure(
        window.page() == Some(Page::MouseTouchpad),
        "  the result's page didn't open",
    )?;
    ensure(
        sidebar.is_searching(),
        "  opening a result ended the search",
    )?;
    let key = "input.pointer.pointer_accel";
    let row = row(&window, key)?;
    ensure(
        row.parent()
            .is_some_and(|item| item.has_css_class(style::HIGHLIGHT)),
        "  the setting's row isn't tinted",
    )?;
    let focused = run_until(Duration::from_secs(5), || {
        gtk::prelude::GtkWindowExt::focus(&window).is_some_and(|focus| focus.is_ancestor(&row))
    });
    ensure(focused, "  the setting's control didn't get the focus")?;

    ensure(
        window.back_to_sidebar(),
        "  Esc didn't go back to the results",
    )?;
    let focus = gtk::prelude::GtkWindowExt::focus(&window);
    ensure(
        focus
            .and_downcast::<gtk::ListBoxRow>()
            .is_some_and(|row| row.index() == 0 && row.is_ancestor(results)),
        "  the focus isn't on the opened result",
    )?;
    ensure(sidebar.stop_search(), "  Esc didn't end the search")?;
    ensure(!sidebar.is_searching(), "  the pages aren't shown again")?;
    ensure(
        sidebar.search_entry().text().is_empty(),
        "  the search field isn't cleared",
    )?;
    ensure(
        gtk::prelude::GtkWindowExt::focus(&window)
            .is_some_and(|focus| focus.is_ancestor(sidebar.search_entry())),
        "  the search field doesn't have the focus",
    )?;
    ensure(
        !sidebar.stop_search(),
        "  Esc ended a search that wasn't there",
    )?;

    sidebar.search("no such setting anywhere");
    ensure(
        sidebar.is_searching() && results.row_at_index(0).is_none(),
        "  a query that matches nothing shows results",
    )?;
    window.destroy();
    Ok(())
}

/// A page without settings is found by its name, and Enter in the search field opens the best
/// result.
pub(crate) fn check_search_pages() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let sidebar = window.sidebar();
    sidebar.search("bluetooth");
    ensure(
        sidebar.results().row_at_index(0).is_some(),
        "  \"bluetooth\" finds nothing",
    )?;
    sidebar.search_entry().emit_activate();
    ensure(
        window.page() == Some(Page::Bluetooth),
        "  Enter didn't open the page",
    )?;
    window.destroy();
    Ok(())
}

/// The sidebar is as wide with long results, and with none, as with the pages, and doesn't ask
/// for more, which a wider window would give it. Large text makes the results longer than the
/// sidebar is wide.
pub(crate) fn check_search_width() -> Outcome {
    let settings = gtk::Settings::default().ok_or("  no GTK settings")?;
    settings.set_gtk_font_name(Some("Sans 14"));
    let outcome = search_widths();
    settings.reset_property("gtk-font-name");
    outcome
}

/// The problems of [`check_search_width`].
pub(crate) fn search_widths() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let sidebar = window.sidebar();
    let pages = sidebar.width();
    let mut problems = String::new();
    for query in ["", "trackpoint", "", "no such setting anywhere"] {
        sidebar.search(query);
        settle(&window)?;
        let width = sidebar.width();
        let (_, natural, _, _) = sidebar.measure(gtk::Orientation::Horizontal, -1);
        if width != pages || natural != pages {
            let _ = writeln!(
                problems,
                "    the sidebar is {width} px wide and asks for {natural} px after searching \
                 \"{query}\", {pages} px with the pages"
            );
        }
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The sidebar with search results, and with none, checked like the pages.
pub(crate) fn check_search_screens(screenshots: Option<&Path>, scheme: &str) -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    let mut problems = String::new();
    for (query, name) in [("scroll", "search"), ("no such setting", "search-nothing")] {
        window.sidebar().search(query);
        let directory = screenshots.map(|dir| dir.join(name));
        if let Err(problem) = check_page(&window, Page::MouseTouchpad, directory.as_deref(), scheme)
        {
            problems.push_str(&problem);
        }
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// A link to a member of a collapsed group expands the group, then tints the member's row and
/// gives its control the focus.
pub(crate) fn check_open_group_link() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    window.show_page(Page::Windows);
    let group = group(&window, "windows.focused")?;
    ensure(!group.is_expanded(), "  the group starts expanded")?;

    let key = "windows.focused.indicator";
    let link = Link::parse(key, Registry::builtin()).map_err(|error| error.to_string())?;
    window.open_link(&link);
    ensure(group.is_expanded(), "  the link didn't expand the group")?;
    let row = row(&window, key)?;
    ensure(
        row.parent()
            .is_some_and(|item| item.is_visible() && item.has_css_class(style::HIGHLIGHT)),
        "  the member's row isn't shown and tinted",
    )?;
    let focused = run_until(Duration::from_secs(5), || {
        gtk::prelude::GtkWindowExt::focus(&window).is_some_and(|focus| focus.is_ancestor(&row))
    });
    ensure(focused, "  the member's control didn't get the focus")?;
    window.destroy();
    Ok(())
}

/// The checks of narrow mode.
pub(crate) fn check_narrow() -> Vec<(String, Outcome)> {
    vec![
        (
            "narrow: the real pages fit next to the sidebar at the threshold".to_owned(),
            check_narrow_threshold(),
        ),
        (
            "narrow: every page fits a third of a laptop screen".to_owned(),
            check_tile_width(),
        ),
        (
            "narrow: a narrow row puts its controls below its text".to_owned(),
            check_row_wrap(),
        ),
        (
            "narrow: a row's text that can't wrap keeps its minimum width".to_owned(),
            check_row_wide_text(),
        ),
        (
            "narrow: one pane at a time, Esc and the back button".to_owned(),
            check_narrow_panes(),
        ),
        (
            "narrow: switching keeps the page and the focus".to_owned(),
            check_narrow_switch(),
        ),
        (
            "narrow: search and deep links".to_owned(),
            check_narrow_search(),
        ),
        (
            "narrow: the lines below the search field and the back bar".to_owned(),
            check_scroll_lines(),
        ),
    ]
}

/// The pages of v0.1, whose widths decide the threshold.
pub(crate) const REAL_PAGES: [Page; 4] = [
    Page::MouseTouchpad,
    Page::Keyboard,
    Page::Windows,
    Page::WindowBehavior,
];

/// Makes the panes of `window` narrow or wide, as resizing the window across the threshold
/// does, and waits for the new layout.
pub(crate) fn set_narrow(window: &Window, narrow: bool) -> Outcome {
    let panes = window.panes().ok_or("  the window has no panes")?;
    panes.set_threshold(if narrow { i32::MAX } else { 0 });
    let switched = run_until(Duration::from_secs(5), || {
        panes.is_narrow() == narrow && window.back_button().is_visible() == narrow
    });
    ensure(
        switched,
        if narrow {
            "  the window didn't get narrow"
        } else {
            "  the window didn't get wide"
        },
    )?;
    settle(window)
}

/// Which panes of `window` are shown (mapped): the sidebar, and the content pane.
pub(crate) fn shown_panes(window: &Window) -> (bool, bool) {
    let content = window.back_button().parent().and_then(|bar| bar.parent());
    (
        window.sidebar().is_mapped(),
        content.is_some_and(|content| content.is_mapped()),
    )
}

/// The widget with the keyboard focus in `window`.
pub(crate) fn focus(window: &Window) -> Option<gtk::Widget> {
    gtk::prelude::GtkWindowExt::focus(window)
}

/// A third of a laptop screen 1646 px wide (2880 px at 175 %), less the borders of a tiled window:
/// every page fits in it without scrolling sideways.
pub(crate) const TILE_WIDTH: i32 = 544;

/// Every real page, with its groups expanded, fits next to the sidebar in a window as wide as
/// the threshold with the controls of every row beside their text, and the window can get
/// narrower than that.
///
/// The width a page needs is measured, not tried: in a tiled window the compositor sets the size.
/// A row needs [`SettingRow::beside_width`], plus what the page puts around it (its padding, the
/// indent of a group's member), plus the page's margins.
pub(crate) fn check_narrow_threshold() -> Outcome {
    let fixture = fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    let (_, sidebar, _, _) = window.sidebar().measure(gtk::Orientation::Horizontal, -1);
    let mut problems = String::new();
    for page in REAL_PAGES {
        window.show_page(page);
        for group in page_view(&window).iter().flat_map(PageView::groups) {
            group.set_expanded(true);
        }
        settle(&window)?;
        let view = page_view(&window).ok_or("  no page view")?;
        // The rows of a page shown just now get their widths with the next layout.
        run_until(Duration::from_secs(5), || {
            rows(view.upcast_ref())
                .iter()
                .any(|row| row.is_mapped() && row.width() > 0)
        });
        let content = view.content();
        let Some((needed, row)) = rows(view.upcast_ref())
            .into_iter()
            .filter(|row| row.is_mapped() && row.width() > 0)
            .map(|row| {
                let around = content.width() - row.width();
                (row.beside_width() + around, row)
            })
            .max_by_key(|(needed, _)| *needed)
        else {
            let _ = writeln!(problems, "    {} shows no rows", page.id());
            continue;
        };
        let width = view.width_for(needed);
        // The separator between the panes is 1 px wide.
        if sidebar + 1 + width > NARROW_WIDTH {
            let _ = writeln!(
                problems,
                "    {} needs {width} px next to the {sidebar} px sidebar for the controls of {} \
                 beside its text, more than the {NARROW_WIDTH} px of the threshold",
                page.id(),
                row.setting().map_or("a row", |setting| setting.key())
            );
        }
    }
    let panes = window.panes().ok_or("  the window has no panes")?;
    let (minimum, _, _, _) = panes.measure(gtk::Orientation::Horizontal, -1);
    if minimum >= NARROW_WIDTH {
        let _ = writeln!(
            problems,
            "    the window can't get narrower than {minimum} px, not below the threshold"
        );
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Every page, with its groups expanded, fits in a window a third of a laptop screen wide
/// without scrolling sideways; narrower still, it scrolls sideways instead of being cut off.
pub(crate) fn check_tile_width() -> Outcome {
    let fixture = fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true)?;
    let window = Window::with_model(&fixture.model);
    // The Sound page with its devices and apps, each with long names, if the stand-in runs.
    let sound = sound_stand_in().and_then(|stand_in| {
        let long =
            "Tiger_Lake-LP_Smart_Sound_Technology_Audio_Controller_HDMI_DisplayPort_3_Output";
        stand_in
            .add_output("long", long)
            .map_err(|error| error.to_string())?;
        stand_in
            .pactl(&["set-default-sink", "long"])
            .map_err(|error| error.to_string())?;
        Ok(stand_in)
    });
    if let Ok(sound) = &sound {
        window.use_sound_server(SoundServer::Address(sound.server()));
    }
    window.present();
    settle(&window)?;
    let mut problems = String::new();
    for page in Page::ALL {
        window.show_page(page);
        let view = page_view(&window).ok_or("  no page view")?;
        for group in view.groups() {
            group.set_expanded(true);
        }
        settle(&window)?;
        if page == Page::Sound && sound.is_ok() {
            // Until the server's devices are shown.
            run_until(Duration::from_secs(5), || false);
            settle(&window)?;
        }
        if !view.fits(TILE_WIDTH) {
            let (content, _, _, _) = view.content().measure(gtk::Orientation::Horizontal, -1);
            let _ = writeln!(
                problems,
                "    {} needs {} px, more than {TILE_WIDTH} px",
                page.id(),
                view.width_for(content)
            );
        }
        let scroller = find::<gtk::ScrolledWindow>(view.upcast_ref()).ok_or("  no scroller")?;
        if scroller.hscrollbar_policy() != gtk::PolicyType::Automatic {
            let _ = writeln!(problems, "    {} can't scroll sideways", page.id());
        }
    }
    let (minimum, _, _, _) = window
        .panes()
        .ok_or("  the window has no panes")?
        .measure(gtk::Orientation::Horizontal, -1);
    if minimum > TILE_WIDTH / 2 {
        let _ = writeln!(
            problems,
            "    the window can't get narrower than {minimum} px, so a narrower tile cuts it off"
        );
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// A slider row has its controls beside its text down to its [`SettingRow::beside_width`], with
/// the slider shrinking first; narrower, the controls are below the text, so the row gets
/// higher, and it can get narrower than that.
pub(crate) fn check_row_wrap() -> Outcome {
    let fixture = fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    window.show_page(Page::MouseTouchpad);
    settle(&window)?;
    let view = page_view(&window).ok_or("  no page view")?;
    let row = rows(view.upcast_ref())
        .into_iter()
        .find(|row| row.controls().iter().any(ObjectExt::is::<gtk::Scale>))
        .ok_or("  no slider row")?;
    let beside = row.beside_width();
    let (minimum, natural, _, _) = row.measure(gtk::Orientation::Horizontal, -1);
    let height = |width: i32| row.measure(gtk::Orientation::Vertical, width).0;
    let mut problems = String::new();
    if natural <= beside {
        let _ = writeln!(
            problems,
            "    the slider doesn't shrink before the controls move: natural {natural} px, beside \
             down to {beside} px"
        );
    }
    if height(beside - 1) <= height(beside) {
        let _ = writeln!(
            problems,
            "    at {} px the row is {} px high, not higher than {} px at {beside} px: the \
             controls didn't move below the text",
            beside - 1,
            height(beside - 1),
            height(beside)
        );
    }
    if minimum >= beside {
        let _ = writeln!(
            problems,
            "    the row can't get narrower than {minimum} px, the width it needs beside"
        );
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// A row whose text can't wrap below a width wider than the text keeps beside its controls (the
/// Wallpaper page's tabs, a stack switcher, beside the tab's buttons): the controls stay beside it
/// only while it gets its minimum width, and move below it on a narrower row. Measuring it below
/// its minimum makes GTK warn, which the group of these checks fails on.
pub(crate) fn check_row_wide_text() -> Outcome {
    let stack = gtk::Stack::new();
    for title in ["Pictures with a long name", "Collections", "Colors"] {
        stack.add_titled(&gtk::Label::new(None), Some(title), title);
    }
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.append(&gtk::Button::with_label("Add Pictures…"));
    buttons.append(&gtk::Button::with_label("Remove"));
    let layout = RowLayout::default();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.set_layout_manager(Some(layout.clone()));
    row.append(&switcher);
    row.append(&buttons);
    let window = gtk::Window::new();
    window.set_child(Some(&row));
    window.present();
    let (text_minimum, _, _, _) = switcher.measure(gtk::Orientation::Horizontal, -1);
    let (controls_minimum, _, _, _) = buttons.measure(gtk::Orientation::Horizontal, -1);
    let beside = layout.beside_width(&row);
    let height = |width: i32| row.measure(gtk::Orientation::Vertical, width).0;
    let mut problems = String::new();
    if text_minimum <= sway_things_gui::widgets::TEXT_WIDTH {
        let _ = writeln!(
            problems,
            "    the switcher needs only {text_minimum} px, so this checks nothing"
        );
    }
    if beside < text_minimum + controls_minimum {
        let _ = writeln!(
            problems,
            "    the controls stay beside the text down to {beside} px, but the text needs \
             {text_minimum} px and the controls {controls_minimum} px"
        );
    }
    let (minimum, _, _, _) = row.measure(gtk::Orientation::Horizontal, -1);
    for width in [minimum, beside - 1, beside, beside + 40] {
        let _ = height(width);
    }
    if height(beside - 1) <= height(beside) {
        let _ = writeln!(
            problems,
            "    at {} px the row isn't higher than at {beside} px: the controls didn't move below \
             the text",
            beside - 1
        );
    }
    window.destroy();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Narrow, the window shows the sidebar, then the page with the back button; Esc and the back
/// button go back to the sidebar, at the open page.
pub(crate) fn check_narrow_panes() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, true) && !window.back_button().is_visible(),
        "  the window doesn't start with both panes and no back button",
    )?;
    let sidebar = window.sidebar();
    ensure(
        sidebar.focus_selected(),
        "  the sidebar can't take the focus",
    )?;
    set_narrow(&window, true)?;
    ensure(
        shown_panes(&window) == (true, false),
        "  with the focus in the sidebar, narrow doesn't show only the sidebar",
    )?;

    let list = sidebar.list();
    list.emit_move_cursor(gtk::MovementStep::DisplayLines, 1, false, false);
    list.emit_activate_cursor_row();
    settle(&window)?;
    ensure(
        window.page() == Page::ALL.get(1).copied(),
        "  Enter doesn't open the page",
    )?;
    ensure(
        shown_panes(&window) == (false, true),
        "  the page doesn't replace the sidebar",
    )?;
    let content = window.content().ok_or("  no content pane")?;
    let back = window.back_button();
    ensure(
        focus(&window).is_some_and(|focus| {
            focus.is_ancestor(&content) || focus == *back.upcast_ref::<gtk::Widget>()
        }),
        "  the focus didn't move to the page",
    )?;

    ensure(window.back_to_sidebar(), "  Esc didn't go back")?;
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, false),
        "  Esc doesn't show the sidebar instead of the page",
    )?;
    let focused = focus(&window)
        .and_downcast::<gtk::ListBoxRow>()
        .map(|row| row.index());
    ensure(
        focused == Some(1),
        "  the focus isn't on the open page's row",
    )?;
    ensure(
        !window.back_to_sidebar(),
        "  Esc went back from the sidebar",
    )?;

    window.show_page(Page::Keyboard);
    settle(&window)?;
    ensure(
        shown_panes(&window) == (false, true),
        "  opening a page doesn't show it",
    )?;
    back.emit_clicked();
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, false) && sidebar.selected() == Some(Page::Keyboard),
        "  the back button doesn't show the sidebar at the open page",
    )?;

    set_narrow(&window, false)?;
    ensure(
        shown_panes(&window) == (true, true) && window.page() == Some(Page::Keyboard),
        "  wide again, the window doesn't show both panes with the open page",
    )?;
    window.destroy();
    Ok(())
}

/// A window of `fixture` with `page` open, shown and laid out.
pub(crate) fn window_at(fixture: &Fixture, page: Page) -> Result<Window, String> {
    let window = Window::with_model(&fixture.model);
    window.present();
    settle(&window)?;
    window.show_page(page);
    settle(&window)?;
    Ok(window)
}

/// Switching between narrow and wide keeps the open page, the pane with the focus and the focus.
pub(crate) fn check_narrow_switch() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = window_at(&fixture, Page::Keyboard)?;
    let row = row(&window, "input.keyboard.repeat_rate")?;
    ensure(
        row.child_focus(gtk::DirectionType::TabForward),
        "  the row's control can't take the focus",
    )?;
    let focused = focus(&window).ok_or("  nothing has the focus")?;

    set_narrow(&window, true)?;
    ensure(
        shown_panes(&window) == (false, true) && window.page() == Some(Page::Keyboard),
        "  with the focus on the page, narrow doesn't show the page",
    )?;
    ensure(
        focus(&window).as_ref() == Some(&focused),
        "  getting narrow moved the focus",
    )?;
    set_narrow(&window, false)?;
    ensure(
        shown_panes(&window) == (true, true) && window.page() == Some(Page::Keyboard),
        "  wide again, the window doesn't show both panes with the open page",
    )?;
    ensure(
        focus(&window).as_ref() == Some(&focused),
        "  getting wide moved the focus",
    )?;
    window.destroy();
    Ok(())
}

/// Narrow, a search typed on the page shows the sidebar with the results, a result opens its
/// page and points to its row, Esc goes back to the result, the search field shows the sidebar,
/// and a deep link opens its page from the sidebar.
pub(crate) fn check_narrow_search() -> Outcome {
    let fixture = fixture(SWAYFX, STORED, USERS, true)?;
    let window = window_at(&fixture, Page::Keyboard)?;
    set_narrow(&window, true)?;
    window.show_page(Page::Keyboard);
    settle(&window)?;
    ensure(
        shown_panes(&window) == (false, true),
        "  opening a page doesn't show it",
    )?;
    let sidebar = window.sidebar();
    sidebar.search("DPI");
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, false) && sidebar.is_searching(),
        "  a search doesn't show the sidebar with the results",
    )?;
    ensure(
        focus(&window).is_some_and(|focus| focus.is_ancestor(sidebar.search_entry())),
        "  the search field doesn't have the focus",
    )?;
    let results = sidebar.results();
    ensure(
        sidebar.focus_selected(),
        "  the results can't take the focus",
    )?;
    results.emit_activate_cursor_row();
    settle(&window)?;
    ensure(
        window.page() == Some(Page::MouseTouchpad) && shown_panes(&window) == (false, true),
        "  the result doesn't show its page",
    )?;
    let accel = row(&window, "input.pointer.pointer_accel")?;
    let pointed = run_until(Duration::from_secs(5), || {
        focus(&window).is_some_and(|focus| focus.is_ancestor(&accel))
    });
    ensure(pointed, "  the setting's control didn't get the focus")?;
    ensure(window.back_to_sidebar(), "  Esc didn't go back")?;
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, false)
            && focus(&window)
                .and_downcast::<gtk::ListBoxRow>()
                .is_some_and(|row| row.index() == 0 && row.is_ancestor(results)),
        "  Esc doesn't show the results with the focus on the opened one",
    )?;
    ensure(sidebar.stop_search(), "  Esc didn't end the search")?;

    // Ctrl+F on the page.
    window.show_page(Page::Keyboard);
    settle(&window)?;
    ensure(
        sidebar.focus_search(),
        "  the search field can't take the focus",
    )?;
    settle(&window)?;
    ensure(
        shown_panes(&window) == (true, false),
        "  the search field with the focus doesn't show the sidebar",
    )?;

    let key = "windows.focused.indicator";
    let link = Link::parse(key, Registry::builtin()).map_err(|error| error.to_string())?;
    window.open_link(&link);
    settle(&window)?;
    ensure(
        window.page() == Some(Page::Windows) && shown_panes(&window) == (false, true),
        "  a deep link doesn't show its page",
    )?;
    let row = row(&window, key)?;
    let pointed = run_until(Duration::from_secs(5), || {
        focus(&window).is_some_and(|focus| focus.is_ancestor(&row))
    });
    ensure(pointed, "  the deep link's row didn't get the focus")?;
    window.destroy();
    Ok(())
}

/// The lines below the search field and the back bar grow from the center as the list or page
/// below them scrolls, keep their height, and the one below the back bar shows only while
/// narrow.
pub(crate) fn check_scroll_lines() -> Outcome {
    let shoot = |window: &Window, name: &str| -> Outcome {
        if let Some(directory) = std::env::var_os("SWAY_THINGS_SCREENSHOTS") {
            check_screen(
                window,
                Some(PathBuf::from(directory).join(format!("{name}.png"))),
            )?;
        }
        Ok(())
    };
    let fixture = fixture(SWAYFX, &format!("{STORED}{DIRECTIVES}"), USERS, true)?;
    let window = Window::with_model(&fixture.model);
    window.set_default_size(1100, 300);
    window.present();
    settle(&window)?;
    let line = window.sidebar().line();
    let height = line.height();
    ensure(
        line.fraction() == 0.0 && line.line_width() == 0 && height >= 1,
        "  the line below the search field shows at the top, or takes no height",
    )?;
    let scroller = find::<gtk::ScrolledWindow>(window.sidebar().upcast_ref())
        .ok_or("  the sidebar has no scrolled window")?;
    let adjustment = scroller.vadjustment();
    ensure(
        adjustment.upper() - adjustment.page_size() > FULL_OFFSET,
        "  the page list doesn't scroll far enough to test",
    )?;
    shoot(&window, "line-sidebar-top")?;
    adjustment.set_value(FULL_OFFSET / 2.0);
    wait_for_paint(&window)?;
    shoot(&window, "line-sidebar-half")?;
    let half = line.line_width();
    ensure(
        (line.fraction() - 0.5).abs() < 0.01 && (half - line.width() / 2).abs() <= 1,
        &format!("  half way, the line is {half} of {} px wide", line.width()),
    )?;
    adjustment.set_value(FULL_OFFSET * 2.0);
    wait_for_paint(&window)?;
    shoot(&window, "line-sidebar-full")?;
    ensure(
        line.line_width() == line.width() && line.height() == height,
        "  scrolled past the offset, the line doesn't span the width at the same height",
    )?;
    adjustment.set_value(0.0);
    wait_for_paint(&window)?;
    ensure(
        line.line_width() == 0,
        &format!(
            "  back at the top, the line stays: {} px, fraction {}, offset {}",
            line.line_width(),
            line.fraction(),
            adjustment.value()
        ),
    )?;

    let back = window.back_line();
    ensure(
        !back.is_visible(),
        "  the line below the back bar shows while wide",
    )?;
    window.show_page(Page::Keyboard);
    set_narrow(&window, true)?;
    window.show_page(Page::Keyboard);
    wait_for_paint(&window)?;
    let view = page_view(&window).ok_or("  no page view")?;
    let page = find::<gtk::ScrolledWindow>(view.upcast_ref()).ok_or("  no page scroller")?;
    ensure(
        back.is_visible() && back.line_width() == 0 && back.height() >= 1,
        &format!(
            "  narrow at the top, the line below the back bar shows or takes no height: \
             visible {}, {} px wide, {} px high, offset {}",
            back.is_visible(),
            back.line_width(),
            back.height(),
            page.vadjustment().value()
        ),
    )?;
    shoot(&window, "line-back-top")?;
    page.vadjustment().set_value(FULL_OFFSET * 2.0);
    wait_for_paint(&window)?;
    shoot(&window, "line-back-full")?;
    ensure(
        back.line_width() == back.width(),
        "  the line below the back bar doesn't follow the page",
    )?;
    set_narrow(&window, false)?;
    ensure(
        !back.is_visible(),
        "  the line below the back bar stays while wide",
    )?;
    Ok(())
}
