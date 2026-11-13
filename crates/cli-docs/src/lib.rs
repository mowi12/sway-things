// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The man pages and shell completions of Sway Things, generated from the clap definitions of the
//! command line.
//!
//! [`files`] renders every file in memory, at its path below `data/`. The binary
//! `sway-things-cli-docs` writes them there (`scripts/update-cli-docs.sh`), and a test fails while
//! the committed files differ. The crate is no default member of the workspace, so a plain
//! `cargo build` and the distro builds never compile it.

use std::io;
use std::path::PathBuf;

use clap::{Arg, Command, CommandFactory as _};
use clap_complete::Shell;
use clap_mangen::Man;
use roff::{Roff, bold, italic, roman};
use sway_things_cli::{Cli, Exit, GUI, GUI_ENV};
use sway_things_core::i18n::LOCALEDIR_VAR;

/// The name of the command line's binary.
pub const BIN: &str = "sway-things";

/// The version in the page headers.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The manual the pages belong to.
const MANUAL: &str = "Sway Things Manual";

/// The directory of the man pages below `data/`.
pub const MAN_DIR: &str = "man";

/// The directory of the shell completions below `data/`.
pub const COMPLETIONS_DIR: &str = "completions";

/// The completion files, by shell, as each shell names them.
pub const COMPLETIONS: [(Shell, &str); 3] = [
    (Shell::Bash, "sway-things.bash"),
    (Shell::Zsh, "_sway-things"),
    (Shell::Fish, "sway-things.fish"),
];

/// The preamble `roff` writes ahead of every rendering, which defines the apostrophe. A page is
/// put together from several renderings, so it is dropped from all but the first.
const PREAMBLE: &str = ".ie \\n(.g .ds Aq \\(aq\n.el .ds Aq '\n";

/// Every generated file: its path below `data/` and its content.
///
/// # Errors
///
/// Only if rendering a page fails, which it doesn't when writing to memory.
pub fn files() -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut cli = Cli::command().disable_help_subcommand(true);
    cli.build();
    let mut files = vec![man_file(BIN, top_page(&cli)?)];
    subcommand_pages(&cli, &mut files)?;
    files.push(man_file(GUI, gui_page()?));
    for (shell, name) in COMPLETIONS {
        let mut content = Vec::new();
        clap_complete::generate(shell, &mut Cli::command(), BIN, &mut content);
        files.push((PathBuf::from(COMPLETIONS_DIR).join(name), content));
    }
    Ok(files)
}

/// The command `sway-things-gui` parses: at most one link. `GApplication` reads the arguments,
/// not clap; this command only describes them, and a test keeps both in line.
pub fn gui_command() -> Command {
    Command::new(GUI)
        .version(VERSION)
        .about("Settings app for sway and SwayFX")
        .long_about(
            "Sway Things changes the settings of sway and SwayFX: live, in its own settings files \
             and in sway config files of its own, which your sway config includes. It never edits \
             your config.\n\n\
             The app runs once per session. Starting sway-things-gui again brings the running \
             window up, at the link if one is given; sway-things open does the same from the \
             command line.",
        )
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(
            Arg::new("link").value_name("LINK").help(
                "A page id, e.g. keyboard, or a setting key, e.g. input.keyboard.repeat_rate",
            ),
        )
}

/// The path of the man page of `name` and its content.
fn man_file(name: &str, content: Vec<u8>) -> (PathBuf, Vec<u8>) {
    (PathBuf::from(MAN_DIR).join(format!("{name}.1")), content)
}

/// The pages of the subcommands of `command` that aren't hidden, and of theirs, as
/// `clap_mangen::generate_to` writes them.
fn subcommand_pages(command: &Command, files: &mut Vec<(PathBuf, Vec<u8>)>) -> io::Result<()> {
    for subcommand in command.get_subcommands().filter(|sub| !sub.is_hide_set()) {
        subcommand_pages(subcommand, files)?;
        let page = Man::new(subcommand.clone());
        let mut out = Page::new(&page);
        out.add(|w| page.render(w))?;
        files.push((PathBuf::from(MAN_DIR).join(page.get_filename()), out.0));
    }
    Ok(())
}

/// The page of `sway-things`: clap's sections, then the exit status, the environment and the
/// files, the version, and the related pages.
fn top_page(cli: &Command) -> io::Result<Vec<u8>> {
    let page = Man::new(cli.clone());
    let mut out = Page::new(&page);
    out.add(|w| page.render_name_section(w))?;
    out.add(|w| page.render_synopsis_section(w))?;
    out.add(|w| page.render_description_section(w))?;
    out.add(|w| page.render_options_section(w))?;
    out.add(|w| page.render_subcommands_section(w))?;
    out.add(|w| exit_status().to_writer(w))?;
    out.add(|w| environment().to_writer(w))?;
    out.add(|w| files_section().to_writer(w))?;
    out.add(|w| page.render_version_section(w))?;
    let mut related: Vec<String> = cli
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .map(|sub| format!("{BIN}-{}", sub.get_name()))
        .collect();
    related.push(GUI.to_owned());
    out.add(|w| see_also(&related).to_writer(w))?;
    Ok(out.0)
}

/// The page of `sway-things-gui`: clap's sections, then the related pages.
fn gui_page() -> io::Result<Vec<u8>> {
    let page = Man::new(gui_command());
    let mut out = Page::new(&page);
    out.add(|w| page.render(w))?;
    let related = [BIN.to_owned(), format!("{BIN}-open")];
    out.add(|w| see_also(&related).to_writer(w))?;
    Ok(out.0)
}

