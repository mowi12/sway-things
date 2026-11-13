// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading a sway config with everything it includes, following `read_config` in sway's
//! `sway/config.c`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::line::{self, Variable};
use super::{Files, expand};
use crate::sway_config::ExpandError;

/// Includes nested deeper than this aren't followed. sway has no such limit, but it also has no
/// config that deep.
const MAX_INCLUDE_DEPTH: usize = 32;

/// Blocks nested deeper than this are skipped with their lines. Every line of a block repeats the
/// lines that opened it, so deep nesting would take quadratic time; sway's own configs nest two
/// deep (`bar { colors {`).
const MAX_BLOCK_DEPTH: usize = 32;

/// A config with more directives than this isn't read to the end.
const MAX_DIRECTIVES: usize = 100_000;

/// A file bigger than this isn't read.
const MAX_FILE_SIZE: usize = 8 * 1024 * 1024;

/// A line longer than this once its variables are replaced isn't read, and neither is the rest
/// of the config.
const MAX_LINE: usize = 8 * 1024 * 1024;

/// The config isn't read further once the text of its lines adds up to this, counted with their
/// blocks' prefixes and their variables replaced, and with the paths of their includes. A short
/// line can stand for a long block prefix or variable, so without a limit the result could grow
/// with the square of the input.
const MAX_TEXT: usize = 64 * 1024 * 1024;

/// A directive of the config: one command, as sway runs it while reading the files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// The file the directive is in.
    pub file: Arc<Path>,
    /// The number of its first line, counted from 1.
    pub line: usize,
    /// The block the directive is in, as words: `["input", "type:touchpad"]`, `["bar"]`,
    /// `["mode", "resize"]`, `["bar", "colors"]`. Empty at the top level.
    ///
    /// `input`, `output` and `seat` settings have their block here also when they are written on
    /// one line (`input type:touchpad tap enabled`), so both spellings read the same.
    pub block: Vec<String>,
    /// The command, as written: `tap`, `gaps`, `client.focused`. sway ignores its case.
    pub name: String,
    /// The arguments as the command gets them: variables replaced, quotes removed and escapes
    /// resolved, except for commands like `exec` and `bindsym`, which get their line as written.
    pub args: Vec<String>,
    /// The line as written, with a continued line joined and blanks at both ends removed.
    pub text: String,
}

impl Directive {
    /// Whether this is the command `name` in the block `block`, without regard to case.
    pub fn is(&self, block: &[&str], name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
            && self.block.len() == block.len()
            && self
                .block
                .iter()
                .zip(block)
                .all(|(have, want)| have.eq_ignore_ascii_case(want))
    }
}

/// An `include` directive and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Include {
    /// The file the directive is in.
    pub file: Arc<Path>,
    /// The number of its line, counted from 1.
    pub line: usize,
    /// The argument, with sway's variables replaced.
    pub pattern: String,
    /// The paths the argument stands for, before following symbolic links; empty if it can't be
    /// expanded. A glob that matches nothing is here as the pattern itself.
    pub paths: Vec<PathBuf>,
    /// The files that were read because of it, with symbolic links resolved.
    pub loaded: Vec<Arc<Path>>,
}

/// Something in the config the parser couldn't follow. None of these stop the parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The file it is about, or in.
    pub file: Arc<Path>,
    /// The number of the line, counted from 1; 0 if it is about the whole file.
    pub line: usize,
    /// What it is.
    pub kind: DiagnosticKind,
}

/// What the parser couldn't follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// The file can't be read. For the main config, nothing was parsed.
    Unreadable {
        /// The error of the operating system.
        message: String,
    },
    /// An `include` whose paths can't be known without running a shell, or that sway itself
    /// rejects. Nothing was included; what the included files would set is unknown.
    UnresolvedInclude {
        /// The argument of the include.
        pattern: String,
        /// Why it can't be expanded.
        reason: ExpandError,
    },
    /// An included path doesn't exist; sway skips it. A glob that matches nothing ends up here
    /// too.
    IncludeNotFound {
        /// The path.
        path: PathBuf,
    },
    /// The file was already read, so sway doesn't read it again. This also ends include cycles.
    AlreadyIncluded {
        /// The file.
        path: PathBuf,
    },
    /// An `include` without exactly one argument, which sway rejects.
    InvalidInclude,
    /// Includes are nested too deeply to follow.
    TooDeep,
    /// A `}` without a block to close.
    UnmatchedBrace,
    /// A block that is still open at the end of the file.
    UnclosedBlock,
    /// A block nested too deeply to follow: it was skipped up to its `}`, with everything in it.
    BlockTooDeep,
    /// The config has too many directives, or a line that is too long once its variables are
    /// replaced; the rest wasn't read.
    TooBig,
}

