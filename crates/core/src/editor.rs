// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opening a file in the user's text editor, as the preference `app.editor_command` says.
//!
//! The command is split into a program and its arguments like a POSIX shell splits words (quotes
//! and backslashes group them), but nothing is expanded and no shell runs it. `{file}` and `{line}`
//! in an argument are filled in; without `{file}` the file is the last argument. An empty command
//! means `$VISUAL`, then `$EDITOR`, run in a terminal (most of them need one), and without those or
//! a terminal the desktop's default application for text files.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_process::{Child, Command};

use crate::registry::ValueError;

/// The key of the preference that holds the editor command.
pub const KEY: &str = "app.editor_command";

/// The program that starts a command in the user's terminal, from the proposed freedesktop.org
/// default terminal specification.
const TERMINAL_EXEC: &str = "xdg-terminal-exec";

/// Why a command can't be split into words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitError {
    /// A single or double quote is opened and never closed.
    UnclosedQuote,
    /// The command ends in a backslash, which escapes nothing.
    TrailingBackslash,
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnclosedQuote => f.write_str("has a quote that isn't closed"),
            Self::TrailingBackslash => f.write_str("ends in a backslash that escapes nothing"),
        }
    }
}

impl std::error::Error for SplitError {}

/// Splits `command` into words like a POSIX shell, without expanding anything: whitespace
/// separates words, single quotes keep everything up to the next one, double quotes keep
/// everything but `\"`, `\\`, `\$` and `` \` ``, and a backslash outside quotes keeps the
/// character after it. `$`, `~`, `*`, `;` and `|` are plain characters.
///
/// # Errors
///
/// [`SplitError`] for a quote that isn't closed or a backslash at the end.
pub fn split(command: &str) -> Result<Vec<String>, SplitError> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                let word = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err(SplitError::UnclosedQuote),
                    }
                }
            }
            '"' => {
                let word = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => word.push(c),
                            Some('\n') => {}
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err(SplitError::UnclosedQuote),
                        },
                        Some(c) => word.push(c),
                        None => return Err(SplitError::UnclosedQuote),
                    }
                }
            }
            '\\' => match chars.next() {
                Some('\n') => {}
                Some(c) => word.get_or_insert_with(String::new).push(c),
                None => return Err(SplitError::TrailingBackslash),
            },
            c if c.is_whitespace() => {
                if let Some(word) = word.take() {
                    words.push(word);
                }
            }
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    words.extend(word);
    Ok(words)
}

/// Checks that `command` can be split into words: the check of the preference's values.
///
/// # Errors
///
/// [`ValueError::NotACommand`] with the reason it can't.
pub fn check(command: &str) -> Result<(), ValueError> {
    split(command).map(drop).map_err(ValueError::NotACommand)
}

/// How to open a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// Run a program with arguments.
    Run(Run),
    /// Open it with the desktop's default application for text files.
    DefaultApp,
}

/// A program to run, with its arguments: never a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The program, a name looked up in `PATH` or a path.
    pub program: OsString,
    /// Its arguments.
    pub args: Vec<OsString>,
}

impl Run {
    /// Starts the program, with nothing on its standard input. Wait for the [`Child`] so it
    /// doesn't stay behind as a zombie.
    ///
    /// # Errors
    ///
    /// The error of the operating system if it can't be started, e.g. because it doesn't exist.
    pub fn spawn(&self) -> io::Result<Child> {
        Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::null())
            .spawn()
    }
}

