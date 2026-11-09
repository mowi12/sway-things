// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generates the escaping fixtures in `ci/integration/` from the command builders and checks that
//! the committed files are up to date. `scripts/check-integration.sh` feeds them to a real sway:
//!
//! - `escaping.conf` must pass `sway --validate`, and every bar ID it sets must exist afterwards;
//! - every command in `escaping-ipc.txt` must succeed, and the seat names must come back unchanged.
//!
//! After changing the builders or the names, regenerate the files with
//! `SWAY_THINGS_UPDATE_FIXTURES=1 cargo test -p sway-things-core`.

mod common;

use common::{Result, push_line};
use sway_things_core::command::{Criteria, CriteriaKey, SwayCmd, SwayText, Target};

/// Names that sway's parsers treat specially. Each one becomes a bar ID in the config and a seat
/// name over IPC (sway creates seats from the config only once a device attaches).
const NAMES: &[&str] = &[
    "with space",
    "multiple   spaces",
    "  padded",
    "semi;colon",
    "comma,list",
    "dollar$sign",
    "$mod",
    "$$",
    r#"double"quote"#,
    "single'quote",
    r#"both"and'quotes"#,
    r#""'"'"#,
    "[brackets]",
    "{brace}",
    "{",
    "}",
    "#hash",
    r#"x" ; exit ; ""#,
    "'; exit; '",
    "=",
    "ünïcödé ✓",
];

/// Criteria values, which may contain anything. sway compiles them with PCRE2 while it validates
/// the config and when it runs the IPC command.
const CRITERIA_VALUES: &[&str] = &[
    r"C:\path\file.txt",
    r#"a"b'c$d;e,f[g]h{i}j"#,
    "line\nbreak\ttab",
    "$mod",
    ".*",
    r"\",
    "ünïcödé ✓",
];

fn name(prefix: &str, name: &str) -> String {
    format!("{prefix}: {name}")
}

fn criteria(value: &str) -> Criteria {
    Criteria::new()
        .exact(CriteriaKey::AppId, value)
        .exact(CriteriaKey::Title, value)
}

/// `for_window` lines: hostile criteria with a plain command, and hostile text in the command,
/// which sway parses a second time when a window appears.
fn for_window_lines(target: Target, out: &mut String) -> Result<()> {
    for value in CRITERIA_VALUES {
        let inner = SwayCmd::new("opacity").float(0.9);
        push_line(
            out,
            &SwayCmd::for_window(criteria(value), inner)?.render(target)?,
        );
    }
    for value in NAMES {
        let inner = SwayCmd::new("title_format").text(SwayText::new(*value)?);
        push_line(
            out,
            &SwayCmd::for_window(criteria("x"), inner)?.render(target)?,
        );
    }
    Ok(())
}

/// One `bar` block per name: sway keeps the current bar across one-line `bar` commands while it
/// reads the config, so only a block starts a new bar.
fn config_fixture() -> Result<String> {
    let mut out = common::header("escaping_fixtures.rs", false);
    for value in NAMES {
        let id = SwayCmd::new("id").text(SwayText::new(name("config", value))?);
        let command = SwayCmd::new("swaybar_command").keyword("true");
        out.push_str("bar {\n");
        for cmd in [id, command] {
            push_line(&mut out, &format!("    {}", cmd.render(Target::Config)?));
        }
        out.push_str("}\n");
    }
    for_window_lines(Target::Config, &mut out)?;
    Ok(out)
}

fn ipc_fixture() -> Result<String> {
    let mut out = common::header("escaping_fixtures.rs", false);
    for value in NAMES {
        let cmd = SwayCmd::new("seat")
            .text(SwayText::new(name("ipc", value))?)
            .keyword("hide_cursor")
            .int(1000);
        push_line(&mut out, &cmd.render(Target::Ipc)?);
    }
    for_window_lines(Target::Ipc, &mut out)?;
    Ok(out)
}

fn list_fixture(mut names: Vec<String>) -> String {
    names.sort();
    let mut out = common::header("escaping_fixtures.rs", false);
    for name in names {
        push_line(&mut out, &name);
    }
    out
}

#[test]
fn fixtures_are_up_to_date() -> Result<()> {
    let bars = NAMES.iter().map(|value| name("config", value)).collect();
    let mut seats: Vec<String> = NAMES.iter().map(|value| name("ipc", value)).collect();
    seats.push("seat0".to_owned());
    let fixtures = [
        ("escaping.conf", config_fixture()?),
        ("escaping-bars.txt", list_fixture(bars)),
        ("escaping-ipc.txt", ipc_fixture()?),
        ("escaping-seats.txt", list_fixture(seats)),
    ];
    common::check(&fixtures)
}
