// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What starts waybar outside Sway Things' files, and what the user changes there so that it
//! runs ours.
//!
//! The user's sway config is read, never run: an `exec` or `exec_always` line is split into
//! words and commands as a POSIX shell would (quotes, backslashes, `;`, `&`, `|`, parentheses),
//! without expanding anything. A command starts waybar when its program is `waybar` (or nix's
//! `.waybar-wrapped`) after `exec`, `env`, `setsid`, `nohup`, options of those and variable
//! assignments; inside `sh -c '…'` (also `bash`, `dash`, `zsh`); after `uwsm app [--]`; and
//! `systemctl --user start|restart waybar[.service]` starts its unit. A line that names waybar
//! but runs something else (`~/.config/waybar/launch.sh`) may start it: Sway Things can't tell,
//! so it doesn't start a second one and shows the line to put in its place.

use std::path::{Path, PathBuf};

use super::WaybarFiles;
use super::process::Waybar;
use crate::sway_config::{Directive, SwayConfig};

/// How waybar is started outside Sway Things' files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Starter {
    /// Nothing starts it: our `75-bar.conf` does.
    Nothing,
    /// An `exec` or `exec_always` line of the user's sway config runs waybar.
    Exec(Line),
    /// An `exec` line starts waybar's systemd unit (`systemctl --user start waybar`).
    UnitStart(Line),
    /// An `exec` line runs something that names waybar, a script say: it may start waybar.
    Maybe(Line),
    /// `swaybar_command waybar` in a `bar` block of the user's sway config: sway starts it as
    /// the bar.
    SwaybarCommand(Line),
    /// A waybar of this session runs as a systemd unit.
    Unit,
}

/// A line of the user's sway config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Its file.
    pub file: PathBuf,
    /// Its number, counted from 1.
    pub line: usize,
    /// The line as written.
    pub text: String,
    /// Whether it already starts waybar with our config (`-c` or `--config` with its path).
    pub ours: bool,
}

/// What a shell command does with waybar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Start {
    /// Nothing that can be seen.
    No,
    /// It runs waybar.
    Program,
    /// It starts waybar's systemd unit.
    Unit,
    /// It names waybar but runs something else.
    Maybe,
}

impl Starter {
    /// How the user's sway config `config` (outside our generated files in `generated`) starts
    /// waybar, or whether a waybar runs as a unit (`running`). A line comes before a unit: the
    /// line is what the user can change.
    pub fn detect(
        config: &SwayConfig,
        generated: &Path,
        running: &[Waybar],
        files: &WaybarFiles,
    ) -> Self {
        let line = |directive: &Directive| Line {
            file: directive.file.to_path_buf(),
            line: directive.line,
            text: directive.text.clone(),
            ours: names_config(&directive.args.join(" "), &files.config()),
        };
        let mut maybe = None;
        for directive in &config.directives {
            if directive.file.starts_with(generated) {
                continue;
            }
            let exec = directive.is(&[], "exec") || directive.is(&[], "exec_always");
            if exec {
                match starts(&directive.args.join(" "), 0) {
                    Start::Program => return Self::Exec(line(directive)),
                    Start::Unit => return Self::UnitStart(line(directive)),
                    Start::Maybe => {
                        maybe.get_or_insert_with(|| line(directive));
                    }
                    Start::No => {}
                }
            }
            if let Some(command) = swaybar_command(directive) {
                if starts(&command, 0) == Start::Program {
                    return Self::SwaybarCommand(line(directive));
                }
            }
        }
        if let Some(line) = maybe {
            return Self::Maybe(line);
        }
        if running.iter().any(|waybar| waybar.unit) {
            return Self::Unit;
        }
        Self::Nothing
    }

    /// Whether something other than our files starts waybar, or may start it.
    pub fn is_elsewhere(&self) -> bool {
        !matches!(self, Self::Nothing)
    }
}

