// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The user's own kanshi config, read as kanshi reads it, as far as Sway Things needs it: its
//! profiles, the displays each one names, and each profile's statements as written, so that a
//! profile can be copied into the file of ours unchanged but for the lines that place
//! workspaces ([`UserProfile::copy`]).
//!
//! The grammar is kanshi's (`man 5 kanshi`): `profile [<name>] { … }`, `output <criteria> …` or
//! `output <criteria> { … }` (in a profile, or as defaults and aliases at the top), `...output`,
//! `exec <command>` (the rest of the line, as it is), `include <path>`, and `#` comments. Words
//! follow scfg, which kanshi reads its config with: a word is quoted with `"` (in which `\`
//! escapes the next character) or with `'` (as it is), and outside quotes `\` escapes the next
//! character too. Nothing here runs anything: an `exec` line is only text that is copied, or left
//! out.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

/// A display of a profile that is any one display, whichever (`output *`).
pub const ONE: &str = "*";
/// Any number of displays, whichever (`...output *`).
pub const ANY: &str = "...*";

/// A profile of the user's kanshi config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserProfile {
    /// Its name as written, or its position (`#1`) for one without.
    pub name: String,
    /// The displays it names, as written: a name, an identifier, an alias, or [`ONE`] and
    /// [`ANY`] for the wildcards.
    pub outputs: Vec<String>,
    /// Its statements as written, in order.
    pub statements: Vec<Statement>,
}

/// A statement of a profile, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// Its text, from its first word to its end (a block with its braces), without a comment
    /// after it.
    pub text: String,
    /// What it is.
    pub kind: Kind,
}

/// What a statement of a profile is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `output` or `...output`: copied as it is.
    Output,
    /// An `exec` line that places workspaces (`swaymsg` with `workspace`): left out of a copy,
    /// since Sway Things places them.
    PlacesWorkspaces,
    /// An `exec` line that runs a hook of Sway Things: left out, the copy runs its own.
    OurHook,
    /// An `exec` line that names workspaces without `swaymsg`, a script of the user's, say:
    /// copied, but it may place workspaces too.
    MaybeWorkspaces,
    /// Any other `exec` line, or a directive kanshi may know and Sway Things doesn't: copied.
    Other,
}

impl UserProfile {
    /// The `exec` lines that name workspaces but aren't recognized as placing them: a copy
    /// keeps them, and they may move workspaces after Sway Things placed them.
    pub fn unsure(&self) -> impl Iterator<Item = &str> {
        self.statements
            .iter()
            .filter(|statement| statement.kind == Kind::MaybeWorkspaces)
            .map(|statement| statement.text.as_str())
    }

    /// The name of its copy in the file of ours: `sway-things-of-` and a hash of its name and
    /// its displays, so it stays the same while the profile does.
    pub fn copy_name(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.name.as_bytes());
        for output in &self.outputs {
            hash.update([0]);
            hash.update(output.as_bytes());
        }
        let digest = hash.finalize();
        let mut name = String::from("sway-things-of-");
        for byte in digest.iter().take(4) {
            let _ = write!(name, "{byte:02x}");
        }
        name
    }

    /// The copy of this profile for the file of ours: its statements as written, but for the
    /// `exec` lines that run a hook of ours and, with `places` (Sway Things places the
    /// workspaces), those that place workspaces; then `extra` (comment lines and our hooks, each
    /// a line without indentation). `from` says where it comes from, in the comment above it.
    /// `None` for a profile without displays.
    pub fn copy(&self, from: &str, extra: &[String], places: bool) -> Option<String> {
        if self.outputs.is_empty() {
            return None;
        }
        let mut block = String::new();
        let named: String = self
            .name
            .chars()
            .map(|char| if char.is_control() { ' ' } else { char })
            .collect();
        if places {
            let _ = writeln!(
                block,
                "# Your profile \u{201c}{named}\u{201d} from {from}, as you wrote it, but for the \
                 lines that place workspaces:\n# Sway Things places them."
            );
        } else {
            let _ = writeln!(
                block,
                "# Your profile \u{201c}{named}\u{201d} from {from}, as you wrote it."
            );
        }
        let _ = writeln!(block, "profile {} {{", self.copy_name());
        for statement in &self.statements {
            if statement.kind == Kind::OurHook
                || (places && statement.kind == Kind::PlacesWorkspaces)
            {
                continue;
            }
            let mut lines = statement.text.lines();
            if let Some(first) = lines.next() {
                let _ = writeln!(block, "    {}", first.trim_start());
            }
            for line in lines {
                let _ = writeln!(block, "{line}");
            }
        }
        for line in extra {
            let _ = writeln!(block, "    {line}");
        }
        block.push_str("}\n");
        Some(block)
    }
}

