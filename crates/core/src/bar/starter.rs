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
    /// the bar. sway runs that command as one program name (`execvp`, with `-b <id>` after it),
    /// so a command with options (`waybar -c …`) starts nothing and isn't this.
    SwaybarCommand(Line),
    /// A waybar of this session runs as a systemd unit.
    Unit,
    /// A `bar` block of the user's that would run waybar, which our `75-bar.conf` took over: sway
    /// starts Sway Things' waybar as that bar ([`super::takeover`]).
    TakenOver(Line),
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
        let mut taken = None;
        let bars = super::takeover::bars(config, generated);
        for directive in &config.directives {
            if super::takeover::in_generated(&directive.file, generated) {
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
                // One word only: with options it is no program sway can run.
                let single = command.split_whitespace().count() == 1;
                if single && starts(&command, 0) == Start::Program {
                    // The bar the line is in: the last one made in its file before it.
                    let bar = bars
                        .iter()
                        .filter(|bar| {
                            bar.theirs && *bar.file == *directive.file && bar.line <= directive.line
                        })
                        .max_by_key(|bar| bar.line);
                    if bar.is_some_and(super::takeover::Bar::taken) {
                        taken.get_or_insert_with(|| line(directive));
                        continue;
                    }
                    return Self::SwaybarCommand(line(directive));
                }
            }
        }
        if let Some(line) = maybe {
            return Self::Maybe(line);
        }
        if let Some(line) = taken {
            return Self::TakenOver(line);
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

    /// Whether this surely starts a waybar of the user's own: a `bar` block's `swaybar_command
    /// waybar`, an `exec` line or a unit start without our config, or a running unit. A waybar of
    /// ours would then be a second bar. A line that only may start waybar isn't sure.
    pub fn starts_theirs(&self) -> bool {
        match self {
            Self::SwaybarCommand(_) | Self::Unit => true,
            Self::Exec(line) | Self::UnitStart(line) => !line.ours,
            Self::Nothing | Self::Maybe(_) | Self::TakenOver(_) => false,
        }
    }
}

/// The `bar` blocks of the user's sway config that run swaybar (no `swaybar_command` with
/// another program) and that Sway Things didn't take over: a bar next to ours. Each with the
/// line it starts on ([`super::takeover::bars`]).
pub fn swaybars(config: &SwayConfig, generated: &Path) -> Vec<(PathBuf, usize)> {
    super::takeover::bars(config, generated)
        .into_iter()
        .filter(|bar| bar.theirs && !bar.taken() && base(bar.program.trim()) == "swaybar")
        .map(|bar| (bar.file, bar.line))
        .collect()
}

/// The numbers of the lines that open a `bar` block (`bar {`, `bar <id> {`), counted from 1.
pub(super) fn block_openers(text: &str) -> Vec<usize> {
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

/// When a changed line of the user's sway config takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakesEffect {
    /// At the next reload of sway: a `bar` block or an `exec_always` line.
    Reload,
    /// At the next login: sway runs `exec` lines only when it starts.
    Login,
}

/// What to change in the user's sway config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Write this line in place of theirs.
    Replace(String),
    /// Remove the `bar { … }` block the line is in: sway can't give `swaybar_command` options,
    /// so no line there can point waybar at our files; without the block, our include starts
    /// waybar.
    RemoveBar,
}

/// The edit of the user's sway config that hands waybar over to our files, step by step: which
/// line of which file, what is written there now, what to do with it, and when it takes effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handover {
    /// The line that starts waybar.
    pub line: Line,
    /// What is written there now, without the indentation (`swaybar_command waybar`).
    pub current: String,
    /// What to change.
    pub edit: Edit,
    /// When the change takes effect.
    pub takes_effect: TakesEffect,
}

/// The edit of the user's sway config for `starter`, if a line of it starts waybar with another
/// config: `None` for a systemd unit (a drop-in, see [`handover`]), our own include, or a line
/// that already runs our files.
pub fn handover_edit(starter: &Starter, files: &WaybarFiles) -> Option<Handover> {
    let line = match starter {
        Starter::Exec(line) | Starter::Maybe(line) | Starter::SwaybarCommand(line)
            if !line.ours =>
        {
            line
        }
        _ => return None,
    };
    let text = line.text.trim();
    if let Starter::SwaybarCommand(_) = starter {
        // A line of a block may be given with the block's words in front (`bar swaybar_command …`).
        let current = text
            .find("swaybar_command")
            .map_or(text, |start| &text[start..]);
        return Some(Handover {
            line: line.clone(),
            current: current.to_owned(),
            edit: Edit::RemoveBar,
            takes_effect: TakesEffect::Reload,
        });
    }
    let takes_effect = if text.starts_with("exec_always") {
        TakesEffect::Reload
    } else {
        TakesEffect::Login
    };
    Some(Handover {
        line: line.clone(),
        current: text.to_owned(),
        edit: Edit::Replace(handover(starter, files)?),
        takes_effect,
    })
}