/// The `bar` blocks of the user's sway config that run swaybar (no `swaybar_command` with
/// another program): a bar next to ours. Each with the line it starts on. The parser gives each
/// line of a block with the block's words in front; which block a line belongs to is told by
/// the lines that open them, read from the file.
pub fn swaybars(config: &SwayConfig, generated: &Path) -> Vec<(PathBuf, usize)> {
    let mut openers: std::collections::BTreeMap<PathBuf, Vec<usize>> =
        std::collections::BTreeMap::new();
    // (file, line of the opener) → whether it runs another program than swaybar.
    let mut bars: Vec<((PathBuf, usize), bool)> = Vec::new();
    for directive in &config.directives {
        if directive.file.starts_with(generated) {
            continue;
        }
        let in_bar = directive
            .block
            .first()
            .is_some_and(|word| word.eq_ignore_ascii_case("bar"));
        if !in_bar {
            continue;
        }
        let file = directive.file.to_path_buf();
        let lines = openers.entry(file.clone()).or_insert_with(|| {
            std::fs::read_to_string(&file)
                .map(|text| block_openers(&text))
                .unwrap_or_default()
        });
        let opener = lines
            .iter()
            .rev()
            .find(|line| **line <= directive.line)
            .copied()
            .unwrap_or(directive.line);
        let key = (file, opener);
        let other = directive.name.eq_ignore_ascii_case("swaybar_command")
            && directive
                .args
                .first()
                .is_none_or(|program| base(program) != "swaybar");
        match bars.iter_mut().find(|(found, _)| *found == key) {
            Some((_, runs_other)) => *runs_other |= other,
            None => bars.push((key, other)),
        }
    }
    bars.into_iter()
        .filter(|(_, other)| !other)
        .map(|(start, _)| start)
        .collect()
}

/// The numbers of the lines that open a `bar` block (`bar {`, `bar <id> {`), counted from 1.
fn block_openers(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.trim();
            line.split_whitespace()
                .next()
                .is_some_and(|word| word.eq_ignore_ascii_case("bar"))
                && line.ends_with('{')
        })
        .map(|(index, _)| index + 1)
        .collect()
}

/// The command of a `swaybar_command` line: in a `bar` block, or written on one line as
/// `bar [<id>] swaybar_command …`.
fn swaybar_command(directive: &Directive) -> Option<String> {
    let in_bar = directive
        .block
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case("bar"));
    if in_bar && directive.name.eq_ignore_ascii_case("swaybar_command") {
        return Some(directive.args.join(" "));
    }
    if directive.is(&[], "bar") {
        let at = directive
            .args
            .iter()
            .position(|arg| arg.eq_ignore_ascii_case("swaybar_command"))?;
        return Some(directive.args.get(at + 1..)?.join(" "));
    }
    None
}

/// The file name of `word`, the program's name.
fn base(word: &str) -> &str {
    Path::new(word)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(word)
}

/// Whether `word` is waybar: `waybar`, `…/waybar` or nix's `.waybar-wrapped`.
pub(crate) fn is_waybar(word: &str) -> bool {
    matches!(base(word), "waybar" | ".waybar-wrapped")
}

/// The commands of a shell command line, each as its words: split at unquoted `;`, `&`, `|`,
/// `(`, `)` and line breaks, quotes and backslashes resolved, nothing expanded.
fn commands(text: &str) -> Vec<Vec<String>> {
    let mut commands = vec![Vec::new()];
    let mut word: Option<String> = None;
    let mut chars = text.chars();
    let end_word = |word: &mut Option<String>, commands: &mut Vec<Vec<String>>| {
        if let (Some(done), Some(command)) = (word.take(), commands.last_mut()) {
            command.push(done);
        }
    };
    while let Some(char) = chars.next() {
        match char {
            '\'' => {
                let current = word.get_or_insert_with(String::new);
                for quoted in chars.by_ref() {
                    if quoted == '\'' {
                        break;
                    }
                    current.push(quoted);
                }
            }
            '"' => {
                let current = word.get_or_insert_with(String::new);
                while let Some(quoted) = chars.next() {
                    match quoted {
                        '"' => break,
                        '\\' => {
                            if let Some(next) = chars.next() {
                                if !matches!(next, '"' | '\\' | '$' | '`') {
                                    current.push('\\');
                                }
                                current.push(next);
                            }
                        }
                        other => current.push(other),
                    }
                }
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    word.get_or_insert_with(String::new).push(next);
                }
            }
            ';' | '&' | '|' | '(' | ')' | '`' | '\n' => {
                end_word(&mut word, &mut commands);
                if commands.last().is_some_and(|command| !command.is_empty()) {
                    commands.push(Vec::new());
                }
            }
            blank if blank.is_whitespace() => end_word(&mut word, &mut commands),
            other => word.get_or_insert_with(String::new).push(other),
        }
    }
    end_word(&mut word, &mut commands);
    commands.retain(|command| !command.is_empty());
    commands
}

