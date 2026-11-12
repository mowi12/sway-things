// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};

use proptest::prelude::*;

use super::line::{self, Variable};
use super::{
    Diagnostic, DiagnosticKind, Directive, Disk, ExpandError, MemoryFiles, SwayConfig,
    default_path, parse,
};
use crate::command::model;

const MAIN: &str = "/home/me/.config/sway/config";

fn files(main: &str) -> MemoryFiles {
    MemoryFiles::new().var("HOME", "/home/me").file(MAIN, main)
}

fn parsed(main: &str) -> SwayConfig {
    parse(Path::new(MAIN), &files(main))
}

/// The directives as `line: block | name args`.
fn listing(config: &SwayConfig) -> Vec<String> {
    config
        .directives
        .iter()
        .map(|directive| {
            let block = if directive.block.is_empty() {
                String::new()
            } else {
                format!("{} | ", directive.block.join(" "))
            };
            let args: Vec<String> = directive
                .args
                .iter()
                .map(|arg| format!("<{arg}>"))
                .collect();
            let file = directive.file.file_name().unwrap().to_str().unwrap();
            format!(
                "{file}:{}: {block}{} {}",
                directive.line,
                directive.name,
                args.join(" ")
            )
            .trim_end()
            .to_owned()
        })
        .collect()
}

fn kinds(config: &SwayConfig) -> Vec<(usize, DiagnosticKind)> {
    config
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.line, diagnostic.kind.clone()))
        .collect()
}

#[test]
fn directives_have_their_place_and_resolved_arguments() {
    let config = parsed(
        r##"# A comment
   # and an indented one

set $mod Mod4
gaps inner 8
client.focused #4c7899 "#285577" '#ffffff' #2e9ef4   #285577
font "pango:Noto Sans \"Display\" 11"
	focus_follows_mouse no
bindsym $mod+Return exec "foot --title 'my term'"
exec_always swaybg -c "#000000"
"##,
    );
    assert_eq!(
        listing(&config),
        [
            "config:4: set <$mod> <Mod4>",
            "config:5: gaps <inner> <8>",
            "config:6: client.focused <#4c7899> <#285577> <#ffffff> <#2e9ef4> <#285577>",
            "config:7: font <pango:Noto Sans \"Display\" 11>",
            "config:8: focus_follows_mouse <no>",
            // Bindings and exec lines get their arguments as written.
            "config:9: bindsym <Mod4+Return> <exec> <\"foot --title 'my term'\">",
            "config:10: exec_always <swaybg> <-c> <\"#000000\">",
        ]
    );
    assert_eq!(config.diagnostics, []);
    assert_eq!(config.files.len(), 1);
    assert_eq!(&*config.files[0], Path::new(MAIN));
    let font = &config.directives[3];
    assert_eq!(font.text, r#"font "pango:Noto Sans \"Display\" 11""#);
    assert!(font.is(&[], "FONT"));
    assert!(!font.is(&["bar"], "font"));
}

#[test]
fn variables_are_replaced_when_a_line_is_read() {
    let config = parsed(
        r"gaps inner $gap
set $gap 4
gaps inner $gap
set $gap 8
set $gap_outer 2
gaps outer $gap_outer
gaps inner $gap
set $both $gap $gap_outer
gaps $both $$gap \$gap $unset
SET $loud 1
$cmd inner 3
set $cmd gaps
$cmd inner $loud
set novar x
set $empty
",
    );
    assert_eq!(
        listing(&config),
        [
            // Not set yet: stays as written.
            "config:1: gaps <inner> <$gap>",
            "config:2: set <$gap> <4>",
            "config:3: gaps <inner> <4>",
            // A later `set` doesn't change the lines before it.
            "config:4: set <$gap> <8>",
            "config:5: set <$gap_outer> <2>",
            // The longer name wins over its prefix `$gap`.
            "config:6: gaps <outer> <2>",
            "config:7: gaps <inner> <8>",
            "config:8: set <$both> <8> <2>",
            "config:9: gaps <8> <2> <$gap> <\\$gap> <$unset>",
            "config:10: SET <$loud> <1>",
            "config:11: $cmd <inner> <3>",
            "config:12: set <$cmd> <gaps>",
            // The command itself can come from a variable.
            "config:13: gaps <inner> <1>",
            "config:14: set <novar> <x>",
            "config:15: set <$empty>",
        ]
    );
}

#[test]
fn blocks_give_their_directives_a_context() {
    let config = parsed(
        r#"input type:touchpad {
    tap enabled
    # a comment
    natural_scroll enabled
}
input "1267:12377:ELAN1300:00 04F3:3059 Touchpad" tap disabled
input type:keyboard xkb_layout "us,de"
input *
output HDMI-A-1
{
    mode 1920x1080@60Hz
}

seat seat0 hide_cursor 5000
bar {
    position top
    colors {
        statusline #ffffff
    }
    status_command while date; do sleep 1; done
}
mode "resize" {
    bindsym Left resize shrink width 10px
}
set $id type:pointer
input $id {
    accel_profile flat
}
gaps inner 4
"#,
    );
    assert_eq!(
        listing(&config),
        [
            "config:2: input type:touchpad | tap <enabled>",
            "config:4: input type:touchpad | natural_scroll <enabled>",
            // The one-line form reads the same as the block.
            "config:6: input 1267:12377:ELAN1300:00 04F3:3059 Touchpad | tap <disabled>",
            "config:7: input type:keyboard | xkb_layout <us,de>",
            "config:8: input <*>",
            // A brace on the next line belongs to the line before it.
            "config:11: output HDMI-A-1 | mode <1920x1080@60Hz>",
            "config:14: seat seat0 | hide_cursor <5000>",
            "config:16: bar | position <top>",
            "config:18: bar colors | statusline <#ffffff>",
            "config:20: bar | status_command <while> <date;> <do> <sleep> <1;> <done>",
            // Inside `mode`, sway keeps the quotes of the whole line.
            "config:23: mode \"resize\" | bindsym <Left> <resize> <shrink> <width> <10px>",
            "config:25: set <$id> <type:pointer>",
            // The name of a block is resolved for every line in it.
            "config:27: input type:pointer | accel_profile <flat>",
            "config:29: gaps <inner> <4>",
        ]
    );
    assert_eq!(config.diagnostics, []);
    assert!(config.directives[0].is(&["INPUT", "type:touchpad"], "Tap"));
}

