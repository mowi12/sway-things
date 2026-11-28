// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Taking over the user's bar without touching their config.
//!
//! sway's `bar <id> <subcommand>` selects the bar config with that ID if there is one, and only
//! makes a new one otherwise (`sway/commands/bar.c`); the user's unnamed `bar { … }` blocks get
//! the IDs `bar-0`, `bar-1`, … in the order they are read. Our include comes after the user's
//! blocks, so `bar bar-0 swaybar_command sway-things` in `75-bar.conf` changes the program of
//! their bar: at the next reload sway runs `sway-things -b bar-0` as that bar, which runs waybar
//! with our files ([`launch_arguments`]), and their bar's other settings stay. The other bars
//! of theirs get `swaybar_command true`, so they show nothing. Removing our lines and reloading
//! brings their bars back exactly as they were. Checked with sway 1.10, 1.12 and SwayFX.
//!
//! [`bars`] reads the bar configs of a sway config as sway makes them, with the program each
//! runs in the end; only bars made before our include can be taken over ([`Bar::can_take`]): a
//! line of ours before their block would make the bar, and their block would then be a second
//! one.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::WaybarFiles;
use crate::sway_config::{Directive, SwayConfig};

pub use crate::command::bar::LAUNCHER;

/// The subcommands of `bar` (`sway/commands/bar.c`): a first word that isn't one is the ID.
const SUBCOMMANDS: &[&str] = &[
    "bindcode",
    "binding_mode_indicator",
    "bindsym",
    "colors",
    "font",
    "gaps",
    "height",
    "hidden_state",
    "icon_theme",
    "id",
    "mode",
    "modifier",
    "output",
    "pango_markup",
    "position",
    "separator_symbol",
    "status_command",
    "status_edge_padding",
    "status_padding",
    "strip_workspace_name",
    "strip_workspace_numbers",
    "swaybar_command",
    "tray_bindcode",
    "tray_bindsym",
    "tray_output",
    "tray_padding",
    "workspace_buttons",
    "workspace_min_width",
    "wrap_scroll",
];

/// Whether `word` is a subcommand of `bar`.
fn is_subcommand(word: &str) -> bool {
    SUBCOMMANDS
        .iter()
        .any(|subcommand| subcommand.eq_ignore_ascii_case(word))
}

/// A bar config of a sway config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    /// Its ID.
    pub id: String,
    /// The file and line that made it.
    pub file: PathBuf,
    /// The number of that line, counted from 1.
    pub line: usize,
    /// Whether a line of the user's made it (not one of our generated files).
    pub theirs: bool,
    /// Whether it was made before the first line of our generated files.
    pub before_ours: bool,
    /// The program it runs in the end (`swaybar_command`, `swaybar` without one).
    pub program: String,
    /// Whether that program is set by a line of ours.
    pub program_ours: bool,
}

impl Bar {
    /// Whether Sway Things can take this bar over: the user made it before our include.
    pub fn can_take(&self) -> bool {
        self.theirs && self.before_ours
    }

    /// Whether a line of ours took it over: it runs our launcher or nothing (`true`).
    pub fn taken(&self) -> bool {
        self.theirs && self.program_ours && matches!(self.program.as_str(), LAUNCHER | "true")
    }

    /// Whether it runs waybar (`swaybar_command waybar`, one word: sway runs it without
    /// options).
    pub fn runs_waybar(&self) -> bool {
        let program = self.program.trim();
        !program.contains(char::is_whitespace)
            && Path::new(program)
                .file_name()
                .is_some_and(|name| name == "waybar")
    }
}

/// Whether `path` is in the folder of our generated files, `generated`, as given or with symbolic
/// links resolved: the parser names an included file by its real path, and `~/.config/sway` is
/// often a link into a dotfiles repository.
pub(super) fn in_generated(path: &Path, generated: &Path) -> bool {
    path.starts_with(generated)
        || std::fs::canonicalize(generated).is_ok_and(|real| path.starts_with(real))
}