/// What the shell command `text` does with waybar; `depth` counts the `sh -c` it is in.
fn starts(text: &str, depth: usize) -> Start {
    let mut found = Start::No;
    for command in commands(text) {
        match command_starts(&command, depth) {
            Start::No => {}
            Start::Maybe => found = Start::Maybe,
            start => return start,
        }
    }
    found
}

/// What one command does with waybar.
fn command_starts(words: &[String], depth: usize) -> Start {
    // The program, after the words that only run the rest.
    let mut rest = words;
    loop {
        let Some((first, after)) = rest.split_first() else {
            return Start::No;
        };
        let skip = matches!(base(first), "exec" | "env" | "setsid" | "nohup" | "nice")
            || first.starts_with('-')
            || (first.contains('=') && !first.starts_with('='));
        if !skip {
            break;
        }
        rest = after;
    }
    let Some((program, arguments)) = rest.split_first() else {
        return Start::No;
    };
    if is_waybar(program) {
        return Start::Program;
    }
    match base(program) {
        "sh" | "bash" | "dash" | "zsh" | "ksh" if depth < 4 => {
            // `sh -c 'text' …`: the text is a command line of its own.
            if let Some(at) = arguments
                .iter()
                .position(|arg| arg == "-c" || arg.ends_with('c') && arg.starts_with('-'))
            {
                if let Some(script) = arguments.get(at + 1) {
                    return starts(script, depth + 1);
                }
            }
        }
        "uwsm" => {
            // `uwsm app [options] [--] waybar …`
            let mut words = arguments
                .iter()
                .skip_while(|word| word.as_str() != "app")
                .skip(1);
            let program = words.find(|word| !word.starts_with('-'));
            if program.is_some_and(|word| is_waybar(word)) {
                return Start::Program;
            }
        }
        "systemctl" => {
            let starting = arguments
                .iter()
                .any(|word| matches!(word.as_str(), "start" | "restart" | "reload-or-restart"));
            let unit = arguments.iter().any(|word| {
                let name = word.strip_suffix(".service").unwrap_or(word);
                name == "waybar" || name.starts_with("waybar@")
            });
            if starting && unit {
                return Start::Unit;
            }
        }
        _ => {}
    }
    // Something else that names waybar, as a script in waybar's directory does.
    if std::iter::once(program)
        .chain(arguments)
        .any(|word| word.to_ascii_lowercase().contains("waybar"))
        && !matches!(
            base(program),
            "pkill" | "killall" | "pgrep" | "pidof" | "kill" | "sway-things"
        )
    {
        return Start::Maybe;
    }
    Start::No
}

/// Whether the shell command `text` passes the file `path` to waybar as its config: the word
/// after `-c` or `--config`, or `--config=…`, is `path`, written as it is, with `~` or `$HOME`
/// for the home directory, or `$XDG_CONFIG_HOME` for the config directory.
pub(crate) fn names_config(text: &str, path: &Path) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_home = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|dir| dir.starts_with('/'))
        .unwrap_or_else(|| format!("{home}/.config"));
    let expand = |word: &str| {
        let word = match word.strip_prefix("~/") {
            Some(rest) => format!("{home}/{rest}"),
            None => word.to_owned(),
        };
        word.replace("${XDG_CONFIG_HOME}", &config_home)
            .replace("$XDG_CONFIG_HOME", &config_home)
            .replace("${HOME}", &home)
            .replace("$HOME", &home)
    };
    let is_ours = |word: &str| {
        let word = expand(word);
        Path::new(&word) == path
            || matches!(
                (std::fs::canonicalize(&word), std::fs::canonicalize(path)),
                (Ok(a), Ok(b)) if a == b
            )
    };
    let mut all: Vec<String> = Vec::new();
    for command in commands(text) {
        all.extend(command);
    }
    // Inside `sh -c '…'` too.
    let nested: Vec<String> = all
        .iter()
        .flat_map(|word| commands(word).into_iter().flatten())
        .collect();
    for words in [&all, &nested] {
        let mut iter = words.iter();
        while let Some(word) = iter.next() {
            if word == "-c" || word == "--config" {
                if iter.next().is_some_and(|next| is_ours(next)) {
                    return true;
                }
            } else if let Some(rest) = word.strip_prefix("--config=") {
                if is_ours(rest) {
                    return true;
                }
            }
        }
    }
    false
}

