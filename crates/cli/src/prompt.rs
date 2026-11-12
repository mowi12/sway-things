// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Asking whether to keep a risky change, with the time left until it is reverted.

use std::future::Future;
use std::io::{self, BufRead, IsTerminal, Write};
use std::thread;
use std::time::Duration;

use sway_things_core::apply::Countdown;

/// Asks the user whether to keep a risky change.
pub trait Prompt {
    /// Whether there is someone to ask. Without, a risky change needs `--yes`.
    fn is_interactive(&self) -> bool;

    /// Asks whether to keep the change, until `countdown` elapses. `true` only if the user says
    /// yes in time.
    fn keep(&mut self, countdown: Countdown) -> impl Future<Output = bool>;
}

/// Asks in the terminal: `Keep? [y/N]` on stderr with the seconds left, the answer from stdin.
#[derive(Debug, Clone, Copy, Default)]
pub struct Terminal;

impl Prompt for Terminal {
    fn is_interactive(&self) -> bool {
        io::stdin().is_terminal() && io::stderr().is_terminal()
    }

    async fn keep(&mut self, countdown: Countdown) -> bool {
        // Reading stdin blocks, so a thread reads the answer while the countdown runs. If the
        // countdown wins, the thread stays blocked until the process ends.
        let (sender, answers) = async_channel::bounded(1);
        thread::spawn(move || {
            let mut line = String::new();
            let answer = match io::stdin().lock().read_line(&mut line) {
                Ok(0) | Err(_) => None,
                Ok(_) => Some(line),
            };
            let _ = sender.send_blocking(answer);
        });
        let mut stderr = io::stderr();
        loop {
            let left = countdown.remaining();
            if left.is_zero() {
                let _ = writeln!(stderr);
                return false;
            }
            let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
            let _ = write!(stderr, "\rKeep? [y/N] {seconds:>2} s ");
            let _ = stderr.flush();
            // Wake up when the next whole second is reached, to show it.
            let fraction = Duration::from_nanos(left.subsec_nanos().into());
            let tick = if fraction.is_zero() {
                Duration::from_secs(1)
            } else {
                fraction
            };
            let answer = futures_lite::future::or(async { Some(answers.recv().await) }, async {
                async_io::Timer::after(tick).await;
                None
            })
            .await;
            match answer {
                Some(Ok(Some(line))) => return is_yes(&line),
                // End of input: no.
                Some(_) => return false,
                None => {}
            }
        }
    }
}

fn is_yes(line: &str) -> bool {
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::is_yes;

    #[test]
    fn only_yes_keeps_the_change() {
        assert!(is_yes("y\n"));
        assert!(is_yes(" YES \n"));
        assert!(!is_yes("\n"));
        assert!(!is_yes("n\n"));
        assert!(!is_yes("yep\n"));
    }
}
