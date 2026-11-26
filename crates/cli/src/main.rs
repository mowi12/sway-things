// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `sway-things`: the command-line interface of Sway Things.

use std::process::ExitCode;

use clap::Parser;
use sway_things_cli::{Cli, Command, run_for_user};

fn main() -> ExitCode {
    // The translations of what the commands print, in the user's language.
    sway_things_core::i18n::init();
    // Wrong arguments end here with clap's message and exit code 2.
    let cli = Cli::parse();
    // What `--color` says, for everything that colors its output.
    cli.color.write_global();
    // sway runs `sway-things -b <id>` as a bar of the user's that Sway Things took over.
    if let Some(id) = &cli.bar {
        return sway_things_cli::run_bar(id, &mut std::io::stderr()).into();
    }
    // Without a command, `sway-things` starts the app or brings its window up.
    let command = cli.command.unwrap_or(Command::Open { link: None });
    async_io::block_on(run_for_user(&command)).into()
}
