// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings of one `output` line, as sway's `cmd_output` reads them.
//!
//! sway takes several settings on one line (`output DP-4 mode 2560x1440 pos 0 0 scale 1`) and
//! reads them one after the other, each subcommand taking as many words as it needs. The config
//! parser gives such a line as one directive named after the first subcommand; `split` makes
//! one directive per subcommand, named as the registry knows it (`res` and `resolution` are
//! `mode`, `position` is `pos`, `enable`, `disable` and `toggle` are `enable` with the word as
//! argument). A line sway rejects (an unknown subcommand, a missing argument) sets nothing.

use crate::sway_config::Directive;

/// The modes of `bg`, as `bg_options` in sway's `commands/output/background.c`.
const BACKGROUND_MODES: [&str; 6] = ["stretch", "center", "fill", "fit", "tile", "solid_color"];

/// Subcommands that take exactly one word.
const ONE_WORD: [&str; 10] = [
    "scale",
    "scale_filter",
    "subpixel",
    "max_render_time",
    "adaptive_sync",
    "render_bit_depth",
    "power",
    "dpms",
    "hdr",
    "allow_tearing",
];

/// Whether `directive` is in an `output <name>` block.
pub(super) fn is_output(directive: &Directive) -> bool {
    directive.block.len() == 2
        && directive
            .block
            .first()
            .is_some_and(|word| word.eq_ignore_ascii_case("output"))
}

/// The directives of the subcommands of an `output` directive, in order. `None` if sway rejects
/// the line: it stores none of its settings then. Whether each value is valid is up to the
/// settings' read functions.
pub(super) fn split(directive: &Directive) -> Option<Vec<Directive>> {
    let words: Vec<&str> = std::iter::once(directive.name.as_str())
        .chain(directive.args.iter().map(String::as_str))
        .collect();
    let mut rest = words.as_slice();
    let mut found = Vec::new();
    while let Some((&subcommand, args)) = rest.split_first() {
        let lower = subcommand.to_ascii_lowercase();
        let (name, taken, words_as_args): (&str, usize, bool) = match lower.as_str() {
            "mode" | "res" | "resolution" => ("mode", mode_words(args)?, true),
            "pos" | "position" => ("pos", position_words(args)?, true),
            "transform" => {
                args.first()?;
                let relative = args
                    .get(1)
                    .is_some_and(|word| *word == "clockwise" || *word == "anticlockwise");
                ("transform", if relative { 2 } else { 1 }, true)
            }
            "enable" | "disable" | "toggle" => ("enable", 0, false),
            "modeline" => ("modeline", (args.len() == 11).then_some(11)?, true),
            "bg" | "background" => ("bg", background_words(args)?, true),
            "color_profile" => ("color_profile", color_profile_words(args)?, true),
            // It ends the line: sway takes its result as the line's.
            "unplug" => return Some(found),
            other if ONE_WORD.contains(&other) => {
                args.first()?;
                (ONE_WORD.iter().find(|known| **known == other)?, 1, true)
            }
            _ => return None,
        };
        let taken_args = args.get(..taken)?;
        let args = if words_as_args {
            taken_args.iter().map(|word| (*word).to_owned()).collect()
        } else {
            vec![subcommand.to_owned()]
        };
        found.push(Directive {
            name: name.to_owned(),
            args,
            ..directive.clone()
        });
        rest = rest.get(1 + taken..)?;
    }
    Some(found)
}

/// `mode [--custom] <width>x<height>[@<rate>Hz]` or `mode [--custom] <width> <height>`.
fn mode_words(args: &[&str]) -> Option<usize> {
    let custom = usize::from(args.first() == Some(&"--custom"));
    let size = args.get(custom)?;
    if starts_with_number_and_more(size) {
        Some(custom + 1)
    } else {
        args.get(custom + 1)?;
        Some(custom + 2)
    }
}

/// `pos <x>,<y>` or `pos <x> <y>`.
fn position_words(args: &[&str]) -> Option<usize> {
    let x = args.first()?;
    if starts_with_number_and_more(x) {
        Some(1)
    } else {
        args.get(1)?;
        Some(2)
    }
}

/// Whether something follows the number at the start of `word`, as sway checks with `strtol`'s
/// end pointer: `1920x1080` and `1920,0` hold both numbers, `1920` only one.
fn starts_with_number_and_more(word: &str) -> bool {
    let trimmed = word.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let unsigned = trimmed.strip_prefix(['-', '+']).unwrap_or(trimmed);
    let digits = unsigned.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        // No number: strtol ends at the start, and the word is checked as a whole.
        return !word.is_empty();
    }
    unsigned.len() > digits
}