/// The directives of `config` in the order sway runs them, with the lines of our `75-bar.conf`
/// where our include puts them if `config` comes without our files (`IncludeStatus::config`),
/// and the file and line of our include.
fn in_order<'a>(
    config: &'a SwayConfig,
    generated: &Path,
) -> (Vec<Directive>, Option<(&'a Path, usize)>) {
    let include = config
        .includes
        .iter()
        .find(|include| {
            in_generated(Path::new(&include.pattern), generated)
                || include
                    .paths
                    .iter()
                    .any(|path| in_generated(path, generated))
                || include
                    .loaded
                    .iter()
                    .any(|path| in_generated(path, generated))
        })
        .map(|include| (&*include.file, include.line));
    let ours_missing = !config
        .directives
        .iter()
        .any(|directive| in_generated(&directive.file, generated));
    let ours_read = match include {
        Some(_) if ours_missing => {
            crate::sway_config::parse(&generated.join("75-bar.conf"), &crate::sway_config::Disk)
                .directives
        }
        _ => Vec::new(),
    };
    let mut ordered = Vec::with_capacity(config.directives.len() + ours_read.len());
    let mut pending = Some(ours_read);
    for directive in &config.directives {
        if include.is_some_and(|(file, line)| *file == *directive.file && directive.line > line) {
            ordered.extend(pending.take().unwrap_or_default());
        }
        ordered.push(directive.clone());
    }
    ordered.extend(pending.take().unwrap_or_default());
    (ordered, include)
}

/// The bar configs of `config` in the order sway makes them, with the program each runs in the
/// end; `generated` is the folder of our generated files. Which lines a `bar { … }` block holds
/// is read from its file.
pub fn bars(config: &SwayConfig, generated: &Path) -> Vec<Bar> {
    let mut bars: Vec<Bar> = Vec::new();
    let mut openers: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    // The block being read: its file, its first line and its bar.
    let mut current: Option<(PathBuf, usize, usize)> = None;
    let (ordered, include) = in_order(config, generated);
    let at_include = |directive: &Directive| {
        include.is_some_and(|(file, line)| *file == *directive.file && directive.line > line)
    };
    let mut seen_ours = false;
    for directive in &ordered {
        let ours = in_generated(&directive.file, generated);
        seen_ours |= ours || at_include(directive);
        let make = |bars: &mut Vec<Bar>, id: Option<&str>| -> usize {
            if let Some(at) = id.and_then(|id| bars.iter().position(|bar| bar.id == id)) {
                return at;
            }
            bars.push(Bar {
                id: id.map_or_else(|| format!("bar-{}", bars.len()), ToOwned::to_owned),
                file: directive.file.to_path_buf(),
                line: directive.line,
                theirs: !ours,
                before_ours: !seen_ours,
                program: "swaybar".to_owned(),
                program_ours: false,
            });
            bars.len() - 1
        };
        let in_bar = directive
            .block
            .first()
            .is_some_and(|word| word.eq_ignore_ascii_case("bar"));
        let (at, name, args): (usize, &str, &[String]) = if in_bar {
            let file = directive.file.to_path_buf();
            let lines = openers.entry(file.clone()).or_insert_with(|| {
                std::fs::read_to_string(&file)
                    .map(|text| super::starter::block_openers(&text))
                    .unwrap_or_default()
            });
            let opener = lines
                .iter()
                .rev()
                .find(|line| **line <= directive.line)
                .copied()
                .unwrap_or(directive.line);
            let at = match &current {
                Some((open_file, open_line, at)) if *open_file == file && *open_line == opener => {
                    *at
                }
                _ => {
                    // `bar <id> {`: the word after `bar` unless it is a nested block
                    // (`colors`).
                    let id = directive
                        .block
                        .get(1)
                        .filter(|word| !is_subcommand(word))
                        .map(String::as_str);
                    let at = make(&mut bars, id);
                    if bars[at].line == directive.line {
                        bars[at].line = opener;
                    }
                    current = Some((file, opener, at));
                    at
                }
            };
            (at, directive.name.as_str(), &directive.args[..])
        } else if directive.block.is_empty() && directive.name.eq_ignore_ascii_case("bar") {
            current = None;
            // `bar [<id>] <subcommand> …` on one line.
            let named = directive.args.len() >= 2
                && !is_subcommand(&directive.args[0])
                && is_subcommand(&directive.args[1]);
            let (id, rest) = if named {
                (Some(directive.args[0].as_str()), &directive.args[1..])
            } else {
                (None, &directive.args[..])
            };
            let at = make(&mut bars, id);
            match rest.split_first() {
                Some((name, args)) => (at, name.as_str(), args),
                None => continue,
            }
        } else {
            current = None;
            continue;
        };
        if name.eq_ignore_ascii_case("swaybar_command") {
            bars[at].program = args.join(" ");
            bars[at].program_ours = ours;
        } else if name.eq_ignore_ascii_case("id") {
            if let Some(id) = args.first() {
                bars[at].id.clone_from(id);
            }
        }
    }
    bars
}

/// The IDs of the bars of `config` that Sway Things can take over, in the order sway makes them.
pub fn takeable(config: &SwayConfig, generated: &Path) -> Vec<String> {
    bars(config, generated)
        .into_iter()
        .filter(Bar::can_take)
        .map(|bar| bar.id)
        .collect()
}

/// The file the launcher writes waybar's output to, in `$XDG_RUNTIME_DIR`: what went wrong when
/// Sway Things' waybar didn't start or ended, for the page and `doctor`. `None` without an
/// absolute runtime folder.
pub fn log_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("sway-things-waybar.log"))
}