/// What the top level of a kanshi config holds, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A profile.
    Profile(UserProfile),
    /// An `include` line, as written (the path with `include`).
    Include(String),
}

/// The profiles and includes of the kanshi config `text`, in order. What can't be read is
/// skipped up to the end of its line, as far as that goes.
pub fn parse(text: &str) -> Vec<Item> {
    let mut reader = Reader::new(text);
    let mut items = Vec::new();
    let mut unnamed = 0;
    loop {
        reader.skip_blank(true);
        let begin = reader.offset_of(reader.at);
        let Some(word) = reader.word() else {
            if reader.peek().is_some() {
                // A brace or something else where a directive should be: past it.
                reader.at += 1;
                continue;
            }
            break;
        };
        match word.as_str() {
            "profile" => {
                reader.skip_blank(false);
                let name = if reader.peek() == Some('{') {
                    None
                } else {
                    reader.word()
                };
                reader.skip_blank(true);
                if reader.peek() != Some('{') {
                    reader.skip_line();
                    continue;
                }
                reader.at += 1;
                unnamed += 1;
                let statements = reader.profile_body();
                let outputs = statements
                    .iter()
                    .filter(|statement| statement.kind == Kind::Output)
                    .filter_map(|statement| criteria(&statement.text))
                    .collect();
                items.push(Item::Profile(UserProfile {
                    name: name.unwrap_or_else(|| format!("#{unnamed}")),
                    outputs,
                    statements,
                }));
            }
            "include" => {
                reader.skip_line_unquoted();
                let end = reader.offset_of(reader.at);
                items.push(Item::Include(text[begin..end].trim().to_owned()));
            }
            // Defaults and aliases of displays, or anything else: past it.
            _ => reader.skip_statement(),
        }
    }
    items
}

/// The path of the `include` line `line` as kanshi reads it: the word after `include`, with
/// its quotes and escapes undone, so `include '~/My Displays.conf'` and
/// `include ~/My\ Displays.conf` both name `~/My Displays.conf`. `None` for another line.
pub fn include_path(line: &str) -> Option<String> {
    let mut reader = Reader::new(line.trim());
    if reader.word()? != "include" {
        return None;
    }
    reader.skip_blank(false);
    reader.word()
}

/// The profiles of the kanshi config `text`, in order.
pub fn profiles(text: &str) -> Vec<UserProfile> {
    parse(text)
        .into_iter()
        .filter_map(|item| match item {
            Item::Profile(profile) => Some(profile),
            Item::Include(_) => None,
        })
        .collect()
}

/// The display an `output` statement names: its criteria, with [`ONE`] or [`ANY`] for a
/// wildcard.
fn criteria(statement: &str) -> Option<String> {
    let (any, rest) = match statement.strip_prefix("...output") {
        Some(rest) => (true, rest),
        None => (false, statement.strip_prefix("output")?),
    };
    let mut reader = Reader::new(rest);
    reader.skip_blank(false);
    let criteria = reader.word()?;
    match criteria.as_str() {
        "" => None,
        "*" if any => Some(ANY.to_owned()),
        "*" => Some(ONE.to_owned()),
        _ => Some(criteria),
    }
}

/// Whether `word` names the program `name`, as is or by a path.
fn is_program(word: &str, name: &str) -> bool {
    word == name || word.ends_with(&format!("/{name}"))
}

/// What an `exec` command does, as far as workspaces go. Only a `swaymsg` whose every command
/// moves or shows a workspace places workspaces and nothing else; a line that does more (sets up
/// a display, starts a program, runs through the shell) is kept, and is only unsure if it names
/// workspaces.
fn classify(command: &str) -> Kind {
    let words: Vec<String> = command
        .split(|char: char| char.is_whitespace() || ",;'\"&|()".contains(char))
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    if words.iter().any(|word| is_program(word, "sway-things"))
        && words.iter().any(|word| word == "hook")
    {
        return Kind::OurHook;
    }
    if let Some(commands) = swaymsg_commands(command) {
        return if !commands.is_empty() && commands.iter().all(|command| places(command)) {
            Kind::PlacesWorkspaces
        } else if commands.iter().any(|command| command.contains("workspace")) {
            Kind::MaybeWorkspaces
        } else {
            Kind::Other
        };
    }
    // A script or a line for the shell: unsure where it runs swaymsg with a workspace, or where
    // the program itself is named after workspaces.
    let swaymsg = words.iter().any(|word| is_program(word, "swaymsg"));
    let first = words.first().map(String::as_str).unwrap_or_default();
    if (swaymsg && words.iter().any(|word| word == "workspace")) || first.contains("workspace") {
        Kind::MaybeWorkspaces
    } else {
        Kind::Other
    }
}

