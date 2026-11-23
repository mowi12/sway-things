// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A change keeps the place on the page: removing, changing and putting back a binding, a switch
//! of media keys and the main modifier leave the page scrolled where it was, with the focus on
//! the row that changed, or on the row after one that is gone.

use super::*;

pub(crate) fn check_places() -> Vec<(String, Outcome)> {
    vec![
        (
            "place: removing, changing and putting back a binding keep the scroll and the focus"
                .to_owned(),
            check_binding_place(),
        ),
        (
            "place: a media key switch and the main modifier keep the scroll and the focus"
                .to_owned(),
            check_switch_place(),
        ),
    ]
}

/// The scrolled window of the open page.
fn page_scroller(window: &Window) -> Result<gtk::ScrolledWindow, String> {
    page_view(window)
        .and_then(|view| find::<gtk::ScrolledWindow>(view.upcast_ref()))
        .ok_or_else(|| "  the page has no scrolled window".to_owned())
}

/// Where `widget` is in the visible part of `scroller`, from its top.
fn on_screen(widget: &impl IsA<gtk::Widget>, scroller: &gtk::ScrolledWindow) -> Option<f32> {
    widget
        .compute_point(scroller, &gtk::graphene::Point::new(0.0, 0.0))
        .map(|point| point.y())
}

/// Scrolls `scroller` so that `widget` is about in the middle, and waits for the layout.
fn scroll_to(window: &Window, scroller: &gtk::ScrolledWindow, widget: &impl IsA<gtk::Widget>) {
    scroll_to_part(window, scroller, widget, 0.5);
}

/// Scrolls `scroller` so that `widget` is `part` of the way down its visible part, and waits for
/// the layout.
fn scroll_to_part(
    window: &Window,
    scroller: &gtk::ScrolledWindow,
    widget: &impl IsA<gtk::Widget>,
    part: f64,
) {
    let adjustment = scroller.vadjustment();
    if let Some(y) = on_screen(widget, scroller) {
        adjustment.set_value(adjustment.value() + f64::from(y) - adjustment.page_size() * part);
    }
    let _ = settle(window);
}

/// Checks that `widget`, which was `before` from the top of the visible part of `scroller`, is
/// still there after `change`. A page that got shorter can't always scroll as far as that
/// needs: then it is scrolled to its end (GTK keeps it there), and the widget is lower on
/// screen than before, never higher.
fn in_place(
    widget: &impl IsA<gtk::Widget>,
    scroller: &gtk::ScrolledWindow,
    before: Option<f32>,
    change: &str,
) -> Outcome {
    let after = on_screen(widget, scroller);
    let adjustment = scroller.vadjustment();
    let value = adjustment.value();
    let end = adjustment.upper() - adjustment.page_size();
    let at_end = value >= end - 1.0;
    ensure(
        matches!((before, after), (Some(before), Some(after))
            if (before - after).abs() < 1.0 || (at_end && after > before)),
        &format!(
            "  {change} moved from {before:?} to {after:?} on screen, the page scrolled to {value} of {end}"
        ),
    )
}

/// What the row of a binding with the focus does, as the row says it.
fn focused_binding(window: &Window) -> Option<String> {
    let focus = gtk::prelude::GtkWindowExt::focus(window)?;
    let item = if focus.is::<gtk::ListBoxRow>() {
        focus
    } else {
        focus.ancestor(gtk::ListBoxRow::static_type())?
    };
    item.downcast_ref::<gtk::ListBoxRow>()?
        .child()?
        .first_child()
        .and_downcast::<gtk::Label>()
        .map(|label| label.text().to_string())
}

/// Checks that the page is scrolled to `value` and the row of `title` has the focus, after a
/// change.
fn kept(
    window: &Window,
    scroller: &gtk::ScrolledWindow,
    value: f64,
    title: &str,
    change: &str,
) -> Outcome {
    let _ = settle(window);
    let now = scroller.vadjustment().value();
    ensure(
        (now - value).abs() < 1.0,
        &format!("  {change} scrolled the page from {value} to {now}"),
    )?;
    let focused = focused_binding(window);
    ensure(
        focused.as_deref() == Some(title),
        &format!("  after {change}, the focus is on {focused:?}, not on {title:?}"),
    )
}