/// `path` for a POSIX shell: `~/` and the rest where it is in the home directory, quoted where
/// it needs to be.
pub fn shell_path(path: &Path) -> String {
    let quote = |text: &str| {
        let plain = !text.is_empty()
            && text
                .chars()
                .all(|char| char.is_ascii_alphanumeric() || "_./-+,:@%=".contains(char));
        if plain {
            text.to_owned()
        } else {
            format!("'{}'", text.replace('\'', "'\\''"))
        }
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(relative) => format!("~/{}", quote(&relative.display().to_string())),
        None => quote(&path.display().to_string()),
    }
}

/// The arguments that point waybar at our files: `-c <config> -s <style>`, for a shell.
pub fn arguments(files: &WaybarFiles) -> String {
    format!(
        "-c {} -s {}",
        shell_path(&files.config()),
        shell_path(&files.style())
    )
}

/// What the user changes so that waybar runs our files, for `starter`: the line to put in place
/// of theirs, or the drop-in for the unit. `None` where nothing is to change (ours already, or
/// our include starts it).
pub fn handover(starter: &Starter, files: &WaybarFiles) -> Option<String> {
    match starter {
        Starter::Nothing => None,
        Starter::Exec(line) | Starter::SwaybarCommand(line) if line.ours => None,
        Starter::Exec(line) | Starter::Maybe(line) => {
            let exec = if line.text.trim_start().starts_with("exec_always") {
                "exec_always"
            } else {
                "exec"
            };
            Some(format!("{exec} waybar {}", arguments(files)))
        }
        Starter::SwaybarCommand(_) => Some(format!("swaybar_command waybar {}", arguments(files))),
        Starter::Unit | Starter::UnitStart(_) => {
            let program =
                crate::delegate::find_program("waybar", std::env::var_os("PATH").as_deref())
                    .unwrap_or_else(|| PathBuf::from("/usr/bin/waybar"));
            let unit_path = |path: &Path| {
                let home = std::env::var_os("HOME").map(PathBuf::from);
                match home
                    .as_deref()
                    .and_then(|home| path.strip_prefix(home).ok())
                {
                    Some(relative) => format!("%h/{}", relative.display()),
                    None => path.display().to_string(),
                }
            };
            Some(format!(
                "[Service]\nExecStart=\nExecStart={} -c {} -s {}",
                program.display(),
                unit_path(&files.config()),
                unit_path(&files.style())
            ))
        }
    }
}

