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
//! variable with a value that falls back to X11 where an app has no Wayland support, and the
//! user's own variables ([`VARIABLES`]), in environment.d's syntax, guarded so a typo can't
//! break the next login.

use super::read::ReadError;
use super::sway::{ConfigFile, Context, Directive, RenderError};
use super::{Apply, Choice, Msg, Page, Registry, Setting, Value, ValueError, When};
use crate::command::SwayCmd;
use crate::environment::syntax::{self, SyntaxError};
use crate::i18n::Text;
use crate::msg;

/// The key of the switch for Qt 5 apps.
pub const QT_WAYLAND: &str = "environment.qt_wayland";
/// The key of the switch for SDL 2 games.
pub const SDL_WAYLAND: &str = "environment.sdl_wayland";
/// The key of the switch for Java apps.
pub const JAVA_NONREPARENTING: &str = "environment.java_nonreparenting";
/// The key of the switch for Electron 28 to 37 apps.
pub const ELECTRON_WAYLAND: &str = "environment.electron_wayland";
/// The key of the user's own variables: a list of `NAME=value`, the value in environment.d's
/// syntax.
pub const VARIABLES: &str = "environment.variables";
/// The key of Sway Things' own portal configuration
/// ([`crate::environment::portals`]): on, it is written where the user has none.
pub const PORTAL_CONFIG: &str = "environment.portal_config";
/// The key of the switch that hands the session's variables to systemd and D-Bus at login.
pub const PASS_SESSION_VARIABLES: &str = "environment.pass_session_variables";
/// The key of how xdg-desktop-portal-wlr asks for the display to share
/// ([`crate::environment::screencast`]).
pub const SCREENCAST_CHOOSER: &str = "environment.screencast_chooser";
/// The key of the display shared while [`SCREENCAST_CHOOSER`] is `output`.
pub const SCREENCAST_OUTPUT: &str = "environment.screencast_output";

/// The choices of [`SCREENCAST_CHOOSER`].
const CHOOSERS: &[Choice] = &[
    Choice::new("default", msg!("xdg-desktop-portal-wlr\u{2019}s own")),
    Choice::new("slurp", msg!("Ask by clicking a display (slurp)")),
    Choice::new("menu", msg!("Pick from a menu")),
    Choice::new("output", msg!("Always share one display")),
];

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

/// The variables the session itself sets, which no setting may change: a login would break.
const SESSION: [&str; 12] = [
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_*",
    "XDG_CURRENT_DESKTOP",
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "SWAYSOCK",
    "I3SOCK",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// The variables of Region & Time's language and formats, which the editor leaves to that page.
const REGION: [&str; 2] = ["LANG", "LC_*"];

/// Whether `name` is one of `names` (a name ending in `*` stands for every name it starts).
fn listed(names: &[&str], name: &str) -> bool {
    names.iter().any(|listed| match listed.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => *listed == name,
    })
}

/// Why an item of [`VARIABLES`] is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableError {
    /// It isn't `NAME=value`.
    NotAnAssignment,
    /// The name isn't letters, digits and `_`, or starts with a digit.
    NotAName,
    /// A setting on this page owns the name: its switch sets it.
    Switch,
    /// A setting on another page owns the name.
    OtherPage(Page),
    /// The session sets the variable itself.
    Session,
    /// `PATH` keeps neither `$PATH` nor `/usr/bin`.
    PathLost,
    /// The value isn't environment.d's syntax.
    Syntax(SyntaxError),
}

impl VariableError {
    /// What is wrong, to show.
    pub fn text(self) -> Text {
        match self {
            Self::NotAnAssignment => msg!("must be NAME=value").into(),
            Self::NotAName => {
                msg!("a name has letters, digits and _, and doesn't start with a digit").into()
            }
            Self::Switch => msg!("a switch on this page sets it: use the switch").into(),
            Self::OtherPage(page) => {
                Text::new(msg!("set it on {page}: that page owns it")).text("page", page.title())
            }
            Self::Session => {
                msg!("the session sets it itself: changing it would break the login").into()
            }
            Self::PathLost => {
                msg!("PATH must keep $PATH or /usr/bin, or no program would be found at login")
                    .into()
            }
            Self::Syntax(error) => error.text(),
        }
    }
}