/// A sway config as sway reads it: the directives of the main file and of every file it
/// includes, in the order sway runs them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SwayConfig {
    /// The files that were read, the main config first.
    pub files: Vec<Arc<Path>>,
    /// Every directive except block starts and ends, in the order sway runs them.
    pub directives: Vec<Directive>,
    /// Every `include`, in order.
    pub includes: Vec<Include>,
    /// What couldn't be followed.
    pub diagnostics: Vec<Diagnostic>,
}

/// The path of the config sway loads without `--config`: the first that exists of
/// `~/.sway/config`, `$XDG_CONFIG_HOME/sway/config`, `~/.i3/config`, `$XDG_CONFIG_HOME/i3/config`,
/// `/etc/sway/config` and `/etc/i3/config`, with `~/.config` for an unset `XDG_CONFIG_HOME`.
///
/// In a running session, `GET_VERSION` names the loaded config, which is the better source.
pub fn default_path(files: &dyn Files) -> Option<PathBuf> {
    default_paths(files)
        .into_iter()
        .find(|path| files.exists(path))
}

/// The paths [`default_path`] tries, in sway's order, whether they exist or not.
pub fn default_paths(files: &dyn Files) -> Vec<PathBuf> {
    let home = files.env("HOME").filter(|home| !home.is_empty());
    let config_home = files
        .env("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .or_else(|| home.as_ref().map(|home| format!("{home}/.config")));
    let etc = Some("/etc".to_owned());
    [
        (&home, ".sway"),
        (&config_home, "sway"),
        (&home, ".i3"),
        (&config_home, "i3"),
        (&etc, "sway"),
        (&etc, "i3"),
    ]
    .into_iter()
    .filter_map(|(prefix, folder)| {
        Some(PathBuf::from(format!(
            "{}/{folder}/config",
            prefix.as_ref()?
        )))
    })
    .collect()
}

/// Reads the config at `main` and everything it includes. Nothing is executed and nothing is
/// written.
///
/// The parser is best-effort: it follows what sway does with the text, but doesn't know which
/// commands exist. What it can't follow is in [`SwayConfig::diagnostics`], and it reads on.
pub fn parse(main: &Path, files: &dyn Files) -> SwayConfig {
    let mut parser = Parser {
        files,
        config: SwayConfig::default(),
        variables: Vec::new(),
        stopped: false,
        budget: MAX_TEXT,
    };
    let path: Arc<Path> = match files.canonicalize(main) {
        Ok(path) => path.into(),
        Err(error) => {
            let file: Arc<Path> = main.into();
            parser.diagnose(
                &file,
                0,
                DiagnosticKind::Unreadable {
                    message: error.to_string(),
                },
            );
            return parser.config;
        }
    };
    parser.load(&path, 0);
    parser.config
}

struct Parser<'a> {
    files: &'a dyn Files,
    config: SwayConfig,
    /// The variables set so far, longest name first.
    variables: Vec<Variable>,
    /// Whether the config was too big to read to the end: nothing more is read.
    stopped: bool,
    /// How much more text the config may expand to, see [`MAX_TEXT`].
    budget: usize,
}

/// An open block.
struct Block {
    /// The line that opened it, without the brace: what sway puts in front of every line inside.
    prefix: Vec<u8>,
    /// How many arguments the prefix has.
    words: usize,
    /// The line it was opened on.
    line: usize,
}

