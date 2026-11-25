// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `brightness`, `night-light` and `hook night-light`: the Brightness & Night Light page from the
//! command line, through [`sway_things_core::brightness`] and [`sway_things_core::night_light`].

use std::io::Write;

use clap::Subcommand;
use sway_things_core::brightness::{self, Device, Logind};
use sway_things_core::generate::Validate;
use sway_things_core::i18n::{Text, trf};
use sway_things_core::msg;
use sway_things_core::night_light::{self, ENABLED, Synced};

use super::change::{Change, set};
use super::{Command, Exit, Io, Prompt, Session, Switch, fail_with};

/// A command of `sway-things brightness`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum BrightnessAction {
    /// Print the brightness in percent
    Get,
    /// Set the brightness, in percent: 50% or 50
    Set {
        /// The brightness, from 0 to 100 (a backlight stays at 1 % at least)
        value: String,
    },
    /// Make it brighter by 5 %
    Up,
    /// Make it darker by 5 %, not below 1 % for a backlight
    Down,
}

/// The percent in `text`: `50%` or `50`, from 0 to 100.
fn parse_percent(text: &str) -> Option<u32> {
    let digits = text.trim().strip_suffix('%').unwrap_or(text.trim());
    digits.parse::<u32>().ok().filter(|percent| *percent <= 100)
}

/// The device `name` names, or the screen's backlight.
fn device_of(name: Option<&str>) -> Result<Device, Text> {
    match name {
        Some(name) => brightness::device(name)
            .ok_or_else(|| Text::new(msg!("There is no backlight named {name}")).arg("name", name)),
        None => brightness::screen().ok_or_else(|| {
            Text::new(msg!(
                "This computer has no backlight that can be set (/sys/class/backlight is empty)"
            ))
        }),
    }
}

/// `brightness`: prints or changes the brightness of the backlight `device` (by its name in
/// sysfs, a keyboard light too), else of the screen's.
pub(super) async fn brightness(
    action: &BrightnessAction,
    device: Option<&str>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    let device = match device_of(device) {
        Ok(device) => device,
        Err(text) => return fail_with(err, &text),
    };
    let raw = match action {
        BrightnessAction::Get => {
            let _ = writeln!(out, "{}%", device.percent());
            return Exit::Ok;
        }
        BrightnessAction::Set { value } => {
            let Some(percent) = parse_percent(value) else {
                let text = Text::new(msg!(
                    "{value} isn't a brightness: write a percent from 0 to 100, such as 50%"
                ))
                .arg("value", value);
                let _ = fail_with(err, &text);
                return Exit::Usage;
            };
            device.raw_for(percent)
        }
        BrightnessAction::Up => device.stepped(true),
        BrightnessAction::Down => device.stepped(false),
    };
    let logind = match Logind::connect().await {
        Ok(logind) => logind,
        Err(error) => return fail_with(err, &error.text()),
    };
    match logind.set(&device, raw).await {
        Ok(()) => Exit::Ok,
        Err(error) => fail_with(err, &error.text()),
    }
}

/// `brightness` or `night-light` in `session`.
pub(super) async fn run<V: Validate>(
    command: &Command,
    session: &mut Session<V>,
    prompt: &mut impl Prompt,
    io: &mut Io<'_>,
) -> Exit {
    match command {
        Command::Brightness { action, device } => {
            brightness(action, device.as_deref(), io.out, io.err).await
        }
        Command::NightLight { switch } => night_light(*switch, session, prompt, io).await,
        _ => Exit::Ok,
    }
}

/// `night-light on|off|toggle`: stores the night light's switch as `set night_light.enabled`
/// does; [`super::run_for_user`] then has the tool follow it.
pub(super) async fn night_light<V: Validate>(
    switch: Switch,
    session: &mut Session<V>,
    prompt: &mut impl Prompt,
    io: &mut Io<'_>,
) -> Exit {
    let on = match switch {
        Switch::On => true,
        Switch::Off => false,
        Switch::Toggle => !night_light::Settings::of(session.applier.state()).on,
    };
    let text = on.to_string();
    let change = Change {
        key: ENABLED,
        device: None,
        text: &text,
        layer: None,
        yes: false,
    };
    set(session, &change, false, prompt, io).await
}

/// Whether the user's config of `session` starts a night light itself.
pub(super) fn starts_its_own<V>(session: &Session<V>) -> bool {
    !night_light::own_lines(session.status.config()).is_empty()
}

/// Has the night light's tool follow the settings of `session` ([`night_light::sync`]) and says
/// on `err` why it runs none. [`Exit::Failure`] only if it couldn't look.
pub(super) fn follow_night_light<V: Validate>(session: &Session<V>, err: &mut dyn Write) -> Exit {
    let external = starts_its_own(session);
    match night_light::sync(session.applier.state(), external) {
        Ok(synced) => {
            report(&synced, external, err);
            Exit::Ok
        }
        Err(error) => fail_with(err, &error.text()),
    }
}

/// Says on `err` why the night light runs none, if it doesn't.
fn report(synced: &Synced, external: bool, err: &mut dyn Write) {
    if let Some(foreign) = &synced.foreign {
        let _ = writeln!(
            err,
            "{}",
            trf(
                msg!(
                    "A night light of your own runs already ({program}, process {pid}), so Sway \
                     Things starts none"
                ),
                &[("program", &foreign.program), ("pid", &foreign.pid)],
            )
        );
    } else if external {
        let _ = writeln!(
            err,
            "{}",
            sway_things_core::i18n::tr(msg!(
                "Your sway config starts a night light itself, so Sway Things starts none"
            ))
        );
    }
    if let Some(problem) = &synced.problem {
        let _ = writeln!(err, "{}", super::error_line(&problem.text().translated()));
    }
}

/// `hook night-light`: starts the tool as the settings say, or ends it.
pub(super) fn hook_night_light<V: Validate>(session: &Session<V>, io: &mut Io<'_>) -> Exit {
    follow_night_light(session, io.err)
}

#[cfg(test)]
mod tests {
    use super::parse_percent;

    #[test]
    fn a_percent_has_an_optional_sign() {
        assert_eq!(parse_percent("50%"), Some(50));
        assert_eq!(parse_percent("50"), Some(50));
        assert_eq!(parse_percent(" 0 "), Some(0));
        assert_eq!(parse_percent("100%"), Some(100));
        assert_eq!(parse_percent("101"), None);
        assert_eq!(parse_percent("-5"), None);
        assert_eq!(parse_percent("half"), None);
    }
}
