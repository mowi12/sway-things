// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds the sway config parser a main config and the files it includes, all in memory, so a run
//! never touches the real file system.
//!
//! The input is the main config, `/home/me/.config/sway/config`. A line `#=> PATH` starts another
//! file: `PATH` is relative to the directory of the main config or absolute. sway reads that line
//! as a comment, so an input is also a config sway can read.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use sway_things_core::sway_config::{MemoryFiles, SwayConfig, parse};

const DIRECTORY: &str = "/home/me/.config/sway";
const MAIN: &str = "/home/me/.config/sway/config";
const MARKER: &[u8] = b"#=> ";
/// More files than this are read as lines of the last one.
const MAX_FILES: usize = 32;

/// The parser's limits on directives and on the text they expand to.
const MAX_DIRECTIVES: usize = 100_000;
const MAX_TEXT: usize = 64 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    let config = parse(Path::new(MAIN), &files(data));
    check(&config);
});

/// The files and environment variables of the input.
fn files(data: &[u8]) -> MemoryFiles {
    let mut files = MemoryFiles::new()
        .var("HOME", "/home/me")
        .var("XDG_CONFIG_HOME", "/home/me/.config")
        .var("XDG_RUNTIME_DIR", "/run/user/1000")
        .var("DIR", "config.d")
        .var("TWO", "a.conf  b.conf")
        .var("STAR", "*.conf");
    let mut path = MAIN.to_owned();
    let mut content = Vec::new();
    let mut count = 1;
    for line in data.split_inclusive(|&byte| byte == b'\n') {
        if let Some(name) = line.strip_prefix(MARKER).filter(|_| count < MAX_FILES) {
            files = files.file(&path, std::mem::take(&mut content));
            count += 1;
            let name = String::from_utf8_lossy(name);
            let name = name.trim_end_matches('\n');
            path = if name.starts_with('/') {
                name.to_owned()
            } else {
                format!("{DIRECTORY}/{name}")
            };
        } else {
            content.extend_from_slice(line);
        }
    }
    files.file(&path, content)
}

/// What holds for every config, whatever the input.
fn check(config: &SwayConfig) {
    for (index, file) in config.files.iter().enumerate() {
        assert!(!config.files[..index].contains(file), "a file read twice");
    }
    for directive in &config.directives {
        assert!(config.files.contains(&directive.file));
        assert!(directive.line >= 1);
    }
    for include in &config.includes {
        assert!(
            include
                .loaded
                .iter()
                .all(|file| config.files.contains(file))
        );
    }
    for diagnostic in &config.diagnostics {
        assert!(
            config.files.contains(&diagnostic.file) || diagnostic.line == 0,
            "a diagnostic about a line of a file that wasn't read"
        );
    }
    assert!(config.directives.len() <= MAX_DIRECTIVES);
    let text: usize = config
        .directives
        .iter()
        .flat_map(|directive| directive.block.iter().chain(&directive.args))
        .map(String::len)
        .sum();
    assert!(text <= MAX_TEXT, "the directives hold {text} bytes");
}
