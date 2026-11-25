// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings that are environment variables, and the Environment page's own settings.
//!
//! A setting of any page can write variables into Sway Things' environment.d file
//! ([`crate::environment`]): it declares the names it owns and a function that makes its
//! variables from its value ([`SettingBuilder::environment`](super::SettingBuilder::environment)).
//! The file collects them from the registry in its order, so a page adds variables without
//! knowing the file.
//!
//! The Environment page has the switches that make older apps use Wayland, each one fixed
//! variable with a value that falls back to X11 where an app has no Wayland support.

use super::{Apply, Msg, Page, Setting, Value};
use crate::msg;

/// The key of the switch for Qt 5 apps.
pub const QT_WAYLAND: &str = "environment.qt_wayland";
/// The key of the switch for SDL 2 games.
pub const SDL_WAYLAND: &str = "environment.sdl_wayland";
/// The key of the switch for Java apps.
pub const JAVA_NONREPARENTING: &str = "environment.java_nonreparenting";
/// The key of the switch for Electron 28 to 37 apps.
pub const ELECTRON_WAYLAND: &str = "environment.electron_wayland";

/// An environment variable a setting writes into Sway Things' environment.d file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    /// Its name: letters, digits and `_`, not starting with a digit.
    pub name: String,
    /// Its value.
    pub value: String,
    /// Whether `value` is written as it is, in environment.d's syntax (`$NAME` expanded, quotes
    /// stripped), which the setting has checked; otherwise it is the value itself, quoted where
    /// needed so it reaches programs unchanged.
    pub raw: bool,
}

impl Variable {
    /// The variable `name` with exactly `value`.
    pub fn literal(name: &str, value: &str) -> Self {
        Self {
            name: name.to_owned(),
            value: value.to_owned(),
            raw: false,
        }
    }
}

/// Makes a setting's variables from its value: none for a value that sets nothing.
pub type EnvironmentFn = fn(&Value) -> Vec<Variable>;

/// How a setting maps to environment variables.
#[derive(Debug, Clone, Copy)]
pub struct EnvironmentMapping {
    pub(super) names: &'static [&'static str],
    pub(super) write: EnvironmentFn,
}

impl EnvironmentMapping {
    /// The names the setting owns: no other setting writes them, and the variable editor refuses
    /// them. A name ending in `*` stands for every name it starts (`LC_*`).
    pub fn names(&self) -> &'static [&'static str] {
        self.names
    }

    /// Whether the setting owns the variable `name`.
    pub fn owns(&self, name: &str) -> bool {
        self.names
            .iter()
            .any(|owned| match owned.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => *owned == name,
            })
    }

    /// The variables for `value`.
    pub fn variables(&self, value: &Value) -> Vec<Variable> {
        (self.write)(value)
    }
}

/// Whether `name` can be the name of an environment variable in an environment.d file: ASCII
/// letters, digits and `_`, not starting with a digit.
pub fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|char| char.is_ascii_alphanumeric() || char == '_')
}

/// The variable `name` with `value` while the switch is on.
fn when_on(value: &Value, name: &str, set: &str) -> Vec<Variable> {
    if matches!(value, Value::Bool(true)) {
        vec![Variable::literal(name, set)]
    } else {
        Vec::new()
    }
}

fn qt(value: &Value) -> Vec<Variable> {
    when_on(value, "QT_QPA_PLATFORM", "wayland;xcb")
}

fn sdl(value: &Value) -> Vec<Variable> {
    when_on(value, "SDL_VIDEODRIVER", "wayland,x11")
}

fn java(value: &Value) -> Vec<Variable> {
    when_on(value, "_JAVA_AWT_WM_NONREPARENTING", "1")
}

fn electron(value: &Value) -> Vec<Variable> {
    when_on(value, "ELECTRON_OZONE_PLATFORM_HINT", "auto")
}

/// The section of the switches.
const WAYLAND: Msg = msg!("Wayland for older apps");

pub(super) fn settings() -> Vec<Setting> {
    vec![
        Setting::toggle(QT_WAYLAND, Page::Environment, false)
            .section(WAYLAND)
            .title(msg!("Qt 5 apps"))
            .subtitle(msg!(
                "QT_QPA_PLATFORM=wayland;xcb: Qt 6 finds Wayland by itself, Qt 5 only when the \
                 session type is wayland"
            ))
            .apply(Apply::Relogin)
            .environment(&["QT_QPA_PLATFORM"], qt)
            .keywords(&[
                msg!("QT_QPA_PLATFORM"),
                msg!("Qt"),
                msg!("KDE apps"),
                msg!("Xwayland"),
            ])
            .build(),
        Setting::toggle(SDL_WAYLAND, Page::Environment, false)
            .section(WAYLAND)
            .title(msg!("SDL 2 games"))
            .subtitle(msg!(
                "SDL_VIDEODRIVER=wayland,x11: SDL 3 and sdl2-compat find Wayland by themselves, \
                 games with an older SDL 2 don't"
            ))
            .apply(Apply::Relogin)
            .environment(&["SDL_VIDEODRIVER"], sdl)
            .keywords(&[
                msg!("SDL_VIDEODRIVER"),
                msg!("SDL"),
                msg!("games"),
                msg!("Xwayland"),
            ])
            .build(),
        Setting::toggle(JAVA_NONREPARENTING, Page::Environment, false)
            .section(WAYLAND)
            .title(msg!("Java apps"))
            .subtitle(msg!(
                "_JAVA_AWT_WM_NONREPARENTING=1: Java apps on Xwayland otherwise show grey or \
                 empty windows"
            ))
            .apply(Apply::Relogin)
            .environment(&["_JAVA_AWT_WM_NONREPARENTING"], java)
            .keywords(&[
                msg!("_JAVA_AWT_WM_NONREPARENTING"),
                msg!("Java"),
                msg!("Swing"),
                msg!("AWT"),
                msg!("JetBrains"),
                msg!("grey window"),
            ])
            .build(),
        Setting::toggle(ELECTRON_WAYLAND, Page::Environment, false)
            .section(WAYLAND)
            .title(msg!("Electron 28 to 37 apps"))
            .subtitle(msg!(
                "ELECTRON_OZONE_PLATFORM_HINT=auto: Electron 38 and newer and Chromium 140 and \
                 newer use Wayland by themselves"
            ))
            .apply(Apply::Relogin)
            .environment(&["ELECTRON_OZONE_PLATFORM_HINT"], electron)
            .keywords(&[
                msg!("ELECTRON_OZONE_PLATFORM_HINT"),
                msg!("Electron"),
                msg!("Chromium"),
                msg!("ozone"),
                msg!("Xwayland"),
            ])
            .build(),
    ]
}