/// `bg <color> solid_color`, or `bg <file …> <mode> [<#fallback>]`: the file can be several
/// words, up to the first one that is a mode.
fn background_words(args: &[&str]) -> Option<usize> {
    if args.len() < 2 {
        return None;
    }
    if args.get(1)?.eq_ignore_ascii_case("solid_color") {
        return Some(2);
    }
    let mode = args.iter().position(|word| {
        BACKGROUND_MODES
            .iter()
            .any(|mode| word.eq_ignore_ascii_case(mode))
    })?;
    if mode == 0 {
        return None;
    }
    let fallback = args.get(mode + 1).is_some_and(|word| word.starts_with('#'));
    Some(mode + 1 + usize::from(fallback))
}

/// `color_profile [--device-primaries] gamma22|srgb` or `color_profile icc <file>`.
fn color_profile_words(args: &[&str]) -> Option<usize> {
    let primaries = usize::from(args.len() >= 2 && args.first() == Some(&"--device-primaries"));
    match *args.get(primaries)? {
        "gamma22" | "srgb" => Some(primaries + 1),
        "icc" if primaries == 0 => {
            args.get(1)?;
            Some(2)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use super::split;
    use crate::sway_config::Directive;

    fn line(words: &[&str]) -> Directive {
        Directive {
            file: Arc::from(Path::new("/config")),
            line: 1,
            block: vec!["output".to_owned(), "DP-4".to_owned()],
            name: words[0].to_owned(),
            args: words[1..].iter().map(|word| (*word).to_owned()).collect(),
            text: words.join(" "),
        }
    }

    fn names(words: &[&str]) -> Option<Vec<(String, Vec<String>)>> {
        split(&line(words)).map(|found| {
            found
                .into_iter()
                .map(|directive| (directive.name, directive.args))
                .collect()
        })
    }

    fn owned(name: &str, args: &[&str]) -> (String, Vec<String>) {
        (
            name.to_owned(),
            args.iter().map(|word| (*word).to_owned()).collect(),
        )
    }

    #[test]
    fn a_line_with_several_settings_is_split() {
        assert_eq!(
            names(&[
                "mode",
                "2560x1440@59.951Hz",
                "pos",
                "0",
                "0",
                "scale",
                "1.5"
            ]),
            Some(vec![
                owned("mode", &["2560x1440@59.951Hz"]),
                owned("pos", &["0", "0"]),
                owned("scale", &["1.5"]),
            ])
        );
        assert_eq!(
            names(&["res", "1920", "1080", "position", "1920,0", "disable"]),
            Some(vec![
                owned("mode", &["1920", "1080"]),
                owned("pos", &["1920,0"]),
                owned("enable", &["disable"]),
            ])
        );
        assert_eq!(
            names(&[
                "Resolution",
                "--custom",
                "1920x1080",
                "transform",
                "90",
                "clockwise"
            ]),
            Some(vec![
                owned("mode", &["--custom", "1920x1080"]),
                owned("transform", &["90", "clockwise"]),
            ])
        );
    }

    #[test]
    fn backgrounds_and_profiles_take_their_words() {
        assert_eq!(
            names(&[
                "bg",
                "~/My",
                "Pictures/a.png",
                "fill",
                "#000000",
                "scale",
                "2"
            ]),
            Some(vec![
                owned("bg", &["~/My", "Pictures/a.png", "fill", "#000000"]),
                owned("scale", &["2"]),
            ])
        );
        assert_eq!(
            names(&["bg", "#123456", "solid_color", "enable"]),
            Some(vec![
                owned("bg", &["#123456", "solid_color"]),
                owned("enable", &["enable"]),
            ])
        );
        assert_eq!(
            names(&["color_profile", "--device-primaries", "srgb", "scale", "1"]).map(|v| v.len()),
            Some(2)
        );
        assert_eq!(
            names(&["color_profile", "icc", "/a.icc", "toggle"]).map(|v| v.len()),
            Some(2)
        );
    }

    #[test]
    fn a_line_sway_rejects_sets_nothing() {
        for words in [
            &["scale"][..],
            &["mode", "1920"],
            &["pos", "0"],
            &["frobnicate", "1"],
            &["scale", "1", "frobnicate"],
            &["bg", "a.png"],
            &["bg", "fill"],
            &["modeline", "1", "2"],
            &["color_profile", "icc"],
        ] {
            assert_eq!(names(words), None, "{words:?}");
        }
        assert_eq!(names(&["unplug", "scale", "2"]), Some(Vec::new()));
    }
}