/// The name and the value of an item of [`VARIABLES`], `NAME=value`.
pub fn split(item: &str) -> Option<(&str, &str)> {
    item.split_once('=')
}

/// Checks the name `name` for the editor: a valid name that no other setting and not the
/// session owns.
///
/// # Errors
///
/// Why the editor refuses it.
pub fn check_name(name: &str) -> Result<(), VariableError> {
    if !is_variable_name(name) {
        return Err(VariableError::NotAName);
    }
    if listed(&SESSION, name) {
        return Err(VariableError::Session);
    }
    if let Some(owner) = Registry::builtin().environment_owner(name) {
        return Err(if owner.page() == Page::Environment {
            VariableError::Switch
        } else {
            VariableError::OtherPage(owner.page())
        });
    }
    if listed(&REGION, name) {
        return Err(VariableError::OtherPage(Page::RegionTime));
    }
    Ok(())
}

/// Checks an item of [`VARIABLES`]: `NAME=value` with a name [`check_name`] takes and a value in
/// environment.d's syntax ([`syntax::check_value`]); a `PATH` must keep `$PATH` or `/usr/bin`.
///
/// # Errors
///
/// Why it is refused.
pub fn check_variable(item: &str) -> Result<(), VariableError> {
    let (name, value) = split(item).ok_or(VariableError::NotAnAssignment)?;
    check_name(name)?;
    syntax::check_value(value).map_err(VariableError::Syntax)?;
    if name == "PATH" {
        let value = syntax::unquote(value);
        let keeps = syntax::refers_to(&value, "PATH")
            || value
                .split(':')
                .any(|part| part.trim_end_matches('/') == "/usr/bin");
        if !keeps {
            return Err(VariableError::PathLost);
        }
    }
    Ok(())
}

/// The check of the items of [`VARIABLES`].
///
/// # Errors
///
/// [`ValueError::NotAVariable`] with what [`check_variable`] found.
pub fn check_item(item: &str) -> Result<(), ValueError> {
    check_variable(item).map_err(ValueError::NotAVariable)
}

/// What to keep in mind before setting `name`, for the variables that every program depends on
/// and that the editor still takes.
pub fn warning(name: &str) -> Option<Msg> {
    match name {
        "PATH" => Some(msg!(
            "Every program is looked up in PATH: a wrong folder can keep apps from starting"
        )),
        "LD_PRELOAD" | "LD_LIBRARY_PATH" => Some(msg!(
            "Every program loads these libraries: a wrong one can break the next login"
        )),
        "XDG_CONFIG_HOME" | "XDG_DATA_DIRS" | "XDG_CONFIG_DIRS" => Some(msg!(
            "Every app looks for its settings and files there: a wrong folder hides them"
        )),
        _ => None,
    }
}

/// The variables of the editor: the items that pass [`check_variable`], written as they are.
fn edited(value: &Value) -> Vec<Variable> {
    let Value::List(items) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter(|item| check_variable(item).is_ok())
        .filter_map(|item| split(item))
        .map(|(name, value)| Variable {
            name: name.to_owned(),
            value: value.to_owned(),
            raw: true,
        })
        .collect()
}

/// An output name xdg-desktop-portal-wlr can take, or empty for none.
fn check_output(name: &str) -> Result<(), ValueError> {
    if name.is_empty() || crate::environment::screencast::is_output_name(name) {
        Ok(())
    } else {
        Err(ValueError::NotAnOutputName)
    }
}