impl Parser<'_> {
    fn diagnose(&mut self, file: &Arc<Path>, line: usize, kind: DiagnosticKind) {
        self.config.diagnostics.push(Diagnostic {
            file: Arc::clone(file),
            line,
            kind,
        });
    }

    /// Stops reading the config at this line, because it is too big to read to the end.
    fn stop(&mut self, file: &Arc<Path>, line: usize) {
        self.diagnose(file, line, DiagnosticKind::TooBig);
        self.stopped = true;
    }

    /// Takes `length` bytes of text from the budget. If that is more than is left, stops reading
    /// the config at this line and returns false.
    fn spend(&mut self, length: usize, file: &Arc<Path>, line: usize) -> bool {
        if let Some(rest) = self.budget.checked_sub(length) {
            self.budget = rest;
            true
        } else {
            self.stop(file, line);
            false
        }
    }

    /// Reads the file at `path`, which is canonical. Returns whether it could be read.
    fn load(&mut self, path: &Arc<Path>, depth: usize) -> bool {
        let content = match self.files.read(path) {
            Ok(content) if content.len() <= MAX_FILE_SIZE => content,
            Ok(_) => {
                self.diagnose(path, 0, DiagnosticKind::TooBig);
                return false;
            }
            Err(error) => {
                self.diagnose(
                    path,
                    0,
                    DiagnosticKind::Unreadable {
                        message: error.to_string(),
                    },
                );
                return false;
            }
        };
        self.config.files.push(Arc::clone(path));
        self.read_lines(path, &content, depth);
        true
    }

    /// Reads the lines of the file at `path`.
    fn read_lines(&mut self, path: &Arc<Path>, content: &[u8], depth: usize) {
        let mut blocks: Vec<Block> = Vec::new();
        // How many blocks deeper than MAX_BLOCK_DEPTH are open; their lines are skipped.
        let mut skipped = 0usize;
        let mut lines = Lines::new(content);
        while let Some((number, raw)) = lines.next_logical() {
            if self.stopped {
                break;
            }
            if self.config.directives.len() >= MAX_DIRECTIVES {
                self.stop(path, number);
                break;
            }
            let text = line::strip_whitespace(&raw);
            if text.is_empty() || text.first() == Some(&b'#') {
                continue;
            }
            // A brace on a line of its own belongs to the line before it.
            let mut opens = false;
            if !matches!(text.last(), Some(b'{' | b'}')) {
                opens = lines.skip_lone_brace();
            }
            let mut expanded = Vec::new();
            // Inside a block nested too deeply only the braces count, which need no prefix.
            if let Some(block) = blocks.last().filter(|_| skipped == 0) {
                expanded.extend_from_slice(&block.prefix);
                expanded.push(b' ');
            }
            expanded.extend_from_slice(text);
            if opens {
                expanded.extend_from_slice(b" {");
            }

            if !self.spend(expanded.len(), path, number) {
                break;
            }
            let raw_args = line::split_args(&expanded);
            match raw_args.last().map(Vec::as_slice) {
                None => continue,
                // A skipped line is in a block, so it would have a prefix.
                Some(b"{") if raw_args.len() > 1 || skipped > 0 => {
                    if skipped > 0 {
                        skipped += 1;
                    } else if blocks.len() >= MAX_BLOCK_DEPTH {
                        self.diagnose(path, number, DiagnosticKind::BlockTooDeep);
                        skipped = 1;
                    } else {
                        let words = &raw_args[..raw_args.len() - 1];
                        blocks.push(Block {
                            prefix: words.join(&b' '),
                            words: words.len(),
                            line: number,
                        });
                    }
                    continue;
                }
                Some(b"}") => {
                    if skipped > 0 {
                        skipped -= 1;
                    } else if blocks.pop().is_none() {
                        self.diagnose(path, number, DiagnosticKind::UnmatchedBrace);
                    }
                    continue;
                }
                Some(_) if skipped > 0 => continue,
                Some(_) => {}
            }
            let block_words = blocks.last().map_or(0, |block| block.words);
            self.command(path, number, text, &expanded, block_words, depth);
        }
        // A config that wasn't read to the end may well close its blocks after that.
        if let Some(block) = blocks.first().filter(|_| !self.stopped) {
            self.diagnose(path, block.line, DiagnosticKind::UnclosedBlock);
        }
    }

    /// Runs the line `text` (number `number`) of the file at `path`, which is `expanded` with the
    /// prefix of its block (`block_words` arguments), the way sway runs it while reading.
    fn command(
        &mut self,
        path: &Arc<Path>,
        number: usize,
        text: &[u8],
        expanded: &[u8],
        block_words: usize,
        depth: usize,
    ) {
        let limit = MAX_LINE.min(self.budget);
        let Some(args) = line::arguments(expanded, &self.variables, limit) else {
            self.stop(path, number);
            return;
        };
        if !self.spend(args.iter().map(Vec::len).sum(), path, number) {
            return;
        }
        let args: Vec<String> = args
            .iter()
            .map(|arg| String::from_utf8_lossy(arg).into_owned())
            .collect();
        let (block, rest) = args.split_at(block_words.min(args.len()));
        let Some((name, rest)) = rest.split_first() else {
            return;
        };
        let mut directive = Directive {
            file: Arc::clone(path),
            line: number,
            block: block.to_vec(),
            name: name.clone(),
            args: rest.to_vec(),
            text: String::from_utf8_lossy(text).into_owned(),
        };
        if directive.block.is_empty() {
            if directive.is(&[], "set") {
                self.set(&directive.args);
            } else if directive.is(&[], "include") {
                self.include(&directive, depth);
                return;
            }
            normalize(&mut directive);
        }
        self.config.directives.push(directive);
    }

    /// `set $name value…`: sway keeps the variables sorted by the length of their names, longest
    /// first, so that `$mod_alt` is found before `$mod`.
    fn set(&mut self, args: &[String]) {
        let Some((name, value)) = args.split_first() else {
            return;
        };
        if !name.starts_with('$') || value.is_empty() {
            return;
        }
        let name = name.as_bytes().to_vec();
        let value = value.join(" ").into_bytes();
        if let Some(variable) = self
            .variables
            .iter_mut()
            .find(|variable| variable.name == name)
        {
            variable.value = value;
        } else {
            self.variables.push(Variable { name, value });
            self.variables
                .sort_by_key(|variable| std::cmp::Reverse(variable.name.len()));
        }
    }

    /// `include <paths>`: reads the files in place, relative to the directory of the file the
    /// directive is in.
    fn include(&mut self, directive: &Directive, depth: usize) {
        let (file, line) = (&directive.file, directive.line);
        let mut include = Include {
            file: Arc::clone(file),
            line,
            pattern: directive.args.join(" "),
            paths: Vec::new(),
            loaded: Vec::new(),
        };
        let [pattern] = directive.args.as_slice() else {
            self.diagnose(file, line, DiagnosticKind::InvalidInclude);
            self.config.includes.push(include);
            return;
        };
        let directory = file.parent().unwrap_or(Path::new("/"));
        match expand(pattern, directory, self.files) {
            Ok(paths) => include.paths = paths,
            Err(reason) => {
                self.diagnose(
                    file,
                    line,
                    DiagnosticKind::UnresolvedInclude {
                        pattern: pattern.clone(),
                        reason,
                    },
                );
            }
        }
        // The include is recorded before its files, so the includes are in the order sway
        // meets them.
        let index = self.config.includes.len();
        let paths = include.paths.clone();
        self.config.includes.push(include);
        // A short pattern can stand for thousands of paths, which are kept and looked up.
        let length = paths.iter().map(|path| path.as_os_str().len()).sum();
        if !self.spend(length, file, line) {
            return;
        }
        for path in paths {
            if self.stopped {
                break;
            }
            let Ok(real) = self.files.canonicalize(&path) else {
                self.diagnose(file, line, DiagnosticKind::IncludeNotFound { path });
                continue;
            };
            let real: Arc<Path> = real.into();
            if self.config.files.contains(&real) {
                self.diagnose(
                    file,
                    line,
                    DiagnosticKind::AlreadyIncluded {
                        path: real.to_path_buf(),
                    },
                );
            } else if depth >= MAX_INCLUDE_DEPTH {
                self.diagnose(file, line, DiagnosticKind::TooDeep);
            } else if self.load(&real, depth + 1) {
                if let Some(include) = self.config.includes.get_mut(index) {
                    include.loaded.push(real);
                }
            }
        }
    }
}

