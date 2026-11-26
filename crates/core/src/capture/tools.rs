// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The programs the Screenshots & Clipboard page needs, and which of the alternatives are
//! installed: a launcher to pick from the clipboard history, an editor for screenshots, a
//! recorder.

use std::path::PathBuf;

use super::CaptureError;
use crate::delegate::find_program;

/// A program a part of the page needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Program {
    /// The name of its executable, looked up in `PATH`.
    pub name: &'static str,
    /// The package that has it, by the name the distributions give it.
    pub package: &'static str,
}

impl Program {
    /// Where it is installed, if it is.
    pub fn find(self) -> Option<PathBuf> {
        find_program(self.name, std::env::var_os("PATH").as_deref())
    }

    /// Whether it is installed.
    pub fn installed(self) -> bool {
        self.find().is_some()
    }

    /// Where it is installed.
    ///
    /// # Errors
    ///
    /// [`CaptureError::Missing`] if it isn't.
    pub fn require(self) -> Result<PathBuf, CaptureError> {
        self.find().ok_or(CaptureError::Missing {
            program: self.name,
            package: self.package,
        })
    }
}

/// grim, which takes the screenshots.
pub const GRIM: Program = Program {
    name: "grim",
    package: "grim",
};
/// slurp, which lets the user select a region.
pub const SLURP: Program = Program {
    name: "slurp",
    package: "slurp",
};
/// wl-copy, which puts a screenshot or a history entry on the clipboard.
pub const WL_COPY: Program = Program {
    name: "wl-copy",
    package: "wl-clipboard",
};
/// wl-paste, whose `--watch` hands every copy to the history.
pub const WL_PASTE: Program = Program {
    name: "wl-paste",
    package: "wl-clipboard",
};
/// cliphist, which keeps the clipboard history, with text and images.
pub const CLIPHIST: Program = Program {
    name: "cliphist",
    package: "cliphist",
};
/// swappy, an editor for screenshots.
pub const SWAPPY: Program = Program {
    name: "swappy",
    package: "swappy",
};
/// satty, an editor for screenshots.
pub const SATTY: Program = Program {
    name: "satty",
    package: "satty",
};
/// wf-recorder, which records the screen.
pub const WF_RECORDER: Program = Program {
    name: "wf-recorder",
    package: "wf-recorder",
};
/// wl-screenrec, which records the screen with the graphics card's encoder.
pub const WL_SCREENREC: Program = Program {
    name: "wl-screenrec",
    package: "wl-screenrec",
};

/// Every program of the page, in the order the system details list them.
pub const ALL: [Program; 9] = [
    GRIM,
    SLURP,
    WL_COPY,
    CLIPHIST,
    SWAPPY,
    SATTY,
    WF_RECORDER,
    WL_SCREENREC,
    WL_PASTE,
];

/// What the screenshot keys need: grim and slurp, and wl-copy to copy.
pub const SCREENSHOTS: [Program; 3] = [GRIM, SLURP, WL_COPY];
/// What the clipboard history needs.
pub const HISTORY: [Program; 3] = [WL_PASTE, CLIPHIST, WL_COPY];
/// What a recording of a region needs besides a recorder.
pub const RECORDING: [Program; 1] = [SLURP];

/// The programs of `programs` that aren't installed, each once.
pub fn missing(programs: &[Program]) -> Vec<Program> {
    let mut missing: Vec<Program> = Vec::new();
    for program in programs {
        if !program.installed() && !missing.iter().any(|seen| seen.name == program.name) {
            missing.push(*program);
        }
    }
    missing
}

/// The packages that have `programs`, each once, in order.
pub fn packages(programs: &[Program]) -> Vec<&'static str> {
    let mut packages: Vec<&'static str> = Vec::new();
    for program in programs {
        if !packages.contains(&program.package) {
            packages.push(program.package);
        }
    }
    packages
}

/// A launcher that shows a list read from its standard input and prints the line picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Launcher {
    /// The program.
    pub program: Program,
    /// Its arguments for a list from standard input (its dmenu mode).
    pub args: &'static [&'static str],
}