/// The sway commands of `command` if it is only `swaymsg [-q] <commands>`, split at `,` and
/// `;` outside quotes and in lower case; `None` for anything else: another program, another
/// option of swaymsg (a query with `-t`), or anything the shell would do more with.
fn swaymsg_commands(command: &str) -> Option<Vec<String>> {
    let words = shell_words(command)?;
    let (program, arguments) = words.split_first()?;
    if !is_program(program, "swaymsg") {
        return None;
    }
    let mut arguments = arguments.iter().peekable();
    while let Some(option) = arguments.next_if(|word| word.starts_with('-')) {
        if option != "-q" && option != "--quiet" {
            return None;
        }
    }
    let joined = arguments.map(String::as_str).collect::<Vec<_>>().join(" ");
    let mut commands = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for char in joined.chars() {
        match char {
            '"' => {
                quoted = !quoted;
                current.push(char);
            }
            ',' | ';' if !quoted => commands.push(std::mem::take(&mut current)),
            _ => current.push(char),
        }
    }
    commands.push(current);
    Some(
        commands
            .iter()
            .map(|command| command.trim().to_lowercase())
            .filter(|command| !command.is_empty())
            .collect(),
    )
}

/// Whether the sway command `command` only shows a workspace or moves one to a display:
/// `workspace N [output …]`, `move workspace to [output] X` or `focus output X`, not
/// `workspace N gaps …`.
fn places(command: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    match words.as_slice() {
        ["workspace", rest @ ..] => !rest.is_empty() && !rest.contains(&"gaps"),
        ["move", "workspace", "to", _, ..] | ["focus", "output", _, ..] => true,
        _ => false,
    }
}

/// The words of `command` as the shell splits them, with quotes and escapes undone; `None` if
/// the shell would do more than run one program: an operator, a substitution, a redirection, a
/// variable, or a quote that isn't closed.
fn shell_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = command.trim().chars();
    while let Some(char) = chars.next() {
        match char {
            '\'' => {
                let current = word.get_or_insert_with(String::new);
                loop {
                    match chars.next()? {
                        '\'' => break,
                        inner => current.push(inner),
                    }
                }
            }
            '"' => {
                let current = word.get_or_insert_with(String::new);
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => current.push(chars.next()?),
                        '$' | '`' => return None,
                        inner => current.push(inner),
                    }
                }
            }
            '\\' => word.get_or_insert_with(String::new).push(chars.next()?),
            ';' | '&' | '|' | '`' | '$' | '(' | ')' | '<' | '>' | '\n' => return None,
            _ if char.is_whitespace() => words.extend(word.take()),
            _ => word.get_or_insert_with(String::new).push(char),
        }
    }
    words.extend(word);
    Some(words)
}