/// How to open `file`, at `line` (counted from 1) if one is known, with the editor `command`
/// (the value of `app.editor_command`). `env` reads an environment variable (`VISUAL`, `EDITOR`,
/// `TERMINAL`, `PATH`); [`std::env::var_os`] in the app.
///
/// A command fills `{file}` and `{line}` into its arguments and drops an argument with `{line}`
/// when no line is known; without `{file}` anywhere, the file is the last argument. An empty
/// command takes `$VISUAL` or else `$EDITOR`, with `+LINE` (the convention of terminal editors)
/// and the file, and runs it in a terminal: with `xdg-terminal-exec` if it is in `PATH`, else
/// with `$TERMINAL -e`. Without an editor or a terminal, the default application opens the file.
/// A variable that is empty, not UTF-8 or can't be split counts as unset.
///
/// # Errors
///
/// [`SplitError`] if `command` can't be split into words.
pub fn resolve(
    command: &str,
    file: &Path,
    line: Option<usize>,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Launch, SplitError> {
    let words = split(command)?;
    if let Some((program, args)) = words.split_first() {
        return Ok(Launch::Run(fill(program, args, file, line)));
    }
    let words = |name: &str| {
        env(name)
            .and_then(|value| value.into_string().ok())
            .and_then(|value| split(&value).ok())
            .filter(|words| !words.is_empty())
    };
    let Some(editor) = words("VISUAL").or_else(|| words("EDITOR")) else {
        return Ok(Launch::DefaultApp);
    };
    let mut editor: Vec<OsString> = editor.into_iter().map(OsString::from).collect();
    editor.extend(line.map(|line| OsString::from(format!("+{line}"))));
    editor.push(file.as_os_str().to_owned());
    if let Some(exec) = find_program(TERMINAL_EXEC, env("PATH").as_deref()) {
        return Ok(Launch::Run(Run {
            program: exec.into_os_string(),
            args: editor,
        }));
    }
    let Some(terminal) = words("TERMINAL") else {
        return Ok(Launch::DefaultApp);
    };
    let mut terminal = terminal.into_iter().map(OsString::from);
    let Some(program) = terminal.next() else {
        return Ok(Launch::DefaultApp);
    };
    let mut args: Vec<OsString> = terminal.collect();
    args.push("-e".into());
    args.extend(editor);
    Ok(Launch::Run(Run { program, args }))
}

/// The program of a split command with its arguments, `{file}` and `{line}` filled in.
fn fill(program: &str, args: &[String], file: &Path, line: Option<usize>) -> Run {
    let mut has_file = program.contains("{file}");
    let mut filled = Vec::with_capacity(args.len() + 1);
    for arg in args {
        let arg = match line {
            Some(line) => arg.replace("{line}", &line.to_string()),
            None if arg.contains("{line}") => continue,
            None => arg.clone(),
        };
        has_file |= arg.contains("{file}");
        filled.push(with_file(&arg, file));
    }
    if !has_file {
        filled.push(file.as_os_str().to_owned());
    }
    let program = match line {
        Some(line) => program.replace("{line}", &line.to_string()),
        None => program.to_owned(),
    };
    Run {
        program: with_file(&program, file),
        args: filled,
    }
}

/// `text` with every `{file}` replaced by `file`, which needn't be UTF-8.
fn with_file(text: &str, file: &Path) -> OsString {
    let mut filled = OsString::new();
    let mut parts = text.split("{file}");
    if let Some(first) = parts.next() {
        filled.push(first);
    }
    for part in parts {
        filled.push(file.as_os_str());
        filled.push(part);
    }
    filled
}

/// The path of the executable `name` in one of the directories of `path` (a `PATH` value).
fn find_program(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use super::*;

    fn words(command: &str) -> Vec<String> {
        split(command).unwrap()
    }

    #[test]
    fn splits_on_whitespace() {
        assert_eq!(
            words("  code  -g\t{file}:{line} "),
            ["code", "-g", "{file}:{line}"]
        );
        assert_eq!(words(""), Vec::<String>::new());
        assert_eq!(words(" \t\n"), Vec::<String>::new());
    }

    #[test]
    fn quotes_group_words() {
        assert_eq!(
            words("'/opt/My Editor/ed' --x"),
            ["/opt/My Editor/ed", "--x"]
        );
        assert_eq!(words(r#""a b"'c d'e"#), ["a bc de"]);
        assert_eq!(words("'' x"), ["", "x"]);
        assert_eq!(words(r#""""#), [""]);
        assert_eq!(words(r#"'a "b" \c'"#), [r#"a "b" \c"#]);
    }

    #[test]
    fn backslashes_escape() {
        assert_eq!(words(r"a\ b c"), ["a b", "c"]);
        assert_eq!(words(r"\'x\'"), ["'x'"]);
        assert_eq!(words(r#""\"\\\$\`\n""#), [r#""\$`\n"#]);
        assert_eq!(words("a\\\nb"), ["ab"]);
    }

    #[test]
    fn expands_nothing() {
        assert_eq!(
            words("$EDITOR ~/x *.conf ; rm | cat && $(id) `id`"),
            [
                "$EDITOR", "~/x", "*.conf", ";", "rm", "|", "cat", "&&", "$(id)", "`id`"
            ]
        );
    }

    #[test]
    fn reports_unclosed_quotes_and_trailing_backslashes() {
        assert_eq!(split("nvim 'x"), Err(SplitError::UnclosedQuote));
        assert_eq!(split(r#"nvim "x"#), Err(SplitError::UnclosedQuote));
        assert_eq!(split(r#"nvim "x\"#), Err(SplitError::UnclosedQuote));
        assert_eq!(split(r"nvim x\"), Err(SplitError::TrailingBackslash));
        assert_eq!(
            check("nvim 'x"),
            Err(ValueError::NotACommand(SplitError::UnclosedQuote))
        );
        assert_eq!(check("nvim +{line} {file}"), Ok(()));
    }

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
            .collect();
        move |name| vars.get(name).cloned()
    }

    fn run(program: &str, args: &[&str]) -> Launch {
        Launch::Run(Run {
            program: program.into(),
            args: args.iter().map(OsString::from).collect(),
        })
    }

    const FILE: &str = "/home/u/.config/sway/config";

    fn open(command: &str, line: Option<usize>, vars: &[(&str, &str)]) -> Launch {
        resolve(command, Path::new(FILE), line, &env(vars)).unwrap()
    }

    #[test]
    fn a_command_gets_the_file_and_the_line() {
        assert_eq!(
            open("nvim +{line} {file}", Some(12), &[]),
            run("nvim", &["+12", FILE])
        );
        assert_eq!(
            open("code -g {file}:{line}", Some(3), &[]),
            run("code", &["-g", &format!("{FILE}:3")])
        );
        assert_eq!(
            open("foot -- nvim '+call cursor({line}, 1)'", Some(7), &[]),
            run("foot", &["--", "nvim", "+call cursor(7, 1)", FILE])
        );
    }

    #[test]
    fn a_command_without_file_gets_it_last() {
        assert_eq!(
            open("gnome-text-editor", Some(4), &[]),
            run("gnome-text-editor", &[FILE])
        );
        assert_eq!(
            open("kate -l {line}", Some(4), &[]),
            run("kate", &["-l", "4", FILE])
        );
    }

    #[test]
    fn without_a_line_arguments_with_it_are_dropped() {
        assert_eq!(open("nvim +{line} {file}", None, &[]), run("nvim", &[FILE]));
        assert_eq!(
            open("kate -l {line}", None, &[("EDITOR", "vi")]),
            run("kate", &["-l", FILE])
        );
        assert_eq!(
            open("code -g {file}:{line}", None, &[]),
            run("code", &["-g", FILE])
        );
    }

    #[test]
    fn a_command_ignores_the_environment() {
        assert_eq!(
            open(
                "mousepad",
                None,
                &[("VISUAL", "nvim"), ("TERMINAL", "foot")]
            ),
            run("mousepad", &[FILE])
        );
    }

    #[test]
    fn a_file_needs_not_be_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let file = Path::new(OsStr::from_bytes(b"/tmp/\xffconfig"));
        let launch = resolve("ed --file={file}", file, None, &env(&[])).unwrap();
        let mut arg = OsString::from("--file=");
        arg.push(file.as_os_str());
        assert_eq!(
            launch,
            Launch::Run(Run {
                program: "ed".into(),
                args: vec![arg],
            })
        );
    }

    #[test]
    fn a_bad_command_is_an_error() {
        assert_eq!(
            resolve("nvim 'x", Path::new(FILE), None, &env(&[])),
            Err(SplitError::UnclosedQuote)
        );
    }

    #[test]
    fn without_command_or_editor_the_default_app_opens_the_file() {
        assert_eq!(open("", Some(1), &[]), Launch::DefaultApp);
        assert_eq!(
            open("  ", None, &[("TERMINAL", "foot")]),
            Launch::DefaultApp
        );
        assert_eq!(
            open(
                "",
                None,
                &[("VISUAL", ""), ("EDITOR", " "), ("TERMINAL", "foot")]
            ),
            Launch::DefaultApp
        );
        assert_eq!(
            open("", None, &[("EDITOR", "nvim 'x"), ("TERMINAL", "foot")]),
            Launch::DefaultApp
        );
    }

    #[test]
    fn the_editor_runs_in_terminal() {
        assert_eq!(
            open("", Some(9), &[("EDITOR", "nvim"), ("TERMINAL", "foot")]),
            run("foot", &["-e", "nvim", "+9", FILE])
        );
        assert_eq!(
            open(
                "",
                None,
                &[
                    ("VISUAL", "emacs -nw"),
                    ("EDITOR", "vi"),
                    ("TERMINAL", "kitty --single-instance")
                ]
            ),
            run("kitty", &["--single-instance", "-e", "emacs", "-nw", FILE])
        );
    }

    #[test]
    fn without_terminal_the_default_app_opens_the_file() {
        assert_eq!(open("", Some(9), &[("EDITOR", "nvim")]), Launch::DefaultApp);
    }

    #[test]
    fn xdg_terminal_exec_comes_first() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        let bin = dir.path().join("bin");
        fs::create_dir(&empty).unwrap();
        fs::create_dir(&bin).unwrap();
        let exec = bin.join(TERMINAL_EXEC);
        fs::write(&exec, "").unwrap();
        // Not executable yet: it doesn't count.
        let path = format!("relative:{}:{}", empty.display(), bin.display());
        let vars = [
            ("EDITOR", "nvim"),
            ("TERMINAL", "foot"),
            ("PATH", path.as_str()),
        ];
        assert_eq!(
            open("", Some(2), &vars),
            run("foot", &["-e", "nvim", "+2", FILE])
        );
        fs::set_permissions(&exec, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            open("", Some(2), &vars),
            run(exec.to_str().unwrap(), &["nvim", "+2", FILE])
        );
        assert_eq!(
            open("", None, &[("EDITOR", "nvim"), ("PATH", path.as_str())]),
            run(exec.to_str().unwrap(), &["nvim", FILE])
        );
    }

    #[test]
    fn a_run_starts_its_program() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("opened");
        let launch = resolve("touch {file}", &marker, None, &env(&[])).unwrap();
        let Launch::Run(run) = launch else {
            panic!("expected a program to run");
        };
        let status = futures_lite::future::block_on(run.spawn().unwrap().status()).unwrap();
        assert!(status.success());
        assert!(marker.exists());
        let missing = Run {
            program: "sway-things-no-such-editor".into(),
            args: Vec::new(),
        };
        assert!(missing.spawn().is_err());
    }
}