#[test]
fn continued_lines_are_one_directive() {
    let config = parsed(
        "bindsym Mod4+Shift+e exec swaynag -t warning \\\n    -m 'Exit?' \\\n    -B 'Yes' 'swaymsg exit'\n\
         # a comment doesn't continue \\\n\
         gaps inner 8\n\
         font monospace\\",
    );
    assert_eq!(
        listing(&config),
        [
            "config:1: bindsym <Mod4+Shift+e> <exec> <swaynag> <-t> <warning> <-m> <'Exit?'> \
             <-B> <'Yes'> <'swaymsg exit'>",
            "config:5: gaps <inner> <8>",
            // A backslash at the very end of the file is just that.
            "config:6: font <monospace\\>",
        ]
    );
    assert_eq!(
        config.directives[0].text,
        "bindsym Mod4+Shift+e exec swaynag -t warning     -m 'Exit?'     -B 'Yes' 'swaymsg exit'"
    );
}

#[test]
fn includes_are_read_in_place_and_in_order() {
    let files = files(
        "gaps inner 1\n\
         set $dir config.d\n\
         include $dir/*.conf\n\
         gaps inner 2\n\
         include ~/.config/sway/sway-things/*.conf\n\
         include /etc/sway/config.d/*\n\
         gaps inner 3\n",
    )
    .file(
        "/home/me/.config/sway/config.d/20-b.conf",
        "gaps outer $outer\n",
    )
    .file(
        "/home/me/.config/sway/config.d/10-a.conf",
        "set $outer 5\ninclude nested/deep.conf\n",
    )
    .file(
        "/home/me/.config/sway/config.d/nested/deep.conf",
        "smart_gaps on\n",
    )
    .file(
        "/home/me/.config/sway/sway-things/10-input.conf",
        "input type:touchpad tap enabled\n",
    );
    let config = parse(Path::new(MAIN), &files);
    assert_eq!(
        listing(&config),
        [
            "config:1: gaps <inner> <1>",
            "config:2: set <$dir> <config.d>",
            "10-a.conf:1: set <$outer> <5>",
            // Relative to the file the include is in.
            "deep.conf:1: smart_gaps <on>",
            // Variables carry over to the files read later.
            "20-b.conf:1: gaps <outer> <5>",
            "config:4: gaps <inner> <2>",
            "10-input.conf:1: input type:touchpad | tap <enabled>",
            "config:7: gaps <inner> <3>",
        ]
    );
    let names: Vec<_> = config
        .files
        .iter()
        .map(|file| file.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "config",
            "10-a.conf",
            "deep.conf",
            "20-b.conf",
            "10-input.conf"
        ]
    );

    // Every include with what it stood for, in the order sway met them.
    let includes: Vec<_> = config
        .includes
        .iter()
        .map(|include| (include.line, include.pattern.as_str(), include.loaded.len()))
        .collect();
    assert_eq!(
        includes,
        [
            (3, "config.d/*.conf", 2),
            (2, "nested/deep.conf", 1),
            (5, "~/.config/sway/sway-things/*.conf", 1),
            (6, "/etc/sway/config.d/*", 0),
        ]
    );
    assert_eq!(
        config.includes[2].paths,
        [PathBuf::from(
            "/home/me/.config/sway/sway-things/10-input.conf"
        )]
    );
    // The glob in /etc matches nothing, which sway skips silently.
    assert_eq!(
        kinds(&config),
        [(
            6,
            DiagnosticKind::IncludeNotFound {
                path: PathBuf::from("/etc/sway/config.d/*")
            }
        )]
    );
}