/// The launchers that can show the clipboard history, in the order they are preferred; bemenu,
/// the plainest, last.
pub const LAUNCHERS: [Launcher; 5] = [
    Launcher {
        program: Program {
            name: "fuzzel",
            package: "fuzzel",
        },
        args: &["--dmenu"],
    },
    Launcher {
        program: Program {
            name: "wofi",
            package: "wofi",
        },
        args: &["--dmenu"],
    },
    Launcher {
        program: Program {
            name: "rofi",
            package: "rofi",
        },
        args: &["-dmenu"],
    },
    Launcher {
        program: Program {
            name: "tofi",
            package: "tofi",
        },
        args: &[],
    },
    Launcher {
        program: Program {
            name: "bemenu",
            package: "bemenu",
        },
        args: &[],
    },
];

/// The first launcher of [`LAUNCHERS`] that is installed.
pub fn launcher() -> Option<Launcher> {
    LAUNCHERS
        .into_iter()
        .find(|launcher| launcher.program.installed())
}

/// The editor for screenshots that is installed: swappy, else satty.
pub fn editor() -> Option<Program> {
    [SWAPPY, SATTY]
        .into_iter()
        .find(|program| program.installed())
}

/// The recorder that is installed: wf-recorder, which works with any graphics card, else
/// wl-screenrec.
pub fn recorder() -> Option<Program> {
    [WF_RECORDER, WL_SCREENREC]
        .into_iter()
        .find(|program| program.installed())
}

/// Runs `program` with `args` (never through a shell), with `input` on its standard input, and
/// returns what it printed on its standard output. Standard error is kept for the error.
///
/// # Blocking
///
/// Waits for the program to end.
///
/// # Errors
///
/// [`CaptureError::Missing`] if it isn't installed, [`CaptureError::Failed`] if it can't be
/// started or ends with a failure, with the last line it printed on standard error.
pub fn run(
    program: Program,
    args: &[&std::ffi::OsStr],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, CaptureError> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let path = program.require()?;
    let failed = |message: String| CaptureError::Failed {
        program: program.name.to_owned(),
        message,
    };
    let mut child = Command::new(path)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| failed(error.to_string()))?;
    // Written on a thread of its own, so a program that prints while it reads can't block on a
    // full pipe while this one waits for it to read.
    let output = std::thread::scope(|scope| {
        if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
            scope.spawn(move || {
                // A program that stops reading early (a launcher that was closed) is no error.
                let _ = stdin.write_all(input);
            });
        }
        child.wait_with_output()
    })
    .map_err(|error| failed(error.to_string()))?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    Err(failed(last_line(&output.stderr)))
}

/// The last line of `text` that isn't blank, without control characters.
pub fn last_line(text: &[u8]) -> String {
    String::from_utf8_lossy(text)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

/// The parts of the page switched on in `state` whose programs aren't all installed, as
/// `installed` tells: the key of the switch and what is missing. A part that needs one of
/// several (a launcher, a recorder) misses the first of them.
pub fn missing_for(
    state: &crate::state::State,
    installed: impl Fn(Program) -> bool,
) -> Vec<(&'static str, Vec<Program>)> {
    let missing = |programs: &[Program]| -> Vec<Program> {
        programs
            .iter()
            .filter(|program| !installed(**program))
            .copied()
            .collect()
    };
    let one_of = |programs: &[Program]| -> Vec<Program> {
        if programs.iter().any(|program| installed(*program)) {
            Vec::new()
        } else {
            programs.first().copied().into_iter().collect()
        }
    };
    let launchers: Vec<Program> = LAUNCHERS.iter().map(|launcher| launcher.program).collect();
    let parts = [
        (super::SCREENSHOTS, missing(&SCREENSHOTS)),
        (
            super::HISTORY,
            [missing(&HISTORY), one_of(&launchers)].concat(),
        ),
        (
            super::RECORDING,
            [missing(&RECORDING), one_of(&[WF_RECORDER, WL_SCREENREC])].concat(),
        ),
    ];
    parts
        .into_iter()
        .filter(|(switch, missing)| super::is_on(state, switch) && !missing.is_empty())
        .collect()
}