/// Gives `input`, `output` and `seat` settings written on one line the block they would have in
/// a block: `input type:touchpad tap enabled` becomes `tap enabled` in `input type:touchpad`.
fn normalize(directive: &mut Directive) {
    let is_block_command = ["input", "output", "seat"]
        .iter()
        .any(|name| directive.name.eq_ignore_ascii_case(name));
    if is_block_command && directive.args.len() >= 2 {
        let identifier = directive.args.remove(0);
        let name = directive.args.remove(0);
        directive.block = vec![std::mem::replace(&mut directive.name, name), identifier];
    }
}

/// The lines of a file, joined where a line ends with a backslash, like sway's
/// `getline_with_cont`.
struct Lines<'a> {
    rest: &'a [u8],
    /// The number of the next physical line.
    number: usize,
}

impl<'a> Lines<'a> {
    fn new(content: &'a [u8]) -> Self {
        Self {
            rest: content,
            number: 1,
        }
    }

    /// The next physical line with its line break, if it has one.
    fn physical(&mut self) -> Option<&'a [u8]> {
        if self.rest.is_empty() {
            return None;
        }
        let end = self
            .rest
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(self.rest.len(), |index| index + 1);
        let (line, rest) = self.rest.split_at(end);
        self.rest = rest;
        self.number += 1;
        Some(line)
    }

    /// The next line with its continuations, without the final line break, and the number of
    /// its first physical line.
    fn next_logical(&mut self) -> Option<(usize, Vec<u8>)> {
        let number = self.number;
        let mut line = self.physical()?.to_vec();
        // A comment doesn't continue, and neither does a backslash at the very end of the file.
        while line.ends_with(b"\\\n") && line.first() != Some(&b'#') {
            let Some(next) = self.physical() else {
                break;
            };
            line.truncate(line.len() - 2);
            line.extend_from_slice(next);
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        Some((number, line))
    }

    /// If the next line that isn't blank is a lone `{`, skips up to and including it.
    fn skip_lone_brace(&mut self) -> bool {
        let (rest, number) = (self.rest, self.number);
        while let Some(line) = self.physical() {
            let line = line::strip_whitespace(line);
            if line.is_empty() {
                continue;
            }
            if line == b"{" {
                return true;
            }
            break;
        }
        (self.rest, self.number) = (rest, number);
        false
    }
}
