// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Region & Time page's services: the system clock through systemd-timedated
//! ([`Timedate`]), the time zones with their names and offsets from the system's tzdata
//! ([`zones`]), and the automatic time zone from the location geoclue finds ([`geo`]).
//!
//! Nothing here is stored by Sway Things: timedated owns the time zone, automatic time (NTP) and
//! the clock. Every change is a D-Bus call that polkit guards (`auth_admin_keep` by default): the
//! user's polkit agent asks for an administrator's password, which Sway Things never sees. When
//! polkit refuses, [`Error::command`] gives the `timedatectl` command that does the same in a
//! terminal, where polkit asks through `pkttyagent`.

pub mod geo;
mod timedate;
pub mod zones;

#[cfg(any(test, feature = "test-support"))]
#[allow(
    missing_docs,
    reason = "zbus's interface macro adds an undocumented method for geoclue's signal"
)]
pub mod stand_in;

use std::fmt;

use crate::i18n::Text;
use crate::msg;

pub use timedate::{Bus, Clock, Timedate};

/// A change of the system clock, for the command that does the same in a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The time zone, by its IANA name (`Europe/Berlin`).
    TimeZone(String),
    /// Automatic time (NTP) on or off.
    Ntp(bool),
    /// The clock, as local date and time in the system's time zone (`2026-11-24 21:35:00`).
    Time(String),
}

impl Change {
    /// The `timedatectl` command that makes the change, with its arguments, each as it is.
    pub fn args(&self) -> Vec<String> {
        let (verb, value) = match self {
            Self::TimeZone(zone) => ("set-timezone", zone.clone()),
            Self::Ntp(on) => ("set-ntp", on.to_string()),
            Self::Time(time) => ("set-time", time.clone()),
        };
        vec!["timedatectl".to_owned(), verb.to_owned(), value]
    }

    /// The command to copy into a terminal: an argument with characters a shell treats
    /// specially is put in single quotes. It is only ever shown, never run.
    pub fn command(&self) -> String {
        self.args()
            .iter()
            .map(|arg| quote(arg))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// `arg` as a shell reads it back: in single quotes unless it only has characters that are safe
/// bare.
fn quote(arg: &str) -> String {
    let safe = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:+=@,".contains(c));
    if safe {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

/// Why timedated can't be read or didn't make a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No system bus, or timedated isn't there (a system without systemd). The text says which.
    Unavailable(Text),
    /// polkit didn't allow the change: the password was wrong or the dialog was canceled, or no
    /// polkit agent runs to ask. The text is timedated's, so untrusted.
    Denied(String),
    /// timedated refused or failed: its message (untrusted text, as data), or what Sway Things
    /// makes of it.
    Failed(Text),
}

impl Error {
    /// What went wrong, to show.
    pub fn text(&self) -> Text {
        match self {
            Self::Unavailable(reason) => {
                Text::new(msg!("The system clock can't be changed: {reason}"))
                    .text("reason", reason.clone())
            }
            Self::Denied(reason) => Text::new(msg!(
                "Not allowed: {reason}. If no password dialog showed up, no polkit agent is \
                 running: start one with sway, such as lxqt-policykit-agent or polkit-gnome, or \
                 run the command below in a terminal."
            ))
            .arg("reason", reason),
            Self::Failed(reason) => reason.clone(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text().english())
    }
}

impl std::error::Error for Error {}

/// `text` with control characters replaced by spaces and trimmed: messages and names come from
/// other programs and are only ever shown as plain text.
pub(crate) fn clean(text: &str) -> String {
    crate::dbus::clean(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_quote_what_a_shell_would_split() {
        assert_eq!(
            Change::TimeZone("Europe/Berlin".to_owned()).command(),
            "timedatectl set-timezone Europe/Berlin"
        );
        assert_eq!(Change::Ntp(false).command(), "timedatectl set-ntp false");
        assert_eq!(
            Change::Time("2026-11-24 21:35:00".to_owned()).command(),
            "timedatectl set-time '2026-11-24 21:35:00'"
        );
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote(""), "''");
        assert_eq!(
            Change::Time("x y".to_owned()).args(),
            ["timedatectl", "set-time", "x y"]
        );
    }
}