/// The lines that hand the session's variables to the systemd user manager and D-Bus
/// activation, while the switch is on: run at once, and at each login from the generated file.
fn pass_session_variables(ctx: &Context<'_>) -> Result<Vec<SwayCmd>, RenderError> {
    if !ctx.bool(PASS_SESSION_VARIABLES)? {
        return Ok(Vec::new());
    }
    // Fixed words, written without quotes like sway's own lines, so the user's line of the same
    // kind reads as the switch.
    let line = |words: &[&'static str]| {
        words
            .iter()
            .chain(crate::apps::autostart::SESSION_VARIABLES.iter())
            .fold(SwayCmd::new("exec"), |command, word| command.keyword(word))
    };
    Ok(vec![
        line(&["systemctl", "--user", "import-environment"]),
        line(&["dbus-update-activation-environment", "--systemd"]),
    ])
}

/// A line of the user's config that imports variables into the user manager does what the
/// switch does.
#[expect(
    clippy::unnecessary_wraps,
    reason = "a ReadFn returns a Result; this line always reads as on"
)]
fn read_passed(_: &[String]) -> Result<Value, ReadError> {
    Ok(Value::Bool(true))
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
/// The section of the portals.
const PORTALS: Msg = msg!("Screen sharing & portals");

#[expect(
    clippy::too_many_lines,
    reason = "one builder per setting, read best as a list"
)]
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
        Setting::list(VARIABLES, Page::Environment, &[])
            .section(msg!("Environment variables"))
            .title(msg!("Environment variables"))
            .subtitle(msg!(
                "Variables for the whole session as NAME=value, in environment.d's syntax: \
                 $NAME and ${NAME:-default} expand, $$ is a $"
            ))
            .check_text(check_item)
            .apply(Apply::Relogin)
            .environment(&[], edited)
            .keywords(&[
                msg!("environment.d"),
                msg!("PATH"),
                msg!("EDITOR"),
                msg!("export"),
                msg!("variable"),
            ])
            .build(),
        Setting::toggle(PORTAL_CONFIG, Page::Environment, false)
            .section(PORTALS)
            .title(msg!("Portal configuration of Sway Things"))
            .subtitle(msg!(
                "~/.config/xdg-desktop-portal/sway-portals.conf: wlr for screen sharing and \
                 screenshots, gtk for the rest; only where you have no configuration there"
            ))
            .apply(Apply::ToolReload)
            .keywords(&[
                msg!("xdg-desktop-portal"),
                msg!("portals.conf"),
                msg!("screen sharing"),
                msg!("file picker"),
            ])
            .build(),
        Setting::toggle(PASS_SESSION_VARIABLES, Page::Environment, false)
            .section(PORTALS)
            .title(msg!("Pass session variables to systemd and D-Bus"))
            .subtitle(msg!(
                "At login, so the portals and the services they start find the display: \
                 WAYLAND_DISPLAY, XDG_CURRENT_DESKTOP and the others"
            ))
            .sway(
                Directive::top("exec").arguments(&["systemctl", "--user", "import-environment"]),
                ConfigFile::Autostart,
                pass_session_variables,
                read_passed,
            )
            .keywords(&[
                msg!("import-environment"),
                msg!("dbus-update-activation-environment"),
                msg!("screen sharing"),
                msg!("portal"),
            ])
            .build(),
        Setting::choice(SCREENCAST_CHOOSER, Page::Environment, "default", CHOOSERS)
            .section(PORTALS)
            .title(msg!("Output chooser"))
            .subtitle(msg!(
                "How an app that shares the screen asks which display: \
                 ~/.config/xdg-desktop-portal-wlr/sway"
            ))
            .apply(Apply::ToolReload)
            .keywords(&[
                msg!("screen sharing"),
                msg!("xdg-desktop-portal-wlr"),
                msg!("chooser"),
                msg!("slurp"),
            ])
            .build(),
        Setting::text(SCREENCAST_OUTPUT, Page::Environment, "")
            .section(PORTALS)
            .title(msg!("Display to share"))
            .subtitle(msg!(
                "Shared without asking, while the chooser is \"Always share one display\""
            ))
            .depends_on(SCREENCAST_CHOOSER, When::OneOf(&["output"]))
            .check_text(check_output)
            .apply(Apply::ToolReload)
            .keywords(&[msg!("screen sharing"), msg!("output_name")])
            .build(),
    ]
}
