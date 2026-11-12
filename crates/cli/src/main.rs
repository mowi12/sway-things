// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `sway-things`: the command-line interface of Sway Things.

use std::process::ExitCode;

use clap::Parser;
use sway_things_cli::{Cli, run_for_user};

fn main() -> ExitCode {
    // Wrong arguments end here with clap's message and exit code 2.
    let cli = Cli::parse();
    async_io::block_on(run_for_user(&cli.command)).into()
}