/// A page put together from renderings: the preamble and the title, then the sections.
struct Page(Vec<u8>);

impl Page {
    /// A page with the preamble and the title line of `man`. `clap_mangen` writes the title line
    /// itself, but drops the empty date, which shifts the source and the manual into the wrong
    /// places; this one names the empty date, so the page has no date and regenerating it
    /// changes nothing.
    fn new(man: &Man) -> Self {
        let name = man.get_filename();
        let title = name.strip_suffix(".1").unwrap_or(&name);
        let line = format!(".TH {title} 1 \"\" \"Sway Things {VERSION}\" \"{MANUAL}\"\n");
        Self([PREAMBLE, &line].concat().into_bytes())
    }

    /// Appends what `render` writes, without the preamble and without a title line.
    fn add(&mut self, render: impl FnOnce(&mut dyn io::Write) -> io::Result<()>) -> io::Result<()> {
        let mut rendered = Vec::new();
        render(&mut rendered)?;
        let rendered = String::from_utf8(rendered)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let rest = rendered.strip_prefix(PREAMBLE).unwrap_or(&rendered);
        for line in rest.split_inclusive('\n') {
            if !line.starts_with(".TH ") {
                self.0.extend_from_slice(line.as_bytes());
            }
        }
        Ok(())
    }
}

/// What an exit code of `sway-things` means.
fn exit_meaning(exit: Exit) -> &'static str {
    match exit {
        Exit::Ok => "Done.",
        Exit::Failure => {
            "Failure: sway rejected a command or a generated file, a file can't be read or \
             written, there is no sway to ask, or the app can't be found or started. For doctor: \
             --fix couldn't write the generated files."
        }
        Exit::Usage => {
            "Wrong arguments: an unknown key or page, a value the setting doesn't take, or \
             --device for a setting that isn't per device."
        }
        Exit::ReadOnly => {
            "Read-only: your sway config doesn't include the generated files yet, so nothing was \
             changed. sway-things doctor shows the line to add."
        }
        Exit::Refused => {
            "Refused: a risky setting without a terminal or --yes, a change that wasn't kept, a \
             setting the compositor doesn't support, or a generated file that was changed outside \
             and not overwritten."
        }
        Exit::Problems => "doctor found at least one problem, also after --fix.",
    }
}

/// The section EXIT STATUS: every exit code and what it means.
fn exit_status() -> Roff {
    let mut roff = Roff::new();
    roff.control("SH", ["EXIT STATUS"]);
    for exit in Exit::ALL {
        roff.control("TP", []);
        roff.text([bold(exit.code().to_string())]);
        roff.text([roman(exit_meaning(exit))]);
    }
    roff
}

/// The section ENVIRONMENT.
fn environment() -> Roff {
    let variables = [
        (
            "SWAYSOCK",
            "The IPC socket of the running sway session, which changes are sent to.",
        ),
        (
            "XDG_CONFIG_HOME",
            "Where the settings files, the generated files and your sway config are; \
             ~/.config if unset.",
        ),
        (
            "XDG_STATE_HOME",
            "Where the list of generated files and the history of the settings files are; \
             ~/.local/state if unset.",
        ),
        (
            GUI_ENV,
            "The binary of the app for open, ahead of the one next to sway-things and the one in \
             PATH.",
        ),
        (
            LOCALEDIR_VAR,
            "The directory of the translations, ahead of the one set at build time.",
        ),
        (
            "LANGUAGE, LC_ALL, LC_MESSAGES, LANG",
            "The language of the messages, as gettext reads them.",
        ),
        (
            "NO_COLOR, CLICOLOR, CLICOLOR_FORCE",
            "Whether --color auto colors the output.",
        ),
    ];
    let mut roff = Roff::new();
    roff.control("SH", ["ENVIRONMENT"]);
    for (name, meaning) in variables {
        roff.control("TP", []);
        roff.text([bold(name)]);
        roff.text([roman(meaning)]);
    }
    roff
}

/// The section FILES.
fn files_section() -> Roff {
    let files = [
        (
            "~/.config/sway-things/settings.toml",
            "The settings shared by all machines, e.g. in your dotfiles.",
        ),
        (
            "~/.config/sway-things/settings.d/<hostname>.toml",
            "The settings of this machine, ahead of the shared ones.",
        ),
        (
            "~/.config/sway/sway-things/*.conf",
            "The generated sway config files, which your sway config includes. Sway Things \
             never writes your own config.",
        ),
        (
            "~/.local/state/sway-things/",
            "The list of generated files and the history of the settings files.",
        ),
    ];
    let mut roff = Roff::new();
    roff.control("SH", ["FILES"]);
    for (path, meaning) in files {
        roff.control("TP", []);
        roff.text([italic(path)]);
        roff.text([roman(meaning)]);
    }
    roff
}

/// The section SEE ALSO: the pages of `related`, then those of sway.
fn see_also(related: &[String]) -> Roff {
    let mut pages: Vec<(&str, &str)> = related.iter().map(|name| (name.as_str(), "1")).collect();
    pages.extend([("sway", "1"), ("sway", "5")]);
    let mut text = Vec::new();
    for (index, (name, section)) in pages.iter().enumerate() {
        if index > 0 {
            text.push(roman(", "));
        }
        text.push(bold(*name));
        text.push(roman(format!("({section})")));
    }
    let mut roff = Roff::new();
    roff.control("SH", ["SEE ALSO"]);
    roff.text(text);
    roff
}
