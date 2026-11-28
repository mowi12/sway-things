// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Why a change wasn't applied or persisted.

use std::fmt;
use std::io;

use crate::command::CommandError;
use crate::devices::Lockout;
use crate::generate::WriteError;
use crate::i18n::Text;
use crate::ipc::{IpcError, Unmet};
use crate::msg;
use crate::paths::PathsError;
use crate::registry::{ConfigFile, RenderError};
use crate::state::StateError;

/// Why a change wasn't applied or persisted.
#[derive(Debug)]
pub enum ApplyError {
    /// The directories of the settings files or the generated files can't be found.
    Paths(PathsError),
    /// The key, the device or the value doesn't fit the registry, or a settings file can't be
    /// changed or saved.
    State(StateError),
    /// The setting can lock the user out (a keyboard layout, a disabled device), so it only
    /// changes through the safety net.
    Risky {
        /// The key.
        key: &'static str,
    },
    /// The compositor doesn't support the setting, or this value of it.
    Unsupported {
        /// The key.
        key: &'static str,
        /// What the setting needs.
        unmet: Unmet,
    },
    /// The value can't be turned into sway commands.
    Render(RenderError),
    /// A command can't be sent over IPC.
    Command(CommandError),
    /// sway rejected a command or can't be reached.
    Ipc(IpcError),
    /// The generated files can't be written.
    Write(WriteError),
    /// There is no session, and the installed `sway` doesn't say what it is either: it can't be
    /// run (with the error), or it prints no version.
    NoCompositor(Option<io::Error>),
    /// The setting has no safety net: it changes with [`Applier::set`].
    NotRisky {
        /// The key.
        key: &'static str,
    },
    /// A risky change waits to be kept or reverted, and another one can't be tried before.
    PendingChange {
        /// The key of the change that waits.
        key: &'static str,
    },
    /// kanshi sets up the displays, which remembers a display's setting for the displays
    /// connected, and there is no session to say which those are.
    DisplaysNeedSession {
        /// The key.
        key: &'static str,
    },
    /// A display can't show another one that is shown elsewhere or shows another itself.
    Mirror,
    /// No risky change waits to be kept or reverted.
    NothingPending,
    /// A risky change can only be tried in a running session.
    NoSession {
        /// The key.
        key: &'static str,
    },
    /// The change would leave the user without a keyboard or a pointing device.
    Lockout {
        /// The key.
        key: &'static str,
        /// What would be left out.
        lockout: Lockout,
    },
    /// sway can only be asked to reload in a running session.
    ReloadWithoutSession,
    /// These generated files couldn't be written, so a reload of sway wouldn't read the changes
    /// that wait for it.
    Unwritten {
        /// The files.
        files: Vec<ConfigFile>,
    },
    /// The module definitions of the bar (`bar.jsonc`) can't be read or written.
    BarModules(crate::bar::ModulesError),
}

impl From<StateError> for ApplyError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

impl From<RenderError> for ApplyError {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

impl From<WriteError> for ApplyError {
    fn from(error: WriteError) -> Self {
        Self::Write(error)
    }
}

impl ApplyError {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Paths(error) => error.text(),
            Self::State(error) => error.text(),
            Self::Risky { key } => {
                Text::new(msg!("{key} can only be changed with the safety net")).arg("key", key)
            }
            Self::Unsupported { key, unmet } => Text::new(msg!("{key} {requirement}"))
                .arg("key", key)
                .text("requirement", unmet.text()),
            Self::Render(error) => error.text(),
            Self::Command(error) => error.text(),
            Self::Ipc(error) => error.text(),
            Self::Write(error) => error.text(),
            Self::NoCompositor(Some(error)) => Text::new(msg!(
                "not in a sway session, and sway can't be run: {error}"
            ))
            .arg("error", error),
            Self::NoCompositor(None) => {
                msg!("not in a sway session, and `sway --version` prints no version").into()
            }
            Self::NotRisky { key } => {
                Text::new(msg!("{key} has no safety net; change it directly")).arg("key", key)
            }
            Self::PendingChange { key } => {
                Text::new(msg!("keep or revert the change of {key} first")).arg("key", key)
            }
            Self::Mirror => msg!(
                "a display can't show one that shows another display itself, or be shown \
                 elsewhere while it shows another"
            )
            .into(),
            Self::DisplaysNeedSession { key } => Text::new(msg!(
                "{key} wasn't changed: kanshi sets up the displays and remembers it for the \
                 displays connected, so it changes only in a running sway session"
            ))
            .arg("key", key),
            Self::NothingPending => msg!("no change waits to be kept or reverted").into(),
            Self::NoSession { key } => Text::new(msg!(
                "{key} can lock you out, so it is only tried in a running sway session, where it \
                 can be reverted"
            ))
            .arg("key", key),
            Self::Lockout { key, lockout } => Text::new(msg!("{key} wasn't changed: {reason}"))
                .arg("key", key)
                .text("reason", lockout.text()),
            Self::ReloadWithoutSession => {
                msg!("sway isn't running, so it can't reload its config").into()
            }
            Self::BarModules(error) => error.text(),
            Self::Unwritten { files } => {
                let names: Vec<&str> = files.iter().map(|file| file.file_name()).collect();
                Text::new(msg!(
                    "{files} couldn't be written, so sway wouldn't read the changes"
                ))
                .arg("files", names.join(", "))
            }
        }
    }
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for ApplyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Paths(error) => Some(error),
            Self::State(error) => Some(error),
            Self::Unsupported { unmet, .. } => Some(unmet),
            Self::Render(error) => Some(error),
            Self::Command(error) => Some(error),
            Self::Ipc(error) => Some(error),
            Self::Write(error) => Some(error),
            Self::BarModules(error) => Some(error),
            Self::NoCompositor(error) => error
                .as_ref()
                .map(|error| error as &(dyn std::error::Error + 'static)),
            Self::Risky { .. }
            | Self::NotRisky { .. }
            | Self::PendingChange { .. }
            | Self::NothingPending
            | Self::Mirror
            | Self::NoSession { .. }
            | Self::DisplaysNeedSession { .. }
            | Self::Lockout { .. }
            | Self::ReloadWithoutSession
            | Self::Unwritten { .. } => None,
        }
    }
}
