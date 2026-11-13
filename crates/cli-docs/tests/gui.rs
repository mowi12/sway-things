// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The man page of `sway-things-gui` describes the arguments the app reads: the clap command in
//! `cli_docs::gui_command` accepts what `sway_things_gui::args::link_argument` accepts.

use cli_docs::gui_command;
use sway_things_gui::args::{ArgumentError, link_argument};

#[test]
fn the_documented_arguments_are_the_ones_the_app_reads() {
    let cases: [&[&str]; 4] = [
        &[],
        &["keyboard"],
        &["input.keyboard.repeat_rate"],
        &["keyboard", "mouse-touchpad"],
    ];
    for arguments in cases {
        let owned: Vec<String> = arguments.iter().map(|text| (*text).to_owned()).collect();
        let read = !matches!(link_argument(&owned), Err(ArgumentError::TooMany));
        let documented = gui_command()
            .try_get_matches_from(
                std::iter::once("sway-things-gui").chain(arguments.iter().copied()),
            )
            .is_ok();
        assert_eq!(documented, read, "{arguments:?}");
    }
}

#[test]
fn the_link_is_the_only_argument() {
    let command = gui_command();
    let positionals: Vec<&str> = command
        .get_positionals()
        .map(|argument| argument.get_id().as_str())
        .collect();
    assert_eq!(positionals, ["link"]);
    assert_eq!(command.get_opts().count(), 0);
    let flags: Vec<&str> = command
        .get_arguments()
        .filter(|argument| !argument.is_positional())
        .map(|argument| argument.get_id().as_str())
        .collect();
    assert!(flags.is_empty(), "{flags:?}");
}