#[test]
fn includes_that_cannot_be_followed_are_reported() {
    let files = files(
        "include $(hostname).conf\n\
         include \"`hostname`.conf\"\n\
         include ~root/sway.conf\n\
         include a | b\n\
         include a.conf b.conf\n\
         include\n\
         include missing.conf\n\
         include loop.conf\n\
         include loop.conf\n\
         include config\n\
         gaps inner 8\n",
    )
    .file(
        "/home/me/.config/sway/loop.conf",
        "include ../sway/loop.conf\ninclude config\ngaps outer 1\n",
    );
    let config = parse(Path::new(MAIN), &files);
    let unresolved = |pattern: &str, reason| DiagnosticKind::UnresolvedInclude {
        pattern: pattern.to_owned(),
        reason,
    };
    let already = |path: &str| DiagnosticKind::AlreadyIncluded {
        path: PathBuf::from(path),
    };
    assert_eq!(
        kinds(&config),
        [
            (
                1,
                unresolved("$(hostname).conf", ExpandError::CommandSubstitution)
            ),
            (
                2,
                unresolved("`hostname`.conf", ExpandError::CommandSubstitution)
            ),
            (3, unresolved("~root/sway.conf", ExpandError::Unsupported)),
            // `include` takes exactly one argument.
            (4, DiagnosticKind::InvalidInclude),
            (5, DiagnosticKind::InvalidInclude),
            (6, DiagnosticKind::InvalidInclude),
            (
                7,
                DiagnosticKind::IncludeNotFound {
                    path: PathBuf::from("/home/me/.config/sway/missing.conf")
                }
            ),
            // A file is read once: the cycle ends, and so does the second include.
            (1, already("/home/me/.config/sway/loop.conf")),
            (2, already(MAIN)),
            (9, already("/home/me/.config/sway/loop.conf")),
            (10, already(MAIN)),
        ]
    );
    // The rest of the config is still read.
    assert_eq!(
        listing(&config),
        [
            "loop.conf:3: gaps <outer> <1>",
            "config:11: gaps <inner> <8>"
        ]
    );
    assert_eq!(config.includes.len(), 12);
}

#[test]
fn a_missing_or_unreadable_config_is_a_diagnostic() {
    let config = parse(Path::new("/nowhere/config"), &MemoryFiles::new());
    assert_eq!(config.directives, []);
    assert!(matches!(
        &config.diagnostics[..],
        [Diagnostic {
            line: 0,
            kind: DiagnosticKind::Unreadable { .. },
            ..
        }]
    ));
    // On the real file system: a directory where the config should be.
    let dir = tempfile::tempdir().unwrap();
    let config = parse(dir.path(), &Disk);
    assert!(matches!(
        &config.diagnostics[..],
        [Diagnostic {
            kind: DiagnosticKind::Unreadable { .. },
            ..
        }]
    ));
}

#[test]
fn unbalanced_braces_are_reported() {
    let config = parsed("}\ngaps inner 1\ninput * {\n  tap enabled\nbar {\n position top\n");
    assert_eq!(
        kinds(&config),
        [
            (1, DiagnosticKind::UnmatchedBrace),
            (3, DiagnosticKind::UnclosedBlock)
        ]
    );
    assert_eq!(
        listing(&config),
        [
            "config:2: gaps <inner> <1>",
            "config:4: input * | tap <enabled>",
            "config:6: input * bar | position <top>",
        ]
    );
}

