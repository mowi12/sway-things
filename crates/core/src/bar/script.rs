// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A custom module's command as a program and its arguments, and a test run of it.
//!
//! waybar runs a custom module's `exec` with `sh -c`. The page lets the user pick a program and
//! type its arguments, one per line, and [`join`] quotes them into the `exec` text, so no word can
//! become shell syntax. [`split`] reads such a text back into words; one that uses the shell
//! (a pipe, a variable, a glob, a redirection) isn't split, and stays text the page only shows.
//!
//! [`test`] runs the words once, as an argument list without a shell (never `sh -c`), with no
//! input, a time limit and a cap on what it reads, and reports what came out, so the user sees
//! what waybar would show before the module is in the bar. Nothing runs unless the user asks.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// The most bytes a test run reads from each of its outputs.
const MAX_OUTPUT: usize = 64 * 1024;

/// `words` as a command line for a POSIX shell: each word as it is where it has only characters
/// that mean nothing to a shell, else in single quotes. An argument that starts with `~/` stays
/// unquoted there, so the shell expands it, as [`split`] does.
pub fn join(words: &[String]) -> String {
    words
        .iter()
        .map(|word| quote(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// One word for a POSIX shell.
fn quote(word: &str) -> String {
    let plain = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|char| char.is_ascii_alphanumeric() || "_./-+,:@%=".contains(char))
    };
    if plain(word) {
        return word.to_owned();
    }
    if let Some(rest) = word.strip_prefix("~/") {
        if plain(rest) {
            return word.to_owned();
        }
        if !rest.is_empty() {
            return format!("~/{}", single(rest));
        }
    }
    single(word)
}

fn single(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The words of `text` as a POSIX shell would split one simple command: blanks between words,
/// `'…'`, `"…"` and `\` quoting, and `~/` at the start of a word as the home directory (`home`).
/// `None` for anything else the shell would do: a pipe, `;`, `&`, a redirection, a variable or
/// command substitution, a glob, braces, a comment, or quotes that don't end; and for an empty
/// command.
pub fn split(text: &str, home: Option<&Path>) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    while let Some(char) = chars.next() {
        match char {
            ' ' | '\t' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        other => word.push(other),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            // `$` and `` ` `` are escaped to keep them literal; read as the shell
                            // reads them, they are plain.
                            escaped @ ('"' | '\\' | '$' | '`') => word.push(escaped),
                            other => {
                                word.push('\\');
                                word.push(other);
                            }
                        },
                        '$' | '`' => return None,
                        other => word.push(other),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.push(chars.next()?);
            }
            '~' if !in_word && chars.peek() == Some(&'/') => {
                in_word = true;
                word.push_str(&home?.display().to_string());
            }
            '|' | '&' | ';' | '<' | '>' | '(' | ')' | '$' | '`' | '*' | '?' | '[' | '{' | '}'
            | '\n' | '\r' => return None,
            '#' | '~' if !in_word => return None,
            // `NAME=value prog` assigns a variable: not one program and its arguments.
            '=' if words.is_empty() && is_name(&word) => return None,
            other => {
                in_word = true;
                word.push(other);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    (!words.is_empty()).then_some(words)
}

/// Whether `word` is a name a shell can assign to.
fn is_name(word: &str) -> bool {
    let mut chars = word.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|char| char.is_ascii_alphanumeric() || char == '_')
}

/// How a test run went.
#[derive(Debug, Clone, PartialEq)]
pub struct TestRun {
    /// What it printed, at most 64 KiB, as UTF-8 with what isn't replaced.
    pub stdout: String,
    /// What it printed as errors, at most 64 KiB.
    pub stderr: String,
    /// Its exit code; `None` if it was ended (the time limit) or ended by a signal.
    pub code: Option<i32>,
    /// How long it ran.
    pub took: Duration,
    /// Whether it was ended for running longer than the time limit.
    pub timed_out: bool,
    /// For JSON output: the fields of the first line, or why it isn't JSON waybar reads.
    pub json: Option<Result<Vec<(String, String)>, String>>,
}

/// Why a test couldn't run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestError {
    /// No program given.
    Empty,
    /// The program isn't there: not in `$PATH`, or no file at the path.
    NotFound(String),
    /// The file is there but isn't executable.
    NotExecutable(PathBuf),
    /// It couldn't be started for another reason.
    Start(String),
}

