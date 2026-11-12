// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A spinner with a short message on stderr, while something takes a moment.

use std::borrow::Cow;
use std::fmt;
use std::time::Duration;

use anstyle::{AnsiColor, Style};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

/// The frames of the spinner.
const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The color of the spinner.
const COLOR: Style = AnsiColor::Cyan.on_default();

/// Shows progress on stderr, if it is a terminal; draws nothing otherwise. The line is cleared
/// when [`Spinner::clear`] is called or the spinner is dropped.
pub struct Spinner {
    bar: ProgressBar,
}

impl Spinner {
    /// A spinner that draws nothing.
    pub fn hidden() -> Self {
        Self {
            bar: ProgressBar::hidden(),
        }
    }

    /// A spinner on stderr, in color if `color`. Hidden if stderr isn't a terminal.
    pub fn stderr(color: bool) -> Self {
        let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
        if bar.is_hidden() {
            return Self { bar };
        }
        let frames: Vec<String> = FRAMES
            .iter()
            .map(|frame| {
                if color {
                    format!("{}{frame}{}", COLOR.render(), COLOR.render_reset())
                } else {
                    (*frame).to_owned()
                }
            })
            // The last tick string is shown when the bar is finished; it is cleared then anyway.
            .chain(std::iter::once(" ".to_owned()))
            .collect();
        let frames: Vec<&str> = frames.iter().map(String::as_str).collect();
        // The template is fixed and valid; should it ever not be, the default style still spins.
        let style = ProgressStyle::with_template("{spinner} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_strings(&frames);
        bar.set_style(style);
        bar.enable_steady_tick(Duration::from_millis(80));
        Self { bar }
    }

    /// Shows `message` next to the spinner, e.g. `Reading your sway config…`.
    pub fn say(&self, message: impl Into<Cow<'static, str>>) {
        self.bar.set_message(message);
    }

    /// Removes the spinner for good, so the output that follows starts on a clean line.
    pub fn clear(&self) {
        self.bar.finish_and_clear();
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::hidden()
    }
}

impl fmt::Debug for Spinner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Spinner")
            .field("hidden", &self.bar.is_hidden())
            .finish()
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}