/// What the user changes so that waybar runs our files, for `starter`: the line to put in place
/// of theirs, or the drop-in for the unit. `None` where nothing is to change (ours already, or
/// our include starts it), and for `swaybar_command`, which takes no options: its `bar` block
/// goes instead ([`handover_edit`]).
pub fn handover(starter: &Starter, files: &WaybarFiles) -> Option<String> {
    match starter {
        Starter::Nothing | Starter::SwaybarCommand(_) | Starter::TakenOver(_) => None,
        Starter::Exec(line) if line.ours => None,
        Starter::Exec(line) | Starter::Maybe(line) => {
            let exec = if line.text.trim_start().starts_with("exec_always") {
                "exec_always"
            } else {
                "exec"
            };
            Some(format!("{exec} waybar {}", arguments(files)))
        }
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

/// Commands for a terminal that the user runs themselves to make the change of `edit`, and to
/// undo it: Sway Things never writes the user's config. The file is named by its real path
/// (with links such as `~/.config/sway` → `~/dotfiles/…` resolved), so `sed -i` changes the file
/// and doesn't replace a link with a copy. A copy of the file comes first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commands {
    /// What makes the change.
    pub change: String,
    /// What takes it back.
    pub undo: String,
}

/// The commands for `edit`: `#` in front of the line that starts waybar, or of every line of the
/// `bar { … }` block it is in; `None` where the file or the end of the block can't be read.
pub fn handover_commands(edit: &Handover) -> Option<Commands> {
    let real = std::fs::canonicalize(&edit.line.file).ok()?;
    let (first, last) = match edit.edit {
        Edit::Replace(_) => (edit.line.line, edit.line.line),
        Edit::RemoveBar => {
            let text = std::fs::read_to_string(&real).ok()?;
            block_around(&text, edit.line.line)?
        }
    };
    let file = quoted(&real.to_string_lossy());
    let backup = quoted(&format!("{}.bak-sway-things", real.to_string_lossy()));
    let range = if first == last {
        first.to_string()
    } else {
        format!("{first},{last}")
    };
    Some(Commands {
        change: format!("cp {file} {backup} && sed -i '{range}s/^/# /' {file}"),
        undo: format!("sed -i '{range}s/^# //' {file}"),
    })
}

/// The commands for waybar's systemd unit: disable it, and enable it again.
pub fn unit_commands() -> Commands {
    Commands {
        change: "systemctl --user disable --now waybar.service".to_owned(),
        undo: "systemctl --user enable --now waybar.service".to_owned(),
    }
}

/// `text` in single quotes for a shell.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The first and last line (counted from 1) of the `bar { … }` block that holds line `line`.
fn block_around(text: &str, line: usize) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let first = block_openers(text)
        .into_iter()
        .filter(|opener| *opener <= line)
        .max()?;
    let mut depth = 0_i32;
    for (index, content) in lines.iter().enumerate().skip(first - 1) {
        let content = content.split('#').next().unwrap_or("");
        depth += i32::try_from(content.matches('{').count()).ok()?;
        depth -= i32::try_from(content.matches('}').count()).ok()?;
        if depth <= 0 {
            return (index + 1 >= line).then_some((first, index + 1));
        }
    }
    None
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
    fn a_bar_block_taken_over_starts_ours() {
        let dir = tempfile::tempdir().unwrap();
        let generated = dir.path().join("sway-things");
        std::fs::create_dir(&generated).unwrap();
        let path = dir.path().join("config");
        std::fs::write(
            &path,
            format!(
                "bar {{\n  swaybar_command waybar\n}}\nbar {{\n  position top\n}}\n\
                 include {}/*.conf\n",
                generated.display()
            ),
        )
        .unwrap();
        let detect = || {
            Starter::detect(
                &parse(&path, &crate::sway_config::Disk),
                &generated,
                &[],
                &files(),
            )
        };
        assert!(matches!(detect(), Starter::SwaybarCommand(line) if line.line == 2));
        assert_eq!(
            swaybars(&parse(&path, &crate::sway_config::Disk), &generated),
            [(path.clone(), 4)]
        );
        std::fs::write(
            generated.join("75-bar.conf"),
            "bar \"bar-0\" swaybar_command sway-things\nbar \"bar-1\" swaybar_command true\n",
        )
        .unwrap();
        let starter = detect();
        assert!(matches!(&starter, Starter::TakenOver(line) if line.line == 2));
        assert!(starter.is_elsewhere() && !starter.starts_theirs());
        // Their other bar shows nothing now: no swaybar next to ours.
        assert_eq!(
            swaybars(&parse(&path, &crate::sway_config::Disk), &generated),
            Vec::<(PathBuf, usize)>::new()
        );
    }

    #[test]
    fn the_commands_comment_out_the_right_lines_in_the_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let real_dir = dir.path().join("dotfiles");
        std::fs::create_dir(&real_dir).unwrap();
        let real = real_dir.join("it's config");
        std::fs::write(
            &real,
            "set $mod Mod4\nexec_always waybar\nbar {\n  colors {\n    background #000000\n  }\n  \
             swaybar_command waybar\n}\nbindsym $mod+q kill\n",
        )
        .unwrap();
        let linked = dir.path().join("linked");
        std::os::unix::fs::symlink(&real_dir, &linked).unwrap();
        let line = |number, text: &str| Line {
            file: linked.join("it's config"),
            line: number,
            text: text.to_owned(),
            ours: false,
        };
        let real = std::fs::canonicalize(&real).unwrap();
        let file = format!("'{}'", real.display()).replace("it's", "it'\\''s");
        let exec = Handover {
            line: line(2, "exec_always waybar"),
            current: "exec_always waybar".to_owned(),
            edit: Edit::Replace(String::new()),
            takes_effect: TakesEffect::Reload,
        };
        let commands = handover_commands(&exec).unwrap();
        assert_eq!(
            commands.change,
            format!(
                "cp {file} {} && sed -i '2s/^/# /' {file}",
                format!("'{}.bak-sway-things'", real.display()).replace("it's", "it'\\''s")
            )
        );
        assert_eq!(commands.undo, format!("sed -i '2s/^# //' {file}"));
        let block = Handover {
            line: line(7, "swaybar_command waybar"),
            edit: Edit::RemoveBar,
            ..exec
        };
        let commands = handover_commands(&block).unwrap();
        assert!(
            commands
                .change
                .ends_with(&format!("sed -i '3,8s/^/# /' {file}"))
        );
        assert_eq!(commands.undo, format!("sed -i '3,8s/^# //' {file}"));
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
        // sway can't give swaybar_command options: no line for it.
        assert_eq!(
            handover(&Starter::SwaybarCommand(line.clone()), &files()),
            None
        );
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

    #[test]
    fn the_handover_names_the_line_what_it_says_and_when_it_takes_effect() {
        let dir = tempfile::tempdir().unwrap();
        let generated = Path::new("/c/sway/sway-things");
        let edit = |text: &str| {
            let path = dir.path().join("config");
            std::fs::write(&path, text).unwrap();
            let config = parse(&path, &crate::sway_config::Disk);
            handover_edit(
                &Starter::detect(&config, generated, &[], &files()),
                &files(),
            )
        };
        let bar = edit("set $mod Mod4\nbar {\n    swaybar_command waybar\n    position top\n}\n")
            .unwrap();
        assert_eq!(bar.line.line, 3);
        assert_eq!(bar.current, "swaybar_command waybar");
        // The block goes.
        assert_eq!(bar.edit, Edit::RemoveBar);
        assert_eq!(bar.takes_effect, TakesEffect::Reload);
        let always = edit("exec_always waybar\n").unwrap();
        assert_eq!(always.current, "exec_always waybar");
        assert!(
            matches!(&always.edit, Edit::Replace(line) if line.starts_with("exec_always waybar -c "))
        );
        assert_eq!(always.takes_effect, TakesEffect::Reload);
        // sway runs `exec` lines only when it starts.
        assert_eq!(
            edit("exec waybar\n").unwrap().takes_effect,
            TakesEffect::Login
        );
        // Nothing to edit: our include starts it, or the line runs our files already.
        assert_eq!(edit("bar {\n    position top\n}\n"), None);
        let ours = format!("exec waybar {}\n", arguments(&files()));
        assert_eq!(edit(&ours), None);
        // sway runs swaybar_command as one program name, so with options it starts nothing
        // (seen on sway 1.10, 1.12 and SwayFX): our include starts waybar then.
        let options = format!(
            "bar {{\n    swaybar_command waybar {}\n}}\n",
            arguments(&files())
        );
        assert_eq!(edit(&options), None);
        let path = dir.path().join("config");
        let config = parse(&path, &crate::sway_config::Disk);
        assert_eq!(
            Starter::detect(&config, generated, &[], &files()),
            Starter::Nothing
        );
    }
}