/// The last lines of the launcher's log that say something went wrong (`[error]`, `[critical]`,
/// or the launcher's own words), at most `count`, if the log was written after `since`.
pub fn log_errors(since: std::time::SystemTime, count: usize) -> Vec<String> {
    let Some(path) = log_path() else {
        return Vec::new();
    };
    let fresh = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .is_ok_and(|modified| modified >= since);
    if !fresh {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let errors: Vec<String> = text
        .lines()
        .filter(|line| {
            line.contains("[error]") || line.contains("[critical]") || line.starts_with(LOG_FAILED)
        })
        .map(|line| line.chars().take(300).collect())
        .collect();
    errors[errors.len().saturating_sub(count)..].to_vec()
}

/// When the launcher last started (it writes its log anew each time), if after `since`.
pub fn log_written(since: std::time::SystemTime) -> Option<std::time::SystemTime> {
    let modified = std::fs::metadata(log_path()?).ok()?.modified().ok()?;
    (modified >= since).then_some(modified)
}

/// How the launcher's own line starts when it can't run waybar.
pub const LOG_FAILED: &str = "sway-things: ";

/// The arguments the launcher runs waybar with: our config and stylesheet. The bar ID sway gives
/// isn't passed on: our config names the bar config it follows, if any.
pub fn launch_arguments(files: &WaybarFiles) -> Vec<OsString> {
    vec![
        OsString::from("-c"),
        files.config().into_os_string(),
        OsString::from("-s"),
        files.style().into_os_string(),
    ]
}

/// The arguments of swaybar when the launcher can't run waybar: the bar `id` sway gave, so
/// swaybar draws the user's own bar as it would without the takeover.
pub fn fallback_arguments(id: &str) -> Vec<OsString> {
    vec![OsString::from("-b"), OsString::from(id)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sway_config::{Disk, parse};

    /// The bars of a config with `text`, whose include of `generated` holds `ours`.
    fn bars_of(text: &str, ours: &str) -> Vec<Bar> {
        let dir = tempfile::tempdir().unwrap();
        let generated = dir.path().join("sway-things");
        std::fs::create_dir(&generated).unwrap();
        std::fs::write(generated.join("75-bar.conf"), ours).unwrap();
        let path = dir.path().join("config");
        std::fs::write(
            &path,
            text.replace(
                "INCLUDE",
                &format!("include {}/*.conf", generated.display()),
            ),
        )
        .unwrap();
        bars(&parse(&path, &Disk), &generated)
    }

    fn summary(bars: &[Bar]) -> Vec<(String, String, bool, bool)> {
        bars.iter()
            .map(|bar| {
                (
                    bar.id.clone(),
                    bar.program.clone(),
                    bar.can_take(),
                    bar.taken(),
                )
            })
            .collect()
    }

    fn row(id: &str, program: &str, can_take: bool, taken: bool) -> (String, String, bool, bool) {
        (id.to_owned(), program.to_owned(), can_take, taken)
    }

    #[test]
    fn unnamed_blocks_are_numbered_as_sway_numbers_them() {
        let bars = bars_of(
            "bar {\n  swaybar_command waybar\n  position top\n}\n\
             bar {\n  colors {\n    background #000000\n  }\n}\nINCLUDE\n",
            "",
        );
        assert_eq!(
            summary(&bars),
            [
                row("bar-0", "waybar", true, false),
                row("bar-1", "swaybar", true, false)
            ]
        );
        assert!(bars[0].runs_waybar());
        assert!(!bars[1].runs_waybar());
        assert_eq!(bars[0].line, 1);
        assert_eq!(bars[1].line, 5);
    }

    #[test]
    fn a_line_of_ours_takes_their_bar_over() {
        let bars = bars_of(
            "bar {\n  swaybar_command waybar\n}\nbar top {\n  position top\n}\n\
             bar bottom swaybar_command waybar\nINCLUDE\n",
            "bar bar-0 swaybar_command sway-things\nbar top swaybar_command true\n\
             bar bottom swaybar_command true\nbar sway-things-waybar swaybar_command true\n",
        );
        assert_eq!(
            summary(&bars),
            [
                row("bar-0", "sway-things", true, true),
                row("top", "true", true, true),
                row("bottom", "true", true, true),
                row("sway-things-waybar", "true", false, false),
            ]
        );
    }

    #[test]
    fn our_lines_are_read_where_the_include_puts_them() {
        let dir = tempfile::tempdir().unwrap();
        let generated = dir.path().join("sway-things");
        std::fs::create_dir(&generated).unwrap();
        std::fs::write(
            generated.join("75-bar.conf"),
            "bar \"bar-0\" swaybar_command sway-things\n",
        )
        .unwrap();
        let path = dir.path().join("config");
        std::fs::write(
            &path,
            format!(
                "bar {{\n  swaybar_command waybar\n}}\ninclude {}/*.conf\nbar {{\n}}\n",
                generated.display()
            ),
        )
        .unwrap();
        // The user's config without the lines of our files, as the include status has it.
        let mut config = parse(&path, &Disk);
        config
            .directives
            .retain(|directive| !directive.file.starts_with(&generated));
        let bars = bars(&config, &generated);
        assert_eq!(summary(&bars), [row("bar-0", "sway-things", true, true)]);
    }

    #[test]
    fn a_sway_folder_linked_into_dotfiles_is_ours_too() {
        // `~/.config/sway` is a link into a dotfiles repository, as with GNU Stow: the parser
        // names the files by their real paths, the include and our folder by the link.
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles/sway");
        let generated_real = real.join("sway-things");
        std::fs::create_dir_all(&generated_real).unwrap();
        std::fs::write(
            generated_real.join("75-bar.conf"),
            "bar \"bar-0\" swaybar_command sway-things\n",
        )
        .unwrap();
        let config_dir = dir.path().join("config");
        std::fs::create_dir(&config_dir).unwrap();
        std::os::unix::fs::symlink(&real, config_dir.join("sway")).unwrap();
        let generated = config_dir.join("sway/sway-things");
        std::fs::write(
            real.join("config"),
            format!(
                "bar {{\n  swaybar_command waybar\n  position top\n}}\ninclude {}/*.conf\n",
                generated.display()
            ),
        )
        .unwrap();
        let main = config_dir.join("sway/config");
        // As parsed, and as the include status gives it, without our files.
        let full = parse(&main, &Disk);
        assert_eq!(
            summary(&bars(&full, &generated)),
            [row("bar-0", "sway-things", true, true)]
        );
        let mut theirs = full.clone();
        theirs
            .directives
            .retain(|directive| !in_generated(&directive.file, &generated));
        assert_eq!(theirs.directives.len(), 2);
        assert_eq!(
            summary(&bars(&theirs, &generated)),
            [row("bar-0", "sway-things", true, true)]
        );
        let files = WaybarFiles {
            dir: config_dir.join("sway-things/waybar"),
        };
        let starter = crate::bar::process::Starter::detect(&theirs, &generated, &[], &files);
        assert!(
            matches!(starter, crate::bar::process::Starter::TakenOver(_)),
            "{starter:?}"
        );
    }

    #[test]
    fn an_id_line_names_the_block_and_a_bar_after_our_include_can_t_be_taken() {
        let bars = bars_of(
            "bar {\n  id main\n  swaybar_command waybar\n}\nINCLUDE\nbar {\n  position top\n}\n",
            "",
        );
        assert_eq!(
            summary(&bars),
            [
                row("main", "waybar", true, false),
                row("bar-1", "swaybar", false, false)
            ]
        );
        assert_eq!(
            takeable(
                &parse(Path::new("/nonexistent"), &Disk),
                Path::new("/nonexistent/sway-things")
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_launcher_runs_waybar_with_our_files() {
        let files = WaybarFiles {
            dir: PathBuf::from("/c/sway-things/waybar"),
        };
        assert_eq!(
            launch_arguments(&files),
            [
                "-c",
                "/c/sway-things/waybar/config.jsonc",
                "-s",
                "/c/sway-things/waybar/style.css"
            ]
            .map(OsString::from)
        );
    }

    #[test]
    fn without_waybar_the_launcher_runs_the_users_bar_with_swaybar() {
        assert_eq!(
            fallback_arguments("my bar"),
            ["-b", "my bar"].map(OsString::from)
        );
    }
}