#[test]
fn deep_and_huge_configs_end_with_a_diagnostic() {
    // A chain of includes longer than the parser follows.
    let mut files = files("include 0.conf\n");
    for index in 0..40 {
        files = files.file(
            &format!("/home/me/.config/sway/{index}.conf"),
            format!("gaps inner {index}\ninclude {}.conf\n", index + 1),
        );
    }
    let config = parse(Path::new(MAIN), &files);
    assert_eq!(config.directives.len(), 32);
    assert!(kinds(&config).contains(&(2, DiagnosticKind::TooDeep)));

    let huge = "gaps inner 1\n".repeat(100_050);
    let config = parsed(&huge);
    assert_eq!(config.directives.len(), 100_000);
    assert_eq!(kinds(&config), [(100_001, DiagnosticKind::TooBig)]);

    let files = files_with_big_include();
    let config = parse(Path::new(MAIN), &files);
    assert_eq!(kinds(&config), [(0, DiagnosticKind::TooBig)]);
    assert_eq!(listing(&config), ["config:2: gaps <inner> <1>"]);
}

fn files_with_big_include() -> MemoryFiles {
    files("include big.conf\ngaps inner 1\n").file(
        "/home/me/.config/sway/big.conf",
        vec![b'#'; 9 * 1024 * 1024],
    )
}

#[test]
fn long_lines_take_linear_time() {
    // Each of these would take minutes if a step were quadratic.
    let dollars = format!("gaps inner {}\n", "$".repeat(2_000_000));
    assert_eq!(parsed(&dollars).directives[0].args[1].len(), 1_000_000);
    let variables = format!("set $v {}\ngaps {}\n", "x".repeat(1000), "$v".repeat(2000));
    assert_eq!(parsed(&variables).directives[1].args[0].len(), 2_000_000);
    let quotes = format!("gaps {}\n", "\"a\" ".repeat(300_000));
    assert_eq!(parsed(&quotes).directives[0].args.len(), 300_000);
    let blanks = format!("gaps inner 1\n{}", "\n".repeat(2_000_000));
    assert_eq!(parsed(&blanks).directives.len(), 1);
    let continued = format!("gaps {}inner 1\n", "\\\n".repeat(1_000_000));
    assert_eq!(parsed(&continued).directives[0].args, ["inner", "1"]);
}

#[test]
fn text_that_is_not_utf8_is_read_anyway() {
    let files = MemoryFiles::new().file(MAIN, b"font \xff\xfe 10\r\ngaps inner 8\r\n".to_vec());
    let config = parse(Path::new(MAIN), &files);
    assert_eq!(
        listing(&config),
        [
            "config:1: font <\u{fffd}\u{fffd}> <10>",
            "config:2: gaps <inner> <8>"
        ]
    );
}

#[test]
fn the_default_config_is_found_in_sways_order() {
    let found = |files: &MemoryFiles| default_path(files).map(|path| path.display().to_string());
    let everywhere = MemoryFiles::new()
        .var("HOME", "/home/me")
        .file("/etc/sway/config", "")
        .file("/etc/i3/config", "")
        .file("/home/me/.config/i3/config", "")
        .file("/home/me/.i3/config", "");
    assert_eq!(found(&everywhere).as_deref(), Some("/home/me/.i3/config"));
    let with_xdg = everywhere.clone().file("/home/me/.config/sway/config", "");
    assert_eq!(
        found(&with_xdg).as_deref(),
        Some("/home/me/.config/sway/config")
    );
    let with_dot_sway = with_xdg.clone().file("/home/me/.sway/config", "");
    assert_eq!(
        found(&with_dot_sway).as_deref(),
        Some("/home/me/.sway/config")
    );
    // XDG_CONFIG_HOME moves the second and fourth place.
    let moved = with_xdg
        .var("XDG_CONFIG_HOME", "/xdg")
        .file("/xdg/sway/config", "");
    assert_eq!(found(&moved).as_deref(), Some("/xdg/sway/config"));
    let system_only = MemoryFiles::new().file("/etc/sway/config", "");
    assert_eq!(found(&system_only).as_deref(), Some("/etc/sway/config"));
    assert_eq!(found(&MemoryFiles::new()), None);
}