/// Where `program` is: a path as it is, else the first executable in `path` (`$PATH`).
///
/// # Errors
///
/// [`TestError::NotFound`] or [`TestError::NotExecutable`].
pub fn locate(program: &str, path: Option<&std::ffi::OsStr>) -> Result<PathBuf, TestError> {
    use std::os::unix::fs::PermissionsExt as _;
    if program.is_empty() {
        return Err(TestError::Empty);
    }
    if program.contains('/') {
        let file = PathBuf::from(program);
        let meta = file
            .metadata()
            .map_err(|_| TestError::NotFound(program.to_owned()))?;
        if !meta.is_file() {
            return Err(TestError::NotFound(program.to_owned()));
        }
        if meta.permissions().mode() & 0o111 == 0 {
            return Err(TestError::NotExecutable(file));
        }
        return Ok(file);
    }
    crate::delegate::find_program(program, path)
        .ok_or_else(|| TestError::NotFound(program.to_owned()))
}

/// Runs `words` (the program, then its arguments) once, without a shell and with no input, and
/// ends it after `limit`. With `lines`, the command is one that keeps printing a line per update
/// (a module without an interval): it is ended once it printed its first line, which is what the
/// bar would show first. With `json`, the first line is read as waybar reads JSON output.
///
/// # Errors
///
/// [`TestError`] if it can't be started.
pub fn test(
    words: &[String],
    limit: Duration,
    lines: bool,
    json: bool,
) -> Result<TestRun, TestError> {
    let (program, arguments) = words.split_first().ok_or(TestError::Empty)?;
    let located = locate(program, std::env::var_os("PATH").as_deref())?;
    let started = Instant::now();
    let mut child = Command::new(&located)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| TestError::Start(error.to_string()))?;
    let stdout = child.stdout.take().map(|pipe| reader(pipe, lines));
    let stderr = child.stderr.take().map(|pipe| reader(pipe, false));
    let mut timed_out = false;
    let mut first_line = false;
    let status = loop {
        if let Ok(Some(status)) = child.try_wait() {
            break Some(status);
        }
        if lines
            && stdout
                .as_ref()
                .is_some_and(|(_, seen)| seen.load(std::sync::atomic::Ordering::Relaxed))
        {
            first_line = true;
            break None;
        }
        if started.elapsed() >= limit {
            timed_out = true;
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let took = started.elapsed();
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let collect = |handle: Option<(std::thread::JoinHandle<Vec<u8>>, _)>| {
        handle
            .and_then(|(thread, _)| thread.join().ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    };
    let mut stdout = collect(stdout);
    if first_line {
        if let Some(end) = stdout.find('\n') {
            stdout.truncate(end + 1);
        }
    }
    let stderr = collect(stderr);
    let json = json.then(|| fields(&stdout));
    Ok(TestRun {
        stdout,
        stderr,
        code: status.and_then(|status| status.code()),
        took,
        timed_out,
        json,
    })
}

/// Reads `pipe` up to [`MAX_OUTPUT`] bytes on a thread of its own; with `lines`, the flag it
/// returns says once a whole line has come.
fn reader(
    mut pipe: impl std::io::Read + Send + 'static,
    lines: bool,
) -> (
    std::thread::JoinHandle<Vec<u8>>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = seen.clone();
    let thread = std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let room = MAX_OUTPUT.saturating_sub(out.len());
                    out.extend_from_slice(buffer.get(..read.min(room)).unwrap_or_default());
                    if lines && out.contains(&b'\n') {
                        flag.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        }
        out
    });
    (thread, seen)
}