fn check_binding_place() -> Outcome {
    use gtk::gdk::{Key, ModifierType};
    let fixture = realistic_fixture()?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    let scroller = page_scroller(&window)?;
    let adjustment = scroller.vadjustment();
    ensure(
        adjustment.upper() > adjustment.page_size() * 1.5,
        "  the page is too short to scroll",
    )?;
    // Remove: the binding stays as a removed one, with the focus.
    let item = list_item(&window, "Split vertically")?;
    scroll_to(&window, &scroller, &item);
    let value = adjustment.value();
    ensure(value > 50.0, "  the page didn't scroll down")?;
    item.grab_focus();
    item.activate();
    opened_editor()?.remove_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    kept(&window, &scroller, value, "Split vertically", "a removal")?;
    // Put back: its button goes away, the focus stays on the row.
    let removed = list_item(&window, "Split vertically")?;
    let restore = all::<gtk::Button>(removed.upcast_ref())
        .into_iter()
        .next()
        .ok_or_else(|| "  the removed binding has no button that puts it back".to_owned())?;
    restore.grab_focus();
    restore.emit_clicked();
    wait_for_model(&fixture.model)?;
    kept(
        &window,
        &scroller,
        value,
        "Split vertically",
        "putting it back",
    )?;
    // Change: new keys, the same row.
    let item = list_item(&window, "Stacked layout")?;
    item.grab_focus();
    item.activate();
    let dialog = opened_editor()?;
    dialog.start_capture();
    key_event(
        dialog.window(),
        "key-pressed",
        Key::y,
        29,
        ModifierType::SUPER_MASK,
    )?;
    dialog.save_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        fixture
            .model
            .binding_items()
            .iter()
            .any(|item| item.contains("$mod+y")),
        "  the new keys weren't stored",
    )?;
    kept(&window, &scroller, value, "Stacked layout", "a change")?;
    // A binding of Sway Things that is removed is gone: the focus goes to the row after it.
    let shown: Vec<String> = shortcut_rows(&window)
        .iter()
        .filter_map(|row| {
            row.first_child()
                .and_downcast::<gtk::Label>()
                .map(|label| label.text().to_string())
        })
        .collect();
    let next = shown
        .iter()
        .position(|title| title == "Run firefox")
        .and_then(|index| shown.get(index + 1))
        .ok_or_else(|| format!("  no row after Run firefox in {shown:?}"))?
        .clone();
    let item = list_item(&window, "Run firefox")?;
    scroll_to(&window, &scroller, &item);
    let value = adjustment.value();
    item.grab_focus();
    item.activate();
    opened_editor()?.remove_button().emit_clicked();
    wait_for_model(&fixture.model)?;
    ensure(
        list_item(&window, "Run firefox").is_err(),
        "  the removed binding of Sway Things is still listed",
    )?;
    kept(
        &window,
        &scroller,
        value,
        &next,
        "removing an added binding",
    )?;
    window.destroy();
    Ok(())
}

fn check_switch_place() -> Outcome {
    let fixture = realistic_fixture()?;
    let window = open(&fixture, Page::KeyboardShortcuts)?;
    let scroller = page_scroller(&window)?;
    // The volume keys go: their rows above the switch go with them, the switch stays in place.
    let volume = row(&window, "shortcuts.media.volume")?;
    let switch = find::<gtk::Switch>(volume.upcast_ref())
        .ok_or_else(|| "  the volume keys have no switch".to_owned())?;
    // Low on screen: the page gets shorter below the switch too, so higher up, the page would
    // end before the switch is back in place, and the check would only see GTK keep the page at
    // its end.
    scroll_to_part(&window, &scroller, &switch, 0.75);
    switch.grab_focus();
    let before = on_screen(&switch, &scroller);
    switch.set_active(false);
    wait_for_model(&fixture.model)?;
    let _ = settle(&window);
    in_place(&switch, &scroller, before, "the switch")?;
    ensure(
        gtk::prelude::GtkWindowExt::focus(&window).as_ref()
            == Some(switch.upcast_ref::<gtk::Widget>()),
        "  the switch lost the focus when it was switched off",
    )?;
    // The main modifier: every key changes, the place doesn't.
    let modifier = row(&window, "shortcuts.modifier")?;
    let choice = find::<gtk::DropDown>(modifier.upcast_ref())
        .ok_or_else(|| "  the main modifier has no drop-down".to_owned())?;
    scroll_to(&window, &scroller, &choice);
    choice.grab_focus();
    let before = on_screen(&choice, &scroller);
    choice.set_selected(1);
    wait_for_model(&fixture.model)?;
    let _ = settle(&window);
    // The keys get shorter, and the page with them: at its end, it can't stay quite in place.
    in_place(&choice, &scroller, before, "the main modifier")?;
    ensure(
        gtk::prelude::GtkWindowExt::focus(&window).is_some_and(|focus| {
            focus == *choice.upcast_ref::<gtk::Widget>() || focus.is_ancestor(&choice)
        }),
        "  the main modifier lost the focus",
    )?;
    window.destroy();
    Ok(())
}