#[test]
fn the_sample_and_generated_configs_parse() {
    let integration = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ci/integration");
    for (main, expected_files) in [("sway.conf", 14), ("swayfx.conf", 17)] {
        let config = parse(&integration.join(main), &Disk);
        // parser.conf includes one file twice and one that doesn't exist, on purpose. The last
        // two includes of sway.conf (lines 42 and 45) only match while the integration tests run.
        assert_eq!(
            kinds(&config)
                .iter()
                .map(|(line, _)| *line)
                .collect::<Vec<_>>(),
            [43, 45, 42, 45],
            "{main}"
        );
        assert_eq!(config.files.len(), expected_files, "{main}");
        assert!(config.directives.len() > 100, "{main}");
    }

    // What the generators write comes back as the settings it was made from.
    let config = parse(&integration.join("sway.conf"), &Disk);
    let generated: Vec<&Directive> = config
        .directives
        .iter()
        .filter(|directive| {
            directive
                .file
                .to_string_lossy()
                .contains("/generated/sway/")
        })
        .collect();
    let find = |block: &[&str], name: &str| {
        generated
            .iter()
            .find(|directive| directive.is(block, name))
            .map(|directive| directive.args.join(" "))
    };
    assert_eq!(generated.len(), 10);
    assert_eq!(
        find(&["input", "type:touchpad"], "tap").as_deref(),
        Some("enabled")
    );
    assert_eq!(
        find(
            &["input", "1267:12377:ELAN1300:00_04F3:3059_Touchpad"],
            "tap"
        )
        .as_deref(),
        Some("disabled")
    );
    assert_eq!(
        find(&["input", "type:keyboard"], "xkb_layout").as_deref(),
        Some("us,de")
    );
    assert_eq!(find(&[], "gaps").as_deref(), Some("inner 6"));
    assert_eq!(find(&[], "font").as_deref(), Some("Noto Sans 11"));
    assert_eq!(
        find(&[], "client.focused").as_deref(),
        Some("#4c7899 #285577 #eeeeee #2e9ef4 #3b6e4f")
    );
}

proptest! {
    /// The arguments of a line are what the independent port of sway's parser gives, which the
    /// command builders are tested against.
    #[test]
    fn lines_split_like_the_port_of_sway(text in "[ -~]{0,40}") {
        if let Ok(expected) = model::config_command(text.as_bytes()) {
            let stripped = line::strip_whitespace(text.as_bytes());
            prop_assert_eq!(line::arguments(stripped, &[]), expected);
        }
    }

    #[test]
    fn hostile_lines_split_like_the_port_of_sway(text in "[a-c \"'\\\\\\[\\]$#{}x0-2]{0,24}") {
        if let Ok(expected) = model::config_command(text.as_bytes()) {
            let stripped = line::strip_whitespace(text.as_bytes());
            prop_assert_eq!(line::arguments(stripped, &[]), expected);
        }
    }

    #[test]
    fn variables_never_break_a_line(
        text in "[a-c $\\\\\"']{0,24}",
        name in "\\$[a-c]{0,3}",
        value in "[a-c $\\\\\"']{0,6}",
    ) {
        let variables = [Variable {
            name: name.into_bytes(),
            value: value.into_bytes(),
        }];
        let _ = line::arguments(text.as_bytes(), &variables);
    }

    /// Any bytes at all: the parser ends, without a panic.
    #[test]
    fn any_bytes_parse(main in proptest::collection::vec(any::<u8>(), 0..400)) {
        let files = MemoryFiles::new().var("HOME", "/home/me").file(MAIN, main);
        let _ = parse(Path::new(MAIN), &files);
    }

    #[test]
    fn config_like_text_parses(
        main in "([a-z$ {}\\\\\"'#*~/.\\[\\]]{0,20}\n){0,12}",
        included in "([a-z$ {}\\\\\"'#*~/.]{0,20}\n){0,6}",
    ) {
        let files = MemoryFiles::new()
            .var("HOME", "/home/me")
            .file(MAIN, format!("include a.conf\ninclude *.conf\n{main}"))
            .file("/home/me/.config/sway/a.conf", included.clone())
            .file("/home/me/.config/sway/b.conf", format!("include config\n{included}"));
        let config = parse(Path::new(MAIN), &files);
        // Every directive is on a line of a file that was read.
        for directive in &config.directives {
            prop_assert!(config.files.contains(&directive.file));
            prop_assert!(directive.line >= 1);
        }
    }
}