/// Reads a kanshi config from left to right.
struct Reader<'a> {
    text: &'a str,
    chars: Vec<(usize, char)>,
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            chars: text.char_indices().collect(),
            text,
            at: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).map(|(_, char)| *char)
    }

    /// The byte offset of the character at `at`, or the end.
    fn offset_of(&self, at: usize) -> usize {
        self.chars
            .get(at)
            .map_or(self.text.len(), |(offset, _)| *offset)
    }

    /// Past spaces and comments, and line breaks too with `lines`.
    fn skip_blank(&mut self, lines: bool) {
        while let Some(char) = self.peek() {
            if char == '#' {
                while self.peek().is_some_and(|char| char != '\n') {
                    self.at += 1;
                }
            } else if char == '\n' && !lines {
                return;
            } else if char.is_whitespace() {
                self.at += 1;
            } else {
                return;
            }
        }
    }

    /// A word at this point, as scfg reads it: a quoted string without its quotes, or a bare
    /// word up to a space, a brace or a quote, with escapes undone, so `"Home Office"`,
    /// `'Home Office'` and `Home\ Office` are all `Home Office`. `None` at a brace, a line
    /// break or the end.
    fn word(&mut self) -> Option<String> {
        if matches!(self.peek()?, '{' | '}' | '\n') {
            return None;
        }
        let mut word = String::new();
        let mut quoted = false;
        while let Some(char) = self.peek() {
            match char {
                '"' | '\'' if word.is_empty() && !quoted => {
                    quoted = true;
                    self.at += 1;
                    while let Some(inner) = self.peek() {
                        if inner == char || inner == '\n' {
                            break;
                        }
                        self.at += 1;
                        if inner == '\\' && char == '"' {
                            if let Some(escaped) = self.peek().filter(|next| *next != '\n') {
                                word.push(escaped);
                                self.at += 1;
                            }
                        } else {
                            word.push(inner);
                        }
                    }
                    if self.peek() == Some(char) {
                        self.at += 1;
                    }
                    break;
                }
                '\\' => {
                    self.at += 1;
                    if let Some(escaped) = self.peek().filter(|next| *next != '\n') {
                        word.push(escaped);
                        self.at += 1;
                    }
                }
                _ if char.is_whitespace() || matches!(char, '{' | '}' | '#' | '"' | '\'') => break,
                _ => {
                    word.push(char);
                    self.at += 1;
                }
            }
        }
        (quoted || !word.is_empty()).then_some(word)
    }

    /// Past a quoted string that starts here, `"…"` (with `\` escapes) or `'…'`, up to the end
    /// of its line if it isn't closed.
    fn skip_quoted(&mut self) {
        let Some(quote) = self.peek() else {
            return;
        };
        self.at += 1;
        while let Some(char) = self.peek() {
            if char == '\n' {
                return;
            }
            self.at += 1;
            if char == quote {
                return;
            }
            if char == '\\' && quote == '"' && self.peek().is_some_and(|next| next != '\n') {
                self.at += 1;
            }
        }
    }

    /// To the end of the line, as it is.
    fn skip_line(&mut self) {
        while self.peek().is_some_and(|char| char != '\n') {
            self.at += 1;
        }
    }

    /// To the end of the line or a comment, past quoted strings, stopping before a `{` that
    /// starts a block and a `}` that ends the profile it is in.
    fn skip_line_unquoted(&mut self) {
        while let Some(char) = self.peek() {
            match char {
                '\n' | '#' | '{' | '}' => return,
                '"' | '\'' => self.skip_quoted(),
                '\\' => self.at = (self.at + 2).min(self.chars.len()),
                _ => self.at += 1,
            }
        }
    }

    /// Past a block `{ … }` that starts here, with nested ones and quoted strings.
    fn skip_block(&mut self) {
        let mut depth = 0_usize;
        while let Some(char) = self.peek() {
            match char {
                '{' => {
                    depth += 1;
                    self.at += 1;
                }
                '}' => {
                    self.at += 1;
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return;
                    }
                }
                '#' => self.skip_line(),
                '"' | '\'' => self.skip_quoted(),
                '\\' => self.at = (self.at + 2).min(self.chars.len()),
                _ => self.at += 1,
            }
        }
    }

    /// Past a directive that started with its first word: the rest of its line, or a block.
    fn skip_statement(&mut self) {
        // The rest of the line, or a block that starts on it: `output "A B C" {`.
        self.skip_line_unquoted();
        if self.peek() == Some('{') {
            self.skip_block();
        }
    }

    /// The statements of a profile, after its `{`, up to and past its `}`.
    fn profile_body(&mut self) -> Vec<Statement> {
        let mut statements = Vec::new();
        loop {
            self.skip_blank(true);
            match self.peek() {
                None => return statements,
                Some('}') => {
                    self.at += 1;
                    return statements;
                }
                Some('{') => {
                    // A block without a directive: past it.
                    self.skip_block();
                    continue;
                }
                Some(_) => {}
            }
            let start = self.offset_of(self.at);
            let Some(word) = self.word() else {
                self.at += 1;
                continue;
            };
            let kind = match word.as_str() {
                "exec" => {
                    self.skip_line();
                    let text = self.text[start..self.offset_of(self.at)].trim_end();
                    let command = text.strip_prefix("exec").unwrap_or_default();
                    statements.push(Statement {
                        text: text.to_owned(),
                        kind: classify(command),
                    });
                    continue;
                }
                "output" | "...output" => Kind::Output,
                _ => Kind::Other,
            };
            // The criteria and directives, or a block after the criteria.
            self.skip_line_unquoted();
            let mut end = self.offset_of(self.at);
            if self.peek() == Some('{') {
                self.skip_block();
                end = self.offset_of(self.at);
            }
            statements.push(Statement {
                text: self.text[start..end].trim_end().to_owned(),
                kind,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ANY, Item, Kind, ONE, include_path, parse, profiles};

    #[test]
    fn profiles_are_read_with_their_displays_and_statements() {
        let text = "# My displays\n\
                    output \"Dell U27 ABC\" {\n  alias $desk\n  scale 1.5\n}\n\
                    include ~/.config/kanshi/sway-things.conf\n\
                    profile \"Home Office\" {\n\
                    \x20   output $desk enable position 0,0 # the big one\n\
                    \x20   output eDP-1 disable\n\
                    \x20   exec swaymsg workspace 1 output '\"Dell U27 ABC\"', workspace 1, move workspace to output '\"Dell U27 ABC\"'\n\
                    \x20   exec swaymsg workspace 1\n\
                    \x20   exec notify-send 'Docked {at home}'\n\
                    \x20   exec ~/bin/arrange-workspaces\n\
                    \x20   exec sway-things hook workspaces\n\
                    }\n\
                    profile {\n    output \"Foo Bar *\" {\n        mode 1920x1080\n        scale 2\n    }\n    ...output * enable\n}\n\
                    profile one { output * enable }\n";
        let items = parse(text);
        assert_eq!(
            items[0],
            Item::Include("include ~/.config/kanshi/sway-things.conf".to_owned())
        );
        let found = profiles(text);
        assert_eq!(found.len(), 3);
        let home = &found[0];
        assert_eq!(home.name, "Home Office");
        assert_eq!(home.outputs, ["$desk", "eDP-1"]);
        let kinds: Vec<Kind> = home
            .statements
            .iter()
            .map(|statement| statement.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                Kind::Output,
                Kind::Output,
                Kind::PlacesWorkspaces,
                Kind::PlacesWorkspaces,
                Kind::Other,
                Kind::MaybeWorkspaces,
                Kind::OurHook,
            ]
        );
        // As written, without the comment; an exec line keeps its braces and quotes.
        assert_eq!(home.statements[0].text, "output $desk enable position 0,0");
        assert_eq!(
            home.statements[4].text,
            "exec notify-send 'Docked {at home}'"
        );
        let second = &found[1];
        assert_eq!(second.name, "#2");
        assert_eq!(second.outputs, ["Foo Bar *", ANY]);
        assert_eq!(
            second.statements[0].text,
            "output \"Foo Bar *\" {\n        mode 1920x1080\n        scale 2\n    }"
        );
        // On one line, the `}` ends the profile.
        assert_eq!(found[2].name, "one");
        assert_eq!(found[2].outputs, [ONE]);
        assert_eq!(found[2].statements[0].text, "output * enable");
    }

    #[test]
    fn a_copy_keeps_the_profile_but_the_lines_that_place_workspaces() {
        let text = "profile Desk {\n    output eDP-1 disable\n    output \"Dell U27 ABC\" {\n        mode 2560x1440\n    }\n\
                    \x20   exec swaymsg workspace 1 output eDP-1\n    exec notify-send docked\n}\n";
        let found = profiles(text);
        let copy = found[0]
            .copy(
                "~/.config/kanshi/config",
                &["exec sway-things hook workspaces".to_owned()],
                true,
            )
            .unwrap();
        assert_eq!(
            copy,
            format!(
                "# Your profile \u{201c}Desk\u{201d} from ~/.config/kanshi/config, as you wrote it, but \
                 for the lines that place workspaces:\n# Sway Things places them.\n\
                 profile {} {{\n    output eDP-1 disable\n    output \"Dell U27 ABC\" {{\n        mode \
                 2560x1440\n    }}\n    exec notify-send docked\n    exec sway-things hook workspaces\n}}\n",
                found[0].copy_name()
            )
        );
        // The same profile, the same name; another one, another name.
        assert_eq!(found[0].copy_name(), profiles(text)[0].copy_name());
        assert_ne!(
            found[0].copy_name(),
            profiles("profile Other { output eDP-1 enable }")[0].copy_name()
        );
        assert!(found[0].copy_name().starts_with("sway-things-of-"));
    }

    #[test]
    fn words_are_read_as_scfg_reads_them() {
        // kanshi 1.9 reads all of these (checked with `kanshi -c`).
        let text = "profile 'Home Office' {\n    output 'Dell U27 ABC' enable\n}\n\
                    profile \"Say \\\"hi\\\"\" {\n    output \"A \\\\ B\" enable\n}\n\
                    profile Lab\\ Desk {\n    output Foo\\ Bar enable\n    output 'it' enable\n}\n\
                    profile '' {\n    output 'Brace { inside' enable\n}\n";
        let found = profiles(text);
        let names: Vec<&str> = found.iter().map(|profile| profile.name.as_str()).collect();
        assert_eq!(names, ["Home Office", "Say \"hi\"", "Lab Desk", ""]);
        assert_eq!(found[0].outputs, ["Dell U27 ABC"]);
        assert_eq!(found[1].outputs, ["A \\ B"]);
        assert_eq!(found[2].outputs, ["Foo Bar", "it"]);
        // A brace in quotes neither opens nor closes anything.
        assert_eq!(found[3].outputs, ["Brace { inside"]);
        assert_eq!(
            found[3].statements[0].text,
            "output 'Brace { inside' enable"
        );
    }

    #[test]
    fn only_a_line_that_does_nothing_but_place_workspaces_is_left_out() {
        use super::classify;
        for command in [
            " swaymsg workspace 1",
            " swaymsg -q workspace 1 output '\"Dell U27 ABC\"', workspace 1, move workspace to output '\"Dell U27 ABC\"'",
            " /usr/bin/swaymsg \"workspace number 3; move workspace to output DP-1\"",
            " swaymsg focus output eDP-1",
        ] {
            assert_eq!(classify(command), Kind::PlacesWorkspaces, "{command}");
        }
        // More than placing: kept, and unsure where it names workspaces.
        for command in [
            " swaymsg \"output eDP-1 pos 0 0; workspace 1\"",
            " swaymsg 'for_window [app_id=x] move to workspace 3'",
            " swaymsg workspace 1; exec foot",
            " sh -c \"swaymsg 'output X enable' && swaymsg workspace 1\"",
            " swaymsg workspace 1 gaps inner 5",
            " ~/bin/arrange-workspaces",
        ] {
            assert_eq!(classify(command), Kind::MaybeWorkspaces, "{command}");
        }
        for command in [
            " swaymsg -t get_workspaces",
            " notify-send 'workspace ready'",
            " swaymsg output eDP-1 disable",
        ] {
            assert_eq!(classify(command), Kind::Other, "{command}");
        }
        assert_eq!(classify(" sway-things hook workspaces"), Kind::OurHook);
    }

    #[test]
    fn without_a_placement_the_lines_that_place_workspaces_stay() {
        let text = "profile Desk {\n    output eDP-1 enable\n    exec swaymsg workspace 1 output eDP-1\n    \
                    exec sway-things hook workspaces\n}\n";
        let copy = profiles(text)[0]
            .copy("~/k", &["exec sway-things hook mirror".to_owned()], false)
            .unwrap();
        assert!(
            copy.contains("exec swaymsg workspace 1 output eDP-1"),
            "{copy}"
        );
        assert!(!copy.contains("hook workspaces"), "{copy}");
        assert!(
            copy.starts_with("# Your profile \u{201c}Desk\u{201d} from ~/k, as you wrote it.\n")
        );
    }

    #[test]
    fn what_cant_be_read_is_skipped() {
        // An unclosed profile, a stray brace and a directive kanshi doesn't know.
        assert_eq!(
            profiles("}\nfoo bar\nprofile a {\n output X enable\n").len(),
            1
        );
        assert_eq!(profiles(""), Vec::new());
        assert_eq!(profiles("profile\n"), Vec::new());
    }

    #[test]
    fn the_path_of_an_include_is_read_as_kanshi_reads_it() {
        for line in [
            "include ~/My\\ Displays.conf",
            "include \"~/My Displays.conf\"",
            "include '~/My Displays.conf'",
            "  include '~/My Displays.conf' # mine",
        ] {
            assert_eq!(
                include_path(line).as_deref(),
                Some("~/My Displays.conf"),
                "{line}"
            );
        }
        assert_eq!(include_path("included ~/a.conf"), None);
        assert_eq!(include_path("profile a {"), None);
        assert_eq!(include_path("include"), None);
    }
}