/// The fields of the first line of `stdout` as waybar reads a custom module's JSON (`text`,
/// `alt`, `tooltip`, `class`, `percentage`), or why it can't.
fn fields(stdout: &str) -> Result<Vec<(String, String)>, String> {
    let line = stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    match serde_json::from_str::<Value>(line) {
        Ok(Value::Object(map)) => Ok(map
            .into_iter()
            .map(|(key, value)| {
                let shown = match value {
                    Value::String(text) => text,
                    Value::Array(items) => items
                        .iter()
                        .map(|item| {
                            item.as_str()
                                .map_or_else(|| item.to_string(), str::to_owned)
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                    other => other.to_string(),
                };
                (key, shown)
            })
            .collect()),
        Ok(_) => Err("not a JSON object".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// How often a command runs a minute when it runs every `seconds`.
pub fn runs_a_minute(seconds: u64) -> u64 {
    60 / seconds.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn words_round_trip_through_the_shell_text() {
        let home = Path::new("/home/me");
        for list in [
            &["date", "+%H:%M"][..],
            &["/usr/bin/printf", "a b", "it's", "$HOME", "*", ""],
            &["~/bin/status.sh", "--json"],
            &["/home/me/my scripts/x", "a\"b\\c"],
        ] {
            let list = words(list);
            let text = join(&list);
            let expanded: Vec<String> = list
                .iter()
                .map(|word| match word.strip_prefix("~/") {
                    Some(rest) => format!("/home/me/{rest}"),
                    None => word.clone(),
                })
                .collect();
            assert_eq!(split(&text, Some(home)), Some(expanded), "{text}");
        }
        assert_eq!(join(&words(&["~/a b"])), "~/'a b'");
    }

    #[test]
    fn shell_syntax_is_not_split() {
        for text in [
            "a | b",
            "a; b",
            "a && b",
            "echo $HOME",
            "echo \"$HOME\"",
            "echo `id`",
            "ls *.txt",
            "a > out",
            "FOO=1 prog",
            "echo 'open",
            "# comment",
            "",
            "   ",
            "~user/x",
        ] {
            assert_eq!(split(text, Some(Path::new("/h"))), None, "{text}");
        }
        assert_eq!(
            split("printf 'a b' \"c d\" e\\ f key=value", None),
            Some(words(&["printf", "a b", "c d", "e f", "key=value"]))
        );
    }

    #[test]
    fn a_test_runs_without_a_shell_and_reads_json() {
        let run = test(
            &words(&["printf", "{\"text\": \"42\", \"class\": [\"a\", \"b\"]}\\n"]),
            Duration::from_secs(5),
            false,
            true,
        )
        .unwrap();
        assert_eq!(run.code, Some(0));
        assert!(!run.timed_out);
        assert_eq!(
            run.json,
            Some(Ok(vec![
                ("class".to_owned(), "a, b".to_owned()),
                ("text".to_owned(), "42".to_owned()),
            ]))
        );
        // No shell: `$HOME` and `;` are plain arguments.
        let run = test(
            &words(&["echo", "$HOME;", "x"]),
            Duration::from_secs(5),
            false,
            false,
        )
        .unwrap();
        assert_eq!(run.stdout, "$HOME; x\n");
        let run = test(
            &words(&["printf", "nope"]),
            Duration::from_secs(5),
            false,
            true,
        )
        .unwrap();
        assert!(matches!(run.json, Some(Err(_))));
    }

    #[test]
    fn a_test_ends_at_its_limit_or_its_first_line() {
        let run = test(
            &words(&["sleep", "10"]),
            Duration::from_millis(200),
            false,
            false,
        )
        .unwrap();
        assert!(run.timed_out);
        assert_eq!(run.code, None);
        assert!(run.took < Duration::from_secs(5));
        let run = test(
            &words(&["sh", "-c", "echo first; sleep 10; echo second"]),
            Duration::from_secs(5),
            true,
            false,
        )
        .unwrap();
        assert_eq!(run.stdout, "first\n");
        assert!(!run.timed_out);
        assert!(run.took < Duration::from_secs(5));
    }

    #[test]
    fn programs_that_can_t_run_are_named() {
        assert_eq!(
            test(&[], Duration::from_secs(1), false, false),
            Err(TestError::Empty)
        );
        assert!(matches!(
            test(
                &words(&["sway-things-no-such-program"]),
                Duration::from_secs(1),
                false,
                false
            ),
            Err(TestError::NotFound(_))
        ));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("script");
        std::fs::write(&file, "#!/bin/sh\n").unwrap();
        assert_eq!(
            locate(&file.display().to_string(), None),
            Err(TestError::NotExecutable(file))
        );
        assert_eq!(runs_a_minute(1), 60);
        assert_eq!(runs_a_minute(0), 60);
        assert_eq!(runs_a_minute(30), 2);
    }
}