/// The command that opens the drop-in for waybar's unit in an editor.
pub const UNIT_EDIT: &str = "systemctl --user edit waybar.service";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sway_config::{MemoryFiles, parse};

    fn config(text: &str) -> SwayConfig {
        let files = MemoryFiles::new().file("/c/config", text);
        parse(Path::new("/c/config"), &files)
    }

    fn files() -> WaybarFiles {
        WaybarFiles {
            dir: PathBuf::from("/c/sway-things/waybar"),
        }
    }

    #[test]
    fn shell_commands_are_split_as_a_shell_splits_them() {
        assert_eq!(
            commands(r#"pkill waybar; sh -c 'a "b c"' && x\ y | z"#),
            [
                vec!["pkill".to_owned(), "waybar".to_owned()],
                vec!["sh".to_owned(), "-c".to_owned(), "a \"b c\"".to_owned()],
                vec!["x y".to_owned()],
                vec!["z".to_owned()],
            ]
        );
    }

    #[test]
    fn every_common_way_of_starting_waybar_is_seen() {
        for text in [
            "waybar",
            "/usr/bin/waybar -l info",
            "--no-startup-id waybar",
            "env GTK_THEME=Adwaita:dark waybar",
            "setsid -f waybar",
            "sh -c 'waybar'",
            "bash -lc \"sleep 1; exec waybar\"",
            "uwsm app -- waybar",
            "uwsm app waybar",
            "pkill waybar; waybar",
            "/nix/store/x-waybar/bin/.waybar-wrapped",
        ] {
            assert_eq!(starts(text, 0), Start::Program, "{text}");
        }
        for text in [
            "systemctl --user start waybar",
            "systemctl --user restart waybar.service",
        ] {
            assert_eq!(starts(text, 0), Start::Unit, "{text}");
        }
        assert_eq!(starts("~/.config/waybar/launch.sh", 0), Start::Maybe);
        for text in [
            "pkill waybar",
            "swaync",
            "kanshi",
            "killall -SIGUSR2 waybar",
        ] {
            assert_eq!(starts(text, 0), Start::No, "{text}");
        }
    }

    #[test]
    fn how_waybar_is_started_is_detected() {
        let generated = Path::new("/c/sway/sway-things");
        let detect = |text: &str| Starter::detect(&config(text), generated, &[], &files());
        assert_eq!(detect("set $mod Mod4\n"), Starter::Nothing);
        assert!(
            matches!(detect("exec waybar\n"), Starter::Exec(line) if line.line == 1 && !line.ours)
        );
        assert!(matches!(
            detect("exec_always pkill waybar; waybar -c /c/sway-things/waybar/config.jsonc\n"),
            Starter::Exec(line) if line.ours
        ));
        // A config path that only contains ours isn't ours.
        assert!(matches!(
            detect("exec waybar -c /c/sway-things/waybar/config.jsonc.bak\n"),
            Starter::Exec(line) if !line.ours
        ));
        assert!(matches!(
            detect("exec --no-startup-id waybar\n"),
            Starter::Exec(_)
        ));
        assert!(matches!(
            detect("exec systemctl --user start waybar\n"),
            Starter::UnitStart(_)
        ));
        assert!(matches!(
            detect("exec_always ~/.config/waybar/launch.sh\n"),
            Starter::Maybe(_)
        ));
        assert_eq!(detect("exec pkill waybar\n"), Starter::Nothing);
        assert!(matches!(
            detect("bar {\n    swaybar_command waybar\n    position top\n}\n"),
            Starter::SwaybarCommand(line) if line.line == 2
        ));
        assert_eq!(detect("bar {\n    position top\n}\n"), Starter::Nothing);
        let unit = Waybar {
            pid: 2,
            start: 1,
            config: None,
            unit: true,
        };
        assert_eq!(
            Starter::detect(&config(""), generated, &[unit], &files()),
            Starter::Unit
        );
    }

    #[test]
    fn swaybars_of_the_user_are_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        std::fs::write(
            &path,
            "bar {\n  position top\n  mode dock\n}\nbar {\n  position top\n  swaybar_command waybar\n}\n",
        )
        .unwrap();
        let config = parse(&path, &crate::sway_config::Disk);
        let found = swaybars(&config, Path::new("/c/sway/sway-things"));
        assert_eq!(found, [(path, 1)]);
    }

    #[test]
    fn the_handover_lines_point_at_our_files() {
        let line = Line {
            file: PathBuf::from("/c/config"),
            line: 3,
            text: "exec_always waybar".into(),
            ours: false,
        };
        let text = handover(&Starter::Exec(line.clone()), &files()).unwrap();
        assert!(text.starts_with("exec_always waybar -c "), "{text}");
        assert!(text.ends_with("waybar/style.css"), "{text}");
        let text = handover(&Starter::SwaybarCommand(line.clone()), &files()).unwrap();
        assert!(text.starts_with("swaybar_command waybar -c "));
        assert!(handover(&Starter::Maybe(line.clone()), &files()).is_some());
        let ours = Line { ours: true, ..line };
        assert_eq!(handover(&Starter::Exec(ours), &files()), None);
        assert!(
            handover(&Starter::Unit, &files())
                .unwrap()
                .contains("ExecStart=\n")
        );
        assert_eq!(shell_path(Path::new("/a b/c")), "'/a b/c'");
    }
}
