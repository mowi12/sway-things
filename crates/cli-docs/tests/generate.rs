// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the generator writes: a man page per command, the extra sections of `sway-things.1` and
//! the completions of each shell.

use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::CommandFactory as _;
use cli_docs::files;
use sway_things_cli::{Cli, Exit};

fn generated() -> std::io::Result<BTreeMap<PathBuf, String>> {
    Ok(files()?
        .into_iter()
        .map(|(path, content)| (path, String::from_utf8_lossy(&content).into_owned()))
        .collect())
}

#[test]
fn there_is_a_page_per_command_and_a_completion_per_shell() {
    let mut expected: Vec<String> = Cli::command()
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .map(|subcommand| format!("man/sway-things-{}.1", subcommand.get_name()))
        .collect();
    expected.extend(
        [
            "man/sway-things.1",
            "man/sway-things-gui.1",
            "completions/sway-things.bash",
            "completions/_sway-things",
            "completions/sway-things.fish",
        ]
        .map(str::to_owned),
    );
    expected.sort();
    let paths: Vec<String> = generated()
        .unwrap()
        .keys()
        .map(|path| path.display().to_string())
        .collect();
    assert_eq!(paths, expected);
}

#[test]
fn every_page_has_one_preamble_and_one_title() {
    for (path, page) in generated()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.starts_with("man"))
    {
        let name = path.file_stem().unwrap().to_str().unwrap();
        assert!(
            page.starts_with(".ie \\n(.g .ds Aq \\(aq\n.el .ds Aq '\n"),
            "{path:?}"
        );
        assert_eq!(page.matches(".ds Aq").count(), 2, "{path:?}");
        let titles: Vec<&str> = page
            .lines()
            .filter(|line| line.starts_with(".TH "))
            .collect();
        assert_eq!(
            titles,
            [format!(
                ".TH {name} 1 \"\" \"Sway Things {}\" \"Sway Things Manual\"",
                env!("CARGO_PKG_VERSION")
            )],
            "{path:?}"
        );
    }
}

#[test]
fn the_main_page_has_the_extra_sections_and_every_exit_code() {
    let files = generated().unwrap();
    let page = &files[&PathBuf::from("man/sway-things.1")];
    let sections: Vec<&str> = page
        .lines()
        .filter_map(|line| line.strip_prefix(".SH "))
        .collect();
    assert_eq!(
        sections,
        [
            "NAME",
            "SYNOPSIS",
            "DESCRIPTION",
            "OPTIONS",
            "SUBCOMMANDS",
            "\"EXIT STATUS\"",
            "ENVIRONMENT",
            "FILES",
            "VERSION",
            "\"SEE ALSO\""
        ]
    );
    for exit in Exit::ALL {
        assert!(
            page.contains(&format!("\n.TP\n\\fB{}\\fR\n", exit.code())),
            "{exit:?}"
        );
    }
    for variable in ["SWAY_THINGS_GUI", "SWAY_THINGS_LOCALEDIR", "SWAYSOCK"] {
        assert!(page.contains(&format!("\\fB{variable}\\fR")), "{variable}");
    }
    assert!(page.contains("\\fBsway\\-things\\-gui\\fR(1)"));
}

#[test]
fn the_zsh_completion_starts_with_compdef() {
    let files = generated().unwrap();
    assert!(
        files[&PathBuf::from("completions/_sway-things")].starts_with("#compdef sway-things\n")
    );
}
