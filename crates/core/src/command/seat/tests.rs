// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{hide_cursor, hide_cursor_when_typing, xcursor_theme};
use crate::command::SwayText;
use crate::command::Target;

#[test]
fn renders() {
    let theme = SwayText::new("Bibata Modern Ice").unwrap();
    let cmd = xcursor_theme(&theme, 24).unwrap();
    assert_eq!(
        cmd.render(Target::Config).unwrap(),
        r#"seat * xcursor_theme "Bibata Modern Ice" 24"#
    );
    assert_eq!(
        hide_cursor(0).unwrap().render(Target::Ipc).unwrap(),
        "seat * hide_cursor 0"
    );
    assert_eq!(
        hide_cursor(5000).unwrap().render(Target::Ipc).unwrap(),
        "seat * hide_cursor 5000"
    );
    assert_eq!(
        hide_cursor_when_typing(false).render(Target::Ipc).unwrap(),
        "seat * hide_cursor when-typing disable"
    );
}

#[test]
fn refuses_what_sway_would_change() {
    assert!(hide_cursor(50).is_err());
    assert!(hide_cursor(100).is_ok());
    let theme = SwayText::new("Adwaita").unwrap();
    assert!(xcursor_theme(&theme, 0).is_err());
    assert!(xcursor_theme(&theme, 513).is_err());
}
